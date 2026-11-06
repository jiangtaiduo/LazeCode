use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use lazecode_fs::tree::{load_tree, FileTree};
use lazecode_fs::ProjectWatcher;
use lazecode_git::{BranchInfo, CommitDetails, CommitInfo, FileDiff, LogOptions, RepoSnapshot};
use lazecode_terminal::{Notifier, Session, SessionKind, SpawnOptions};
use lazecode_text::Document;

use crate::commands::Command;
use crate::settings::{canonical, Language, Settings, ThemeMode, WorkspaceFile};

static NEXT_TAB: AtomicU64 = AtomicU64::new(1);
static NEXT_TERMINAL: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarView {
    Files,
    Git,
    Consoles,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelTab {
    Console,
    Git,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub message: String,
    pub kind: ToastKind,
    pub expires: Instant,
}

pub struct UiState {
    pub sidebar_visible: bool,
    pub sidebar_view: SidebarView,
    pub sidebar_width: f32,
    pub panel_visible: bool,
    pub panel_tab: PanelTab,
    pub panel_height: f32,
    pub panel_maximised: bool,
    pub settings_open: bool,
    pub settings_section: usize,
    pub command_palette_open: bool,
    pub palette_query: String,
    pub palette_selection: usize,
    pub tree_filter: String,
    pub branch_query: String,
    pub show_minimap: bool,
    pub focus_editor: bool,
    pub focus_terminal: bool,
    /// Deferred (project, branch) log filter pick from the overlay menu.
    /// `None` branch = current HEAD; `__all__` = all-refs.
    pub log_branch_pick: Option<(usize, Option<String>)>,
    /// Deferred (project, author) author-filter pick from the overlay menu.
    /// `None` = all authors.
    pub log_author_pick: Option<(usize, Option<String>)>,
    pub git_branches_visible: bool,
    pub git_branches_width: f32,
    pub git_details_width: f32,
    pub git_details_visible: bool,
    pub git_jump_to_commit: Option<String>,
    pub git_log_compact: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            sidebar_visible: true,
            sidebar_view: SidebarView::Files,
            sidebar_width: 262.0,
            panel_visible: true,
            panel_tab: PanelTab::Console,
            panel_height: 248.0,
            panel_maximised: false,
            settings_open: false,
            settings_section: 0,
            command_palette_open: false,
            palette_query: String::new(),
            palette_selection: 0,
            tree_filter: String::new(),
            branch_query: String::new(),
            show_minimap: true,
            focus_editor: false,
            focus_terminal: false,
            log_branch_pick: None,
            log_author_pick: None,
            git_branches_visible: true,
            git_branches_width: 210.0,
            git_details_width: 340.0,
            git_details_visible: true,
            git_jump_to_commit: None,
            git_log_compact: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MarkdownViewMode {
    #[default]
    Code,
    Preview,
    Split,
}

pub struct FileTab {
    pub doc: Document,
    pub external: bool,
    pub md_mode: MarkdownViewMode,
    pub annotate: bool,
    pub blame: Option<Vec<lazecode_git::BlameLine>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiffViewMode {
    #[default]
    Unified,
    SideBySide,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffSource {
    Worktree {
        path: String,
    },
    Commit {
        commit: String,
        path: String,
    },
    CompareWithLocal {
        commit: String,
        path: String,
    },
    CompareCommits {
        from: String,
        to: String,
        path: String,
    },
    Files {
        old_path: PathBuf,
        new_path: PathBuf,
    },
}

pub struct DiffTab {
    pub source: DiffSource,
    pub path: String,
    pub diff: Option<FileDiff>,
    pub loading: bool,
    pub error: Option<String>,
    pub view_mode: DiffViewMode,
    pub current_hunk: usize,
}

pub enum TabKind {
    File(FileTab),
    Diff(DiffTab),
}

pub struct Tab {
    pub id: u64,
    pub kind: TabKind,
}

impl Tab {
    pub fn title(&self) -> String {
        match &self.kind {
            TabKind::File(file) => file.doc.title(),
            TabKind::Diff(diff) => {
                let base = diff.path.rsplit('/').next().unwrap_or(&diff.path);
                match &diff.source {
                    DiffSource::Worktree { .. } => base.to_string(),
                    DiffSource::Commit { commit, .. } => {
                        format!("{base} ({})", &commit[..commit.len().min(7)])
                    }
                    DiffSource::CompareWithLocal { commit, .. } => {
                        format!("{base} (Local ↔ {})", &commit[..commit.len().min(7)])
                    }
                    DiffSource::CompareCommits { from, to, .. } => {
                        format!(
                            "{base} ({}..{})",
                            &from[..from.len().min(7)],
                            &to[..to.len().min(7)]
                        )
                    }
                    DiffSource::Files { .. } => base.to_string(),
                }
            }
        }
    }
    pub fn path(&self) -> Option<&str> {
        match &self.kind {
            TabKind::File(file) => file.doc.path.to_str(),
            TabKind::Diff(diff) => Some(diff.path.as_str()),
        }
    }

    pub fn is_dirty(&self) -> bool {
        matches!(&self.kind, TabKind::File(file) if file.doc.dirty)
    }
}

pub struct TerminalTab {
    pub id: u64,
    pub session: Session,
    pub title: String,
    pub kind: SessionKind,
    pub spawn_command: Option<String>,
    pub started: Instant,
}

/// One worktree of a project: a checkout with its own tree, Git state,
/// watcher and consoles. The main checkout is always present and first.
pub struct WorktreeState {
    pub name: String,
    pub path: PathBuf,
    /// True for the project's registered root (`.git` is a directory, not a
    /// `gitdir:` file).
    pub is_main: bool,
    pub tree: FileTree,
    pub git: Option<RepoSnapshot>,
    pub git_error: Option<String>,
    pub terminals: Vec<TerminalTab>,
    pub active_terminal: Option<u64>,
    pub watcher: Option<ProjectWatcher>,
    pub last_watch_poll: Instant,
    pub git_loading: bool,
    /// UI-side branch filter for the Git log view (per worktree).
    pub log_branch: Option<String>,
}

impl WorktreeState {
    pub fn branch(&self) -> String {
        self.git
            .as_ref()
            .and_then(|git| git.branch.clone())
            .unwrap_or_else(|| {
                if let Some(head) = self.git.as_ref().and_then(|git| git.head.as_ref()) {
                    format!("{} (detached)", head.short)
                } else {
                    "no branch".to_string()
                }
            })
    }

    /// Load the tree/git/watcher state of one checkout without spawning a
    /// console. Callers attach consoles afterwards via `open_terminal_in`.
    pub fn load(name: String, path: PathBuf, is_main: bool) -> Self {
        let tree = load_tree(&path);
        let watcher = lazecode_fs::watch_project(&path).ok();
        let (git, git_error) = match lazecode_git::snapshot(&path) {
            Ok(snapshot) => (Some(snapshot), None),
            Err(err) => {
                let message = err.to_string();
                if message.contains("not a Git repository") {
                    (None, None)
                } else {
                    (None, Some(message))
                }
            }
        };
        Self {
            name,
            path,
            is_main,
            tree,
            git,
            git_error,
            terminals: Vec::new(),
            active_terminal: None,
            watcher,
            last_watch_poll: Instant::now(),
            git_loading: false,
            log_branch: None,
        }
    }
}

/// Filter state of the Git log view (persisted per project until changed).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LogQuery {
    /// Substring/`.*/Cc` commit search (client-side).
    pub text: String,
    /// Show only commits of this branch/tag; empty = current HEAD.
    pub branch: String,
    /// Server-side author filter (`--author`).
    pub author: String,
    /// Server-side path filter (`-- <path>`).
    pub path: String,
    /// Only commits after this Unix timestamp (0 = no filter).
    pub since: i64,
    /// Date filter preset: all/today/week/month.
    pub date: DatePreset,
    /// All refs instead of the current branch's history.
    pub all_refs: bool,
    pub first_parent: bool,
    pub no_merges: bool,
    /// Sort by topo order (true) or commit date (false).
    pub topo_order: bool,
    /// Case sensitivity of `text`; and regex flag.
    pub use_regex: bool,
    pub case_sensitive: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DatePreset {
    #[default]
    All,
    Today,
    Week,
    Month,
}

impl DatePreset {
    pub fn seconds_back(self, now: i64) -> i64 {
        match self {
            Self::All => 0,
            Self::Today => now - now.rem_euclid(86_400),
            Self::Week => now - 7 * 86_400,
            Self::Month => now - 30 * 86_400,
        }
    }
}

/// Commits reachable from the project's HEAD (including HEAD itself), for
/// dimming out-of-HEAD log rows. `None` = unborn HEAD / not a repo.
fn log_ancestor_set(root: &Path) -> Option<std::collections::HashSet<String>> {
    lazecode_git::head_ancestors(root)
}

/// All directory prefixes of the given branch names.
fn expanded_ref_dirs(branches: &[lazecode_git::BranchInfo]) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    for branch in branches {
        if !branch.name.contains('/') {
            continue;
        }
        let mut path = String::new();
        for part in branch.name.split('/').filter(|part| !part.is_empty()) {
            path = if path.is_empty() {
                part.to_string()
            } else {
                format!("{path}/{part}")
            };
            set.insert(path.clone());
        }
    }
    set
}

pub struct ProjectState {
    /// The registered project root (the main checkout).
    pub root: PathBuf,
    pub name: String,
    pub initials: String,
    pub tone: (u8, u8, u8),
    pub tree: FileTree,
    pub expanded: HashSet<PathBuf>,
    pub git: Option<RepoSnapshot>,
    pub git_error: Option<String>,
    pub log: Vec<CommitInfo>,
    pub log_error: Option<String>,
    pub branches: Vec<BranchInfo>,
    pub branch_query: String,
    /// Expanded directory paths of the Git log branch tree.
    pub ref_dirs_expanded: std::collections::HashSet<String>,
    /// Multi-selection of the branch tree (full ref names).
    pub ref_selection: std::collections::HashSet<String>,
    pub favorites: std::collections::HashSet<String>,
    pub commit_file_dirs_expanded: std::collections::HashSet<String>,
    pub tags: Vec<String>,
    /// HEAD anchor of each tag (short hash), for the tags rows.
    pub tag_targets: Vec<(String, String)>,
    /// Cached remotes (name, url) of the project, refreshed on `refresh_branches`.
    pub remotes: Vec<lazecode_git::RemoteInfo>,
    /// Commits the current branch has that `origin/<branch>` does not.
    /// Remains 0 when no upstream is configured.
    pub pending_push: usize,
    /// Conflicted paths while a sequencer operation (merge/rebase/cherry-
    /// pick/revert) is in flight; empty otherwise.
    pub conflicts: Vec<lazecode_git::ConflictEntry>,
    /// The sequencer's subject line, for the conflict banner.
    pub sequencer: Option<lazecode_git::SequencerState>,
    /// The Git log filter state (text filter lives here; branch mirrors log_branch).
    pub log_query: LogQuery,
    /// Continuation state of the pager: the offset the next page starts at.
    pub log_loaded: usize,
    /// The final lane state of the loaded prefix (the next page's seed).
    pub log_seed: Vec<Option<String>>,
    /// True while another page fetch is in flight.
    pub log_paging: bool,
    pub log_branch: Option<String>,
    pub selected_commit: Option<String>,
    pub commit_details: Option<CommitDetails>,
    pub commit_details_attempted: Option<String>,
    pub tabs: Vec<Tab>,
    pub active_tab: Option<u64>,
    pub commit_message: String,
    pub amend: bool,
    pub unchecked_paths: HashSet<String>,
    /// All worktrees incl. the main one at index 0.
    pub worktrees: Vec<WorktreeState>,
    /// Index into `worktrees` of the console/tree pane target.
    pub active_worktree: usize,
    pub watcher: Option<ProjectWatcher>,
    pub last_watch_poll: Instant,
    pub git_loading: bool,
    pub log_loading: bool,
}

impl ProjectState {
    pub fn open(
        root: PathBuf,
        settings: &Settings,
        notifier: Option<Arc<dyn Notifier>>,
    ) -> Result<Self, String> {
        let root = canonical(&root);
        if !root.is_dir() {
            return Err(format!("{} is not a directory", root.display()));
        }
        let name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string());
        let initials = initials_of(&name);
        let tone = tone_for(&root);
        let tree = load_tree(&root);
        let watcher = lazecode_fs::watch_project(&root).ok();
        let (git, git_error) = match lazecode_git::snapshot(&root) {
            Ok(snapshot) => (Some(snapshot), None),
            Err(err) => {
                let message = err.to_string();
                if message.contains("not a Git repository") {
                    (None, None)
                } else {
                    (None, Some(message))
                }
            }
        };
        let (log, branches, log_error) = if git.is_some() {
            match (
                lazecode_git::log(&root, LogOptions::default()),
                lazecode_git::branches(&root),
            ) {
                (Ok(mut log), Ok(branches)) => {
                    let head_ancestors = log_ancestor_set(&root);
                    for commit in log.iter_mut() {
                        commit.in_head =
                            head_ancestors.as_ref().map(|set| set.contains(&commit.id));
                    }
                    (log, branches, None)
                }
                (Err(err), _) | (_, Err(err)) => (Vec::new(), Vec::new(), Some(err.to_string())),
            }
        } else {
            (Vec::new(), Vec::new(), None)
        };
        let init_dirs = expanded_ref_dirs(&branches);
        let mut project = Self {
            root,
            name,
            initials,
            tone,
            tree,
            expanded: HashSet::new(),
            git,
            git_error,
            log,
            log_error,
            branches,
            branch_query: String::new(),
            // The branch tree starts fully expanded (PyCharm default):
            ref_dirs_expanded: init_dirs,
            ref_selection: std::collections::HashSet::new(),
            favorites: std::collections::HashSet::new(),
            commit_file_dirs_expanded: std::collections::HashSet::new(),
            tags: Vec::new(),
            tag_targets: Vec::new(),
            remotes: Vec::new(),
            pending_push: 0,
            conflicts: Vec::new(),
            sequencer: None,
            log_query: LogQuery::default(),
            log_loaded: 0,
            log_seed: Vec::new(),
            log_paging: false,
            log_branch: None,
            selected_commit: None,
            commit_details: None,
            commit_details_attempted: None,
            tabs: Vec::new(),
            active_tab: None,
            commit_message: String::new(),
            amend: false,
            unchecked_paths: HashSet::new(),
            worktrees: Vec::new(),
            active_worktree: 0,
            watcher,
            last_watch_poll: Instant::now(),
            git_loading: false,
            log_loading: false,
        };
        if let Some(first) = project.log.first() {
            project.selected_commit = Some(first.id.clone());
        }
        let main = WorktreeState::load("main".to_string(), project.root.clone(), true);
        project.worktrees.push(main);
        if let Err(err) = project.open_terminal(None, None, SessionKind::Shell, settings, notifier)
        {
            // A bad shell setting must not make the whole project (and every
            // restored tab) disappear from the workspace.
            log::warn!(
                "cannot open initial terminal for {}: {err}",
                project.root.display()
            );
        }
        Ok(project)
    }

    pub fn branch(&self) -> String {
        self.git
            .as_ref()
            .and_then(|git| git.branch.clone())
            .unwrap_or_else(|| {
                if let Some(head) = self.git.as_ref().and_then(|git| git.head.as_ref()) {
                    format!("{} (detached)", head.short)
                } else {
                    "no branch".to_string()
                }
            })
    }

    pub fn changes(&self) -> &[lazecode_git::Change] {
        self.git
            .as_ref()
            .map(|git| git.changes.as_slice())
            .unwrap_or(&[])
    }

    pub fn checked_changes(&self) -> Vec<String> {
        self.changes()
            .iter()
            .filter(|change| !self.unchecked_paths.contains(&change.path))
            .map(|change| change.path.clone())
            .collect()
    }

    pub fn refresh_git(&mut self) {
        self.git_loading = true;
        match lazecode_git::snapshot(&self.root) {
            Ok(snapshot) => {
                self.git = Some(snapshot);
                self.git_error = None;
            }
            Err(err) => self.git_error = Some(err.to_string()),
        }
        self.prune_unchecked();
        self.git_loading = false;
    }

    /// Refresh the Git snapshot of one worktree by its path.
    pub fn refresh_git_for(&mut self, path: &Path) {
        let Some(worktree) = self
            .worktrees
            .iter_mut()
            .find(|worktree| worktree.path == path)
        else {
            return;
        };
        worktree.git_loading = true;
        match lazecode_git::snapshot(path) {
            Ok(snapshot) => {
                worktree.git = Some(snapshot);
                worktree.git_error = None;
            }
            Err(err) => worktree.git_error = Some(err.to_string()),
        }
        worktree.git_loading = false;
    }

    pub fn prune_unchecked_public(&mut self) {
        self.prune_unchecked();
    }

    fn prune_unchecked(&mut self) {
        let existing: HashSet<String> = self
            .changes()
            .iter()
            .map(|change| change.path.clone())
            .collect();
        self.unchecked_paths.retain(|path| existing.contains(path));
    }

    pub fn refresh_log(&mut self) {
        if self.git.is_none() {
            return;
        }
        self.log_loading = true;
        match lazecode_git::log(&self.root, LogOptions::default()) {
            Ok(log) => {
                if let Some(branch) = &self.log_branch {
                    self.log = log
                        .into_iter()
                        .filter(|commit| commit.refs.iter().any(|label| label.label == *branch))
                        .collect();
                } else {
                    self.log = log;
                }
                self.log_error = None;
                if !self
                    .log
                    .iter()
                    .any(|commit| Some(&commit.id) == self.selected_commit.as_ref())
                {
                    self.selected_commit = self.log.first().map(|commit| commit.id.clone());
                    self.commit_details = None;
                }
            }
            Err(err) => self.log_error = Some(err.to_string()),
        }
        self.log_loading = false;
    }

    pub fn refresh_branches(&mut self) {
        if let Ok(branches) = lazecode_git::branches(&self.root) {
            // Expand any new directories the refresh brought in.
            let new_dirs = expanded_ref_dirs(&branches);
            let dirs: Vec<String> = new_dirs.into_iter().collect();
            self.ref_dirs_expanded.extend(dirs);
            self.branches = branches;
        }
        if let Ok(remotes) = lazecode_git::remote_list(&self.root) {
            self.remotes = remotes;
        }
        self.pending_push = self
            .remotes
            .first()
            .and(Some(
                lazecode_git::pending_count(&self.root, &self.branch()).unwrap_or(0),
            ))
            .unwrap_or(0);
        if let Ok(tags) = lazecode_git::tags(&self.root) {
            self.tags = tags;
        }
        // Target of each tag => short-commit-id rows.
        let mut targets = Vec::new();
        for tag in &self.tags {
            let target = lazecode_git::log_ref(
                &self.root,
                tag,
                lazecode_git::LogOptions {
                    limit: 1,
                    ..Default::default()
                },
            )
            .ok()
            .and_then(|commits| commits.first().map(|commit| commit.short.clone()))
            .unwrap_or_default();
            targets.push((tag.clone(), target));
        }
        self.tag_targets = targets;
    }

    pub fn refresh_conflicts(&mut self) {
        if self.git.is_none() {
            return;
        }
        self.conflicts = lazecode_git::conflict_list(&self.root).unwrap_or_default();
        self.sequencer = lazecode_git::sequencer_state(&self.root).ok();
        if self.conflicts.is_empty() && self.sequencer == Some(lazecode_git::SequencerState::None) {
            self.sequencer = None;
        }
    }

    pub fn active_tab_index(&self) -> Option<usize> {
        let id = self.active_tab?;
        self.tabs.iter().position(|tab| tab.id == id)
    }

    /// The worktree whose tree/consoles/gits are shown in the pane.
    pub fn active_worktree(&self) -> &WorktreeState {
        &self.worktrees[self
            .active_worktree
            .min(self.worktrees.len().saturating_sub(1))]
    }

    /// Name of the focused worktree, for UI identity checks.
    pub fn active_worktree_index(&self) -> Option<usize> {
        if self.worktrees.is_empty() {
            return None;
        }
        Some(self.active_worktree)
    }

    pub fn active_worktree_mut(&mut self) -> &mut WorktreeState {
        let index = self
            .active_worktree
            .min(self.worktrees.len().saturating_sub(1));
        &mut self.worktrees[index]
    }

    pub fn worktree_index(&self, name: &str) -> Option<usize> {
        self.worktrees.iter().position(|wt| wt.name == name)
    }

    /// Switch to an already attached worktree, or attach one that exists in
    /// Git but is not yet loaded into the model. Returns `Ok(Some(()))` when
    /// a new worktree was attached, `Ok(None)` when it was already loaded.
    pub fn ensure_worktree(&mut self, name: &str) -> Result<Option<()>, String> {
        if let Some(index) = self.worktree_index(name) {
            self.active_worktree = index;
            return Ok(None);
        }
        // A worktree registered in git but not attached yet: load it cold
        // (no console spawns automatically; a worktree keeps its own set).
        let all = lazecode_git::list_worktrees(&self.root).map_err(|err| err.to_string())?;
        let Some(info) = all.into_iter().find(|worktree| worktree.name == name) else {
            return Err(format!("worktree '{name}' does not exist"));
        };
        let worktree = WorktreeState::load(info.name, info.path, false);
        let index = self.worktrees.len();
        self.worktrees.push(worktree);
        self.active_worktree = index;
        Ok(Some(()))
    }

    /// Register a freshly created worktree and focus it.
    pub fn attach_worktree(&mut self, worktree: WorktreeState) {
        // A worktree with the same path may already exist.
        if let Some(index) = self
            .worktrees
            .iter()
            .position(|existing| existing.path == worktree.path)
        {
            self.worktrees[index] = worktree;
            self.active_worktree = index;
            return;
        }
        self.worktrees.push(worktree);
        self.active_worktree = self.worktrees.len() - 1;
    }

    /// Drop the model entry for a linked worktree after git removal.
    pub fn detach_worktree(&mut self, name: &str) {
        let Some(index) = self.worktrees.iter().position(|wt| wt.name == name) else {
            return;
        };
        self.worktrees.remove(index);
        if self.active_worktree >= self.worktrees.len() {
            self.active_worktree = self.worktrees.len().saturating_sub(1);
        }
    }

    pub fn tab_by_id(&self, id: u64) -> Option<&Tab> {
        self.tabs.iter().find(|tab| tab.id == id)
    }

    pub fn tab_mut_by_id(&mut self, id: u64) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|tab| tab.id == id)
    }

    pub fn open_file(&mut self, path: PathBuf, settings: &Settings) -> Result<u64, String> {
        let path = if path.is_absolute() {
            path
        } else {
            self.root.join(path)
        };
        // macOS temp paths (/var) are symlinked to /private/var; compare the
        // canonical form so the same file never opens twice.
        let path = path.canonicalize().unwrap_or(path);
        if let Some(tab) = self
            .tabs
            .iter()
            .find(|tab| matches!(&tab.kind, TabKind::File(file) if file.doc.path == path))
        {
            self.active_tab = Some(tab.id);
            return Ok(tab.id);
        }
        let doc = Document::open(&path, settings.editor.tab_width, settings.editor.use_spaces)
            .map_err(|err| err.to_string())?;
        let id = NEXT_TAB.fetch_add(1, Ordering::Relaxed);
        let external = !path.starts_with(&self.root);
        self.tabs.push(Tab {
            id,
            kind: TabKind::File(FileTab {
                doc,
                external,
                md_mode: MarkdownViewMode::Code,
                annotate: false,
                blame: None,
            }),
        });
        self.active_tab = Some(id);
        self.expand_parents(&path);
        Ok(id)
    }

    pub fn open_diff_source(&mut self, source: DiffSource) -> u64 {
        if let Some(tab) = self.tabs.iter().find(|tab| match &tab.kind {
            TabKind::Diff(diff) => diff.source == source,
            _ => false,
        }) {
            self.active_tab = Some(tab.id);
            return tab.id;
        }
        let id = NEXT_TAB.fetch_add(1, Ordering::Relaxed);
        let path = match &source {
            DiffSource::Worktree { path }
            | DiffSource::Commit { path, .. }
            | DiffSource::CompareWithLocal { path, .. }
            | DiffSource::CompareCommits { path, .. } => path.clone(),
            DiffSource::Files { old_path, new_path } => format!(
                "{} ↔ {}",
                old_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("left"),
                new_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("right"),
            ),
        };
        self.tabs.push(Tab {
            id,
            kind: TabKind::Diff(DiffTab {
                source,
                path,
                diff: None,
                loading: true,
                error: None,
                view_mode: DiffViewMode::Unified,
                current_hunk: 0,
            }),
        });
        self.active_tab = Some(id);
        id
    }

    pub fn open_diff(&mut self, path: String) -> u64 {
        self.open_diff_source(DiffSource::Worktree { path })
    }

    pub fn open_commit_diff(&mut self, commit: String, path: String) -> u64 {
        self.open_diff_source(DiffSource::Commit { commit, path })
    }

    pub fn open_compare_with_local_diff(&mut self, commit: String, path: String) -> u64 {
        self.open_diff_source(DiffSource::CompareWithLocal { commit, path })
    }

    pub fn open_compare_commits_diff(&mut self, from: String, to: String, path: String) -> u64 {
        self.open_diff_source(DiffSource::CompareCommits { from, to, path })
    }

    pub fn open_files_diff(&mut self, old_path: PathBuf, new_path: PathBuf) -> u64 {
        self.open_diff_source(DiffSource::Files { old_path, new_path })
    }

    pub fn set_diff_for_source(&mut self, source: &DiffSource, result: Result<FileDiff, String>) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| match &tab.kind {
            TabKind::Diff(diff) => &diff.source == source,
            _ => false,
        }) {
            if let TabKind::Diff(diff) = &mut tab.kind {
                diff.loading = false;
                match result {
                    Ok(file_diff) => {
                        diff.diff = Some(file_diff);
                        diff.error = None;
                    }
                    Err(err) => {
                        diff.diff = None;
                        diff.error = Some(err);
                    }
                }
            }
        }
    }

    pub fn set_diff(&mut self, path: &str, result: Result<FileDiff, String>) {
        if let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|tab| matches!(&tab.kind, TabKind::Diff(diff) if diff.path == path))
        {
            if let TabKind::Diff(diff) = &mut tab.kind {
                diff.loading = false;
                match result {
                    Ok(file_diff) => {
                        diff.diff = Some(file_diff);
                        diff.error = None;
                    }
                    Err(err) => {
                        diff.diff = None;
                        diff.error = Some(err);
                    }
                }
            }
        }
    }

    pub fn close_tab(&mut self, id: u64) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        self.tabs.remove(index);
        if self.active_tab == Some(id) {
            self.active_tab = self
                .tabs
                .get(index.saturating_sub(1))
                .or_else(|| self.tabs.first())
                .map(|tab| tab.id);
        }
    }

    pub fn expand_parents(&mut self, path: &Path) {
        let Ok(rel) = path.strip_prefix(&self.root) else {
            return;
        };
        let mut current = PathBuf::new();
        for component in rel.components() {
            current.push(component);
            if current.as_os_str().is_empty() {
                continue;
            }
            self.expanded.insert(current.clone());
        }
    }

    pub fn term_by_id(&self, id: u64) -> Option<&TerminalTab> {
        self.worktrees
            .iter()
            .find_map(|wt| wt.terminals.iter().find(|terminal| terminal.id == id))
    }

    pub fn term_by_id_mut(&mut self, id: u64) -> Option<&mut TerminalTab> {
        self.worktrees
            .iter_mut()
            .find_map(|wt| wt.terminals.iter_mut().find(|terminal| terminal.id == id))
    }

    /// Console index (in the pane) that owns the terminal id.
    pub fn active_terminal_owner(&self, id: u64) -> Option<(usize, usize)> {
        for (wt_index, wt) in self.worktrees.iter().enumerate() {
            if let Some(term_index) = wt.terminals.iter().position(|terminal| terminal.id == id) {
                return Some((wt_index, term_index));
            }
        }
        None
    }

    pub fn open_terminal(
        &mut self,
        command: Option<String>,
        title: Option<String>,
        kind: SessionKind,
        settings: &Settings,
        notifier: Option<Arc<dyn Notifier>>,
    ) -> Result<u64, String> {
        let cwd = self.active_worktree().path.clone();
        self.open_terminal_in(cwd, command, title, kind, settings, notifier)
    }

    /// Open a console inside the active worktree rooted at `cwd` (which
    /// differs for linked worktrees).
    pub fn open_terminal_in(
        &mut self,
        cwd: PathBuf,
        command: Option<String>,
        title: Option<String>,
        kind: SessionKind,
        settings: &Settings,
        notifier: Option<Arc<dyn Notifier>>,
    ) -> Result<u64, String> {
        let shell = settings.terminal.shell.clone();
        let options = SpawnOptions {
            shell,
            command: command.clone(),
            cwd: cwd.clone(),
            cols: 110,
            rows: 28,
            scrollback: settings.terminal.scrollback,
            notifier,
            kind,
        };
        let session = lazecode_terminal::spawn(options).map_err(|err| err.to_string())?;
        let id = NEXT_TERMINAL.fetch_add(1, Ordering::Relaxed);
        let title = title.unwrap_or_else(|| session.command_label.clone());
        let terminal = self.active_worktree_mut();
        terminal.terminals.push(TerminalTab {
            id,
            session,
            title,
            kind,
            spawn_command: command,
            started: Instant::now(),
        });
        terminal.active_terminal = Some(id);
        Ok(id)
    }

    pub fn close_terminal(&mut self, id: u64) {
        let Some(wt_index) = self
            .worktrees
            .iter()
            .position(|wt| wt.terminals.iter().any(|terminal| terminal.id == id))
        else {
            return;
        };
        let wt = &mut self.worktrees[wt_index];
        let Some(index) = wt.terminals.iter().position(|terminal| terminal.id == id) else {
            return;
        };
        wt.terminals.remove(index);
        if wt.active_terminal == Some(id) {
            wt.active_terminal = wt
                .terminals
                .get(index.saturating_sub(1))
                .or_else(|| wt.terminals.first())
                .map(|terminal| terminal.id);
        }
    }

    pub fn restart_terminal(
        &mut self,
        id: u64,
        settings: &Settings,
        notifier: Option<Arc<dyn Notifier>>,
    ) -> Result<(), String> {
        let Some((wt_index, term_index)) = self.active_terminal_owner(id) else {
            return Err("terminal not found".into());
        };
        let wt = &mut self.worktrees[wt_index];
        let old = &wt.terminals[term_index];
        let command = old.spawn_command.clone();
        let title = old.title.clone();
        let kind = old.kind;
        let shell = settings.terminal.shell.clone();
        let session = lazecode_terminal::spawn(SpawnOptions {
            shell,
            command,
            cwd: wt.path.clone(),
            cols: 110,
            rows: 28,
            scrollback: settings.terminal.scrollback,
            notifier,
            kind,
        })
        .map_err(|err| err.to_string())?;
        wt.terminals[term_index].session = session;
        wt.terminals[term_index].title = title;
        wt.terminals[term_index].started = Instant::now();
        wt.active_terminal = Some(id);
        Ok(())
    }
}

pub struct AppModel {
    pub settings: Settings,
    pub projects: Vec<ProjectState>,
    pub active_project: Option<usize>,
    pub ui: UiState,
    pub toasts: Vec<Toast>,
    pub recent: Vec<PathBuf>,
    pub commands: Vec<Command>,
    pub notifier: Option<Arc<dyn Notifier>>,
    pub should_quit: bool,
    pub version: &'static str,
    /// Favorite local branches the branch tree decorates/pins.
    pub favorite_branches: HashSet<String>,
}

impl AppModel {
    pub fn new(settings: Settings, notifier: Option<Arc<dyn Notifier>>) -> Self {
        let workspace = WorkspaceFile::load();
        let mut model = Self {
            ui: UiState {
                sidebar_width: settings.sidebar_width,
                panel_height: settings.panel_height,
                show_minimap: settings.editor.show_minimap,
                sidebar_visible: workspace.sidebar_visible.unwrap_or(true),
                panel_visible: workspace.panel_visible.unwrap_or(true),
                sidebar_view: match workspace.sidebar_view.as_deref() {
                    Some("git") => SidebarView::Git,
                    Some("consoles") => SidebarView::Consoles,
                    _ => SidebarView::Files,
                },
                ..Default::default()
            },
            settings,
            projects: Vec::new(),
            active_project: None,
            toasts: Vec::new(),
            recent: workspace.recent.clone(),
            commands: Vec::new(),
            notifier,
            should_quit: false,
            version: env!("CARGO_PKG_VERSION"),
            favorite_branches: workspace.favorite_branches.iter().cloned().collect(),
        };
        model.restore_workspace(workspace);
        if model.projects.is_empty() {
            model.ui.sidebar_visible = false;
            model.ui.panel_visible = false;
        }
        model
    }

    fn restore_workspace(&mut self, workspace: WorkspaceFile) {
        let mut restored = Vec::new();
        for path in &workspace.projects {
            if let Ok(index) = self.open_project(path.clone()) {
                restored.push((index, path.clone()));
            }
        }
        if let Some(active) = &workspace.active_project {
            if let Some((index, _)) = restored.iter().find(|(_, path)| path == active) {
                self.active_project = Some(*index);
            }
        }
        for (index, path) in &restored {
            if let Some(tabs) = workspace.open_tabs.get(path) {
                for tab_path in tabs {
                    let _ = self.projects[*index].open_file(tab_path.clone(), &self.settings);
                }
            }
            if let Some(active_tab) = workspace.active_tabs.get(path) {
                if let Some(tab) = self.projects[*index].tabs.iter().find(
                    |tab| matches!(&tab.kind, TabKind::File(file) if &file.doc.path == active_tab),
                ) {
                    self.projects[*index].active_tab = Some(tab.id);
                }
            }
            if let Some(name) = workspace.active_worktrees.get(path) {
                if let Some(worktree_index) = self.projects[*index].worktree_index(name) {
                    self.projects[*index].active_worktree = worktree_index;
                }
            }
        }
        if self.active_project.is_none() {
            self.active_project = if self.projects.is_empty() {
                None
            } else {
                Some(0)
            };
        }
    }

    pub fn active_project(&self) -> Option<&ProjectState> {
        self.active_project
            .and_then(|index| self.projects.get(index))
    }

    pub fn active_project_mut(&mut self) -> Option<&mut ProjectState> {
        self.active_project
            .and_then(|index| self.projects.get_mut(index))
    }

    pub fn project_at(&self, index: usize) -> Option<&ProjectState> {
        self.projects.get(index)
    }

    pub fn project_at_mut(&mut self, index: usize) -> Option<&mut ProjectState> {
        self.projects.get_mut(index)
    }

    pub fn open_project(&mut self, path: PathBuf) -> Result<usize, String> {
        let path = canonical(&path);
        if let Some(index) = self
            .projects
            .iter()
            .position(|project| project.root == path)
        {
            self.active_project = Some(index);
            return Ok(index);
        }
        let project = ProjectState::open(path, &self.settings, self.notifier.clone())?;
        Ok(self.add_loaded_project(project))
    }

    /// Insert a project that was loaded on a worker thread.
    pub fn add_loaded_project(&mut self, project: ProjectState) -> usize {
        if let Some(index) = self
            .projects
            .iter()
            .position(|existing| existing.root == project.root)
        {
            self.active_project = Some(index);
            return index;
        }
        let path = project.root.clone();
        let name = project.name.clone();
        let was_empty = self.projects.is_empty();
        let index = self.projects.len();
        self.projects.push(project);
        if was_empty {
            self.ui.sidebar_visible = true;
            self.ui.panel_visible = true;
        }
        self.active_project = Some(index);
        self.recent.retain(|recent| recent != &path);
        self.recent.insert(0, path);
        self.recent.truncate(12);
        let message = if self.settings.language.is_zh() {
            format!("已打开 {name}")
        } else {
            format!("Opened {name}")
        };
        self.toast(message, ToastKind::Success);
        index
    }

    pub fn index_of_root(&self, root: &Path) -> Option<usize> {
        self.projects
            .iter()
            .position(|project| project.root == root)
    }

    pub fn close_project(&mut self, index: usize) {
        if index >= self.projects.len() {
            return;
        }
        self.projects.remove(index);
        self.active_project = match self.active_project {
            Some(active) if active == index => {
                if self.projects.is_empty() {
                    None
                } else {
                    Some(active.min(self.projects.len() - 1))
                }
            }
            Some(active) if active > index => Some(active - 1),
            other => other,
        };
    }

    pub fn switch_project(&mut self, index: usize) {
        if index < self.projects.len() {
            self.active_project = Some(index);
        }
    }

    pub fn queue(&mut self, command: Command) {
        self.commands.push(command);
    }

    pub fn drain_commands(&mut self) -> Vec<Command> {
        std::mem::take(&mut self.commands)
    }

    pub fn toast(&mut self, message: impl Into<String>, kind: ToastKind) {
        let message = message.into();
        self.toasts.push(Toast {
            message,
            kind,
            expires: Instant::now() + Duration::from_secs(4),
        });
        if self.toasts.len() > 5 {
            self.toasts.remove(0);
        }
    }

    pub fn tick(&mut self) {
        let now = Instant::now();
        self.toasts.retain(|toast| toast.expires > now);
    }

    pub fn total_running(&self) -> usize {
        self.projects
            .iter()
            .flat_map(|project| project.worktrees.iter())
            .flat_map(|worktree| worktree.terminals.iter())
            .filter(|terminal| terminal.session.is_running())
            .count()
    }

    pub fn total_changes(&self) -> usize {
        self.projects
            .iter()
            .map(|project| project.changes().len())
            .sum()
    }

    pub fn toggle_theme(&mut self) {
        self.settings.theme = match self.settings.theme {
            ThemeMode::Dark => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::Dark,
        };
        let _ = self.settings.save();
    }

    pub fn set_language(&mut self, language: Language) {
        self.settings.language = language;
        let _ = self.settings.save();
    }

    pub fn save_settings(&self) {
        if let Err(err) = self.settings.save() {
            log::warn!("cannot save settings: {err}");
        }
    }

    pub fn save_workspace(&mut self) {
        let mut file = WorkspaceFile {
            recent: self.recent.clone(),
            projects: self
                .projects
                .iter()
                .map(|project| project.root.clone())
                .collect(),
            active_project: self.active_project().map(|project| project.root.clone()),
            sidebar_view: Some(
                match self.ui.sidebar_view {
                    SidebarView::Files => "files",
                    SidebarView::Git => "git",
                    SidebarView::Consoles => "consoles",
                }
                .to_string(),
            ),
            sidebar_visible: Some(self.ui.sidebar_visible),
            sidebar_width: Some(self.ui.sidebar_width),
            panel_visible: Some(self.ui.panel_visible),
            panel_height: Some(self.ui.panel_height),
            open_tabs: BTreeMap::new(),
            active_tabs: BTreeMap::new(),
            active_worktrees: BTreeMap::new(),
            favorite_branches: self.favorite_branches.iter().cloned().collect(),
        };
        for project in &self.projects {
            let files: Vec<PathBuf> = project
                .tabs
                .iter()
                .filter_map(|tab| match &tab.kind {
                    TabKind::File(file) => Some(file.doc.path.clone()),
                    TabKind::Diff(_) => None,
                })
                .collect();
            file.open_tabs.insert(project.root.clone(), files);
            if let Some(active) = project.active_tab {
                if let Some(tab) = project.tab_by_id(active) {
                    if let TabKind::File(file_tab) = &tab.kind {
                        file.active_tabs
                            .insert(project.root.clone(), file_tab.doc.path.clone());
                    }
                }
            }
            file.active_worktrees
                .insert(project.root.clone(), project.active_worktree().name.clone());
        }
        if let Err(err) = file.save() {
            log::warn!("cannot save workspace: {err}");
        }
    }

    pub fn project_summary(&self) -> String {
        let name = self
            .active_project()
            .map(|project| project.name.clone())
            .unwrap_or_else(|| "LazeCode".to_string());
        name
    }
}

fn initials_of(name: &str) -> String {
    let letters: String = name.chars().filter(|ch| ch.is_alphanumeric()).collect();
    if letters.is_empty() {
        "LC".to_string()
    } else if letters.chars().count() == 1 {
        letters.to_uppercase()
    } else {
        letters.chars().take(2).collect::<String>().to_uppercase()
    }
}

fn tone_for(path: &Path) -> (u8, u8, u8) {
    const TONES: [(u8, u8, u8); 8] = [
        (168, 189, 131),
        (138, 180, 255),
        (185, 167, 255),
        (240, 198, 116),
        (245, 169, 127),
        (240, 166, 202),
        (87, 217, 163),
        (251, 139, 139),
    ];
    let mut hash = 1469598103934665603u64;
    for byte in path.to_string_lossy().bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    TONES[(hash % TONES.len() as u64) as usize]
}
