use std::collections::HashMap;
use std::time::Instant;

use lazecode_fs::SearchHit;
use lazecode_terminal::Snapshot;
use lazecode_text::Cursor;

use crate::icons::Icon;
use crate::overlays::UpdateState;

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

#[derive(Default)]
pub struct TermSearchState {
    pub open: bool,
    pub query: String,
    pub matches: Vec<lazecode_terminal::TerminalMatch>,
    pub index: usize,
    pub focus_requested: bool,
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
    Remotes,
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
        error: Option<String>,
    },
    DeleteBranch {
        name: String,
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
    },
    CloseDirtyTab {
        tab_id: u64,
        name: String,
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
    ShelfManager {
        shelves: Vec<lazecode_git::ShelfEntry>,
        selected: usize,
        /// 列表加载失败原因（原先静默置空，用户无从感知）。
        error: Option<String>,
        /// 首帧已加载过（空列表≠未加载，避免每帧重复 spawn git）。
        loaded: bool,
    },
    Quit,
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
    pub terminal_snapshots: HashMap<u64, Snapshot>,
    pub terminal_selections: HashMap<u64, Option<TermSelection>>,
    pub terminal_focus: Option<u64>,
    pub terminal_search: HashMap<u64, TermSearchState>,
    pub terminal_drag: Option<u64>,
    pub terminal_scroll_fraction: HashMap<u64, f32>,
    pub terminal_preedit: HashMap<u64, String>,
    /// Fractional trackpad wheel remainder per terminal session.
    pub terminal_wheel: HashMap<u64, f32>,
    /// Last snapshot wake counter per terminal; skips rebuilding unchanged grids.
    pub terminal_wake: HashMap<u64, u64>,
    /// egui textures for each pane's kitty graphics images.
    pub terminal_graphics: HashMap<u64, crate::terminal::GraphicsCache>,
    pub editor_focus: Option<u64>,
    pub editor_drag: bool,
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
    pub applied_theme: Option<lazecode_core::ThemeMode>,
    pub last_frame: Instant,
    pub md_cache: egui_commonmark::CommonMarkCache,
    pub image_zoom: HashMap<u64, f32>,
    pub cursor_epoch: f64,
    pub agent_menu: Option<egui::Rect>,
    pub agent_menu_frame: u64,
    pub frame_count: u64,
    pub branch_create_error: Option<String>,
    pub pending_open: Option<(std::path::PathBuf, usize, usize)>,
    pub toast_created: f64,
    pub last_revealed_tab: Option<u64>,
    pub last_revealed_cmp_file: Option<String>,
    pub ime_preedit: String,
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
    pub commit_view_menu_frame: u64,
    pub cmp_view_menu_frame: u64,
    pub new_ignore_pattern: String,
    pub tree_submenu: Option<(String, egui::Rect)>,
    pub branch_submenu: Option<BranchSubmenuState>,
    pub branch_sections_collapsed: std::collections::HashSet<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BranchSubmenuState {
    pub branch_name: String,
    pub is_current: bool,
    pub is_remote: bool,
    pub anchor: egui::Rect,
}

/// CommitDrawer 顶部页签（稳定版 tabCommit / tabShelf）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommitTab {
    Commit,
    Shelf,
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
            terminal_snapshots: HashMap::new(),
            terminal_selections: HashMap::new(),
            terminal_focus: None,
            terminal_search: HashMap::new(),
            terminal_drag: None,
            terminal_scroll_fraction: HashMap::new(),
            terminal_preedit: HashMap::new(),
            terminal_wheel: HashMap::new(),
            terminal_wake: HashMap::new(),
            terminal_graphics: HashMap::new(),
            new_ignore_pattern: String::new(),
            commit_view_menu_frame: 0,
            cmp_view_menu_frame: 0,
            editor_focus: None,
            editor_drag: false,
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
            image_zoom: HashMap::new(),
            agent_menu: None,
            agent_menu_frame: 0,
            frame_count: 0,
            branch_create_error: None,
            pending_open: None,
            toast_created: 0.0,
            last_revealed_tab: None,
            last_revealed_cmp_file: None,
            ime_preedit: String::new(),
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
            tree_submenu: None,
            branch_submenu: None,
            branch_sections_collapsed: std::collections::HashSet::new(),
        }
    }
}

impl UiRuntime {
    pub fn open_menu(&mut self, kind: MenuKind, anchor: egui::Rect, payload: Option<String>) {
        self.tree_submenu = None;
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

    pub fn clear_terminal_cache(&mut self) {
        // Cache entries are keyed by globally unique session ids. Keeping a
        // stale focus id from another project makes the new project's active
        // console appear dead until it is clicked, so drop it and let the
        // panel latch the new active session.
        self.terminal_snapshots.clear();
        self.terminal_selections.clear();
        self.terminal_search.clear();
        self.terminal_preedit.clear();
        self.terminal_wheel.clear();
        self.terminal_graphics.clear();
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
        self.terminal_scroll_fraction.remove(&id);
        if self.terminal_focus == Some(id) {
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
    GoToSymbol,
    RenameSymbol,
    GoToDefinition,
    Quit,
}

pub const PALETTE_WIDTH: f32 = 640.0;
