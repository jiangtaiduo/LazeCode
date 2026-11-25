//! Context-menu state and its kind table.

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
