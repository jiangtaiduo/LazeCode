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
    Search { query: String, request_id: u64 },
    SaveTab(u64),
    SaveAll,
    NewTerminal,
    NewTerminalCommand { title: String, command: String },
    RestartTerminal(u64),
    CloseTerminal(u64),
    SaveWorkspace,
    Quit,
}
