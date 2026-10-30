//! 端到端管线基准（引擎 + 线格式），供 `npm run bench:pipeline` 调用。
//!
//! 链路：真实 portable-pty（`/bin/sh -c 'seq …'`）→ 8KB 读块 →
//! `NativeTerminal::write` → `snapshot_wire_diff(false)` → 落盘。
//!
//! 与 `bench_terminal` 的区别：那边用 `engine_bench_stream` 造字节，
//! 只测解析器；这里走真实 PTY 与真实 shell，测的是「PTY 到达 → 解析 →
//! 差分帧编码」这一整条原生路径。仍**不包含** Tauri IPC、React、
//! Canvas 绘制与 UI 启动 —— 那些在 Node 侧无从观测。
//!
//! 产物（Node 侧读取）：
//!   - 帧文件：每帧 `u32 len`（小端）+ 帧字节。首帧是 `snapshot_wire()`
//!     全量帧（VTN1），其余是增量帧（VTN3），与前端首帧重建 → 增量合并
//!     的真实顺序一致。
//!   - 负载文件：PTY 原样读到的字节，供 xterm.js 对照与 RSS 测量。
//!
//! stdout 只输出一行 JSON；诊断走 stderr，避免污染结果。

use std::io::{BufWriter, Read, Write};
use std::time::Instant;

use lazecode_lib::vt::NativeTerminal;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde_json::json;

const COLS: u16 = 120;
const ROWS: u16 = 40;
const CHUNK: usize = 8192;
/// 约 1,500,000 行：数字 + 换行 ≈ 10.9 MB；PTY 的 ONLCR 会把 LF 变 CRLF，
/// 实际负载略大于 12 MB，满足「>10MB 确定性负载」的要求。
const SEQ_LINES: u32 = 1_500_000;

fn mb(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in bytes {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// 最近秩百分位（0.5 / 0.95）。样本为 0 时返回 0。
fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (q * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

fn write_frame(out: &mut BufWriter<std::fs::File>, frame: &[u8]) -> std::io::Result<()> {
    let len = u32::try_from(frame.len()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "单帧超过 4GB，长度前缀放不下",
        )
    })?;
    out.write_all(&len.to_le_bytes())?;
    out.write_all(frame)
}

struct PipelineStats {
    payload: Vec<u8>,
    frames: usize,
    diff_bytes: usize,
    frame_file_bytes: usize,
    full_frame_bytes: usize,
    timings: Vec<f64>,
    read_ms: f64,
    drain_ms: f64,
}

/// 跑一次真实 PTY 管线。`frames_path` / `payload_path` 由调用方给出。
fn run_pipeline(
    frames_path: &std::path::Path,
    payload_path: &std::path::Path,
) -> std::io::Result<PipelineStats> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: ROWS,
            cols: COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| std::io::Error::other(format!("openpty 失败: {e}")))?;

    let mut cmd = CommandBuilder::new("/bin/sh");
    cmd.arg("-c");
    cmd.arg(format!("seq 1 {SEQ_LINES}"));
    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| std::io::Error::other(format!("启动 seq 失败: {e}")))?;
    // 关闭从端句柄，否则 master 读不到 EOF。
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| std::io::Error::other(format!("获取 PTY reader 失败: {e}")))?;

    let frame_file = std::fs::File::create(frames_path)?;
    let mut writer = BufWriter::with_capacity(256 * 1024, frame_file);

    let mut term = NativeTerminal::new(COLS, ROWS)
        .map_err(|e| std::io::Error::other(format!("创建终端失败: {e}")))?;

    // 首帧全量：前端 CellDiffCache 必须先用全量帧建屏，之后才谈增量合并。
    let full = term.snapshot_wire();
    write_frame(&mut writer, &full)?;
    let full_frame_bytes = full.len();

    let mut payload = Vec::with_capacity(CHUNK * 2048);
    let mut buf = vec![0u8; CHUNK];
    // PTY 的 read 常返回远小于 8KB 的碎片；这里攒满 8KB 再喂终端，
    // 保证「每 8KB 一块、每块一帧」的口径可复现，也避免帧文件被放大约 8 倍。
    let mut pending: Vec<u8> = Vec::with_capacity(CHUNK * 2);
    let mut timings = Vec::new();
    let mut frames = 0usize;
    let mut diff_bytes = 0usize;
    let mut read_ms = 0.0f64;
    let drain_start = Instant::now();

    loop {
        let read_start = Instant::now();
        let read = reader.read(&mut buf);
        read_ms += read_start.elapsed().as_secs_f64() * 1000.0;

        let n = match read {
            Ok(0) => break,
            Ok(n) => n,
            // macOS 上从端退出后 master 读会报 EIO，等同 EOF。
            Err(_) => break,
        };

        payload.extend_from_slice(&buf[..n]);
        pending.extend_from_slice(&buf[..n]);

        let mut consumed = 0usize;
        while pending.len() - consumed >= CHUNK {
            let chunk = &pending[consumed..consumed + CHUNK];
            consumed += CHUNK;

            // 计时只覆盖解析 + 差分编码：文件写入是基准产物，不是被测路径。
            let started = Instant::now();
            term.write(chunk);
            let frame = term.snapshot_wire_diff(false);
            timings.push(started.elapsed().as_secs_f64() * 1000.0);

            diff_bytes += frame.len();
            write_frame(&mut writer, &frame)?;
            frames += 1;
        }
        if consumed > 0 {
            pending.drain(..consumed);
        }
    }

    // 尾块不足 8KB 也照常处理，否则最后一段内容不会进帧。
    if !pending.is_empty() {
        let started = Instant::now();
        term.write(&pending);
        let frame = term.snapshot_wire_diff(false);
        timings.push(started.elapsed().as_secs_f64() * 1000.0);
        diff_bytes += frame.len();
        write_frame(&mut writer, &frame)?;
        frames += 1;
    }

    let drain_ms = drain_start.elapsed().as_secs_f64() * 1000.0;
    let _ = child.wait();
    writer.flush()?;

    std::fs::write(payload_path, &payload)?;

    Ok(PipelineStats {
        frame_file_bytes: std::fs::metadata(frames_path)?.len() as usize,
        full_frame_bytes,
        payload,
        frames,
        diff_bytes,
        timings,
        read_ms,
        drain_ms,
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let frames_path = args
        .next()
        .unwrap_or_else(|| "target/bench/pipeline-frames.bin".to_string());
    let payload_path = args
        .next()
        .unwrap_or_else(|| "target/bench/pipeline-payload.bin".to_string());

    let frames_path = std::path::PathBuf::from(frames_path);
    let payload_path = std::path::PathBuf::from(payload_path);
    if let Some(parent) = frames_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(parent) = payload_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let stats = run_pipeline(&frames_path, &payload_path)?;

    let mut sorted = stats.timings.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let total_ms: f64 = stats.timings.iter().sum();

    let result = json!({
        "payload": {
            "bytes": stats.payload.len(),
            "fnv1a32": format!("{:08x}", fnv1a32(&stats.payload)),
            "command": format!("/bin/sh -c 'seq 1 {SEQ_LINES}'"),
        },
        "config": {
            "cols": COLS,
            "rows": ROWS,
            "chunk_bytes": CHUNK,
        },
        "frames": {
            "count": stats.frames,
            "full_frames": 1,
            "diff_frames": stats.frames,
            "diff_bytes": stats.diff_bytes,
            "full_frame_bytes": stats.full_frame_bytes,
            "file_bytes": stats.frame_file_bytes,
        },
        "parse_diff": {
            "total_ms": total_ms,
            "p50_ms": percentile(&sorted, 0.50),
            "p95_ms": percentile(&sorted, 0.95),
            "max_ms": sorted.last().copied().unwrap_or(0.0),
            "mb_per_s": mb(stats.payload.len()) / (total_ms / 1000.0).max(f64::EPSILON),
            "samples": stats.timings.len(),
        },
        "pty": {
            // 累计阻塞在 reader.read 上的时间；处理块时 PTY 缓冲继续灌入，
            // 因此它小于 drain_ms 是正常的。
            "read_ms": stats.read_ms,
            "drain_ms": stats.drain_ms,
        },
    });

    println!("{result}");
    Ok(())
}
