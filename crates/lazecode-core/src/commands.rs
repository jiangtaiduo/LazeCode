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
    CheckoutBranch(String),
    CreateBranch(String),
    DeleteBranch(String),
    RenameBranch {
        from: String,
        to: String,
    },
    CreateTag(String),
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
    },
    StashApply(String),
    StashDrop(String),
    ShelfSave(String),
    ShelfRestore {
        name: String,
        delete_after: bool,
    },
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
    FetchAll,
    Push {
        remote: String,
        branch: String,
        set_upstream: bool,
        force: bool,
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
    /// Apply a unified patch to the worktree or index (hunk staging).
    ApplyPatch {
        patch: String,
        cached: bool,
        reverse: bool,
        root: std::path::PathBuf,
    },
    RefreshLog,
    /// The log filter changed: re-query with the new options.
    LogFilterChanged,
    /// Load the next page of commits into the current log view.
    LogLoadMore,
    SelectCommit(String),
    OpenDiff(String),
    Search {
        query: String,
        request_id: u64,
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
    /// Create a linked worktree from the current HEAD of the active project
    /// at `parent/name`, then open it.
    CreateWorktree {
        name: String,
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
