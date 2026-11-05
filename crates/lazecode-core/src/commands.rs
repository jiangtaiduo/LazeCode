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
    RefreshLog,
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
