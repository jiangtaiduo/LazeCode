//! 打开文件的外部修改监听。
//!
//! 前端把「当前打开的文件列表」推给 `fs_watch_files`，这里监听这些文件的
//! **父目录**（非递归），把命中已注册文件的事件以 `fs://external-change`
//! 推回前端，由前端决定自动重载还是提示冲突。
//!
//! 为什么监听父目录而不是文件本身：git、部分编辑器用「写临时文件 + rename
//! 覆盖」保存，文件级监听挂在被换掉的旧 inode 上会静默失效，之后再也收不到
//! 事件。目录级监听天然覆盖原地写入与 rename 两种落盘方式。
//!
//! # 目录监听（`fs_watch_dirs`）必须是递归的
//!
//! 文件树要在外部程序（构建工具、agent CLI、Finder）新建文件时自动刷新，
//! 而这些文件可以出现在**任意深度**的子目录里。notify 的 FSEvents 后端在
//! NonRecursive 模式下只放行「根本身 + 根的直接子项」两类事件路径
//! （fsevent.rs 回调里的 `parent_path == p` 过滤），`project/src/new.ts`
//! 这类深度 2 的路径会被 Rust 侧直接丢弃 —— 前端根本收不到事件，文件树
//! 就停在旧内容上（实测过的 bug）。因此目录监听必须 `RecursiveMode::Recursive`。
//!
//! 递归的代价是事件量：一次 `npm install` 亚秒内产生成百上千个文件事件。
//! [`DirChangeCoalescer`] 按「变更目录」合并（250ms 窗口，每目录至多一条），
//! 超过容量上限时退化为「整根一条」让前端整树刷新。

use crate::commands::{authorize_workspace_path, WorkspaceRegistry};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, State};

/// 外部修改事件通道名。负载 `{ path }` = 被修改文件**注册时前端给出的路径**
/// （openFilePaths 原文 —— 前端拿它当缓冲键，报错形态会静默查不到缓冲）。
pub const EXTERNAL_CHANGE_EVENT: &str = "fs://external-change";

/// 目录变更事件通道名（`fs_watch_dirs`）。负载 `{ path }` = 被监听目录的
/// 注册原文。只表示「这个目录里有东西变了」，前端据此刷新文件树；
/// 不携带是哪个文件变了 —— 那对重构树没有意义。
pub const DIR_CHANGE_EVENT: &str = "fs://dir-change";

/// 事件负载（与前端 fileWatch.ts 的订阅保持一致）
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalChangePayload {
    pub path: String,
}

/// watcher 本体的持有者：按**窗口 label** 分组，两套互不相干的表。
///
/// - `files`：打开文件的事件监听（`fs_watch_files`）
/// - `dirs`：目录监听（`fs_watch_dirs`）
///
/// 必须分开：两者的更新来源不同（前者跟 openFilePaths、后者跟 workspacePath），
/// 共用一张表会让「换了个标签页」把目录监听顶掉、反过来也成立。进程级单槽在
/// 多窗口下还会被后开窗口整体顶掉。移除某条（或 drop 其值）即停止对应的监听。
pub struct FileWatchState {
    pub files: Mutex<HashMap<String, RecommendedWatcher>>,
    pub dirs: Mutex<HashMap<String, RecommendedWatcher>>,
}

impl Default for FileWatchState {
    fn default() -> Self {
        Self {
            files: Mutex::new(HashMap::new()),
            dirs: Mutex::new(HashMap::new()),
        }
    }
}

impl FileWatchState {
    /// 窗口销毁时清掉该窗口的两类 watcher（drop = 停止其全部目录监听）。
    pub fn stop_for_window(&self, window_label: &str) {
        if let Ok(mut map) = self.files.lock() {
            map.remove(window_label);
        }
        if let Ok(mut map) = self.dirs.lock() {
            map.remove(window_label);
        }
    }
}

/// 规范化路径用于匹配：能 canonicalize 就规范化，失败（已被删除等）退回原路径。
fn norm(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// 注册索引：查找键（事件可能上报的形态）→ 注册值（前端给出的原始路径）。
///
/// FSEvents/inotify 上报的形态与前端传入的未必同形（`/tmp` 软链、大小写等），
/// 而**回调必须返回前端的原始形态**：前端用它做缓冲键，报错形态会静默查不到缓冲。
type FileIndex = HashMap<PathBuf, PathBuf>;

/// 登记一个文件：原始形态与规范化形态都作为查找键，值统一为原始形态。
fn register(index: &mut FileIndex, raw: PathBuf) {
    let canonical = norm(&raw);
    if canonical != raw {
        index.insert(canonical, raw.clone());
    }
    index.insert(raw.clone(), raw);
}

/// 事件路径命中则返回应回调给前端的路径（注册时的原始形态）。
///
/// `files` 为空索引时调用方应按「直通」处理（见 spawn_watcher），不走这里。
fn lookup_registered(event_path: &Path, index: &FileIndex) -> Option<PathBuf> {
    index
        .get(event_path)
        .or_else(|| index.get(&norm(event_path)))
        .cloned()
}

type SharedFiles = Arc<Mutex<FileIndex>>;

/// 目录变更的合并发送器：按「变更目录」去重，窗口期内每目录至多发一条。
///
/// 递归监听后事件量与子树规模成正比（构建期间 node_modules 每秒成百事件），
/// 逐事件 emit 会把 WKWebView 事件队列灌满 —— 与终端脏信号洪水同病，且前端
/// 的 250ms 防抖只能压制刷新频率，压不住事件本身的 IPC 分发。这里在 Rust 侧
/// 先合并：pending 集合去重，flusher 线程每 [`DIR_CHANGE_FLUSH_MS`] 发一批。
///
/// 容量上限：pending 超过 [`DIR_CHANGE_PENDING_CAP`]（构建风暴）时整批丢弃、
/// 只保留 fallback（被监听的根），前端收到根路径就整树重读 —— 语义保守但正确。
struct DirChangeCoalescer {
    pending: Mutex<HashSet<PathBuf>>,
    scheduled: AtomicBool,
    /// 风暴退化哨兵：pending 超限时整批丢弃、只保留这个路径（被监听的根），
    /// 前端对根做整树刷新，覆盖所有被淹没的子目录变更。
    fallback: PathBuf,
    emit: Box<dyn Fn(PathBuf) + Send + Sync>,
}

/// 合并窗口。250ms 与前端 `debounceDirectoryChanges` 的默认值同量级：
/// Rust 侧负责「事件 → 每目录一条」，前端再按目录防抖收敛刷新。
const DIR_CHANGE_FLUSH_MS: u64 = 250;
/// pending 目录数上限：超过即退化为整根一条。
const DIR_CHANGE_PENDING_CAP: usize = 500;

impl DirChangeCoalescer {
    fn new(fallback: PathBuf, emit: Box<dyn Fn(PathBuf) + Send + Sync>) -> Self {
        Self {
            pending: Mutex::new(HashSet::new()),
            scheduled: AtomicBool::new(false),
            fallback,
            emit,
        }
    }

    fn hit(self: &Arc<Self>, dir: PathBuf) {
        {
            let mut guard = match self.pending.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            if guard.len() >= DIR_CHANGE_PENDING_CAP {
                // 风暴退化：明细全部淹没时逐目录刷新已无意义，整批丢弃、
                // 只留根哨兵让前端整树重读。
                guard.clear();
                guard.insert(self.fallback.clone());
            } else {
                guard.insert(dir);
            }
        }
        self.schedule();
    }

    /// 有丢唤醒风险的双检调度：`scheduled` 置 false 前后各查一次 pending。
    /// 极端交错下可能多起一个 flusher 重复发同一条 —— 前端按目录防抖幂等，
    /// 重复无害；丢发才是真 bug（树停在旧内容）。
    fn schedule(self: &Arc<Self>) {
        if self.scheduled.swap(true, Ordering::SeqCst) {
            return;
        }
        // 无句柄可存 thread handle：flusher 自退出（pending 清空），watcher
        // 被 drop 时线程最多再活一个窗口期。线程失败时的兜底直发仍要用
        // 句柄，闭包与外层各持一份。
        let this = Arc::clone(self);
        let this_for_thread = Arc::clone(&this);
        let spawned = std::thread::Builder::new()
            .name("dir-change-coalescer".to_string())
            .spawn(move || {
                let this = this_for_thread;
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(DIR_CHANGE_FLUSH_MS));
                    let batch: Vec<PathBuf> = match this.pending.lock() {
                        Ok(mut g) => g.drain().collect(),
                        Err(_) => return,
                    };
                    for dir in batch {
                        (this.emit)(dir);
                    }
                    this.scheduled.store(false, Ordering::SeqCst);
                    if this.pending.lock().map(|g| g.is_empty()).unwrap_or(true) {
                        return;
                    }
                    this.scheduled.store(true, Ordering::SeqCst);
                }
            });
        if spawned.is_err() {
            // 线程起不来：退化为逐条直发，宁可事件多也不能丢变更
            this.scheduled.store(false, Ordering::SeqCst);
            let batch: Vec<PathBuf> = match this.pending.lock() {
                Ok(mut g) => g.drain().collect(),
                Err(_) => return,
            };
            for dir in batch {
                (this.emit)(dir);
            }
        }
    }
}

/// 把 FSEvents 事件路径换算成「内容变化的目录」（原始根前缀形态）。
///
/// 事件路径是监听根的 canonical 形态下的绝对路径；树节点路径来自
/// `read_directory`（以注册时的原文为基）。直接回传 canonical 形态会与
/// 树节点对不上（/tmp → /private/tmp），因此前缀 Surgery 回注册原文：
/// 事件 == 根 → 根原文；事件在根下 → 根原文 + 相对部分的父目录。
/// 返回 None = 不在任何根下（不该发生，防御）。
fn changed_dir_of(event_path: &Path, root_norm: &Path, root_orig: &str) -> Option<PathBuf> {
    let rel = event_path.strip_prefix(root_norm).ok()?;
    match rel.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(parent) => Some(PathBuf::from(root_orig).join(parent)),
        None => Some(PathBuf::from(root_orig)),
    }
}

/// 事件是否落在 `<root>/.git` 子树内（只看根下第一层；子模块的 `sub/.git`
/// 不在此列，其事件到前端后无害——树里没有该节点，刷新是引用级 no-op）。
///
/// **必须过滤**：应用自己的 git 刷新（status/diff 的 opportunistic index
/// 更新、index.lock 的建删）每次都会写 `.git`，实测一轮刷新命令组产生
/// `index.lock` rename + `index` rename 事件。不过滤就是自激循环：
/// 刷新 → 写 `.git` → dir-change → 再刷新（"变更列表一直在刷新"的实测
/// 回归）。旧版非递归监听恰好把 `.git/*` 事件全部丢弃，顺带掩盖了它；
/// 改递归后暴露。`.git` 永远不出现在文件树里（read_directory 的忽略规则），
/// 过滤不损失任何树刷新语义；外部 git 操作改工作区文件时事件照常触发。
fn event_under_git_dir(event_path: &Path, root_norm: &Path) -> bool {
    event_path
        .strip_prefix(root_norm)
        .ok()
        .and_then(|rel| rel.components().next().map(|c| c.as_os_str() == ".git"))
        .unwrap_or(false)
}

/// `fs_watch_dirs` 的事件处理核心：`.git` 过滤 → 变更目录提取 → 合并入队。
///
/// 抽成独立函数是为了端到端测试能走**生产同款组合**（spawn_watcher 直通
/// 回调 + 本函数），而不是复刻一遍闭包逻辑让「测试只测了一半」。
fn process_dir_watch_event(
    event_path: &Path,
    roots: &[(PathBuf, String)],
    coalescer: &Arc<DirChangeCoalescer>,
) {
    for (root_norm, root_orig) in roots {
        if event_under_git_dir(event_path, root_norm) {
            continue;
        }
        if let Some(changed) = changed_dir_of(event_path, root_norm, root_orig) {
            coalescer.hit(changed);
        }
    }
}

/// 创建监听器并注册 `dirs`。
///
/// `mode` 决定递归语义：`fs_watch_files` 用 NonRecursive（父目录内命中即回调），
/// `fs_watch_dirs` 必须用 Recursive（见模块文档「目录监听必须是递归的」）。
///
/// 两种模式，由 `files` 是否为空决定：
/// - **非空**：事件路径命中注册文件时回调其注册原文（`fs_watch_files`）。
///   前端拿这个路径当缓冲键，所以必须返回注册形态而非事件形态。
/// - **空**：直通，目录内一切变更都回调事件路径本身（`fs_watch_dirs`）。
///   目录监听不关心「哪个文件变了」，只需要知道「这个目录变了」。
///
/// 拆成独立函数是为了能脱离 Tauri AppHandle 做集成测试（见 mod tests）——
/// 测试与命令走同一注册路径，避免「测试只测了一半」。
fn spawn_watcher(
    files: SharedFiles,
    dirs: impl IntoIterator<Item = PathBuf>,
    mode: RecursiveMode,
    on_change: impl Fn(PathBuf) + Send + 'static,
) -> Result<RecommendedWatcher, String> {
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        // 先收集命中再回调：不持锁进入调用方代码，避免回调里再碰集合时死锁
        let hits: Vec<PathBuf> = {
            let Ok(guard) = files.lock() else { return };
            // 空索引 = 直通模式（fs_watch_dirs 用）：目录内一切变更都上报。
            // 检查要放在持锁内：并发下「读长度为 0」与「逐个 lookup」之间
            // 若有人插入了条目，直通判定就失效了。
            if guard.is_empty() {
                event.paths.clone()
            } else {
                event
                    .paths
                    .iter()
                    .filter_map(|p| lookup_registered(p, &guard))
                    .collect()
            }
        };
        for path in hits {
            on_change(path);
        }
    })
    .map_err(|e| e.to_string())?;
    for dir in dirs {
        // 单个目录监听失败（已被删除等）不整体报错：那些文件只是收不到事件，
        // 让整个注册失败会连带正常文件也失去监听。
        let _ = watcher.watch(&dir, mode);
    }
    Ok(watcher)
}

/// 替换**该窗口**的监听集合为 `paths`（打开文件列表的全量快照）。
///
/// 全量替换而非增删 diff：集合只有几十条，重建的开销可以忽略，
/// 而 diff 需要额外状态与边界处理。
///
/// 失败时把旧 watcher 放回：否则 `remove` 之后 `spawn_watcher` 报错
/// （notify 的 inotify watch 数上限 ENOSPC / FSEvents 启动失败）会让该窗口
/// 从此没有任何监听，且只有等 openFilePaths 再次变化才重建 —— 期间外部
/// 修改（git checkout / 别的编辑器保存）全部收不到，编辑器静默显示陈旧内容。
#[tauri::command]
pub fn fs_watch_files(
    window: tauri::WebviewWindow,
    state: State<'_, FileWatchState>,
    registry: State<'_, WorkspaceRegistry>,
    paths: Vec<String>,
) -> Result<(), String> {
    if paths.len() > 2000 {
        return Err("打开文件过多，拒绝注册无界监听".to_string());
    }
    // 只监听已授权工作区内的路径：否则 renderer 可让内核监视 ~/.ssh 等目录。
    for p in &paths {
        authorize_workspace_path(&registry, p, false)?;
    }
    let label = window.label().to_string();
    // 先停本窗口的旧监听：切换项目/开关标签期间避免新旧两份事件叠加。
    // 其余窗口的条目不受影响（旧实现是进程级单槽，会连带顶掉它们）。
    // 取出后在锁外 drop：停止监听要回收 notify 的内部线程，不该卡住
    // 其它窗口的注册。
    let stale = state
        .files
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&label);

    let mut index = FileIndex::new();
    let mut dirs = HashSet::new();
    for p in &paths {
        let raw = PathBuf::from(p);
        register(&mut index, raw.clone());
        if let Some(parent) = raw.parent().filter(|d| !d.as_os_str().is_empty()) {
            dirs.insert(norm(parent));
        }
    }
    if index.is_empty() {
        // 没有可监听的文件（关闭项目 / 关掉全部标签）：旧监听已停，直接返回。
        // 语义正确 —— 无打开文件时不需要任何监听。
        return Ok(());
    }

    let shared: SharedFiles = Arc::new(Mutex::new(index));
    let emit_window = window.clone();
    let emit_label = label.clone();
    let watcher = spawn_watcher(shared, dirs, RecursiveMode::NonRecursive, move |path| {
        let payload = ExternalChangePayload {
            path: path.to_string_lossy().into_owned(),
        };
        // 只发给注册了这份文件集合的窗口：负载里的路径是该窗口的缓冲键，
        // 定向发送（emit 是全窗口广播，多窗口下会让无关窗口白跑处理）。
        let _ = emit_window.emit_to(emit_label.as_str(), EXTERNAL_CHANGE_EVENT, payload);
    });

    let watcher = match watcher {
        Ok(w) => w,
        Err(e) => {
            // 新监听没建成：放回旧的，至少维持失效前的可见性
            // （比「完全没有监听」好；下一轮 openFilePaths 变化仍会重建）。
            if let Some(old) = stale {
                if let Ok(mut guard) = state.files.lock() {
                    guard.insert(label.clone(), old);
                }
            }
            return Err(e);
        }
    };
    // 旧监听在这里才真正 drop（停止其 notify 线程）：新的已就绪，
    // 两者不重叠，也不会出现「新的建好了、旧的还没停」的双份事件窗口。
    drop(stale);

    state
        .files
        .lock()
        .map_err(|e| e.to_string())?
        .insert(label, watcher);
    Ok(())
}

/// 监听**目录**（项目根与追加的外部根），把目录子树内发生的文件系统变更
/// 以「变更目录」为粒度上报为 `fs://dir-change`。
///
/// 与 `fs_watch_files` 的分工：
/// - `fs_watch_files` 监听「打开中的文件」的父目录，事件精确到文件，前端据此
///   做外部修改判定（有脏缓冲时提示冲突）；
/// - 本命令关心「哪个目录的内容集合变了」，前端据此刷新文件树对应子树。
///
/// 为什么单独一条命令而不是把目录混进 `fs_watch_files`：两者的负载语义不同
/// （一个要给缓冲键、一个要给目录路径），前端处理路径也不同。混用会让前端
/// 无法判断该走外部修改判定还是树刷新。
///
/// **递归监听**：外部程序（构建工具、agent CLI）写入的文件可以出现在任意
/// 深度的子目录。notify 的 FSEvents 后端在 NonRecursive 下只放行根的直接
/// 子项事件（fsevent.rs 的 `parent_path == p` 过滤），`project/src/new.ts`
/// 会被 Rust 侧静默丢弃 —— 文件树就停在旧内容上（实测过的 bug），因此必须
/// Recursive。事件量由 [`DirChangeCoalescer`] 在 Rust 侧按目录合并。
///
/// 负载 `path` = 变更发生的目录（注册原文前缀形态，可能是根本身）：
/// - 根或外部根 → 前端整树/整根刷新（保持既有语义）；
/// - 根下子目录 → 前端只重读该目录并合并进树（节点未加载时忽略，
///   展开时会现读）。
#[tauri::command]
pub fn fs_watch_dirs(
    window: tauri::WebviewWindow,
    state: State<'_, FileWatchState>,
    registry: State<'_, WorkspaceRegistry>,
    paths: Vec<String>,
) -> Result<(), String> {
    if paths.len() > 2000 {
        return Err("监听目录过多，拒绝注册无界监听".to_string());
    }
    for p in &paths {
        if !p.trim().is_empty() {
            authorize_workspace_path(&registry, p, false)?;
        }
    }
    let label = window.label().to_string();
    let stale = state.dirs.lock().map_err(|e| e.to_string())?.remove(&label);

    // (规范化形态, 注册原文)：规范化形态用于事件前缀匹配（FSEvents 报的是
    // canonical 路径），原文用于拼负载（前端拿它与 workspacePath / 树节点比较，
    // 规范化后的形态如 /private/tmp 会匹配不上）。
    let mut roots: Vec<(PathBuf, String)> = Vec::new();
    let mut dirs: HashSet<PathBuf> = HashSet::new();
    for p in &paths {
        if p.trim().is_empty() {
            continue;
        }
        // norm：软链（/tmp → /private/tmp）与尾斜杠归一成同一形态，
        // 否则同一目录的重复形态会被当成两个 watcher。
        let normalized = norm(Path::new(p));
        if dirs.insert(normalized.clone()) {
            roots.push((normalized, p.clone()));
        }
    }
    if dirs.is_empty() {
        // 无目录可听（未打开项目）：旧监听已停，直接返回。
        return Ok(());
    }

    let emit_window = window.clone();
    let emit_label = label.clone();
    // 风暴退化哨兵：pending 超限时只留「第一个根」，前端整树刷新兜底。
    let fallback_root = roots[0].1.clone();
    let coalescer = Arc::new(DirChangeCoalescer::new(
        PathBuf::from(&fallback_root),
        Box::new(move |dir| {
            let payload = ExternalChangePayload {
                path: dir.to_string_lossy().into_owned(),
            };
            // 定向发送：emit 是全窗口广播，多窗口下会让无关窗口白跑树刷新。
            let _ = emit_window.emit_to(emit_label.as_str(), DIR_CHANGE_EVENT, payload);
        }),
    ));
    let coalescer_for_cb = Arc::clone(&coalescer);
    let watcher = spawn_watcher(
        Arc::new(Mutex::new(FileIndex::new())),
        dirs.iter().cloned().collect::<Vec<PathBuf>>(),
        RecursiveMode::Recursive,
        move |event_path| {
            // `.git` 先过滤、再按「变更目录」入队合并（见
            // process_dir_watch_event：刷新 → 写 .git → 事件 → 再刷新的
            // 自激循环防线，与嵌套根的多根匹配都在其中）。
            process_dir_watch_event(&event_path, &roots, &coalescer_for_cb);
        },
    );

    let watcher = match watcher {
        Ok(w) => w,
        Err(e) => {
            if let Some(old) = stale {
                if let Ok(mut guard) = state.dirs.lock() {
                    guard.insert(label.clone(), old);
                }
            }
            return Err(e);
        }
    };
    drop(stale);

    state
        .dirs
        .lock()
        .map_err(|e| e.to_string())?
        .insert(label, watcher);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个测试独立的临时目录，避免并行跑时互相踩
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("file_watch_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // ---- fs_watch_dirs 的行为契约 ----

    /// files 与 dirs 是两张独立的表。
    ///
    /// 共用一张表会让「换了个打开的标签页」把目录监听顶掉、反过来也成立 ——
    /// 这正是本需求要修的「文件树不自动同步」的根因之一。锁死分离。
    #[test]
    fn state_keeps_file_and_dir_watchers_separate() {
        let state = FileWatchState::default();
        if let Ok(mut files) = state.files.lock() {
            files.insert(
                "a".to_string(),
                spawn_watcher(
                    Arc::new(Mutex::new(FileIndex::new())),
                    std::iter::empty::<PathBuf>(),
                    RecursiveMode::NonRecursive,
                    |_| {},
                )
                .unwrap(),
            );
        }
        if let Ok(mut dirs) = state.dirs.lock() {
            dirs.insert(
                "a".to_string(),
                spawn_watcher(
                    Arc::new(Mutex::new(FileIndex::new())),
                    std::iter::empty::<PathBuf>(),
                    RecursiveMode::NonRecursive,
                    |_| {},
                )
                .unwrap(),
            );
        }
        assert_eq!(state.files.lock().map(|m| m.len()).unwrap_or(0), 1);
        assert_eq!(state.dirs.lock().map(|m| m.len()).unwrap_or(0), 1);

        // stop_for_window 必须同时清两张表：窗口销毁时漏任一张都会漏掉
        // notify 线程（表现为进程退不干净）。
        state.stop_for_window("a");
        assert_eq!(
            state.files.lock().map(|m| m.len()).unwrap_or(0),
            0,
            "files 应被清空"
        );
        assert_eq!(
            state.dirs.lock().map(|m| m.len()).unwrap_or(0),
            0,
            "dirs 应被清空"
        );
    }

    /// 目录里新增 / 删除文件都要触发事件。
    ///
    /// 这覆盖需求的两个方向：Finder 删文件 → 树移除该节点；Finder 加文件 →
    /// 树插入新节点。只测一侧会让另一侧悄悄失效。
    #[test]
    fn dir_change_fires_on_create_and_remove() {
        let dir = temp_dir("dirchange");
        let known = dir.join("known.txt");
        std::fs::write(&known, "x").unwrap();

        let (tx, rx) = std::sync::mpsc::channel();
        // 空 FileIndex = 不做任何路径过滤：目录内的一切变更都上报
        // （fs_watch_dirs 的语义就是不关心具体是哪个文件）。
        let shared: SharedFiles = Arc::new(Mutex::new(FileIndex::new()));
        let watcher = spawn_watcher(
            shared,
            std::iter::once(dir.clone()),
            RecursiveMode::NonRecursive,
            move |p| {
                let _ = tx.send(p);
            },
        )
        .expect("创建目录监听器");
        std::thread::sleep(std::time::Duration::from_millis(200));

        let dir_norm = norm(&dir);
        // 事件路径可能是注册形态或 canonical 形态（/var → /private/var），
        // 也可能就是被监听的目录本身：FSEvents 合并目录级变更时会这样上报。
        let in_watched = |p: &Path| {
            p == dir.as_path()
                || p == dir_norm.as_path()
                || p.starts_with(&dir)
                || p.starts_with(&dir_norm)
                || norm(p).starts_with(&dir_norm)
        };

        // 新增
        let created = dir.join("created.txt");
        std::fs::write(&created, "new").unwrap();
        let first = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("新增文件应触发目录变更");
        // 只断言「事件落在被监听目录里」：fs_watch_dirs 的消费者根本不看
        // 事件路径（收到任何一条就逐个根广播），FSEvents 上报子项或目录本身
        // 都满足契约；断言必须指向文件会把实现的时序细节当成语义。
        assert!(
            in_watched(&first),
            "新增事件应来自被监听目录，实际: {first:?}"
        );
        // 排空队列，避免上一条事件的余波污染下面的断言
        while rx.try_recv().is_ok() {}

        // 删除
        std::fs::remove_file(&created).unwrap();
        let second = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("删除文件应触发目录变更");
        // 文件已不存在，norm() 无法 canonicalize；注册形态与 canonical 形态
        // 都要能匹配上。
        assert!(
            in_watched(&second),
            "删除事件应来自被监听目录，实际: {second:?}"
        );

        drop(watcher);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 负载形状是前后端契约：前端按 `{ path }` 解构（fileWatch.ts 的
    /// ExternalChangePayload）。形状一漂移，事件就会在前端静默丢掉 —— 编一个
    /// 这里能挡住的回归。
    #[test]
    fn payload_serializes_as_path_object() {
        let json = serde_json::to_value(ExternalChangePayload {
            path: "/a/b.txt".into(),
        })
        .expect("序列化");
        assert_eq!(json, serde_json::json!({ "path": "/a/b.txt" }));
    }

    #[test]
    fn lookup_returns_registered_form_for_any_event_form() {
        let dir = temp_dir("match");
        let file = dir.join("a.txt");
        std::fs::write(&file, "x").unwrap();

        let mut index = FileIndex::new();
        register(&mut index, file.clone());

        assert_eq!(
            lookup_registered(&file, &index),
            Some(file.clone()),
            "原始形态命中并返回注册原文"
        );
        assert_eq!(
            lookup_registered(&norm(&file), &index),
            Some(file.clone()),
            "规范化形态命中也必须返回注册原文（前端按它查缓冲）"
        );
        assert_eq!(
            lookup_registered(&dir.join("other.txt"), &index),
            None,
            "同目录无关文件不应命中"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 核心链路：写盘 → 监听收到 → 回调带对路径。这就是前端能感知外部修改的全部依赖。
    #[test]
    fn write_to_watched_file_fires_callback() {
        let dir = temp_dir("write");
        let file = dir.join("t.txt");
        std::fs::write(&file, "before").unwrap();

        let mut index = FileIndex::new();
        register(&mut index, file.clone());
        let shared: SharedFiles = Arc::new(Mutex::new(index));
        let (tx, rx) = std::sync::mpsc::channel();
        let watcher = spawn_watcher(
            shared,
            std::iter::once(dir.clone()),
            RecursiveMode::NonRecursive,
            move |p| {
                let _ = tx.send(p);
            },
        )
        .expect("创建监听器并注册目录");

        // FSEvents 打包事件有延迟，先让监听生效再写
        std::thread::sleep(std::time::Duration::from_millis(200));
        std::fs::write(&file, "after").unwrap();

        let got = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("写盘后应收到外部修改事件");
        assert_eq!(got, file, "回调必须带注册时的原始路径（前端缓冲键）");

        drop(watcher);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 负例：同目录里未注册文件的变化不得触发回调，否则前端会被无关事件刷屏
    #[test]
    fn unwatched_sibling_file_is_ignored() {
        let dir = temp_dir("ignore");
        let watched = dir.join("watched.txt");
        let sibling = dir.join("sibling.txt");
        std::fs::write(&watched, "w").unwrap();
        std::fs::write(&sibling, "s").unwrap();

        let mut index = FileIndex::new();
        register(&mut index, watched.clone());
        let shared: SharedFiles = Arc::new(Mutex::new(index));
        let (tx, rx) = std::sync::mpsc::channel();
        let watcher = spawn_watcher(
            shared,
            std::iter::once(dir.clone()),
            RecursiveMode::NonRecursive,
            move |p| {
                let _ = tx.send(p);
            },
        )
        .expect("创建监听器并注册目录");

        // FSEvents 可能在监听建立后补发 watched.txt 的初始事件；这属于
        // 已注册文件，不是本测试要排除的 sibling。先给监听器时间稳定，
        // 再排空初始事件；随后只把 sibling 的事件视为失败。
        std::thread::sleep(std::time::Duration::from_millis(500));
        while rx.try_recv().is_ok() {}
        std::fs::write(&sibling, "changed").unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match rx.recv_timeout(remaining) {
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                Err(e) => panic!("等待期间通道异常：{e}"),
                Ok(p) if p == watched => {}
                Ok(p) => panic!("未注册文件不应触发回调，却收到 {p:?}"),
            }
        }

        drop(watcher);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- 递归目录监听（外部写入子目录文件的根因修复） ----

    /// 递归模式必须放行**深层**事件：外部程序往 `root/src/` 写新文件时，
    /// 事件路径是 `root/src/new.txt`（深度 2）。NonRecursive 的 FSEvents
    /// 过滤会把它丢掉（parent 是 `root/src` 不是 root）—— 这正是「文件树
    /// 不自动刷新」的根因。Recursive 模式下该事件必须到达回调。
    #[test]
    fn dir_watch_recursive_fires_for_nested_file() {
        let dir = temp_dir("recursive");
        let sub = dir.join("src");
        std::fs::create_dir_all(&sub).unwrap();

        let shared: SharedFiles = Arc::new(Mutex::new(FileIndex::new()));
        let (tx, rx) = std::sync::mpsc::channel();
        let watcher = spawn_watcher(
            shared,
            std::iter::once(dir.clone()),
            RecursiveMode::Recursive,
            move |p| {
                let _ = tx.send(p);
            },
        )
        .expect("创建递归监听器");
        std::thread::sleep(std::time::Duration::from_millis(200));

        let created = sub.join("new.txt");
        std::fs::write(&created, "x").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut got = false;
        while std::time::Instant::now() < deadline {
            match rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())) {
                Ok(p) if p.starts_with(norm(&dir)) => {
                    got = true;
                    break;
                }
                Ok(_) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                Err(e) => panic!("等待期间通道异常：{e}"),
            }
        }
        assert!(got, "深度 2 的新建文件必须触发事件（树自动刷新的根基）");

        drop(watcher);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 变更目录提取：事件路径 → 「内容变化的目录」（父目录语义，注册原文前缀）。
    #[test]
    fn changed_dir_maps_event_to_parent_dir() {
        let root_norm = Path::new("/tmp/proj");
        let root_orig = "/tmp/proj";
        // 深度 2 的文件 → 其父目录
        assert_eq!(
            changed_dir_of(Path::new("/tmp/proj/src/new.txt"), root_norm, root_orig),
            Some(PathBuf::from("/tmp/proj/src"))
        );
        // 深度 1 的文件 → 根本身
        assert_eq!(
            changed_dir_of(Path::new("/tmp/proj/top.txt"), root_norm, root_orig),
            Some(PathBuf::from("/tmp/proj"))
        );
        // 事件就是根（FSEvents 的目录级合并上报）→ 根本身
        assert_eq!(
            changed_dir_of(Path::new("/tmp/proj"), root_norm, root_orig),
            Some(PathBuf::from("/tmp/proj"))
        );
        // 不在根下：None
        assert_eq!(
            changed_dir_of(Path::new("/etc/hosts"), root_norm, root_orig),
            None
        );
        // 原文与 canonical 不同形（/var → /private/var）：负载必须回到原文形态
        assert_eq!(
            changed_dir_of(
                Path::new("/private/var/folders/proj/src/a.txt"),
                Path::new("/private/var/folders/proj"),
                "/var/folders/proj"
            ),
            Some(PathBuf::from("/var/folders/proj/src"))
        );
    }

    /// `.git` 子树事件必须被过滤（刷新自激循环的回归测试）。
    ///
    /// 应用自己的 git 刷新每次都写 `.git/index` / `.git/index.lock`；这些
    /// 事件对文件树毫无意义，放行就是「刷新 → 写 .git → 事件 → 再刷新」
    /// 的死循环（实测）。`.git` 永远不出现在文件树里，过滤零损失。
    #[test]
    fn git_dir_events_are_filtered() {
        let root_norm = Path::new("/tmp/proj");
        // .git 下任意深度都过滤
        assert!(event_under_git_dir(
            Path::new("/tmp/proj/.git/index"),
            root_norm
        ));
        assert!(event_under_git_dir(Path::new("/tmp/proj/.git"), root_norm));
        assert!(event_under_git_dir(
            Path::new("/tmp/proj/.git/refs/heads/main"),
            root_norm
        ));
        // 正常路径不过滤
        assert!(!event_under_git_dir(
            Path::new("/tmp/proj/src/new.txt"),
            root_norm
        ));
        assert!(!event_under_git_dir(
            Path::new("/tmp/proj/.github/ci.yml"),
            root_norm
        ));
        // 不在根下：false
        assert!(!event_under_git_dir(
            Path::new("/other/.git/index"),
            root_norm
        ));
        // 根本身：false（变更目录是根，照常上报）
        assert!(!event_under_git_dir(Path::new("/tmp/proj"), root_norm));
    }

    /// 端到端：递归监听下向 `<root>/.git/index` 写文件不得触发回调；
    /// 同期向正常子目录写文件必须触发（证明过滤器没有误伤）。
    #[test]
    fn dir_watch_ignores_git_dir_writes() {
        let dir = temp_dir("gitfilter");
        let git_dir = dir.join(".git");
        let src = dir.join("src");
        std::fs::create_dir_all(&git_dir).unwrap();
        std::fs::create_dir_all(&src).unwrap();

        let shared: SharedFiles = Arc::new(Mutex::new(FileIndex::new()));
        let (tx, rx) = std::sync::mpsc::channel();
        // 走生产同款组合：spawn_watcher 直通回调 + process_dir_watch_event
        // （.git 过滤 + 变更目录提取 + 合并），而不是复刻闭包逻辑。
        let roots = vec![(norm(&dir), dir.to_string_lossy().into_owned())];
        let coalescer = Arc::new(DirChangeCoalescer::new(
            dir.clone(),
            Box::new(move |p| {
                let _ = tx.send(p);
            }),
        ));
        let coalescer_for_cb = Arc::clone(&coalescer);
        let roots_for_cb = roots.clone();
        let watcher = spawn_watcher(
            shared,
            std::iter::once(dir.clone()),
            RecursiveMode::Recursive,
            move |p| process_dir_watch_event(&p, &roots_for_cb, &coalescer_for_cb),
        )
        .expect("创建递归监听器");
        std::thread::sleep(std::time::Duration::from_millis(200));

        // 先写 .git（模拟 git 刷新的 index 更新）：时限内不得有任何事件
        std::fs::write(git_dir.join("index"), "x").unwrap();
        std::fs::write(git_dir.join("index.lock"), "x").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1500);
        let mut git_leaked = false;
        while std::time::Instant::now() < deadline {
            match rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())) {
                Ok(p) if p.starts_with(&git_dir) => {
                    git_leaked = true;
                    break;
                }
                Ok(_) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                Err(e) => panic!("通道异常：{e}"),
            }
        }
        assert!(!git_leaked, ".git 子树的写盘事件必须被过滤");

        // 再写正常子目录：必须触发（过滤器没有连坐）。负载是「变更目录」
        // （事件路径的父目录、注册原文形态）。
        std::fs::write(src.join("new.txt"), "x").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut got_src = false;
        while std::time::Instant::now() < deadline {
            match rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())) {
                Ok(p) if p == src => {
                    got_src = true;
                    break;
                }
                Ok(_) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                Err(e) => panic!("通道异常：{e}"),
            }
        }
        assert!(got_src, "非 .git 的写盘必须照常触发且负载为变更目录");

        drop(watcher);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 合并发送：同目录多次变更窗口期内只发一条；flush 后可再次触发。
    #[test]
    fn coalescer_dedups_within_window_and_recovers() {
        let (tx, rx) = std::sync::mpsc::channel::<PathBuf>();
        let c = Arc::new(DirChangeCoalescer::new(
            PathBuf::from("/root"),
            Box::new(move |p| {
                let _ = tx.send(p);
            }),
        ));
        // 同一目录 hit 三次：只应发出一条
        c.hit(PathBuf::from("/root/src"));
        c.hit(PathBuf::from("/root/src"));
        c.hit(PathBuf::from("/root/src"));
        // 不同目录各发一条
        c.hit(PathBuf::from("/root/lib"));

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut got: Vec<PathBuf> = Vec::new();
        while std::time::Instant::now() < deadline && got.len() < 2 {
            match rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())) {
                Ok(p) => got.push(p),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                Err(e) => panic!("通道异常：{e}"),
            }
        }
        let mut sorted = got.clone();
        sorted.sort();
        assert_eq!(
            sorted,
            vec![PathBuf::from("/root/lib"), PathBuf::from("/root/src")],
            "同目录多次变更必须合并为一条，实际: {got:?}"
        );

        // flush 完成后再次 hit 仍要能发出（flusher 退出后的再调度）
        std::thread::sleep(std::time::Duration::from_millis(DIR_CHANGE_FLUSH_MS + 100));
        c.hit(PathBuf::from("/root/docs"));
        let again = rx.recv_timeout(std::time::Duration::from_secs(5));
        assert_eq!(
            again.ok(),
            Some(PathBuf::from("/root/docs")),
            "flusher 退出后的新变更必须重新调度发出"
        );
    }

    /// 容量退化：pending 超限时明细被丢弃、只留根哨兵。
    #[test]
    fn coalescer_falls_back_to_root_on_overflow() {
        let (tx, rx) = std::sync::mpsc::channel::<PathBuf>();
        let c = Arc::new(DirChangeCoalescer::new(
            PathBuf::from("/root"),
            Box::new(move |p| {
                let _ = tx.send(p);
            }),
        ));
        for i in 0..=DIR_CHANGE_PENDING_CAP {
            c.hit(PathBuf::from(format!("/root/dir{i}")));
        }
        let batch = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("应有 flush");
        assert_eq!(batch, PathBuf::from("/root"), "超限后必须退化为根哨兵");
        // 排空：不应还有第二条
        assert!(rx
            .recv_timeout(std::time::Duration::from_millis(DIR_CHANGE_FLUSH_MS + 200))
            .is_err());
    }
}
