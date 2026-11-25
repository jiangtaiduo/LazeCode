//! Dialog state: every modal variant, its payload and the Git-tools tab.

use super::menu::{GitToolsTab, RebaseTodoRow};
use super::terminal::TerminalCloseAction;

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
