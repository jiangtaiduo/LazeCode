//! Project lifecycle and Git state: opening, tree/log/branch refresh, worktree
//! attach/switch and path retargeting.

use super::*;
use lazecode_fs::tree::NodeKind;

impl ProjectState {
    /// Virtual "Single files" workspace for files chosen via Open: no tree,
    /// Git state or watcher, consoles start in HOME.
    pub fn single_files() -> Self {
        // Virtual checkout for ad-hoc files: no tree, Git state or watcher;
        // consoles override their cwd to HOME.
        let root = crate::settings::Settings::config_dir().join("Single files");
        let _ = std::fs::create_dir_all(&root);
        let name = "Single files".to_string();
        let initials = "SF".to_string();
        let tone = tone_for(&root);
        let worktree = WorktreeState {
            name: "main".to_string(),
            path: root.clone(),
            is_main: true,
            tree: lazecode_fs::tree::FileTree::default(),
            git: None,
            git_error: None,
            terminals: Vec::new(),
            active_terminal: None,
            active_editor_tab: None,
            watcher: None,
            git_loading: false,
            log_branch: None,
        };
        Self {
            root,
            name,
            initials,
            tone,
            root_expanded: false,
            expanded: HashSet::new(),
            git: None,
            git_error: None,
            log: Vec::new(),
            log_generation: 0,
            log_error: None,
            branches: Vec::new(),
            branch_query: String::new(),
            ref_dirs_expanded: std::collections::HashSet::new(),
            ref_selection: Vec::new(),
            commit_file_dirs_collapsed: std::collections::HashSet::new(),
            commit_files_root_collapsed: false,
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
            log_reached_end: true,
            log_branch: None,
            selected_commit: None,
            selected_commits: Vec::new(),
            is_single_file: true,
            commit_details: None,
            commit_details_attempted: None,
            commit_range: None,
            commit_range_attempted: None,
            tabs: Vec::new(),
            active_tab: None,
            commit_message: String::new(),
            amend: false,
            checked_paths: HashSet::new(),
            collapsed_lists: HashSet::new(),
            change_selection: HashSet::new(),
            change_anchor: None,
            open_push_after_commit: false,
            worktrees: vec![worktree],
            active_worktree: 0,
            watcher: None,
            git_loading: false,
            log_loading: false,
            compare_session: None,
            stash_details: HashMap::new(),
        }
    }

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
        let tree =
            lazecode_fs::tree::load_tree_filtered(&root, 20_000, Some(&settings.ignored_patterns));
        let watcher = lazecode_fs::watch_project(&root, &settings.ignored_patterns).ok();
        let (git, git_error) = match lazecode_git::snapshot(&root) {
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
        let (log, branches, log_error) = if git.is_some() {
            match (
                lazecode_git::log(
                    &root,
                    LogOptions {
                        all_refs: true,
                        ..LogOptions::default()
                    },
                ),
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
        // Tags belong to the initial branch menu; the log refresh reloads them
        // later, but waiting for it would leave the first menu without Tags.
        let tags = if git.is_some() {
            lazecode_git::tags(&root).unwrap_or_default()
        } else {
            Vec::new()
        };
        let mut project = Self {
            root,
            name,
            initials,
            tone,
            root_expanded: true,
            expanded: HashSet::new(),
            git,
            git_error,
            log,
            log_generation: 0,
            log_error,
            branches,
            branch_query: String::new(),
            // The branch tree starts fully expanded (PyCharm default):
            ref_dirs_expanded: init_dirs,
            ref_selection: Vec::new(),
            commit_file_dirs_collapsed: std::collections::HashSet::new(),
            commit_files_root_collapsed: false,
            tags,
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
            commit_range: None,
            commit_range_attempted: None,
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
            git_loading: false,
            log_loading: false,
            compare_session: None,
            stash_details: HashMap::new(),
        };
        // The worktree projection must exist before branch/changes accessors
        // are used (refresh_branches reads the active branch for pending count).
        // The project owns the main checkout's watcher/tree/snapshot; creating
        // another WorktreeState::load here would duplicate all three and leave
        // an unread watcher channel growing in the background.
        let main_worktree = WorktreeState::from_loaded(
            "main".to_string(),
            project.root.clone(),
            true,
            tree,
            None,
            project.git.clone(),
            project.git_error.clone(),
        );
        project.worktrees.push(main_worktree);
        // Populate remotes/tags/ahead-behind in the initial frame so the
        // branch menu, Push dialog and Remotes dialog are usable immediately.
        project.refresh_branches();
        if let Some(first) = project.log.first() {
            project.selected_commit = Some(first.id.clone());
        }
        if let Some(first) = project.log.first() {
            project.selected_commits = vec![first.id.clone()];
        }
        if let Some(file_path) = file_to_open {
            let _ = project.open_file(file_path, settings);
        }
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
    pub fn reload_tree(&mut self, settings: &Settings) {
        if self.is_single_file {
            return;
        }
        let root = self.active_root().to_path_buf();
        // Keep children for collapsed folders loaded. Expansion is then a
        // presentation-only change and cannot trigger a synchronous rescan.
        let tree =
            lazecode_fs::tree::load_tree_filtered(&root, 20_000, Some(&settings.ignored_patterns));
        let expanded: Vec<PathBuf> = self.expanded.iter().cloned().collect();
        self.active_worktree_mut().tree = tree;
        // A fresh bounded scan can skip folders that were already open; keep
        // them populated so expansion still shows their contents.
        for rel in expanded {
            self.ensure_tree_children(&rel, &settings.ignored_patterns);
        }
    }

    /// Fill a folder's direct children when the bounded scan skipped them.
    pub fn ensure_tree_children(&mut self, rel: &Path, ignored_patterns: &[String]) -> bool {
        let root = self.active_root().to_path_buf();
        let tree = &mut self.active_worktree_mut().tree;
        let Some(node) = lazecode_fs::tree::find_mut(tree, rel) else {
            return false;
        };
        if node.kind != NodeKind::Folder || node.children_loaded {
            return false;
        }
        node.children =
            lazecode_fs::tree::load_children(&root, rel, Some(ignored_patterns), 20_000);
        node.children_loaded = true;
        tree.file_count = lazecode_fs::file_count(tree);
        true
    }

    /// Recreate watchers after the ignore list changed. Watchers keep a
    /// snapshot of the patterns, so a settings edit must rebuild them.
    pub fn rebuild_watchers(&mut self, settings: &Settings) {
        if self.is_single_file {
            return;
        }
        self.watcher = lazecode_fs::watch_project(&self.root, &settings.ignored_patterns).ok();
        for worktree in self.worktrees.iter_mut().skip(1) {
            worktree.watcher =
                lazecode_fs::watch_project(&worktree.path, &settings.ignored_patterns).ok();
        }
    }

    /// Root of the checkout currently shown in the editor/source-control pane.
    pub fn active_root(&self) -> &Path {
        &self.active_worktree().path
    }

    /// Git snapshot of the focused worktree.
    pub fn active_git(&self) -> Option<&RepoSnapshot> {
        self.active_worktree().git.as_ref()
    }

    pub fn active_git_error(&self) -> Option<&str> {
        self.active_worktree().git_error.as_deref()
    }

    pub fn active_git_loading(&self) -> bool {
        self.active_worktree().git_loading
    }

    pub fn active_tree(&self) -> &FileTree {
        &self.active_worktree().tree
    }

    pub fn branch(&self) -> String {
        self.active_worktree().branch()
    }

    pub fn changes(&self) -> &[lazecode_git::Change] {
        self.active_git()
            .map(|git| git.changes.as_slice())
            .unwrap_or(&[])
    }

    pub fn ignored_files(&self) -> &[String] {
        self.active_git()
            .map(|git| git.ignored_files.as_slice())
            .unwrap_or(&[])
    }

    pub fn ignored_dirs(&self) -> &[String] {
        self.active_git()
            .map(|git| git.ignored_dirs.as_slice())
            .unwrap_or(&[])
    }

    pub fn is_git_ignored(&self, rel_path: &std::path::Path) -> bool {
        self.active_git()
            .is_some_and(|git| is_snapshot_ignored(git, rel_path))
    }

    pub fn checked_changes(&self) -> Vec<String> {
        self.changes()
            .iter()
            .filter(|change| self.checked_paths.contains(&change.path))
            .map(|change| change.path.clone())
            .collect()
    }

    pub fn refresh_git(&mut self) {
        if self.is_single_file {
            self.git_loading = false;
            return;
        }
        self.git_loading = true;
        match lazecode_git::snapshot(&self.root) {
            Ok(snapshot) => {
                self.git = Some(snapshot);
                self.git_error = None;
            }
            Err(err) => {
                let message = err.to_string();
                if is_not_a_git_repository(&message) {
                    self.git = None;
                    self.git_error = None;
                } else {
                    self.git_error = Some(message);
                }
            }
        }
        self.prune_unchecked();
        self.git_loading = false;
    }

    /// Refresh the Git snapshot of one worktree by its path.
    pub fn refresh_git_for(&mut self, path: &Path) {
        if self.is_single_file {
            return;
        }
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
            Err(err) => {
                let message = err.to_string();
                if is_not_a_git_repository(&message) {
                    worktree.git = None;
                    worktree.git_error = None;
                } else {
                    worktree.git_error = Some(message);
                }
            }
        }
        worktree.git_loading = false;
    }

    /// Apply a completed Git snapshot job to the matching checkout and keep
    /// the compatibility projection in sync when that checkout is focused.
    pub fn apply_git_snapshot(
        &mut self,
        path: &Path,
        result: Result<RepoSnapshot, String>,
    ) -> bool {
        let is_main = self.root == path;
        let worktree_index = self
            .worktrees
            .iter()
            .position(|worktree| worktree.path == path);
        let Some(worktree_index) = worktree_index else {
            return false;
        };
        let active = self.active_worktree == worktree_index;
        let worktree = &mut self.worktrees[worktree_index];
        worktree.git_loading = false;
        match result {
            Ok(snapshot) => {
                if is_main || active {
                    self.git = Some(snapshot.clone());
                    self.git_error = None;
                }
                worktree.git = Some(snapshot);
                worktree.git_error = None;
            }
            Err(err) => {
                if is_not_a_git_repository(&err) {
                    if is_main || active {
                        self.git = None;
                        self.git_error = None;
                    }
                    worktree.git = None;
                    worktree.git_error = None;
                } else {
                    if is_main || active {
                        self.git_error = Some(err.clone());
                    }
                    worktree.git_error = Some(err);
                }
            }
        }
        if active {
            self.prune_unchecked();
        }
        true
    }

    pub fn prune_unchecked_public(&mut self) {
        self.prune_unchecked();
    }

    pub(crate) fn prune_unchecked(&mut self) {
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
        if self.active_git().is_none() {
            return;
        }
        let root = self.active_root().to_path_buf();
        self.log_loading = true;
        match lazecode_git::log(&root, LogOptions::default()) {
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
                self.log_generation = self.log_generation.wrapping_add(1);
                if !self
                    .log
                    .iter()
                    .any(|commit| Some(&commit.id) == self.selected_commit.as_ref())
                {
                    self.selected_commit = self.log.first().map(|commit| commit.id.clone());
                    self.selected_commits = self
                        .log
                        .first()
                        .map(|commit| vec![commit.id.clone()])
                        .unwrap_or_default();
                    self.commit_details = None;
                    self.commit_details_attempted = None;
                    self.commit_range = None;
                    self.commit_range_attempted = None;
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
        if self.active_git().is_none() {
            return;
        }
        let root = self.active_root().to_path_buf();
        self.conflicts = lazecode_git::conflict_list(&root).unwrap_or_default();
        self.sequencer = lazecode_git::sequencer_state(&root).ok();
        if self.conflicts.is_empty() && self.sequencer == Some(lazecode_git::SequencerState::None) {
            self.sequencer = None;
        }
    }

    pub fn active_tab_index(&self) -> Option<usize> {
        let id = self.active_tab?;
        self.active_worktree_tabs().position(|tab| tab.id == id)
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

    /// Editor tabs of the checkout currently shown in the tab bar.
    pub fn active_worktree_tabs(&self) -> impl Iterator<Item = &Tab> + '_ {
        let root = self.active_root();
        self.tabs.iter().filter(move |tab| tab.worktree == root)
    }

    /// Activate an editor tab only when it belongs to the focused checkout.
    pub fn activate_tab(&mut self, id: u64) {
        let Some(tab) = self.tab_by_id(id) else {
            return;
        };
        if tab.worktree != self.active_root() {
            return;
        }
        self.set_active_tab(id);
    }

    pub(crate) fn set_active_tab(&mut self, id: u64) {
        self.active_tab = Some(id);
        let index = self
            .active_worktree
            .min(self.worktrees.len().saturating_sub(1));
        if let Some(worktree) = self.worktrees.get_mut(index) {
            worktree.active_editor_tab = Some(id);
        }
    }

    pub fn worktree_index(&self, name: &str) -> Option<usize> {
        self.worktrees.iter().position(|wt| wt.name == name)
    }

    pub(crate) fn focus_worktree(&mut self, index: usize) {
        let index = index.min(self.worktrees.len().saturating_sub(1));
        // Remember the tab the user was looking at before leaving the old
        // checkout, then restore the target's own tab. Tabs stay in one
        // registry for background jobs, but only the active checkout renders.
        if let Some(previous) = self.worktrees.get_mut(self.active_worktree) {
            previous.active_editor_tab = self.active_tab;
        }
        self.active_worktree = index;
        let target_path = self.worktrees[index].path.clone();
        self.active_tab = self.worktrees[index]
            .active_editor_tab
            .filter(|id| {
                self.tabs
                    .iter()
                    .any(|tab| tab.id == *id && tab.worktree == target_path)
            })
            .or_else(|| {
                self.tabs
                    .iter()
                    .rev()
                    .find(|tab| tab.worktree == target_path)
                    .map(|tab| tab.id)
            });
        self.worktrees[index].active_editor_tab = self.active_tab;
        // Checked paths, multi-selection and anchors are repo-relative but
        // belong to the previously visible checkout. Carrying them across a
        // switch would commit/stage files in the wrong worktree.
        self.checked_paths.clear();
        self.change_selection.clear();
        self.change_anchor = None;
        // Conflicts/sequencer state is project-level but belongs to the active
        // checkout; carrying it across a worktree switch showed a stale
        // conflict banner over a clean tree.
        self.refresh_conflicts();
    }

    /// Switch to an attached worktree or attach an existing one; returns whether
    /// it was newly attached.
    pub fn ensure_worktree(
        &mut self,
        name: &str,
        ignored_patterns: &[String],
    ) -> Result<Option<()>, String> {
        if let Some(index) = self.worktree_index(name) {
            self.focus_worktree(index);
            return Ok(None);
        }
        // A worktree registered in git but not attached yet: load it cold
        // (no console spawns automatically; a worktree keeps its own set).
        let all = lazecode_git::list_worktrees(&self.root).map_err(|err| err.to_string())?;
        let Some(info) = all.into_iter().find(|worktree| worktree.name == name) else {
            return Err(format!("worktree '{name}' does not exist"));
        };
        let worktree = WorktreeState::load(info.name, info.path, false, ignored_patterns);
        let index = self.worktrees.len();
        self.worktrees.push(worktree);
        self.focus_worktree(index);
        Ok(Some(()))
    }

    /// Switch to an attached worktree and rescan its tree from disk. The scan
    /// is intentional: a checkout can change while another one is focused.
    pub fn switch_worktree(
        &mut self,
        name: &str,
        settings: &Settings,
    ) -> Result<Option<()>, String> {
        let attached = self.ensure_worktree(name, &settings.ignored_patterns)?;
        self.reload_tree(settings);
        Ok(attached)
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
            self.focus_worktree(index);
            return;
        }
        self.worktrees.push(worktree);
        self.focus_worktree(self.worktrees.len() - 1);
    }

    /// Drop the model entry for a linked worktree after git removal.
    pub fn detach_worktree(&mut self, name: &str) {
        let Some(index) = self.worktrees.iter().position(|wt| wt.name == name) else {
            return;
        };
        let removed_path = self.worktrees[index].path.clone();
        self.worktrees.remove(index);
        self.tabs.retain(|tab| tab.worktree != removed_path);
        if self.active_worktree >= self.worktrees.len() {
            self.active_worktree = self.worktrees.len().saturating_sub(1);
        } else if self.active_worktree > index {
            self.active_worktree -= 1;
        }
        let active_root = self.active_root().to_path_buf();
        self.active_tab = self
            .tabs
            .iter()
            .rev()
            .find(|tab| tab.worktree == active_root)
            .map(|tab| tab.id);
        if let Some(worktree) = self.worktrees.get_mut(self.active_worktree) {
            worktree.active_editor_tab = self.active_tab;
        }
        self.checked_paths.clear();
        self.change_selection.clear();
        self.change_anchor = None;
    }

    /// Point open tabs and path-keyed project state at a renamed file or
    /// directory. The caller performs the filesystem rename first; this keeps
    /// buffers, tab titles and Git-relative selections consistent so the file
    /// watcher does not try to reload the old path.
    pub fn retarget_path(&mut self, old: &Path, new: &Path) -> bool {
        let old = canonical_after_rename(old);
        let new = new.canonicalize().unwrap_or_else(|_| new.to_path_buf());

        // Tree-relative state belongs to the checkout that owns the renamed
        // path; linked worktrees are not under `self.root`.
        let owner_root = self
            .worktrees
            .iter()
            .find(|worktree| old.starts_with(&worktree.path))
            .map(|worktree| worktree.path.clone())
            .unwrap_or_else(|| self.root.clone());
        if old == new {
            return false;
        }
        let mut changed = false;
        for tab in &mut self.tabs {
            let worktree = tab.worktree.clone();
            let old_rel = relative_slash(&old, &worktree);
            let new_rel = relative_slash(&new, &worktree);
            match &mut tab.kind {
                TabKind::File(file) => {
                    if let Some(path) = retarget_absolute(&file.doc.path, &old, &new) {
                        file.doc.retarget(path);
                        file.stamp_disk();
                        changed = true;
                    }
                }
                TabKind::Image(image) => {
                    if let Some(path) = retarget_absolute(&image.path, &old, &new) {
                        image.path = path;
                        changed = true;
                    }
                }
                TabKind::Diff(diff) => {
                    if let DiffSource::Files { old_path, new_path } = &mut diff.source {
                        if let Some(path) = retarget_absolute(old_path, &old, &new) {
                            *old_path = path;
                            changed = true;
                        }
                        if let Some(path) = retarget_absolute(new_path, &old, &new) {
                            *new_path = path;
                            changed = true;
                        }
                    } else if let Some(path) = diff_source_path(&mut diff.source) {
                        if let Some(updated) =
                            retarget_relative(path, old_rel.as_deref(), new_rel.as_deref())
                        {
                            *path = updated;
                            changed = true;
                        }
                    }
                    if let Some(updated) =
                        retarget_relative(&diff.path, old_rel.as_deref(), new_rel.as_deref())
                    {
                        diff.path = updated;
                        changed = true;
                    }
                }
                TabKind::Compare(_) => {}
            }
        }
        let old_rel = relative_slash(&old, &owner_root);
        let new_rel = relative_slash(&new, &owner_root);
        let retarget_set = |set: &mut HashSet<String>| {
            if old_rel.is_none() || new_rel.is_none() {
                return false;
            }
            let mut changed = false;
            let mut next = HashSet::with_capacity(set.len());
            for path in set.drain() {
                match retarget_relative(&path, old_rel.as_deref(), new_rel.as_deref()) {
                    Some(updated) => {
                        changed = true;
                        next.insert(updated);
                    }
                    None => {
                        next.insert(path);
                    }
                }
            }
            *set = next;
            changed
        };
        changed |= retarget_set(&mut self.checked_paths);
        changed |= retarget_set(&mut self.change_selection);
        let mut expanded = HashSet::with_capacity(self.expanded.len());
        for path in self.expanded.drain() {
            expanded
                .insert(retarget_checkout_relative(&path, &old, &new, &owner_root).unwrap_or(path));
        }
        self.expanded = expanded;
        changed
    }

    /// Cached file groups of one stash; loads (one native git pass) on first
    /// use. Clone so callers can keep rendering while mutating the model.
    pub fn stash_details_for(&mut self, stash: &str) -> Option<lazecode_git::StashDetails> {
        if let Some(details) = self.stash_details.get(stash) {
            return Some(details.clone());
        }
        let root = self.active_root().to_path_buf();
        let details = lazecode_git::stash_details(&root, stash).ok()?;
        self.stash_details
            .insert(stash.to_string(), details.clone());
        Some(details)
    }

    /// Every changed path of a stash (tracked, then untracked) — the ordering
    /// the stash tree and the diff tab's ←/→ navigation use.
    pub fn stash_paths_for(&mut self, stash: &str) -> Vec<String> {
        let Some(details) = self.stash_details_for(stash) else {
            return Vec::new();
        };
        let mut paths = details.tracked;
        paths.extend(details.untracked);
        paths
    }

    pub fn invalidate_stash_details(&mut self) {
        self.stash_details.clear();
    }

    /// Expand every ancestor directory of `path` in the tree view.
    pub fn expand_parents(&mut self, path: &Path, ignored_patterns: &[String]) {
        let Ok(rel) = path.strip_prefix(self.active_root()) else {
            return;
        };
        let mut current = PathBuf::new();
        for component in rel.components() {
            current.push(component);
            if current.as_os_str().is_empty() {
                continue;
            }
            self.expanded.insert(current.clone());
            self.ensure_tree_children(&current, ignored_patterns);
        }
    }
}
