use std::collections::HashMap;
use std::time::Instant;

use lazecode_fs::SearchHit;
use lazecode_terminal::Snapshot;
use lazecode_text::Cursor;

use crate::icons::Icon;

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
    /// 架藏（页签）右键：payload = name \u{1} deleted \u{1} paths…。
    ShelfContext {
        project: usize,
    },
    Project,
    Branch,
    Worktree,
    LogBranchFilter {
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
}

#[derive(Default)]
pub struct PaletteState {
    pub query: String,
    pub selected: usize,
    pub focus_requested: bool,
}

pub struct UiRuntime {
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
    pub editor_focus: Option<u64>,
    pub editor_drag: bool,
    pub sidebar_drag: bool,
    pub panel_drag: bool,
    /// 分支比较页：pane 高度分隔条拖拽中（松手时落盘 settings）。
    pub compare_split_drag: bool,
    /// 分支比较页：详情列宽度分隔条拖拽中。
    pub compare_details_drag: bool,
    pub settings_open: bool,
    pub about_open: bool,
    pub applied_theme: Option<lazecode_core::ThemeMode>,
    pub last_frame: Instant,
    pub cursor_epoch: f64,
    pub agent_menu: Option<egui::Rect>,
    pub agent_menu_frame: u64,
    pub frame_count: u64,
    pub branch_create_error: Option<String>,
    pub pending_open: Option<(std::path::PathBuf, usize, usize)>,
    pub toast_created: f64,
    pub last_revealed_tab: Option<u64>,
    pub ime_preedit: String,
    // --- 提交抽屉（稳定版 CommitDrawer）状态 ---
    /// Commit / Shelf 页签。
    pub commit_tab: CommitTab,
    /// 稳定版 ^P：Group by Directory（树视图）。
    pub commit_tree_view: bool,
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
            editor_focus: None,
            editor_drag: false,
            sidebar_drag: false,
            panel_drag: false,
            compare_split_drag: false,
            compare_details_drag: false,
            settings_open: false,
            about_open: false,
            applied_theme: None,
            last_frame: Instant::now(),
            cursor_epoch: 0.0,
            agent_menu: None,
            agent_menu_frame: 0,
            frame_count: 0,
            branch_create_error: None,
            pending_open: None,
            toast_created: 0.0,
            last_revealed_tab: None,
            ime_preedit: String::new(),
            commit_tab: CommitTab::Commit,
            commit_tree_view: false,
            commit_view_menu_open: false,
            shelf_entries: Vec::new(),
            shelf_deleted: Vec::new(),
            shelf_loaded: None,
            shelf_files_map: HashMap::new(),
            shelf_selected: None,
            shelf_file_sel: Vec::new(),
        }
    }
}

impl UiRuntime {
    pub fn open_menu(&mut self, kind: MenuKind, anchor: egui::Rect, payload: Option<String>) {
        self.menu = Some(MenuState {
            kind,
            anchor,
            payload,
            opened_frame: self.frame_count,
        });
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
        // Snapshots/selections are cheap to rebuild; keyboard focus is not.
        // Preserve the focused session id so switching away and back does not
        // silently drop keystrokes.
        let focus = self.terminal_focus;
        self.terminal_snapshots.clear();
        self.terminal_selections.clear();
        self.terminal_search.clear();
        self.terminal_focus = focus;
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
    Quit,
}

pub const PALETTE_WIDTH: f32 = 640.0;
