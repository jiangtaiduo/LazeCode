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
    /// Unmerged index entries (content or delete/modify conflicts).
    pub conflicted: bool,
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
        if self.conflicted {
            return 'C';
        }
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
    pub ignored_files: Vec<String>,
    pub ignored_dirs: Vec<String>,
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
    /// upstream branch (local heads only), e.g. `origin/main`.
    pub tracking: Option<String>,
    pub ahead: usize,
    pub behind: usize,
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
    /// `Some(false)` = commit not reachable from the current HEAD (dimmed in
    /// the log view). `None` = unknown/not applicable.
    pub in_head: Option<bool>,
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

/// Log query options, mirroring `git log` flags exposed by the Git log view.
#[derive(Clone, Debug, Default)]
pub struct LogOptions {
    /// Maximum number of commits to return (0 = default 300, capped at 5000).
    pub limit: usize,
    pub first_parent: bool,
    /// Hide merge commits (`--no-merges`).
    pub no_merges: bool,
    /// Topological order (parents before children). `false` = date order.
    pub topo_order: bool,
    /// Only commits whose matching author name equals this string
    /// (`--author`, exact regex-escaped match by the caller).
    pub author: Option<String>,
    /// Only commits after this Unix timestamp in seconds (`--since`).
    pub since: Option<i64>,
    /// Only commits touching this path (`-- <path>`).
    pub path: Option<String>,
    /// Include local and remote branches, not just the tip's history.
    /// `refs/stash` is deliberately excluded (matches `--branches --remotes`).
    pub all_refs: bool,
    /// Walk only this branch/tag/ref (`Some("main")`), the caller's filter.
    pub branch: Option<String>,
    /// `branch` holds a `left..right` range: tips[1] is never emitted and
    /// its ancestry is hidden from the walk.
    pub range_walk: bool,
    /// Commits reachable from these ids are not part of HEAD's history.
    pub head_ancestors: Option<std::collections::HashSet<String>>,
}
