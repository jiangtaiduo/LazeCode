//! 原生终端解析基准（供 `npm run bench:terminal` 调用）。
//!
//! 只测解析器本身，不含渲染与 IPC。与 xterm.js 的对照必须使用同一份
//! 字节流：本程序把 `engine_bench_stream` 的产物写到磁盘，Node 侧读取
//! 同一文件，并通过 FNV-1a 校验两边确实一致。
//!
//! 输出一行 JSON 到 stdout；诊断信息一律走 stderr，避免污染结果。
//!
//! 内存不在本基准里比较：Ghostty 引擎使用 Zig allocator，Rust 的
//! global allocator 观测不到其保留量；要测内存必须用进程 RSS 工具，
//! 而那会把 Rust/Node 运行时也算进去，不能当作解析器内存。

use std::hint::black_box;
use std::time::Instant;

use lazecode_lib::vt::{engine_bench_stream, NativeTerminal};
use serde_json::json;

const COLS: u16 = 120;
const ROWS: u16 = 40;
const CHUNK: usize = 8192;
const WARMUP_CHUNKS: usize = 64;
const SNAPSHOT_ITERS: usize = 2000;

fn mb(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// 取中位数，降低单次调度的抖动；按吞吐排序后选中间那次。
fn median_by<T: Clone, F: Fn(&T) -> f64>(runs: &[T], key: F) -> T {
    let mut sorted = runs.to_vec();
    sorted.sort_by(|a, b| {
        key(a)
            .partial_cmp(&key(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    sorted[sorted.len() / 2].clone()
}

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in bytes {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

#[derive(Clone)]
struct ParseResult {
    elapsed_ms: f64,
    mb_per_s: f64,
}

fn measure_parse(payload: &[u8], chunk_size: usize) -> ParseResult {
    let mut term = NativeTerminal::new(COLS, ROWS).expect("创建终端失败");

    let warmup = WARMUP_CHUNKS.min(payload.len() / chunk_size.max(1));
    for chunk in payload.chunks(chunk_size).take(warmup) {
        term.parse_only(chunk);
    }

    let start = Instant::now();
    for chunk in payload.chunks(chunk_size) {
        term.parse_only(chunk);
    }
    let elapsed = start.elapsed().as_secs_f64();

    ParseResult {
        elapsed_ms: elapsed * 1000.0,
        mb_per_s: mb(payload.len()) / elapsed,
    }
}

#[derive(Clone)]
struct PipelineResult {
    elapsed_ms: f64,
    mb_per_s: f64,
    frames: usize,
    diff_bytes: usize,
}

/// 应用真实路径的保守上界：每读一块 PTY 就取一次增量帧。
/// 生产里帧率由 rAF 决定（通常 60fps），这里按 8KB 一块计算，
/// 相当于把帧率放到最大，是比生产更重的负载。
fn measure_pipeline(payload: &[u8]) -> PipelineResult {
    let mut term = NativeTerminal::new(COLS, ROWS).expect("创建终端失败");

    for chunk in payload.chunks(CHUNK).take(WARMUP_CHUNKS) {
        term.write(chunk);
        black_box(term.snapshot_wire_diff(false));
    }

    let mut diff_bytes = 0usize;
    let mut frames = 0usize;
    let start = Instant::now();
    for chunk in payload.chunks(CHUNK) {
        term.write(chunk);
        diff_bytes += term.snapshot_wire_diff(false).len();
        frames += 1;
    }
    let elapsed = start.elapsed().as_secs_f64();

    PipelineResult {
        elapsed_ms: elapsed * 1000.0,
        mb_per_s: mb(payload.len()) / elapsed,
        frames,
        diff_bytes,
    }
}

fn measure_snapshot() -> f64 {
    let payload = engine_bench_stream(8_000);
    let mut term = NativeTerminal::new(COLS, ROWS).expect("创建终端失败");
    for chunk in payload.chunks(CHUNK) {
        term.parse_only(chunk);
    }

    for _ in 0..100 {
        black_box(term.snapshot());
    }

    let start = Instant::now();
    for _ in 0..SNAPSHOT_ITERS {
        black_box(term.snapshot());
    }
    start.elapsed().as_secs_f64() / SNAPSHOT_ITERS as f64 * 1000.0
}

fn main() {
    let stream_path = std::env::args().nth(1);
    let lines: usize = std::env::var("BENCH_LINES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(120_000);

    let payload = engine_bench_stream(lines);
    if let Some(path) = &stream_path {
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent).expect("创建基准目录失败");
        }
        std::fs::write(path, &payload).expect("写入基准流失败");
    }

    let parse_runs: Vec<ParseResult> = (0..3).map(|_| measure_parse(&payload, CHUNK)).collect();
    let bulk_runs: Vec<ParseResult> = (0..3)
        .map(|_| measure_parse(&payload, payload.len().max(1)))
        .collect();
    let pipeline_runs: Vec<PipelineResult> = (0..3).map(|_| measure_pipeline(&payload)).collect();
    let parse = median_by(&parse_runs, |run| run.mb_per_s);
    let bulk = median_by(&bulk_runs, |run| run.mb_per_s);
    let pipeline = median_by(&pipeline_runs, |run| run.mb_per_s);
    let snapshot_ms = measure_snapshot();

    let result = json!({
        "payload": {
            "bytes": payload.len(),
            "lines": lines,
            "fnv1a32": format!("{:08x}", fnv1a32(&payload)),
        },
        "parse": {
            "chunk_bytes": CHUNK,
            "elapsed_ms": parse.elapsed_ms,
            "mb_per_s": parse.mb_per_s,
            "runs_mb_per_s": parse_runs.iter().map(|run| run.mb_per_s).collect::<Vec<_>>(),
        },
        "parseBulk": {
            "elapsed_ms": bulk.elapsed_ms,
            "mb_per_s": bulk.mb_per_s,
            "runs_mb_per_s": bulk_runs.iter().map(|run| run.mb_per_s).collect::<Vec<_>>(),
        },
        "pipeline": {
            "chunk_bytes": CHUNK,
            "elapsed_ms": pipeline.elapsed_ms,
            "mb_per_s": pipeline.mb_per_s,
            "frames": pipeline.frames,
            "diff_bytes": pipeline.diff_bytes,
            "runs_mb_per_s": pipeline_runs.iter().map(|run| run.mb_per_s).collect::<Vec<_>>(),
        },
        "snapshot": {
            "cols": COLS,
            "rows": ROWS,
            "ms_per_frame": snapshot_ms,
            "fps": 1000.0 / snapshot_ms,
        },
    });

    println!("{result}");
}
