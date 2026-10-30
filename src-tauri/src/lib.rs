#[cfg(test)]
mod diff_stream_test;
#[cfg(test)]
mod git_menu_test;

pub mod clipboard;
pub mod commands;
pub mod error_proto;
/// 扩展清单与能力门（执行器之前先有门，见模块头）
pub mod extensions;
/// 文件内容读缓存 + 轻量探针（变更检测快路径，见模块头）
pub mod file_cache;
/// 文本编码检测/回写 + 原子保存（非 UTF-8 项目文件）
pub mod file_encoding;
/// 文件分块读（大文件分页预览的后端一半，见模块头）
pub mod file_range;
pub mod file_watch;
pub mod ghostty;
pub mod language;
/// 语言服务器进程注册表（Zed LspStore 的进程管理一半，见模块头）
pub mod lsp;
pub mod menu;
pub mod pty;
pub mod vt;
pub mod window_state;

use commands::*;
use file_watch::*;
use ghostty::*;
use pty::*;
// `build_menu` 保留给外部（菜单只按固定英文构造的场景）与测试用，
// 启动链路一律走 build_menu_with 以带上语言。
use menu::{build_menu_with, emit_menu_event, labels_for};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::menu::MenuEvent;
use tauri::Manager;
use window_state::{load_state, save_state, to_logical, WindowState};

/// 当前聚焦的窗口 label（`""` = 没有窗口聚焦，例如整个应用被 Cmd+Tab 切走）。
///
/// 存在的理由：`on_menu_event` 是 app 级回调，`MenuEvent` 只有 id、不带窗口
/// label。菜单属于**活动窗口**，事件若不按聚焦窗口定向、而是广播给全部窗口，
/// 多窗口下点一次 `File > New Window` 会开出两个窗口、点 `Close Project` 会把
/// 未聚焦窗口正在用的项目一起关掉（Session 数据丢失）。
///
/// 由 `WindowEvent::Focused(bool)` 维护：失焦时置空而不是保留旧值 —— 保留旧值
/// 会让「应用整体失焦期间点击的菜单」仍发给那个窗口，而它已经不可见。
static FOCUSED_WINDOW: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// 退出前写盘协议：ExitRequested 先 prevent_exit，广播 `app://before-exit`；
/// 前端把当前项目与所有快照的脏缓冲写完后调用 `finish_app_exit`，失败则
/// 调用 `cancel_app_exit` 保持应用打开。超时兜底避免事件丢失时无法退出。
pub(crate) static EXIT_FLUSH_PENDING: AtomicBool = AtomicBool::new(false);
pub(crate) static EXIT_FLUSH_DONE: AtomicBool = AtomicBool::new(false);

/// panic 日志目录。hook 在 `run()` 开头就注册，那时还没有 AppHandle 可取
/// `app_log_dir()`，故在 setup 里补填；这之前的崩溃只落到 stderr。
static PANIC_LOG_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// 崩溃现场日志：release 是 `panic = "abort"` + `strip = true`，panic 直接终止
/// 整个 IDE（未保存内容随之丢失）且没有任何可查记录。hook 把消息与位置追加写进
/// `app_log_dir()/panic.log`，事后至少能定位到文件与行号。
///
/// hook 内一律不能 panic：日志写失败就丢弃（hook 自己再 panic 会二次 abort，
/// 比少一条日志糟得多），stderr 也用 `writeln!` 而非 `eprintln!`（后者写失败会 panic）。
fn install_panic_hook() {
    use std::io::Write;

    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<未知位置>".to_string());
        // payload 通常是 panic! 的格式化结果（&str 或 String）；其它类型给占位，
        // 不为取消息再冒一次 panic 的风险。
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<非字符串 panic 负载>".to_string());
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let mut record = format!(
            "[{secs}] thread '{}' panicked at {location}: {message}",
            std::thread::current().name().unwrap_or("<unnamed>")
        );
        // 默认 hook 已被替换，RUST_BACKTRACE 的回溯能力得自己补回来
        // （capture() 自带环境变量门控，未开启时零开销地返回未捕获状态）。
        let backtrace = std::backtrace::Backtrace::capture();
        if matches!(
            backtrace.status(),
            std::backtrace::BacktraceStatus::Captured
        ) {
            record.push_str(&format!("\n{backtrace}"));
        }

        let _ = writeln!(std::io::stderr(), "{record}");
        if let Some(dir) = PANIC_LOG_DIR.get() {
            // 追加而非覆盖：每次崩溃的现场都要留着。
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("panic.log"))
            {
                let _ = writeln!(file, "{record}");
            }
        }
    }));
}

/// 应用配置目录（`{config_dir}/io.github.jiangtaiduo.lazecode`）。
///
/// 与 tauri 的 `app_config_dir()` 同一路径：那里实现是
/// `dirs::config_dir().join(config.identifier)`，identifier 见
/// tauri.conf.json。启动链路拿不到 AppHandle，因此这里自己拼一次 ——
/// 两处必须与 identifier 保持一致。
pub fn app_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("io.github.jiangtaiduo.lazecode"))
}

/// 同 `app_config_dir`，但拿不到时返回临时目录。
///
/// 只用于「读一次偏好、读不到就用默认值」的启动链路：那里失败没有合理的
/// 处理方式（不能在 setup 里 return Err 让应用起不来），而落到 temp_dir 后
/// 文件必然不存在，等价于「用默认语言」。调用方必须是这种容错场景。
fn app_config_dir_or_default() -> PathBuf {
    app_config_dir().unwrap_or_else(std::env::temp_dir)
}

/// 启动时读取上次的窗口几何（启动链路里独立函数，便于测试与失败隔离）。
///
/// 放在 `run()` 开头、Builder 之前拿 —— 它不依赖 AppHandle，只需要
/// `app_config_dir`。拿不到时返回 `None`（首次启动 / 目录不可用），
/// 窗口按 tauri.conf.json 的默认值创建。
///
/// 注意与 tauri.conf.json 的关系：配置里的 `width/height` 仍是「首次启动
/// 的默认值」，恢复时在这里覆盖它。两者是同一套语义的两个阶段。
///
/// 只恢复 `main`：`win_*` 是用户主动「New Window」开出来的，tauri 没有
/// 「恢复上次全部窗口」的入口，硬起一堆无人使用的窗口反而打扰。
/// 窗口尺寸下限，与 `tauri.conf.json` 的 minWidth/minHeight 一致。
const MIN_WINDOW_W: f64 = 1024.0;
const MIN_WINDOW_H: f64 = 700.0;

fn restore_window_state() -> Option<WindowState> {
    let dir = app_config_dir()?;
    load_state(&dir, "main")
}

/// 把窗口几何写盘（窗口事件路径调用）。
///
/// 每个窗口各写一份（label 来自 `window.label()`）：共用单文件会让多窗口
/// 互相覆盖，见 `state_file_path` 的说明。
///
/// 写盘失败只记日志：持久化是增强，不能让它中断窗口的正常移动/缩放/关闭。
fn persist_window_state_for(label: &str, state: &WindowState) {
    let Some(dir) = app_config_dir() else {
        return;
    };
    if let Err(e) = save_state(&dir, label, state) {
        log::warn!("保存窗口 {label} 状态失败: {e}");
    }
}

/// 应用上次保存的窗口几何。
///
/// 顺序很重要：先恢复尺寸再恢复位置、最后恢复最大化。反过来会在
/// 多显示器环境下把窗口放到错误屏幕上（先定位到旧坐标、再因尺寸变化
/// 被系统挪位）。
///
/// 每步都独立容错：任一步失败只放弃该字段，其余的仍然生效。
///
/// 参数用 `WebviewWindow`：`get_webview_window("main")` 返回的就是它，
/// 而窗口几何的 getter/setter 在这两个类型上都有等价 API（内部同一份
/// 原生窗口）。
fn apply_window_state<R: tauri::Runtime>(app: &tauri::AppHandle<R>, state: &WindowState) {
    let window = match app.get_webview_window("main") {
        Some(w) => w,
        None => {
            log::warn!("未找到主窗口，跳过窗口状态恢复");
            return;
        }
    };
    apply_state_to_window(
        &window,
        state,
        "main",
        MIN_WINDOW_W,
        MIN_WINDOW_H,
        1440.0,
        920.0,
    );
}

/// 按 label 读取持久化的窗口几何并应用到指定的 WebviewWindow
pub fn apply_saved_window_state<R: tauri::Runtime>(
    window: &tauri::webview::WebviewWindow<R>,
    label: &str,
    min_w: f64,
    min_h: f64,
    default_w: f64,
    default_h: f64,
) {
    let Some(dir) = app_config_dir() else {
        return;
    };
    let Some(state) = load_state(&dir, label) else {
        return;
    };
    if state.is_empty() {
        return;
    }
    apply_state_to_window(window, &state, label, min_w, min_h, default_w, default_h);
}

fn apply_state_to_window<R: tauri::Runtime>(
    window: &tauri::webview::WebviewWindow<R>,
    state: &WindowState,
    label: &str,
    min_w: f64,
    min_h: f64,
    _default_w: f64,
    _default_h: f64,
) {
    // scale factor：优先用采集时记录的，窗口当前值兜底
    let scale = state
        .scale_factor
        .filter(|s| *s > 0.0)
        .unwrap_or_else(|| window.scale_factor().unwrap_or(1.0));

    // 先收集显示器：尺寸与位置都要用它来夹。
    let monitors = collect_monitor_rects(window, scale);
    let primary = window.primary_monitor().ok().flatten().map(|m| {
        let rect = m.work_area();
        (
            to_logical(rect.position.x, scale),
            to_logical(rect.position.y, scale),
            to_logical(rect.position.x + rect.size.width as i32, scale),
            to_logical(rect.position.y + rect.size.height as i32, scale),
        )
    });

    // 尺寸：inner_size 系列的入参是逻辑像素，而存盘前已换算成逻辑值
    let (mut win_w, mut win_h) = (
        window
            .inner_size()
            .map(|s| s.width as f64)
            .unwrap_or(1440.0),
        window
            .inner_size()
            .map(|s| s.height as f64)
            .unwrap_or(920.0),
    );
    if let (Some(w), Some(h)) = (state.width, state.height) {
        // 防御：损坏/手改出极端值会把窗口缩成一条线
        let (w, h) = window_state::clamp_size_into_monitors(
            w.clamp(min_w, 16384.0),
            h.clamp(min_h, 16384.0),
            &monitors,
            primary,
        );
        win_w = w;
        win_h = h;
        // 恢复失败只记日志：窗口保持当前尺寸，用户可手动调整（R4）。
        if let Err(e) = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
            width: w,
            height: h,
        })) {
            log::warn!("窗口 {label} 恢复尺寸失败: {e}");
        }
    }

    // 位置：按夹好的尺寸把整块窗口拉进显示器。只夹左上角是不够的 ——
    // 换分辨率后窗口可能比屏幕还高，左上角在屏内但内容大半截在屏外。
    if let (Some(x), Some(y)) = (state.x, state.y) {
        let (x, y) =
            window_state::clamp_position_into_monitors(x, y, win_w, win_h, &monitors, primary);
        if let Err(e) =
            window.set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }))
        {
            log::warn!("窗口 {label} 恢复位置失败: {e}");
        }
    }

    // 最大化放最后：系统在某些平台会因 set_size/set_position 重置最大化状态
    if state.maximized == Some(true) {
        if let Err(e) = window.maximize() {
            log::warn!("窗口 {label} 恢复最大化失败: {e}");
        }
    }
}

/// 收集所有显示器的工作区矩形（逻辑像素）。
///
/// 任一显示器取不到就跳过它：宁可少一个可见区域，也不能让恢复失败。
fn collect_monitor_rects<R: tauri::Runtime>(
    window: &tauri::webview::WebviewWindow<R>,
    scale: f64,
) -> Vec<(f64, f64, f64, f64)> {
    let mut rects = Vec::new();
    if let Ok(monitors) = window.available_monitors() {
        for m in monitors {
            let rect = m.work_area();
            let left = to_logical(rect.position.x, scale);
            let top = to_logical(rect.position.y, scale);
            let right = to_logical(rect.position.x + rect.size.width as i32, scale);
            let bottom = to_logical(rect.position.y + rect.size.height as i32, scale);
            rects.push((left, top, right, bottom));
        }
    }
    rects
}

#[cfg(target_os = "macos")]
fn ensure_macos_gui_environment() {
    let current_path = std::env::var("PATH").unwrap_or_default();
    let mut parts: Vec<String> = current_path
        .split(':')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();

    // 1. 尝试从用户登录 shell 获取真实 PATH（带 1.5s 有界超时）
    if let Ok(shell) = std::env::var("SHELL") {
        if !shell.is_empty() {
            let mut cmd = std::process::Command::new(&shell);
            cmd.args(["-l", "-c", "printf '%s' \"$PATH\""]);
            if let Ok(output) = crate::commands::run_with_timeout_spawn(
                &mut cmd,
                std::time::Duration::from_millis(1500),
            ) {
                if output.status.success() {
                    let shell_path = String::from_utf8_lossy(&output.stdout);
                    for p in shell_path.split(':') {
                        let trimmed = p.trim();
                        if !trimmed.is_empty() && !parts.iter().any(|existing| existing == trimmed)
                        {
                            parts.push(trimmed.to_string());
                        }
                    }
                }
            }
        }
    }

    // 2. 常见开发者 bin 目录兜底（Homebrew / Cargo / local bin 等）
    let fallback_dirs = [
        "/opt/homebrew/bin",
        "/opt/homebrew/sbin",
        "/usr/local/bin",
        "/usr/local/sbin",
    ];
    for dir in fallback_dirs {
        if std::path::Path::new(dir).exists() && !parts.iter().any(|p| p == dir) {
            parts.insert(0, dir.to_string());
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            let user_dirs = [
                format!("{home}/.cargo/bin"),
                format!("{home}/.local/bin"),
                format!("{home}/go/bin"),
            ];
            for dir in user_dirs {
                if std::path::Path::new(&dir).exists() && !parts.iter().any(|p| p == &dir) {
                    parts.insert(0, dir);
                }
            }
        }
    }

    let merged_path = parts.join(":");
    std::env::set_var("PATH", merged_path);
}

/// 把进程 fd 软上限抬到 [`FD_SOFT_TARGET`]。
///
/// 实测（macOS 26，Finder 启动的 .app）：`getrlimit(RLIMIT_NOFILE)` 的软上限是
/// **256**，硬上限 unlimited —— 从 Terminal.app 启动的进程是 1M，所以开发时
/// 看不出来，双击安装的正式包才会撞上。每个常驻 PTY 占 2 个 fd（master +
/// 读端 dup），加上 WebView / 文件监听 / LSP 的基础占用（空闲实测 81），
/// 约 85 个会话后 `openpty`/`spawn` 开始 `EMFILE`。10 项目 × 10 控制台的目标
/// 需要 100+ 个常驻会话，因此必须在创建任何资源前抬限。
///
/// 取值 10240：远低于 `kern.maxfilesperproc`（实测 92160），且 100 会话
/// + 全部非终端资源也只占几百个 fd；不取 unlimited 是为了失败时容易归因。
#[cfg(target_os = "macos")]
pub const FD_SOFT_TARGET: u64 = 10240;

/// 提升 fd 软上限（幂等；macOS 以外的平台为空实现）。
///
/// 必须在创建 Tauri Builder / 任何 PTY 之前调用。失败只记日志：fd 上限是
/// 容量保护，不是启动前提，拿不到新上限时应用仍应可用（只是会话数受限）。
#[cfg(target_os = "macos")]
pub fn ensure_fd_limit() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let mut limit: libc::rlimit = std::mem::zeroed();
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) != 0 {
            log::warn!("读取 fd 上限失败: {}", std::io::Error::last_os_error());
            return;
        }
        // macOS 的 rlim_t 就是 u64（函数本身是 macOS-only）。
        if limit.rlim_cur >= FD_SOFT_TARGET {
            log::info!("fd 软上限已足够: {}", limit.rlim_cur);
            return;
        }
        // rlim_max 可能是 RLIM_INFINITY（u64::MAX），不能直接拿来当目标值。
        let hard = limit.rlim_max;
        let target = if hard == libc::RLIM_INFINITY {
            FD_SOFT_TARGET
        } else {
            FD_SOFT_TARGET.min(hard)
        };
        let next = libc::rlimit {
            rlim_cur: target as libc::rlim_t,
            rlim_max: hard,
        };
        if libc::setrlimit(libc::RLIMIT_NOFILE, &next) == 0 {
            log::info!("fd 软上限: {} -> {}", limit.rlim_cur, target);
        } else {
            log::warn!(
                "提升 fd 上限失败（{} -> {}）: {}",
                limit.rlim_cur,
                target,
                std::io::Error::last_os_error()
            );
        }
    });
}

#[cfg(not(target_os = "macos"))]
pub fn ensure_fd_limit() {}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 先抬 fd 上限：PTY 会话、文件监听、LSP 都按这个额度分配（见函数注释）。
    ensure_fd_limit();
    #[cfg(target_os = "macos")]
    ensure_macos_gui_environment();
    // 第一件事：release 是 panic = "abort"，崩溃时进程直接没，装 hook 才能留下现场。
    install_panic_hook();
    let saved_window_state = restore_window_state();
    // setup 闭包要求 'static：克隆一份进去，run() 里保留原值不再使用
    let restore_in_setup = saved_window_state.clone();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(PtyManager::new())
        .manage(commands::WindowTitleRegistry(std::sync::Mutex::new(
            std::collections::HashMap::new(),
        )))
        // 已授权的工作区根目录集合：read_file_content / write_file_content /
        // fs_delete / fs_paste 只允许落在这些根之内。前端的项目树与编辑器
        // 只操作工作区内路径，越界请求一律来自被诱导的仓库内容，必须拒绝。
        .manage(commands::WorkspaceRegistry::global().clone())
        .manage(commands::PathGrantRegistry::default())
        .manage(file_watch::FileWatchState::default())
        // LSP server 进程表（与 PTY 会话表同级：退出时统一收尸，见 RunEvent::Exit）
        .manage(lsp::LspRegistry::default())
        // 文件读缓存（read_file_content 读穿，write 失效；纯优化层，见 file_cache.rs）
        .manage(file_cache::FileCache::default())
        .manage(commands::ConflictState::default())
        .manage(commands::MergeToolState::default())
        // 窗口关闭时清掉标题注册表条目：否则「打开的项目」分区
        // 会一直列出已关闭窗口的项目，点击聚焦时报「窗口不存在」。
        //
        // 另外在这里采集窗口几何：Moved/Resized 时把位置与尺寸写盘，
        // 下次启动按它恢复（见 restore_window_state）。Destroyed 时补最后
        // 一次采集 —— 用户直接关窗口时 CloseRequested 之后不再有 Resized，
        // 只有最后落一次盘才不丢「退出前那次拖动」。
        .on_window_event(|window, event| {
            // 聚焦跟踪：菜单事件要定向到活动窗口（见 FOCUSED_WINDOW 的说明）。
            // 失焦置空而非保留旧 label —— 应用整体切走时点菜单，发事件给
            // 不可见的窗口比丢弃更糟。
            if let tauri::WindowEvent::Focused(focused) = event {
                let label = window.label().to_string();
                if let Ok(mut guard) = FOCUSED_WINDOW.lock() {
                    if *focused {
                        *guard = label;
                    } else if guard.as_str() == label {
                        guard.clear();
                    }
                }
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let current_label = window.label().to_string();
                // 只有在尝试关闭主窗口时，才拦截并优先处理未关闭的独立子窗口；
                // 独立子窗口自身关闭时必须正常放行，绝不可互相拦截导致死锁！
                if current_label == "main" {
                    let app = window.app_handle();
                    let child_order = ["win_merge_tool", "win_conflicts"];
                    let mut target_child = None;
                    for child_lbl in child_order {
                        if let Some(child_win) = app.get_webview_window(child_lbl) {
                            target_child = Some(child_win);
                            break;
                        }
                    }

                    if let Some(child_win) = target_child {
                        api.prevent_close();
                        if let Err(e) = child_win.unminimize() {
                            log::warn!("恢复子窗口最小化失败: {e}");
                        }
                        if let Err(e) = child_win.show() {
                            log::warn!("显示子窗口失败: {e}");
                        }
                        if let Err(e) = child_win.set_focus() {
                            log::warn!("聚焦子窗口失败: {e}");
                        }
                    }
                }
            }
            let window_label = window.label().to_string();
            // 采集窗口几何：Moved/Resized/Destroyed 都要。最小化期间不采集
            // —— 那会把「最小化位置」当正常位置写盘，恢复后窗口跑到错误的地方。
            let should_capture = matches!(
                event,
                tauri::WindowEvent::Moved(_)
                    | tauri::WindowEvent::Resized(_)
                    | tauri::WindowEvent::Destroyed
            );
            if should_capture {
                // 最小化 / 原生全屏的跳过判定已内置于 capture
                if let Ok(scale) = window.scale_factor() {
                    if let Some(state) = window_state::capture(window, scale) {
                        persist_window_state_for(&window_label, &state);
                    }
                }
            }
            if let tauri::WindowEvent::Destroyed = event {
                let label = window.label().to_string();
                if label == "win_conflicts" || label == "win_merge_tool" {
                    let app = window.app_handle();
                    let has_conflicts = app.get_webview_window("win_conflicts").is_some()
                        && label != "win_conflicts";
                    let has_merge = app.get_webview_window("win_merge_tool").is_some()
                        && label != "win_merge_tool";
                    if !has_conflicts && !has_merge {
                        use tauri::Emitter;
                        if let Err(e) = app.emit("ide_conflict_closed", ()) {
                            log::warn!("广播 ide_conflict_closed 失败: {e}");
                        }
                    }
                }
                if let Some(registry) = window.try_state::<commands::WindowTitleRegistry>() {
                    if let Ok(mut guard) = registry.0.lock() {
                        guard.remove(&label);
                    }
                }
                // 窗口销毁兜底：终止该窗口创建的全部 PTY 会话。
                // 前端关窗口只调 getCurrentWindow().close()，没有机会
                // 逐个 pty_close，shell 子进程与读线程会一直泄漏。
                if let Some(manager) = window.try_state::<PtyManager>() {
                    // 只清理本窗口创建的 PTY：多窗口（多项目）下 close_all
                    // 会把其它窗口正在使用的终端一起误杀。
                    manager.close_for_window(&label);
                }
                // 同理停掉本窗口的文件监听：watcher 随窗口一起消失，
                // 否则窗口反复开关会累积无人接收事件的监听器（目录 fd 泄漏）。
                if let Some(watch) = window.try_state::<file_watch::FileWatchState>() {
                    watch.stop_for_window(&label);
                }
            }
        })
        // 顶层导航守卫（P0）：Markdown 外链一点即让 webview 整页导航到远程站点，
        // 远程 JS 将继承全部 IPC 命令（含 pty_write = 以用户身份执行 shell）。
        // Tauri 2 的应用级拦截点是 plugin 的 `on_navigation`
        //（WebviewWindowBuilder 的同名方法是建窗时选项，管不到配置自动建的主窗口）。
        // 前端点击拦截是第一道（带确认与文案），这里是绕不过的第二道：
        // 本地文档放行，外部 http(s) 送系统浏览器，其余 scheme 直接拦。
        .plugin(
            tauri::plugin::Builder::<tauri::Wry>::new("navigation-guard")
                .on_navigation(|_webview, url| {
                    use commands::NavigationDecision;
                    match commands::navigation_policy(url.as_str()) {
                        NavigationDecision::Allow => true,
                        NavigationDecision::OpenExternally => {
                            if let Err(e) = commands::open_url_in_browser(url.as_str()) {
                                log::warn!("外部链接送浏览器失败 {}: {}", url.as_str(), e);
                            }
                            false
                        }
                        NavigationDecision::Block => {
                            log::warn!("拦截非本地页面导航: {}", url.as_str());
                            false
                        }
                    }
                })
                .build(),
        )
        .setup(move |app| {
            // Production must not run `git -C` before a workspace is registered:
            // an empty registry is fail-closed from here on. Tests never call
            // setup, so they keep the permissive test-only behavior.
            crate::commands::WorkspaceRegistry::global().set_enforced();
            // panic 日志目录：run() 开头装 hook 时拿不到 AppHandle，
            // 这里补填（这之前的崩溃只有 stderr）。
            if let Ok(dir) = app.path().app_log_dir() {
                let _ = std::fs::create_dir_all(&dir);
                // hook 安装与 setup 的时序：重复进入时已设置，保留首次值即可，
                // 不是错误（R4：这里的"丢弃"是显式语义，不是静默吞错）。
                if PANIC_LOG_DIR.set(dir).is_err() {
                    log::debug!("panic 日志目录已设置过，保留首次值");
                }
            }
            // 恢复上次的窗口位置/尺寸/最大化状态。首次启动（无状态文件）
            // 时 saved_window_state 为 None，窗口保持 tauri.conf.json 的
            // 默认值。恢复放在 setup 而不是 Builder：那时窗口已创建，但
            // 还没显示（setup 在首次 show 之前跑），用户看不到「先跳默认
            // 位置再跳到恢复位置」的闪烁。
            if let Some(state) = &restore_in_setup {
                apply_window_state(app.handle(), state);
            }
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            // 原生应用菜单：菜单项只 emit `app-menu://<id>`，业务全在前端。
            // 放在 setup 里而不是 Builder::menu：后者要求字符串字面量，
            // 且拿不到 AppHandle 去构造带本地化标题的子菜单。
            //
            // 首次构造就用上次保存的语言：菜单早于前端代码执行，若这里用
            // 硬编码英文，用户切到中文后重启会看到「菜单先英文再跳中文」。
            // 读不到 language 文件时 labels_for 内部回落英文。
            let menu_labels = labels_for(language::load_language(&app_config_dir_or_default()));
            let app_menu = build_menu_with(app.handle(), menu_labels)?;
            app.set_menu(app_menu)?;
            // on_menu_event 是 app 级回调，MenuEvent 不带窗口 label
            // （tauri 2.11 的 MenuEvent 只有 id 字段）。因此按全局维护的
            // 聚焦窗口定向 emit：只让活动窗口的前端实例响应，
            // 未聚焦窗口的 React state 独立，不该被别处的菜单点击驱动。
            app.on_menu_event(move |app_handle, event: MenuEvent| {
                let label = FOCUSED_WINDOW.lock().map(|l| l.clone()).unwrap_or_default();
                let window = if !label.is_empty() {
                    app_handle.get_webview_window(&label)
                } else {
                    // 没有聚焦记录时，兜底取当前第一个可见窗口（解决点击原生菜单栏导致窗口短暂失焦的问题）
                    app_handle
                        .webview_windows()
                        .into_iter()
                        .find(|(_, w)| w.is_visible().unwrap_or(false))
                        .map(|(_, w)| w)
                };

                if let Some(win) = window {
                    emit_menu_event(&win, event.id().as_ref());
                } else {
                    // 完全没有可见窗口时的 macOS 菜单兜底：
                    // 点击 File -> New Window 或 Open Project 时直接新起一个窗口
                    let id = event.id().as_ref();
                    if id == menu::ids::NEW_WINDOW || id == menu::ids::OPEN_PROJECT {
                        let app = app_handle.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Err(e) = commands::create_new_window(app).await {
                                log::error!("无窗口时菜单创建新窗口失败: {e}");
                            }
                        });
                    } else {
                        log::debug!("菜单事件 {} 无可用窗口，已丢弃", id);
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            clipboard_file_paths,
            read_directory,
            read_file_content,
            read_file_text,
            pick_path,
            read_image_bytes,
            write_file_content,
            write_file_text,
            register_workspace,
            unregister_workspace,
            fs_create_file,
            fs_create_directory,
            fs_rename,
            fs_delete,
            fs_path_exists,
            file_probe,
            fs_paste,
            fs_reveal,
            open_external_url,
            fs_diff_files,
            git_status,
            git_add,
            git_log,
            git_branches,
            git_checkout,
            git_create_branch,
            git_commit,
            git_push,
            global_search,
            replace_in_files,
            search_files,
            git_diff,
            git_show,
            git_diff_commits,
            git_file_at_rev,
            git_commit_files,
            git_commits_files,
            git_commits_diff,
            detect_ghostty,
            ghostty_list_themes,
            ghostty_load_theme,
            ghostty_show_config,
            launch_ghostty,
            pty_spawn,
            pty_write,
            pty_resize,
            pty_close,
            pty_close_by_cwd,
            pty_session_busy,
            vt_images,
            vt_image_data,
            vt_snapshot,
            vt_snapshot_diff,
            vt_scroll,
            vt_wheel,
            vt_mouse,
            vt_mouse_tracking,
            vt_set_colors,
            vt_encode_key,
            vt_set_selection,
            vt_select_word,
            vt_select_line,
            vt_clear_selection,
            vt_selected_text,
            vt_search,
            vt_engine_info,
            git_revert_commit,
            git_discard_file_changes,
            git_repo_info,
            git_init,
            git_pending_commits,
            git_remote_list,
            git_remote_add,
            git_remote_remove,
            git_remote_rename,
            git_remote_set_url,
            git_pull,
            git_fetch,
            git_rename_branch,
            git_delete_branch,
            git_create_tag,
            git_list_tags,
            git_delete_tag,
            git_clone,
            git_submodule_list,
            git_submodule_add,
            git_submodule_update,
            git_submodule_sync,
            git_worktree_list,
            git_worktree_add,
            git_worktree_remove,
            git_lfs_status,
            git_lfs_pull,
            git_lfs_track,
            git_credential_status,
            git_credential_set_helper,
            git_apply_patch,
            git_compare_refs,
            git_compare_commits,
            git_compare_branches,
            git_cherry_pick,
            git_sequencer_state,
            git_merge_abort,
            git_merge_continue,
            git_rebase_continue,
            git_rebase_abort,
            git_rebase_skip,
            git_cherry_pick_continue,
            git_cherry_pick_abort,
            git_reset,
            git_blame,
            git_file_history,
            git_diff_ref_worktree,
            git_stash_list,
            git_stash_push,
            git_stash_files,
            git_stash_apply,
            git_stash_drop,
            git_merge,
            git_rebase,
            git_amend_message,
            git_drop_commit,
            git_drop_commits,
            git_squash_commits,
            git_commit_patch,
            git_diff_patch,
            git_checkout_file_revision,
            git_file_change_hunks,
            git_rebase_interactive,
            git_list_shelves,
            git_shelve_save,
            git_delete_shelf,
            git_apply_shelf,
            git_diff_patch_paths,
            git_apply_shelf_partial,
            git_list_deleted_shelves,
            git_restore_shelf,
            git_rename_shelf,
            git_delete_shelf_file,
            git_delete_shelf_files,
            git_read_shelf_patch,
            git_import_shelf_patch,
            git_shelf_diff_preview,
            changelists_load,
            changelist_create,
            changelist_delete,
            changelist_assign,
            git_merge_file_content,
            git_conflict_kinds,
            git_merge_file_marked,
            git_stage_file,
            git_use_merge_side,
            set_window_title,
            set_app_menu_language,
            list_other_windows,
            focus_window,
            create_new_window,
            commands::finish_app_exit,
            commands::cancel_app_exit,
            commands::open_conflict_window,
            commands::get_conflict_repo_path,
            commands::open_merge_tool_window,
            commands::get_merge_tool_params,
            commands::close_window,
            commands::restart_app,
            commands::is_conflict_window_open,
            fs_watch_files,
            fs_watch_dirs,
            file_range::read_file_range,
            lsp::lsp_server_start,
            lsp::lsp_server_send,
            lsp::lsp_server_stop,
            lsp::lsp_server_status,
            extensions::list_extensions
        ])
        .build(tauri::generate_context!())
        // R1 特批（见 scripts/verify-rust-discipline.mjs PANIC_ALLOW）：
        // 应用构造失败意味着无 WebView / 无运行时，进程不可能继续服务；
        // panic 走已安装的 hook 落 panic.log，下次启动可上报。直接 exit(1)
        // 会静默跳过这条崩溃上报链。行为：启动即崩，但有日志可查。
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // 应用退出兜底：窗口销毁路径（on_window_event → close_for_window）
            // 覆盖不了「退出时没有任何窗口销毁事件」的场景，而 setsid + 忽略
            // SIGHUP 的守护进程（用户跑 `npm run dev &`）会活过父进程退出。
            // RunEvent::Exit 是 event loop 真正结束前最后一个回调。
            match event {
                tauri::RunEvent::ExitRequested { ref api, .. } => {
                    let child_order = ["win_merge_tool", "win_conflicts"];
                    let mut target_child = None;
                    for child_lbl in child_order {
                        if let Some(child_win) = app_handle.get_webview_window(child_lbl) {
                            target_child = Some(child_win);
                            break;
                        }
                    }
                    if target_child.is_none() {
                        for (lbl, win) in app_handle.webview_windows() {
                            if lbl != "main" {
                                target_child = Some(win);
                                break;
                            }
                        }
                    }

                    if let Some(child_win) = target_child {
                        api.prevent_exit();
                        if let Err(e) = child_win.unminimize() {
                            log::warn!("恢复子窗口最小化失败: {e}");
                        }
                        if let Err(e) = child_win.show() {
                            log::warn!("显示子窗口失败: {e}");
                        }
                        if let Err(e) = child_win.set_focus() {
                            log::warn!("聚焦子窗口失败: {e}");
                        }
                    } else if !EXIT_FLUSH_DONE.load(Ordering::SeqCst) {
                        // 主窗口还在：先 prevent 退出，广播写盘事件；前端完成
                        // 后调用 finish_app_exit 再真正退出。没有主窗口时
                        // （close-requested 已写盘并 destroy）直接放行。
                        if let Some(main) = app_handle.get_webview_window("main") {
                            api.prevent_exit();
                            if !EXIT_FLUSH_PENDING.swap(true, Ordering::SeqCst) {
                                use tauri::Emitter;
                                if let Err(e) = main.emit("app://before-exit", ()) {
                                    log::warn!("广播退出前写盘事件失败: {e}");
                                }
                                let app = app_handle.clone();
                                std::thread::spawn(move || {
                                    std::thread::sleep(std::time::Duration::from_secs(10));
                                    if EXIT_FLUSH_PENDING.load(Ordering::SeqCst)
                                        && !EXIT_FLUSH_DONE.load(Ordering::SeqCst)
                                    {
                                        log::warn!("退出前写盘超时，强制退出");
                                        EXIT_FLUSH_DONE.store(true, Ordering::SeqCst);
                                        app.exit(0);
                                    }
                                });
                            }
                        }
                    }
                }
                tauri::RunEvent::Opened { urls } => {
                    // Finder / Dock "Open Recent" / "Open With" on macOS. The
                    // frontend owns workspace switching, so forward the local
                    // file URLs and let it confirm dirty buffers as usual.
                    let paths: Vec<String> = urls
                        .iter()
                        .filter_map(|url| url.to_file_path().ok())
                        .map(|path| path.to_string_lossy().into_owned())
                        .collect();
                    if !paths.is_empty() {
                        if let Some(main) = app_handle.get_webview_window("main") {
                            use tauri::Emitter;
                            if let Err(e) = main.emit("app://open-paths", paths) {
                                log::warn!("广播打开路径失败: {e}");
                            }
                            if let Err(e) = main.set_focus() {
                                log::warn!("聚焦主窗口失败: {e}");
                            }
                        }
                    }
                }
                tauri::RunEvent::Reopen {
                    has_visible_windows,
                    ..
                } => {
                    if !has_visible_windows {
                        let mut restored = false;
                        for (_, win) in app_handle.webview_windows() {
                            if let Ok(true) = win.is_minimized() {
                                if let Err(e) = win.unminimize() {
                                    log::warn!("Reopen 取消最小化失败: {e}");
                                }
                                if let Err(e) = win.show() {
                                    log::warn!("Reopen 显示窗口失败: {e}");
                                }
                                if let Err(e) = win.set_focus() {
                                    log::warn!("Reopen 聚焦窗口失败: {e}");
                                }
                                restored = true;
                                break;
                            } else if let Ok(false) = win.is_visible() {
                                if let Err(e) = win.show() {
                                    log::warn!("Reopen 显示隐藏窗口失败: {e}");
                                }
                                if let Err(e) = win.set_focus() {
                                    log::warn!("Reopen 聚焦窗口失败: {e}");
                                }
                                restored = true;
                                break;
                            }
                        }
                        if !restored {
                            let app = app_handle.clone();
                            tauri::async_runtime::spawn(async move {
                                if let Err(e) = commands::create_new_window(app).await {
                                    log::error!("Reopen 创建新窗口失败: {e}");
                                }
                            });
                        }
                    }
                }
                tauri::RunEvent::Exit => {
                    if let Some(manager) = app_handle.try_state::<PtyManager>() {
                        manager.close_all();
                    }
                    // LSP server 同理：不杀就成孤儿（见 LspRegistry::shutdown_all）
                    if let Some(registry) = app_handle.try_state::<lsp::LspRegistry>() {
                        registry.shutdown_all();
                    }
                }
                _ => {}
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 崩溃日志的核心契约：消息 + 位置落到 panic.log，且是**追加**（第二次
    /// 崩溃不能盖掉第一次的现场）。安装的是真 hook，测完把原 hook 还回去，
    /// 免得替换掉 libtest 的捕获 hook。
    #[test]
    fn panic_hook_appends_message_and_location_to_log_file() {
        let dir = std::env::temp_dir().join(format!("lazecode_panic_log_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // 本进程内只有这个测试用 PANIC_LOG_DIR
        PANIC_LOG_DIR.set(dir.clone()).expect("独占 PANIC_LOG_DIR");

        let previous = std::panic::take_hook();
        install_panic_hook();
        let _ = std::panic::catch_unwind(|| panic!("boom-7"));
        let _ = std::panic::catch_unwind(|| panic!("boom-8"));
        std::panic::set_hook(previous);

        let log = std::fs::read_to_string(dir.join("panic.log")).expect("应写入 panic.log");
        assert!(
            log.contains("boom-7"),
            "第一次崩溃的消息及位置要留下：{log}"
        );
        assert!(log.contains("boom-8"), "追加而非覆盖：{log}");
        assert!(log.contains("src/lib.rs:"), "缺少 panic 位置：{log}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
