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
    /// Commit 抽屉（源控视图）宽度，独立于文件树抽屉持久化（稳定版语义）。
    pub commit_width: f32,
    pub panel_visible: bool,
    /// 终端 dock 独立可见（稳定版 grid：main / dock / bottom / status 四行）。
    pub terminal_visible: bool,
    pub panel_tab: PanelTab,
    pub panel_height: f32,
    /// 终端 dock 高度（稳定版默认 340）。
    pub terminal_height: f32,
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
    pub goto_line_open: bool,
    pub goto_line_input: String,
    /// One-shot: 「Commit and Push」的提交已成功，shell 下一次绘制时打开
    /// Push 对话框并复位（JobResult 消费点置位，UI 消费）。
    pub open_push_dialog: bool,
    /// 分支比较会话（数据缓存 + 双侧 UI 状态）。稳定版的比较页是常驻
    /// 标签（换 ref 不丢状态），对话框方案下挂在 UiState 以便后台任务
    /// 回填，重开对话框保留上次比较。
    pub compare_branches: CompareBranchesState,
}

/// 详情列文件缓存：一次「选中提交 / 多选范围 / 整侧」的文件清单。
#[derive(Default, Clone, Debug)]
pub struct CompareDetail {
    /// `"c:{commit}"` 或 `"r:{from}..{to}"`，回填时匹配。
    pub key: String,
    pub loading: bool,
    pub error: Option<String>,
    pub files: Vec<lazecode_git::CompareFile>,
}

/// 日期过滤（对齐稳定版 compareBranches 的 Date 菜单）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareDateFilter {
    Today,
    Last7Days,
    Last30Days,
}

/// 单侧的 UI 状态（选择、过滤、树折叠），互不串扰。
#[derive(Default, Clone, Debug)]
pub struct CompareBranchesSideView {
    /// 选中的提交 id（显示顺序）。
    pub selected: Vec<String>,
    pub anchor: Option<String>,
    /// ⌘A 整侧全选：详情直接用该侧的 merge-base 文件清单。
    pub select_all: bool,
    pub collapsed_dirs: HashSet<String>,
    pub selected_file: Option<String>,
    pub query: String,
    pub regex: bool,
    pub match_case: bool,
    pub author: Option<String>,
    pub date: Option<CompareDateFilter>,
    /// Branch(ref) 过滤（按提交所属引用名过滤显示行）。
    pub selected_ref_filter: Option<String>,
    /// 本侧详情文件缓存（选中提交/范围 diff 的清单）。**每侧独立**：
    /// 两个 pane 不能共用，否则一侧的选择会污染另一侧的详情。
    pub detail: Option<CompareDetail>,
    /// 路径过滤（非空即触发重查）。
    pub path: String,
    pub path_edit_open: bool,
    pub path_edit: String,
}

/// 分支比较会话。`left_ref/right_ref` 是方向身份；job 回填时按
/// (refs, paths) 匹配，不匹配的结果丢弃。
#[derive(Default, Clone, Debug)]
pub struct CompareBranchesState {
    /// 会话所属项目（比较数据只对应该仓库）。跨项目切换时 refs 可能
    /// 同名，没有 root 就会把 A 仓库的数据填进 B 仓库的会话。
    pub root: PathBuf,
    pub left_ref: String,
    pub right_ref: String,
    pub left_path: String,
    pub right_path: String,
    /// 每侧提交加载的上限（Load More 递增）。
    pub limit: usize,
    pub sides: Option<lazecode_git::CompareBranches>,
    pub loading: bool,
    pub error: Option<String>,
    pub loading_more: [bool; 2],
    pub left_view: CompareBranchesSideView,
    pub right_view: CompareBranchesSideView,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            sidebar_visible: true,
            sidebar_view: SidebarView::Files,
            sidebar_width: 262.0,
            commit_width: 330.0,
            panel_visible: true,
            terminal_visible: true,
            panel_tab: PanelTab::Console,
            panel_height: 248.0,
            terminal_height: 340.0,
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
            goto_line_open: false,
            goto_line_input: String::new(),
            open_push_dialog: false,
            compare_branches: CompareBranchesState::default(),
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
    /// 分支比较页（稳定版 compare-branches 中央标签页；单实例）。
    Compare(CompareTab),
}

/// 分支比较标签页：只保存方向身份，数据在 `UiState::compare_branches`。
#[derive(Clone, Debug)]
pub struct CompareTab {
    pub left: String,
    pub right: String,
    /// 标签页标题（打开时按语言生成）。
    pub title: String,
}

pub struct Tab {
    pub id: u64,
    pub kind: TabKind,
}

impl Tab {
    pub fn title(&self) -> String {
        match &self.kind {
            TabKind::File(file) => file.doc.title(),
            TabKind::Compare(compare) => compare.title.clone(),
            TabKind::Diff(diff) => {
                // 稳定版 diff 标签标题（EditorTabBar）：路径不在标题里，
                // 文件名由 diff 视图内部标题栏展示；两种语言同文案。
                match &diff.source {
                    DiffSource::Worktree { .. } => "Working Tree Diff".to_string(),
                    DiffSource::Commit { commit, .. } => {
                        format!("Diff ({})", &commit[..commit.len().min(7)])
                    }
                    DiffSource::CompareWithLocal { commit, .. } => {
                        format!("Diff (Local ↔ {})", &commit[..commit.len().min(7)])
                    }
                    DiffSource::CompareCommits { from, to, .. } => format!(
                        "Diff ({} ↔ {})",
                        &from[..from.len().min(7)],
                        &to[..to.len().min(7)]
                    ),
                    DiffSource::Files { .. } => diff
                        .path
                        .rsplit('/')
                        .next()
                        .unwrap_or(&diff.path)
                        .to_string(),
                }
            }
        }
    }
    pub fn path(&self) -> Option<&str> {
        match &self.kind {
            TabKind::File(file) => file.doc.path.to_str(),
            TabKind::Diff(diff) => Some(diff.path.as_str()),
            TabKind::Compare(_) => None,
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
    /// 分支树多选（**点击顺序**保留：多选恰两个时「比较分支」以第 1 个
    /// 为 left、第 2 个为 right，对齐稳定版；HashSet 的随机序会导致左右
    /// 随机互换）。
    pub ref_selection: Vec<String>,
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
    /// True if the history walk has reached the root commit (no more commits to load).
    pub log_reached_end: bool,
    pub log_branch: Option<String>,
    pub selected_commit: Option<String>,
    pub selected_commits: Vec<String>,
    pub is_single_file: bool,
    pub commit_details: Option<CommitDetails>,
    pub commit_details_attempted: Option<String>,
    pub tabs: Vec<Tab>,
    pub active_tab: Option<u64>,
    pub commit_message: String,
    pub amend: bool,
    /// 对齐稳定版语义：勾选 = 包含进本次提交，默认全部不勾，提交范围
    /// 完全由用户显式勾选决定（正向集合；Amend 不受勾选限制）。
    pub checked_paths: HashSet<String>,
    /// Collapsed changelist groups of the source-control list.
    pub collapsed_lists: HashSet<String>,
    /// Multi-selection of the source-control file rows (repo-relative).
    pub change_selection: HashSet<String>,
    /// Anchor row of the selection (Shift-range base), in displayed order.
    pub change_anchor: Option<String>,
    /// Set when the user clicks 「Commit and Push」: on commit success the
    /// shell opens the push dialog (JobResult 消费点写回 UiState)。
    pub open_push_after_commit: bool,
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
        let path = canonical(&root);
        let (root, is_single_file, file_to_open) = if path.is_file() {
            let parent = path.parent().unwrap_or(&path).to_path_buf();
            (parent, true, Some(path.clone()))
        } else if path.is_dir() {
            (path, false, None)
        } else {
            return Err(format!("{} does not exist", path.display()));
        };
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
            ref_selection: Vec::new(),
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
            log_reached_end: false,
            log_branch: None,
            selected_commit: None,
            selected_commits: Vec::new(),
            is_single_file,
            commit_details: None,
            commit_details_attempted: None,
            tabs: Vec::new(),
            active_tab: None,
            commit_message: String::new(),
            amend: false,
            checked_paths: HashSet::new(),
            collapsed_lists: HashSet::new(),
            change_selection: HashSet::new(),
            change_anchor: None,
            open_push_after_commit: false,
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
        if let Some(first) = project.log.first() {
            project.selected_commits = vec![first.id.clone()];
        }
        if let Some(file_path) = file_to_open {
            let _ = project.open_file(file_path, settings);
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
            .filter(|change| self.checked_paths.contains(&change.path))
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
        // 勾选集合、行多选与锚点都按当前变更路径裁剪，消失的文件不留幽灵状态。
        self.checked_paths.retain(|path| existing.contains(path));
        self.change_selection.retain(|path| existing.contains(path));
        if let Some(anchor) = &self.change_anchor {
            if !existing.contains(anchor) {
                self.change_anchor = None;
            }
        }
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
        // 完全同源（同提交同文件）：只激活不重置，避免重复加载与后台任务
        // 去重（同 key 会被丢弃）竞争导致永远转圈。
        if let Some(tab) = self.tabs.iter().find(|tab| match &tab.kind {
            TabKind::Diff(diff) => diff.source == source,
            _ => false,
        }) {
            self.active_tab = Some(tab.id);
            return tab.id;
        }
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
        // 稳定版语义（App.tsx enterDiff）：diff 是全局单例标签，换文件/
        // 换提交/换来源只换内容不新开标签；任两文件互比（Files）不参与。
        if !matches!(source, DiffSource::Files { .. }) {
            let reuse = self.tabs.iter().position(|tab| {
                matches!(
                    &tab.kind,
                    TabKind::Diff(diff) if !matches!(diff.source, DiffSource::Files { .. })
                )
            });
            if let Some(index) = reuse {
                if let TabKind::Diff(diff) = &mut self.tabs[index].kind {
                    diff.source = source.clone();
                    diff.path = path;
                    diff.diff = None;
                    diff.loading = true;
                    diff.error = None;
                    diff.current_hunk = 0;
                }
                self.active_tab = Some(self.tabs[index].id);
                return self.tabs[index].id;
            }
        }
        let id = NEXT_TAB.fetch_add(1, Ordering::Relaxed);
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

    /// 打开分支比较页（单实例，对齐稳定版：再开同一功能只是更新方向并
    /// 激活既有标签页；标题在 UI 层填充为 i18n 文案）。
    pub fn open_compare_branches(&mut self, left: String, right: String, title: String) -> u64 {
        if let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|tab| matches!(tab.kind, TabKind::Compare(_)))
        {
            if let TabKind::Compare(compare) = &mut tab.kind {
                if compare.left != left || compare.right != right {
                    compare.left = left;
                    compare.right = right;
                    compare.title = title;
                }
            }
            self.active_tab = Some(tab.id);
            return tab.id;
        }
        let id = NEXT_TAB.fetch_add(1, Ordering::Relaxed);
        self.tabs.push(Tab {
            id,
            kind: TabKind::Compare(CompareTab { left, right, title }),
        });
        self.active_tab = Some(id);
        id
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

    pub fn close_other_tabs(&mut self, id: u64) {
        self.tabs.retain(|tab| tab.id == id);
        self.active_tab = Some(id);
    }

    pub fn close_all_tabs(&mut self) {
        self.tabs.clear();
        self.active_tab = None;
    }

    pub fn close_tabs_to_right(&mut self, id: u64) {
        if let Some(pos) = self.tabs.iter().position(|tab| tab.id == id) {
            self.tabs.truncate(pos + 1);
            if !self.tabs.iter().any(|tab| Some(tab.id) == self.active_tab) {
                self.active_tab = Some(id);
            }
        }
    }

    /// 关闭目标标签左侧的所有标签（稳定版「关闭左侧标签」，首标签禁用）。
    pub fn close_tabs_to_left(&mut self, id: u64) {
        if let Some(pos) = self.tabs.iter().position(|tab| tab.id == id) {
            self.tabs.drain(..pos);
            if !self.tabs.iter().any(|tab| Some(tab.id) == self.active_tab) {
                self.active_tab = Some(id);
            }
        }
    }

    pub fn close_unmodified_tabs(&mut self) {
        self.tabs.retain(|tab| tab.is_dirty());
        if !self.tabs.iter().any(|tab| Some(tab.id) == self.active_tab) {
            self.active_tab = self.tabs.first().map(|tab| tab.id);
        }
    }

    pub fn close_other_terminals(&mut self, id: u64) {
        let wt = self.active_worktree_mut();
        wt.terminals.retain(|term| term.id == id);
        wt.active_terminal = Some(id);
    }

    pub fn close_all_terminals(&mut self) {
        let wt = self.active_worktree_mut();
        wt.terminals.clear();
        wt.active_terminal = None;
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
                commit_width: settings.commit_drawer_width,
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
        let is_single_file = project.is_single_file;
        let was_empty = self.projects.is_empty();
        let index = self.projects.len();
        self.projects.push(project);
        if was_empty {
            self.ui.sidebar_visible = true;
            self.ui.panel_visible = true;
        }
        self.active_project = Some(index);
        if !is_single_file {
            self.recent.retain(|recent| recent != &path);
            self.recent.insert(0, path);
            self.recent.truncate(12);
        }
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
                .filter(|p| !p.is_single_file)
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
            commit_drawer_width: Some(self.ui.commit_width),
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
                    TabKind::Diff(_) | TabKind::Compare(_) => None,
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
