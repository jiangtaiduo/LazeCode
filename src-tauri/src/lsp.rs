//! 语言服务器进程注册表（Zed `LspStore` 的进程管理一半）。
//!
//! Zed 把 LSP 拆成薄传输层（`lsp/`）+ 按 worktree/语言复用的 `LspStore`。
//! 进程按「窗口 + 语言」登记，start/stop/status，退出码可查；
//! Monaco 的 MonacoLspClient 通过本模块的 send/read 传输获得补全、定义、
//! 引用、重命名、诊断与代码操作。
//!
//! 信任说明：server 二进制由用户配置（与终端 shell 白名单同类信任，
//! 见 `resolve_shell` 的注释）。命令只做非空 + NUL 校验；
//! 已知 server 白名单是后续项（配好第一批内置 server 时再收紧）。
//!
//! 分层（与 commands.rs 的 `*_impl` 同模式）：命令本体只是 State 包装，
//! `*_impl` 拿 `&LspRegistry`，测试直调 impl —— 不复制逻辑进测试。
//!
//! 传输：stdout 的 LSP 帧由读线程解析成 JSON 字符串，经 `lsp://message`
//! 事件推给前端；`lsp_server_send` 负责把前端 JSON 加上 Content-Length
//! 头写进 stdin。Monaco 的 `MonacoLspClient` 只需实现一个 IMessageTransport
//! 适配这层，就能得到补全/定义/引用/重命名/诊断/代码操作等能力。

use serde::Serialize;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, State};

/// 语言服务器输出帧 → 前端的回调：参数是 language 与原始 JSON 文本。
pub type LspEmitter = Arc<dyn Fn(&str, String) + Send + Sync>;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LspWireMessage {
    language: String,
    message: String,
}

struct LspServer {
    command: String,
    child: Child,
    writer: Arc<Mutex<ChildStdin>>,
    started_ms: u64,
}

#[derive(Default)]
pub struct LspRegistry {
    servers: Mutex<HashMap<String, LspServer>>,
}

impl LspRegistry {
    /// 全部 server 下线（应用退出兜底，与 PtyManager::close_all 同模式）：
    /// 子进程不会随父进程自动消失，不杀就成孤儿。
    pub fn shutdown_all(&self) {
        let servers = match self.servers.lock() {
            Ok(mut guard) => std::mem::take(&mut *guard),
            Err(_) => return,
        };
        for (_, server) in servers {
            kill_and_reap(server.child);
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LspServerDto {
    pub language: String,
    pub command: String,
    pub pid: u32,
    pub running: bool,
    pub started_ms: u64,
    /// 已退出时的退出码（对应"server 崩了"的可见性，见 status 的注释）
    pub exit_code: Option<i32>,
    /// 常驻内存（KB）。Zed 的内存投诉几乎全是 rust-analyzer 这类 server 吃掉的，
    /// 而编辑器侧看不见是谁吃的——这个字段就是 attribution（归因）：
    /// 哪个 server 胖一目了然，而不是对着进程管理器猜。
    /// None = 已退出 / 非 Unix / 采样失败（采样是 best-effort，不影响 status 主体）。
    pub rss_kb: Option<u64>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn check_no_nul(label: &str, value: &str) -> Result<(), String> {
    if value.contains('\0') {
        return Err(format!("{label} 含 NUL 字节"));
    }
    Ok(())
}

/// 起一个 server 进程并接好三条管道。
///
/// 无 `.output()`/`.wait()`：调用线程只 spawn 就返回（R2 无界阻塞零容忍）。
/// 失败（命令不存在/无权执行）直接透 spawn 错误，调用方看得见。
/// R6 审批位（见 scripts/verify-rust-discipline.mjs APPROVED_SPAWNERS）。
fn spawn_lsp_server(
    command: &str,
    args: &[String],
    root: &str,
) -> Result<(Child, ChildStdin, ChildStdout, std::process::ChildStderr), String> {
    let mut cmd = std::process::Command::new(command);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if !root.trim().is_empty() {
        cmd.current_dir(root);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动语言服务器失败 ({command}): {e}"))?;
    let stdin = child.stdin.take().ok_or("无法获取语言服务器 stdin")?;
    let stdout = child.stdout.take().ok_or("无法获取语言服务器 stdout")?;
    let stderr = child.stderr.take().ok_or("无法获取语言服务器 stderr")?;
    Ok((child, stdin, stdout, stderr))
}

/// 读 stdout 上的 LSP 帧（Content-Length 头 + JSON body），逐帧推给前端。
fn pump_lsp_stdout(language: String, stdout: ChildStdout, emitter: Option<LspEmitter>) {
    let mut reader = BufReader::new(stdout);
    loop {
        let mut content_length: Option<usize> = None;
        let mut line = String::new();
        // 头部：空行结束；允许 CRLF/LF。
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => return,
                Ok(_) => {
                    let trimmed = line.trim_end_matches(['\r', '\n']);
                    if trimmed.is_empty() {
                        break;
                    }
                    if let Some(v) = trimmed.strip_prefix("Content-Length:") {
                        content_length = v.trim().parse::<usize>().ok();
                    }
                }
                Err(e) => {
                    log::warn!("LSP[{language}] stdout 读取失败: {e}");
                    return;
                }
            }
        }
        let Some(len) = content_length else {
            log::warn!("LSP[{language}] 收到缺少 Content-Length 的帧");
            continue;
        };
        if len == 0 || len > 64 * 1024 * 1024 {
            log::warn!("LSP[{language}] 非法 Content-Length: {len}");
            return;
        }
        let mut body = vec![0u8; len];
        if let Err(e) = reader.read_exact(&mut body) {
            log::warn!("LSP[{language}] 读取 body 失败: {e}");
            return;
        }
        let text = String::from_utf8_lossy(&body).into_owned();
        if let Some(cb) = &emitter {
            cb(&language, text);
        }
    }
}

fn pump_lsp_stderr(language: String, stderr: std::process::ChildStderr) {
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            log::debug!("LSP[{language}] {line}");
        }
    });
}

fn dto_of(language: &str, server: &mut LspServer) -> LspServerDto {
    // try_wait 非阻塞：只刷新状态。退出状态会被 Child 缓存，再次调用返回同一值。
    // Err（极少，系统调用失败）按"仍在运行"处理 —— 误报存活比误报死亡安全
    //（误报死亡会让调用方放弃一个活着的 server）。
    let (running, exit_code) = match server.child.try_wait() {
        Ok(Some(status)) => (false, status.code()),
        _ => (true, None),
    };
    // RSS 只对存活进程采样：已退出的 pid 可能被系统重用，此时 ps 读到的是
    // 别人的内存，归因错误比没有归因更坏。
    let rss_kb = if running {
        process_rss_kb(server.child.id())
    } else {
        None
    };
    LspServerDto {
        language: language.to_string(),
        command: server.command.clone(),
        pid: server.child.id(),
        running,
        started_ms: server.started_ms,
        exit_code,
        rss_kb,
    }
}

/// 采样某 pid 的常驻内存（KB），失败给 None（best-effort，不污染 status）。
///
/// 实现选 `ps -o rss=` 而不是读 `/proc`：后者要自己乘页大小（Linux 页
/// 4K/64K 按机器而异，算错就静默差一个数量级），`ps` 直接给 KB，macOS/Linux
/// 通用。status 是低频手动/定时动作，3s 有界 spawn 的成本可忽略。
/// R6 审批位（见 scripts/verify-rust-discipline.mjs APPROVED_SPAWNERS）。
#[cfg(unix)]
fn process_rss_kb(pid: u32) -> Option<u64> {
    use std::process::Command;
    use std::time::Duration;
    let mut cmd = Command::new("ps");
    cmd.args(["-o", "rss=", "-p", &pid.to_string()]);
    let out = crate::commands::run_with_timeout_spawn(&mut cmd, Duration::from_secs(3)).ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

#[cfg(not(unix))]
fn process_rss_kb(_pid: u32) -> Option<u64> {
    None
}

/// 杀掉一个已移出注册表的 server：kill 发出去就走，收尸丢后台线程。
/// kill 失败（已退出）也继续 —— 目标状态是"旧进程不在"，它已经不在了。
fn kill_and_reap(mut child: Child) {
    let _ = child.kill();
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

/// 启动（或重启）某语言的 server：已存在先杀旧进程再起新进程。
///
/// 重启语义（对齐 Zed 的 debounced restarts）：配置变更/崩溃后调同一个命令
/// 即可恢复，不需要先 stop。
#[tauri::command]
pub fn lsp_server_start(
    window: tauri::WebviewWindow,
    language: String,
    command: String,
    args: Vec<String>,
    root: String,
    registry: State<'_, LspRegistry>,
) -> Result<LspServerDto, String> {
    let emit_window = window.clone();
    let emitter: LspEmitter = Arc::new(move |lang, message| {
        let _ = emit_window.emit(
            "lsp://message",
            LspWireMessage {
                language: lang.to_string(),
                message,
            },
        );
    });
    // 每个窗口独立 server：两个窗口同时开 Go/Python 时，后启动的不能杀掉
    // 先启动的（旧实现按 language 全局键，B 窗口会顶掉 A 的进程）。
    let key = lsp_key(&window, &language);
    lsp_start_impl(
        &registry,
        &key,
        &language,
        &command,
        &args,
        &root,
        Some(emitter),
    )
}

fn lsp_key(window: &tauri::WebviewWindow, language: &str) -> String {
    format!("{}::{language}", window.label())
}

/// 把前端 JSON-RPC 文本写入语言服务器 stdin（自动加 LSP Content-Length 头）。
#[tauri::command]
pub async fn lsp_server_send(
    window: tauri::WebviewWindow,
    language: String,
    message: String,
    registry: State<'_, LspRegistry>,
) -> Result<(), String> {
    let key = lsp_key(&window, &language);
    let writer = {
        let servers = registry
            .servers
            .lock()
            .map_err(|_| "lsp 注册表锁失效".to_string())?;
        servers
            .get(&key)
            .map(|s| Arc::clone(&s.writer))
            .ok_or_else(|| format!("没有运行中的语言服务器: {language}"))?
    };
    // stdin 管道满时 write_all 会阻塞；不能占住 ipc worker（整个 IDE 排队）。
    tauri::async_runtime::spawn_blocking(move || write_lsp_message(&writer, &message))
        .await
        .map_err(|e| format!("lsp_server_send 任务失败: {e}"))?
}

#[cfg_attr(not(test), allow(dead_code))] // 生产走 async 写入；测试直接调同步实现
fn lsp_send_impl(registry: &LspRegistry, key: &str, message: &str) -> Result<(), String> {
    let writer = {
        let servers = registry
            .servers
            .lock()
            .map_err(|_| "lsp 注册表锁失效".to_string())?;
        servers
            .get(key)
            .map(|s| Arc::clone(&s.writer))
            .ok_or_else(|| format!("没有运行中的语言服务器: {key}"))?
    };
    write_lsp_message(&writer, message)
}

fn write_lsp_message(writer: &Arc<Mutex<ChildStdin>>, message: &str) -> Result<(), String> {
    let mut w = writer
        .lock()
        .map_err(|_| "语言服务器 stdin 锁失效".to_string())?;
    let header = format!("Content-Length: {}\r\n\r\n", message.len());
    w.write_all(header.as_bytes())
        .and_then(|_| w.write_all(message.as_bytes()))
        .and_then(|_| w.flush())
        .map_err(|e| format!("写入语言服务器失败: {e}"))
}

fn lsp_start_impl(
    registry: &LspRegistry,
    key: &str,
    language: &str,
    command: &str,
    args: &[String],
    root: &str,
    emitter: Option<LspEmitter>,
) -> Result<LspServerDto, String> {
    if language.trim().is_empty() {
        return Err("language 为空".to_string());
    }
    if command.trim().is_empty() {
        return Err("server 命令为空".to_string());
    }
    check_no_nul("server 命令", command)?;
    for a in args {
        check_no_nul("server 参数", a)?;
    }
    let mut servers = registry
        .servers
        .lock()
        .map_err(|_| "lsp 注册表锁失效".to_string())?;
    if let Some(old) = servers.remove(key) {
        kill_and_reap(old.child);
    }
    let (child, stdin, stdout, stderr) = spawn_lsp_server(command, args, root)?;
    // stdout 读线程只做解析与 emit，不碰注册表锁。
    let lang = language.to_string();
    std::thread::spawn(move || pump_lsp_stdout(lang, stdout, emitter));
    pump_lsp_stderr(language.to_string(), stderr);
    let mut server = LspServer {
        command: command.to_string(),
        child,
        writer: Arc::new(Mutex::new(stdin)),
        started_ms: now_ms(),
    };
    let dto = dto_of(language, &mut server);
    servers.insert(key.to_string(), server);
    Ok(dto)
}

/// 停止某语言的 server：不存在即报错（调用方多半是过期 UI，不是幂等场景）。
#[tauri::command]
pub fn lsp_server_stop(
    window: tauri::WebviewWindow,
    language: String,
    registry: State<'_, LspRegistry>,
) -> Result<String, String> {
    lsp_stop_impl(&registry, &lsp_key(&window, &language), &language)
}

fn lsp_stop_impl(registry: &LspRegistry, key: &str, language: &str) -> Result<String, String> {
    let old = registry
        .servers
        .lock()
        .map_err(|_| "lsp 注册表锁失效".to_string())?
        .remove(key)
        .ok_or_else(|| format!("没有运行中的语言服务器: {language}"))?;
    kill_and_reap(old.child);
    Ok(format!("已停止语言服务器: {language}"))
}

/// 全部 server 状态：已退出的保留条目 + 退出码（server 崩了要看得见，
/// 而不是静默消失；清理用 stop）。
#[tauri::command]
pub fn lsp_server_status(registry: State<'_, LspRegistry>) -> Result<Vec<LspServerDto>, String> {
    lsp_status_impl(&registry)
}

fn lsp_status_impl(registry: &LspRegistry) -> Result<Vec<LspServerDto>, String> {
    let mut servers = registry
        .servers
        .lock()
        .map_err(|_| "lsp 注册表锁失效".to_string())?;
    let mut out: Vec<LspServerDto> = servers
        .iter_mut()
        .map(|(key, server)| dto_of(key.rsplit("::").next().unwrap_or(key), server))
        .collect();
    out.sort_by(|a, b| a.language.cmp(&b.language));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `/bin/cat` 当假 server：无输入即阻塞读，不退出（stdin 管道开着），
    /// 正好锁定"启动 → 存活 → 停止 → 消失"的生命周期。
    #[cfg(unix)]
    #[test]
    fn lifecycle_start_status_stop() {
        let reg = LspRegistry::default();
        let dto =
            lsp_start_impl(&reg, "test", "test", "/bin/cat", &[], "", None).expect("cat 应能启动");
        assert!(dto.running, "刚启动应为 running");
        assert!(dto.pid > 0, "应有 pid");

        // 同语言再起 = 重启：旧进程被换掉
        let dto2 =
            lsp_start_impl(&reg, "test", "test", "/bin/cat", &[], "", None).expect("重启应成功");
        assert!(dto2.running);
        assert_ne!(dto.pid, dto2.pid, "重启应是新进程");

        let all = lsp_status_impl(&reg).expect("status");
        assert_eq!(all.len(), 1);
        assert!(all[0].running);

        lsp_stop_impl(&reg, "test", "test").expect("stop");
        let all = lsp_status_impl(&reg).expect("status");
        assert!(all.is_empty(), "stop 后注册表应空");
    }

    /// `/usr/bin/false` 秒退（macOS 上 false 不在 /bin，/usr/bin 双平台都有）：
    /// status 必须能看到 running=false + 退出码，而不是静默消失（server 崩溃可见性）。
    #[cfg(unix)]
    #[test]
    fn exited_server_reports_exit_code() {
        let reg = LspRegistry::default();
        lsp_start_impl(&reg, "doomed", "doomed", "/usr/bin/false", &[], "", None)
            .expect("false 应能 spawn");
        std::thread::sleep(std::time::Duration::from_millis(300));
        let all = lsp_status_impl(&reg).expect("status");
        assert_eq!(all.len(), 1, "退出的 server 条目应保留");
        assert!(!all[0].running, "应标记为未运行");
        assert_eq!(all[0].exit_code, Some(1), "/usr/bin/false 退出码为 1");
        assert_eq!(
            all[0].rss_kb, None,
            "已退出进程不采样 RSS（防 pid 重用归因错误）"
        );
    }

    /// 存活 server 的 RSS 可采样（>0）：内存归因字段不断言具体数值
    /// （机器而异），只断言"能采到"，采样失败则 status 主体不受影响。
    #[cfg(unix)]
    #[test]
    fn running_server_reports_rss() {
        let reg = LspRegistry::default();
        let dto =
            lsp_start_impl(&reg, "rss", "rss", "/bin/cat", &[], "", None).expect("cat 应能启动");
        assert!(dto.running);
        match dto.rss_kb {
            Some(kb) => assert!(kb > 0, "cat 的 RSS 应大于 0，实际 {kb}"),
            None => panic!("存活进程应能采到 RSS（ps 不可用时才允许 None，本机 ps 必然存在）"),
        }
        lsp_stop_impl(&reg, "rss", "rss").expect("stop");
    }

    #[test]
    fn start_validation_and_stop_missing() {
        let reg = LspRegistry::default();
        assert!(
            lsp_start_impl(&reg, "", "", "/bin/cat", &[], "", None).is_err(),
            "空 language 应拒"
        );
        assert!(
            lsp_start_impl(&reg, "x", "x", "", &[], "", None).is_err(),
            "空命令应拒"
        );
        assert!(
            lsp_start_impl(&reg, "x", "x", "/bin/ca\0t", &[], "", None).is_err(),
            "NUL 应拒"
        );
        assert!(
            lsp_start_impl(
                &reg,
                "nope",
                "nope",
                "/nonexistent-binary-xyz",
                &[],
                "",
                None
            )
            .is_err(),
            "不存在的二进制应报错"
        );
        assert!(
            lsp_stop_impl(&reg, "ghost", "ghost").is_err(),
            "停不存在的应报错"
        );
    }

    /// 两个窗口的同语言 server 必须并存：旧实现按 language 全局键，B 窗口
    /// 启动会杀掉 A 的进程，并把 A 的文档写进 B 的 server。
    #[cfg(unix)]
    #[test]
    fn same_language_servers_are_isolated_per_window() {
        let reg = LspRegistry::default();
        let a = lsp_start_impl(&reg, "win-a::py", "py", "/bin/cat", &[], "", None).unwrap();
        let b = lsp_start_impl(&reg, "win-b::py", "py", "/bin/cat", &[], "", None).unwrap();
        assert_ne!(a.pid, b.pid, "两个窗口应各自持有进程");
        let all = lsp_status_impl(&reg).unwrap();
        assert_eq!(all.len(), 2, "同语言不同窗口不能互相顶掉: {all:?}");
        lsp_send_impl(&reg, "win-a::py", "{}").unwrap();
        lsp_stop_impl(&reg, "win-a::py", "py").unwrap();
        let all = lsp_status_impl(&reg).unwrap();
        assert_eq!(all.len(), 1, "停 A 不能影响 B");
        assert!(lsp_send_impl(&reg, "win-b::py", "{}").is_ok(), "B 仍可写");
        lsp_stop_impl(&reg, "win-b::py", "py").unwrap();
    }

    /// send 必须写 Content-Length 帧到 stdin；cat 不回应也不应报错。
    #[cfg(unix)]
    #[test]
    fn send_writes_framed_message() {
        let reg = LspRegistry::default();
        lsp_start_impl(&reg, "cat-send", "cat-send", "/bin/cat", &[], "", None)
            .expect("cat 应能启动");
        lsp_send_impl(
            &reg,
            "cat-send",
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        )
        .expect("写入 stdin");
        assert!(
            lsp_send_impl(&reg, "missing", "{}").is_err(),
            "不存在的 server 应报错"
        );
        lsp_stop_impl(&reg, "cat-send", "cat-send").expect("stop");
    }
}
