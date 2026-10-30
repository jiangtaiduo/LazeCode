//! 打开文件的外部修改监听。
//!
//! 前端把「当前打开的文件列表」推给 `fs_watch_files`，这里监听这些文件的
//! **父目录**（非递归），把命中已注册文件的事件以 `fs://external-change`
//! 推回前端，由前端决定自动重载还是提示冲突。
//!
//! 为什么监听父目录而不是文件本身：git、部分编辑器用「写临时文件 + rename
//! 覆盖」保存，文件级监听挂在被换掉的旧 inode 上会静默失效，之后再也收不到
//! 事件。目录级监听天然覆盖原地写入与 rename 两种落盘方式。

use crate::commands::{authorize_workspace_path, WorkspaceRegistry};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
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

/// 创建监听器并注册 `dirs`（各打开文件的父目录，非递归）。
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
    on_change: impl Fn(PathBuf) + Send + 'static,
) -> Result<RecommendedWatcher, String> {
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        // 先收集命中再回调：不持锁进入调用方代码，避免回调里再碰集合时死锁
        let hits: Vec<PathBuf> = {
            let Ok(guard) = files.lock() else { return };
            // 空索引 = 直通模式（fs_watch_dirs 用）：目录内一切变更都上报。
            // 检出要放在持锁内：并发下「读长度为 0」与「逐个 lookup」之间
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
        let _ = watcher.watch(&dir, RecursiveMode::NonRecursive);
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
    let watcher = spawn_watcher(shared, dirs, move |path| {
        let payload = ExternalChangePayload {
            path: path.to_string_lossy().into_owned(),
        };
        // 只发给注册了这份文件集合的窗口：负载里的路径是该窗口的缓冲键，
        // 广播给别的窗口只会让它们查不到缓冲、白跑一遍处理。
        let _ = emit_window.emit(EXTERNAL_CHANGE_EVENT, payload);
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

/// 监听**目录**（项目根与追加的外部根），把目录内发生的任何文件系统变更
/// 上报为 `fs://dir-change`。
///
/// 与 `fs_watch_files` 的分工：
/// - `fs_watch_files` 监听「打开中的文件」的父目录，事件精确到文件，前端据此
///   做外部修改判定（有脏缓冲时提示冲突）；
/// - 本命令只关心「目录里多了/少了/改名了什么」，不关心谁的内容变了。它回答
///   的问题是「Finder 或别处动了这个项目吗」，前端据此刷新文件树。
///
/// 为什么单独一条命令而不是把目录混进 `fs_watch_files`：两者的负载语义不同
/// （一个要给缓冲键、一个要给目录路径），前端处理路径也不同。混用会让前端
/// 无法判断该走外部修改判定还是树刷新。
///
/// 非递归：只监听根目录自身。子目录的变更由前端在展开目录时按需补监听
/// （当前实现为惰性：展开了才重新调用本命令包含它）。
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

    let mut dirs: HashSet<PathBuf> = HashSet::new();
    for p in &paths {
        if p.trim().is_empty() {
            continue;
        }
        // norm：软链（/tmp → /private/tmp）与尾斜杠归一成同一形态，
        // 否则同一目录的重复形态会被当成两个 watcher。
        dirs.insert(norm(Path::new(p)));
    }
    if dirs.is_empty() {
        // 无目录可听（未打开项目）：旧监听已停，直接返回。
        return Ok(());
    }

    let emit_window = window.clone();
    // 上报的路径用注册时的原文：前端拿它与 workspacePath / 外部根直接比较，
    // 规范化后的形态（如 /private/tmp）会匹配不上。
    let originals: Vec<PathBuf> = paths
        .iter()
        .filter(|p| !p.trim().is_empty())
        .map(PathBuf::from)
        .collect();
    let watcher = spawn_watcher(
        Arc::new(Mutex::new(FileIndex::new())),
        dirs,
        move |_event_path| {
            // 事件落在哪个目录上不重要：前端只需要「这个项目的文件系统变了」。
            // 逐个根各发一条，让前端自己判断要不要刷（外部根与项目根的刷新
            // 代价不同，由前端决定）。
            for original in &originals {
                let payload = ExternalChangePayload {
                    path: original.to_string_lossy().into_owned(),
                };
                let _ = emit_window.emit(DIR_CHANGE_EVENT, payload);
            }
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
        let watcher = spawn_watcher(shared, std::iter::once(dir.clone()), move |p| {
            let _ = tx.send(p);
        })
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
        let watcher = spawn_watcher(shared, std::iter::once(dir.clone()), move |p| {
            let _ = tx.send(p);
        })
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
        let watcher = spawn_watcher(shared, std::iter::once(dir.clone()), move |p| {
            let _ = tx.send(p);
        })
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
}
