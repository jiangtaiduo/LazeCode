use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use lazecode_core::{AppModel, ProjectState};
use lazecode_fs::SearchHit;
use lazecode_terminal::{probe_busy_many, BusyTarget, Snapshot};
use lazecode_text::Cursor;

use crate::icons::Icon;
use crate::update_ui::UpdateState;

#[derive(Clone, Copy, Debug)]
pub struct TermSelection {
    /// Absolute grid line (negative = scrollback).
    pub start: (i32, usize),
    pub end: (i32, usize),
}

impl TermSelection {
    pub fn normalised(&self) -> ((i32, usize), (i32, usize)) {
        if (self.start.0, self.start.1) <= (self.end.0, self.end.1) {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        }
    }
}

/// Periodic `ps -t` result for one terminal pane.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerminalBusyState {
    pub busy: bool,
    pub outputting: bool,
    pub commands: Vec<String>,
    pub last_output_ms: Option<u64>,
}

/// In-flight one-off probe for a terminal-close confirmation.
struct PendingCloseProbe {
    action: TerminalCloseAction,
    pane_ids: Vec<u64>,
    rx: std::sync::mpsc::Receiver<Vec<(u64, TerminalBusyState)>>,
}

/// In-flight one-off probe for the quit guard.
struct PendingQuitProbe {
    pane_ids: Vec<u64>,
    rx: std::sync::mpsc::Receiver<Vec<(u64, TerminalBusyState)>>,
}

/// Action closed by the terminal confirmation modal.
#[derive(Clone, Debug)]
pub enum TerminalCloseAction {
    Pane(u64),
    Tab(u64),
    Others(u64),
    All,
    Project(usize),
}

/// Stable console-activity thresholds.
pub const OUTPUT_ACTIVE_MS: u64 = 2_000;
pub const INPUT_ECHO_GRACE_MS: u64 = 200;
pub const UI_REDRAW_GRACE_MS: u64 = 500;

/// Whether recent PTY output lights the project marquee: echo right after
/// input or a resize does not count, and a console already outputting stays
/// hot across a resize.
pub fn console_outputting(
    busy: bool,
    last_output_ms: Option<u64>,
    last_input_ms: Option<u64>,
    last_resize_ms: Option<u64>,
    was_outputting: bool,
) -> bool {
    if !busy {
        return false;
    }
    let Some(output_age) = last_output_ms.filter(|age| *age < OUTPUT_ACTIVE_MS) else {
        return false;
    };
    let after_input = last_input_ms.is_some_and(|input_age| {
        output_age <= input_age && input_age.saturating_sub(output_age) <= INPUT_ECHO_GRACE_MS
    });
    if after_input {
        return false;
    }
    let after_resize = last_resize_ms.is_some_and(|resize_age| {
        output_age <= resize_age && resize_age.saturating_sub(output_age) <= UI_REDRAW_GRACE_MS
    });
    !after_resize || was_outputting
}

/// Derived Git-log view: the filtered commit list, its lane layout and the
/// author-chip list. Recomputing all three per frame dominated the log view's
/// frame time (a regex filter recompiled the pattern per commit and the lane
/// layout cloned lane strings per row), so they are cached until the log, the
/// filter or the graph toggle changes.
pub struct GitLogCache {
    /// `project index | log generation | filter fingerprint | show graph`.
    pub key: String,
    pub filtered: Vec<lazecode_git::CommitInfo>,
    pub dag: lazecode_git::DagLayout,
    pub authors: Vec<String>,
}

/// Debounce window for terminal reflow while a pane is being resized.
const TERM_RESIZE_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(120);

/// Per-pane reflow gate. Resizing rewraps the whole scrollback and raises
/// SIGWINCH, so a drag that changes the size on every frame both hitches the
/// UI (proportional to the retained history) and storms full-screen TUIs with
/// repaints. The first layout applies immediately; later size changes apply
/// once the target has been stable for the debounce window.
#[derive(Clone, Copy, Debug)]
pub struct TermResizeGate {
    /// Size last handed to the session.
    applied: Option<(u16, u16)>,
    /// Newest target seen while a reflow is deferred.
    seen: Option<(u16, u16)>,
    seen_at: std::time::Instant,
}

impl Default for TermResizeGate {
    fn default() -> Self {
        Self {
            applied: None,
            seen: None,
            seen_at: std::time::Instant::now(),
        }
    }
}

impl TermResizeGate {
    /// Feed the pane's current target; true when the session should reflow now.
    pub fn should_apply(&mut self, target: (u16, u16)) -> bool {
        if self.seen != Some(target) {
            self.seen = Some(target);
            self.seen_at = std::time::Instant::now();
            if self.applied.is_none() {
                self.applied = Some(target);
                return true;
            }
            return false;
        }
        if self.applied == Some(target) {
            return false;
        }
        if self.seen_at.elapsed() >= TERM_RESIZE_DEBOUNCE {
            self.applied = Some(target);
            true
        } else {
            false
        }
    }

    /// Time left before the deferred target may be applied.
    pub fn remaining(&self) -> std::time::Duration {
        TERM_RESIZE_DEBOUNCE.saturating_sub(self.seen_at.elapsed())
    }

    /// Forget the applied size (a different session now owns this pane id).
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// One background scan for the terminal find bar.
struct PendingTermSearch {
    query: String,
    rx: std::sync::mpsc::Receiver<Vec<lazecode_terminal::TerminalMatch>>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// Find-in-terminal state. The scan walks the whole retained history, so it
/// runs on a worker after an input debounce instead of inline on the UI
/// thread (one full pass per keystroke cost ~70ms at 43k lines and seconds at
/// the cold byte cap).
#[derive(Default)]
pub struct TermSearchState {
    pub open: bool,
    pub query: String,
    pub matches: Vec<lazecode_terminal::TerminalMatch>,
    pub index: usize,
    pub focus_requested: bool,
    /// Earliest time the next scan may start (input debounce).
    pending_at: Option<std::time::Instant>,
    /// Scan currently running; superseding it cancels the worker.
    inflight: Option<PendingTermSearch>,
}

/// Input debounce for the find bar: one scan per typing burst.
const TERM_SEARCH_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(120);

impl TermSearchState {
    /// Open the find bar with an empty query.
    pub fn open_bar(&mut self) {
        self.open = true;
        self.focus_requested = true;
        self.query.clear();
        self.index = 0;
        self.matches.clear();
        self.pending_at = None;
        self.cancel_inflight();
    }

    /// Close the find bar: drop results and stop any running scan.
    pub fn close_bar(&mut self) {
        self.open = false;
        self.pending_at = None;
        self.cancel_inflight();
        self.matches.clear();
        self.index = 0;
    }

    /// Record a query edit: restart the debounce window.
    pub fn query_changed(&mut self) {
        self.index = 0;
        self.matches.clear();
        self.pending_at = Some(std::time::Instant::now() + TERM_SEARCH_DEBOUNCE);
        self.cancel_inflight();
    }

    /// Start the debounced scan and apply finished results. Returns whether
    /// `matches` changed.
    pub fn pump_search(
        &mut self,
        session: &lazecode_terminal::Session,
        ctx: &egui::Context,
    ) -> bool {
        let mut changed = false;
        if self.query.trim().is_empty() {
            self.pending_at = None;
            self.cancel_inflight();
            if !self.matches.is_empty() {
                self.matches.clear();
                changed = true;
            }
            return changed;
        }
        if let Some(at) = self.pending_at {
            let now = std::time::Instant::now();
            if now >= at {
                self.pending_at = None;
                self.cancel_inflight();
                let (rx, cancel) = session.search_async(self.query.clone());
                self.inflight = Some(PendingTermSearch {
                    query: self.query.clone(),
                    rx,
                    cancel,
                });
            } else {
                ctx.request_repaint_after(at - now + std::time::Duration::from_millis(1));
            }
        }
        if let Some(pending) = self.inflight.take() {
            match pending.rx.try_recv() {
                Ok(matches) => {
                    if pending.query == self.query {
                        self.matches = matches;
                        self.index = 0;
                        changed = true;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.inflight = Some(pending);
                    ctx.request_repaint_after(std::time::Duration::from_millis(16));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {}
            }
        }
        changed
    }

    fn cancel_inflight(&mut self) {
        if let Some(pending) = self.inflight.take() {
            pending
                .cancel
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

impl Drop for TermSearchState {
    fn drop(&mut self) {
        self.cancel_inflight();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKind {
    /// 提交抽屉 ⋮ 扩展菜单（贮藏 / 架藏库入口）。
    CommitExtras,
    /// 提交抽屉 👁 视图选项（按目录分组 / 显示被忽略的文件）。
    CommitViewOptions,
    /// 比较抽屉 👁 视图选项（按目录分组）。
    CompareViewOptions {
        project: usize,
    },
    ShelfContext {
        project: usize,
    },
    /// 贮藏条目右键菜单：弹出 / 应用 / Unstash… / 丢弃 / 清空 / 显示差异。
    StashEntryMenu {
        project: usize,
    },
    /// Stash 页签文件/目录行右键菜单（payload = `<stash>\x01<f|d>\x01<path>`）。
    StashFileMenu {
        project: usize,
    },
    Project,
    Branch,
    Worktree,
    LogBranchFilter {
        project: usize,
    },
    /// Git log date filter dropdown menu.
    LogDateFilter {
        project: usize,
    },
    LogCommitMenu {
        project: usize,
    },
    /// Git log branch tree context menu (payload: name \u{1} "remote"|"local").
    BranchContext {
        project: usize,
    },
    /// Tag row context menu (payload = tag name).
    TagContext {
        project: usize,
    },
    /// Project tree right-click context menu (payload: relative path).
    TreeContext {
        project: usize,
        is_dir: bool,
    },
    /// Editor tab right-click context menu (payload: path).
    EditorTabContext {
        project: usize,
        tab_id: u64,
    },
    /// Terminal tab right-click context menu (payload: title).
    TerminalTabContext {
        project: usize,
        /// Tab owning the menu.
        tab_id: u64,
        /// Active pane of that tab (copy cwd / rename / close pane targets).
        terminal_id: u64,
    },
    /// 提交信息历史下拉（dock 内时钟按钮）：列出 settings.commit_history。
    CommitHistory {
        project: usize,
    },
    /// 分支比较对话框的 User 过滤菜单（side: 0=left 1=right）。
    CompareUserFilter {
        side: u8,
    },
    /// 分支比较对话框的 Date 过滤菜单。
    CompareDateFilter {
        side: u8,
    },
    /// 分支比较对话框的 Branch(ref) 过滤菜单。
    CompareRefFilter {
        side: u8,
    },
    /// Changelist group header context menu (payload: changelist name).
    ChangelistHeaderContext {
        project: usize,
    },
    /// Changelist file row context menu (payload: file relative path).
    ChangeFileContext {
        project: usize,
    },
    /// Git log sort and options menu (by date/topo, first-parent, no-merges).
    LogSortOptions {
        project: usize,
    },
    /// Git log path filter input popover.
    LogPathFilter {
        project: usize,
    },
    /// Git commit details file row context menu (payload: commit_id \u{1} path).
    CommitDetailsFileContext {
        project: usize,
    },
    /// Code editor context menu (payload: file path).
    EditorContext {
        project: usize,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GitToolsTab {
    #[default]
    Submodules,
    Lfs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RebaseTodoRow {
    pub action: String,
    pub commit: String,
    pub message: String,
}

pub struct MenuState {
    pub kind: MenuKind,
    pub anchor: egui::Rect,
    /// Selection context (e.g. the commit a context menu targets).
    pub payload: Option<String>,
    pub opened_frame: u64,
}

pub enum DialogState {
    None,
    NewBranch {
        value: String,
        start_point: Option<String>,
        checkout: bool,
        error: Option<String>,
    },
    DeleteBranch {
        name: String,
        force: bool,
        remote: bool,
        error: Option<String>,
    },
    RenameBranch {
        from: String,
        value: String,
        error: Option<String>,
    },
    Push {
        remote: String,
        branch: String,
        set_upstream: bool,
        force: bool,
        push_tags: bool,
        remotes: Vec<lazecode_git::RemoteInfo>,
        pending: Vec<lazecode_git::CommitInfo>,
        status: Option<String>,
    },
    Pull {
        mode: lazecode_git::PullMode,
        status: Option<String>,
    },
    Remotes {
        remotes: Vec<lazecode_git::RemoteInfo>,
        new_name: String,
        new_url: String,
        editing: Option<String>,
        error: Option<String>,
    },
    Reset {
        hash: String,
        mode: lazecode_git::ResetMode,
    },
    NewTag {
        commit: String,
        value: String,
        error: Option<String>,
    },
    NewWorktree {
        name: String,
        branch: String,
        create_new: bool,
        location: String,
        error: Option<String>,
    },
    CheckoutRevision {
        value: String,
        error: Option<String>,
    },
    RemoveWorktree {
        name: String,
        worktree_path: std::path::PathBuf,
        dirty: bool,
        changes: usize,
    },
    CloseDirtyTab {
        tab_id: u64,
        name: String,
    },
    /// A terminal tab/pane/project still has commands running; confirm before
    /// killing them.
    ConfirmTerminalClose {
        action: TerminalCloseAction,
        commands: Vec<String>,
    },
    NewFile {
        dir: std::path::PathBuf,
        value: String,
        error: Option<String>,
    },
    NewDirectory {
        dir: std::path::PathBuf,
        value: String,
        error: Option<String>,
    },
    RenamePath {
        old_path: std::path::PathBuf,
        value: String,
        error: Option<String>,
    },
    DeletePath {
        path: std::path::PathBuf,
        is_dir: bool,
    },
    /// 未跟踪文件批量 Delete…：全部移入系统废纸篓（对齐稳定版
    /// trashUnversionedConfirm；单文件走 DeletePath 复用同一确认）。
    ConfirmDeleteUntracked {
        paths: Vec<std::path::PathBuf>,
    },
    /// 删除架藏前确认（稳定版删除架藏/架藏文件都有确认；此前直接删）。
    ConfirmDeleteShelf {
        name: String,
        /// true = 「最近删除」中的彻底删除（不可恢复）。
        purge: bool,
    },
    ConflictResolve {
        path: String,
        ours: String,
        theirs: String,
        worktree: String,
    },
    FileHistory {
        path: String,
        entries: Vec<lazecode_git::HistoryEntry>,
        selected: usize,
        /// 首帧已加载过（空列表≠未加载，避免每帧重复起 git 子进程）。
        loaded: bool,
    },
    GitTools {
        tab: GitToolsTab,
        submodules: Vec<lazecode_git::SubmoduleEntry>,
        lfs_status: Option<lazecode_git::LfsStatus>,
        /// 两个页签各加载一次（空子模块列表≠未加载，避免每帧 spawn git）。
        loaded: (bool, bool),
    },
    CloneRepo {
        url: String,
        dest: String,
        error: Option<String>,
    },
    InteractiveRebase {
        onto: String,
        todos: Vec<RebaseTodoRow>,
    },
    Squash {
        commits: Vec<String>,
        value: String,
        error: Option<String>,
    },
    /// 架藏保存：`paths` 为勾选/选中的文件（空=全部已跟踪变更）。
    NewShelf {
        value: String,
        error: Option<String>,
        paths: Vec<String>,
    },
    /// 重命名架藏。
    RenameShelf {
        from: String,
        value: String,
        error: Option<String>,
    },
    GoToLine {
        value: String,
        error: Option<String>,
    },
    RenameTerminal {
        terminal_id: u64,
        value: String,
        error: Option<String>,
    },
    /// LSP `textDocument/rename` (F2). `path`/`line`/`col` pin the symbol;
    /// `value` is the new name.
    RenameSymbol {
        tab_id: u64,
        path: String,
        line: usize,
        col: usize,
        value: String,
        error: Option<String>,
    },
    NewChangelist {
        value: String,
        error: Option<String>,
    },
    /// Stash 按钮（对齐稳定版 StashDialog）：说明 + 是否含未跟踪文件。
    StashCreate {
        message: String,
        include_untracked: bool,
        error: Option<String>,
    },
    /// Unstash… 对话框：当前分支、可选新分支、弹出 / 恢复索引。
    Unstash {
        stash: String,
        /// 贮藏说明（对话框标题）。
        message: String,
        /// 当前分支（只读行）。
        branch: String,
        as_new_branch: String,
        pop: bool,
        reinstate_index: bool,
        error: Option<String>,
    },
    StashManager {
        stashes: Vec<lazecode_git::StashEntry>,
        selected: usize,
        /// 列表加载失败原因（原先静默置空，用户无从感知）。
        error: Option<String>,
        /// 首帧已加载过（空列表≠未加载，避免每帧重复 spawn git）。
        loaded: bool,
    },
    /// Discard（Rollback）前的统一确认弹窗。
    ConfirmDiscard {
        paths: Vec<String>,
    },
    /// 删除变更列表确认：移入默认列表 or 回滚（丢弃）其文件更改。
    DeleteChangelist {
        name: String,
        count: usize,
        rollback: bool,
    },
    /// Unshelve（对齐稳定版）：逐文件勾选 + 目标 changelist（现有列表或
    /// 新建）。`files` 为 (path, selected)。
    Unshelve {
        name: String,
        files: Vec<(String, bool)>,
        target: Option<String>,
        new_name: String,
        error: Option<String>,
    },
    /// 右键「Commit File(s)...」：为显式路径集输入提交信息。
    CommitFiles {
        paths: Vec<String>,
        value: String,
        error: Option<String>,
    },
    /// 「Get from Revision」：输入 revision，把单个路径恢复为该版本。
    GetFileFromRevision {
        path: String,
        revision: String,
        error: Option<String>,
    },
    /// 把选中的本地更改提取成一个独立提交（其余更改保持未提交）。
    ExtractChanges {
        paths: Vec<String>,
        value: String,
        error: Option<String>,
    },
    /// 把选中的本地更改拣选到另一个分支的工作区。
    CherryPickChanges {
        paths: Vec<String>,
        target: String,
        error: Option<String>,
    },
    ShelfManager {
        shelves: Vec<lazecode_git::ShelfEntry>,
        selected: usize,
        /// 列表加载失败原因（原先静默置空，用户无从感知）。
        error: Option<String>,
        /// 首帧已加载过（空列表≠未加载，避免每帧重复 spawn git）。
        loaded: bool,
    },
    Quit {
        commands: Vec<String>,
        /// Set when quitting is the last step of applying an update; the
        /// confirmation handler relaunches the installed build instead of
        /// only exiting.
        restart_update: bool,
    },
}

#[derive(Default)]
pub struct FindState {
    pub open: bool,
    pub last_revision: u64,
    pub last_signature: String,
    pub query: String,
    pub replacement: String,
    pub case_sensitive: bool,
    pub regex: bool,
    pub whole_word: bool,
    pub matches: Vec<(Cursor, Cursor)>,
    pub index: usize,
    pub focus_requested: bool,
    pub opened_frame: u64,
}

#[derive(Default)]
pub struct SearchState {
    pub open: bool,
    pub query: String,
    pub replacement: String,
    pub replace_mode: bool,
    pub file_mask: String,
    pub case_sensitive: bool,
    pub regex: bool,
    pub whole_word: bool,
    pub results: Vec<SearchHit>,
    pub selected: usize,
    pub searched: bool,
    pub focus_requested: bool,
    pub request_id: u64,
    pub opened_frame: u64,
    /// When the query/flags last changed; drives the short debounce before
    /// the async filesystem search starts.
    pub dirty_since: Option<f64>,
    /// Query+flags signature represented by the current results.
    pub signature: String,
    pub searching: bool,
}

#[derive(Default)]
pub struct PaletteState {
    pub query: String,
    pub selected: usize,
    pub focus_requested: bool,
    pub opened_frame: u64,
}

#[derive(Default)]
pub struct LspHoverState {
    pub tab_id: u64,
    pub line: usize,
    pub col: usize,
    pub pointer: egui::Pos2,
    /// When the pointer settled on this character.
    pub since: Option<Instant>,
    pub request: Option<i64>,
    pub requested_at: Option<Instant>,
    pub result: Option<lazecode_core::lsp::Hover>,
}

#[derive(Default)]
pub struct LspCompletionState {
    pub tab_id: u64,
    pub request: Option<i64>,
    pub requested_at: Option<Instant>,
    pub items: Vec<lazecode_core::lsp::CompletionItem>,
    pub selected: usize,
    /// Start of the word prefix that `insert_text` replaces.
    pub prefix_start: Cursor,
    /// Cursor and revision when the request was sent; a response that arrives
    /// after the user moved or typed is stale and must not be inserted.
    pub request_cursor: Cursor,
    pub revision: u64,
    /// Caret screen position, refreshed by the editor while the popup is open.
    pub anchor: egui::Pos2,
}

#[derive(Default)]
pub struct LspSymbolState {
    pub tab_id: u64,
    pub request: Option<i64>,
    pub requested_at: Option<Instant>,
    pub items: Vec<lazecode_core::lsp::DocumentSymbol>,
    pub selected: usize,
}

/// Cross-frame state for the optional language-server features. Requests are
/// ids into `AppModel::lsp`; completed responses are polled here each frame.
#[derive(Default)]
pub struct LspUiState {
    pub hover: Option<LspHoverState>,
    pub completion: Option<LspCompletionState>,
    pub symbols: Option<LspSymbolState>,
    /// Set by the editor after a completion trigger character.
    pub completion_due: Option<(u64, Instant)>,
    /// Set by Cmd+Click / F12; sent after the current input batch.
    pub goto_request: Option<(u64, Cursor)>,
    /// Set by Option+Cmd+L.
    pub format_request: Option<u64>,
    /// Set by Cmd+Shift+O.
    pub symbols_request: Option<u64>,
    pub goto_pending: Option<i64>,
    pub format_pending: Option<(u64, i64)>,
    pub rename_pending: Option<i64>,
}

pub struct UiRuntime {
    pub lsp: LspUiState,
    pub palette: PaletteState,
    pub search: SearchState,
    pub find: FindState,
    pub menu: Option<MenuState>,
    pub dialog: DialogState,
    /// One-off close/quit `ps` probes running off-thread, drained by the
    /// `poll_*_probe` helpers.
    pending_close_probe: Option<PendingCloseProbe>,
    pending_quit_probe: Option<PendingQuitProbe>,
    pub terminal_snapshots: HashMap<u64, Snapshot>,
    pub terminal_selections: HashMap<u64, Option<TermSelection>>,
    pub terminal_focus: Option<u64>,
    pub terminal_search: HashMap<u64, TermSearchState>,
    pub terminal_drag: Option<u64>,
    pub terminal_scroll_fraction: HashMap<u64, f32>,
    pub terminal_preedit: HashMap<u64, String>,
    /// Fractional trackpad wheel remainder per terminal session.
    pub terminal_wheel: HashMap<u64, f32>,
    /// Reflow debounce per pane (a drag must not rewrap the scrollback per frame).
    pub terminal_resize: HashMap<u64, TermResizeGate>,
    /// Last snapshot wake counter per terminal; skips rebuilding unchanged grids.
    pub terminal_wake: HashMap<u64, u64>,
    /// egui textures for each pane's kitty graphics images.
    pub terminal_graphics: HashMap<u64, crate::terminal::GraphicsCache>,
    /// Per-pane process activity from the 2s background probe.
    pub terminal_busy: HashMap<u64, TerminalBusyState>,
    /// Filtered Git-log view + lane layout, reused until its inputs change.
    pub git_log_cache: Option<GitLogCache>,
    /// Set by the app shell: wakes the event loop when an off-thread close/
    /// quit probe finishes, so the confirmation dialog lands next frame
    /// instead of waiting for the next poll deadline.
    pub wake_notifier: Option<std::sync::Arc<dyn lazecode_terminal::Notifier>>,
    pub editor_focus: Option<u64>,
    pub editor_drag: bool,
    /// Vertical scroll offset of the project rail (0 = newest/top).
    pub project_rail_scroll: f32,
    /// Lazily uploaded titlebar copy of the packaged application icon.
    pub app_icon: Option<egui::TextureHandle>,
    /// Titlebar control rectangles in egui/window coordinates. The macOS
    /// native `zoom:` guard uses these to keep double-clicks on buttons from
    /// reaching AppKit's titlebar zoom.
    pub titlebar_controls: Vec<egui::Rect>,
    pub sidebar_drag: bool,
    pub panel_drag: bool,
    /// 分支比较页：pane 高度分隔条拖拽中（松手时落盘 settings）。
    /// Diff 页边缘气球：文案 + 出现时刻（1.8s 后消失）。
    pub diff_balloon: Option<(String, std::time::Instant)>,
    /// Diff 页导航后的滚动定位请求（tab id, 组序号）。
    pub diff_scroll: Option<(u64, usize)>,
    /// Diff 视图模式下拉打开中的 tab。
    pub diff_mode_open: Option<u64>,
    /// Compare 抽屉视图选项弹层。
    pub cmp_view_menu_open: bool,
    /// 分支比较页：每侧的 DAG 图开关与详情根行折叠（会话内瞬时态）。
    pub cb_show_graph: [bool; 2],
    pub cb_root_collapsed: [bool; 2],
    pub compare_split_drag: bool,
    /// 分支比较页：详情列宽度分隔条拖拽中。
    pub compare_details_drag: bool,
    pub settings_open: bool,
    /// Settings changed the UI/editor/terminal font: reinstall before the
    /// next frame so egui does not keep galleys from the old atlas.
    pub fonts_dirty: bool,
    pub about_open: bool,
    /// Software update window (settings → Updates / auto-check notice).
    pub update_open: bool,
    /// Update endpoint results, in-flight job and staging status.
    pub update: UpdateState,
    pub applied_theme: Option<(lazecode_core::ThemeMode, lazecode_core::ThemeSkin)>,
    pub last_frame: Instant,
    pub md_cache: egui_commonmark::CommonMarkCache,
    /// Per-tab rendered-Markdown scroll offset, in points.
    pub md_scroll: HashMap<u64, f32>,
    /// Bounded decoded-image cache for image tabs (not `egui_extras`' global
    /// loader cache, which never evicts).
    pub image_cache: crate::image_cache::ImageCache,
    /// Open image paths, used to drop textures as soon as a tab closes.
    pub media_project_roots: Vec<PathBuf>,
    pub media_markdown_paths: HashSet<PathBuf>,
    pub media_gif_paths: HashSet<PathBuf>,
    pub image_zoom: HashMap<u64, f32>,
    pub cursor_epoch: f64,
    pub agent_menu: Option<egui::Rect>,
    pub agent_menu_frame: u64,
    /// Per-frame rects of open popovers, pushed by the overlay draw code and
    /// taken by the shell at the start of the next frame: egui routes the
    /// wheel by rect containment only (no z-order test), so panels need to
    /// know where an overlay drawn later in the frame will land.
    pub menu_hit_rects: Vec<egui::Rect>,
    pub frame_count: u64,
    pub branch_create_error: Option<String>,
    pub pending_open: Option<(std::path::PathBuf, usize, usize)>,
    pub toast_created: f64,
    pub last_revealed_tab: Option<u64>,
    pub last_revealed_cmp_file: Option<String>,
    pub ime_preedit: String,
    /// True while a Chinese/Japanese/Korean IME owns an uncommitted
    /// composition. Enter must select the candidate, not confirm the dialog
    /// or insert a newline.
    pub ime_composing: bool,
    /// Whether the previous frame had a surface asking egui for IME input.
    /// A composition cannot outlive that focus: winit clears its marked text
    /// on `set_ime_allowed(false)` (and on input-source changes) without
    /// emitting `Preedit("")`/`Commit`, and egui-winit drops `Ime::Disabled`,
    /// so the filter itself must treat the next frame as composition-free.
    pub ime_focus_seen: bool,
    // --- 提交抽屉（稳定版 CommitDrawer）状态 ---
    /// Commit / Shelf 页签。
    pub commit_tab: CommitTab,
    /// 稳定版 ^P：Group by Directory（树视图）。
    pub commit_tree_view: bool,
    /// 稳定版：是否展示被 .gitignore 忽略的文件。
    pub commit_show_ignored: bool,
    /// 工具栏眼睛视图选项弹层开关。
    pub commit_view_menu_open: bool,
    /// Shelf 页签数据（页签激活时同步加载：读本地补丁文件，无子进程）。
    pub shelf_entries: Vec<lazecode_git::ShelfEntry>,
    pub shelf_deleted: Vec<lazecode_git::ShelfEntry>,
    /// 已加载的架藏数据所属仓库（切换项目后重新加载）。
    pub shelf_loaded: Option<std::path::PathBuf>,
    /// 架藏名 → 文件列表（加载时从补丁解析）。
    pub shelf_files_map: HashMap<String, Vec<String>>,
    pub shelf_selected: Option<String>,
    /// 选中的架藏文件（仓库相对路径，单架藏内多选）。
    pub shelf_file_sel: Vec<String>,
    // --- Stash 页签状态 ---
    /// 贮藏列表（抽屉可见时按仓库加载；决定页签是否显示）。
    pub stash_entries: Vec<lazecode_git::StashEntry>,
    /// 已加载贮藏数据所属仓库（切换项目或 git 变更后重新加载）。
    pub stash_loaded: Option<std::path::PathBuf>,
    /// 当前选中的贮藏，如 `stash@{0}`。
    pub stash_selected: Option<String>,
    /// 展开的贮藏条目（`stash@{n}`）。每个条目独立展开/折叠；首帧只展开
    /// 第一条，避免多条贮藏各自铺开整棵文件树。
    pub stash_expanded: std::collections::HashSet<String>,
    /// 折叠的树根/目录键（`<stash>:tracked:…` / `<stash>:untracked:…`；
    /// 默认展开）。
    pub stash_collapsed: std::collections::HashSet<String>,
    /// 选中的贮藏文件（仓库相对路径）。
    pub stash_file_sel: Option<String>,
    pub commit_view_menu_frame: u64,
    pub cmp_view_menu_frame: u64,
    pub new_ignore_pattern: String,
    pub tree_submenu: Option<(String, egui::Rect)>,
    pub branch_submenu: Option<BranchSubmenuState>,
    pub branch_sections_collapsed: std::collections::HashSet<String>,
    /// 控制台侧栏两级分组的折叠键（`p:<项目根>` / `w:<worktree 路径>`；
    /// 默认为空，即全部展开）。
    pub consoles_collapsed: std::collections::HashSet<String>,
    /// 设置页快捷键录入态：`Some(动作)` = 等待用户按下新组合键。
    pub keybind_capture: Option<crate::keybindings::Action>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BranchSubmenuState {
    pub branch_name: String,
    pub is_current: bool,
    pub is_remote: bool,
    pub anchor: egui::Rect,
}

/// CommitDrawer 顶部页签（稳定版 tabCommit / tabShelf + 贮藏）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommitTab {
    Commit,
    Shelf,
    /// 仅在仓库存在 stash 记录时显示。
    Stash,
}

impl Default for UiRuntime {
    fn default() -> Self {
        Self {
            lsp: LspUiState::default(),
            palette: PaletteState::default(),
            search: SearchState::default(),
            find: FindState::default(),
            menu: None,
            dialog: DialogState::None,
            pending_close_probe: None,
            pending_quit_probe: None,
            terminal_snapshots: HashMap::new(),
            terminal_selections: HashMap::new(),
            terminal_focus: None,
            terminal_search: HashMap::new(),
            terminal_drag: None,
            terminal_scroll_fraction: HashMap::new(),
            terminal_preedit: HashMap::new(),
            terminal_wheel: HashMap::new(),
            terminal_resize: HashMap::new(),
            terminal_wake: HashMap::new(),
            terminal_graphics: HashMap::new(),
            terminal_busy: HashMap::new(),
            git_log_cache: None,
            wake_notifier: None,
            new_ignore_pattern: String::new(),
            commit_view_menu_frame: 0,
            cmp_view_menu_frame: 0,
            editor_focus: None,
            editor_drag: false,
            project_rail_scroll: 0.0,
            app_icon: None,
            titlebar_controls: Vec::new(),
            sidebar_drag: false,
            panel_drag: false,
            diff_balloon: None,
            diff_scroll: None,
            diff_mode_open: None,
            cmp_view_menu_open: false,
            cb_show_graph: [true, true],
            cb_root_collapsed: [false, false],
            compare_split_drag: false,
            compare_details_drag: false,
            settings_open: false,
            fonts_dirty: false,
            about_open: false,
            update_open: false,
            update: UpdateState::default(),
            applied_theme: None,
            last_frame: Instant::now(),
            cursor_epoch: 0.0,
            md_cache: egui_commonmark::CommonMarkCache::default(),
            md_scroll: HashMap::new(),
            image_cache: crate::image_cache::ImageCache::default(),
            media_project_roots: Vec::new(),
            media_markdown_paths: HashSet::new(),
            media_gif_paths: HashSet::new(),
            image_zoom: HashMap::new(),
            agent_menu: None,
            agent_menu_frame: 0,
            menu_hit_rects: Vec::new(),
            frame_count: 0,
            branch_create_error: None,
            pending_open: None,
            toast_created: 0.0,
            last_revealed_tab: None,
            last_revealed_cmp_file: None,
            ime_preedit: String::new(),
            ime_composing: false,
            ime_focus_seen: false,
            commit_tab: CommitTab::Commit,
            commit_tree_view: false,
            commit_show_ignored: false,
            commit_view_menu_open: false,
            shelf_entries: Vec::new(),
            shelf_deleted: Vec::new(),
            shelf_loaded: None,
            shelf_files_map: HashMap::new(),
            shelf_selected: None,
            shelf_file_sel: Vec::new(),
            stash_entries: Vec::new(),
            stash_loaded: None,
            stash_selected: None,
            stash_expanded: std::collections::HashSet::new(),
            stash_collapsed: std::collections::HashSet::new(),
            stash_file_sel: None,
            tree_submenu: None,
            branch_submenu: None,
            branch_sections_collapsed: std::collections::HashSet::new(),
            consoles_collapsed: std::collections::HashSet::new(),
            keybind_capture: None,
        }
    }
}

impl UiRuntime {
    /// Chinese/Japanese/Korean IMEs can report the Enter that leaves a
    /// composition as a plain key event without an `ImeEvent::Commit` (macOS
    /// Shuangpin does). Every text surface treats Enter as "confirm", so when
    /// a composition is active: commit the preedit text into the focused
    /// field, drop the key, and let the next Enter confirm the dialog.
    ///
    /// If the IME does send its own `Commit` in the same frame, keep that text
    /// untouched. Either way the Enter that ends composition is suppressed;
    /// the next Enter confirms normally.
    pub fn filter_ime_enter(&mut self, ui: &egui::Ui) {
        // Nothing asked for IME input last frame, so any composition state is
        // stale: winit cleared its marked text without telling egui (see
        // `ime_focus_seen`). Dropping it here is what keeps a later Enter from
        // committing a preedit that no longer exists on screen.
        if !self.ime_focus_seen {
            self.ime_composing = false;
            self.ime_preedit.clear();
        }
        let mut composing = self.ime_composing;
        let mut preedit = self.ime_preedit.clone();
        let mut ime_activity = false;
        let mut committed = false;
        let pointer_pressed = ui.input(|input| input.pointer.any_pressed());
        ui.input(|input| {
            for event in &input.events {
                match event {
                    egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) => {
                        preedit = text.clone();
                        composing = !text.is_empty();
                        ime_activity = true;
                    }
                    egui::Event::Ime(egui::ImeEvent::Commit(_)) => {
                        preedit.clear();
                        composing = false;
                        ime_activity = true;
                        committed = true;
                    }
                    egui::Event::Key {
                        key: egui::Key::Escape,
                        pressed: true,
                        ..
                    } => {
                        preedit.clear();
                        composing = false;
                    }
                    _ => {}
                }
            }
        });
        if pointer_pressed {
            preedit.clear();
            composing = false;
        }
        let enter_pressed = ui.input(|input| input.key_pressed(egui::Key::Enter));
        if enter_pressed && composing {
            // macOS/egui-winit reports the Enter that leaves a composition as
            // a plain key event, without an ImeEvent::Commit. Synthesize the
            // commit so the preedit text lands in the field, then drop the
            // key so it cannot also press the dialog's confirm button.
            if !committed && !preedit.is_empty() {
                // Chinese IME preedits separate syllables with spaces (for
                // example `test c c d`). A direct Enter commit is the user's
                // literal text, so those separators must not leak into file
                // names or editor content.
                let text = preedit.replace(' ', "");
                ui.input_mut(|input| {
                    input
                        .events
                        .push(egui::Event::Ime(egui::ImeEvent::Commit(text)));
                });
            }
            preedit.clear();
            composing = false;
            ime_activity = true;
        }
        if composing || ime_activity {
            ui.input_mut(|input| {
                input.events.retain(|event| {
                    !matches!(
                        event,
                        egui::Event::Key {
                            key: egui::Key::Enter,
                            pressed: true,
                            ..
                        }
                    )
                });
            });
        }
        self.ime_preedit = preedit;
        self.ime_composing = composing;
    }

    /// Decode the packaged application icon once and reuse it in the
    /// titlebar and the empty-workspace page.
    pub fn app_icon_texture(&mut self, ctx: &egui::Context) -> egui::TextureId {
        self.app_icon
            .get_or_insert_with(|| {
                let bytes = include_bytes!("../../../packaging/macos/icon-1024.png");
                let decoded = image::load_from_memory(bytes)
                    .expect("embedded LazeCode application icon must be a valid PNG")
                    .to_rgba8();
                let resized = image::imageops::resize(
                    &decoded,
                    64,
                    64,
                    image::imageops::FilterType::Lanczos3,
                );
                let pixels = egui::ColorImage::from_rgba_unmultiplied([64, 64], resized.as_raw());
                ctx.load_texture("lazecode-app-icon", pixels, egui::TextureOptions::LINEAR)
            })
            .id()
    }

    /// True while a full-screen surface or popover owns input. Used by the
    /// shell to place a full-screen hit-test blocker and by the editor and
    /// terminal to keep keystrokes out of the document behind it.
    pub fn blocks_input(&self, model: &AppModel) -> bool {
        self.settings_open
            || self.about_open
            || self.search.open
            || model.ui.command_palette_open
            || self.menu.is_some()
            || self.agent_menu.is_some()
            || self.update_open
            || !matches!(self.dialog, DialogState::None)
    }

    /// Hand this frame's popover rects to the shell and start a fresh buffer.
    pub fn take_menu_hit_rects(&mut self) -> Vec<egui::Rect> {
        std::mem::take(&mut self.menu_hit_rects)
    }

    /// True when scroll-wheel input this frame belongs to an overlay instead
    /// of the panels underneath: modals own the whole window, popover menus
    /// own their (previous frame's) rects. egui delivers the wheel to every
    /// scroll surface containing the pointer — first consumer wins, and
    /// panels always draw before overlays — so the shell must intercept the
    /// delta before the panels run.
    pub fn blocks_wheel(
        &self,
        model: &AppModel,
        pos: Option<egui::Pos2>,
        menu_rects: &[egui::Rect],
    ) -> bool {
        self.settings_open
            || self.about_open
            || self.search.open
            || model.ui.command_palette_open
            || self.update_open
            || !matches!(self.dialog, DialogState::None)
            || pos.is_some_and(|pos| menu_rects.iter().any(|rect| rect.contains(pos)))
    }

    /// Close the topmost overlay. Used by Escape and by the native Cmd+W
    /// menu accelerator while a modal (settings, dialog, update, menu) owns
    /// input; it must never close the editor tab underneath.
    pub fn close_overlay(&mut self, model: &mut AppModel) {
        if model.ui.command_palette_open {
            model.ui.command_palette_open = false;
            self.palette.focus_requested = false;
        } else if self.update_open {
            // Checking may be dismissed; its worker keeps running and a found
            // release reappears as the title-bar badge. Downloading and
            // installing must stay visible until they finish.
            if !matches!(
                self.update.view(&model.settings),
                crate::update_ui::UpdateView::Downloading { .. }
                    | crate::update_ui::UpdateView::Installing
            ) {
                self.update_open = false;
            }
        } else if !matches!(self.dialog, DialogState::None) {
            self.dialog = DialogState::None;
        } else if self.menu.is_some()
            || self.tree_submenu.is_some()
            || self.branch_submenu.is_some()
        {
            self.menu = None;
            self.tree_submenu = None;
            self.branch_submenu = None;
        } else if self.agent_menu.is_some() {
            self.agent_menu = None;
        } else if self.settings_open {
            self.settings_open = false;
        } else if self.about_open {
            self.about_open = false;
        } else if self.search.open {
            self.search.open = false;
        } else if self.find.open {
            self.find.open = false;
        }
    }

    pub fn open_menu(&mut self, kind: MenuKind, anchor: egui::Rect, payload: Option<String>) {
        self.tree_submenu = None;
        self.branch_submenu = None;
        self.menu = Some(MenuState {
            kind,
            anchor,
            payload,
            opened_frame: self.frame_count,
        });
    }

    pub fn close_menu(&mut self) {
        self.menu = None;
        self.tree_submenu = None;
        self.branch_submenu = None;
    }
    pub fn toggle_menu(&mut self, kind: MenuKind, anchor: egui::Rect, payload: Option<String>) {
        if matches!(&self.menu, Some(m) if m.kind == kind) {
            self.menu = None;
        } else {
            self.open_menu(kind, anchor, payload);
        }
    }

    pub fn open_agent_menu(&mut self, anchor: egui::Rect) {
        self.agent_menu = Some(anchor);
        self.agent_menu_frame = self.frame_count;
    }

    pub fn toggle_agent_menu(&mut self, anchor: egui::Rect) {
        if self.agent_menu.is_some() {
            self.agent_menu = None;
        } else {
            self.open_agent_menu(anchor);
        }
    }

    pub fn set_editor_focus(&mut self, id: u64) {
        self.editor_focus = Some(id);
        self.terminal_focus = None;
    }

    pub fn set_terminal_focus(&mut self, id: u64) {
        self.terminal_focus = Some(id);
        self.editor_focus = None;
    }

    /// True when a text surface (console or editor) owns the keyboard: no
    /// overlay/text field is taking keys, and the focused surface is the
    /// active pane/tab of the active project.
    pub fn typing_surface_owns_keys(&self, model: &AppModel, ctx: &egui::Context) -> bool {
        if self.blocks_input(model) || ctx.text_edit_focused() {
            return false;
        }
        let Some(project) = model.active_project() else {
            return false;
        };
        if let Some(pane_id) = self.terminal_focus {
            if !model.ui.terminal_visible || self.terminal_search.values().any(|search| search.open)
            {
                return false;
            }
            let worktree = project.active_worktree();
            return worktree
                .active_tab_id()
                .and_then(|tab_id| worktree.tab_by_id(tab_id))
                .is_some_and(|tab| tab.active_pane == pane_id);
        }
        if let Some(tab_id) = self.editor_focus {
            return project.active_tab == Some(tab_id)
                && project
                    .tab_by_id(tab_id)
                    .is_some_and(|tab| matches!(tab.kind, lazecode_core::TabKind::File(_)));
        }
        false
    }

    pub fn open_palette(&mut self) {
        self.palette.query.clear();
        self.palette.selected = 0;
        self.palette.focus_requested = true;
        self.palette.opened_frame = self.frame_count;
        self.menu = None;
        self.agent_menu = None;
        self.settings_open = false;
        self.about_open = false;
        self.search.open = false;
        self.find.open = false;
        self.dialog = DialogState::None;
    }

    pub fn close_overlays(&mut self) {
        self.menu = None;
        self.agent_menu = None;
        self.dialog = DialogState::None;
        self.search.open = false;
        self.settings_open = false;
        self.about_open = false;
    }

    /// Drop decoded images for closed image tabs and flush `egui_extras`
    /// caches for Markdown previews when their project/tab set changes.
    pub fn prune_media_caches(&mut self, ctx: &egui::Context, model: &AppModel) {
        use lazecode_core::TabKind;

        let mut open_images = HashSet::new();
        let mut gifs = HashSet::new();
        let mut markdown = HashSet::new();
        let mut roots = Vec::with_capacity(model.projects.len());
        for project in &model.projects {
            roots.push(project.root.clone());
            for worktree in project.worktrees.iter().skip(1) {
                roots.push(worktree.path.clone());
            }
            for tab in &project.tabs {
                match &tab.kind {
                    TabKind::Image(image) => {
                        open_images.insert(image.path.clone());
                        if image
                            .path
                            .extension()
                            .and_then(|ext| ext.to_str())
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("gif"))
                        {
                            gifs.insert(image.path.clone());
                        }
                    }
                    TabKind::File(file)
                        if file
                            .doc
                            .path
                            .extension()
                            .and_then(|ext| ext.to_str())
                            .is_some_and(|ext| {
                                ext.eq_ignore_ascii_case("md")
                                    || ext.eq_ignore_ascii_case("markdown")
                            }) =>
                    {
                        markdown.insert(file.doc.path.clone());
                    }
                    _ => {}
                }
            }
        }

        self.image_cache.retain_paths(&open_images);

        if roots != self.media_project_roots
            || markdown != self.media_markdown_paths
            || gifs != self.media_gif_paths
        {
            // Flush the markdown/image caches on project or Markdown-tab
            // changes; per-frame flushing would reload visible images.
            ctx.forget_all_images();
            self.md_cache = egui_commonmark::CommonMarkCache::default();
            self.media_project_roots = roots;
            self.media_markdown_paths = markdown;
            self.media_gif_paths = gifs;
        }
    }

    /// Aggregate live process activity for one project.
    pub fn project_busy(&self, project: &ProjectState) -> TerminalBusyState {
        let mut out = TerminalBusyState::default();
        for pane in project
            .worktrees
            .iter()
            .flat_map(|worktree| worktree.terminals.iter())
            .flat_map(|tab| tab.panes.iter())
        {
            let Some(state) = self.terminal_busy.get(&pane.id) else {
                continue;
            };
            if !state.busy {
                continue;
            }
            out.busy = true;
            out.outputting |= state.outputting;
            for command in &state.commands {
                if !out.commands.iter().any(|existing| existing == command) {
                    out.commands.push(command.clone());
                }
            }
        }
        out
    }

    pub fn busy_count(&self) -> usize {
        self.terminal_busy
            .values()
            .filter(|state| state.busy)
            .count()
    }

    pub fn busy_commands_for(&self, pane_ids: &[u64]) -> Vec<String> {
        let mut commands = Vec::new();
        for id in pane_ids {
            let Some(state) = self.terminal_busy.get(id) else {
                continue;
            };
            if !state.busy {
                continue;
            }
            for command in &state.commands {
                if !commands.iter().any(|existing| existing == command) {
                    commands.push(command.clone());
                }
            }
        }
        commands
    }

    /// Probe panes off-thread, bypassing the 2s cache (used on close/quit).
    fn spawn_busy_probe(
        &self,
        model: &AppModel,
        pane_ids: &[u64],
    ) -> std::sync::mpsc::Receiver<Vec<(u64, TerminalBusyState)>> {
        struct ProbeInput {
            id: u64,
            target: BusyTarget,
            output_ms: Option<u64>,
            input_ms: Option<u64>,
            resize_ms: Option<u64>,
            was_outputting: bool,
        }
        let mut rows: Vec<ProbeInput> = Vec::new();
        for project in &model.projects {
            for pane in project
                .worktrees
                .iter()
                .flat_map(|worktree| worktree.terminals.iter())
                .flat_map(|tab| tab.panes.iter())
            {
                if !pane_ids.contains(&pane.id) {
                    continue;
                }
                let was_outputting = self
                    .terminal_busy
                    .get(&pane.id)
                    .is_some_and(|state| state.outputting);
                rows.push(ProbeInput {
                    id: pane.id,
                    target: pane.session.busy_target(),
                    output_ms: pane.session.last_output_ms(),
                    input_ms: pane.session.last_input_ms(),
                    resize_ms: pane.session.last_resize_ms(),
                    was_outputting,
                });
            }
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let wake = self.wake_notifier.clone();
        std::thread::Builder::new()
            .name("lazecode-close-probe".to_string())
            .spawn(move || {
                let targets: Vec<BusyTarget> = rows.iter().map(|row| row.target.clone()).collect();
                let infos = probe_busy_many(&targets);
                let results: Vec<(u64, TerminalBusyState)> = rows
                    .into_iter()
                    .zip(infos)
                    .map(|(row, info)| {
                        (
                            row.id,
                            TerminalBusyState {
                                busy: info.busy,
                                outputting: console_outputting(
                                    info.busy,
                                    row.output_ms,
                                    row.input_ms,
                                    row.resize_ms,
                                    row.was_outputting,
                                ),
                                commands: info.commands,
                                last_output_ms: row.output_ms,
                            },
                        )
                    })
                    .collect();
                let _ = tx.send(results);
                if let Some(notifier) = &wake {
                    notifier.notify();
                }
            })
            .ok();
        rx
    }

    fn apply_busy_results(&mut self, results: Vec<(u64, TerminalBusyState)>) {
        for (id, state) in results {
            self.terminal_busy.insert(id, state);
        }
    }

    /// Ask before killing panes when a busy command is running. The probe
    /// runs off-thread; `poll_pending_close_probe` applies the outcome.
    pub fn request_terminal_close(&mut self, model: &mut AppModel, action: TerminalCloseAction) {
        let pane_ids = self.terminal_close_panes(model, &action);
        if pane_ids.is_empty() {
            self.apply_terminal_close(model, action);
            return;
        }
        let rx = self.spawn_busy_probe(model, &pane_ids);
        self.pending_close_probe = Some(PendingCloseProbe {
            action,
            pane_ids,
            rx,
        });
    }

    /// Drain a finished close probe (if any) and apply its decision: close
    /// the panes when idle, otherwise show the confirmation dialog. Returns
    /// true when the UI should redraw.
    pub fn poll_pending_close_probe(&mut self, model: &mut AppModel) -> bool {
        let Some(pending) = self.pending_close_probe.as_mut() else {
            return false;
        };
        let results = match pending.rx.try_recv() {
            Ok(results) => results,
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Vec::new(),
        };
        let pane_ids = pending.pane_ids.clone();
        let action = pending.action.clone();
        self.pending_close_probe = None;
        self.apply_busy_results(results);
        let commands = self.busy_commands_for(&pane_ids);
        if commands.is_empty() {
            self.apply_terminal_close(model, action);
        } else {
            self.dialog = DialogState::ConfirmTerminalClose { action, commands };
        }
        true
    }

    pub fn apply_terminal_close(&mut self, model: &mut AppModel, action: TerminalCloseAction) {
        match action {
            TerminalCloseAction::Pane(pane_id) => {
                let Some((project_index, _, _)) =
                    model
                        .projects
                        .iter()
                        .enumerate()
                        .find_map(|(index, project)| {
                            project
                                .pane_owner(pane_id)
                                .map(|owner| (index, owner.0, owner.1))
                        })
                else {
                    return;
                };
                let project = &mut model.projects[project_index];
                project.close_terminal(pane_id);
                self.forget_terminal(pane_id);
                if model.active_project == Some(project_index) {
                    match project.active_worktree().active_terminal {
                        Some(next) => self.set_terminal_focus(next),
                        None => self.terminal_focus = None,
                    }
                }
            }
            TerminalCloseAction::Tab(tab_id) => {
                let Some(project_index) = model.projects.iter().position(|project| {
                    project
                        .worktrees
                        .iter()
                        .any(|worktree| worktree.tab_by_id(tab_id).is_some())
                }) else {
                    return;
                };
                let panes: Vec<u64> = model.projects[project_index]
                    .worktrees
                    .iter()
                    .find_map(|worktree| worktree.tab_by_id(tab_id))
                    .map(|tab| tab.panes.iter().map(|pane| pane.id).collect())
                    .unwrap_or_default();
                let project = &mut model.projects[project_index];
                project.close_terminal_tab(tab_id);
                for id in panes {
                    self.forget_terminal(id);
                }
                if model.active_project == Some(project_index) {
                    match project.active_worktree().active_terminal {
                        Some(next) => self.set_terminal_focus(next),
                        None => self.terminal_focus = None,
                    }
                }
            }
            TerminalCloseAction::Others(keep_pane) => {
                let Some(project_index) = model
                    .projects
                    .iter()
                    .position(|project| project.term_by_id(keep_pane).is_some())
                else {
                    return;
                };
                let project = &mut model.projects[project_index];
                let keep_tab = project
                    .active_worktree()
                    .tab_of_pane(keep_pane)
                    .map(|tab| tab.id);
                let doomed: Vec<u64> = project
                    .active_worktree()
                    .terminals
                    .iter()
                    .filter(|tab| Some(tab.id) != keep_tab)
                    .flat_map(|tab| tab.panes.iter().map(|pane| pane.id))
                    .collect();
                project.close_other_terminals(keep_pane);
                for id in doomed {
                    self.forget_terminal(id);
                }
                self.set_terminal_focus(keep_pane);
            }
            TerminalCloseAction::All => {
                let Some(project_index) = model.active_project else {
                    return;
                };
                let project = &mut model.projects[project_index];
                let doomed: Vec<u64> = project
                    .active_worktree()
                    .terminals
                    .iter()
                    .flat_map(|tab| tab.panes.iter().map(|pane| pane.id))
                    .collect();
                project.close_all_terminals();
                for id in doomed {
                    self.forget_terminal(id);
                }
                self.terminal_focus = None;
            }
            TerminalCloseAction::Project(index) => {
                if index >= model.projects.len() {
                    return;
                }
                let active = model.active_project == Some(index);
                // Snapshots and probes are keyed by pane id; forget the closed
                // project's panes instead of clearing every other project's
                // last complete frame.
                let doomed: Vec<u64> = model.projects[index]
                    .worktrees
                    .iter()
                    .flat_map(|worktree| worktree.terminals.iter())
                    .flat_map(|tab| tab.panes.iter().map(|pane| pane.id))
                    .collect();
                model.close_project(index);
                for id in doomed {
                    self.forget_terminal(id);
                }
                model.save_workspace();
                self.clear_terminal_cache();
                if active {
                    self.terminal_focus = None;
                }
            }
        }
    }

    fn terminal_close_panes(&self, model: &AppModel, action: &TerminalCloseAction) -> Vec<u64> {
        match action {
            TerminalCloseAction::Pane(id) => vec![*id],
            TerminalCloseAction::Tab(tab_id) => model
                .projects
                .iter()
                .flat_map(|project| project.worktrees.iter())
                .find_map(|worktree| worktree.tab_by_id(*tab_id))
                .map(|tab| tab.panes.iter().map(|pane| pane.id).collect())
                .unwrap_or_default(),
            TerminalCloseAction::Others(keep) => model
                .projects
                .iter()
                .find(|project| project.term_by_id(*keep).is_some())
                .and_then(|project| {
                    project
                        .active_worktree()
                        .tab_of_pane(*keep)
                        .map(|tab| tab.id)
                })
                .map(|keep_tab| {
                    model
                        .projects
                        .iter()
                        .find(|project| project.term_by_id(*keep).is_some())
                        .into_iter()
                        .flat_map(|project| project.active_worktree().terminals.iter())
                        .filter(|tab| tab.id != keep_tab)
                        .flat_map(|tab| tab.panes.iter().map(|pane| pane.id))
                        .collect()
                })
                .unwrap_or_default(),
            TerminalCloseAction::All => model
                .active_project()
                .map(|project| {
                    project
                        .active_worktree()
                        .terminals
                        .iter()
                        .flat_map(|tab| tab.panes.iter().map(|pane| pane.id))
                        .collect()
                })
                .unwrap_or_default(),
            TerminalCloseAction::Project(index) => model
                .projects
                .get(*index)
                .map(|project| {
                    project
                        .worktrees
                        .iter()
                        .flat_map(|worktree| worktree.terminals.iter())
                        .flat_map(|tab| tab.panes.iter().map(|pane| pane.id))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// Begin the async quit guard: probe every console off-thread, then
    /// `poll_quit_probe` either shows the Quit dialog (unsaved tabs, running
    /// commands) or sets `should_quit`.
    pub fn request_quit_probe(&mut self, model: &AppModel) {
        let pane_ids: Vec<u64> = model
            .projects
            .iter()
            .flat_map(|project| project.worktrees.iter())
            .flat_map(|worktree| worktree.terminals.iter())
            .flat_map(|tab| tab.panes.iter().map(|pane| pane.id))
            .collect();
        let rx = self.spawn_busy_probe(model, &pane_ids);
        self.pending_quit_probe = Some(PendingQuitProbe { pane_ids, rx });
    }

    /// Drain a finished quit probe and apply its decision. Returns true when
    /// the UI should redraw.
    pub fn poll_quit_probe(&mut self, model: &mut AppModel) -> bool {
        let Some(pending) = self.pending_quit_probe.as_mut() else {
            return false;
        };
        let results = match pending.rx.try_recv() {
            Ok(results) => results,
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Vec::new(),
        };
        let pane_ids = pending.pane_ids.clone();
        self.pending_quit_probe = None;
        self.apply_busy_results(results);
        let commands = self.busy_commands_for(&pane_ids);
        let unsaved = model
            .projects
            .iter()
            .any(|project| project.tabs.iter().any(|tab| tab.is_dirty()));
        if unsaved || !commands.is_empty() {
            self.dialog = DialogState::Quit {
                commands,
                restart_update: false,
            };
        } else {
            model.should_quit = true;
        }
        true
    }

    pub fn clear_terminal_cache(&mut self) {
        // Drop a stale focus id from another project or its console looks
        // dead until clicked; the panel latches the new active session.
        // Snapshot cells are per pane and are the last complete frames: keep
        // them so the first draw after a switch can hold a coherent screen
        // while a TUI finishes its resize repaint. `forget_terminal` frees
        // them when the pane closes.
        self.terminal_selections.clear();
        self.terminal_search.clear();
        self.terminal_preedit.clear();
        self.terminal_wheel.clear();
        self.terminal_graphics.clear();
        self.terminal_resize.clear();
        // Keep `terminal_busy`: switching projects must not make a running
        // green ring disappear until the next 2s probe. Entries are keyed by
        // globally unique pane ids and `forget_terminal` removes closed ones.
        self.terminal_focus = None;
    }

    /// Drop cached rendering state for one closed pane (its PTY is gone).
    pub fn forget_terminal(&mut self, id: u64) {
        self.terminal_snapshots.remove(&id);
        self.terminal_selections.remove(&id);
        self.terminal_search.remove(&id);
        self.terminal_preedit.remove(&id);
        self.terminal_wheel.remove(&id);
        self.terminal_wake.remove(&id);
        self.terminal_graphics.remove(&id);
        self.terminal_busy.remove(&id);
        self.terminal_scroll_fraction.remove(&id);
        self.terminal_resize.remove(&id);
        if self.terminal_focus == Some(id) {
            self.terminal_focus = None;
        }
    }

    /// Drop per-pane state for panes the model no longer owns. The core can
    /// close panes on its own (`settle_single_files_workspace` drops an
    /// emptied Single files project outright), so call-site cleanup is not
    /// exhaustive; one retain per frame is cheap and idempotent, and keeps
    /// kitty texture caches from outliving their panes.
    pub fn prune_terminal_caches(&mut self, model: &AppModel) {
        let mut live: Vec<u64> = Vec::new();
        for project in &model.projects {
            for worktree in &project.worktrees {
                for tab in &worktree.terminals {
                    live.extend(tab.panes.iter().map(|pane| pane.id));
                }
            }
        }
        let alive = |id: &u64| live.contains(id);
        self.terminal_snapshots.retain(|id, _| alive(id));
        self.terminal_selections.retain(|id, _| alive(id));
        self.terminal_search.retain(|id, _| alive(id));
        self.terminal_preedit.retain(|id, _| alive(id));
        self.terminal_wheel.retain(|id, _| alive(id));
        self.terminal_wake.retain(|id, _| alive(id));
        self.terminal_graphics.retain(|id, _| alive(id));
        self.terminal_busy.retain(|id, _| alive(id));
        self.terminal_scroll_fraction.retain(|id, _| alive(id));
        self.terminal_resize.retain(|id, _| alive(id));
        if self.terminal_focus.is_some_and(|id| !alive(&id)) {
            self.terminal_focus = None;
        }
    }
}

pub struct PaletteItem {
    pub icon: Icon,
    pub title: String,
    pub subtitle: String,
    pub shortcut: Option<String>,
    pub action: PaletteAction,
}

#[derive(Clone, Debug)]
pub enum PaletteAction {
    OpenFile(std::path::PathBuf),
    ToggleSidebar,
    ToggleConsole,
    OpenSettings,
    CheckUpdates,
    ToggleTheme,
    OpenProject,
    NewTerminal,
    SaveFile,
    CloseTab,
    OpenGitLog,
    RefreshGit,
    OpenSearch,
    OpenPush,
    OpenPull,
    OpenRemotes,
    FetchAll,
    StashChanges,
    ShelvedChanges,
    About,
    SwitchProject(usize),
    SwitchWorktree { project: usize, name: String },
    CloneRepo,
    StashManager,
    ShelfManager,
    GitTools,
    CompareBranches,
    GoToLine,
    FormatDocument,
    FormatJson,
    MinifyJson,
    GoToSymbol,
    RenameSymbol,
    GoToDefinition,
    Quit,
}

pub const PALETTE_WIDTH: f32 = 640.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_output_lights_marquee() {
        assert!(console_outputting(true, Some(100), None, None, false));
    }

    #[test]
    fn input_echo_does_not_light_marquee() {
        assert!(!console_outputting(true, Some(100), Some(180), None, false));
        assert!(console_outputting(true, Some(100), Some(350), None, false));
    }

    #[test]
    fn resize_repaint_does_not_light_marquee() {
        assert!(!console_outputting(true, Some(100), None, Some(300), false));
        assert!(console_outputting(true, Some(100), None, Some(700), false));
    }

    #[test]
    fn resize_keeps_an_already_hot_marquee() {
        assert!(console_outputting(true, Some(100), None, Some(300), true));
    }

    #[test]
    fn stale_output_or_idle_console_stays_dark() {
        assert!(!console_outputting(true, Some(2_500), None, None, false));
        assert!(!console_outputting(false, Some(100), None, None, false));
    }

    fn enter_event() -> egui::Event {
        egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn preedit_event(text: &str) -> egui::Event {
        egui::Event::Ime(egui::ImeEvent::Preedit {
            text: text.to_string(),
            active_range_chars: None,
        })
    }

    fn filter_frame(rt: &mut UiRuntime, events: Vec<egui::Event>) -> (bool, Option<String>) {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::Vec2::new(800.0, 600.0),
            )),
            events,
            ..Default::default()
        };
        let mut enter = false;
        let mut commit = None;
        let mut output = ctx.run_ui(input, |ui| {
            rt.filter_ime_enter(ui);
            enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
            commit = ui.input(|input| {
                input.events.iter().find_map(|event| match event {
                    egui::Event::Ime(egui::ImeEvent::Commit(text)) => Some(text.clone()),
                    _ => None,
                })
            });
        });
        output.textures_delta.clear();
        (enter, commit)
    }

    #[test]
    fn ime_preedit_enter_commits_instead_of_confirming() {
        let mut rt = UiRuntime::default();
        let (enter, commit) = filter_frame(&mut rt, vec![preedit_event("ce"), enter_event()]);
        assert!(!enter, "Enter must not confirm the dialog");
        assert_eq!(
            commit.as_deref(),
            Some("ce"),
            "Enter must commit the preedit text"
        );
        let (enter, _) = filter_frame(&mut rt, vec![enter_event()]);
        assert!(enter, "the next Enter confirms normally");
    }

    #[test]
    fn ime_direct_enter_strips_preedit_separators() {
        let mut rt = UiRuntime::default();
        let (enter, commit) =
            filter_frame(&mut rt, vec![preedit_event("test c c d"), enter_event()]);
        assert!(!enter, "Enter must not confirm the dialog");
        assert_eq!(
            commit.as_deref(),
            Some("testccd"),
            "IME syllable separators must not reach the text field"
        );
    }

    #[test]
    fn ime_commit_enter_is_not_a_confirm() {
        let mut rt = UiRuntime::default();
        let (enter, commit) = filter_frame(
            &mut rt,
            vec![
                preedit_event("ce"),
                egui::Event::Ime(egui::ImeEvent::Commit("测".into())),
                enter_event(),
            ],
        );
        assert!(
            !enter,
            "the Enter that commits a candidate must not confirm"
        );
        assert_eq!(
            commit.as_deref(),
            Some("测"),
            "the IME's own commit must not be replaced"
        );
    }

    #[test]
    fn plain_enter_still_confirms() {
        let mut rt = UiRuntime::default();
        let (enter, _) = filter_frame(&mut rt, vec![enter_event()]);
        assert!(enter);
    }

    #[test]
    fn losing_ime_focus_drops_a_stale_composition() {
        // Frame N: a text field holds IME focus and the IME reports a preedit.
        let mut rt = UiRuntime {
            ime_focus_seen: true,
            ..UiRuntime::default()
        };
        let (enter, _) = filter_frame(&mut rt, vec![preedit_event("ce")]);
        assert!(!enter);
        assert!(rt.ime_composing);

        // Frame N+1: the focused surface is gone. winit cleared its marked
        // text without an event egui can see, so the Enter must be a real
        // confirm and must not commit the stale preedit.
        rt.ime_focus_seen = false;
        let (enter, commit) = filter_frame(&mut rt, vec![enter_event()]);
        assert!(enter, "a stale preedit must not swallow the next Enter");
        assert_eq!(commit, None, "a stale preedit must not be committed");
        assert!(!rt.ime_composing);
        assert!(rt.ime_preedit.is_empty());
    }
}
