use std::path::PathBuf;

use lazecode_text::TextDiff;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    TypeChanged,
}

impl ChangeKind {
    pub fn letter(self) -> char {
        match self {
            ChangeKind::Added => 'A',
            ChangeKind::Modified => 'M',
            ChangeKind::Deleted => 'D',
            ChangeKind::Renamed => 'R',
            ChangeKind::TypeChanged => 'T',
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ChangeKind::Added => "Added",
            ChangeKind::Modified => "Modified",
            ChangeKind::Deleted => "Deleted",
            ChangeKind::Renamed => "Renamed",
            ChangeKind::TypeChanged => "Type changed",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Change {
    pub path: String,
    pub staged: Option<ChangeKind>,
    pub unstaged: Option<ChangeKind>,
    pub untracked: bool,
    pub additions: u32,
    pub deletions: u32,
    pub binary: bool,
}

impl Change {
    pub fn is_staged(&self) -> bool {
        self.staged.is_some()
    }

    pub fn is_unstaged(&self) -> bool {
        self.unstaged.is_some() || self.untracked
    }

    /// Single letter shown in the tree/source-control list.
    pub fn letter(&self) -> char {
        if self.untracked {
            return 'U';
        }
        self.unstaged
            .or(self.staged)
            .map(ChangeKind::letter)
            .unwrap_or('M')
    }

    pub fn kind(&self) -> ChangeKind {
        self.unstaged
            .or(self.staged)
            .unwrap_or(ChangeKind::Modified)
    }
}

#[derive(Clone, Debug)]
pub struct HeadInfo {
    pub id: String,
    pub short: String,
    pub message: String,
    pub author: String,
    pub time: i64,
}

#[derive(Clone, Debug)]
pub struct RepoSnapshot {
    pub root: PathBuf,
    pub branch: Option<String>,
    pub detached: bool,
    pub unborn: bool,
    pub ahead: usize,
    pub behind: usize,
    pub changes: Vec<Change>,
    pub head: Option<HeadInfo>,
}

impl RepoSnapshot {
    pub fn has_tracked_changes(&self) -> bool {
        self.changes
            .iter()
            .any(|change| change.staged.is_some() || change.unstaged.is_some())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefKind {
    Head,
    LocalBranch,
    RemoteBranch,
    Tag,
}

#[derive(Clone, Debug)]
pub struct RefLabel {
    pub label: String,
    pub kind: RefKind,
}

#[derive(Clone, Debug)]
pub struct BranchInfo {
    pub name: String,
    pub full_name: String,
    pub remote: bool,
    pub current: bool,
    pub target: String,
}

/// One registered worktree of a repository. The main worktree is a member of
/// every project's list, carrying `name = "main"`.
#[derive(Clone, Debug)]
pub struct WorktreeInfo {
    /// Stable identifier: `"main"` or the directory name under `.git/worktrees/`.
    pub name: String,
    /// Absolute checkout path. The base for tree/search/watch/console cwd.
    pub path: PathBuf,
    /// Branch the worktree has checked out (`None` = detached or unborn HEAD).
    pub branch: Option<String>,
    pub locked: bool,
    /// True when the checkout target vanished and only the registration remains.
    pub prunable: bool,
}

#[derive(Clone, Debug)]
pub struct GraphEdge {
    pub from_lane: usize,
    pub to_lane: usize,
    pub color_lane: usize,
    pub target_commit: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CommitInfo {
    pub id: String,
    pub short: String,
    pub message: String,
    pub body: String,
    pub author: String,
    pub email: String,
    pub time: i64,
    pub parents: Vec<String>,
    pub refs: Vec<RefLabel>,
    pub lane: usize,
    pub lane_count: usize,
    pub edges: Vec<GraphEdge>,
}

impl CommitInfo {
    pub fn summary(&self) -> &str {
        &self.message
    }
}

#[derive(Clone, Debug)]
pub struct FileStat {
    pub path: String,
    pub kind: ChangeKind,
    pub additions: u32,
    pub deletions: u32,
    pub binary: bool,
}

#[derive(Clone, Debug)]
pub struct CommitDetails {
    pub info: CommitInfo,
    pub files: Vec<FileStat>,
}

#[derive(Clone, Debug)]
pub struct FileDiff {
    pub path: String,
    pub kind: ChangeKind,
    pub old_text: String,
    pub new_text: String,
    pub diff: TextDiff,
    pub binary: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LogOptions {
    pub limit: usize,
    pub first_parent: bool,
}
