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
    ConflictResolve {
        path: String,
        ours: String,
        theirs: String,
        worktree: String,
    },
    CompareBranches {
        left: String,
        right: String,
        sides: Option<Box<(lazecode_git::CompareSide, lazecode_git::CompareSide)>>,
        loading: bool,
    },
    CompareCommits {
        from: String,
        to: String,
        files: Vec<(String, String)>,
        loading: bool,
    },
    StashManager {
        stashes: Vec<lazecode_git::StashEntry>,
        selected: usize,
    },
    ShelfManager {
        shelves: Vec<lazecode_git::ShelfEntry>,
        selected: usize,
    },
    FileHistory {
        path: String,
        entries: Vec<lazecode_git::HistoryEntry>,
        selected: usize,
    },
    GitTools {
        tab: GitToolsTab,
        submodules: Vec<lazecode_git::SubmoduleEntry>,
        lfs_status: Option<lazecode_git::LfsStatus>,
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
    pub settings_open: bool,
    pub about_open: bool,
    pub applied_theme: Option<lazecode_core::ThemeMode>,
    pub last_frame: Instant,
    pub cursor_epoch: f64,
    pub agent_menu: Option<egui::Rect>,
    pub branch_create_error: Option<String>,
    pub pending_open: Option<(std::path::PathBuf, usize, usize)>,
    pub toast_created: f64,
    pub last_revealed_tab: Option<u64>,
    pub ime_preedit: String,
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
            settings_open: false,
            about_open: false,
            applied_theme: None,
            last_frame: Instant::now(),
            cursor_epoch: 0.0,
            agent_menu: None,
            branch_create_error: None,
            pending_open: None,
            toast_created: 0.0,
            last_revealed_tab: None,
            ime_preedit: String::new(),
        }
    }
}

impl UiRuntime {
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
    Quit,
}

pub const PALETTE_WIDTH: f32 = 640.0;
