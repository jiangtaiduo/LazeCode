use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Instant;

use lazecode_fs::tree::FileTree;
use lazecode_fs::ProjectWatcher;
use lazecode_git::{BranchInfo, CommitDetails, CommitInfo, FileDiff, LogOptions, RepoSnapshot};
use lazecode_terminal::{Notifier, Session, SessionKind};
use lazecode_text::Document;

use crate::commands::Command;
use crate::lsp::LspManager;
use crate::settings::{canonical, Settings};

static NEXT_TAB: AtomicU64 = AtomicU64::new(1);
static NEXT_TERMINAL: AtomicU64 = AtomicU64::new(1);
mod app;
mod project;
mod tabs;
mod terminals;
/// Hard cap per worktree. Agent consoles get a separate global cap, but a
/// worktree still needs room for its initial shell plus ten agents.
const MAX_TERMINALS_PER_WORKTREE: usize = 12;
/// Agent consoles are much heavier than a shell. Keep the default bounded
/// even when many projects/worktrees are open.
const AGENT_SCROLLBACK_LINES: usize = 4_000;
const AGENT_COLD_BYTES: usize = 16 * 1024 * 1024;
/// Stable `MAX_PANES_PER_TAB`: a tab splits into at most four panes.
pub const MAX_PANES_PER_TAB: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SidebarView {
    Files,
    Git,
    Consoles,
    /// 稳定版 Compare Versions：Changes Between 抽屉（专属会话）。
    Compare,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PanelTab {
    Console,
    Git,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum TerminalPlacement {
    #[default]
    Bottom,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum GitSubtab {
    #[default]
    Log,
    Console,
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

/// The part of the workspace layout that belongs to one checkout. The editor
/// tabs and terminal tabs already live on [`WorktreeState`]; this snapshot
/// covers the surrounding panes, so switching projects/worktrees restores the
/// surface the user was looking at instead of carrying the previous layout
/// across checkouts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct WorktreeViewState {
    pub sidebar_visible: bool,
    pub sidebar_view: SidebarView,
    pub panel_visible: bool,
    pub panel_maximised: bool,
    pub panel_tab: PanelTab,
    pub git_subtab: GitSubtab,
    pub terminal_visible: bool,
    pub terminal_maximised: bool,
}

impl Default for WorktreeViewState {
    fn default() -> Self {
        Self {
            sidebar_visible: true,
            sidebar_view: SidebarView::Files,
            panel_visible: true,
            panel_maximised: false,
            panel_tab: PanelTab::Console,
            git_subtab: GitSubtab::Log,
            terminal_visible: false,
            terminal_maximised: false,
        }
    }
}

impl WorktreeViewState {
    fn capture(ui: &UiState) -> Self {
        Self {
            sidebar_visible: ui.sidebar_visible,
            sidebar_view: ui.sidebar_view,
            panel_visible: ui.panel_visible,
            panel_maximised: ui.panel_maximised,
            panel_tab: ui.panel_tab,
            git_subtab: ui.git_subtab,
            terminal_visible: ui.terminal_visible,
            terminal_maximised: ui.terminal_maximised,
        }
    }

    /// Compare sessions are deliberately not persisted across restarts, so a
    /// persisted layout falls back to the regular Git drawer.
    fn persisted(self) -> Self {
        let mut view = self;
        if view.sidebar_view == SidebarView::Compare {
            view.sidebar_view = SidebarView::Git;
        }
        view
    }

    fn apply(self, ui: &mut UiState) {
        ui.sidebar_visible = self.sidebar_visible;
        ui.sidebar_view = self.sidebar_view;
        ui.panel_visible = self.panel_visible;
        ui.panel_maximised = self.panel_maximised;
        ui.panel_tab = self.panel_tab;
        ui.git_subtab = self.git_subtab;
        ui.terminal_visible = self.terminal_visible;
        ui.terminal_maximised = self.terminal_maximised;
    }
}

pub struct UiState {
    pub sidebar_visible: bool,
    pub sidebar_view: SidebarView,
    pub sidebar_width: f32,
    /// Commit 抽屉（源控视图）宽度，独立于文件树抽屉持久化（稳定版语义）。
    pub commit_width: f32,
    /// Compare Versions 抽屉宽度（默认 320，可拖动右缘调整）。
    pub compare_width: f32,
    pub panel_visible: bool,
    /// 终端 dock 独立可见（稳定版 grid：main / dock / bottom / status 四行）。
    pub terminal_visible: bool,
    /// Git log 分支树搜索框开关（稳定版 branchSearchOpen，🔍 按钮切换）。
    pub branch_search_open: bool,
    pub panel_tab: PanelTab,
    pub panel_height: f32,
    /// 终端 dock 高度（稳定版默认 340）。
    pub terminal_height: f32,
    pub terminal_placement: TerminalPlacement,
    pub terminal_width: f32,
    pub git_subtab: GitSubtab,
    pub panel_maximised: bool,
    pub terminal_maximised: bool,
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
    pub git_details_card_height: f32,
    pub git_jump_to_commit: Option<String>,
    pub git_log_compact: bool,
    pub git_log_show_graph: bool,
    pub goto_line_open: bool,
    pub goto_line_input: String,
    /// One-shot: 「Commit and Push」的提交已成功，shell 下一次绘制时打开
    /// Push 对话框并复位（JobResult 消费点置位，UI 消费）。
    pub open_push_dialog: bool,
    /// 自动定位树中打开的文件（对齐稳定版 Always Select Opened File，默认 true）。
    pub auto_locate: bool,
    /// 树中选中的文件路径（用于高亮和 reveal）。
    pub tree_selected: Option<std::path::PathBuf>,
    /// 树中多选的文件集合（支持 Cmd/Ctrl、Shift 多选）。
    pub tree_selection: std::collections::HashSet<std::path::PathBuf>,
    /// 滚动定位请求（路径 + 模式：quiet 模式为 nearest，非 quiet 为 center）。
    pub tree_reveal: Option<(std::path::PathBuf, bool)>,
    /// 树定位闪烁的目标路径及触发时间（单次定位时高亮/闪烁）。
    pub locate_flash: Option<(std::path::PathBuf, std::time::Instant)>,
    /// Branch comparison session (data plus both UI sides), kept across
    /// dialog openings so the last comparison survives.
    pub compare_branches: CompareBranchesState,
    /// Saved layout per checkout path. The active checkout is mirrored into
    /// this map whenever the workspace is saved so a restart can restore it.
    pub worktree_views: HashMap<PathBuf, WorktreeViewState>,
    /// Checkout whose layout is currently mirrored into the fields above.
    pub view_target: Option<PathBuf>,
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

/// Git-log detail column cache for a multi-commit selection. The `key` covers
/// both the diff endpoints and the selected commits, so changing the
/// selection invalidates even a range with identical endpoints.
#[derive(Clone, Debug, Default)]
pub struct CommitRangeDetails {
    pub key: String,
    pub from: String,
    pub to: String,
    pub count: usize,
    pub files: Vec<lazecode_git::FileStat>,
    pub error: Option<String>,
    pub loading: bool,
}

impl CommitRangeDetails {
    /// Re-key an already loaded file list for a new selection. The detail
    /// column keeps painting the previous rows while the new range is fetched,
    /// and every action still uses the new endpoints.
    pub fn rekey_from(&self, next: &Self) -> Self {
        Self {
            key: next.key.clone(),
            from: next.from.clone(),
            to: next.to.clone(),
            count: next.count,
            files: self.files.clone(),
            error: None,
            loading: true,
        }
    }
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
            compare_width: 320.0,
            panel_visible: true,
            // Consoles open explicitly; the home screen must not show the
            // toggle as active before one exists.
            terminal_visible: false,
            branch_search_open: false,
            panel_tab: PanelTab::Console,
            panel_height: 248.0,
            terminal_height: 340.0,
            terminal_placement: TerminalPlacement::Bottom,
            terminal_width: 480.0,
            git_subtab: GitSubtab::Log,
            panel_maximised: false,
            terminal_maximised: false,
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
            git_details_card_height: 132.0,
            git_jump_to_commit: None,
            git_log_compact: false,
            git_log_show_graph: true,
            goto_line_open: false,
            goto_line_input: String::new(),
            open_push_dialog: false,
            auto_locate: true,
            tree_selected: None,
            tree_selection: std::collections::HashSet::new(),
            tree_reveal: None,
            locate_flash: None,
            compare_branches: CompareBranchesState::default(),
            worktree_views: HashMap::new(),
            view_target: None,
        }
    }
}

impl UiState {
    /// A user opened an editor target by double-clicking a file or change.
    /// If the console is covering the whole stage, get it out of the way.
    /// Keep `terminal_maximised` so re-opening the console later still uses
    /// the user's chosen full-height layout.
    pub fn reveal_editor_area(&mut self) {
        if self.terminal_visible && self.terminal_maximised {
            self.terminal_visible = false;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskStamp {
    pub modified: Option<std::time::SystemTime>,
    pub len: u64,
}

impl DiskStamp {
    pub fn read(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        Some(Self {
            modified: metadata.modified().ok(),
            len: metadata.len(),
        })
    }
}

pub struct FileTab {
    pub doc: Document,
    pub external: bool,
    /// Tab label override for read-only snapshots ("README.md (HEAD)");
    /// the document keeps its real path for syntax/language detection.
    pub title_override: Option<String>,
    pub md_mode: MarkdownViewMode,
    pub annotate: bool,
    pub blame: Option<Vec<lazecode_git::BlameLine>>,
    /// Last observed disk metadata, checked periodically so a missed watcher
    /// event cannot leave a stale buffer.
    pub disk_stamp: Option<DiskStamp>,
}

impl FileTab {
    pub fn stamp_disk(&mut self) {
        self.disk_stamp = DiskStamp::read(&self.doc.path);
    }

    pub fn disk_changed(&self) -> bool {
        match DiskStamp::read(&self.doc.path) {
            Some(stamp) => self.disk_stamp != Some(stamp),
            // A file removed elsewhere is an external change too; reload
            // surfaces the error instead of keeping the old buffer.
            None => self.disk_stamp.is_some(),
        }
    }
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
    /// Index (stage 0) ↔ working tree, for the changes-list
    /// 「Compare with Local」 action.
    Index {
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
    /// 贮藏中单个文件的差异（基线 ↔ 贮藏内容，含未跟踪快照）。
    Stash {
        stash: String,
        path: String,
    },
    /// 贮藏文件的「更改前」版本 ↔ 当前工作区（Compare Before with Local）。
    StashBeforeLocal {
        stash: String,
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
    /// Current change-group index (Monaco `getLineChanges()` semantics:
    /// contiguous edit runs, not git hunks — see `count_change_groups`).
    pub current_hunk: usize,
    /// Edge-escalation state for the balloon: 0 none, +1 = "last difference"
    /// shown (next ▼ opens the next file), -1 = first-difference shown.
    pub edge_pending: i8,
    /// First reveal done: after load, the first group is scrolled into view
    /// exactly once (stable `revealedRef`).
    pub revealed: bool,
}

pub enum TabKind {
    File(FileTab),
    Diff(DiffTab),
    /// 分支比较页（稳定版 compare-branches 中央标签页；单实例）。
    Compare(CompareTab),
    Image(ImageTab),
}

#[derive(Clone, Debug)]
pub struct ImageTab {
    pub path: PathBuf,
    pub external: bool,
}

/// Whether the path is a plain JSON document. JSONC is deliberately excluded
/// because its comments cannot be round-tripped by the formatter.
pub fn is_json_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
}

pub fn is_image_path(path: &std::path::Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "ico" | "bmp" | "svg" | "icns"
    )
}

/// 分支比较标签页：只保存方向身份，数据在 `UiState::compare_branches`。
/// 稳定版 CompareDrawer 的会话状态：两个提交间的差异文件清单。
#[derive(Clone, Debug, Default)]
pub struct CompareSession {
    pub from: String,
    pub to: String,
    /// `true` = `from` ↔ 本地工作区（Compare with Local）；此时 `to` 为空，
    /// 文件清单来自提交树与工作区的全量比较，双击文件打开
    /// `DiffSource::CompareWithLocal`。
    pub local: bool,
    pub minimized: bool,
    pub loading: bool,
    pub files: Vec<lazecode_git::CompareFile>,
    /// 目录树展开序（目录优先、字典序），供 diff 页 ←/→ 跨文件导航。
    pub nav_order: Vec<String>,
    pub group_by_dir: bool,
    pub selected: Option<String>,
    pub root_expanded: bool,
    pub collapsed: std::collections::HashSet<String>,
}

/// 目录树展开序：与稳定版 CompareDrawer renderDirNodes 一致 —— 每层
/// 目录在前、文件在后，同层按名字排序。
pub fn compare_nav_order(files: &[lazecode_git::CompareFile]) -> Vec<String> {
    #[derive(Default)]
    struct Node {
        dirs: std::collections::BTreeMap<String, Node>,
        files: Vec<String>,
    }
    let mut root = Node::default();
    for file in files {
        let parts: Vec<&str> = file.path.split('/').filter(|p| !p.is_empty()).collect();
        let mut cur = &mut root;
        for part in &parts[..parts.len().saturating_sub(1)] {
            cur = cur.dirs.entry((*part).to_string()).or_default();
        }
        if let Some(name) = parts.last() {
            cur.files.push((*name).to_string());
        }
    }
    fn walk(node: &Node, prefix: &str, out: &mut Vec<String>) {
        for (name, dir) in &node.dirs {
            let child = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            walk(dir, &child, out);
        }
        for name in &node.files {
            out.push(if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            });
        }
    }
    let mut out = Vec::new();
    walk(&root, "", &mut out);
    out
}

/// Empty tree id (SHA-1), matching the existing Compare-panel range rule: a
/// selection whose oldest commit is a root diffs from the empty tree so the
/// root commit's files are included.
pub const EMPTY_TREE_ID: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// Resolve a Git-log multi-selection to the diff range shown in the detail
/// column: parent of the oldest selected commit → newest selected commit.
/// `Project.log` is newest-first, so the smallest position is the newest
/// endpoint. Selections outside the loaded log are ignored.
pub fn selected_commit_range(
    log: &[CommitInfo],
    selected: &[String],
) -> Option<CommitRangeDetails> {
    if selected.len() < 2 {
        return None;
    }
    let mut positions: Vec<usize> = selected
        .iter()
        .filter_map(|id| log.iter().position(|commit| &commit.id == id))
        .collect();
    positions.sort_unstable();
    positions.dedup();
    if positions.len() < 2 {
        return None;
    }
    let newest = positions[0];
    let oldest = *positions.last()?;
    let from = log[oldest]
        .parents
        .first()
        .cloned()
        .unwrap_or_else(|| EMPTY_TREE_ID.to_string());
    let to = log[newest].id.clone();
    let ids = positions
        .iter()
        .map(|position| log[*position].id.as_str())
        .collect::<Vec<_>>()
        .join(",");
    Some(CommitRangeDetails {
        key: format!("r:{from}..{to}|{ids}"),
        from,
        to,
        count: positions.len(),
        ..CommitRangeDetails::default()
    })
}

fn kind_rank(kind: lazecode_git::ChangeKind) -> u8 {
    match kind {
        lazecode_git::ChangeKind::Modified => 0,
        lazecode_git::ChangeKind::Renamed | lazecode_git::ChangeKind::TypeChanged => 1,
        lazecode_git::ChangeKind::Added => 2,
        lazecode_git::ChangeKind::Deleted => 3,
    }
}

/// Merge a range file list by path. `compare_commits` already emits one row
/// per path, but the detail tree must never duplicate a path even when the
/// input comes from several diff sources.
pub fn aggregate_commit_files(files: &[lazecode_git::CompareFile]) -> Vec<lazecode_git::FileStat> {
    let mut merged: HashMap<String, lazecode_git::FileStat> = HashMap::new();
    let mut order = Vec::new();
    for file in files {
        let kind = lazecode_git::ChangeKind::from_status(file.status);
        if let Some(stat) = merged.get_mut(&file.path) {
            stat.additions = stat.additions.saturating_add(file.additions);
            stat.deletions = stat.deletions.saturating_add(file.deletions);
            if kind_rank(kind) > kind_rank(stat.kind) {
                stat.kind = kind;
            }
        } else {
            merged.insert(
                file.path.clone(),
                lazecode_git::FileStat {
                    path: file.path.clone(),
                    kind,
                    additions: file.additions,
                    deletions: file.deletions,
                    binary: false,
                },
            );
            order.push(file.path.clone());
        }
    }
    order
        .into_iter()
        .filter_map(|path| merged.remove(&path))
        .collect()
}

#[derive(Clone, Debug)]
pub struct CompareTab {
    pub left: String,
    pub right: String,
    /// 标签页标题（打开时按语言生成）。
    pub title: String,
}

pub struct Tab {
    pub id: u64,
    /// Checkout this editor tab belongs to. Tabs are kept in one project-wide
    /// registry so background jobs and save-all can still reach an inactive
    /// worktree, but only the active checkout's tabs are rendered.
    pub worktree: PathBuf,
    pub kind: TabKind,
}

impl Tab {
    pub fn title(&self) -> String {
        match &self.kind {
            TabKind::File(file) => file
                .title_override
                .clone()
                .unwrap_or_else(|| file.doc.title()),
            TabKind::Compare(compare) => compare.title.clone(),
            TabKind::Image(img) => img
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Image".to_string()),
            TabKind::Diff(diff) => match &diff.source {
                DiffSource::Worktree { .. } => "Working Tree Diff".to_string(),
                DiffSource::Index { .. } => "Index Diff".to_string(),
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
                DiffSource::Stash { stash, .. } => {
                    format!("Diff ({stash})")
                }
                DiffSource::StashBeforeLocal { .. } => "Diff (Before ↔ Local)".to_string(),
                DiffSource::Files { .. } => diff
                    .path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&diff.path)
                    .to_string(),
            },
        }
    }
    pub fn path(&self) -> Option<&str> {
        match &self.kind {
            TabKind::File(file) => file.doc.path.to_str(),
            TabKind::Diff(diff) => Some(diff.path.as_str()),
            TabKind::Image(img) => img.path.to_str(),
            TabKind::Compare(_) => None,
        }
    }
    pub fn is_dirty(&self) -> bool {
        matches!(&self.kind, TabKind::File(file) if file.doc.dirty)
    }
}

/// One PTY inside a terminal tab. Stable keeps one session per split pane;
/// the tab only groups them for display.
pub struct TerminalPane {
    pub id: u64,
    pub session: Session,
    pub title: String,
    pub kind: SessionKind,
    pub spawn_command: Option<String>,
    pub started: Instant,
}

/// How the panes of a terminal tab are arranged (stable
/// `.terminal-split-row` / `.terminal-split-column`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaneSplit {
    /// One pane filling the tab.
    #[default]
    None,
    /// Panes side by side (`Split Right`).
    Row,
    /// Panes stacked (`Split Down`).
    Column,
}

pub struct TerminalTab {
    pub id: u64,
    /// At least one pane; a tab disappears when its last pane closes.
    pub panes: Vec<TerminalPane>,
    /// Pane that owns keyboard focus and whose title the tab shows.
    pub active_pane: u64,
    pub split: PaneSplit,
}

impl TerminalTab {
    pub fn is_running(&self) -> bool {
        self.panes.iter().any(|pane| pane.session.is_running())
    }

    pub fn pane(&self, id: u64) -> Option<&TerminalPane> {
        self.panes.iter().find(|pane| pane.id == id)
    }

    pub fn pane_mut(&mut self, id: u64) -> Option<&mut TerminalPane> {
        self.panes.iter_mut().find(|pane| pane.id == id)
    }

    /// Tab label: the active pane's title, like stable's tab `name`.
    pub fn title(&self) -> &str {
        self.pane(self.active_pane)
            .or_else(|| self.panes.first())
            .map(|pane| pane.title.as_str())
            .unwrap_or_default()
    }
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
    /// Terminal tabs of this worktree; each tab holds one or more panes.
    pub terminals: Vec<TerminalTab>,
    /// Active **pane** id (the pane that owns focus). Stable's
    /// `activeTabByProject` is derived from the tab that owns this pane.
    pub active_terminal: Option<u64>,
    /// Last active editor tab while this checkout was focused.
    pub active_editor_tab: Option<u64>,
    pub watcher: Option<ProjectWatcher>,
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
    pub fn is_git_ignored(&self, rel_path: &std::path::Path) -> bool {
        self.git
            .as_ref()
            .is_some_and(|git| is_snapshot_ignored(git, rel_path))
    }

    pub fn tab_by_id(&self, tab_id: u64) -> Option<&TerminalTab> {
        self.terminals.iter().find(|tab| tab.id == tab_id)
    }

    pub fn tab_by_id_mut(&mut self, tab_id: u64) -> Option<&mut TerminalTab> {
        self.terminals.iter_mut().find(|tab| tab.id == tab_id)
    }

    /// Tab that owns `pane_id`.
    pub fn tab_of_pane(&self, pane_id: u64) -> Option<&TerminalTab> {
        self.terminals
            .iter()
            .find(|tab| tab.pane(pane_id).is_some())
    }

    pub fn pane(&self, pane_id: u64) -> Option<&TerminalPane> {
        self.terminals.iter().find_map(|tab| tab.pane(pane_id))
    }

    /// Tab containing the active pane (stable's active tab).
    pub fn active_tab(&self) -> Option<&TerminalTab> {
        let pane_id = self.active_terminal?;
        self.tab_of_pane(pane_id).or_else(|| self.terminals.first())
    }

    pub fn active_tab_id(&self) -> Option<u64> {
        self.active_tab().map(|tab| tab.id)
    }

    /// Number of PTYs in this worktree (used for the per-worktree cap).
    pub fn pane_count(&self) -> usize {
        self.terminals.iter().map(|tab| tab.panes.len()).sum()
    }

    /// Load the tree/git/watcher state of one checkout without spawning a
    /// console. Callers attach consoles afterwards via `open_terminal_in`.
    pub fn load(name: String, path: PathBuf, is_main: bool, ignored_patterns: &[String]) -> Self {
        let tree = lazecode_fs::tree::load_tree_filtered(&path, 20_000, Some(ignored_patterns));
        let watcher = lazecode_fs::watch_project(&path, ignored_patterns).ok();
        let (git, git_error) = match lazecode_git::snapshot(&path) {
            Ok(snapshot) => (Some(snapshot), None),
            Err(err) => {
                let message = err.to_string();
                if is_not_a_git_repository(&message) {
                    (None, None)
                } else {
                    (None, Some(message))
                }
            }
        };
        Self::from_loaded(name, path, is_main, tree, watcher, git, git_error)
    }

    /// Build a worktree from already-loaded state. Used for the main checkout,
    /// whose watcher/tree/snapshot are owned by [`ProjectState`], so opening a
    /// project does not scan or watch the same root twice.
    #[allow(clippy::too_many_arguments)]
    pub fn from_loaded(
        name: String,
        path: PathBuf,
        is_main: bool,
        tree: FileTree,
        watcher: Option<ProjectWatcher>,
        git: Option<lazecode_git::RepoSnapshot>,
        git_error: Option<String>,
    ) -> Self {
        Self {
            name,
            path,
            is_main,
            tree,
            git,
            git_error,
            terminals: Vec::new(),
            active_terminal: None,
            active_editor_tab: None,
            watcher,
            git_loading: false,
            log_branch: None,
        }
    }
}

/// Filter state of the Git log view (persisted per project until changed).
#[derive(Clone, Debug, PartialEq)]
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

impl Default for LogQuery {
    fn default() -> Self {
        Self {
            text: String::new(),
            branch: String::new(),
            author: String::new(),
            path: String::new(),
            since: 0,
            date: DatePreset::All,
            all_refs: true,
            first_parent: false,
            no_merges: false,
            topo_order: false,
            use_regex: false,
            case_sensitive: false,
        }
    }
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
    set.insert("Local".to_string());
    set.insert("Remote".to_string());
    set.insert("Tags".to_string());
    for branch in branches {
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
    pub root_expanded: bool,
    pub expanded: HashSet<PathBuf>,
    pub git: Option<RepoSnapshot>,
    pub git_error: Option<String>,
    pub log: Vec<CommitInfo>,
    /// Bumped whenever `log` changes. The Git-log view derives its filtered
    /// list and lane layout from it and caches both on this generation, so a
    /// per-frame recompute cannot go stale.
    pub log_generation: u64,
    pub log_error: Option<String>,
    pub branches: Vec<BranchInfo>,
    pub branch_query: String,
    /// Expanded directory paths of the Git log branch tree.
    pub ref_dirs_expanded: std::collections::HashSet<String>,
    /// Branch-tree multi-selection in click order: comparing exactly two uses
    /// the first as left and the second as right.
    pub ref_selection: Vec<String>,
    /// Directories whose children are hidden in the commit-details file tree.
    /// Empty means every directory is expanded.
    pub commit_file_dirs_collapsed: std::collections::HashSet<String>,
    pub commit_files_root_collapsed: bool,
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
    /// Cached file list for a multi-commit Git-log selection.
    pub commit_range: Option<CommitRangeDetails>,
    /// Key of the last requested range, so the detail poll does not re-spawn
    /// a failed job for the same selection every frame.
    pub commit_range_attempted: Option<String>,

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
    pub git_loading: bool,
    pub log_loading: bool,
    /// Compare Versions 会话（稳定版 CompareDrawer）。
    pub compare_session: Option<CompareSession>,
    /// Per-stash tracked/untracked groups (`stash@{n}` → details). Loaded on
    /// demand by the Stash tab and the diff tab's cross-file navigation; the
    /// UI and background refresh invalidate it through `invalidate_stash_details`.
    pub stash_details: HashMap<String, lazecode_git::StashDetails>,
}

pub struct AppModel {
    pub settings: Settings,
    pub projects: Vec<ProjectState>,
    pub active_project: Option<usize>,
    pub ui: UiState,
    pub toasts: Vec<Toast>,
    pub recent: Vec<PathBuf>,
    /// Recently opened files, independent of the directory recents. Files
    /// opened inside projects and in the ad-hoc workspace both land here.
    pub recent_files: Vec<PathBuf>,
    pub commands: Vec<Command>,
    pub notifier: Option<Arc<dyn Notifier>>,
    /// Optional language-server sessions + aggregated diagnostics.
    pub lsp: LspManager,
    pub should_quit: bool,
    pub version: &'static str,
    /// Favorite local branches the branch tree decorates/pins.
    pub favorite_branches: HashSet<String>,
    pub git_cmd_log: Vec<String>,
}

/// A missing repository is an expected state of an ordinary folder, not a
/// red Git failure. Keep the snapshot/error projection empty so the drawer can
/// show one actionable "initialize" empty state.
fn is_not_a_git_repository(message: &str) -> bool {
    message.contains("not a Git repository")
}

fn retarget_absolute(path: &Path, old: &Path, new: &Path) -> Option<PathBuf> {
    if path == old {
        return Some(new.to_path_buf());
    }
    path.strip_prefix(old).ok().map(|rest| new.join(rest))
}
/// Retarget a checkout-relative tree path after `old` became `new`.
///
/// Tree rows, `expanded` and the one-shot navigation state are relative to the
/// checkout root. Comparing them with absolute paths (as `retarget_absolute`
/// does) can never match, so keep both sides relative to the same root.
fn retarget_checkout_relative(path: &Path, old: &Path, new: &Path, root: &Path) -> Option<PathBuf> {
    let old_rel = old.strip_prefix(root).ok()?;
    let new_rel = new.strip_prefix(root).ok()?;
    if path == old_rel {
        return Some(new_rel.to_path_buf());
    }
    path.strip_prefix(old_rel)
        .ok()
        .map(|rest| new_rel.join(rest))
}

/// Canonicalise the surviving parent and re-attach the *original* file name.
///
/// The old path itself is gone after a rename, and canonicalising it is not
/// only useless but wrong on the default (case-insensitive) macOS filesystem:
/// `canonicalize("…/readme.md")` still succeeds after `readme.md` became
/// `README.md` and folds the name to the new case, making a case-only rename
/// compare equal and skip every retarget.
fn canonical_after_rename(path: &Path) -> PathBuf {
    let Some(parent) = path.parent() else {
        return path.to_path_buf();
    };
    let Some(name) = path.file_name() else {
        return path.to_path_buf();
    };
    parent
        .canonicalize()
        .map(|parent| parent.join(name))
        .unwrap_or_else(|_| path.to_path_buf())
}

fn relative_slash(path: &Path, root: &Path) -> Option<String> {
    path.strip_prefix(root)
        .ok()
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
}

fn retarget_relative(value: &str, old: Option<&str>, new: Option<&str>) -> Option<String> {
    let old = old?;
    let new = new?;
    if value == old {
        return Some(new.to_string());
    }
    let rest = value.strip_prefix(old)?.strip_prefix('/')?;
    Some(if rest.is_empty() {
        new.to_string()
    } else {
        format!("{new}/{rest}")
    })
}

fn diff_source_path(source: &mut DiffSource) -> Option<&mut String> {
    match source {
        DiffSource::Files { .. } => None,
        DiffSource::Worktree { path }
        | DiffSource::Index { path }
        | DiffSource::Commit { path, .. }
        | DiffSource::CompareWithLocal { path, .. }
        | DiffSource::CompareCommits { path, .. }
        | DiffSource::Stash { path, .. }
        | DiffSource::StashBeforeLocal { path, .. } => Some(path),
    }
}

fn initials_of(name: &str) -> String {
    let letters: Vec<char> = name.chars().filter(|ch| ch.is_alphanumeric()).collect();
    if letters.is_empty() {
        return "LC".to_string();
    }

    let segments: Vec<&str> = name
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|segment| !segment.is_empty())
        .collect();
    let mut initials = Vec::with_capacity(2);
    if segments.len() > 1 {
        for segment in segments {
            if let Some(ch) = segment.chars().next() {
                initials.push(ch);
                if initials.len() == 2 {
                    break;
                }
            }
        }
    } else {
        let has_lower = letters.iter().any(|ch| ch.is_lowercase());
        let has_upper = letters.iter().any(|ch| ch.is_uppercase());
        if has_lower && has_upper {
            // camelCase / PascalCase: word-initial capitals, plus a lowercase
            // first letter (it marks the first word just as a capital does).
            if letters[0].is_lowercase() {
                initials.push(letters[0]);
            }
            for ch in &letters {
                if ch.is_uppercase() {
                    initials.push(*ch);
                    if initials.len() == 2 {
                        break;
                    }
                }
            }
        } else {
            initials.extend(letters.iter().take(2).copied());
        }
    }

    // Short words (e.g. "Foo") may yield only one capital; fill to two so the
    // rail badge stays visually stable whenever the name has enough letters.
    for ch in &letters {
        if initials.len() == 2 {
            break;
        }
        if !initials
            .iter()
            .any(|chosen| chosen.to_uppercase().eq(ch.to_uppercase()))
        {
            initials.push(*ch);
        }
    }
    initials
        .into_iter()
        .take(2)
        .collect::<String>()
        .to_uppercase()
}

/// Project identity colors. Keep red and green out of this palette: those
/// hues are reserved for the console activity ring and its output runner.
const PROJECT_TONES: [(u8, u8, u8); 10] = [
    (138, 180, 255), // blue
    (185, 167, 255), // violet
    (240, 198, 116), // gold
    (129, 140, 248), // indigo
    (240, 166, 202), // pink
    (103, 232, 249), // cyan
    (245, 169, 127), // orange
    (196, 181, 253), // lavender
    (56, 189, 248),  // sky
    (232, 121, 249), // magenta
];

fn tone_for(path: &Path) -> (u8, u8, u8) {
    let mut hash = 1469598103934665603u64;
    for byte in path.to_string_lossy().bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    PROJECT_TONES[(hash % PROJECT_TONES.len() as u64) as usize]
}

fn unique_project_tone(
    used: impl Iterator<Item = (u8, u8, u8)>,
    fallback: (u8, u8, u8),
) -> (u8, u8, u8) {
    let used: HashSet<(u8, u8, u8)> = used.collect();
    if !used.contains(&fallback) {
        return fallback;
    }
    PROJECT_TONES
        .iter()
        .copied()
        .find(|tone| !used.contains(tone))
        .unwrap_or(fallback)
}
/// Longest matching worktree root wins; standalone files register their
/// parent directory so `current_dir` is valid at spawn.
fn lsp_root_for(project: &ProjectState, path: &Path) -> PathBuf {
    project
        .worktrees
        .iter()
        .filter(|worktree| path.starts_with(&worktree.path))
        .max_by_key(|worktree| worktree.path.components().count())
        .map(|worktree| worktree.path.clone())
        .unwrap_or_else(|| project.root.clone())
}

/// Helper to determine if a relative path (or any of its ancestors) is ignored by Git.
pub fn is_snapshot_ignored(git: &lazecode_git::RepoSnapshot, rel_path: &std::path::Path) -> bool {
    if rel_path.as_os_str().is_empty() {
        return false;
    }
    let rel_str = rel_path.to_string_lossy().replace('\\', "/");
    if git.ignored_files.binary_search(&rel_str).is_ok() {
        return true;
    }
    for ancestor in rel_path.ancestors() {
        if ancestor == rel_path || ancestor.as_os_str().is_empty() {
            continue;
        }
        let anc_str = ancestor.to_string_lossy().replace('\\', "/");
        if git.ignored_dirs.binary_search(&anc_str).is_ok() {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod initials_tests {
    use super::initials_of;

    #[test]
    fn project_initials_follow_camel_and_separator_rules() {
        // camelCase / PascalCase: capital letters, including a lowercase start.
        assert_eq!(initials_of("LazeCode"), "LC");
        assert_eq!(initials_of("lazeCode"), "LC");
        assert_eq!(initials_of("myAwesomeProject"), "MA");
        // Separated words: first letter of each segment.
        assert_eq!(initials_of("my project"), "MP");
        assert_eq!(initials_of("my-project"), "MP");
        assert_eq!(initials_of("my_project"), "MP");
        // Uniform case: first two letters.
        assert_eq!(initials_of("lazecode"), "LA");
        assert_eq!(initials_of("LAZECODE"), "LA");
        // At most two characters, even with many segments.
        assert_eq!(initials_of("one-two-three-four"), "OT");
        assert_eq!(initials_of("a"), "A");
        assert_eq!(initials_of("---"), "LC");
    }
}

#[cfg(test)]
mod workspace_restore_tests {
    use super::*;
    use crate::settings::Settings;

    fn blank_model(settings: Settings) -> AppModel {
        AppModel {
            settings: settings.clone(),
            projects: Vec::new(),
            active_project: None,
            ui: UiState::default(),
            toasts: Vec::new(),
            recent: Vec::new(),
            recent_files: Vec::new(),
            commands: Vec::new(),
            notifier: None,
            lsp: LspManager::new(false, None),
            should_quit: false,
            version: "test",
            favorite_branches: HashSet::new(),
            git_cmd_log: Vec::new(),
        }
    }

    #[test]
    fn workspace_restore_applies_saved_worktree_layout() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let view = WorktreeViewState {
            terminal_visible: true,
            panel_visible: false,
            sidebar_view: SidebarView::Git,
            panel_tab: PanelTab::Git,
            ..Default::default()
        };
        let mut workspace = crate::settings::WorkspaceFile {
            projects: vec![root.clone()],
            active_project: Some(root.clone()),
            ..Default::default()
        };
        workspace
            .worktree_views
            .entry(root)
            .or_default()
            .insert("main".to_string(), view);

        let mut model = blank_model(Settings::default());
        model.restore_workspace(workspace);
        model.sync_worktree_view();

        assert!(model.ui.terminal_visible);
        assert!(!model.ui.panel_visible);
        assert_eq!(model.ui.sidebar_view, SidebarView::Git);
        assert_eq!(model.ui.panel_tab, PanelTab::Git);
    }
}

#[cfg(test)]
mod commit_range_tests {
    use super::*;

    fn commit(id: &str, parent: &str) -> CommitInfo {
        CommitInfo {
            id: id.to_string(),
            short: id.to_string(),
            message: id.to_string(),
            body: String::new(),
            author: "test".to_string(),
            email: "test@example.com".to_string(),
            time: 0,
            parents: if parent.is_empty() {
                Vec::new()
            } else {
                vec![parent.to_string()]
            },
            in_head: None,
            refs: Vec::new(),
            lane: 0,
            lane_count: 1,
            edges: Vec::new(),
        }
    }

    #[test]
    fn selection_uses_parent_of_oldest_to_newest_in_log_order() {
        // Log is newest-first: c is the newest of these three, a the oldest.
        let log = [
            commit("c", "b"),
            commit("b", "a"),
            commit("a", "root"),
            commit("root", ""),
        ];
        let selected = vec!["b".to_string(), "a".to_string()];
        let range = selected_commit_range(&log, &selected).unwrap();
        assert_eq!(range.from, "root");
        assert_eq!(range.to, "b");
        assert_eq!(range.count, 2);
        // Click order must not affect the endpoints or the signature.
        let reversed = selected_commit_range(&log, &["a".to_string(), "b".to_string()]).unwrap();
        assert_eq!(reversed.key, range.key);

        let widened =
            selected_commit_range(&log, &["c".to_string(), "b".to_string(), "a".to_string()])
                .unwrap();
        assert_eq!(widened.from, "root");
        assert_eq!(widened.to, "c");
        assert_ne!(widened.key, range.key);

        assert!(selected_commit_range(&log, &["b".to_string()]).is_none());
    }

    #[test]
    fn root_selection_starts_at_the_empty_tree() {
        let log = [commit("b", "a"), commit("a", "")];
        let range = selected_commit_range(&log, &["b".to_string(), "a".to_string()]).unwrap();
        assert_eq!(range.from, EMPTY_TREE_ID);
        assert_eq!(range.to, "b");
    }

    #[test]
    fn aggregation_dedupes_paths_and_sums_change_stats() {
        let files = [
            lazecode_git::CompareFile {
                path: "src/a.rs".to_string(),
                status: 'A',
                additions: 4,
                deletions: 0,
            },
            lazecode_git::CompareFile {
                path: "src/a.rs".to_string(),
                status: 'M',
                additions: 2,
                deletions: 1,
            },
            lazecode_git::CompareFile {
                path: "src/b.rs".to_string(),
                status: 'D',
                additions: 0,
                deletions: 7,
            },
        ];
        let stats = aggregate_commit_files(&files);
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].path, "src/a.rs");
        assert_eq!(stats[0].kind, lazecode_git::ChangeKind::Added);
        assert_eq!((stats[0].additions, stats[0].deletions), (6, 1));
        assert_eq!(stats[1].kind, lazecode_git::ChangeKind::Deleted);
    }

    #[test]
    fn rekeying_keeps_the_previous_file_list_visible_while_loading() {
        let log = [commit("c", "b"), commit("b", "a"), commit("a", "")];
        let first = selected_commit_range(&log, &["b".into(), "a".into()]).unwrap();
        let second = selected_commit_range(&log, &["c".into(), "b".into()]).unwrap();
        let loaded = CommitRangeDetails {
            key: first.key.clone(),
            from: first.from.clone(),
            to: first.to.clone(),
            count: first.count,
            files: aggregate_commit_files(&[lazecode_git::CompareFile {
                path: "src/a.rs".into(),
                status: 'M',
                additions: 3,
                deletions: 1,
            }]),
            error: None,
            loading: false,
        };

        let pending = loaded.rekey_from(&second);
        assert_eq!(pending.files.len(), loaded.files.len());
        assert_eq!(pending.files[0].path, "src/a.rs");
        assert_eq!(
            (pending.files[0].additions, pending.files[0].deletions),
            (3, 1)
        );
        assert_eq!(pending.key, second.key);
        assert_eq!((pending.from.as_str(), pending.to.as_str()), ("a", "c"));
        assert!(pending.loading);
        assert!(pending.error.is_none());
    }
}
