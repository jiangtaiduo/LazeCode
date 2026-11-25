//! Editor and diff tabs: opening files and diff sources, tab lookup and the
//! close variants, as an inherent `ProjectState` impl.

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use lazecode_git::FileDiff;

use crate::settings::Settings;

use super::*;

impl ProjectState {
    pub fn tab_by_id(&self, id: u64) -> Option<&Tab> {
        self.tabs.iter().find(|tab| tab.id == id)
    }

    pub fn tab_mut_by_id(&mut self, id: u64) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|tab| tab.id == id)
    }

    pub fn open_file(&mut self, path: PathBuf, settings: &Settings) -> Result<u64, String> {
        let path = if path.is_absolute() {
            path
        } else if self.is_single_file {
            dirs::home_dir()
                .unwrap_or_else(|| self.root.clone())
                .join(path)
        } else {
            self.active_root().join(path)
        };
        // macOS temp paths (/var) are symlinked to /private/var; compare the
        // canonical form so the same file never opens twice.
        let path = path.canonicalize().unwrap_or(path);
        // A file belongs to the checkout that contains it; files outside every
        // attached checkout belong to the focused one. This keeps restored
        // linked-worktree files from being filed under main.
        let worktree_root = self
            .worktrees
            .iter()
            .filter(|worktree| path.starts_with(&worktree.path))
            .max_by_key(|worktree| worktree.path.components().count())
            .map(|worktree| worktree.path.clone())
            .unwrap_or_else(|| self.active_root().to_path_buf());
        let visible = worktree_root == self.active_root();
        let existing = self
            .tabs
            .iter()
            .find(|tab| {
                tab.worktree == worktree_root
                    && match &tab.kind {
                        TabKind::File(file) => file.doc.path == path,
                        TabKind::Image(img) => img.path == path,
                        _ => false,
                    }
            })
            .map(|tab| tab.id);
        if let Some(id) = existing {
            if visible {
                self.set_active_tab(id);
            }
            return Ok(id);
        }
        let id = NEXT_TAB.fetch_add(1, Ordering::Relaxed);
        // The Single files checkout is virtual: every ad-hoc file lives
        // outside its metadata root by design. Only real projects use the
        // "diff" badge for files opened from outside the checkout.
        let external = !self.is_single_file && !path.starts_with(&worktree_root);
        if is_image_path(&path) {
            self.tabs.push(Tab {
                id,
                worktree: worktree_root,
                kind: TabKind::Image(ImageTab {
                    path: path.clone(),
                    external,
                }),
            });
            if visible {
                self.set_active_tab(id);
                self.expand_parents(&path, &settings.ignored_patterns);
            }
            return Ok(id);
        }
        let doc = Document::open(&path, settings.editor.tab_width, settings.editor.use_spaces)
            .map_err(|err| err.to_string())?;
        let is_md = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| {
                ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown")
            });
        self.tabs.push(Tab {
            id,
            worktree: worktree_root,
            kind: TabKind::File(FileTab {
                doc,
                external,
                title_override: None,
                md_mode: if is_md {
                    MarkdownViewMode::Preview
                } else {
                    MarkdownViewMode::Code
                },
                annotate: false,
                blame: None,
                disk_stamp: DiskStamp::read(&path),
            }),
        });
        if visible {
            self.set_active_tab(id);
            self.expand_parents(&path, &settings.ignored_patterns);
        }
        Ok(id)
    }

    pub fn open_diff_source(&mut self, source: DiffSource) -> u64 {
        self.open_diff_source_inner(source, false)
    }

    /// 「Show Diff in a New Tab」：跳过单例标签复用，总是新开一个 diff 标签。
    pub fn open_diff_source_new_tab(&mut self, source: DiffSource) -> u64 {
        self.open_diff_source_inner(source, true)
    }

    /// never writes back to the checkout.
    pub fn open_revision_snapshot(
        &mut self,
        source_path: &Path,
        bytes: &[u8],
        label: String,
        settings: &Settings,
    ) -> Result<u64, String> {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        source_path.hash(&mut hasher);
        bytes.hash(&mut hasher);
        let hash = hasher.finish();
        let name = source_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "version".to_string());
        let dir = std::env::temp_dir().join("lazecode-revisions");
        std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
        let file = dir.join(format!("{hash:016x}-{name}"));
        if !file.is_file() {
            std::fs::write(&file, bytes).map_err(|err| err.to_string())?;
        }
        let worktree_root = self.active_root().to_path_buf();
        if let Some(id) = self
            .tabs
            .iter()
            .find(|tab| {
                tab.worktree == worktree_root
                    && match &tab.kind {
                        TabKind::File(file_tab) => file_tab.doc.path == file,
                        TabKind::Image(image) => image.path == file,
                        _ => false,
                    }
            })
            .map(|tab| tab.id)
        {
            self.set_active_tab(id);
            return Ok(id);
        }
        let id = NEXT_TAB.fetch_add(1, Ordering::Relaxed);
        if is_image_path(source_path) {
            self.tabs.push(Tab {
                id,
                worktree: worktree_root,
                kind: TabKind::Image(ImageTab {
                    path: file,
                    external: false,
                }),
            });
            self.set_active_tab(id);
            return Ok(id);
        }
        let mut doc = Document::open(&file, settings.editor.tab_width, settings.editor.use_spaces)
            .map_err(|err| err.to_string())?;
        doc.read_only = true;
        self.tabs.push(Tab {
            id,
            worktree: worktree_root,
            kind: TabKind::File(FileTab {
                doc,
                external: false,
                title_override: Some(label),
                md_mode: MarkdownViewMode::Code,
                annotate: false,
                blame: None,
                disk_stamp: DiskStamp::read(&file),
            }),
        });
        self.set_active_tab(id);
        Ok(id)
    }

    fn open_diff_source_inner(&mut self, source: DiffSource, force_new: bool) -> u64 {
        let worktree_root = self.active_root().to_path_buf();
        let is_img = match &source {
            DiffSource::Worktree { path }
            | DiffSource::Index { path }
            | DiffSource::Commit { path, .. }
            | DiffSource::CompareWithLocal { path, .. }
            | DiffSource::CompareCommits { path, .. }
            | DiffSource::Stash { path, .. }
            | DiffSource::StashBeforeLocal { path, .. } => {
                is_image_path(std::path::Path::new(path))
            }
            DiffSource::Files { new_path, .. } => is_image_path(new_path),
        };
        if is_img {
            let path_buf = match &source {
                DiffSource::Worktree { path }
                | DiffSource::Index { path }
                | DiffSource::Commit { path, .. }
                | DiffSource::CompareWithLocal { path, .. }
                | DiffSource::CompareCommits { path, .. }
                | DiffSource::Stash { path, .. }
                | DiffSource::StashBeforeLocal { path, .. } => worktree_root.join(path),
                DiffSource::Files { new_path, .. } => new_path.clone(),
            };
            if let Ok(id) = self.open_file(path_buf, &Settings::default()) {
                return id;
            }
        }
        // 完全同源（同提交同文件）：只激活不重置，避免重复加载与后台任务
        // 去重（同 key 会被丢弃）竞争导致永远转圈。DiffSource paths are
        // repo-relative, so the owning checkout is part of the identity.
        let existing = self
            .tabs
            .iter()
            .find(|tab| {
                tab.worktree == worktree_root
                    && matches!(&tab.kind, TabKind::Diff(diff) if diff.source == source)
            })
            .map(|tab| tab.id);
        if let Some(id) = existing.filter(|_| !force_new) {
            self.set_active_tab(id);
            return id;
        }
        let path = match &source {
            DiffSource::Worktree { path }
            | DiffSource::Index { path }
            | DiffSource::Commit { path, .. }
            | DiffSource::CompareWithLocal { path, .. }
            | DiffSource::CompareCommits { path, .. }
            | DiffSource::Stash { path, .. }
            | DiffSource::StashBeforeLocal { path, .. } => path.clone(),
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
        if !force_new && !matches!(source, DiffSource::Files { .. }) {
            let reuse = self.tabs.iter().position(|tab| {
                tab.worktree == worktree_root
                    && matches!(
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
                    diff.edge_pending = 0;
                    diff.revealed = false;
                }
                let id = self.tabs[index].id;
                self.set_active_tab(id);
                return id;
            }
        }
        let id = NEXT_TAB.fetch_add(1, Ordering::Relaxed);
        self.tabs.push(Tab {
            id,
            worktree: worktree_root,
            kind: TabKind::Diff(DiffTab {
                source,
                path,
                diff: None,
                loading: true,
                error: None,
                view_mode: DiffViewMode::Unified,
                current_hunk: 0,
                edge_pending: 0,
                revealed: false,
            }),
        });
        self.set_active_tab(id);
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

    /// 激活既有标签页；标题在 UI 层填充为 i18n 文案）。
    pub fn open_compare_branches(&mut self, left: String, right: String, title: String) -> u64 {
        let worktree_root = self.active_root().to_path_buf();
        if let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|tab| tab.worktree == worktree_root && matches!(tab.kind, TabKind::Compare(_)))
        {
            if let TabKind::Compare(compare) = &mut tab.kind {
                if compare.left != left || compare.right != right {
                    compare.left = left;
                    compare.right = right;
                    compare.title = title;
                }
            }
            let id = tab.id;
            self.set_active_tab(id);
            return id;
        }
        let id = NEXT_TAB.fetch_add(1, Ordering::Relaxed);
        self.tabs.push(Tab {
            id,
            worktree: worktree_root,
            kind: TabKind::Compare(CompareTab { left, right, title }),
        });
        self.set_active_tab(id);
        id
    }

    /// user has switched to another checkout in the meantime.
    pub fn set_diff_for_source(
        &mut self,
        root: &Path,
        source: &DiffSource,
        result: Result<FileDiff, String>,
    ) {
        // "Show Diff in a New Tab" can leave two tabs with the same source.
        // Fill every match so the newly created tab cannot stay on its
        // loading spinner while the older sibling receives the result.
        for tab in self.tabs.iter_mut().filter(|tab| {
            tab.worktree == root
                && matches!(&tab.kind, TabKind::Diff(diff) if &diff.source == source)
        }) {
            if let TabKind::Diff(diff) = &mut tab.kind {
                diff.loading = false;
                match &result {
                    Ok(file_diff) => {
                        if diff.path.is_empty() {
                            diff.path = file_diff.path.clone();
                        }
                        diff.diff = Some(file_diff.clone());
                        diff.error = None;
                    }
                    Err(err) => {
                        diff.diff = None;
                        diff.error = Some(err.clone());
                    }
                }
            }
        }
    }

    pub fn set_diff(&mut self, path: &str, result: Result<FileDiff, String>) {
        let root = self.active_root().to_path_buf();
        if let Some(tab) = self.tabs.iter_mut().find(|tab| {
            tab.worktree == root && matches!(&tab.kind, TabKind::Diff(diff) if diff.path == path)
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

    pub fn close_tab(&mut self, id: u64) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        let worktree = self.tabs[index].worktree.clone();
        self.tabs.remove(index);
        if self.active_tab == Some(id) {
            let next = self
                .tabs
                .iter()
                .take(index)
                .rev()
                .find(|tab| tab.worktree == worktree)
                .or_else(|| {
                    self.tabs
                        .iter()
                        .skip(index)
                        .find(|tab| tab.worktree == worktree)
                })
                .map(|tab| tab.id);
            self.active_tab = next;
            if worktree == self.active_root() {
                let wt_index = self
                    .active_worktree
                    .min(self.worktrees.len().saturating_sub(1));
                if let Some(worktree) = self.worktrees.get_mut(wt_index) {
                    worktree.active_editor_tab = next;
                }
            }
        }
    }

    pub fn close_other_tabs(&mut self, id: u64) {
        let Some(tab) = self.tab_by_id(id) else {
            return;
        };
        let worktree = tab.worktree.clone();
        self.tabs
            .retain(|tab| tab.worktree != worktree || tab.id == id);
        if worktree == self.active_root() {
            self.set_active_tab(id);
        }
    }

    pub fn close_all_tabs(&mut self) {
        let worktree = self.active_root().to_path_buf();
        self.tabs.retain(|tab| tab.worktree != worktree);
        self.active_tab = None;
        let wt_index = self
            .active_worktree
            .min(self.worktrees.len().saturating_sub(1));
        if let Some(active) = self.worktrees.get_mut(wt_index) {
            active.active_editor_tab = None;
        }
    }

    pub fn close_tabs_to_right(&mut self, id: u64) {
        if let Some(pos) = self.tabs.iter().position(|tab| tab.id == id) {
            let worktree = self.tabs[pos].worktree.clone();
            let mut index = 0usize;
            self.tabs.retain(|tab| {
                let keep = tab.worktree != worktree || index <= pos;
                index += 1;
                keep
            });
            if !self
                .active_worktree_tabs()
                .any(|tab| Some(tab.id) == self.active_tab)
            {
                self.set_active_tab(id);
            }
        }
    }

    /// 关闭目标标签左侧的所有标签（稳定版「关闭左侧标签」，首标签禁用）。
    pub fn close_tabs_to_left(&mut self, id: u64) {
        if let Some(pos) = self.tabs.iter().position(|tab| tab.id == id) {
            let worktree = self.tabs[pos].worktree.clone();
            let mut index = 0usize;
            self.tabs.retain(|tab| {
                let keep = tab.worktree != worktree || index >= pos;
                index += 1;
                keep
            });
            if !self
                .active_worktree_tabs()
                .any(|tab| Some(tab.id) == self.active_tab)
            {
                self.set_active_tab(id);
            }
        }
    }

    pub fn close_unmodified_tabs(&mut self) {
        let worktree = self.active_root().to_path_buf();
        self.tabs
            .retain(|tab| tab.worktree != worktree || tab.is_dirty());
        if !self
            .active_worktree_tabs()
            .any(|tab| Some(tab.id) == self.active_tab)
        {
            let next = self.active_worktree_tabs().last().map(|tab| tab.id);
            self.active_tab = next;
            let wt_index = self
                .active_worktree
                .min(self.worktrees.len().saturating_sub(1));
            if let Some(worktree) = self.worktrees.get_mut(wt_index) {
                worktree.active_editor_tab = next;
            }
        }
    }
}
