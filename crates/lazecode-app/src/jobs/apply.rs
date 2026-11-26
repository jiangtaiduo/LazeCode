//! Job completion handling: apply each `JobResult` to the model/runtime.

use super::*;

impl JobQueue {
    pub fn apply(
        &mut self,
        model: &mut AppModel,
        runtime: &mut UiRuntime,
        completion: JobCompletion,
    ) {
        self.in_flight.remove(&completion.key);
        let result = completion.result;
        match result {
            JobResult::ProjectLoaded { path, result } => match result {
                Ok(project) => {
                    model.add_loaded_project(*project);
                    runtime.clear_terminal_cache();
                }
                Err(err) => model.toast(
                    if model.settings.language.is_zh() {
                        format!("无法打开 {}：{err}", path.display())
                    } else {
                        format!("Cannot open {}: {err}", path.display())
                    },
                    ToastKind::Error,
                ),
            },
            JobResult::CmpSessionLoaded {
                root,
                from,
                to,
                local,
                result,
            } => {
                let pending_key = local.then(|| format!("cmplocal:{root:?}:{from}"));
                let rerun = pending_key
                    .as_ref()
                    .is_some_and(|key| self.pending_compare_local.remove(key));
                let reload_commit = from.clone();
                let mut reload = false;
                if let Some(project) = model.project_mut_for_git_root(&root) {
                    if let Some(session) = project.compare_session.as_mut() {
                        // A newer compare request may have replaced the
                        // session while this job was running; drop the stale
                        // file list instead of filling the wrong drawer.
                        if session.from == from && session.to == to && session.local == local {
                            if rerun {
                                // A newer refresh was requested while this
                                // scan ran; its result is the one to show.
                                session.loading = true;
                                reload = true;
                            } else {
                                match result {
                                    Ok(files) => {
                                        session.nav_order =
                                            lazecode_core::model::compare_nav_order(&files);
                                        session.files = files;
                                        session.loading = false;
                                    }
                                    Err(_err) => {
                                        session.files.clear();
                                        session.nav_order.clear();
                                        session.loading = false;
                                    }
                                }
                            }
                        }
                    }
                }
                if reload {
                    self.load_compare_local(root, reload_commit);
                }
            }
            JobResult::TreeLoaded { root, tree } => {
                // The main worktree also feeds the project-level tree; re-queue
                // once with fresher disk state if events arrived mid-scan.
                let position =
                    model
                        .projects
                        .iter()
                        .enumerate()
                        .find_map(|(project_index, project)| {
                            if project.root == root {
                                return Some((project_index, 0usize));
                            }
                            project
                                .worktrees
                                .iter()
                                .position(|worktree| worktree.path == root)
                                .map(|wt| (project_index, wt))
                        });
                let expanded: Vec<PathBuf> = position
                    .as_ref()
                    .map(|(index, _)| model.projects[*index].expanded.iter().cloned().collect())
                    .unwrap_or_default();
                if let Some((project_index, wt_index)) = position {
                    let project = &mut model.projects[project_index];
                    project.worktrees[wt_index].tree = tree;
                    // A fresh bounded scan can skip folders that are already
                    // open; keep their children loaded after a watcher refresh.
                    for rel in &expanded {
                        project.ensure_tree_children(rel, &model.settings.ignored_patterns);
                    }
                }
                if self.pending_trees.remove(&root) {
                    self.refresh_tree(root, Arc::new(model.settings.ignored_patterns.clone()));
                }
            }
            JobResult::GitRefreshed { root, result } => {
                let mut local_compare = None;
                if let Some(project) = model.project_mut_for_git_root(&root) {
                    let worktree_index = project
                        .worktrees
                        .iter()
                        .position(|worktree| worktree.path == root);
                    let old_head = worktree_index.and_then(|index| {
                        project.worktrees[index].git.as_ref().map(|git| {
                            (
                                git.head.as_ref().map(|head| head.id.clone()),
                                git.branch.clone(),
                            )
                        })
                    });
                    let query = project.log_query.clone();
                    project.apply_git_snapshot(&root, result);
                    let new_head = worktree_index.and_then(|index| {
                        project.worktrees[index].git.as_ref().map(|git| {
                            (
                                git.head.as_ref().map(|head| head.id.clone()),
                                git.branch.clone(),
                            )
                        })
                    });
                    if old_head != new_head {
                        self.refresh_log(root.clone(), query, 0);
                        self.refresh_branches(root.clone());
                    }
                    if project.active_root() == root {
                        local_compare = project
                            .compare_session
                            .as_ref()
                            .filter(|session| session.local)
                            .map(|session| session.from.clone());
                    }
                }
                if let Some(commit) = local_compare {
                    if let Some(session) = model
                        .project_mut_for_git_root(&root)
                        .and_then(|project| project.compare_session.as_mut())
                    {
                        session.loading = true;
                    }
                    self.load_compare_local(root.clone(), commit);
                }
                if self.pending_git.remove(&root) {
                    self.refresh_git(root, None);
                }
            }
            JobResult::LogLoaded {
                root,
                branch,
                query,
                offset,
                result,
            } => {
                let rerun = self.pending_log.remove(&(root.clone(), offset));
                let Some(project) = model.project_mut_for_git_root(&root) else {
                    return;
                };
                // Stale completion (filters changed while the page was in
                // flight) is dropped; the fresh query already re-spawned.
                let stale = log_query_fingerprint(&project.log_query.clone())
                    != log_query_fingerprint(&query);
                if stale {
                    let latest = project.log_query.clone();
                    if rerun {
                        self.refresh_log(root, latest, offset);
                    }
                    return;
                }
                match result {
                    Ok((mut commits, lanes)) => {
                        project.log_error = None;
                        if offset == 0 {
                            project.log_reached_end = commits.len() < LOG_PAGE_SIZE;
                            project.log = commits;
                        } else {
                            // Pages re-walk from the tips, so drop commits
                            // already shown.
                            let seen: std::collections::HashSet<&str> = project
                                .log
                                .iter()
                                .map(|commit| commit.id.as_str())
                                .collect();
                            commits.retain(|commit| !seen.contains(commit.id.as_str()));
                            if commits.is_empty() {
                                project.log_reached_end = true;
                            }
                            project.log.extend(commits);
                        }
                        project.log_seed = lanes;
                        project.log_generation = project.log_generation.wrapping_add(1);
                        project.log_loaded = project.log.len();
                        project.log_paging = false;
                        if !project
                            .log
                            .iter()
                            .any(|commit| Some(&commit.id) == project.selected_commit.as_ref())
                        {
                            project.selected_commit =
                                project.log.first().map(|commit| commit.id.clone());
                            project.selected_commits = project
                                .log
                                .first()
                                .map(|commit| vec![commit.id.clone()])
                                .unwrap_or_default();
                            project.commit_details = None;
                            project.commit_details_attempted = None;
                            project.commit_range = None;
                            project.commit_range_attempted = None;
                        }
                    }
                    Err(err) => {
                        // Diffs for detached pages report errors noisily — the
                        // view shows the first error and stops paging.
                        if offset == 0 {
                            project.log_error = Some(err);
                            project.log_loading = false;
                        }
                        project.log_paging = false;
                    }
                }
                project.log_branch = branch;
                project.log_loading = false;
                if rerun {
                    let latest = project.log_query.clone();
                    self.refresh_log(root, latest, offset);
                }
            }
            JobResult::BranchesLoaded { root, result, tags } => {
                if let Some(project) = model.project_mut_for_git_root(&root) {
                    project.branches = result;
                    project.tags = tags;
                }
                if self.pending_branches.remove(&root) {
                    self.refresh_branches(root);
                }
            }
            JobResult::CommitDetails { root, id, result } => {
                if let Some(project) = model.project_mut_for_git_root(&root) {
                    let selected = project.selected_commit.as_deref() == Some(id.as_str());
                    let mut error = None;
                    match result {
                        Ok(details) => {
                            if selected {
                                project.commit_details = Some(*details);
                            }
                        }
                        Err(err) => {
                            if selected {
                                project.commit_details = None;
                                // Keep `commit_details_attempted` set: retrying
                                // every poll would spin forever on a missing or
                                // corrupt commit. Re-selecting the row clears it.
                                error = Some(err);
                            }
                        }
                    }
                    if let Some(err) = error {
                        model.toast(err, ToastKind::Error);
                    }
                }
            }

            JobResult::CommitRangeDetails { root, key, result } => {
                if let Some(project) = model.project_mut_for_git_root(&root) {
                    let Some(current) = lazecode_core::selected_commit_range(
                        &project.log,
                        &project.selected_commits,
                    ) else {
                        return;
                    };
                    // `project_mut_for_git_root` matches the checkout; the key
                    // also matches the exact selection, so a range completed
                    // after the user changed selection is discarded.
                    if current.key != key {
                        return;
                    }
                    let mut error = None;
                    match result {
                        Ok(files) => {
                            project.commit_range = Some(lazecode_core::CommitRangeDetails {
                                key: current.key,
                                from: current.from,
                                to: current.to,
                                count: current.count,
                                files: lazecode_core::aggregate_commit_files(&files),
                                error: None,
                                loading: false,
                            });
                        }
                        Err(err) => {
                            project.commit_range = Some(lazecode_core::CommitRangeDetails {
                                key: current.key,
                                from: current.from,
                                to: current.to,
                                count: current.count,
                                files: Vec::new(),
                                error: Some(err.clone()),
                                loading: false,
                            });
                            error = Some(err);
                        }
                    }
                    if let Some(err) = error {
                        model.toast(err, ToastKind::Error);
                    }
                }
            }

            JobResult::DiffLoaded {
                root,
                source,
                result,
            } => {
                // A refresh requested while this load was in flight is more
                // recent than this result (stage/rollback/commit landed in
                // between). Drop the stale payload and reload once.
                let key = format!("diff:{}:{source:?}", root.display());
                if self.pending_diffs.remove(&key) {
                    let still_open = model
                        .project_mut_for_git_root(&root)
                        .is_some_and(|project| {
                            project.tabs.iter().any(|tab| {
                                tab.worktree == root
                                    && matches!(
                                        &tab.kind,
                                        TabKind::Diff(diff) if diff.source == source
                                    )
                            })
                        });
                    if still_open {
                        self.diff_source(root, source);
                    }
                    return;
                }
                if let Some(project) = model.project_mut_for_git_root(&root) {
                    project.set_diff_for_source(&root, &source, result);
                }
            }
            JobResult::SearchDone {
                root,
                request_id,
                query,
                result,
            } => {
                // Drop the registry entry only if this is still the newest search
                // for the worktree.
                if self
                    .search_cancels
                    .get(&root)
                    .is_some_and(|(id, _)| *id == request_id)
                {
                    self.search_cancels.remove(&root);
                }
                if runtime.search.request_id == request_id
                    && runtime.search.query == query
                    && model
                        .active_project()
                        .is_some_and(|project| project.active_root() == root)
                {
                    match result {
                        Ok(hits) => {
                            runtime.search.results = hits;
                            runtime.search.searched = true;
                            runtime.search.searching = false;
                            runtime.search.selected = 0;
                        }
                        Err(err) => {
                            runtime.search.results.clear();
                            runtime.search.searched = true;
                            runtime.search.searching = false;
                            model.toast(err, ToastKind::Error);
                        }
                    }
                }
            }
            JobResult::CompareBranchesLoaded {
                root,
                left,
                right,
                left_path,
                right_path,
                result,
            } => {
                let state = &mut model.ui.compare_branches;
                // 回填前先核对会话身份（项目 + 方向 + 路径）：任何一项
                // 变化都意味着结果已过期，丢弃。
                if state.root == root
                    && state.left_ref == left
                    && state.right_ref == right
                    && state.left_path == left_path
                    && state.right_path == right_path
                {
                    state.loading = false;
                    match result {
                        Ok(data) => {
                            state.sides = Some(data);
                            state.error = None;
                        }
                        Err(err) => state.error = Some(err),
                    }
                    // 换数据后详情缓存作废。
                    for view in [&mut state.left_view, &mut state.right_view] {
                        view.selected.clear();
                        view.anchor = None;
                        view.select_all = false;
                        view.selected_file = None;
                        view.detail = None;
                    }
                }
            }
            JobResult::CompareBranchesCommits {
                root,
                side,
                exclude,
                include,
                result,
            } => {
                let state = &mut model.ui.compare_branches;
                let (this, other) = if side == 0 {
                    (&state.left_ref, &state.right_ref)
                } else {
                    (&state.right_ref, &state.left_ref)
                };
                // root + range 语义必须与当前会话一致，否则丢弃。
                if state.root == root && *this == include && *other == exclude {
                    if let Ok((commits, truncated)) = result {
                        if let Some(sides) = &mut state.sides {
                            let target = if side == 0 {
                                &mut sides.left
                            } else {
                                &mut sides.right
                            };
                            target.commits = commits;
                            target.truncated = truncated;
                        }
                    }
                }
                state.loading_more[side as usize] = false;
            }
            JobResult::CompareFilesLoaded { root, key, result } => {
                // 详情缓存是每侧独立的：root+key 都匹配才回填。
                let state = &mut model.ui.compare_branches;
                if state.root != root {
                    return;
                }
                for view in [&mut state.left_view, &mut state.right_view] {
                    if let Some(detail) = &mut view.detail {
                        if detail.key == key {
                            detail.loading = false;
                            match &result {
                                Ok(files) => {
                                    detail.files = files.clone();
                                    detail.error = None;
                                }
                                Err(err) => detail.error = Some(err.clone()),
                            }
                        }
                    }
                }
            }
            JobResult::Discarded {
                root,
                paths,
                message,
            } => {
                model.toast(message, ToastKind::Success);
                if !root.as_os_str().is_empty() {
                    if let Some(project) = model.project_mut_for_git_root(&root) {
                        // Close tabs of rolled-back files (even dirty): the
                        // file is gone, a tab would be a ghost buffer.
                        let mut closed: Vec<u64> = Vec::new();
                        for path in &paths {
                            let absolute = root.join(path);
                            for tab in &project.tabs {
                                if let TabKind::File(file) = &tab.kind {
                                    if file.doc.path == absolute {
                                        closed.push(tab.id);
                                    }
                                }
                            }
                        }
                        for tab_id in closed {
                            project.close_tab(tab_id);
                        }
                    }
                    model.settle_single_files_workspace();
                    let log_query = model
                        .project_for_git_root(&root)
                        .map(|project| project.log_query.clone())
                        .unwrap_or_default();
                    self.refresh_git(root.clone(), None);
                    self.refresh_log(root.clone(), log_query, 0);
                    self.refresh_branches(root.clone());
                }
            }
            JobResult::BranchDeleteNeedsForce { root, name } => {
                if model.index_of_git_root(&root).is_some() {
                    runtime.dialog = lazecode_ui::runtime::DialogState::DeleteBranch {
                        name,
                        force: true,
                        remote: false,
                        error: Some(
                            if model.settings.language.is_zh() {
                                "该分支尚未合并，确认后会强制删除。"
                            } else {
                                "This branch is not fully merged; confirm to force-delete it."
                            }
                            .to_string(),
                        ),
                    };
                }
            }
            JobResult::OpDone {
                root,
                message,
                ok,
                refresh,
                reload_files,
                clear_commit_message,
            } => {
                // Stash jobs carry a `stash…` key: their result changes the
                // stash list, so the drawer tab must reload it.
                if completion.key.starts_with("stash") {
                    runtime.stash_loaded = None;
                    runtime.stash_entries.clear();
                    runtime.stash_selected = None;
                    runtime.stash_expanded.clear();
                    runtime.stash_collapsed.clear();
                    runtime.stash_file_sel = None;
                    if let Some(project) = model.active_project_mut() {
                        project.invalidate_stash_details();
                    }
                }
                if completion.key.starts_with("shelf")
                    || completion.key.starts_with("cherry-changes")
                {
                    // Create/restore-delete/rename all change the shelf list;
                    // it is otherwise loaded only once per checkout.
                    runtime.shelf_loaded = None;
                    runtime.shelf_entries.clear();
                    runtime.shelf_deleted.clear();
                    runtime.shelf_files_map.clear();
                    runtime.shelf_selected = None;
                    runtime.shelf_file_sel.clear();
                }
                model.toast(
                    message,
                    if ok {
                        ToastKind::Success
                    } else {
                        ToastKind::Error
                    },
                );
                if refresh && !root.as_os_str().is_empty() {
                    let Some(index) = model.index_of_git_root(&root) else {
                        return;
                    };
                    let log_query = model.projects[index].log_query.clone();
                    self.refresh_git(root.clone(), None);
                    self.refresh_log(root.clone(), log_query, 0);
                    self.refresh_branches(root.clone());
                    let project = &mut model.projects[index];
                    // Hunk stage/rollback, staging a file or a commit all
                    // change what a worktree diff should show. Re-request
                    // those tabs instead of leaving the pre-op diff painted.
                    let mut stale_diffs = Vec::new();
                    // Sequencer/conflict state follows every git op, but only
                    // the checkout that actually ran the operation owns it.
                    let active_checkout = project.active_root() == root;
                    if active_checkout {
                        project.refresh_conflicts();
                    }
                    let local_compare = if active_checkout {
                        project
                            .compare_session
                            .as_ref()
                            .filter(|session| session.local)
                            .map(|session| session.from.clone())
                    } else {
                        None
                    };
                    project.commit_details = None;
                    // OpDone cleared the cached details; allow the next
                    // ensure_selected_details poll to refetch them. Without
                    // this the details column stays blank after stage/fetch/
                    // commit until the user re-clicks the commit.
                    project.commit_details_attempted = None;
                    project.commit_range = None;
                    project.commit_range_attempted = None;
                    if clear_commit_message {
                        if ok {
                            // 提交成功：入历史（去重、上限 10 条，对齐
                            // 稳定版 useCommitMessageHistory）。
                            let message = project.commit_message.trim().to_string();
                            model.settings.remember_commit_message(&message);
                            if project.open_push_after_commit {
                                project.open_push_after_commit = false;
                                model.ui.open_push_dialog = true;
                            }
                        } else {
                            project.open_push_after_commit = false;
                        }
                        project.commit_message.clear();
                    }
                    if reload_files {
                        for tab in &mut project.tabs {
                            if let TabKind::File(file) = &mut tab.kind {
                                if !file.doc.dirty {
                                    if let Err(err) = file.doc.reload() {
                                        log::warn!(
                                            "cannot reload {} after checkout: {err}",
                                            file.doc.path.display()
                                        );
                                    }
                                }
                            }
                        }
                    }
                    for tab in &project.tabs {
                        if tab.worktree != root {
                            continue;
                        }
                        if let TabKind::Diff(diff) = &tab.kind {
                            if matches!(
                                diff.source,
                                DiffSource::Worktree { .. }
                                    | DiffSource::Index { .. }
                                    | DiffSource::CompareWithLocal { .. }
                            ) {
                                stale_diffs.push(diff.source.clone());
                            }
                        }
                    }
                    for source in stale_diffs {
                        if let Some(project) = model.project_mut_for_git_root(&root) {
                            if let Some(tab) = project.tabs.iter_mut().find(|tab| {
                                tab.worktree == root
                                    && matches!(&tab.kind, TabKind::Diff(diff) if diff.source == source)
                            }) {
                                if let TabKind::Diff(diff) = &mut tab.kind {
                                    diff.loading = true;
                                    diff.error = None;
                                }
                            }
                        }
                        self.diff_source(root.clone(), source);
                    }
                    if let Some(commit) = local_compare {
                        if let Some(project) = model.project_mut_for_git_root(&root) {
                            if let Some(session) = project.compare_session.as_mut() {
                                if session.local {
                                    session.loading = true;
                                }
                            }
                        }
                        self.load_compare_local(root.clone(), commit);
                    }
                }
            }
            JobResult::Saved {
                root,
                path,
                revision,
                result,
            } => {
                let mut save_error = None;
                let mut saved = false;
                if let Some(project) = model.project_mut_for_git_root(&root) {
                    if let Some(tab) = project.tabs.iter_mut().find(
                        |tab| matches!(&tab.kind, TabKind::File(file) if file.doc.path == path),
                    ) {
                        if let TabKind::File(file) = &mut tab.kind {
                            match result {
                                Err(err) => save_error = Some(err),
                                Ok(()) => {
                                    if file.doc.revision == revision {
                                        file.doc.dirty = false;
                                    }
                                    file.stamp_disk();
                                    saved = true;
                                }
                            }
                        }
                    }
                }
                if let Some(err) = save_error {
                    model.toast(
                        if model.settings.language.is_zh() {
                            format!("保存失败：{err}")
                        } else {
                            format!("Save failed: {err}")
                        },
                        ToastKind::Error,
                    );
                }
                if saved {
                    let commit = model
                        .project_mut_for_git_root(&root)
                        .filter(|project| project.active_root() == root)
                        .and_then(|project| {
                            project
                                .compare_session
                                .as_ref()
                                .filter(|session| session.local)
                                .map(|session| session.from.clone())
                        });
                    if let Some(commit) = commit {
                        if let Some(session) = model
                            .project_mut_for_git_root(&root)
                            .and_then(|project| project.compare_session.as_mut())
                        {
                            session.loading = true;
                        }
                        self.load_compare_local(root, commit);
                    }
                }
            }
            JobResult::WorktreeAdded {
                project_root,
                name,
                result,
            } => match result {
                Ok(path) => {
                    let ignored_patterns = model.settings.ignored_patterns.clone();
                    if let Some(project) = model
                        .projects
                        .iter_mut()
                        .find(|project| project.root == project_root)
                    {
                        project.attach_worktree(WorktreeState::load(
                            name.clone(),
                            path.clone(),
                            false,
                            &ignored_patterns,
                        ));
                        project.log_loading = true;
                    }
                    self.refresh_git(path.clone(), None);
                    let query = model
                        .projects
                        .iter()
                        .find(|project| project.root == project_root)
                        .map(|project| project.log_query.clone())
                        .unwrap_or_default();
                    self.refresh_log(path.clone(), query, 0);
                    self.refresh_branches(project_root.clone());
                    model.toast(
                        if model.settings.language.is_zh() {
                            format!("已创建工作树 {name}")
                        } else {
                            format!("Created worktree {name}")
                        },
                        ToastKind::Success,
                    );
                }
                Err(err) => {
                    model.toast(
                        if model.settings.language.is_zh() {
                            format!("无法创建工作树 {name}：{err}")
                        } else {
                            format!("Cannot create worktree {name}: {err}")
                        },
                        ToastKind::Error,
                    );
                }
            },
            JobResult::WorktreeRemoved {
                project_root,
                name,
                result,
            } => {
                let mut fallback = None;
                if let Some(project) = model
                    .projects
                    .iter_mut()
                    .find(|project| project.root == project_root)
                {
                    match result {
                        Ok(()) => {
                            project.detach_worktree(&name);
                            fallback = Some((
                                project.active_root().to_path_buf(),
                                project.log_query.clone(),
                            ));
                        }
                        Err(err) => {
                            model.toast(err, ToastKind::Error);
                        }
                    }
                }
                if let Some((root, query)) = fallback {
                    self.refresh_git(root.clone(), None);
                    self.refresh_log(root, query, 0);
                }
            }
        }
    }
}
