//! App-level model operations: project open/switch, workspace restore, toasts,
//! command queuing, LSP and settings persistence.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::lsp::{self, ServerKey};
use crate::settings::{Language, ThemeMode, WorkspaceFile};

use super::*;

impl AppModel {
    pub fn new(settings: Settings, notifier: Option<Arc<dyn Notifier>>) -> Self {
        let workspace = WorkspaceFile::load();
        let lsp = LspManager::new(settings.lsp_enabled, notifier.clone());
        let mut model = Self {
            ui: UiState {
                sidebar_width: settings.sidebar_width,
                commit_width: settings.commit_drawer_width,
                compare_width: settings.compare_drawer_width,
                panel_height: settings.panel_height,
                show_minimap: settings.editor.show_minimap,
                sidebar_visible: workspace.sidebar_visible.unwrap_or(true),
                panel_visible: workspace.panel_visible.unwrap_or(true),
                sidebar_view: match workspace.sidebar_view.as_deref() {
                    Some("git") => SidebarView::Git,
                    Some("consoles") => SidebarView::Consoles,
                    _ => SidebarView::Files,
                },
                terminal_placement: match workspace.terminal_placement.as_deref() {
                    Some("right") => TerminalPlacement::Right,
                    _ => TerminalPlacement::Bottom,
                },
                terminal_width: workspace.terminal_width.unwrap_or(480.0),
                terminal_height: workspace.terminal_height.unwrap_or(340.0),
                git_subtab: GitSubtab::Log,
                ..Default::default()
            },
            settings,
            projects: Vec::new(),
            active_project: None,
            toasts: Vec::new(),
            recent: workspace.recent.clone(),
            recent_files: workspace.recent_files.clone(),
            commands: Vec::new(),
            notifier,
            lsp,
            should_quit: false,
            version: env!("CARGO_PKG_VERSION"),
            favorite_branches: workspace.favorite_branches.iter().cloned().collect(),
            git_cmd_log: Vec::new(),
        };
        model.restore_workspace(workspace);
        model.migrate_favorite_branches();
        if model.projects.is_empty() {
            model.ui.sidebar_visible = false;
            model.ui.panel_visible = false;
        }
        model
    }

    fn favorite_branch_key(root: &Path, branch: &str) -> String {
        // Branch names repeat across projects; qualify persisted favourites
        // with the checkout root so favouriting `main` in one repo cannot
        // decorate every other project's `main`.
        format!("{}\u{1}{branch}", root.display())
    }

    pub fn is_favorite_branch(&self, root: &Path, branch: &str) -> bool {
        self.favorite_branches
            .contains(&Self::favorite_branch_key(root, branch))
    }

    pub fn toggle_favorite_branch(&mut self, root: &Path, branch: &str) {
        let key = Self::favorite_branch_key(root, branch);
        if !self.favorite_branches.remove(&key) {
            self.favorite_branches.insert(key);
        }
    }

    fn migrate_favorite_branches(&mut self) {
        // Older workspace files stored bare branch names globally. Apply them
        // once to the active project instead of decorating every checkout.
        let legacy: Vec<String> = self
            .favorite_branches
            .iter()
            .filter(|key| !key.contains('\u{1}'))
            .cloned()
            .collect();
        if legacy.is_empty() {
            return;
        }
        let root = self
            .active_project()
            .map(|project| project.root.clone())
            .or_else(|| self.projects.first().map(|project| project.root.clone()));
        if let Some(root) = root {
            for branch in legacy {
                self.favorite_branches
                    .insert(Self::favorite_branch_key(&root, &branch));
            }
        }
        self.favorite_branches.retain(|key| key.contains('\u{1}'));
    }

    pub(crate) fn restore_workspace(&mut self, workspace: WorkspaceFile) {
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
        let ignored_patterns = self.settings.ignored_patterns.clone();
        for (index, path) in &restored {
            let project = &mut self.projects[*index];
            // Attach saved linked worktrees before opening tabs: open_file
            // assigns each path to its owning checkout.
            if let Some(names) = workspace.open_worktrees.get(path) {
                for name in names {
                    if name != "main" {
                        let _ = project.ensure_worktree(name, &ignored_patterns);
                    }
                }
            }
            if let Some(name) = workspace.active_worktrees.get(path) {
                let _ = project.ensure_worktree(name, &ignored_patterns);
            }
            if let Some(tabs) = workspace.open_tabs.get(path) {
                for tab_path in tabs {
                    let _ = project.open_file(tab_path.clone(), &self.settings);
                }
            }
            if let Some(active_tab) = workspace.active_tabs.get(path) {
                let target = project
                    .tabs
                    .iter()
                    .find(|tab| {
                        matches!(&tab.kind, TabKind::File(file) if &file.doc.path == active_tab)
                    })
                    .map(|tab| (tab.id, tab.worktree.clone()));
                if let Some((tab_id, worktree)) = target {
                    if let Some(worktree_index) = project
                        .worktrees
                        .iter()
                        .position(|candidate| candidate.path == worktree)
                    {
                        project.focus_worktree(worktree_index);
                        project.activate_tab(tab_id);
                    }
                }
            }
            if let Some(draft) = workspace.commit_drafts.get(path) {
                project.commit_message = draft.clone();
            }
            let saved_views: Vec<(PathBuf, WorktreeViewState)> = workspace
                .worktree_views
                .get(path)
                .map(|views| {
                    project
                        .worktrees
                        .iter()
                        .filter_map(|worktree| {
                            views
                                .get(&worktree.name)
                                .map(|view| (worktree.path.clone(), *view))
                        })
                        .collect()
                })
                .unwrap_or_default();
            for (worktree_path, view) in saved_views {
                self.ui.worktree_views.insert(worktree_path, view);
            }
        }
        let active_before_single = self.active_project;
        if !workspace.single_files.is_empty() {
            let index = self.ensure_single_files();
            for path in &workspace.single_files {
                let _ = self.projects[index].open_file(path.clone(), &self.settings);
            }
            if self.projects[index].tabs.is_empty() {
                // Every saved ad-hoc file disappeared; do not leave an empty
                // "Single files" workspace behind.
                self.projects.remove(index);
                self.active_project = active_before_single;
            } else {
                if let Some(active) = &workspace.active_single_file {
                    let active_id = self.projects[index]
                        .tabs
                        .iter()
                        .find(|tab| match &tab.kind {
                            TabKind::File(file) => &file.doc.path == active,
                            TabKind::Image(img) => &img.path == active,
                            _ => false,
                        })
                        .map(|tab| tab.id);
                    if let Some(active_id) = active_id {
                        let project = &mut self.projects[index];
                        project.active_tab = Some(active_id);
                        project.active_worktree_mut().active_editor_tab = Some(active_id);
                    }
                }
                if workspace.single_files_active {
                    self.active_project = Some(index);
                } else {
                    self.active_project = active_before_single.or(Some(index));
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
        if path.is_file() {
            return self.open_files(vec![path]);
        }
        if let Some(index) = self
            .projects
            .iter()
            .position(|project| project.root == path && !project.is_single_file)
        {
            self.active_project = Some(index);
            return Ok(index);
        }
        let project = ProjectState::open(path, &self.settings, self.notifier.clone())?;
        Ok(self.add_loaded_project(project))
    }

    pub fn single_files_index(&self) -> Option<usize> {
        self.projects
            .iter()
            .position(|project| project.is_single_file)
    }

    /// Ensure the one ad-hoc files workspace exists and make it active.
    pub fn ensure_single_files(&mut self) -> usize {
        if let Some(index) = self.single_files_index() {
            self.active_project = Some(index);
            return index;
        }
        let mut project = ProjectState::single_files();
        let preferred = PROJECT_TONES[self.projects.len() % PROJECT_TONES.len()];
        project.tone = unique_project_tone(
            self.projects.iter().map(|existing| existing.tone),
            preferred,
        );
        let index = self.projects.len();
        self.projects.push(project);
        self.active_project = Some(index);
        index
    }

    /// Live agent panes across every project and worktree. Shell consoles do
    /// not count; this is the memory admission control for OMP-like agents.
    pub fn live_agent_console_count(&self) -> usize {
        self.projects
            .iter()
            .flat_map(|project| project.worktrees.iter())
            .flat_map(|worktree| worktree.terminals.iter())
            .flat_map(|tab| tab.panes.iter())
            .filter(|pane| pane.kind == SessionKind::Agent)
            .count()
    }

    /// Spawn an agent console with the global admission limit applied. This is
    /// the single entry point used by the agent menu and command palette.
    pub fn open_agent_console(
        &mut self,
        title: String,
        command: String,
        settings: &Settings,
        notifier: Option<Arc<dyn Notifier>>,
    ) -> Result<u64, String> {
        let max = settings.terminal.max_agent_consoles.max(1);
        if self.live_agent_console_count() >= max {
            return Err(if settings.language.is_zh() {
                format!("最多同时打开 {max} 个智能体控制台")
            } else {
                format!("At most {max} agent consoles can be open at once")
            });
        }
        self.active_project_mut()
            .ok_or_else(|| "no active project".to_string())?
            .open_terminal(
                Some(command),
                Some(title),
                SessionKind::Agent,
                settings,
                notifier,
            )
    }

    /// Open files in the "Single files" workspace; directories always go
    /// through `open_project` instead.
    pub fn open_files(&mut self, paths: Vec<PathBuf>) -> Result<usize, String> {
        if paths.is_empty() {
            return Err("no files selected".to_string());
        }
        let index = self.ensure_single_files();
        let mut opened = 0usize;
        let mut first_error: Option<String> = None;
        for path in paths {
            let path = canonical(&path);
            if !path.is_file() {
                first_error.get_or_insert_with(|| format!("{} is not a file", path.display()));
                continue;
            }
            match self.projects[index].open_file(path.clone(), &self.settings) {
                Ok(_) => {
                    opened += 1;
                    self.note_recent_file(path);
                }
                Err(err) => {
                    first_error.get_or_insert(err);
                }
            }
        }
        if opened == 0 {
            if self.projects[index].tabs.is_empty() {
                self.projects.remove(index);
                self.active_project = self
                    .active_project
                    .filter(|active| *active < self.projects.len())
                    .or_else(|| (!self.projects.is_empty()).then_some(0));
            }
            return Err(first_error.unwrap_or_else(|| "could not open files".to_string()));
        }
        self.active_project = Some(index);
        if let Some(project) = self.projects.get_mut(index) {
            project.root_expanded = false;
        }
        let message = if self.settings.language.is_zh() {
            format!("已在单文件页面打开 {opened} 个文件")
        } else {
            format!("Opened {opened} file(s) in Single files")
        };
        self.toast(message, ToastKind::Success);
        Ok(index)
    }

    pub fn note_recent_file(&mut self, path: PathBuf) {
        let path = canonical(&path);
        self.recent_files.retain(|recent| recent != &path);
        self.recent_files.insert(0, path);
        self.recent_files.truncate(12);
    }

    /// Keep every path-keyed piece of UI state in sync after a rename. This is
    /// deliberately broader than tabs: the tree selection, reveal request and
    /// recent-files list must not keep pointing at the old name.
    pub fn retarget_path(&mut self, old: &Path, new: &Path) {
        let old = canonical_after_rename(old);
        let new = new.canonicalize().unwrap_or_else(|_| new.to_path_buf());
        if old == new {
            return;
        }
        let settings = &self.settings;
        for project in &mut self.projects {
            project.retarget_path(&old, &new);
        }
        if let Some(index) = self.active_project {
            if let Some(project) = self.projects.get_mut(index) {
                project.reload_tree(settings);
            }
        }
        if let Some(path) = self.ui.tree_selected.take() {
            self.ui.tree_selected = Some(retarget_absolute(&path, &old, &new).unwrap_or(path));
        }
        let mut selection = std::collections::HashSet::with_capacity(self.ui.tree_selection.len());
        for path in self.ui.tree_selection.drain() {
            selection.insert(retarget_absolute(&path, &old, &new).unwrap_or(path));
        }
        self.ui.tree_selection = selection;
        if let Some((path, center)) = self.ui.tree_reveal.take() {
            self.ui.tree_reveal =
                Some((retarget_absolute(&path, &old, &new).unwrap_or(path), center));
        }
        if let Some((path, at)) = self.ui.locate_flash.take() {
            self.ui.locate_flash = Some((retarget_absolute(&path, &old, &new).unwrap_or(path), at));
        }
        let mut recent = Vec::with_capacity(self.recent_files.len());
        for path in self.recent_files.drain(..) {
            recent.push(retarget_absolute(&path, &old, &new).unwrap_or(path));
        }
        self.recent_files = recent;
        self.save_workspace();
    }

    pub fn forget_recent_file(&mut self, path: &Path) {
        self.recent_files.retain(|recent| recent != path);
        self.save_workspace();
    }

    /// Open a file inside an existing project (tree, search, palette) and
    /// keep the global recent-files list fresh.
    pub fn open_file_in(&mut self, index: usize, path: PathBuf) -> Result<u64, String> {
        let result = self
            .projects
            .get_mut(index)
            .ok_or_else(|| "project not found".to_string())?
            .open_file(path.clone(), &self.settings);
        if result.is_ok() {
            self.note_recent_file(path);
        }
        result
    }

    /// Insert a project that was loaded on a worker thread.
    pub fn add_loaded_project(&mut self, mut project: ProjectState) -> usize {
        if project.is_single_file {
            let index = self.ensure_single_files();
            let active = project.active_tab;
            let target_root = self.projects[index].active_root().to_path_buf();
            let mut tabs = std::mem::take(&mut project.tabs);
            // A file opened via ProjectState::open(file) starts with the file's
            // parent as a temporary worktree. The singleton ad-hoc workspace
            // owns all such tabs, so re-home them before inserting.
            for tab in &mut tabs {
                tab.worktree = target_root.clone();
            }
            for tab in &mut self.projects[index].tabs {
                tab.worktree = target_root.clone();
            }
            self.projects[index].tabs.extend(tabs);
            if let Some(active) = active {
                self.projects[index].active_tab = Some(active);
                self.projects[index].active_worktree_mut().active_editor_tab = Some(active);
            }
            return index;
        }
        if let Some(index) = self
            .projects
            .iter()
            .position(|existing| !existing.is_single_file && existing.root == project.root)
        {
            self.active_project = Some(index);
            return index;
        }
        let preferred = PROJECT_TONES[self.projects.len() % PROJECT_TONES.len()];
        project.tone = unique_project_tone(
            self.projects.iter().map(|existing| existing.tone),
            preferred,
        );
        let path = project.root.clone();
        let name = project.name.clone();
        let had_real_project = self.projects.iter().any(|project| !project.is_single_file);
        let index = self.projects.len();
        self.projects.push(project);
        if !had_real_project {
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

    /// Project owning a Git operation root: either the registered project
    /// root or any attached linked worktree.
    pub fn index_of_git_root(&self, root: &Path) -> Option<usize> {
        self.projects.iter().position(|project| {
            project.root == root
                || project
                    .worktrees
                    .iter()
                    .any(|worktree| worktree.path == root)
        })
    }

    pub fn project_for_git_root(&self, root: &Path) -> Option<&ProjectState> {
        self.index_of_git_root(root)
            .and_then(|index| self.projects.get(index))
    }

    pub fn project_mut_for_git_root(&mut self, root: &Path) -> Option<&mut ProjectState> {
        let index = self.index_of_git_root(root)?;
        self.projects.get_mut(index)
    }

    /// Close a central tab and settle the Single files workspace when the
    /// last ad-hoc tab disappears.
    pub fn close_tab(&mut self, index: usize, id: u64) {
        let Some(project) = self.projects.get_mut(index) else {
            return;
        };
        project.close_tab(id);
        self.settle_single_files_workspace();
    }

    pub fn close_other_tabs(&mut self, index: usize, id: u64) {
        let Some(project) = self.projects.get_mut(index) else {
            return;
        };
        project.close_other_tabs(id);
        self.settle_single_files_workspace();
    }

    pub fn close_tabs_to_right(&mut self, index: usize, id: u64) {
        let Some(project) = self.projects.get_mut(index) else {
            return;
        };
        project.close_tabs_to_right(id);
        self.settle_single_files_workspace();
    }

    pub fn close_tabs_to_left(&mut self, index: usize, id: u64) {
        let Some(project) = self.projects.get_mut(index) else {
            return;
        };
        project.close_tabs_to_left(id);
        self.settle_single_files_workspace();
    }

    pub fn close_unmodified_tabs(&mut self, index: usize) {
        let Some(project) = self.projects.get_mut(index) else {
            return;
        };
        project.close_unmodified_tabs();
        self.settle_single_files_workspace();
    }

    pub fn close_all_tabs(&mut self, index: usize) {
        let Some(project) = self.projects.get_mut(index) else {
            return;
        };
        project.close_all_tabs();
        self.settle_single_files_workspace();
    }

    /// Leave an emptied "Single files" checkout: fall back to an open project
    /// or the home screen, keeping it only when it owns consoles and is sole.
    pub fn settle_single_files_workspace(&mut self) {
        let Some(index) = self.active_project else {
            return;
        };
        let Some(project) = self.projects.get(index) else {
            return;
        };
        if !project.is_single_file || !project.tabs.is_empty() {
            return;
        }
        let has_terminals = project
            .worktrees
            .iter()
            .any(|worktree| !worktree.terminals.is_empty());
        if self.projects.len() > 1 {
            self.close_project(index);
            let target_has_terminals = self.active_project().is_some_and(|project| {
                project
                    .worktrees
                    .iter()
                    .any(|worktree| !worktree.terminals.is_empty())
            });
            if !target_has_terminals {
                self.ui.terminal_visible = false;
                self.ui.terminal_maximised = false;
            }
        } else if !has_terminals {
            self.close_project(index);
            self.ui.terminal_visible = false;
            self.ui.terminal_maximised = false;
        }
    }

    pub fn close_project(&mut self, index: usize) {
        if index >= self.projects.len() {
            return;
        }
        let closed_root = self.projects[index].root.clone();
        let closed_single_file = self.projects[index].is_single_file;
        self.projects.remove(index);
        // A project disappears from recents while it is open; closing it makes
        // it eligible for the recent list again.
        if !closed_single_file {
            self.recent.retain(|recent| recent != &closed_root);
            self.recent.insert(0, closed_root);
            self.recent.truncate(12);
        }
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

    /// Mirror the active checkout's saved pane layout into [`UiState`].
    ///
    /// Call once at the start of every frame so an activation caused by any
    /// command, palette action or restore path gets the same save/restore
    /// behaviour; the swap is deliberately deferred to the frame boundary so
    /// the layout rectangles are computed from the restored state.
    pub fn sync_worktree_view(&mut self) {
        let target = self
            .active_project()
            .map(|project| project.active_worktree().path.clone());
        if target == self.ui.view_target {
            return;
        }
        let first_checkout = self.ui.view_target.is_none();
        let current = WorktreeViewState::capture(&self.ui);
        if let Some(previous) = self.ui.view_target.take() {
            self.ui.worktree_views.insert(previous, current);
        }
        // Tree rows are relative paths scoped to the visible checkout. A
        // selection carried into another worktree would highlight nothing and,
        // worse, let batch actions (delete/discard) resolve same-named paths
        // against the wrong checkout.
        self.ui.tree_selection.clear();
        self.ui.tree_selected = None;
        self.ui.tree_reveal = None;
        self.ui.locate_flash = None;
        let Some(path) = target else {
            return;
        };
        if let Some(saved) = self.ui.worktree_views.get(&path).copied() {
            saved.apply(&mut self.ui);
        } else if first_checkout {
            // Preserve the layout restored from the workspace file for the
            // first visible checkout instead of replacing it with defaults.
            self.ui.worktree_views.insert(path.clone(), current);
        } else {
            let view = WorktreeViewState::default();
            self.ui.worktree_views.insert(path.clone(), view);
            view.apply(&mut self.ui);
        }
        self.ui.view_target = Some(path);
    }

    pub fn switch_project(&mut self, index: usize) {
        if index < self.projects.len() {
            self.active_project = Some(index);
        }
    }

    /// Remove one entry from the recent-project list and persist the
    /// workspace immediately so the menu's delete action survives a restart.
    pub fn forget_recent(&mut self, path: &Path) {
        self.recent.retain(|recent| recent != path);
        self.save_workspace();
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

    /// Drive the LSP integration once per frame: apply events, start servers,
    /// send debounced changes, close disappeared files.
    pub fn sync_lsp(&mut self) {
        if self.lsp.enabled() != self.settings.lsp_enabled {
            self.lsp.set_enabled(self.settings.lsp_enabled);
        }
        if !self.settings.lsp_enabled {
            return;
        }
        self.lsp.poll();
        // Reconciliation builds per-server keys and can clone a whole buffer
        // for a full-text sync, so it runs only when the open document set or
        // a buffer revision changed. Protocol events above still drain every
        // frame.
        let signature = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            for project in &self.projects {
                project.root.hash(&mut hasher);
                for worktree in &project.worktrees {
                    worktree.path.hash(&mut hasher);
                }
                for tab in &project.tabs {
                    if let TabKind::File(file) = &tab.kind {
                        file.doc.path.hash(&mut hasher);
                        file.doc.revision.hash(&mut hasher);
                    }
                }
            }
            hasher.finish()
        };
        if !self.lsp.note_reconcile_signature(signature) {
            return;
        }
        let mut active = HashSet::new();
        {
            let lsp = &mut self.lsp;
            for project in &self.projects {
                for tab in &project.tabs {
                    let TabKind::File(file) = &tab.kind else {
                        continue;
                    };
                    let Some(language) = lsp::language_id_for_path(&file.doc.path) else {
                        continue;
                    };
                    let root = lsp_root_for(project, &file.doc.path);
                    let key = ServerKey::new(root, language);
                    if !lsp.ensure_server(&key) {
                        continue;
                    }
                    active.insert(file.doc.path.clone());
                    let needs_text =
                        lsp.document_needs_text(&key, &file.doc.path, file.doc.revision);
                    if needs_text {
                        lsp.note_document(
                            key,
                            file.doc.path.clone(),
                            language,
                            file.doc.revision,
                            file.doc.text(),
                        );
                    } else {
                        lsp.touch_document(&key, &file.doc.path);
                    }
                }
            }
        }
        self.lsp.retain_documents(&active);
    }

    /// Stop every language server. Called on quit so children do not outlive
    /// the app (they are not tied to our process group by OS defaults).
    pub fn shutdown_lsp(&mut self) {
        self.lsp.shutdown_all();
    }

    pub fn total_running(&self) -> usize {
        self.projects
            .iter()
            .flat_map(|project| project.worktrees.iter())
            .flat_map(|worktree| worktree.terminals.iter())
            .flat_map(|tab| tab.panes.iter())
            .filter(|pane| pane.session.is_running())
            .count()
    }

    pub fn total_changes(&self) -> usize {
        self.projects
            .iter()
            .map(|project| project.changes().len())
            .sum()
    }
    /// Reveal the active tab in the tree: `quiet` scrolls into view, otherwise
    /// centre it with the highlight pulse.
    pub fn locate_active_file(&mut self, quiet: bool) {
        let Some(active_index) = self.active_project else {
            return;
        };
        let project = &mut self.projects[active_index];
        let Some(active_tab_id) = project.active_tab else {
            return;
        };
        let Some(tab) = project.tab_by_id(active_tab_id) else {
            return;
        };
        let Some(file_path_str) = tab.path() else {
            return;
        };
        let file_path = std::path::PathBuf::from(file_path_str);
        project.root_expanded = true;
        project.expand_parents(&file_path);
        project.reload_tree(&self.settings);

        let rel = file_path
            .strip_prefix(project.active_root())
            .unwrap_or(&file_path)
            .to_path_buf();
        // Locating is navigation, not a selection change: batch actions
        // (delete/discard) depend on the multi-selection, so it must survive a
        // tab click. The tree paints the active file with its own accent bar
        // (`sidebar.rs`), which is what makes the active tab locatable without
        // destroying the user's work set.
        self.ui.tree_selected = Some(rel.clone());
        self.ui.tree_reveal = Some((rel.clone(), quiet));
        if !quiet {
            self.ui.locate_flash = Some((rel, std::time::Instant::now()));
        }
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
        let active_view = self.active_project().map(|project| {
            (
                project.active_worktree().path.clone(),
                WorktreeViewState::capture(&self.ui),
            )
        });
        if let Some((path, view)) = active_view {
            self.ui.worktree_views.insert(path, view);
        }
        let single_index = self.single_files_index();
        let single_files: Vec<PathBuf> = single_index
            .and_then(|index| self.projects.get(index))
            .map(|project| {
                project
                    .tabs
                    .iter()
                    .filter_map(|tab| match &tab.kind {
                        TabKind::File(file) => Some(file.doc.path.clone()),
                        TabKind::Image(img) => Some(img.path.clone()),
                        TabKind::Diff(_) | TabKind::Compare(_) => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let active_single_file = single_index.and_then(|index| {
            let project = self.projects.get(index)?;
            let tab = project.tab_by_id(project.active_tab?)?;
            match &tab.kind {
                TabKind::File(file) => Some(file.doc.path.clone()),
                TabKind::Image(img) => Some(img.path.clone()),
                _ => None,
            }
        });
        let single_files_active = single_index.is_some() && self.active_project == single_index;
        let mut file = WorkspaceFile {
            recent: self.recent.clone(),
            recent_files: self.recent_files.clone(),
            projects: self
                .projects
                .iter()
                .filter(|p| !p.is_single_file)
                .map(|project| project.root.clone())
                .collect(),
            active_project: self
                .active_project()
                .filter(|project| !project.is_single_file)
                .map(|project| project.root.clone()),
            single_files,
            active_single_file,
            single_files_active,
            sidebar_view: Some(
                match self.ui.sidebar_view {
                    SidebarView::Files => "files",
                    SidebarView::Git => "git",
                    SidebarView::Consoles => "consoles",
                    // Compare 会话不跨重启保留：落回 Git 视图。
                    SidebarView::Compare => "git",
                }
                .to_string(),
            ),
            sidebar_visible: Some(self.ui.sidebar_visible),
            sidebar_width: Some(self.ui.sidebar_width),
            commit_drawer_width: Some(self.ui.commit_width),
            panel_visible: Some(self.ui.panel_visible),
            panel_height: Some(self.ui.panel_height),
            terminal_placement: Some(
                match self.ui.terminal_placement {
                    TerminalPlacement::Right => "right",
                    TerminalPlacement::Bottom => "bottom",
                }
                .to_string(),
            ),
            terminal_width: Some(self.ui.terminal_width),
            terminal_height: Some(self.ui.terminal_height),
            open_tabs: BTreeMap::new(),
            active_tabs: BTreeMap::new(),
            active_worktrees: BTreeMap::new(),
            open_worktrees: BTreeMap::new(),
            worktree_views: BTreeMap::new(),
            favorite_branches: self.favorite_branches.iter().cloned().collect(),
            commit_drafts: BTreeMap::new(),
        };
        for project in &self.projects {
            if project.is_single_file {
                continue;
            }
            if !project.commit_message.trim().is_empty() {
                file.commit_drafts
                    .insert(project.root.clone(), project.commit_message.clone());
            }
            let files: Vec<PathBuf> = project
                .tabs
                .iter()
                .filter_map(|tab| match &tab.kind {
                    TabKind::File(file) => Some(file.doc.path.clone()),
                    TabKind::Image(img) => Some(img.path.clone()),
                    TabKind::Diff(_) | TabKind::Compare(_) => None,
                })
                .collect();
            file.open_tabs.insert(project.root.clone(), files);
            if let Some(active) = project.active_tab {
                if let Some(tab) = project.tab_by_id(active) {
                    match &tab.kind {
                        TabKind::File(file_tab) => {
                            file.active_tabs
                                .insert(project.root.clone(), file_tab.doc.path.clone());
                        }
                        TabKind::Image(img) => {
                            file.active_tabs
                                .insert(project.root.clone(), img.path.clone());
                        }
                        _ => {}
                    }
                }
            }
            file.active_worktrees
                .insert(project.root.clone(), project.active_worktree().name.clone());
            file.open_worktrees.insert(
                project.root.clone(),
                project
                    .worktrees
                    .iter()
                    .skip(1)
                    .map(|worktree| worktree.name.clone())
                    .collect(),
            );
            let views = file.worktree_views.entry(project.root.clone()).or_default();
            for worktree in &project.worktrees {
                if let Some(view) = self.ui.worktree_views.get(&worktree.path) {
                    views.insert(worktree.name.clone(), view.persisted());
                }
            }
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

    pub fn log_git_cmd(&mut self, cmd: impl Into<String>) {
        let time_str = unsafe {
            let now = libc::time(std::ptr::null_mut());
            let mut tm: libc::tm = std::mem::zeroed();
            if !libc::localtime_r(&now, &mut tm).is_null() {
                format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
            } else {
                "--:--:--".to_string()
            }
        };
        self.git_cmd_log
            .push(format!("{time_str}: [lazecode] git {}", cmd.into()));
        if self.git_cmd_log.len() > 1000 {
            self.git_cmd_log.drain(0..100);
        }
    }
}
