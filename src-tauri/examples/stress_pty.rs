//! 100 会话极限压测（审计用，不属于产品路径）。
//!
//! 复刻 `pty_spawn` 的每会话资源形态：openpty + 读端 dup + 写端 dup +
//! 原生终端 + 读线程，然后回答三个问题：
//!   1. 进程 fd 软上限为 256（macOS GUI 启动态）时还能建几个会话；
//!   2. 上限放开后 100 会话能否全部产出、解析、快照且互不串线；
//!   3. 100 会话的 RSS / fd / 线程与快照延迟量级。
//!
//! 用法：`cargo run --release --example stress_pty -- --sessions 100 --lines 2000`
//! 输出一行 JSON 到 stdout（诊断走 stderr）。

use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lazecode_lib::vt::NativeTerminal;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};

struct Session {
    master: Box<dyn MasterPty + Send>,
    term: Arc<Mutex<NativeTerminal>>,
    bytes: Arc<AtomicUsize>,
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
    reader: Option<std::thread::JoinHandle<()>>,
}

fn fd_count() -> usize {
    std::fs::read_dir("/dev/fd").map(|d| d.count()).unwrap_or(0)
}

fn rss_kb() -> u64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .trim()
            .parse()
            .unwrap_or(0),
        Err(_) => 0,
    }
}

fn fd_limit() -> (u64, u64) {
    // 与 lib.rs 的 ensure_fd_limit 同一个系统调用，用于证明继承值与提升结果
    unsafe {
        let mut limit: libc::rlimit = std::mem::zeroed();
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) != 0 {
            return (0, 0);
        }
        (limit.rlim_cur, limit.rlim_max)
    }
}

fn thread_count() -> usize {
    // macOS 没有 /proc：ps -M 每条线程一行（含表头）
    let out = std::process::Command::new("ps")
        .args(["-M", "-p", &std::process::id().to_string()])
        .output();
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .lines()
            .count()
            .saturating_sub(1),
        Err(_) => 0,
    }
}

/// 把 WIRE 全量帧解码回可搜索的文本（审计用，只处理 codepoint 路径）。
///
/// 布局：56B 头 + style_count×12B 样式表 + cols×rows×8B 单元格。
/// 单元格低 21 位是 codepoint；0 视为空格。
fn decode_snapshot_text(frame: &[u8]) -> String {
    const HEADER: usize = 56;
    const STYLE: usize = 12;
    const CELL: usize = 8;
    if frame.len() < HEADER {
        return String::new();
    }
    let u16_at = |at: usize| u16::from_le_bytes([frame[at], frame[at + 1]]) as usize;
    let cols = u16_at(4);
    let rows = u16_at(6);
    let style_count = u16_at(12);
    let cells_at = HEADER + style_count * STYLE;
    if cols == 0 || rows == 0 || frame.len() < cells_at + cols * rows * CELL {
        return String::new();
    }
    let mut text = String::with_capacity(cols * rows + rows);
    for row in 0..rows {
        for col in 0..cols {
            let at = cells_at + (row * cols + col) * CELL;
            let packed = u64::from_le_bytes(frame[at..at + 8].try_into().unwrap());
            let codepoint = (packed & 0x1F_FFFF) as u32;
            if codepoint == 0 {
                text.push(' ');
            } else if let Some(ch) = char::from_u32(codepoint) {
                text.push(ch);
            }
        }
        text.push('\n');
    }
    text
}

fn main() {
    let mut sessions_wanted = 100usize;
    let mut lines_per_session = 2000usize;
    let mut raise_limit = false;
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--sessions" if i + 1 < args.len() => {
                sessions_wanted = args[i + 1].parse().unwrap_or(100);
                i += 2;
            }
            "--lines" if i + 1 < args.len() => {
                lines_per_session = args[i + 1].parse().unwrap_or(2000);
                i += 2;
            }
            // 复现 Finder 启动态（soft=256）时验证 lib.rs 的提限路径
            "--raise-limit" => {
                raise_limit = true;
                i += 1;
            }
            _ => i += 1,
        }
    }
    let limits_before = fd_limit();
    if raise_limit {
        lazecode_lib::ensure_fd_limit();
    }
    let limits_after = fd_limit();

    let mut sessions: Vec<Session> = Vec::new();
    let mut first_error: Option<String> = None;
    let started = Instant::now();

    for n in 0..sessions_wanted {
        let pty_system = native_pty_system();
        let pair = match pty_system.openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        }) {
            Ok(p) => p,
            Err(e) => {
                first_error = Some(format!("openpty #{n}: {e}"));
                break;
            }
        };
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.cwd("/tmp");
        cmd.env("TERM", "xterm-256color");
        cmd.arg("-c");
        cmd.arg(format!(
            "i=0; while [ $i -lt {lines_per_session} ]; do echo \"line $i payload for session {n}\"; i=$((i+1)); done; echo MARK-{n}; sleep 600"
        ));
        let child = match pair.slave.spawn_command(cmd) {
            Ok(c) => Arc::new(Mutex::new(c)),
            Err(e) => {
                first_error = Some(format!("spawn #{n}: {e}"));
                break;
            }
        };
        drop(pair.slave);
        let mut reader = match pair.master.try_clone_reader() {
            Ok(r) => r,
            Err(e) => {
                first_error = Some(format!("clone_reader #{n}: {e}"));
                break;
            }
        };
        let writer = match pair.master.take_writer() {
            Ok(w) => w,
            Err(e) => {
                first_error = Some(format!("take_writer #{n}: {e}"));
                break;
            }
        };
        drop(writer);
        let term = match NativeTerminal::new(120, 40) {
            Ok(t) => Arc::new(Mutex::new(t)),
            Err(e) => {
                first_error = Some(format!("terminal #{n}: {e}"));
                break;
            }
        };
        let bytes = Arc::new(AtomicUsize::new(0));
        let bytes_reader = Arc::clone(&bytes);
        let term_reader = Arc::clone(&term);
        let reader_handle = std::thread::Builder::new()
            .name(format!("stress-reader-{n}"))
            .spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(len) => {
                            if let Ok(mut t) = term_reader.lock() {
                                t.write(&buf[..len]);
                            }
                            bytes_reader.fetch_add(len, Ordering::Relaxed);
                        }
                    }
                }
            })
            .ok();
        sessions.push(Session {
            master: pair.master,
            term,
            bytes,
            child,
            reader: reader_handle,
        });
    }

    // 与生产 PtyManager 相同的全局 scrollback 预算：先应用再灌满历史，
    // 否则 100×20k 行会按每会话 128MB 的无预算口径把 RSS 冲到 12GB。
    let quota = lazecode_lib::pty::scrollback_quota_for(sessions.len());
    let quota_started = Instant::now();
    for s in &sessions {
        if let Ok(mut term) = s.term.lock() {
            let _ = term.set_scrollback_bytes(quota);
        }
    }
    let quota_apply_ms = quota_started.elapsed().as_secs_f64() * 1000.0;

    // 等所有会话把脚本输出消费完（MARK 之后是 sleep，读线程保持存活）
    let expected: usize = lines_per_session * 20; // 宽松下界（实际每行 ~30 字节）
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        let done = sessions
            .iter()
            .filter(|s| s.bytes.load(Ordering::Relaxed) >= expected)
            .count();
        if done == sessions.len() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let produced = sessions
        .iter()
        .filter(|s| s.bytes.load(Ordering::Relaxed) >= expected)
        .count();
    // 让最后的 MARK 行完成解析与滚动入视口（阈值只保证大部分输出已消费）
    std::thread::sleep(Duration::from_millis(500));

    let fd_after_spawn = fd_count();
    let rss_after_spawn = rss_kb();
    let threads_after_spawn = thread_count();

    // 快照延迟：每会话一帧增量 + 一帧全量
    let mut diff_ns = 0u128;
    let mut full_ns = 0u128;
    let mut markers_ok = 0usize;
    let mut cross_talk = 0usize;
    for (n, s) in sessions.iter().enumerate() {
        let mut t = s.term.lock().unwrap();
        let start = Instant::now();
        let diff = t.snapshot_wire_diff(false);
        diff_ns += start.elapsed().as_nanos();
        std::hint::black_box(&diff);
        let start = Instant::now();
        let full = t.snapshot_wire();
        full_ns += start.elapsed().as_nanos();
        let text = decode_snapshot_text(&full);
        let needle = format!("MARK-{n}");
        if text.contains(&needle) {
            markers_ok += 1;
        }
        // 其它会话的 MARK 不应出现在本终端快照里（编号是前缀，需整词匹配）
        for other in 0..sessions.len() {
            if other != n && text.contains(&format!("MARK-{other} ")) {
                cross_talk += 1;
                break;
            }
        }
    }

    let snapshots = sessions.len().max(1);
    let result = serde_json::json!({
        "requested": sessions_wanted,
        "created": sessions.len(),
        "fdLimitBefore": limits_before,
        "fdLimitAfter": limits_after,
        "scrollbackQuotaMb": quota / (1024 * 1024),
        "quotaApplyMs": quota_apply_ms,
        "firstError": first_error,
        "producedAllOutput": produced,
        "markersInOwnSnapshot": markers_ok,
        "crossTalkSnapshots": cross_talk,
        "fdAfterSpawn": fd_after_spawn,
        "rssMbAfterSpawn": rss_after_spawn / 1024,
        "threadsAfterSpawn": threads_after_spawn,
        "avgDiffSnapshotUs": (diff_ns as f64 / snapshots as f64) / 1000.0,
        "avgFullSnapshotUs": (full_ns as f64 / snapshots as f64) / 1000.0,
        "totalSeconds": started.elapsed().as_secs_f64(),
    });
    println!("{result}");

    // 收尸：与 close_session 同序（先关主端，再 kill）
    for s in sessions {
        drop(s.master);
        if let Some(handle) = s.reader {
            drop(handle); // detach
        }
        if let Ok(mut c) = s.child.lock() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}
