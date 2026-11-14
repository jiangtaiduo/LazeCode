/// Commands that require I/O, Git work or process control. The UI pushes
/// these into `AppModel::commands`; the application shell executes them.
#[derive(Clone, Debug)]
pub enum Command {
    OpenProjectDialog,
    OpenProject(std::path::PathBuf),
    CloseProject(usize),
    ReloadGit,
    ReloadTree,
    Stage(Vec<String>),
    Unstage(Vec<String>),
    Commit,
    /// Commit an explicit path set with its own message (context-menu
    /// 「Commit File(s)...」), bypassing the sidebar checkbox state.
    CommitPaths {
        paths: Vec<String>,
        message: String,
    },
    /// Create a changelist (persisted JSON in the repo git-dir).
    ChangelistCreate(String),
    /// Delete a changelist; `rollback` discards the uncommitted changes of
    /// its files instead of re-homing them to the default list.
    ChangelistDelete {
        name: String,
        rollback: bool,
    },
    /// Move files into a changelist.
    ChangelistAssign {
        paths: Vec<String>,
        list: String,
    },
    CheckoutBranch(String),
    CreateBranch {
        name: String,
        start_point: Option<String>,
        checkout: bool,
    },
    DeleteBranch {
        name: String,
        force: bool,
    },
    /// Delete `<remote>/<branch>` on the remote (never the local branch).
    DeleteRemoteBranch(String),
    RenameBranch {
        from: String,
        to: String,
    },
    CreateTag {
        name: String,
        /// Commit to tag; empty means HEAD.
        commit: String,
    },
    DeleteTag(String),
    CherryPick {
        hash: String,
    },
    Merge {
        ref_name: String,
    },
    MergeContinue,
    MergeAbort,
    Rebase {
        onto: String,
    },
    RebaseContinue,
    RebaseAbort,
    RebaseSkip,
    ContinueCherryPick,
    AbortCherryPick,
    StashPush {
        message: Option<String>,
        include_untracked: bool,
    },
    StashApply(String),
    StashDrop(String),
    /// Shelve the given paths (empty = all tracked changes) as a named patch.
    ShelfSave {
        name: String,
        paths: Vec<String>,
    },
    /// Apply a shelf back onto the worktree; `paths` restricts the restore
    /// to a file subset (unshelve dialog) and `target_list` optionally
    /// assigns the restored files to a (created) changelist.
    ShelfRestore {
        name: String,
        paths: Option<Vec<String>>,
        target_list: Option<String>,
        delete_after: bool,
    },
    /// 删除架藏（移入「最近删除」，可恢复；对齐稳定版语义）。
    ShelfDelete(String),
    /// 重命名架藏。
    ShelfRename {
        from: String,
        to: String,
    },
    /// 从「最近删除」恢复架藏。
    ShelfRestoreDeleted(String),
    /// 彻底删除「最近删除」中的架藏（不可恢复）。
    ShelfPurgeDeleted(String),
    RevertCommit {
        hash: String,
    },
    DropCommit {
        hash: String,
        force: bool,
    },
    ResetTo {
        hash: String,
        mode: lazecode_git::ResetMode,
    },
    SquashCommits {
        hashes: Vec<String>,
        message: String,
    },
    /// Roll back the uncommitted changes of these files (确认后执行；
    /// 新增文件回滚后其编辑器标签会被一并关闭).
    DiscardFiles(Vec<String>),
    /// Stage untracked paths into the index（对齐 PyCharm/stable 的
    /// Add to VCS；成功后刷新快照让文件移出 Unversioned 组）.
    AddToVcs(Vec<String>),
    /// Create Patch from Local Changes：先弹系统保存对话框（execute 里
    /// 处理），选中目标文件后再由后台任务计算并写出补丁。
    SavePatch(Vec<String>),
    /// 真正执行交互式变基（todo 由 UI 编排；此前只有对话框没有实现）。
    InteractiveRebase {
        onto: String,
        steps: Vec<lazecode_git::RebaseStep>,
    },
    CheckoutRevision(String),
    GitInit,
    UndoCommitHead,
    EditCommitMessageHead(String),
    GoToLine(usize),
    /// Open or activate a file and place the caret at an LSP position
    /// (`character` is a UTF-16 code-unit column from the server).
    OpenFileAt {
        path: std::path::PathBuf,
        line: u32,
        character: u32,
    },
    /// Apply an LSP workspace edit (rename) to open buffers and files on disk.
    ApplyWorkspaceEdit(crate::lsp::WorkspaceEdit),
    FetchAll,
    Push {
        remote: String,
        branch: String,
        set_upstream: bool,
        force: bool,
        push_tags: bool,
    },
    Pull {
        mode: lazecode_git::PullMode,
    },
    RemoteAdd {
        name: String,
        url: String,
    },
    RemoteRemove(String),
    RemoteRename {
        from: String,
        to: String,
    },
    RemoteSetUrl {
        name: String,
        url: String,
    },
    /// Atomically apply a remote edit: rename (when needed) then set the URL.
    RemoteEdit {
        from: String,
        name: String,
        url: String,
    },
    /// Push one local tag to a remote.
    PushTag {
        remote: String,
        tag: String,
    },
    /// Apply a unified patch to the worktree or index (hunk staging).
    ApplyPatch {
        patch: String,
        cached: bool,
        reverse: bool,
        root: std::path::PathBuf,
    },
    /// 后台分支比较（对话框打开/换方向/路径过滤触发）。
    CompareBranches {
        left: String,
        right: String,
        left_path: String,
        right_path: String,
    },
    /// 某一侧加载更多提交（Load More，稳定版 +500 递增）。
    CompareBranchesMore {
        /// 0 = left 侧，1 = right 侧。
        side: u8,
        exclude: String,
        include: String,
        limit: usize,
        path: String,
    },
    /// 详情列文件清单：单提交 / 多选范围（稳定版 git_commit_files /
    /// git_compare_commits）。`key` 用于回填匹配。
    CompareFiles {
        key: String,
        commit: String,
        from: String,
        to: String,
    },
    RefreshLog,
    /// The log filter changed: re-query with the new options.
    LogFilterChanged,
    /// 打开 Compare Versions 会话（稳定版 CompareDrawer）。
    CompareOpen {
        from: String,
        to: String,
    },
    /// 交换比较方向（工具栏 ⇆），重载文件清单并换向已打开的 diff。
    CompareSwap,
    /// 关闭会话：抽屉切回 Git 视图，diff 单例标签保留内容但导航清单失效。
    CompareClose,
    /// Load the next page of commits into the current log view.
    LogLoadMore,
    SelectCommit(String),
    OpenDiff(String),
    OpenCommitDiff {
        commit: String,
        path: String,
    },
    OpenCompareWithLocalDiff {
        commit: String,
        path: String,
    },
    OpenCompareCommitsDiff {
        from: String,
        to: String,
        path: String,
    },
    OpenFilesDiff {
        old_path: std::path::PathBuf,
        new_path: std::path::PathBuf,
    },
    ToggleDiffViewMode(u64),
    NextDiffHunk(u64),
    PrevDiffHunk(u64),
    Search {
        query: String,
        request_id: u64,
        options: lazecode_fs::SearchOptions,
    },
    ReplaceInFiles {
        query: String,
        replacement: String,
        options: lazecode_fs::SearchOptions,
    },
    SaveTab(u64),
    SaveAll,
    NewTerminal,
    NewTerminalCommand {
        title: String,
        command: String,
    },
    RestartTerminal(u64),
    CloseTerminal(u64),
    /// Create a linked worktree from the chosen branch at `location`.
    CreateWorktree {
        name: String,
        branch: String,
        location: std::path::PathBuf,
    },
    /// Switch the project pane to another registered worktree (by name).
    SwitchWorktree(String),
    /// Close a linked worktree (registration + checkout); `force` discards
    /// uncommitted changes.
    RemoveWorktree {
        name: String,
        force: bool,
    },
    SaveWorkspace,
    Quit,
}
