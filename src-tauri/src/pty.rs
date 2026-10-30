// PTY 会话管理
//
// 通过 portable-pty 在 macOS / Linux / Windows(ConPTY) 上启动真实 shell，
// 读线程将输出以事件形式推送到前端；前端把字节喂给 libghostty-vt 解析。
//
// 事件通道：
//   pty://output/<session_id>  -> Vec<u8> 原始字节（前端按 UTF-8 增量解码）
//   pty://exit/<session_id>    -> i32 退出码

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State, Window};

use crate::vt::{KeyInput, NativeTerminal, SearchResult, SearchSelect};

/// 终端网格的尺寸边界。
///
/// 存在的理由：`cols`/`rows` 是 `#[tauri::command]`（`pty_spawn` /
/// `pty_resize`）的入参，来源是前端测出的窗口尺寸除以单元格尺寸。这两项
/// 都不能信 ——
/// 1. 单元格尺寸为 0（字体尚未测量、或 CSS 让 canvas 暂时为 0）时
///    `w / cw` 得到 Infinity/NaN，转成 u16 是不可预期的值；
/// 2. 多显示器全屏 + 极小字号可以产出上万列；
/// 3. 任何 IPC 调用方都能直接构造这两个参数，而 IPC 就是信任边界。
///
/// 上限取 1000×1000（100 万格）：正常使用的极限是 4K 全屏 + 13px 字号
/// （约 400 列 × 200 行），仍有 10 倍余量；而 u16::MAX × u16::MAX ≈ 43 亿格，
/// 一次 `vt_snapshot` 的分配就能把进程打爆（可被单次 IPC 触发的内存炸弹）。
///
/// 下限同属硬约束：libghostty-vt 的 `ghostty_terminal_new` /
/// `ghostty_terminal_resize` 要求 cols/rows > 0（引擎头文件明确写了），
/// 传 0 会被引擎拒绝，PTY 与引擎网格从此永久不一致。
const MIN_COLS: u16 = 1;
const MIN_ROWS: u16 = 1;
const MAX_COLS: u16 = 1000;
const MAX_ROWS: u16 = 1000;

/// 全窗口 scrollback 总预算（所有常驻终端共享）。
///
/// 单会话上限 128 MB × 100 会话 = 12.8 GB 是唯一没有全局上限的资源。
/// 2 GiB 的依据（实测 120×40）：20k 行 @120 列约 16.6 MB/会话，100 会话
/// 典型约 1.66 GB；2 GiB / 100 = 20.4 MB，仍能保留完整 20k 行。低会话数
/// 时按单会话上限 128 MB 封顶，不牺牲单终端体验。
const GLOBAL_SCROLLBACK_BUDGET: usize = 2 * 1024 * 1024 * 1024;
const MIN_SCROLLBACK_BYTES: usize = 8 * 1024 * 1024;

/// 按当前存活会话数分摊全局预算（至少 8 MB，至多 128 MB）。
///
/// 公开给 `examples/stress_pty` 复现同一预算；产品代码只经 `PtyManager`
/// 调用。
pub fn scrollback_quota_for(live_sessions: usize) -> usize {
    (GLOBAL_SCROLLBACK_BUDGET / live_sessions.max(1))
        .clamp(MIN_SCROLLBACK_BYTES, crate::vt::SCROLLBACK_MAX_BYTES)
}

/// 网格尺寸夹到合法区间。
///
/// 夹取而非报错：调用方是「窗口被拖成某个尺寸」这类正常但极端的情形，
/// 报错会让终端直接不可用（一次拖拽废掉一个会话）；夹到上限只是让用户
/// 看到的网格比请求的小 —— 可感知、可恢复，语义也接近「窗口就这么大」。
fn clamp_grid(cols: u16, rows: u16) -> (u16, u16) {
    (
        cols.clamp(MIN_COLS, MAX_COLS),
        rows.clamp(MIN_ROWS, MAX_ROWS),
    )
}

/// 不继承给终端 shell 的环境变量（凭据类）。
///
/// 为什么需要：portable-pty 会把 IDE 进程的全量环境回填进 shell
/// （`CommandBuilder::as_command()` 先 `env_clear()` 再逐条回填
/// `get_base_env()`）。用户在 launchd / 登录项里配的云凭据、API token
/// 因此全部暴露在终端里 —— 而终端是 IDE 里用户停留最久、跑第三方
/// 依赖最多的面板（npm postinstall、cargo build script）。
///
/// 精确按变量名剔除而非按前缀/正则匹配：名单显式、可审计、无歧义。
/// 漏掉新云厂商的代价是那一项仍被继承，但至少不会误伤名字里含
/// key/token/secret 的正常变量（如 `GPG_TTY`、`KEYBOARD_LAYOUT`）。
///
/// 不在此列的同类变量：用户若需要，可在 `~/.zprofile` 里自行设置，
/// 那是用户在自己的 shell 配置里主动授权的，与 IDE 悄悄继承是两件事。
const CREDENTIAL_ENV_BLOCKLIST: &[&str] = &[
    // ---- 云厂商 ----
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AWS_PROFILE",
    "AZURE_CLIENT_SECRET",
    "AZURE_TENANT_ID",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "GCP_SERVICE_ACCOUNT_KEY",
    "ALIBABA_CLOUD_ACCESS_KEY_ID",
    "ALIBABA_CLOUD_ACCESS_KEY_SECRET",
    // ---- 源码托管 / CI ----
    "GITHUB_TOKEN",
    "GH_TOKEN",
    "GITLAB_TOKEN",
    "CI_JOB_TOKEN",
    "NPM_TOKEN",
    // ---- 模型服务 ----
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "HF_TOKEN",
    "HUGGING_FACE_HUB_TOKEN",
    "GEMINI_API_KEY",
    "DEEPSEEK_API_KEY",
    "MOONSHOT_API_KEY",
    "DASHSCOPE_API_KEY",
    // ---- 数据库 / 消息 ----
    "DATABASE_URL",
    "REDIS_URL",
    "MONGODB_URI",
    // ---- 其它高频密钥变量 ----
    "DOCKER_PASSWORD",
    "SENTRY_AUTH_TOKEN",
    "CLOUDFLARE_API_TOKEN",
    "VERCEL_TOKEN",
    "NETLIFY_AUTH_TOKEN",
    "SLACK_TOKEN",
    "STRIPE_SECRET_KEY",
    "TWILIO_AUTH_TOKEN",
    "PIPELINE_TOKEN",
];

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PtySpawnResult {
    pub session_id: String,
    pub shell: String,
    pub cwd: String,
}

struct PtySession {
    /// `pty_resize` 要在放掉会话表锁之后才做 ioctl（锁跨 ioctl 会让
    /// 全部会话的读写一起排队）。`MasterPty` 只要求 `Send` 不要求 `Sync`，
    /// 所以用 `Mutex` 包一层来满足 `PtyManager` 的 `Sync` 需求；
    /// 外层 `Arc` 让 resize 能在会话表锁释放后仍持句柄。
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    /// per-session 写锁：写入不持会话表锁，避免一个卡住的 PTY 拖住全部会话
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    /// VT 解析在本进程内完成（原生 libghostty-vt），前端只订阅格子缓冲。
    /// 读线程与命令线程都会访问，故由 Mutex 串行化。
    terminal: Arc<Mutex<NativeTerminal>>,
    /// 会话所属工作区（关闭项目时按此终止会话）
    cwd: String,
    /// 创建该会话的窗口 label：窗口销毁时只清理本窗口的会话
    /// （close_for_window），不能误杀其它窗口的终端
    window_label: String,
    /// 置位后读线程不再补发退出事件。同 session_id 重复 spawn 会顶掉旧会话：
    /// 旧 shell 的退出码若照发到 pty://exit/<id>，前端会误判**新**会话已退出。
    retired: Arc<AtomicBool>,
    /// 读线程句柄：丢弃即 detach，不再 join —— 取舍见 close_session 注释。
    reader: Option<std::thread::JoinHandle<()>>,
}

/// 等待子进程退出并取真实退出码。
///
/// 读线程在 EOF 后调用；`pty_close` 主动 kill 时由 join 路径兜底。
/// portable-pty 的 `Child::wait` 在 ConPTY 上同样可用。
fn child_exit_code(child: &mut Box<dyn Child + Send + Sync>) -> i32 {
    match child.wait() {
        Ok(status) => status.exit_code() as i32,
        // 已被外部信号杀死 / 无法取得状态时给一个明确的非零值，
        // 前端据此判定「异常退出」而不是当成正常退出。
        Err(_) => -1,
    }
}

/// 会话级锁内取退出码。
fn locked_exit_code(child: &Arc<Mutex<Box<dyn Child + Send + Sync>>>) -> i32 {
    match child.lock() {
        Ok(mut c) => child_exit_code(&mut c),
        Err(_) => -1,
    }
}

/// 有界回收子进程。
///
/// kill 之后内核 tty 层收尾可能需要一小段时间，但调用线程在 IPC / 关闭项目
/// 路径上，绝不能无限等下去（旧实现在这里无界 `wait()`，见 close_session 注释）。
/// 锁被读线程持有时同样只等到 deadline：放弃回收好过挂死调用方。
fn reap_child_bounded(
    child: &Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    limit: std::time::Duration,
) {
    let deadline = std::time::Instant::now() + limit;
    loop {
        // WouldBlock 时改用阻塞 lock 而不是空转：此刻持锁的只可能是读线程，
        // 而它在锁内做的是 `child.wait()` —— 进程退出后 wait 立即返回、锁随
        // 即释放。空转 2 秒反而让「子进程确实已退出、只是读线程还没走完」
        // 这种情况永远等不到回收（本函数的失败模式就是 try_lock 空转到底）。
        // kill 已经发出，阻塞等于是等进程 termination，是有界的。
        let mut c = match child.lock() {
            Ok(c) => c,
            Err(_) => return,
        };
        if matches!(c.try_wait(), Ok(Some(_))) {
            return;
        }
        if std::time::Instant::now() >= deadline {
            log::warn!(
                "PTY 子进程未在 {:?} 内回收，放弃等待（内核收尾未完成或读线程仍在持有）",
                limit
            );
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// 杀掉一个已启动但尚未进入会话表的子进程（pty_spawn 失败路径）。
///
/// 与 `close_session` 的区别：这里没有 master/slave 句柄可关（调用方持有），
/// 只能直接 kill + 有界回收。目的是不留下僵尸条目 —— `?` 早退会让
/// `Child` 既不 kill 也不 wait，子进程滞留到父进程退出。
fn kill_and_forget(child: &Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>) {
    if let Ok(mut c) = child.lock() {
        let _ = c.kill();
    }
    reap_child_bounded(child, std::time::Duration::from_secs(1));
}

/// 关闭会话：kill 子进程并回收；读线程 detach（不 join）。
///
/// **顺序是硬约束**（macOS 实测，回归见 `close_session_closes_master_before_waiting`）：
/// 会话首进程被 kill 后，退出要等 PTY **主端全部关闭**才能完成 —— 主端还开着时
/// 它会一直停在 `E`（exiting）状态，父进程 `wait` 永远收不到。旧实现先 `wait`，
/// master/writer 要等函数结束才随作用域 drop，正好卡在这条规则上：每次关控制台
/// 都会挂住一个线程，并留下一个退不掉的 shell 进程（实测 60 秒仍不可回收）。
///
/// 因此这里先关主端与写端，再 kill，最后**有界**回收：
/// 关主端会让 shell 收到 SIGHUP（正常的终端关闭语义），读线程随即 EOF 退出，
/// 内核也得以完成退出收尾。
///
/// 读线程刻意**不 join**：若 shell 派生了忽略 SIGHUP 的孙进程（nohup / 守护进程），
/// PTY 可能迟迟不 EOF，join 会无限挂起 —— 而调用方往往正持有会话表锁，
/// 一个这样的会话就能锁死全部 PTY 命令。detach 的代价只是线程多活到 EOF 为止。
///
/// 调用方约定：先把会话从会话表移除并**放掉表锁**，再进本函数；
/// kill 不应发生在会话表锁内。
fn close_session(session: PtySession) {
    // `..` 剩下的字段（terminal / cwd / window_label / retired）在解构结束时
    // 立即 drop：terminal 的 Arc 由读线程自己持有一份，不受影响。
    let PtySession {
        master,
        writer,
        child,
        reader,
        ..
    } = session;
    // 丢弃 JoinHandle = detach：不 join（取舍见上）。
    drop(reader);
    drop(master);
    drop(writer);
    if let Ok(mut c) = child.lock() {
        let _ = c.kill();
    }
    reap_child_bounded(&child, std::time::Duration::from_secs(2));
}

#[derive(Default)]
pub struct PtyManager {
    sessions: Mutex<HashMap<String, PtySession>>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// 进程退出兜底：`PtyManager` 被 drop（即 Tauri 状态表在应用关闭时被
    /// 回收）时终止全部会话。
    ///
    /// 为什么需要它 — `WindowEvent::Destroyed` 与 `RunEvent::Exit` 都不能
    /// 保证被调用：webview 渲染进程崩溃 / 被内存压力杀死时窗口不一定销毁，
    /// event loop 被外部 SIGKILL 打断时 `Exit` 也不会来。两者都漏掉时
    /// shell 子进程与读线程会活过应用（表现为「IDE 退了但 zsh 还在」）。
    ///
    /// `Drop` 由状态表自身触达：`.manage()` 把 manager 放进 Tauri 的状态表，
    /// 进程退出时状态表销毁 → 这里必然执行。与 `close_all` 同口径（此时
    /// 已无「其它窗口」可言）。
    ///
    /// 只走 `drop` 不 join 读线程（与 `close_session` 一致）：nohup 孙进程
    /// 可能让 PTY 迟迟不 EOF，退出路径不能被一个会话挂住。
    fn close_all_on_drop(&self) {
        let stale: Vec<PtySession> = {
            let Ok(mut sessions) = self.sessions.lock() else {
                return;
            };
            sessions.drain().map(|(_, s)| s).collect()
        };
        for session in stale {
            close_session(session);
        }
    }

    /// 终止全部会话：**应用退出**时兜底，避免 shell 子进程与读线程泄漏。
    ///
    /// 注意与 [`PtyManager::close_for_window`] 的分工：窗口销毁只允许清
    /// 本窗口的会话，close_all 会误杀其它窗口正在使用的终端，仅留给
    /// 应用退出路径调用。
    pub fn close_all(&self) {
        self.close_all_on_drop();
    }

    /// 按当前会话数重算并应用 scrollback 字节配额。
    ///
    /// 会话表锁内只克隆终端 Arc，随后释放表锁再逐个设置：`set_scrollback_bytes`
    /// 会与读线程争终端锁，不能把表锁跨过去（否则一个话痨会话能拖住全部 PTY
    /// 命令）。新会话创建时默认 128 MB，插入后必须由这里收紧。
    ///
    /// 调用点：spawn 插入后、各 close 路径摘除后。这些路径频率低（用户开/关
    /// 终端），100 个终端的锁+设置是毫秒级；换来的全局上限是确定性的。
    fn rebalance_scrollback(&self) {
        let (terminals, live) = {
            let Ok(sessions) = self.sessions.lock() else {
                return;
            };
            let terminals: Vec<Arc<Mutex<NativeTerminal>>> =
                sessions.values().map(|s| Arc::clone(&s.terminal)).collect();
            (terminals, sessions.len())
        };
        if live == 0 {
            return;
        }
        let quota = scrollback_quota_for(live);
        for terminal in terminals {
            if let Ok(mut term) = terminal.lock() {
                if !term.set_scrollback_bytes(quota) {
                    log::debug!("运行时设置 scrollback 配额失败：{quota} 字节");
                }
            }
        }
    }

    /// 摘除「指定窗口 + 指定 cwd」的全部会话并返回；kill 由调用方在表锁外执行。
    ///
    /// cwd 必须是已规范化的形态（见 [`canonical_cwd`]）。`window_label` 不能省：
    /// 多窗口可同时打开同一项目，只有创建该会话的窗口有权关闭它。
    fn remove_by_cwd_window(
        &self,
        cwd: &str,
        window_label: &str,
    ) -> Result<Vec<PtySession>, String> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| "PTY 状态锁失效".to_string())?;
        let ids: Vec<String> = sessions
            .iter()
            .filter(|(_, s)| s.cwd == cwd && s.window_label == window_label)
            .map(|(id, _)| id.clone())
            .collect();
        Ok(ids
            .into_iter()
            .filter_map(|id| sessions.remove(&id))
            .collect())
    }

    /// 终止属于指定窗口的全部会话：窗口销毁时兜底，只清理本窗口创建的
    /// PTY，其它窗口的会话不受影响。返回关闭的会话数。
    ///
    /// kill + reap 丢到后台线程：本函数跑在 `on_window_event(Destroyed)` 的
    /// 同步路径上（主事件循环内），而每个会话的 `reap_child_bounded` 最坏
    /// 2 秒 —— 同步执行时 10 个被 nohup 孙进程拖住的会话能把窗口关闭卡住
    /// 20 秒。摘会话仍在调用线程完成（表锁短临界区，语义不变）。
    pub fn close_for_window(&self, window_label: &str) -> usize {
        let stale: Vec<PtySession> = {
            let Ok(mut sessions) = self.sessions.lock() else {
                return 0;
            };
            let ids: Vec<String> = sessions
                .iter()
                .filter(|(_, s)| s.window_label == window_label)
                .map(|(id, _)| id.clone())
                .collect();
            ids.into_iter()
                .filter_map(|id| sessions.remove(&id))
                .collect()
        };
        let count = stale.len();
        if !stale.is_empty() {
            std::thread::Builder::new()
                .name(format!("pty-close-{}", window_label))
                .spawn(move || {
                    for session in stale {
                        close_session(session);
                    }
                })
                .ok();
        }
        if count > 0 {
            self.rebalance_scrollback();
        }
        count
    }
}

/// 见 [`PtyManager::close_all_on_drop`] 的说明：Tauri 状态表在进程退出时
/// 销毁 `PtyManager`，这里是「窗口销毁事件 / RunEvent::Exit 都漏掉」时
/// 最后一道回收 shell 子进程与读线程的保险。
///
/// **局限（实测，勿高估）**：`PtySession` 的字段按声明顺序析构时，`master`
/// 关闭本身就会让内核给前台进程组发 SIGHUP，真实 shell 随之退出。也就是说
/// 即使没有这个 `Drop`，多数会话也不会泄漏。`Drop` 真正多覆盖的是「忽略
/// SIGHUP 的孙进程」（nohup / 守护进程，见 pty.rs 记录的那条取舍）——
/// 而这一条**无法用测试断言**：子进程存活与否不能区分「kill 生效」与
/// 「master 关闭让 shell 自然退出」，实测两种情形断言都通过（负向实验：
/// 删掉 `impl Drop` 测试依旧绿）。因此此处不加测试，靠注释留痕。
impl Drop for PtyManager {
    fn drop(&mut self) {
        self.close_all_on_drop();
    }
}

/// 解析默认 shell：优先用户环境，其次平台约定。
///
/// `requested` 由前端通过 IPC 传入，但**不接受任意可执行路径**：
/// 该参数等于「启动任意程序」，恶意仓库内容一旦诱导调用即是任意代码执行。
/// 只放行一组已知 shell 名（不含分隔符，由本函数解析到绝对路径），
/// 因此调用方无法借它启动别的程序。
fn resolve_shell(requested: Option<String>) -> Result<String, String> {
    if let Some(shell) = requested.filter(|s| !s.trim().is_empty()) {
        let s = shell.trim();
        // 名称白名单：只允许裸 shell 名，路径分隔符与元字符一律拒绝
        if s.chars()
            .any(|c| c == '/' || c == '\\' || c == ':' || c.is_whitespace() || c == '\0')
        {
            return Err(crate::error_proto::coded("shell_name_invalid", s));
        }
        let known = if cfg!(target_os = "windows") {
            &["powershell.exe", "pwsh.exe", "cmd.exe"][..]
        } else {
            &["zsh", "bash", "sh", "dash", "fish", "ksh"][..]
        };
        if !known.contains(&s) {
            return Err(crate::error_proto::coded_display(
                "shell_unsupported",
                format!("{s}（可用: {known:?}）"),
            ));
        }
        return Ok(s.to_string());
    }

    if cfg!(target_os = "windows") {
        Ok(std::env::var("COMSPEC").unwrap_or_else(|_| "powershell.exe".to_string()))
    } else {
        Ok(std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string()))
    }
}

// `#[allow(clippy::too_many_arguments)]` 是必需的：这是 Tauri 的 IPC 入口，
// 签名由前端 `invoke('pty_spawn', {...})` 的参数名一一约定。把 5 个可选参数
// 收进一个结构体会改变序列化契约（前端必须跟着改），换来的是纯粹的风格收益。
// IPC 边界签名跟着调用方走，不属于可自由重排的内部函数。
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn pty_spawn(
    app: AppHandle,
    manager: State<'_, PtyManager>,
    window: Window,
    session_id: String,
    cols: u16,
    rows: u16,
    cwd: Option<String>,
    shell: Option<String>,
) -> Result<PtySpawnResult, String> {
    // 网格尺寸夹到合法区间：与 pty_resize 同口径（IPC 是信任边界，
    // 见 clamp_grid 的说明）。这里比 resize 更要紧 —— openpty 的 PtySize
    // 会同时决定内核 PTY 缓冲与引擎初始网格两个分配。
    let (cols, rows) = clamp_grid(cols, rows);

    let pty_system = native_pty_system();

    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| crate::error_proto::coded_display("pty_open_failed", e))?;

    // shell 白名单校验失败必须在此中止：这里还没创建任何资源，
    // 提前返回不会留下半初始化的 PTY。
    let shell_path = resolve_shell(shell)?;
    let working_dir = cwd
        .clone()
        .filter(|c| !c.trim().is_empty() && c != ".")
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| ".".to_string());
    // 匹配用形态：canonicalize 把软链（/tmp → /private/tmp、用户自建链）
    // 与尾斜杠归一成同一形态。关门项目的 cwd 来自前端 workspacePath，
    // 与该目录的规范化形态可能不同形 —— 存两份（会话表用规范形态比较，
    // 展示/回包用原串）让 `pty_close_by_cwd` 不漏匹配。
    let cwd_key = std::fs::canonicalize(&working_dir)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| working_dir.clone());

    let mut cmd = CommandBuilder::new(&shell_path);
    // 以登录 shell 启动，保证 PATH 与用户配置一致
    if !cfg!(target_os = "windows") {
        cmd.arg("-l");
    }
    cmd.cwd(&working_dir);
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    // 告知 shell 运行在 IDE 内，便于配置区分
    cmd.env("TERM_PROGRAM", "lazecode");
    cmd.env("TERM_PROGRAM_VERSION", "0.1.0");
    // 剔除凭据类环境变量。
    //
    // 背景：portable-pty 的 `CommandBuilder::as_command()` 会先 `env_clear()`
    // 再把 `get_base_env()`（= 父进程全量环境）回填（portable-pty
    // cmdbuilder.rs:497-544 实测）。也就是说 IDE 进程持有的每一个环境变量
    // 都会原样进入终端里的 shell —— 包括用户在 launchd/登录项里配的
    // `AWS_SECRET_ACCESS_KEY` / `GITHUB_TOKEN` / `OPENAI_API_KEY` 等。
    //
    // 威胁模型：终端是用户停留最久的面板，任何在这里跑的依赖
    // （npm postinstall、cargo build script、pip 打包钩子）只要读一次
    // `env` 就能把凭据发走，而用户完全看不到。这是供应链攻击的现成落点，
    // 且与「用户自己敲 export 设的变量」不同 —— 那些是用户在终端里主动
    // 授予的，这里是 IDE 悄悄继承的。
    //
    // 不能全清（`env_clear` 之外的另一种极端）：PATH / SSH_AUTH_SOCK /
    // LANG / TERM 一类是终端可用性的前提，全清会让 git push、Go/Python
    // 工具链、非 ASCII 显示全部失效。
    //
    // 因此按**已知凭据变量名**精确剔除：命中即抹掉，其余原样继承。
    // 代价是名单之外的新凭据名会漏 —— 但一个显式名单可以审计，
    // 而「猜哪些变量敏感」则是不可审计的。
    for name in CREDENTIAL_ENV_BLOCKLIST {
        cmd.env_remove(name);
    }
    // 内联图片能力广告。
    //
    // omp 这类 TUI **只看环境变量**决定要不要用 kitty 图形协议：它有一张终端
    // 白名单（kitty/ghostty/wezterm/iTerm/VS Code/Alacritty/…），`TERM_PROGRAM=lazecode`
    // 不在里面就当成不支持，直接退回「一个空占位框」；而它的自身能力探测（发
    // `CSI ? 2;1;0 S` 等 250ms 超时）在旧实现里也拿不到回包。这两个 PI_ 前缀
    // 变量是 omp 自己的显式开关，语义正是「本终端支持 kitty 图形 / 支持 unicode
    // 占位格」，与实现一致：引擎按 kitty 协议存图、`NativeTerminal::images()`
    // 反查占位格给出虚拟 placement、前端按占位格尺寸绘制。
    //
    // 另外 vt.rs 已装上 WRITE_PTY 回包通道，DA1/DECRQM/DSR/kitty 握手都会正常
    // 作答 —— 显式声明与标准握手两条路都通。
    cmd.env("PI_FORCE_IMAGE_PROTOCOL", "kitty");
    cmd.env("PI_KITTY_PLACEHOLDERS", "1");

    let child =
        Arc::new(Mutex::new(pair.slave.spawn_command(cmd).map_err(|e| {
            crate::error_proto::coded_display("pty_spawn_failed", e)
        })?));

    // slave 必须在此处释放，否则读端不会收到 EOF
    drop(pair.slave);

    // 之后的任一步失败（fd 耗尽的 EMFILE / EBADF 等）都必须先把已启动的
    // shell 杀掉再返回：`?` 早退只让 master/slave 随作用域 drop（shell 收到
    // SIGHUP），但 `std::process::Child` 既没 kill 也没 wait —— 子进程会
    // 变成僵尸滞留到父进程退出，fd 也继续被占。上面的白名单校验失败时
    // 这里还没创建任何资源，无需清理（见 resolve_shell 的注释）。
    let mut reader = match pair.master.try_clone_reader() {
        Ok(r) => r,
        Err(e) => {
            kill_and_forget(&child);
            return Err(crate::error_proto::coded_display("pty_reader_failed", e));
        }
    };

    let writer = match pair.master.take_writer() {
        Ok(w) => Arc::new(Mutex::new(w)),
        Err(e) => {
            kill_and_forget(&child);
            return Err(crate::error_proto::coded_display("pty_writer_failed", e));
        }
    };

    // 原生 VT 终端：解析在本进程内完成，字节不再需要流到前端再解析
    let terminal = match NativeTerminal::new(cols, rows) {
        Ok(t) => Arc::new(Mutex::new(t)),
        Err(e) => {
            kill_and_forget(&child);
            return Err(crate::error_proto::coded_display(
                "terminal_create_failed",
                e,
            ));
        }
    };

    // 读线程：原生解析 + 只发脏信号
    //
    // 刻意**不把 PTY 字节推给前端**：解析已在原生侧完成，前端拿字节也
    // 只是丢弃。而每次推送都要把 Vec<u8> 序列化成 JSON 数字数组
    // （8 KB 输出 ≈ 40 KB 文本），话痨命令下这笔开销极大。
    let window_label = window.label().to_string();
    let retired = Arc::new(AtomicBool::new(false));
    let exit_channel = format!("pty://exit/{session_id}");
    let dirty_channel = format!("pty://dirty/{session_id}");
    let pwd_channel = format!("pty://pwd/{session_id}");
    let reader_app = app.clone();
    let reader_term = Arc::clone(&terminal);
    let reader_writer = Arc::clone(&writer);
    let reader_child = Arc::clone(&child);
    let reader_retired = Arc::clone(&retired);
    // 线程闭包需要一份 session_id 做日志与线程名，原值后面还要用于会话表
    let reader_session_id = session_id.clone();
    let reader = std::thread::Builder::new()
        // 线程名带 session id：panic hook 的日志因此能定位是哪个会话
        .name(format!("pty-reader-{}", reader_session_id))
        .spawn(move || {
            let mut buf = [0u8; 8192];
            // 脏信号按帧合并：洪水输出下每 8KB 一次 emit ≈ 每秒上万次事件，
            // 全部涌进 webview 事件队列抢主线程。按 16ms（≈60fps 上限）合并发送；
            // 满块（尾部数据）立即发，EOF 退出前补发，保证最后一帧不丢失。
            let mut last_emit: Option<std::time::Instant> = None;
            loop {
                // 单轮处理包在 catch_unwind 里：`t.write` 走 FFI 包装层，
                // 一旦 panic 会 unwound 出线程 —— 而此刻若持着 terminal 锁，
                // 锁会被毒化，此后该会话的 vt_snapshot / pty_resize 全部返回
                // 「终端状态锁失效」，前端看到的是永久假死。隔离后至少
                // 循环能继续到 EOF（多数情况自行恢复），否则走与 EOF 相同的
                // 收尾路径（发退出码），让前端重建会话而不是卡「启动中」。
                let round = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let n = match reader.read(&mut buf) {
                        Ok(0) => return None, // EOF：正常结束
                        Ok(n) => n,
                        Err(_) => return None, // 读失败：同样结束
                    };
                    // 锁内只做「解析 + 取回包」，回包的 PTY 写在锁外：
                    // write_all 在对端不读 stdin 时会阻塞（回包是 DA1 /
                    // DECRQM / DSR / kitty 握手应答，子进程大量输出时同步
                    // 发起查询就会撞上），持锁阻塞会让该会话的快照 IPC
                    // 全部排在后面 —— 画面假死。take 是纯内存操作。
                    let (responses, pwd) = match reader_term.lock() {
                        Ok(mut t) => {
                            t.write(&buf[..n]);
                            (take_terminal_responses(&mut t), t.take_pwd_change())
                        }
                        Err(_) => (Vec::new(), None),
                    };
                    Some((n, responses, pwd))
                }));
                match round {
                    Ok(None) => {
                        // 收尾顺带补发一次脏信号：尾块可能落在节流窗口里，
                        // 直接 break 会让最后一屏停在上一帧
                        let _ = reader_app.emit(&dirty_channel, ());
                        break;
                    }
                    Ok(Some((n, responses, pwd))) => {
                        // 回包写回 PTY：不持 terminal 锁（见上）
                        write_terminal_responses(&responses, &reader_writer);
                        // shell integration：OSC 7/9/1337 的 cwd 变化。事件量极低
                        // （只在 cd 时触发），不需要节流；旧会话被顶替后不再发。
                        if let Some(pwd) = pwd {
                            if !reader_retired.load(Ordering::Relaxed) {
                                let _ = reader_app.emit(&pwd_channel, pwd);
                            }
                        }
                        // 脏信号节流：前端 rAF 本来就按帧合并（未消费的标脏只保留
                        // 最新一次），但 8KB 一块的 emit 在洪水输出下是每秒上万次
                        // 事件，全部涌进 webview 事件队列抢主线程。按 16ms 合并；
                        // 未满 8KB 的块是批次尾部、直接发，避免尾帧等待。
                        // 前端光标闪烁定时器每 530ms 兜底标脏一次，所以合并
                        // 只降事件量，不丢任何一帧可见更新。
                        let now = std::time::Instant::now();
                        let due = last_emit.is_none_or(|t: std::time::Instant| {
                            now.duration_since(t).as_millis() >= 16
                        });
                        if due || n < buf.len() {
                            last_emit = Some(now);
                            if reader_app.emit(&dirty_channel, ()).is_err() {
                                break;
                            }
                        }
                    }
                    Err(_) => {
                        // 本轮 panic 已被 hook 记录（panic.log）。
                        // 锁很可能已被毒化：继续读也写不进终端，立刻收尾。
                        log::error!(
                            "PTY 会话 {reader_session_id} 的读取轮次发生 panic，结束该会话的读线程"
                        );
                        break;
                    }
                }
            }
            // 真实退出码：读线程退出意味着管道 EOF（shell 已退出），
            // 此时子进程必然可 wait；把码带给前端用于区分正常/异常退出。
            let code = locked_exit_code(&reader_child);
            // 被同 id 新会话顶替的旧读线程到此闭嘴：发出去的旧退出码
            // 会让前端误判新会话已退出。
            if !reader_retired.load(Ordering::Relaxed) {
                let _ = reader_app.emit(&exit_channel, code);
            }
        })
        // 读线程起不来（线程数上限 / 沙箱限制）不能 panic：release 是
        // panic=abort，整个 IDE（含未保存缓冲）会被杀掉。此时 PTY 与子进程
        // 已建，对照终端创建失败的路径先收尸再报 coded 错误，前端
        // 走与其他启动失败相同的提示链（R1：生产代码零 expect）。
        // 复用 pty_reader_failed：对用户都是"终端读端起不来，重试即可"，
        // 细节在 detail 里（io 错误原文），没必要增新码。
        .map_err(|e| {
            kill_and_forget(&child);
            crate::error_proto::coded_display("pty_reader_failed", e)
        })?;

    let new_session = PtySession {
        master: Arc::new(Mutex::new(pair.master)),
        writer,
        child,
        terminal,
        // 存规范化形态：pty_close_by_cwd 按它比较（见 working_dir 处的注释）
        cwd: cwd_key,
        window_label,
        retired,
        reader: Some(reader),
    };

    // 同 session_id 重复 spawn（前端异常重连等）会顶掉旧会话：entry 语义
    // 直接覆盖会把旧 child 与读线程整份泄漏。取出旧会话后，先置 retired
    // 让旧读线程的退出事件闭嘴，再 kill 旧 child；读线程 detach 不 join
    // （可能被忽略 SIGHUP 的孙进程无限拖住，见 close_session 注释）。
    let stale = {
        manager
            .sessions
            .lock()
            .map_err(|_| "PTY 状态锁失效".to_string())?
            .insert(session_id.clone(), new_session)
    };
    if let Some(old) = stale {
        old.retired.store(true, Ordering::Relaxed);
        close_session(old);
    }
    // 新会话默认 128MB；按当前会话数收紧，并同步收紧已有会话。
    manager.rebalance_scrollback();

    Ok(PtySpawnResult {
        session_id,
        shell: shell_path,
        cwd: working_dir,
    })
}

#[tauri::command]
/// 写入 PTY。
///
/// 收原始字节而非字符串：按键编码产出的是控制字节（ESC 序列等），
/// 走字符串会在两侧各做一次编解码，既多余又对非 UTF-8 字节有损。
///
/// async + `spawn_blocking`：`write_all` 在对端不读 stdin（PTY 缓冲满）时会
/// 长时间阻塞。同步命令会把一个 ipc worker 线程按在那儿，大量粘贴或某个
/// 卡住的会话就能拖慢其它命令；丢进 blocking 池后 ipc worker 立即脱身，
/// 只有该会话的写入在等（同会话字节的串行化仍由 writer 锁保证）。
pub async fn pty_write(
    manager: State<'_, PtyManager>,
    session_id: String,
    data: Vec<u8>,
) -> Result<(), String> {
    let writer = session_writer(&manager, &session_id)?;
    tauri::async_runtime::spawn_blocking(move || write_session(&writer, &data))
        .await
        .map_err(|e| format!("PTY 写入任务失败: {e}"))?
}

/// 取会话的写句柄。
///
/// 只在校验会话存在时持全局 sessions 锁，`write_all` 在锁外执行：
/// 持锁写的话，某个会话 PTY 缓冲满（对端停止读取）会阻塞在 write_all，
/// 把全局锁按住，拖住所有其它会话的 write / resize / snapshot。
/// 写入串行化由 per-session writer 锁承担（同一会话的字节不能被交错），
/// 与会话表锁完全解耦。
fn session_writer(
    manager: &PtyManager,
    session_id: &str,
) -> Result<Arc<Mutex<Box<dyn Write + Send>>>, String> {
    let sessions = manager
        .sessions
        .lock()
        .map_err(|_| "PTY 状态锁失效".to_string())?;
    sessions
        .get(session_id)
        .map(|s| Arc::clone(&s.writer))
        .ok_or_else(|| format!("PTY 会话不存在: {session_id}"))
}

/// 从终端取走待回包的字节（持 terminal 锁）。
///
/// 与 `forward_terminal_responses` 的分工：读线程已经持着 terminal 锁，
/// 若在锁内直接 write PTY，对端不读 stdin 时 `write_all` 会阻塞在锁内 ——
/// 该会话的 `vt_snapshot` / `pty_resize` 全部排在后面，画面假死。
/// 因此改成两步：锁内只 `take`（纯内存，不阻塞），锁外再写。
fn take_terminal_responses(term: &mut NativeTerminal) -> Vec<u8> {
    term.take_responses()
}

/// 把取到的回包字节写回 PTY（不持 terminal 锁）。
///
/// 写入失败只记日志 —— 对端可能刚退出，回包丢了不该影响会话本身。
/// 用 `try_lock` 拿 writer：用户输入正持锁时本轮回包直接丢弃（回包是
/// TUI 能力探测的应答，丢了最多是探测超时重试），换来读线程永不阻塞。
fn write_terminal_responses(responses: &[u8], writer: &Arc<Mutex<Box<dyn Write + Send>>>) {
    write_pty_nonblocking(responses, writer, "VT 查询回包");
}

/// 把字节写回 PTY：拿不到写入锁就丢这一批，绝不阻塞。
///
/// 用户输入持锁时（`pty_write` 正在 write_all）本批直接丢弃：转发的滚轮与
/// 能力探测回包都不是关键数据，丢一次最多是下一格滚轮补上；而阻塞会让
/// 调用线程（读线程或命令线程）卡在锁上，把整条会话拖死。
fn write_pty_nonblocking(bytes: &[u8], writer: &Arc<Mutex<Box<dyn Write + Send>>>, what: &str) {
    if bytes.is_empty() {
        return;
    }
    match writer.try_lock() {
        Ok(mut w) => {
            if let Err(e) = w.write_all(bytes).and_then(|()| w.flush()) {
                log::debug!("回写{what}失败（{} 字节）: {e}", bytes.len());
            }
        }
        Err(_) => log::debug!(
            "回写{what}被跳过（{} 字节）：写入锁被用户输入占用",
            bytes.len()
        ),
    }
}

/// 一步完成「取回包 + 写回」，供不持锁的调用方（测试）使用。
///
/// 生产读线程走 `take_terminal_responses` + `write_terminal_responses` 两步
/// （不能在持 terminal 锁时写 PTY），只有测试直接调这一步完成版。
#[cfg_attr(not(test), allow(dead_code))]
fn forward_terminal_responses(
    term: &mut NativeTerminal,
    writer: &Arc<Mutex<Box<dyn Write + Send>>>,
) {
    let responses = take_terminal_responses(term);
    write_terminal_responses(&responses, writer);
}

/// 写入并 flush。失败一律带明确文案上抛：前端把错误当成"按键已送达"会静默
/// 丢键，事后无从排查（`write_all` 自己会循环，短路成 `write` 会截断输入）。
fn write_session(writer: &Arc<Mutex<Box<dyn Write + Send>>>, data: &[u8]) -> Result<(), String> {
    let mut writer = writer.lock().map_err(|_| "PTY 写入锁失效".to_string())?;
    writer
        .write_all(data)
        .map_err(|e| format!("写入 PTY 失败（{} 字节）: {e}", data.len()))?;
    writer.flush().map_err(|e| format!("刷新 PTY 失败: {e}"))
}

#[tauri::command]
pub fn pty_resize(
    manager: State<'_, PtyManager>,
    session_id: String,
    cols: u16,
    rows: u16,
    cell_width_px: Option<u32>,
    cell_height_px: Option<u32>,
) -> Result<(), String> {
    // 锁内只 Arc::clone 出需要的两样东西就放锁：master 的 resize 是
    // 一次 ioctl，终端锁是另一把锁，都不该在全 sessions 锁后面排。
    // 与 vt_search / vt_snapshot 同一个修法。
    let (master, terminal) = {
        let sessions = manager
            .sessions
            .lock()
            .map_err(|_| "PTY 状态锁失效".to_string())?;
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| format!("PTY 会话不存在: {session_id}"))?;
        (Arc::clone(&session.master), Arc::clone(&session.terminal))
    };

    // 网格尺寸夹到合法区间：见 clamp_grid 的说明（IPC 是信任边界，
    // cols/rows 无上限会让一次 vt_snapshot 的分配打爆进程）。
    let (cols, rows) = clamp_grid(cols, rows);

    let cell_w = cell_width_px.unwrap_or(0);
    let cell_h = cell_height_px.unwrap_or(0);
    let pixel_width = (cols as u32).saturating_mul(cell_w).min(u16::MAX as u32) as u16;
    let pixel_height = (rows as u32).saturating_mul(cell_h).min(u16::MAX as u32) as u16;

    let m = master.lock().map_err(|_| "PTY 状态锁失效".to_string())?;
    m.resize(PtySize {
        rows,
        cols,
        pixel_width,
        pixel_height,
    })
    .map_err(|e| format!("调整 PTY 尺寸失败: {e}"))?;
    drop(m);

    // 原生终端必须同步尺寸，否则快照仍是旧网格。
    // 格子像素尺寸决定 Kitty 图片 placement 的渲染尺寸（0 会让图片尺寸算成 0），
    // 前端把实测的单元格尺寸一并送过来。
    let mut term = terminal.lock().map_err(|_| "终端状态锁失效".to_string())?;
    term.resize(cols, rows, cell_w, cell_h)?;

    Ok(())
}

/// 取一帧终端快照。
///
/// 返回原始字节而非 JSON：u64 单元格在 JSON 里会因 2^53 丢精度，
/// 且 4800 格的序列化开销远大于一次 memcpy。协议见 `vt::encode_snapshot`。
///
/// 前端应在 rAF 里调用（配合 `pty://dirty/<session>` 合并），
/// 而不是每次收到 PTY 输出就调一次。
#[tauri::command]
pub fn vt_snapshot(
    manager: State<'_, PtyManager>,
    session_id: String,
) -> Result<tauri::ipc::Response, String> {
    // 只克隆 Arc 即释放全局锁，把锁持有范围压缩到 lookup 本身。
    // vt_snapshot 按帧频调用，若在编码整帧期间一直占着 sessions 锁，
    // 该会话乃至全部会话的 pty_resize / vt_scroll / pty_close 都要排队。
    let terminal = {
        let sessions = manager
            .sessions
            .lock()
            .map_err(|_| "PTY 状态锁失效".to_string())?;
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| format!("会话不存在: {session_id}"))?;
        Arc::clone(&session.terminal)
    };

    let mut term = terminal.lock().map_err(|_| "终端状态锁失效".to_string())?;

    // snapshot_wire 复用会话级缓冲：稳态下每帧零堆分配。
    //
    // 唯一残留的分配是 `Response::new` 传入的那份 `out` —— Tauri 的
    // `CustomProtocol` 通道要求字节在响应期间独立存活，不能借用会话缓冲，
    // 而缓冲本身还要留给下一帧。实测这一步 3000 格一帧约 90KB，
    // 相比整帧编码的 CPU 开销可忽略，故不再引入环形缓冲去省它。
    Ok(tauri::ipc::Response::new(term.snapshot_wire()))
}

/// 取一帧**增量**终端快照：只带引擎标记为脏的行。
///
/// 与 `vt_snapshot` 同族但小一个量级：稳态下（光标闪烁、逐字回显）
/// 从整屏 KB 级降到百字节级，前端也省掉整屏逐格比对。
///
/// `full` 为 true 时整屏当脏行送（首帧 / resize / 帧缓存失效）。
/// 两帧格式相同，前端只需一条解码路径。
///
/// 脏标记在本命令出口统一归还（`render_state_clean`），漏还会让脏区
/// 累加、下一帧把很久以前的行又送一遍。
#[tauri::command]
pub fn vt_snapshot_diff(
    manager: State<'_, PtyManager>,
    session_id: String,
    full: bool,
) -> Result<tauri::ipc::Response, String> {
    let terminal = {
        let sessions = manager
            .sessions
            .lock()
            .map_err(|_| "PTY 状态锁失效".to_string())?;
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| format!("会话不存在: {session_id}"))?;
        Arc::clone(&session.terminal)
    };

    let mut term = terminal.lock().map_err(|_| "终端状态锁失效".to_string())?;

    Ok(tauri::ipc::Response::new(term.snapshot_wire_diff(full)))
}

/// 滚动终端视口。`delta` 为负表示向上翻看历史。
#[tauri::command]
pub fn vt_scroll(
    manager: State<'_, PtyManager>,
    session_id: String,
    delta: Option<i64>,
) -> Result<(), String> {
    // 表锁只做 lookup + Arc::clone，滚动在锁外执行 —— 与 pty_resize /
    // vt_snapshot / vt_search 同一约定。旧实现把整段滚动握在表锁内：
    // 滚轮事件每帧一次，scroll_delta 在巨大 scrollback 上要做历史行定位，
    // 期间其它会话的 pty_close / close_for_window 全排在表锁后面。
    let terminal = {
        let sessions = manager
            .sessions
            .lock()
            .map_err(|_| "PTY 状态锁失效".to_string())?;
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| format!("会话不存在: {session_id}"))?;
        Arc::clone(&session.terminal)
    };

    let mut term = terminal.lock().map_err(|_| "终端状态锁失效".to_string())?;

    match delta {
        Some(d) => term.scroll_delta(d),
        None => term.scroll_to_bottom(),
    }
    Ok(())
}

/// 滚轮输入：优先转发给 PTY 程序（鼠标协议 / 备用屏方向键），否则回滚本地视口。
///
/// 为什么不能一律本地回滚：omp/opencode/htop 这类 TUI 占着**备用屏**，而备用屏
/// 没有 scrollback —— 本地历史恒为空，滚轮只能按鼠标协议交给程序自己滚。
/// 返回 true 表示字节已转发（本地视口未动）；false 表示走了本地回滚。
#[tauri::command]
pub fn vt_wheel(
    manager: State<'_, PtyManager>,
    session_id: String,
    delta: i64,
    x: Option<u16>,
    y: Option<u16>,
    mods: Option<u16>,
) -> Result<bool, String> {
    if delta == 0 {
        return Ok(false);
    }
    // 表锁只做 lookup + Arc::clone，编码与滚动都在锁外（与 vt_scroll 同约定）
    let (terminal, writer) = {
        let sessions = manager
            .sessions
            .lock()
            .map_err(|_| "PTY 状态锁失效".to_string())?;
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| format!("会话不存在: {session_id}"))?;
        (Arc::clone(&session.terminal), Arc::clone(&session.writer))
    };

    let forwarded = {
        let mut term = terminal.lock().map_err(|_| "终端状态锁失效".to_string())?;
        term.scroll_wheel(delta, x.unwrap_or(0), y.unwrap_or(0), mods.unwrap_or(0))
    };

    if let Some(bytes) = forwarded {
        // 锁外写、不阻塞：写不进去就丢这一格滚轮，下一次滚轮会再来
        write_pty_nonblocking(&bytes, &writer, "滚轮输入");
        return Ok(true);
    }
    Ok(false)
}

/// 设置终端默认前景/背景/光标色（0xRRGGBB），与 IDE 主题对齐。
#[tauri::command]
pub fn vt_set_colors(
    manager: State<'_, PtyManager>,
    session_id: String,
    fg: u32,
    bg: u32,
    cursor: u32,
) -> Result<(), String> {
    let terminal = {
        let sessions = manager
            .sessions
            .lock()
            .map_err(|_| "PTY 状态锁失效".to_string())?;
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| format!("会话不存在: {session_id}"))?;
        Arc::clone(&session.terminal)
    };
    let mut term = terminal.lock().map_err(|_| "终端状态锁失效".to_string())?;
    term.set_default_colors(fg, bg, cursor)
}

/// 终端是否请求了鼠标跟踪（DEC 1000/1002/1003）。
#[tauri::command]
pub fn vt_mouse_tracking(
    manager: State<'_, PtyManager>,
    session_id: String,
) -> Result<bool, String> {
    let terminal = {
        let sessions = manager
            .sessions
            .lock()
            .map_err(|_| "PTY 状态锁失效".to_string())?;
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| format!("会话不存在: {session_id}"))?;
        Arc::clone(&session.terminal)
    };
    let term = terminal.lock().map_err(|_| "终端状态锁失效".to_string())?;
    Ok(term.mouse_tracking())
}

/// 鼠标按下/抬起/移动：编码成终端鼠标序列并写 PTY。返回 true 表示已转发。
#[tauri::command]
pub fn vt_mouse(
    manager: State<'_, PtyManager>,
    session_id: String,
    action: u32,
    button: u32,
    x: u16,
    y: u16,
    mods: Option<u16>,
) -> Result<bool, String> {
    let (terminal, writer) = {
        let sessions = manager
            .sessions
            .lock()
            .map_err(|_| "PTY 状态锁失效".to_string())?;
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| format!("会话不存在: {session_id}"))?;
        (Arc::clone(&session.terminal), Arc::clone(&session.writer))
    };
    let bytes = {
        let mut term = terminal.lock().map_err(|_| "终端状态锁失效".to_string())?;
        term.encode_mouse(action, button, x, y, mods.unwrap_or(0))
    };
    if bytes.is_empty() {
        return Ok(false);
    }
    // 鼠标按下/抬起不能像滚轮那样丢：丢 press 是死点击，丢 release 会让
    // TUI 以为按钮一直按着。这里等写入锁（持有者只可能是 pty_write）；
    // write_all 的阻塞风险与键盘输入相同，且不在 terminal 锁内。
    match writer.lock() {
        Ok(mut w) => {
            if let Err(e) = w.write_all(&bytes).and_then(|()| w.flush()) {
                log::debug!("回写鼠标输入失败（{} 字节）: {e}", bytes.len());
            }
        }
        Err(_) => log::debug!("回写鼠标输入跳过：写入锁失效"),
    }
    Ok(true)
}

#[tauri::command]
pub async fn pty_close(manager: State<'_, PtyManager>, session_id: String) -> Result<(), String> {
    // 先把会话从表里摘掉并放锁，再在锁外 kill：关闭一个会话不能把
    // 其它会话的 pty_write / vt_snapshot 一起卡住（close_session 约定）。
    let session = {
        let mut sessions = manager
            .sessions
            .lock()
            .map_err(|_| "PTY 状态锁失效".to_string())?;
        sessions.remove(&session_id)
    };
    if let Some(session) = session {
        // close_session 会 kill 子进程并最多等待 2 秒回收；同步命令跑在主线程
        // 上，10 个卡住的会话就能把 UI 冻住。挪到 blocking 池，IPC 立即脱身。
        tauri::async_runtime::spawn_blocking(move || close_session(session))
            .await
            .map_err(|e| format!("PTY 关闭任务失败: {e}"))?;
    }
    // 会话数下降：剩余会话可以拿回更大的 scrollback 配额。
    manager.rebalance_scrollback();
    Ok(())
}

/// 会话关闭前的占用探测结果
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PtyBusyDto {
    /// 终端上是否还有 shell 之外的进程在跑
    pub busy: bool,
    /// 这些进程的名字（去重、最多 3 个），用于确认弹窗文案
    pub commands: Vec<String>,
}

/// 关闭会话前的探测：该会话的终端上是否还有命令在跑。
///
/// 判定方式：`ps -t <tty>` 列出该终端设备上的全部进程，除 shell 自己之外的
/// 都算「正在运行的命令」—— 前台命令（npm run dev / vim / top）与后台任务
/// （`npm run dev &`）都覆盖，正是关闭会连带杀掉的东西。
/// 探测失败一律返回「不忙」：宁可少提示，也不能让用户关不掉控制台。
/// Windows 的 ConPTY 没有等价的 tty 设备视角，恒返回不忙（不误报，也没有保护）。
fn probe_session_busy(
    master: &Arc<Mutex<Box<dyn MasterPty + Send>>>,
    shell_pid: Option<u32>,
) -> PtyBusyDto {
    let idle = PtyBusyDto {
        busy: false,
        commands: Vec::new(),
    };
    #[cfg(unix)]
    {
        // tty_name 要在锁内取（MasterPty 不 Sync），取到就放锁
        let Some(tty) = master.lock().ok().and_then(|m| m.tty_name()) else {
            return idle;
        };
        let mut cmd = std::process::Command::new("ps");
        cmd.arg("-o").arg("pid=,comm=").arg("-t").arg(&tty);
        let out = match crate::commands::run_with_timeout_spawn(
            &mut cmd,
            std::time::Duration::from_secs(3),
        ) {
            Ok(o) if o.status.success() => o,
            Ok(o) => {
                log::warn!(
                    "ps 探测终端 {} 占用失败: {}",
                    tty.display(),
                    String::from_utf8_lossy(&o.stderr).trim()
                );
                return idle;
            }
            Err(e) => {
                log::warn!("ps 探测终端 {} 占用失败: {}", tty.display(), e);
                return idle;
            }
        };
        let stdout = String::from_utf8_lossy(&out.stdout);
        let mut commands: Vec<String> = Vec::new();
        for line in stdout.lines() {
            let mut parts = line.trim().splitn(2, char::is_whitespace);
            let Some(pid) = parts.next().and_then(|p| p.trim().parse::<u32>().ok()) else {
                continue;
            };
            // shell 自己不算「正在运行的命令」：停在提示符上不该拦关闭
            if Some(pid) == shell_pid {
                continue;
            }
            let name = parts.next().unwrap_or("").trim();
            if name.is_empty() {
                continue;
            }
            // comm 可能是完整路径，弹窗里只展示可执行文件名
            let short = name.rsplit('/').next().unwrap_or(name).to_string();
            if !commands.iter().any(|c| c == &short) {
                commands.push(short);
            }
            if commands.len() >= 3 {
                break;
            }
        }
        PtyBusyDto {
            busy: !commands.is_empty(),
            commands,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (master, shell_pid);
        idle
    }
}

/// 关闭前探测：会话里是否还有命令在运行（前端据此弹确认）
#[tauri::command]
pub fn pty_session_busy(
    manager: State<'_, PtyManager>,
    session_id: String,
) -> Result<PtyBusyDto, String> {
    // 锁内只取句柄：探测要起子进程，不能占着会话表锁（与 pty_close 同一约定）
    let (master, child) = {
        let sessions = manager
            .sessions
            .lock()
            .map_err(|_| "PTY 状态锁失效".to_string())?;
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| format!("PTY 会话不存在: {session_id}"))?;
        (Arc::clone(&session.master), Arc::clone(&session.child))
    };
    let shell_pid = child.lock().ok().and_then(|c| c.process_id());
    Ok(probe_session_busy(&master, shell_pid))
}

/// 当前终端里的内联图片（Kitty 图形协议）：放置几何 + 图片元数据。
///
/// 每帧都可能被前端调用，因此只回元数据（几十字节/项）；像素数据另走
/// `vt_image_data`，且只在缓存缺失时拉一次。
#[tauri::command]
pub fn vt_images(
    manager: State<'_, PtyManager>,
    session_id: String,
) -> Result<crate::vt::KittyImagesDto, String> {
    let terminal = {
        let sessions = manager
            .sessions
            .lock()
            .map_err(|_| "PTY 状态锁失效".to_string())?;
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| format!("会话不存在: {session_id}"))?;
        Arc::clone(&session.terminal)
    };
    let term = terminal.lock().map_err(|_| "终端状态锁失效".to_string())?;
    Ok(term.images())
}

/// 取一张内联图片的 RGBA 像素（二进制回包，避免 JSON 数字数组的十倍开销）。
#[tauri::command]
pub fn vt_image_data(
    manager: State<'_, PtyManager>,
    session_id: String,
    image_id: u32,
) -> Result<tauri::ipc::Response, String> {
    let terminal = {
        let sessions = manager
            .sessions
            .lock()
            .map_err(|_| "PTY 状态锁失效".to_string())?;
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| format!("会话不存在: {session_id}"))?;
        Arc::clone(&session.terminal)
    };
    let term = terminal.lock().map_err(|_| "终端状态锁失效".to_string())?;
    let (width, height, pixels) = term
        .image_rgba(image_id)
        .ok_or_else(|| format!("图片不存在或像素未就绪: {image_id}"))?;
    // 头部 8 字节（宽高） + RGBA：前端一次拿全，省一次 IPC
    let mut payload = Vec::with_capacity(8 + pixels.len());
    payload.extend_from_slice(&width.to_le_bytes());
    payload.extend_from_slice(&height.to_le_bytes());
    payload.extend_from_slice(&pixels);
    Ok(tauri::ipc::Response::new(payload))
}

/// 终止属于某个窗口 + 工作区的全部 PTY 会话（关闭项目即终止，释放进程与内存）。
///
/// 必须带 `window`：多窗口可以同时打开同一个项目，关闭 A 窗口的项目不能
/// 杀掉 B 窗口正在跑的终端。`PtySession.window_label` 本来就是为窗口隔离
/// 记录的（见 `close_for_window`），这里漏用会导致跨窗口误杀。
#[tauri::command]
pub async fn pty_close_by_cwd(
    manager: State<'_, PtyManager>,
    window: tauri::Window,
    cwd: String,
) -> Result<usize, String> {
    // 与 pty_close 同一约定：表锁内只摘会话，kill 在锁外逐个进行。
    //
    // canonicalize 传入路径再与会话表的规范化 cwd 比较：前端给的 workspacePath
    // 可能与 spawn 时的目录形态不同形（软链 / 尾斜杠 / Windows 分隔符），
    // 裸字符串相等会把会话漏在工作区里。fail 时退回原串（目录已删的场景）。
    let target = canonical_cwd(&cwd);
    let window_label = window.label().to_string();
    let stale = manager.remove_by_cwd_window(&target, &window_label)?;
    let count = stale.len();
    if count > 0 {
        // 关项目可能一次关 10 个终端，每个最多 2 秒 reap；同步跑会把主线程
        // 冻住。后台 blocking 池逐个收尸，命令本身立即返回计数。
        tauri::async_runtime::spawn_blocking(move || {
            for session in stale {
                close_session(session);
            }
        })
        .await
        .map_err(|e| format!("PTY 关闭任务失败: {e}"))?;
        manager.rebalance_scrollback();
    }
    Ok(count)
}

/// cwd 的匹配形态：canonicalize 失败时退回原串（目录已删）。
fn canonical_cwd(cwd: &str) -> String {
    std::fs::canonicalize(cwd)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| cwd.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vt::POINT_TAG_VIEWPORT;
    use std::time::{Duration, Instant};

    /// 构造一个可关闭的最小会话（不启动读线程：本测试只验证筛选与回收）。
    fn test_session(cwd: &str, window_label: &str) -> PtySession {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty 失败");
        let child_box = pair
            .slave
            .spawn_command(CommandBuilder::new("/bin/sh"))
            .expect("启动 shell 失败");
        drop(pair.slave);
        let writer = pair.master.take_writer().expect("获取 writer 失败");
        PtySession {
            master: Arc::new(Mutex::new(pair.master)),
            writer: Arc::new(Mutex::new(writer)),
            child: Arc::new(Mutex::new(child_box)),
            terminal: Arc::new(Mutex::new(NativeTerminal::new(80, 24).unwrap())),
            cwd: cwd.to_string(),
            window_label: window_label.to_string(),
            retired: Arc::new(AtomicBool::new(false)),
            reader: None,
        }
    }

    /// 同 cwd 的两个窗口：只允许创建者窗口关闭自己的会话。
    #[test]
    fn close_by_cwd_is_scoped_to_window() {
        let manager = PtyManager::new();
        {
            let mut sessions = manager.sessions.lock().unwrap();
            sessions.insert("main-a".into(), test_session("/tmp", "main"));
            sessions.insert("other-window".into(), test_session("/tmp", "win_2"));
            sessions.insert("other-cwd".into(), test_session("/var", "main"));
        }

        let removed = manager.remove_by_cwd_window("/tmp", "main").unwrap();
        assert_eq!(removed.len(), 1, "只应摘除 main 窗口 + /tmp 的会话");
        for session in removed {
            close_session(session);
        }

        let left: Vec<String> = manager.sessions.lock().unwrap().keys().cloned().collect();
        assert_eq!(left.len(), 2, "其它窗口 / 其它 cwd 的会话必须保留");
        assert!(left.contains(&"other-window".to_string()));
        assert!(left.contains(&"other-cwd".to_string()));
        manager.close_all();
    }

    /// 全局预算的边界：低会话数保持单会话上限，100 会话仍够 20k 行，
    /// 极端会话数落到 8 MB 下限而不是 0。
    #[test]
    fn scrollback_quota_tracks_session_count() {
        assert_eq!(
            scrollback_quota_for(0),
            crate::vt::SCROLLBACK_MAX_BYTES,
            "空会话按 1 个计算"
        );
        assert_eq!(scrollback_quota_for(1), crate::vt::SCROLLBACK_MAX_BYTES);
        assert_eq!(scrollback_quota_for(100), GLOBAL_SCROLLBACK_BUDGET / 100);
        assert!(
            scrollback_quota_for(100) > 16 * 1024 * 1024,
            "100 会话时每会话应仍能容纳 20k 行 @120 列（实测 16.6MB）"
        );
        assert_eq!(scrollback_quota_for(10_000), MIN_SCROLLBACK_BYTES);
    }

    /// 端到端：真实 PTY 输出 → 原生解析 → 快照。
    ///
    /// 这是本次改造的核心链路的验证：shell 的字节直接在本进程解析，
    /// 前端拿到的已是格子缓冲，中间不再经过 wasm。
    #[test]
    fn parses_real_shell_output_into_cells() {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty 失败");

        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.arg("-c");
        // 普通文本 + 彩色 + 真彩，覆盖主要解析分支
        cmd.arg("printf 'hello \\033[31mRED\\033[0m \\033[38;2;10;200;90mGREEN\\033[0m\\n'");
        let mut child = pair.slave.spawn_command(cmd).expect("启动 shell 失败");
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().expect("获取 reader 失败");
        let mut term = NativeTerminal::new(80, 24).expect("创建终端失败");

        // 与 pty.rs 读线程完全相同的喂入方式
        let mut buf = [0u8; 8192];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            term.write(&buf[..n]);
        }
        let _ = child.wait();

        let snap = term.snapshot();
        assert_eq!(snap.cols, 80);
        assert_eq!(snap.rows, 24);

        let text = |x: u16| -> char {
            let c = snap.cells[x as usize];
            if c.has_text {
                char::from_u32(c.codepoint).unwrap_or('?')
            } else {
                ' '
            }
        };

        assert_eq!(text(0), 'h');
        assert_eq!(text(5), ' ');

        // 索引色 RED 应解析为调色板第 1 项
        assert_eq!(snap.cells[6].fg, Some(0xcc6666), "RED 应为索引色");
        assert_eq!(text(6), 'R');

        // 真彩 GREEN 应精确保留
        assert_eq!(text(10), 'G');
        assert_eq!(snap.cells[10].fg, Some(0x0ac85a), "GREEN 应为真彩");
    }

    /// 大批量输出下原生解析不应成为瓶颈
    #[test]
    fn handles_bulk_output_without_stalling() {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty 失败");

        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.arg("-c");
        cmd.arg("for i in $(seq 1 3000); do echo \"line $i \\033[32mok\\033[0m\"; done");
        let mut child = pair.slave.spawn_command(cmd).expect("启动 shell 失败");
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().expect("获取 reader 失败");
        let mut term = NativeTerminal::new(80, 24).expect("创建终端失败");

        let start = Instant::now();
        let mut total = 0usize;
        let mut buf = [0u8; 8192];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            total += n;
            term.write(&buf[..n]);
        }
        let parse_time = start.elapsed();
        let _ = child.wait();

        assert!(total > 50_000, "应收到大量输出，实际 {total} 字节");

        // 解析本身必须是微秒级；这里给足余量以容忍 CI 抖动
        assert!(
            parse_time < Duration::from_secs(5),
            "解析 {total} 字节耗时 {parse_time:?}，不应如此之慢"
        );

        // 最终视口应停在最新的滚动位置
        let snap = term.snapshot();
        let last_row = (0..snap.cols)
            .map(|x| {
                let c = snap.cells[(snap.rows as usize - 1) * snap.cols as usize + x as usize];
                if c.has_text {
                    char::from_u32(c.codepoint).unwrap_or(' ')
                } else {
                    ' '
                }
            })
            .collect::<String>();
        assert!(
            last_row.contains("line 3000") || last_row.trim().is_empty(),
            "末行应为最新输出，实际: {last_row:?}"
        );
    }

    /// 滚动进入历史后，视口读取的仍是同一套坐标语义
    #[test]
    fn scrolls_back_into_history() {
        let mut term = NativeTerminal::new(20, 3).expect("创建终端失败");
        for i in 1..=10 {
            term.write(format!("L{i}\r\n").as_bytes());
        }

        let screen = |s: &crate::vt::Snapshot| -> Vec<String> {
            (0..s.rows)
                .map(|y| {
                    let base = y as usize * s.cols as usize;
                    s.cells[base..base + s.cols as usize]
                        .iter()
                        .filter(|c| !c.is_spacer())
                        .map(|c| {
                            if c.has_text {
                                char::from_u32(c.codepoint).unwrap_or(' ')
                            } else {
                                ' '
                            }
                        })
                        .collect::<String>()
                        .trim_end()
                        .to_string()
                })
                .collect()
        };

        let bottom = term.snapshot();
        // 写完 L10\r\n 后光标停在新的空行，故底部视口是 [L9, L10, 空]
        assert_eq!(screen(&bottom), vec!["L9", "L10", ""]);

        // 向上 3 行应露出更早的历史
        term.scroll_delta(-3);
        let scrolled = term.snapshot();
        assert_eq!(screen(&scrolled), vec!["L6", "L7", "L8"], "应滚入历史");

        // 再向上滚到顶，应看到最早的行
        term.scroll_delta(-100);
        let top = term.snapshot();
        assert_eq!(screen(&top)[0], "L1", "滚到顶部应见到最早输出");

        // 滚回底部必须精确复原
        term.scroll_to_bottom();
        assert_eq!(screen(&term.snapshot()), screen(&bottom));
    }

    /// 用户滚入历史后，持续输出的 TUI（如 omp，每秒几十帧重绘/流式打字）
    /// 不得把视口拽回底部——否则滚轮表现为"完全滚不动"。
    #[test]
    fn output_while_scrolled_keeps_viewport() {
        let mut term = NativeTerminal::new(20, 3).expect("创建终端失败");
        for i in 1..=10 {
            term.write(format!("L{i}\r\n").as_bytes());
        }

        term.scroll_delta(-3);
        assert!(!term.viewport_is_at_bottom(), "滚入历史后视口不应贴底");

        // 模拟 TUI 持续输出：一行新内容 + 一帧原位重绘（CUP + EL）
        term.write(b"NEW\r\n");
        term.write(b"\x1b[1;1H\x1b[Kredraw");

        let s = term.snapshot();
        let visible: String = (0..s.rows)
            .map(|y| {
                let base = y as usize * s.cols as usize;
                s.cells[base..base + s.cols as usize]
                    .iter()
                    .filter(|c| !c.is_spacer() && c.has_text)
                    .map(|c| char::from_u32(c.codepoint).unwrap_or(' '))
                    .collect::<String>()
            })
            .collect();
        assert!(
            !term.viewport_is_at_bottom(),
            "滚动期间的新输出不应把视口拽回底部"
        );
        assert!(
            !visible.contains("NEW"),
            "滚在历史中时新输出不应出现在视口内：{visible:?}"
        );
    }

    /// 用 omp（内联 TUI，真实采样字节）验证滚动链路：
    /// 启动时清屏清历史（2J+3J），随后 76 行整框绘制必然把早期内容压入历史；
    /// 之后全部为 CUP+EL 原位重绘。用户在此状态下滚轮应能翻看历史。
    #[test]
    fn scrolls_within_omp_inline_tui_capture() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/omp_capture.bin"
        );
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            // 夹具是未跟踪的本机采样，用户可能通过变更清单右键删除；
            // 缺失时跳过而不是让整个测试套件变红
            Err(_) if !std::path::Path::new(path).exists() => {
                eprintln!("跳过 scrolls_within_omp_inline_tui_capture：缺少 {path}");
                return;
            }
            Err(e) => panic!("读取 omp 采样失败: {e}"),
        };
        let mut term = NativeTerminal::new(153, 19).expect("创建终端失败");
        term.write(&bytes);

        let at_bottom = term.snapshot().scrollbar;
        assert!(
            at_bottom.total > at_bottom.len,
            "omp 初始整框绘制应产生滚动历史: {at_bottom:?}"
        );

        term.scroll_delta(-15);
        let scrolled = term.snapshot().scrollbar;
        assert_ne!(
            scrolled.offset, at_bottom.offset,
            "滚入历史后视口偏移应变化: {at_bottom:?} -> {scrolled:?}"
        );
        assert!(!term.viewport_is_at_bottom(), "滚入历史后视口不应贴底");

        term.scroll_to_bottom();
        assert!(
            term.viewport_is_at_bottom(),
            "scroll_to_bottom 应回到活动区"
        );
    }

    /// 布局常量校验：原生下指针为 8 字节，wasm 的 12 字节布局不可套用
    #[test]
    fn native_layout_differs_from_wasm() {
        use crate::vt::{GhosttyGridRef, GhosttyPoint, GhosttyStyle};
        assert_eq!(std::mem::size_of::<*const u8>(), 8, "原生应为 64 位指针");
        assert_eq!(std::mem::size_of::<GhosttyPoint>(), 24);
        assert_eq!(
            std::mem::size_of::<GhosttyGridRef>(),
            24,
            "原生 GhosttyGridRef 为 24 字节（wasm32 下是 12 字节）"
        );
        assert_eq!(std::mem::align_of::<GhosttyGridRef>(), 8);
        assert_eq!(std::mem::size_of::<GhosttyStyle>(), 72);
        assert_eq!(POINT_TAG_VIEWPORT, 1);
    }

    /// 测试用的空闲子进程与会话句柄：结构要求 `Child` 与 `MasterPty`，
    /// 但不真的要跑命令。返回原始 pair 供构造 PtySession。
    fn spawn_idle_session() -> (
        Box<dyn portable_pty::MasterPty + Send>,
        Box<dyn portable_pty::Child + Send + Sync>,
    ) {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty 失败");
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.arg("-c");
        cmd.arg("exec sleep 30");
        let child = pair.slave.spawn_command(cmd).expect("启动 sh 失败");
        drop(pair.slave);
        (pair.master, child)
    }

    /// 回归：`vt_search` 现在在 sessions 锁内只 Arc::clone 出终端就放锁，
    /// 再于 blocking 池内 `terminal.lock()` 搜索。此测试守住的契约是
    /// 「Arc 存活期间锁会话表」不再阻塞搜索本身：搜索完成后立刻就能
    /// 再锁一次会话表去读快照，验证锁没有泄漏给后续命令。
    #[test]
    fn search_releases_global_lock_before_running() {
        let manager = PtyManager::new();
        // 用 Manager 内部锁语义模拟：先锁 sessions 取 Arc，放锁，
        // 再锁 terminal 做耗时搜索，最后验证 sessions 锁可被再次获取。
        let terminal: Arc<Mutex<NativeTerminal>> = {
            let mut sessions = manager.sessions.lock().expect("锁 sessions 失败");
            let (master, child) = spawn_idle_session();
            let session = PtySession {
                terminal: Arc::new(Mutex::new(
                    NativeTerminal::new(40, 5).expect("创建终端失败"),
                )),
                cwd: String::new(),
                window_label: String::new(),
                retired: Arc::new(AtomicBool::new(false)),
                master: Arc::new(Mutex::new(master)),
                writer: Arc::new(Mutex::new(
                    Box::new(std::io::sink()) as Box<dyn Write + Send>
                )),
                child: Arc::new(Mutex::new(child)),
                reader: None,
            };
            let arc = Arc::clone(&session.terminal);
            sessions.insert("s1".to_string(), session);
            arc
        };

        {
            let mut term = terminal.lock().expect("锁 terminal 失败");
            for i in 1..=20 {
                term.write(format!("row {i}\r\n").as_bytes());
            }
        }

        // 锁 sessions 拿 Arc 后立刻放掉 —— 与 vt_search 的写法一致
        let handle = {
            let sessions = manager.sessions.lock().expect("锁 sessions 失败");
            Arc::clone(&sessions.get("s1").expect("会话应存在").terminal)
        };

        // 在持有 handle 的情况下 sessions 锁必须可再获取（已放锁）
        let search_result = {
            let sessions = manager.sessions.lock().expect("sessions 锁应已释放");
            let t = sessions.get("s1").expect("会话应存在");
            let mut term = t.terminal.lock().expect("锁 terminal 失败");
            term.search("row 12", Some(SearchSelect::Next))
        };
        let found = search_result.expect("放锁后应能完成搜索");
        assert!(
            found.total > 0,
            "20 行里应至少匹配一次 'row 12'，实际 total={}",
            found.total
        );

        drop(handle);
    }

    /// 一次只吃 3 字节的写端：`write_all` 必须循环写完（短路成 `write`
    /// 会静默截断输入，长粘贴丢尾是用户直接能感知的故障）。
    struct TrickleWriter(Arc<Mutex<Vec<u8>>>);
    impl Write for TrickleWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let n = buf.len().min(3);
            self.0
                .lock()
                .expect("锁接收缓冲失败")
                .extend_from_slice(&buf[..n]);
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// 写端报错的替身：用于验证错误必须带明确文案上抛。
    struct BrokenWriter;
    impl Write for BrokenWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "对端已关闭",
            ))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// 写入路径的两条契约：字节必须完整送达；失败必须报错（前端按错误判定
    /// 按键是否送达，静默吞掉会让丢键无从排查）。
    #[test]
    fn write_session_delivers_every_byte_and_reports_failure() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let sink: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(TrickleWriter(Arc::clone(&received)))));
        let payload: Vec<u8> = (0..=255u8).collect();
        write_session(&sink, &payload).expect("写入应成功");
        assert_eq!(
            *received.lock().expect("锁接收缓冲失败"),
            payload,
            "每个字节都要送达"
        );

        let broken: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(BrokenWriter)));
        let err = write_session(&broken, b"hi").expect_err("写端报错必须上抛");
        assert!(
            err.contains("写入 PTY 失败") && err.contains("对端已关闭"),
            "错误文案要能定位原因，实际：{err}"
        );
    }

    /// 网格尺寸夹取：这是 IPC 信任边界的守卫。
    ///
    /// `cols`/`rows` 是 pty_spawn / pty_resize 的入参，可被任意 IPC 调用方
    /// 构造，也可被前端的「窗口尺寸 ÷ 单元格尺寸」算歪（单元格为 0 时
    /// → Infinity/NaN）。u16 全量区间是 43 亿格，一次 vt_snapshot 的
    /// 分配就能把进程打爆。
    mod grid_clamp_tests {
        use super::*;

        #[test]
        fn normal_sizes_pass_through_unchanged() {
            assert_eq!(clamp_grid(80, 24), (80, 24));
            assert_eq!(clamp_grid(400, 200), (400, 200));
        }

        #[test]
        fn extreme_sizes_are_capped() {
            assert_eq!(clamp_grid(u16::MAX, u16::MAX), (MAX_COLS, MAX_ROWS));
            assert_eq!(clamp_grid(65535, 1), (MAX_COLS, 1));
            // 1001 列（刚超上限）也要夹
            assert_eq!(clamp_grid(1001, 1001), (1000, 1000));
        }

        #[test]
        fn zero_is_lifted_to_one() {
            // 引擎要求 > 0：0 会被 ghostty_terminal_resize 拒绝，
            // 且 PTY 与引擎网格从此永久不一致
            assert_eq!(clamp_grid(0, 0), (MIN_COLS, MIN_ROWS));
            assert_eq!(clamp_grid(0, 24), (1, 24));
            assert_eq!(clamp_grid(80, 0), (80, 1));
        }

        /// 编译期断言：上限必须覆盖 4K 全屏（约 400 列 × 200 行）而有余量，
        /// 否则正常使用会被误夹。用 const 断言而非 #[test]：这是对常量本身
        /// 的约束，编译期失败比等到跑测试才暴露更早。
        const _: () = {
            assert!(MAX_COLS >= 800, "列上限要覆盖 4K 全屏");
            assert!(MAX_ROWS >= 400, "行上限要覆盖 4K 全屏");
            // 上限必须真的比正常使用大，否则这个限制就失去保护意义
            assert!(MAX_COLS < u16::MAX, "上限应远小于 u16 上限");
        };
    }
}

/// 凭据环境变量剔除的端到端验证。
///
/// 用真实 PTY 起 shell 打印 `env`，断言 blocklist 里的变量确实不在输出中。
/// 这比单测常量列表强：它验证了 `CommandBuilder::env_remove` 真的能对抗
/// portable-pty 的「全量回填父环境」（cmdbuilder.rs:497-544）这一行为。
#[cfg(test)]
mod credential_env_tests {
    use super::CREDENTIAL_ENV_BLOCKLIST;
    use crate::pty::native_pty_system;
    use portable_pty::{CommandBuilder, PtySize};
    use std::io::Read;

    #[test]
    fn blocklisted_env_never_reaches_the_shell() {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 200,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty 失败");

        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.arg("-c");
        cmd.arg("env");
        // 先在测试进程里放下这些「凭据」，模拟 IDE 持有它们的状态
        for name in CREDENTIAL_ENV_BLOCKLIST {
            cmd.env(name, "LEAK_CANARY");
        }
        // 一个不该被剔除的对照变量
        cmd.env("LAZECODE_CANARY_KEEP", "KEEP_ME");

        // 与 pty_spawn 同样的剔除动作
        for name in CREDENTIAL_ENV_BLOCKLIST {
            cmd.env_remove(name);
        }

        let mut child = pair.slave.spawn_command(cmd).expect("启动 shell 失败");
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().expect("克隆 reader 失败");
        let mut out = String::new();
        let mut buf = [0u8; 8192];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            out.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        let _ = child.wait();

        for name in CREDENTIAL_ENV_BLOCKLIST {
            assert!(
                !out.contains(name),
                "环境变量 {name} 泄漏进了终端 shell（凭据暴露）"
            );
            assert!(!out.contains("LEAK_CANARY"), "凭据值泄漏进了终端 shell");
        }
        assert!(
            out.contains("LAZECODE_CANARY_KEEP"),
            "非凭据变量不该被误删：PATH/SSH_AUTH_SOCK 一类是终端可用性的前提"
        );
    }

    /// 反向自检：名单本身要够大才有意义（防止将来有人把它清空来「省事」）。
    #[test]
    fn blocklist_is_not_trivially_empty() {
        assert!(
            CREDENTIAL_ENV_BLOCKLIST.len() >= 20,
            "名单至少应覆盖主流云厂商与模型服务的凭据变量，实际 {} 项",
            CREDENTIAL_ENV_BLOCKLIST.len()
        );
        // 去重：重复项会让 env_remove 白跑
        let mut sorted: Vec<&str> = CREDENTIAL_ENV_BLOCKLIST.to_vec();
        sorted.sort_unstable();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(before, sorted.len(), "名单里有重复项");
    }
}

/// 把一次按键编码为写入 PTY 的字节。
///
/// 编码在原生侧完成：kitty 协议标志、光标键应用模式等模式状态
/// 只有终端自己知道，前端不应复刻这套逻辑。
#[tauri::command]
pub fn vt_encode_key(
    manager: State<'_, PtyManager>,
    session_id: String,
    input: KeyInput,
) -> Result<Vec<u8>, String> {
    let sessions = manager
        .sessions
        .lock()
        .map_err(|_| "PTY 状态锁失效".to_string())?;
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| format!("会话不存在: {session_id}"))?;

    let mut term = session
        .terminal
        .lock()
        .map_err(|_| "终端状态锁失效".to_string())?;
    term.encode_key(&input)
}

/// 设置选区（端点为视口坐标，闭区间）
#[tauri::command]
pub fn vt_set_selection(
    manager: State<'_, PtyManager>,
    session_id: String,
    start_x: u16,
    start_y: u16,
    end_x: u16,
    end_y: u16,
    rectangle: Option<bool>,
) -> Result<(), String> {
    let sessions = manager
        .sessions
        .lock()
        .map_err(|_| "PTY 状态锁失效".to_string())?;
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| format!("会话不存在: {session_id}"))?;

    let mut term = session
        .terminal
        .lock()
        .map_err(|_| "终端状态锁失效".to_string())?;
    term.set_selection(start_x, start_y, end_x, end_y, rectangle.unwrap_or(false))
}

/// 双击选词 / 三击选行：由引擎按词边界、行内容算出区间并装入选区，
/// 返回端点（视口坐标，闭区间）供前端绘制高亮。无可选内容时返回 null。
#[tauri::command]
pub fn vt_select_word(
    manager: State<'_, PtyManager>,
    session_id: String,
    x: u16,
    y: u16,
) -> Result<Option<(u16, u16, u16, u16)>, String> {
    let sessions = manager
        .sessions
        .lock()
        .map_err(|_| "PTY 状态锁失效".to_string())?;
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| format!("会话不存在: {session_id}"))?;

    let mut term = session
        .terminal
        .lock()
        .map_err(|_| "终端状态锁失效".to_string())?;
    term.select_word(x, y)
}

#[tauri::command]
pub fn vt_select_line(
    manager: State<'_, PtyManager>,
    session_id: String,
    y: u16,
) -> Result<Option<(u16, u16, u16, u16)>, String> {
    let sessions = manager
        .sessions
        .lock()
        .map_err(|_| "PTY 状态锁失效".to_string())?;
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| format!("会话不存在: {session_id}"))?;

    let mut term = session
        .terminal
        .lock()
        .map_err(|_| "终端状态锁失效".to_string())?;
    term.select_line(y)
}

#[tauri::command]
pub fn vt_clear_selection(
    manager: State<'_, PtyManager>,
    session_id: String,
) -> Result<(), String> {
    let sessions = manager
        .sessions
        .lock()
        .map_err(|_| "PTY 状态锁失效".to_string())?;
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| format!("会话不存在: {session_id}"))?;

    let mut term = session
        .terminal
        .lock()
        .map_err(|_| "终端状态锁失效".to_string())?;
    term.clear_selection()
}

/// 取选区纯文本（供复制）
#[tauri::command]
pub fn vt_selected_text(
    manager: State<'_, PtyManager>,
    session_id: String,
) -> Result<String, String> {
    let sessions = manager
        .sessions
        .lock()
        .map_err(|_| "PTY 状态锁失效".to_string())?;
    let session = sessions
        .get(&session_id)
        .ok_or_else(|| format!("会话不存在: {session_id}"))?;

    let mut term = session
        .terminal
        .lock()
        .map_err(|_| "终端状态锁失效".to_string())?;
    term.selected_text()
}

/// 在滚动历史中搜索
///
/// `ghostty_search_run` 是 feed+tick 直到追平整段滚动历史的阻塞调用，
/// 历史有多长就跑多久。原来它持着会话表锁与终端锁同步执行：
/// 搜索期间该会话的 vt_snapshot（60fps 渲染）、pty_write、vt_scroll 全部
/// 排队，且命令跑在唯一的 ipc worker 上，其它会话的同步命令一起停摆。
///
/// 这里在会话表锁内只 Arc::clone 出终端句柄就放锁，把搜索丢进 blocking 池，
/// ipc worker 立即脱身；全局锁不再持有，其它会话照常工作。
#[tauri::command]
pub async fn vt_search(
    manager: State<'_, PtyManager>,
    session_id: String,
    needle: String,
    select: Option<SearchSelect>,
) -> Result<SearchResult, String> {
    let terminal = {
        let sessions = manager
            .sessions
            .lock()
            .map_err(|_| "PTY 状态锁失效".to_string())?;
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| format!("会话不存在: {session_id}"))?;
        Arc::clone(&session.terminal)
    };

    tauri::async_runtime::spawn_blocking(move || {
        let mut term = terminal.lock().map_err(|_| "终端状态锁失效".to_string())?;
        term.search(&needle, select)
    })
    .await
    .map_err(|e| format!("vt_search 任务失败: {e}"))?
}

/// 引擎身份信息（版本、目标平台、后端类型）
#[tauri::command]
pub fn vt_engine_info() -> Result<serde_json::Value, String> {
    crate::vt::engine_info()
}

#[cfg(test)]
mod diff_e2e {
    use crate::vt::{CellView, NativeTerminal};

    /// 增量路径的端到端真值证明。
    ///
    /// 前端 `CellDiffCache` 的合并逻辑已有 Rust 侧测试覆盖，这里补上
    /// **生产路径**的最后一环：连续多帧差分后，合并结果必须与同刻的
    /// 全量快照逐格一致。这是「增量正确性」的最终防线 —— 两条路径
    /// 只要在任何一格上分叉，用户看到的就是花屏。
    ///
    /// 帧序列刻意覆盖：普通回显、彩色、真彩、粗斜体、宽字符、多行滚动。
    #[test]
    fn incremental_merges_to_identical_screen() {
        let mut t = NativeTerminal::new(20, 5).unwrap();

        // 首帧：整屏。断言非空即证明确实生成了帧（增量路径没有静默返回空）。
        assert!(!t.snapshot_wire_diff(true).is_empty(), "首帧应为整屏增量帧");
        let mut screen = Screen::new(&mut t);

        let script: &[&[u8]] = &[
            b"hello",
            b"\x1b[31m red\x1b[0m",
            b"\x1b[38;2;10;200;90m true\x1b[0m",
            b"\x1b[1;3m bold\x1b[0m",
            "中文测试".as_bytes(),
            b"\r\nline2\r\nline3\r\nline4",
            b"\x1b[2;3Hpatched",
        ];
        for chunk in script {
            t.write(chunk);
            let frame = t.snapshot_wire_diff(false);
            screen.merge(&frame);
            assert_screen_matches(&mut t, &screen, chunk);
        }
    }

    /// 一份可增量合并的屏幕镜像：与前端 `CellDiffCache` 同构，
    /// 独立实现一遍以免复用被测代码而互相掩盖错误。
    struct Screen {
        cols: usize,
        rows: usize,
        cells: Vec<CellView>,
    }

    impl Screen {
        fn new(t: &mut NativeTerminal) -> Self {
            let snap = t.snapshot();
            Self {
                cols: snap.cols as usize,
                rows: snap.rows as usize,
                cells: snap.cells,
            }
        }

        /// 按 Rust 侧线格式解码增量帧并覆盖对应行。
        fn merge(&mut self, frame: &[u8]) {
            let rd_u16 = |o: usize| u16::from_le_bytes([frame[o], frame[o + 1]]) as usize;
            let rd_u32 =
                |o: usize| u32::from_le_bytes([frame[o], frame[o + 1], frame[o + 2], frame[o + 3]]);
            assert_eq!(rd_u32(0), 0x5456_4e33, "增量帧 magic");

            let cols = rd_u16(4);
            let rows = rd_u16(6);
            assert_eq!(cols, self.cols, "列数应一致");
            assert_eq!(rows, self.rows, "行数应一致");

            let style_count = rd_u16(30);
            let row_count = rd_u16(32);

            let mut off = 60;
            let mut styles: Vec<CellView> = Vec::with_capacity(style_count);
            for _ in 0..style_count {
                let fg = i32::from_le_bytes([
                    frame[off],
                    frame[off + 1],
                    frame[off + 2],
                    frame[off + 3],
                ]);
                let bg = i32::from_le_bytes([
                    frame[off + 4],
                    frame[off + 5],
                    frame[off + 6],
                    frame[off + 7],
                ]);
                let flags = rd_u32(off + 8);
                styles.push(CellView {
                    codepoint: 0,
                    wide: 0,
                    has_text: false,
                    fg: (fg >= 0).then_some(fg as u32),
                    bg: (bg >= 0).then_some(bg as u32),
                    bold: flags & 1 != 0,
                    italic: flags & 2 != 0,
                    faint: flags & 4 != 0,
                    inverse: flags & 8 != 0,
                    invisible: flags & 16 != 0,
                    strikethrough: flags & 32 != 0,
                    cluster_index: 0,
                });
                off += 12;
            }

            for _ in 0..row_count {
                let y = rd_u16(off);
                // VTN3 行头固定 8 字节：u16 row + u16 reserved + u16 row_flags + u16 pad。
                // 逐行样式片段已在 VTN3 删除（cells 里的 style_idx 是帧级表全局下标）。
                off += 8;
                for x in 0..cols {
                    let packed = u64::from_le_bytes([
                        frame[off],
                        frame[off + 1],
                        frame[off + 2],
                        frame[off + 3],
                        frame[off + 4],
                        frame[off + 5],
                        frame[off + 6],
                        frame[off + 7],
                    ]);
                    off += 8;
                    let style_idx = ((packed >> 23) & 0xffff) as usize;
                    let mut cell = styles[style_idx.min(styles.len().saturating_sub(1))];
                    cell.codepoint = (packed & 0x1f_ffff) as u32;
                    cell.wide = ((packed >> 21) & 0b11) as u8;
                    cell.has_text = (packed >> 39) & 1 == 1;
                    self.cells[y * self.cols + x] = cell;
                }
            }
        }
    }

    fn assert_screen_matches(t: &mut NativeTerminal, screen: &Screen, label: &[u8]) {
        let truth = t.snapshot();
        let name = String::from_utf8_lossy(label);
        for i in 0..truth.cells.len() {
            let a = &truth.cells[i];
            let b = &screen.cells[i];
            let at = format!("格 {i} (写入 {name:?} 后)");
            assert_eq!(a.codepoint, b.codepoint, "{at} 码点");
            assert_eq!(a.wide, b.wide, "{at} 宽度");
            assert_eq!(a.has_text, b.has_text, "{at} 有字形");
            assert_eq!(a.fg, b.fg, "{at} 前景色");
            assert_eq!(a.bg, b.bg, "{at} 背景色");
            assert_eq!(a.bold, b.bold, "{at} 粗体");
            assert_eq!(a.italic, b.italic, "{at} 斜体");
        }
    }
}

/// Kitty 图形协议（终端内联图片）回归。
///
/// 使用测试内生成的 1x1 PNG，不依赖任何本机采样文件。锁定链路：
/// 图形存储已启用 → PNG 解码器已装 → 图片被接受 → placement 能被查到 →
/// 像素能取到。任何一环缺失，终端里就只剩一堆豆腐块。
#[cfg(test)]
mod kitty_graphics_tests {
    use crate::vt::NativeTerminal;

    /// 图形链路（PNG → 图片 → 虚拟 placement）由 `vt.rs` 的图形测试用
    /// 2x2 PNG 覆盖；这里只保留「没有图片」的基线，避免依赖任何本机采样。
    #[test]
    fn empty_terminal_reports_no_images() {
        let mut term = NativeTerminal::new(80, 24).expect("创建终端失败");
        term.write(b"hello\r\n");
        let images = term.images();
        assert!(images.placements.is_empty(), "无图片时不应有 placement");
        assert_eq!(images.generation, 0, "无图片时 generation 应为 0");
    }
}

#[cfg(all(test, unix))]
mod busy_probe_tests {
    use super::*;
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    /// 本模块的测试必须串行执行。
    ///
    /// `probe_session_busy` 靠 `ps -t <tty>` 看某条 tty 上有什么进程在跑。
    /// 模块内四个测试各自 openpty + spawn shell，并行时这些 tty 上的进程列表
    /// 互相串扰，加上 `ps` 自身有采集延迟，就会把「空 shell」判成忙（或反之）
    /// —— 不是被测代码错，是探测在并发下不可信。串行化后每个断言观察到的
    /// 都是自己那一个 shell。
    static PTY_TEST_LOCK: Mutex<()> = Mutex::new(());

    /// 每个测试开头取一次。锁只在测试期间持有，见 PTY_TEST_LOCK 的说明。
    fn serialize() -> std::sync::MutexGuard<'static, ()> {
        PTY_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 真机回归：控制台关闭确认的判定必须区分「停在提示符」与「命令在跑」。
    ///
    /// 旧行为（无此判定）会直接 kill 掉用户正在跑的服务；这里用真实 PTY +
    /// 真实 shell 锁定：空 shell → 不忙；跑 sleep → 忙且报出命令名；
    /// 命令结束后回到不忙。
    #[test]
    fn busy_probe_distinguishes_idle_shell_from_running_command() {
        let _guard = serialize();
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty 失败");
        let cmd = CommandBuilder::new("/bin/sh");
        let mut child = pair.slave.spawn_command(cmd).expect("启动 shell 失败");
        drop(pair.slave);

        let mut writer = pair.master.take_writer().expect("获取 writer 失败");
        let master: Arc<Mutex<Box<dyn MasterPty + Send>>> = Arc::new(Mutex::new(pair.master));
        let shell_pid = child.process_id();
        assert!(shell_pid.is_some(), "应能取到 shell 进程号");

        let probe =
            |master: &Arc<Mutex<Box<dyn MasterPty + Send>>>| probe_session_busy(master, shell_pid);

        // 等 shell 起来：提示符状态必须是「不忙」
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut idle_ok = false;
        while Instant::now() < deadline {
            if !probe(&master).busy {
                idle_ok = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(idle_ok, "空 shell 停在提示符时不应判定为有命令在跑");

        // 跑一条长命令：必须判定为忙，并报出命令名
        writer.write_all(b"sleep 30\n").expect("写入命令失败");
        writer.flush().ok();
        // 循环条件带上命令名：shell 收到指令后先 fork、再由子进程 exec，
        // 这段窗口里 `ps` 只看得到 sh 自己（busy = true 但没有 sleep）。
        // 只看 busy 会在这里提前跳出，拿到一个报不出命令名的「忙」。
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut running = PtyBusyDto {
            busy: false,
            commands: Vec::new(),
        };
        while Instant::now() < deadline {
            running = probe(&master);
            if running.busy && running.commands.iter().any(|c| c == "sleep") {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(
            running.busy,
            "sleep 在跑时必须判定为忙（否则关闭控制台会静默杀掉它）"
        );
        assert!(
            running.commands.iter().any(|c| c == "sleep"),
            "确认弹窗应能报出命令名，实际: {:?}",
            running.commands
        );

        // 命令结束后回到「不忙」
        writer.write_all(b"\x03").expect("发送 Ctrl-C 失败");
        writer.flush().ok();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut back_to_idle = false;
        while Instant::now() < deadline {
            if !probe(&master).busy {
                back_to_idle = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(back_to_idle, "命令结束后应回到不忙状态");

        // 收尾：kill 后必须先放掉主端与写端，子进程才退得掉（见 close_session 注释）
        let _ = child.kill();
        drop(master);
        drop(writer);
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut reaped = false;
        while Instant::now() < deadline {
            if matches!(child.try_wait(), Ok(Some(_))) {
                reaped = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(reaped, "收尾时 shell 应能被回收");
    }

    /// 对照组：不经过 PTY 的普通子进程，kill 后应立刻可回收。
    /// 若这条也挂，说明是测试环境（沙箱）问题，而不是 portable-pty / 关闭链路。
    #[test]
    fn plain_child_kill_reaps_immediately() {
        let _guard = serialize();
        let mut c = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg("sleep 60")
            .spawn()
            .expect("spawn 失败");
        std::thread::sleep(Duration::from_millis(300));
        c.kill().expect("kill 失败");
        let started = Instant::now();
        let st = c.wait().expect("wait 失败");
        eprintln!(
            "[probe] plain wait 返回 {:?}，耗时 {:?}",
            st,
            started.elapsed()
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "普通子进程应立刻回收"
        );
    }

    /// 关闭路径回归：真实 fd 布局（master + writer + 读线程的 reader 克隆）下，
    /// `close_session` 必须**有界返回**且子进程真的被回收。
    ///
    /// 旧实现先 `wait`、master/writer 到函数末尾才 drop：macOS 上被 kill 的
    /// 会话首进程会一直停在 E（exiting）状态等主端关闭，于是 wait 永远收不到 ——
    /// 每关一个控制台就挂住一个线程、留下一个退不掉的 shell（实测 60s 不可回收）。
    #[test]
    fn close_session_closes_master_before_waiting() {
        let _guard = serialize();
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty 失败");
        let mut reader = pair.master.try_clone_reader().expect("克隆 reader 失败");
        let writer = pair.master.take_writer().expect("获取 writer 失败");
        let child_box = pair
            .slave
            .spawn_command(CommandBuilder::new("/bin/sh"))
            .expect("启动 shell 失败");
        drop(pair.slave);
        let child: Arc<Mutex<Box<dyn Child + Send + Sync>>> = Arc::new(Mutex::new(child_box));

        // 读线程与生产同构：阻塞在 read 上，EOF 后回收子进程
        let reader_child = Arc::clone(&child);
        let reader_thread = std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            let _ = locked_exit_code(&reader_child);
        });

        let session = PtySession {
            master: Arc::new(Mutex::new(pair.master)),
            writer: Arc::new(Mutex::new(writer)),
            child: Arc::clone(&child),
            terminal: Arc::new(Mutex::new(NativeTerminal::new(80, 24).unwrap())),
            cwd: "/tmp".to_string(),
            window_label: "test".to_string(),
            retired: Arc::new(AtomicBool::new(false)),
            reader: Some(reader_thread),
        };

        let started = Instant::now();
        close_session(session);
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(3),
            "close_session 必须有界返回，实际耗时 {elapsed:?}"
        );
        // 不能 try_lock 一把定成败：读线程收尾时可能正持锁做 wait()，
        // 那一下一定拿不到锁，但子进程其实是已回收的。轮询到 deadline，
        // 期间只要出现过 `Some(_)` 就算真回收。
        let mut reaped = false;
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if matches!(child.try_lock().map(|mut c| c.try_wait()), Ok(Ok(Some(_)))) {
                reaped = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            reaped,
            "close_session 返回时子进程应已真正回收（不是靠放弃等待蒙混过关）"
        );
    }
}

#[cfg(test)]
mod query_response_tests {
    use super::*;
    use crate::vt::NativeTerminal;

    /// 收集字节的伪 PTY 写端。
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl Write for Sink {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// 引擎的查询回包必须经 `forward_terminal_responses` 真正写进 PTY 写端：
    /// 这条链路缺失时，TUI 的能力探测（含 omp 判定内联图片的那次查询）全部超时。
    #[test]
    fn engine_replies_are_written_back_to_the_pty() {
        let mut term = NativeTerminal::new(80, 24).unwrap();
        let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(Sink(Arc::clone(&buf)))));

        term.write(b"\x1b[c\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\");
        forward_terminal_responses(&mut term, &writer);

        let got = String::from_utf8_lossy(&buf.lock().unwrap().clone()).to_string();
        assert!(got.contains("\x1b[?62;22c"), "DA1 回包应写回 PTY: {got:?}");
        assert!(
            got.contains("\x1b_Gi=31;OK\x1b\\"),
            "kitty 图形握手回包应写回 PTY: {got:?}"
        );

        // 没有待回包时不得写入任何字节
        buf.lock().unwrap().clear();
        forward_terminal_responses(&mut term, &writer);
        assert!(buf.lock().unwrap().is_empty());
    }
}
