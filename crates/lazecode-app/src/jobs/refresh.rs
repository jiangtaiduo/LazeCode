//! Git/FS refresh jobs: project load, git snapshot, tree, log page,
//! branches, commit details, diffs and the selected-commit poll.

use std::path::PathBuf;
use std::sync::Arc;

use lazecode_core::{AppModel, DiffSource};
use lazecode_terminal::Notifier;

use super::*;

impl JobQueue {
    pub fn load_project(
        &mut self,
        path: PathBuf,
        settings: Settings,
        notifier: Option<Arc<dyn Notifier>>,
    ) {
        let key = format!("project:{}", path.display());
        let path_for_job = path.clone();
        self.spawn("project", key, move || {
            let result =
                ProjectState::open(path_for_job.clone(), &settings, notifier).map(Box::new);
            JobResult::ProjectLoaded {
                path: path_for_job,
                result,
            }
        });
    }

    pub fn refresh_git(&mut self, root: PathBuf, _notifier: Option<Arc<dyn Notifier>>) {
        let key = format!("git:{}", root.display());
        if self.in_flight.contains(&key) {
            self.pending_git.insert(root);
            return;
        }
        let job_root = root.clone();
        self.spawn("git", key, move || {
            let result = lazecode_git::snapshot(&job_root).map_err(|err| err.to_string());
            JobResult::GitRefreshed {
                root: job_root,
                result,
            }
        });
    }

    /// Rescan one worktree's tree off the UI thread; watcher bursts coalesce
    /// via `pending_trees`.
    pub fn refresh_tree(&mut self, root: PathBuf, ignored_patterns: Arc<Vec<String>>) {
        let key = format!("tree:{}", root.display());
        if self.in_flight.contains(&key) {
            self.pending_trees.insert(root);
            return;
        }
        let job_root = root.clone();
        self.spawn("tree", key, move || {
            let tree =
                lazecode_fs::tree::load_tree_filtered(&job_root, 20_000, Some(&ignored_patterns));
            JobResult::TreeLoaded {
                root: job_root,
                tree,
            }
        });
    }

    pub fn refresh_log(&mut self, root: PathBuf, query: lazecode_core::LogQuery, offset: usize) {
        let key = format!(
            "log:{}:{}:skip={}",
            root.display(),
            log_query_fingerprint(&query),
            offset
        );
        let job_root = root.clone();
        let pending_key = (root.clone(), offset);
        let spawned = self.spawn("log", key, move || {
            let options = log_options_from_query(&query, &job_root, LOG_PAGE_SIZE, offset);
            let result = match lazecode_git::log(&job_root, options) {
                Ok(commits) => {
                    let lanes = lazecode_git::log::lanes_after(&commits);
                    Ok((commits, lanes))
                }
                Err(err) => Err(err.to_string()),
            };
            JobResult::LogLoaded {
                root: job_root,
                branch: (!query.branch.trim().is_empty()).then(|| query.branch.clone()),
                query,
                offset,
                result,
            }
        });
        if !spawned {
            self.pending_log.insert(pending_key);
        }
    }

    pub fn refresh_branches(&mut self, root: PathBuf) {
        let key = format!("branches:{}", root.display());
        let job_root = root.clone();
        let spawned = self.spawn("branches", key, move || {
            let result = lazecode_git::branches(&job_root).unwrap_or_default();
            let tags = lazecode_git::tags(&job_root).unwrap_or_default();
            JobResult::BranchesLoaded {
                root: job_root,
                result,
                tags,
            }
        });
        if !spawned {
            self.pending_branches.insert(root);
        }
    }

    pub fn commit_details(&mut self, root: PathBuf, id: String) {
        let key = format!("details:{}:{id}", root.display());
        let job_root = root.clone();
        let job_id = id.clone();
        self.spawn("details", key, move || {
            let result = lazecode_git::commit_details(&job_root, &job_id)
                .map(Box::new)
                .map_err(|err| err.to_string());
            JobResult::CommitDetails {
                root: job_root,
                id: job_id,
                result,
            }
        });
    }

    pub fn diff_source(&mut self, root: PathBuf, source: DiffSource) {
        let key = format!("diff:{}:{source:?}", root.display());
        let job_root = root.clone();
        let job_source = source.clone();
        let spawned = self.spawn("diff", key.clone(), move || {
            let result = match &job_source {
                DiffSource::Worktree { path } => {
                    lazecode_git::file_diff(&job_root, path).map_err(|err| err.to_string())
                }
                DiffSource::Index { path } => {
                    lazecode_git::index_file_diff(&job_root, path).map_err(|err| err.to_string())
                }
                DiffSource::Commit { commit, path } => {
                    lazecode_git::commit_file_diff(&job_root, commit, path)
                        .map_err(|err| err.to_string())
                }
                DiffSource::CompareWithLocal { commit, path } => {
                    lazecode_git::diff_ref_worktree(&job_root, commit, path)
                        .map_err(|err| err.to_string())
                }
                DiffSource::CompareCommits { from, to, path } => {
                    lazecode_git::diff_commits(&job_root, from, to, path)
                        .map_err(|err| err.to_string())
                }
                DiffSource::Stash { stash, path } => {
                    lazecode_git::stash_file_diff(&job_root, stash, path)
                        .map_err(|err| err.to_string())
                }
                DiffSource::StashBeforeLocal { stash, path } => {
                    lazecode_git::stash_before_local_diff(&job_root, stash, path)
                        .map_err(|err| err.to_string())
                }
                DiffSource::Files { old_path, new_path } => {
                    lazecode_git::diff_two_files(old_path, new_path).map_err(|err| err.to_string())
                }
            };
            JobResult::DiffLoaded {
                root: job_root,
                source: job_source,
                result,
            }
        });
        if !spawned {
            self.pending_diffs.insert(key);
        }
    }

    pub(super) fn load_compare_local(&mut self, root: PathBuf, commit: String) {
        let key = format!("cmplocal:{root:?}:{commit}");
        let job_root = root.clone();
        let job_commit = commit.clone();
        let spawned = self.spawn("compare-local", key.clone(), move || {
            let result = lazecode_git::compare_commit_to_worktree(&job_root, &job_commit)
                .map_err(|err| err.to_string());
            JobResult::CmpSessionLoaded {
                root: job_root,
                from: job_commit,
                to: String::new(),
                local: true,
                result,
            }
        });
        if !spawned {
            self.pending_compare_local.insert(key);
        }
    }
    #[allow(dead_code)]
    pub fn file_diff(&mut self, root: PathBuf, path: String) {
        self.diff_source(root, DiffSource::Worktree { path });
    }

    pub fn is_saving(&self, path: &std::path::Path) -> bool {
        self.in_flight.contains(&format!("save:{}", path.display()))
    }

    pub fn ensure_selected_details(&mut self, model: &mut AppModel) {
        let Some(project) = model.active_project_mut() else {
            return;
        };
        let Some(id) = project.selected_commit.clone() else {
            return;
        };
        let loaded = project
            .commit_details
            .as_ref()
            .is_some_and(|details| details.info.id == id);
        if loaded || project.commit_details_attempted.as_deref() == Some(id.as_str()) {
            return;
        }
        let root = project.active_root().to_path_buf();
        project.commit_details_attempted = Some(id.clone());
        self.commit_details(root, id);
    }
}
