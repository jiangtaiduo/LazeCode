use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};
use lazecode_core::{
    AppModel, Command, DiffSource, DiffViewMode, ProjectState, Settings, TabKind, ToastKind,
    WorktreeState,
};
use lazecode_fs::SearchHit;
use lazecode_git::{BranchInfo, CommitDetails, CommitInfo, FileDiff, LogOptions, RepoSnapshot};
use lazecode_terminal::Notifier;
use lazecode_ui::UiRuntime;

/// Commit count loaded per log page. Small enough for a snappy first frame,
/// big enough that a repo rarely needs more than a couple of pages.
const LOG_PAGE_SIZE: usize = 300;

/// Build the `gix`-crate options for one page of a filtered log query. The
/// query keeps semantic filters; `offset` is this page's start index, which
/// always re-walks from the same tips so layout stays prefix-stable.
fn log_options_from_query(
    query: &lazecode_core::LogQuery,
    job_root: &std::path::Path,
    limit: usize,
    offset: usize,
) -> LogOptions {
    let head_ancestors = if query.all_refs || !query.branch.trim().is_empty() {
        // Only meaningful when history carries non-HEAD commits; cheap walk.
        lazecode_git::head_ancestors(job_root)
    } else {
        None
    };
    let since = match query.date {
        lazecode_core::DatePreset::All => (query.since > 0).then_some(query.since),
        lazecode_core::DatePreset::Today => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            Some(now - (now % 86400))
        }
        lazecode_core::DatePreset::Week => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            Some(now.saturating_sub(7 * 86400))
        }
        lazecode_core::DatePreset::Month => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            Some(now.saturating_sub(30 * 86400))
        }
    };
    let has_branch = !query.branch.trim().is_empty();
    LogOptions {
        limit: limit + offset,
        first_parent: query.first_parent,
        no_merges: query.no_merges,
        topo_order: query.topo_order,
        author: (!query.author.trim().is_empty()).then(|| query.author.clone()),
        since,
        path: (!query.path.trim().is_empty()).then(|| query.path.clone()),
        all_refs: query.all_refs && !has_branch,
        branch: has_branch.then(|| query.branch.trim().to_string()),
        head_ancestors,
        range_walk: false,
    }
}

/// Stable key so unrelated filter edits don't clobber the in-flight dedup.
fn log_query_fingerprint(query: &lazecode_core::LogQuery) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}",
        query.branch,
        query.author,
        query.path,
        query.date as u8,
        query.since,
        query.all_refs,
        query.first_parent,
        query.no_merges,
        query.topo_order,
    )
}

pub enum JobResult {
    ProjectLoaded {
        path: PathBuf,
        result: Result<Box<ProjectState>, String>,
    },
    GitRefreshed {
        root: PathBuf,
        result: Result<RepoSnapshot, String>,
    },
    CmpSessionLoaded {
        root: PathBuf,
        result: Result<Vec<lazecode_git::CompareFile>, String>,
    },
    LogLoaded {
        root: PathBuf,
        branch: Option<String>,
        query: lazecode_core::LogQuery,
        /// Page offset this job loaded (0 = fresh full query).
        offset: usize,
        result: Result<(Vec<CommitInfo>, Vec<Option<String>>), String>,
    },
    BranchesLoaded {
        root: PathBuf,
        result: Vec<BranchInfo>,
    },
    CommitDetails {
        root: PathBuf,
        id: String,
        result: Result<Box<CommitDetails>, String>,
    },
    DiffLoaded {
        root: PathBuf,
        source: DiffSource,
        result: Result<FileDiff, String>,
    },
    SearchDone {
        root: PathBuf,
        request_id: u64,
        query: String,
        result: Result<Vec<SearchHit>, String>,
    },
    OpDone {
        root: PathBuf,
        message: String,
        ok: bool,
        refresh: bool,
        reload_files: bool,
        clear_commit_message: bool,
    },
    /// 文件更改已回滚：除常规刷新外，还要关闭已不存在文件的编辑器标签
    /// （对齐稳定版：新增/未跟踪文件回滚后关闭标签页）。
    Discarded {
        root: PathBuf,
        paths: Vec<String>,
        message: String,
    },
    /// 分支比较整单回填（按 root+refs+paths 匹配 UiState 缓存；root 防
    /// 跨项目同名分支错配）。
    CompareBranchesLoaded {
        root: PathBuf,
        left: String,
        right: String,
        left_path: String,
        right_path: String,
        result: Result<lazecode_git::CompareBranches, String>,
    },
    /// 某一侧「Load More」的提交列表回填。
    CompareBranchesCommits {
        root: PathBuf,
        side: u8,
        exclude: String,
        include: String,
        result: Result<(Vec<lazecode_git::CommitInfo>, bool), String>,
    },
    /// 详情列文件清单回填（root+key 匹配）。
    CompareFilesLoaded {
        root: PathBuf,
        key: String,
        result: Result<Vec<lazecode_git::CompareFile>, String>,
    },
    Saved {
        root: PathBuf,
        path: PathBuf,
        revision: u64,
        result: Result<(), String>,
    },
    WorktreeAdded {
        project_root: PathBuf,
        name: String,
        result: Result<PathBuf, String>,
    },
    WorktreeRemoved {
        project_root: PathBuf,
        name: String,
        result: Result<(), String>,
    },
}

pub struct JobCompletion {
    pub key: String,
    pub result: JobResult,
}

pub struct JobQueue {
    pub rx: Receiver<JobCompletion>,
    tx: Sender<JobCompletion>,
    notifier: Arc<dyn Notifier>,
    in_flight: HashSet<String>,
    pending_git: HashSet<PathBuf>,
}

impl JobQueue {
    pub fn new(notifier: Arc<dyn Notifier>) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        Self {
            rx,
            tx,
            notifier,
            in_flight: HashSet::new(),
            pending_git: HashSet::new(),
        }
    }

    fn spawn(
        &mut self,
        name: &'static str,
        key: String,
        job: impl FnOnce() -> JobResult + Send + 'static,
    ) {
        if !self.in_flight.insert(key.clone()) {
            log::debug!("dropping duplicate {name} job for {key}");
            return;
        }
        let tx = self.tx.clone();
        let notifier = self.notifier.clone();
        let job_key = key.clone();
        let thread = std::thread::Builder::new()
            .name(format!("lazecode-{name}"))
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
                    .unwrap_or_else(|_| JobResult::OpDone {
                        root: PathBuf::new(),
                        message: "background task panicked".to_string(),
                        ok: false,
                        refresh: false,
                        reload_files: false,
                        clear_commit_message: false,
                    });
                let _ = tx.send(JobCompletion {
                    key: job_key,
                    result,
                });
                notifier.notify();
            });
        if let Err(err) = thread {
            self.in_flight.remove(&key);
            log::error!("cannot spawn {name} job: {err}");
        }
    }

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

    pub fn refresh_log(&mut self, root: PathBuf, query: lazecode_core::LogQuery, offset: usize) {
        let key = format!(
            "log:{}:{}:skip={}",
            root.display(),
            log_query_fingerprint(&query),
            offset
        );
        let job_root = root.clone();
        self.spawn("log", key, move || {
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
    }

    pub fn refresh_branches(&mut self, root: PathBuf) {
        let key = format!("branches:{}", root.display());
        let job_root = root.clone();
        self.spawn("branches", key, move || {
            let result = lazecode_git::branches(&job_root).unwrap_or_default();
            JobResult::BranchesLoaded {
                root: job_root,
                result,
            }
        });
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
        self.spawn("diff", key, move || {
            let result = match &job_source {
                DiffSource::Worktree { path } => {
                    lazecode_git::file_diff(&job_root, path).map_err(|err| err.to_string())
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
        let root = project.root.clone();
        project.commit_details_attempted = Some(id.clone());
        self.commit_details(root, id);
    }

    pub fn execute(&mut self, model: &mut AppModel, runtime: &mut UiRuntime, command: Command) {
        match command {
            Command::OpenProjectDialog => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Open project")
                    .pick_folder()
                {
                    self.load_project(path, model.settings.clone(), model.notifier.clone());
                }
            }
            Command::SavePatch(paths) => {
                // 稳定版 Create Patch from Local Changes：保存对话框在 UI
                // 线程（与 OpenProjectDialog 同一先例），写文件走后台任务。
                let default_name = match paths.len() {
                    1 => paths[0].rsplit('/').next().unwrap_or("changes").to_string(),
                    _ => "changes".to_string(),
                };
                let Some(dest) = rfd::FileDialog::new()
                    .set_title("Create Patch")
                    .set_file_name(format!("{default_name}.patch"))
                    .add_filter("Patch files", &["patch", "diff"])
                    .save_file()
                else {
                    return;
                };
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                let paths = paths.clone();
                self.spawn(
                    "save-patch",
                    format!("patch:{root:?}:{paths:?}:{dest:?}"),
                    move || {
                        let result =
                            lazecode_git::diff_patch_paths(&root, &paths).and_then(|patch| {
                                std::fs::write(&dest, patch).map_err(|err| anyhow::anyhow!("{err}"))
                            });
                        let name = dest
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        JobResult::OpDone {
                            root,
                            message: match &result {
                                Ok(()) => {
                                    if zh {
                                        format!("补丁已保存到 {name}")
                                    } else {
                                        format!("Patch saved to {name}")
                                    }
                                }
                                Err(err) => err.to_string(),
                            },
                            ok: result.is_ok(),
                            refresh: false,
                            reload_files: false,
                            clear_commit_message: false,
                        }
                    },
                );
            }
            Command::OpenProject(path) => {
                self.load_project(path, model.settings.clone(), model.notifier.clone());
            }
            Command::CloseProject(index) => {
                let dirty = model
                    .project_at(index)
                    .is_some_and(|project| project.tabs.iter().any(|tab| tab.is_dirty()));
                if dirty {
                    model.toast(
                        if model.settings.language.is_zh() {
                            "该项目有未保存的文件，请先保存或关闭标签页"
                        } else {
                            "This project has unsaved files; save or close its tabs first"
                        },
                        ToastKind::Warning,
                    );
                } else {
                    model.close_project(index);
                    model.save_workspace();
                    runtime.clear_terminal_cache();
                }
            }
            Command::ReloadGit => {
                if let Some(project) = model.active_project() {
                    let root = project.root.clone();
                    self.refresh_git(root, model.notifier.clone());
                }
            }
            Command::ReloadTree => {
                let ignored = model.settings.ignored_patterns.clone();
                if let Some(project) = model.active_project_mut() {
                    project.tree = lazecode_fs::tree::load_tree_with_expanded(
                        &project.root,
                        Some(&project.expanded),
                        20_000,
                        Some(&ignored),
                    );
                }
            }
            Command::Stage(paths) => {
                if let Some(project) = model.active_project() {
                    let root = project.root.clone();
                    let zh = model.settings.language.is_zh();
                    let key = format!("stage:{}", root.display());
                    let job_root = root.clone();
                    self.spawn("stage", key, move || {
                        let result = lazecode_git::stage(&job_root, &paths);
                        match result {
                            Ok(()) => JobResult::OpDone {
                                root: job_root,
                                message: if zh {
                                    format!("已暂存 {} 个文件", paths.len())
                                } else {
                                    format!("Staged {} files", paths.len())
                                },
                                ok: true,
                                refresh: true,
                                reload_files: false,
                                clear_commit_message: false,
                            },
                            Err(err) => JobResult::OpDone {
                                root: job_root,
                                message: err.to_string(),
                                ok: false,
                                refresh: false,
                                reload_files: false,
                                clear_commit_message: false,
                            },
                        }
                    });
                }
            }
            Command::Unstage(paths) => {
                if let Some(project) = model.active_project() {
                    let root = project.root.clone();
                    let zh = model.settings.language.is_zh();
                    let key = format!("unstage:{}", root.display());
                    let job_root = root.clone();
                    self.spawn("unstage", key, move || {
                        let result = lazecode_git::unstage(&job_root, &paths);
                        match result {
                            Ok(()) => JobResult::OpDone {
                                root: job_root,
                                message: if zh {
                                    format!("已取消暂存 {} 个文件", paths.len())
                                } else {
                                    format!("Unstaged {} files", paths.len())
                                },
                                ok: true,
                                refresh: true,
                                reload_files: false,
                                clear_commit_message: false,
                            },
                            Err(err) => JobResult::OpDone {
                                root: job_root,
                                message: err.to_string(),
                                ok: false,
                                refresh: false,
                                reload_files: false,
                                clear_commit_message: false,
                            },
                        }
                    });
                }
            }
            Command::Commit => {
                let Some(project) = model.active_project() else {
                    return;
                };
                let root = project.root.clone();
                let message = project.commit_message.clone();
                let amend = project.amend;
                let paths = project.checked_changes();
                let excluded: Vec<String> = project
                    .changes()
                    .iter()
                    .map(|change| change.path.clone())
                    .filter(|path| !paths.contains(path))
                    .collect();
                let changes = project.changes().len();
                let zh = model.settings.language.is_zh();
                if message.trim().is_empty() {
                    model.toast(
                        if zh {
                            "请先填写提交信息"
                        } else {
                            "Add a commit message first"
                        },
                        ToastKind::Warning,
                    );
                    return;
                }
                if changes == 0 && !amend {
                    model.toast(
                        if zh {
                            "没有可提交内容"
                        } else {
                            "Nothing to commit"
                        },
                        ToastKind::Warning,
                    );
                    return;
                }
                let key = format!("commit:{}", root.display());
                let job_root = root.clone();
                self.spawn("commit", key, move || {
                    let result =
                        lazecode_git::commit_checked(&job_root, &paths, &excluded, &message, amend);
                    match result {
                        Ok(id) => JobResult::OpDone {
                            root: job_root,
                            message: if zh {
                                format!("已提交 {}", &id[..id.len().min(8)])
                            } else {
                                format!("Committed {}", &id[..id.len().min(8)])
                            },
                            ok: true,
                            refresh: true,
                            reload_files: false,
                            clear_commit_message: true,
                        },
                        Err(err) => JobResult::OpDone {
                            root: job_root,
                            message: err.to_string(),
                            ok: false,
                            refresh: false,
                            reload_files: false,
                            clear_commit_message: false,
                        },
                    }
                });
            }
            Command::CommitPaths { paths, message } => {
                let Some(project) = model.active_project() else {
                    return;
                };
                let root = project.root.clone();
                let excluded: Vec<String> = project
                    .changes()
                    .iter()
                    .map(|change| change.path.clone())
                    .filter(|path| !paths.contains(path))
                    .collect();
                let zh = model.settings.language.is_zh();
                if message.trim().is_empty() {
                    model.toast(
                        if zh {
                            "请先填写提交信息"
                        } else {
                            "Add a commit message first"
                        },
                        ToastKind::Warning,
                    );
                    return;
                }
                let key = format!("commit-paths:{}", root.display());
                let job_root = root.clone();
                let message = message.clone();
                self.spawn("commit-paths", key, move || {
                    let result =
                        lazecode_git::commit_checked(&job_root, &paths, &excluded, &message, false);
                    match result {
                        Ok(id) => JobResult::OpDone {
                            root: job_root,
                            message: if zh {
                                format!("已提交 {}", &id[..id.len().min(8)])
                            } else {
                                format!("Committed {}", &id[..id.len().min(8)])
                            },
                            ok: true,
                            refresh: true,
                            reload_files: false,
                            clear_commit_message: false,
                        },
                        Err(err) => JobResult::OpDone {
                            root: job_root,
                            message: err.to_string(),
                            ok: false,
                            refresh: false,
                            reload_files: false,
                            clear_commit_message: false,
                        },
                    }
                });
            }
            Command::ChangelistCreate(name) => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                let name = name.clone();
                self.spawn(
                    "clist-create",
                    format!("clist:{root:?}:{name}"),
                    move || {
                        let mut lists = lazecode_git::Changelists::open(&root);
                        let result = lists.create_list_checked(&name).map(|_| ());
                        JobResult::OpDone {
                            root,
                            message: match &result {
                                Ok(_) => {
                                    if zh {
                                        format!("已创建变更列表 {name}")
                                    } else {
                                        format!("Created changelist {name}")
                                    }
                                }
                                Err(err) => err.to_string(),
                            },
                            ok: result.is_ok(),
                            refresh: true,
                            reload_files: false,
                            clear_commit_message: false,
                        }
                    },
                );
            }
            Command::ChangelistDelete { name, rollback } => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                let name = name.clone();
                self.spawn(
                    "clist-delete",
                    format!("clist-d:{root:?}:{name}:{rollback}"),
                    move || {
                        // rollback=true 逐文件回滚（对齐稳定版 deleteChangelist
                        // 的 Rollback 分支）；否则文件隐式回归默认列表。
                        let mut lists = lazecode_git::Changelists::open(&root);
                        let orphans = match lists.remove_list(&name) {
                            Ok(orphans) => orphans,
                            Err(error) => {
                                return JobResult::OpDone {
                                    root: root.clone(),
                                    message: error.to_string(),
                                    ok: false,
                                    refresh: false,
                                    reload_files: false,
                                    clear_commit_message: false,
                                };
                            }
                        };
                        let result: anyhow::Result<Vec<String>> = if rollback {
                            let mut first_error: Option<anyhow::Error> = None;
                            for path in &orphans {
                                if let Err(err) = lazecode_git::discard_file_changes(&root, path) {
                                    if first_error.is_none() {
                                        first_error = Some(err);
                                    }
                                }
                            }
                            match first_error {
                                None => Ok(orphans),
                                Some(err) => Err(err),
                            }
                        } else {
                            Ok(orphans)
                        };
                        JobResult::OpDone {
                            root,
                            message: match &result {
                                Ok(orphans) => {
                                    if zh {
                                        format!("已删除变更列表 {name}（{} 个文件）", orphans.len())
                                    } else {
                                        format!(
                                            "Deleted changelist {name} ({} files)",
                                            orphans.len()
                                        )
                                    }
                                }
                                Err(err) => err.to_string(),
                            },
                            ok: result.is_ok(),
                            refresh: true,
                            reload_files: rollback,
                            clear_commit_message: false,
                        }
                    },
                );
            }
            Command::ChangelistAssign { paths, list } => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                let paths = paths.clone();
                let count = paths.len();
                self.spawn(
                    "clist-assign",
                    format!("clist-a:{root:?}:{list}:{paths:?}"),
                    move || {
                        let mut lists = lazecode_git::Changelists::open(&root);
                        match lists.assign_many(&paths, &list) {
                            Ok(()) => JobResult::OpDone {
                                root,
                                message: if zh {
                                    format!("已移动 {count} 个文件到 {list}")
                                } else {
                                    format!("Moved {count} files to {list}")
                                },
                                ok: true,
                                refresh: true,
                                reload_files: false,
                                clear_commit_message: false,
                            },
                            Err(error) => JobResult::OpDone {
                                root,
                                message: error.to_string(),
                                ok: false,
                                refresh: false,
                                reload_files: false,
                                clear_commit_message: false,
                            },
                        }
                    },
                );
            }
            Command::CompareBranches {
                left,
                right,
                left_path,
                right_path,
            } => {
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn(
                    "cb",
                    format!("cb:{root:?}:{left}:{right}:{left_path}:{right_path}"),
                    move || {
                        let result = lazecode_git::compare_branches(
                            &root,
                            &left,
                            &right,
                            Some(&left_path),
                            Some(&right_path),
                            1000,
                        )
                        .map_err(|err| err.to_string());
                        JobResult::CompareBranchesLoaded {
                            root,
                            left,
                            right,
                            left_path,
                            right_path,
                            result,
                        }
                    },
                );
            }
            Command::CompareBranchesMore {
                side,
                exclude,
                include,
                limit,
                path,
            } => {
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn(
                    "cb-more",
                    format!("cbm:{root:?}:{side}:{exclude}:{include}:{limit}:{path}"),
                    move || {
                        // 多取 1 条探测截断（对齐稳定版 fetch_limit=limit+1）。
                        let options = lazecode_git::LogOptions {
                            limit: limit + 1,
                            branch: Some(format!("{exclude}..{include}")),
                            topo_order: true,
                            path: (!path.is_empty()).then_some(path.clone()),
                            ..Default::default()
                        };
                        let result = lazecode_git::log(&root, options)
                            .map(|mut commits| {
                                let truncated = commits.len() > limit;
                                commits.truncate(limit);
                                (commits, truncated)
                            })
                            .map_err(|err| err.to_string());
                        JobResult::CompareBranchesCommits {
                            root,
                            side,
                            exclude,
                            include,
                            result,
                        }
                    },
                );
            }
            Command::CompareFiles {
                key,
                commit,
                from,
                to,
            } => {
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn("cb-files", format!("cbf:{root:?}:{key}"), move || {
                    let result = if !commit.is_empty() {
                        lazecode_git::commit_files(&root, &commit)
                    } else {
                        lazecode_git::compare_commits(&root, &from, &to)
                    }
                    .map_err(|err| err.to_string());
                    JobResult::CompareFilesLoaded { root, key, result }
                });
            }
            Command::CheckoutBranch(name) => {
                let Some(project) = model.active_project() else {
                    return;
                };
                let dirty = project
                    .tabs
                    .iter()
                    .any(|tab| matches!(tab.kind, TabKind::File(_)) && tab.is_dirty());
                if dirty {
                    model.toast(
                        "Save or discard editor changes before switching branches",
                        ToastKind::Warning,
                    );
                    return;
                }
                let root = project.root.clone();
                let zh = model.settings.language.is_zh();
                let key = format!("checkout:{}", root.display());
                let job_root = root.clone();
                self.spawn("checkout", key, move || {
                    let result = lazecode_git::checkout_branch(&job_root, &name);
                    match result {
                        Ok(()) => JobResult::OpDone {
                            root: job_root,
                            message: if zh {
                                format!("已切换到 {name}")
                            } else {
                                format!("Checked out {name}")
                            },
                            ok: true,
                            refresh: true,
                            reload_files: true,
                            clear_commit_message: false,
                        },
                        Err(err) => JobResult::OpDone {
                            root: job_root,
                            message: err.to_string(),
                            ok: false,
                            refresh: false,
                            reload_files: false,
                            clear_commit_message: false,
                        },
                    }
                });
            }
            Command::CreateBranch(name) => {
                let Some(project) = model.active_project() else {
                    return;
                };
                let root = project.root.clone();
                let zh = model.settings.language.is_zh();
                let key = format!("branch:{}", root.display());
                let job_root = root.clone();
                self.spawn("branch", key, move || {
                    let result = lazecode_git::create_branch(&job_root, &name, false);
                    match result {
                        Ok(()) => JobResult::OpDone {
                            root: job_root,
                            message: if zh {
                                format!("已创建分支 {name}")
                            } else {
                                format!("Created branch {name}")
                            },
                            ok: true,
                            refresh: true,
                            reload_files: false,
                            clear_commit_message: false,
                        },
                        Err(err) => JobResult::OpDone {
                            root: job_root,
                            message: err.to_string(),
                            ok: false,
                            refresh: false,
                            reload_files: false,
                            clear_commit_message: false,
                        },
                    }
                });
            }
            Command::DeleteBranch(name) => {
                let Some(project) = model.active_project() else {
                    return;
                };
                let root = project.root.clone();
                let zh = model.settings.language.is_zh();
                let key = format!("delete-branch:{}", root.display());
                let job_root = root.clone();
                self.spawn("delete-branch", key, move || {
                    let result = lazecode_git::delete_branch(&job_root, &name);
                    match result {
                        Ok(()) => JobResult::OpDone {
                            root: job_root,
                            message: if zh {
                                format!("已删除分支 {name}")
                            } else {
                                format!("Deleted branch {name}")
                            },
                            ok: true,
                            refresh: true,
                            reload_files: false,
                            clear_commit_message: false,
                        },
                        Err(err) => JobResult::OpDone {
                            root: job_root,
                            message: err.to_string(),
                            ok: false,
                            refresh: false,
                            reload_files: false,
                            clear_commit_message: false,
                        },
                    }
                });
            }
            Command::RenameBranch { from, to } => {
                let Some(project) = model.active_project() else {
                    return;
                };
                let root = project.root.clone();
                let zh = model.settings.language.is_zh();
                let key = format!("rename-branch:{}:{from}", root.display());
                let job_root = root.clone();
                self.spawn("rename-branch", key, move || {
                    let result = lazecode_git::rename_branch(&job_root, &from, &to);
                    match result {
                        Ok(_) => JobResult::OpDone {
                            root: job_root,
                            message: if zh {
                                format!("已重命名分支 {from} → {to}")
                            } else {
                                format!("Renamed branch {from} → {to}")
                            },
                            ok: true,
                            refresh: true,
                            reload_files: false,
                            clear_commit_message: false,
                        },
                        Err(err) => JobResult::OpDone {
                            root: job_root,
                            message: err.to_string(),
                            ok: false,
                            refresh: false,
                            reload_files: false,
                            clear_commit_message: false,
                        },
                    }
                });
            }
            Command::CreateTag { name, commit } => {
                let Some(project) = model.active_project() else {
                    return;
                };
                let root = project.root.clone();
                let zh = model.settings.language.is_zh();
                let key = format!("create-tag:{}:{name}:{commit}", root.display());
                let job_root = root.clone();
                self.spawn("create-tag", key, move || {
                    let result = lazecode_git::create_tag(&job_root, &name, &commit, None);
                    match result {
                        Ok(()) => JobResult::OpDone {
                            root: job_root,
                            message: if zh {
                                format!("已创建标签 {name}")
                            } else {
                                format!("Created tag {name}")
                            },
                            ok: true,
                            refresh: true,
                            reload_files: false,
                            clear_commit_message: false,
                        },
                        Err(err) => JobResult::OpDone {
                            root: job_root,
                            message: err.to_string(),
                            ok: false,
                            refresh: false,
                            reload_files: false,
                            clear_commit_message: false,
                        },
                    }
                });
            }
            Command::DeleteTag(name) => {
                let Some(project) = model.active_project() else {
                    return;
                };
                let root = project.root.clone();
                let zh = model.settings.language.is_zh();
                let key = format!("delete-tag:{}:{name}", root.display());
                let job_root = root.clone();
                self.spawn("delete-tag", key, move || {
                    let result = lazecode_git::delete_tag(&job_root, &name);
                    match result {
                        Ok(()) => JobResult::OpDone {
                            root: job_root,
                            message: if zh {
                                format!("已删除标签 {name}")
                            } else {
                                format!("Deleted tag {name}")
                            },
                            ok: true,
                            refresh: true,
                            reload_files: false,
                            clear_commit_message: false,
                        },
                        Err(err) => JobResult::OpDone {
                            root: job_root,
                            message: err.to_string(),
                            ok: false,
                            refresh: false,
                            reload_files: false,
                            clear_commit_message: false,
                        },
                    }
                });
            }
            Command::FetchAll => {
                let zh = model.settings.language.is_zh();
                self.spawn("fetch-all", "fetch-all".to_string(), move || {
                    JobResult::OpDone {
                        root: PathBuf::new(),
                        message: if zh {
                            "fetch 也将在 M4 连接远程后生效".to_string()
                        } else {
                            "fetch will connect to remotes in M4".to_string()
                        },
                        ok: true,
                        refresh: false,
                        reload_files: false,
                        clear_commit_message: false,
                    }
                });
            }
            Command::Push {
                remote,
                branch,
                set_upstream,
                force,
            } => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn("push", format!("push:{root:?}:{branch}"), move || {
                    let result = lazecode_git::push(&root, &remote, &branch, set_upstream, force);
                    match result {
                        Ok(text) => JobResult::OpDone {
                            root: root.clone(),
                            message: {
                                let trimmed = text.lines().last().unwrap_or("").to_string();
                                if trimmed.is_empty() {
                                    if zh {
                                        format!("已推送到 {remote}")
                                    } else {
                                        format!("Pushed to {remote}")
                                    }
                                } else {
                                    trimmed
                                }
                            },
                            ok: true,
                            refresh: true,
                            reload_files: false,
                            clear_commit_message: false,
                        },
                        Err(err) => JobResult::OpDone {
                            root: root.clone(),
                            message: err.to_string(),
                            ok: false,
                            refresh: false,
                            reload_files: false,
                            clear_commit_message: false,
                        },
                    }
                });
            }
            Command::Pull { mode } => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn("pull", format!("pull:{root:?}:{mode:?}"), move || {
                    let result = lazecode_git::pull(&root, mode);
                    match result {
                        Ok(text) => JobResult::OpDone {
                            root: root.clone(),
                            message: {
                                if zh {
                                    "已拉取最新提交".to_string()
                                } else {
                                    "Pulled latest commits".to_string()
                                }
                            },
                            ok: true,
                            refresh: true,
                            reload_files: !text.is_empty(),
                            clear_commit_message: false,
                        },
                        Err(err) => JobResult::OpDone {
                            root: root.clone(),
                            message: err.to_string(),
                            ok: false,
                            refresh: false,
                            reload_files: false,
                            clear_commit_message: false,
                        },
                    }
                });
            }
            Command::RemoteAdd { name, url } => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn(
                    "remote-add",
                    format!("remote-add:{root:?}:{name}"),
                    move || {
                        let result = lazecode_git::remote_add(&root, &name, &url);
                        match result {
                            Ok(()) => JobResult::OpDone {
                                root: root.clone(),
                                message: if zh {
                                    format!("已添加远程 {name}")
                                } else {
                                    format!("Added remote {name}")
                                },
                                ok: true,
                                refresh: true,
                                reload_files: false,
                                clear_commit_message: false,
                            },
                            Err(err) => JobResult::OpDone {
                                root: root.clone(),
                                message: err.to_string(),
                                ok: false,
                                refresh: false,
                                reload_files: false,
                                clear_commit_message: false,
                            },
                        }
                    },
                );
            }
            Command::RemoteRemove(name) => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn(
                    "remote-remove",
                    format!("remote-rm:{root:?}:{name}"),
                    move || {
                        let result = lazecode_git::remote_remove(&root, &name);
                        match result {
                            Ok(()) => JobResult::OpDone {
                                root: root.clone(),
                                message: if zh {
                                    format!("已移除远程 {name}")
                                } else {
                                    format!("Removed remote {name}")
                                },
                                ok: true,
                                refresh: true,
                                reload_files: false,
                                clear_commit_message: false,
                            },
                            Err(err) => JobResult::OpDone {
                                root: root.clone(),
                                message: err.to_string(),
                                ok: false,
                                refresh: false,
                                reload_files: false,
                                clear_commit_message: false,
                            },
                        }
                    },
                );
            }
            Command::RemoteRename { from, to } => {
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn(
                    "remote-rename",
                    format!("remote-rn:{root:?}:{from}"),
                    move || {
                        let ok = lazecode_git::remote_rename(&root, &from, &to);
                        let message = match &ok {
                            Ok(()) => format!("Renamed remote {from} → {to}"),
                            Err(err) => err.to_string(),
                        };
                        JobResult::OpDone {
                            root,
                            message,
                            ok: ok.is_ok(),
                            refresh: true,
                            reload_files: false,
                            clear_commit_message: false,
                        }
                    },
                );
            }
            Command::RemoteSetUrl { name, url } => {
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn(
                    "remote-url",
                    format!("remote-url:{root:?}:{name}"),
                    move || {
                        let ok = lazecode_git::remote_set_url(&root, &name, &url);
                        let message = match &ok {
                            Ok(()) => format!("Updated remote {name}"),
                            Err(err) => err.to_string(),
                        };
                        JobResult::OpDone {
                            root,
                            message,
                            ok: ok.is_ok(),
                            refresh: true,
                            reload_files: false,
                            clear_commit_message: false,
                        }
                    },
                );
            }
            Command::Merge { ref_name } => {
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn("merge", format!("merge:{root:?}:{ref_name}"), move || {
                    let result = lazecode_git::merge(&root, &ref_name);
                    JobResult::OpDone {
                        root,
                        message: match &result {
                            Ok(_) => format!("Merged {ref_name}"),
                            Err(err) => err.to_string(),
                        },
                        ok: result.is_ok(),
                        refresh: true,
                        reload_files: true,
                        clear_commit_message: false,
                    }
                });
            }
            Command::MergeContinue => {
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn("merge-c", format!("mc:{root:?}"), move || {
                    let result = lazecode_git::merge_continue(&root);
                    JobResult::OpDone {
                        root,
                        message: match &result {
                            Ok(_) => "merge finished".to_string(),
                            Err(err) => err.to_string(),
                        },
                        ok: result.is_ok(),
                        refresh: true,
                        reload_files: true,
                        clear_commit_message: false,
                    }
                });
            }
            Command::MergeAbort => {
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn("merge-a", format!("ma:{root:?}"), move || {
                    let result = lazecode_git::merge_abort(&root);
                    JobResult::OpDone {
                        root,
                        message: match &result {
                            Ok(_) => "merge aborted".to_string(),
                            Err(err) => err.to_string(),
                        },
                        ok: result.is_ok(),
                        refresh: true,
                        reload_files: true,
                        clear_commit_message: false,
                    }
                });
            }
            Command::Rebase { onto } => {
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn("rebase", format!("rb:{root:?}:{onto}"), move || {
                    let result = lazecode_git::rebase(&root, &onto);
                    JobResult::OpDone {
                        root,
                        message: match &result {
                            Ok(_) => format!("Rebased onto {onto}"),
                            Err(err) => err.to_string(),
                        },
                        ok: result.is_ok(),
                        refresh: true,
                        reload_files: true,
                        clear_commit_message: false,
                    }
                });
            }
            Command::InteractiveRebase { onto, steps } => {
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn(
                    "rebase-i",
                    format!("rbi:{root:?}:{onto}:{steps:?}"),
                    move || {
                        let result = lazecode_git::rebase_interactive(&root, &onto, &steps);
                        JobResult::OpDone {
                            root,
                            message: match &result {
                                Ok(_) => format!("Interactive rebase onto {onto} done"),
                                Err(err) => err.to_string(),
                            },
                            ok: result.is_ok(),
                            refresh: true,
                            reload_files: true,
                            clear_commit_message: false,
                        }
                    },
                );
            }
            Command::RebaseContinue => {
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn("rebase-c", format!("rc:{root:?}"), move || {
                    let result = lazecode_git::rebase_continue(&root);
                    JobResult::OpDone {
                        root,
                        message: match &result {
                            Ok(_) => "rebase continuing".to_string(),
                            Err(err) => err.to_string(),
                        },
                        ok: result.is_ok(),
                        refresh: true,
                        reload_files: true,
                        clear_commit_message: false,
                    }
                });
            }
            Command::RebaseAbort => {
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn("rebase-a", format!("ra:{root:?}"), move || {
                    let result = lazecode_git::rebase_abort(&root);
                    JobResult::OpDone {
                        root,
                        message: match &result {
                            Ok(_) => "rebase aborted".to_string(),
                            Err(err) => err.to_string(),
                        },
                        ok: result.is_ok(),
                        refresh: true,
                        reload_files: true,
                        clear_commit_message: false,
                    }
                });
            }
            Command::RebaseSkip => {
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn("rebase-s", format!("rs:{root:?}"), move || {
                    let result = lazecode_git::rebase_skip(&root);
                    JobResult::OpDone {
                        root,
                        message: match &result {
                            Ok(_) => "rebase skipped".to_string(),
                            Err(err) => err.to_string(),
                        },
                        ok: result.is_ok(),
                        refresh: true,
                        reload_files: true,
                        clear_commit_message: false,
                    }
                });
            }
            Command::CherryPick { hash } => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn("cherry-pick", format!("cp:{root:?}:{hash}"), move || {
                    let result = lazecode_git::cherry_pick(&root, &hash);
                    JobResult::OpDone {
                        root,
                        message: match &result {
                            Ok(_) => if zh {
                                format!("已 cherry-pick {hash}")
                            } else {
                                format!("Cherry-picked {hash}")
                            }
                            .to_string(),
                            Err(err) => err.to_string(),
                        },
                        ok: result.is_ok(),
                        refresh: true,
                        reload_files: true,
                        clear_commit_message: false,
                    }
                });
            }
            Command::ContinueCherryPick => {
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn("cp-continue", format!("cpc:{root:?}"), move || {
                    let result = lazecode_git::history::cherry_pick_continue(&root);
                    JobResult::OpDone {
                        root,
                        message: match &result {
                            Ok(_) => "cherry-pick 继续".to_string(),
                            Err(err) => err.to_string(),
                        },
                        ok: result.is_ok(),
                        refresh: true,
                        reload_files: true,
                        clear_commit_message: false,
                    }
                });
            }
            Command::AbortCherryPick => {
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn("cp-abort", format!("cpa:{root:?}"), move || {
                    let result = lazecode_git::history::cherry_pick_abort(&root);
                    JobResult::OpDone {
                        root,
                        message: match &result {
                            Ok(_) => "cherry-pick 已中止".to_string(),
                            Err(err) => err.to_string(),
                        },
                        ok: result.is_ok(),
                        refresh: true,
                        reload_files: true,
                        clear_commit_message: false,
                    }
                });
            }
            Command::RevertCommit { hash } => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn("revert", format!("rv:{root:?}:{hash}"), move || {
                    let result = lazecode_git::revert(&root, &hash);
                    JobResult::OpDone {
                        root,
                        message: match &result {
                            Ok(_) => {
                                if zh {
                                    format!("已 revert {hash}")
                                } else {
                                    format!("Reverted {hash}")
                                }
                            }
                            Err(err) => err.to_string(),
                        },
                        ok: result.is_ok(),
                        refresh: true,
                        reload_files: true,
                        clear_commit_message: false,
                    }
                });
            }
            Command::DropCommit { hash, force } => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn("drop", format!("drop:{root:?}:{hash}"), move || {
                    let result = lazecode_git::drop_commit(&root, &hash, force);
                    JobResult::OpDone {
                        root,
                        message: match &result {
                            Ok(_) => {
                                if zh {
                                    format!("已删除提交 {hash}")
                                } else {
                                    format!("Dropped commit {hash}")
                                }
                            }
                            Err(err) => err.to_string(),
                        },
                        ok: result.is_ok(),
                        refresh: true,
                        reload_files: true,
                        clear_commit_message: false,
                    }
                });
            }
            Command::ResetTo { hash, mode } => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn(
                    "reset",
                    format!("reset:{root:?}:{hash}:{mode:?}"),
                    move || {
                        let result = lazecode_git::reset(&root, &hash, mode);
                        JobResult::OpDone {
                            root,
                            message: match &result {
                                Ok(_) => {
                                    if zh {
                                        format!("已重置到 {hash}")
                                    } else {
                                        format!("Reset to {hash}")
                                    }
                                }
                                Err(err) => err.to_string(),
                            },
                            ok: result.is_ok(),
                            refresh: true,
                            reload_files: true,
                            clear_commit_message: false,
                        }
                    },
                );
            }
            Command::SquashCommits { hashes, message } => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                let hashes = hashes.clone();
                let message = message.clone();
                self.spawn(
                    "squash",
                    format!("sq:{root:?}:{}", hashes.len()),
                    move || {
                        let result = lazecode_git::squash_commits(&root, &hashes, Some(&message));
                        JobResult::OpDone {
                            root,
                            message: match &result {
                                Ok(msg) => {
                                    if zh {
                                        format!("压缩提交成功: {msg}")
                                    } else {
                                        format!("Squash succeeded: {msg}")
                                    }
                                }
                                Err(err) => err.to_string(),
                            },
                            ok: result.is_ok(),
                            refresh: true,
                            reload_files: true,
                            clear_commit_message: false,
                        }
                    },
                );
            }
            Command::AddToVcs(paths) => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                let paths = paths.clone();
                self.spawn("vcs-add", format!("vcsa:{root:?}:{paths:?}"), move || {
                    // 对齐稳定版 handleAddToVcs：git add 后刷新快照，
                    // 文件随之移出 Unversioned 组；toast 文案也逐字对齐。
                    let result = lazecode_git::ops::stage(&root, &paths);
                    let what = if paths.len() > 1 {
                        paths.len().to_string()
                    } else {
                        paths
                            .first()
                            .and_then(|path| path.rsplit('/').next())
                            .unwrap_or("")
                            .to_string()
                    };
                    JobResult::OpDone {
                        root,
                        message: match &result {
                            Ok(()) => {
                                if zh {
                                    format!("已将 {what} 加入版本控制")
                                } else {
                                    format!("Added {what} to version control")
                                }
                            }
                            Err(err) => {
                                if zh {
                                    format!("加入版本控制失败：{err}")
                                } else {
                                    format!("Failed to add to version control: {err}")
                                }
                            }
                        },
                        ok: result.is_ok(),
                        // 未跟踪 → 已暂存是状态列表的结构性变化，必须刷新。
                        refresh: true,
                        reload_files: false,
                        clear_commit_message: false,
                    }
                });
            }
            Command::DiscardFiles(paths) => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                let paths = paths.clone();
                self.spawn("discard", format!("disc:{root:?}:{paths:?}"), move || {
                    // 逐文件回滚（对齐稳定版 git_discard_file_changes）；
                    // 结果走 Discarded 变体：回滚的新增文件需要关闭其
                    // 编辑器标签（文件已不存在，reload 无意义）。
                    let mut first_error: Option<anyhow::Error> = None;
                    for path in &paths {
                        if let Err(err) = lazecode_git::discard_file_changes(&root, path) {
                            if first_error.is_none() {
                                first_error = Some(err);
                            }
                        }
                    }
                    match first_error {
                        None => {
                            let count = paths.len();
                            JobResult::Discarded {
                                root,
                                paths,
                                message: if zh {
                                    format!("已放弃 {count} 个文件的更改")
                                } else {
                                    format!("Discarded changes in {count} files")
                                },
                            }
                        }
                        Some(err) => JobResult::OpDone {
                            root,
                            message: err.to_string(),
                            ok: false,
                            refresh: true,
                            reload_files: false,
                            clear_commit_message: false,
                        },
                    }
                });
            }
            Command::CheckoutRevision(revision) => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                let revision = revision.clone();
                self.spawn(
                    "checkout_rev",
                    format!("corev:{root:?}:{revision}"),
                    move || {
                        let result = lazecode_git::checkout_revision(&root, &revision);
                        JobResult::OpDone {
                            root,
                            message: match &result {
                                Ok(_) => {
                                    if zh {
                                        format!("已检出版本 {revision}")
                                    } else {
                                        format!("Checked out revision {revision}")
                                    }
                                }
                                Err(err) => err.to_string(),
                            },
                            ok: result.is_ok(),
                            refresh: true,
                            reload_files: true,
                            clear_commit_message: false,
                        }
                    },
                );
            }
            Command::GitInit => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn("init", format!("init:{root:?}"), move || {
                    let result = lazecode_git::git_init(&root);
                    JobResult::OpDone {
                        root,
                        message: match &result {
                            Ok(_) => {
                                if zh {
                                    "Git 仓库初始化成功".to_string()
                                } else {
                                    "Git repository initialized".to_string()
                                }
                            }
                            Err(err) => err.to_string(),
                        },
                        ok: result.is_ok(),
                        refresh: true,
                        reload_files: false,
                        clear_commit_message: false,
                    }
                });
            }
            Command::UndoCommitHead => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                self.spawn("undo_head", format!("undo:{root:?}"), move || {
                    let result = lazecode_git::undo_commit_head(&root);
                    JobResult::OpDone {
                        root,
                        message: match &result {
                            Ok(_) => {
                                if zh {
                                    "已撤销 HEAD 提交（保留改动）".to_string()
                                } else {
                                    "Undid HEAD commit".to_string()
                                }
                            }
                            Err(err) => err.to_string(),
                        },
                        ok: result.is_ok(),
                        refresh: true,
                        reload_files: true,
                        clear_commit_message: false,
                    }
                });
            }
            Command::EditCommitMessageHead(message) => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                let message = message.clone();
                self.spawn("amend_msg", format!("amend_msg:{root:?}"), move || {
                    let result = lazecode_git::amend_commit_message(&root, &message);
                    JobResult::OpDone {
                        root,
                        message: match &result {
                            Ok(_) => {
                                if zh {
                                    "已修改提交信息".to_string()
                                } else {
                                    "Amended commit message".to_string()
                                }
                            }
                            Err(err) => err.to_string(),
                        },
                        ok: result.is_ok(),
                        refresh: true,
                        reload_files: false,
                        clear_commit_message: false,
                    }
                });
            }
            Command::GoToLine(line) => {
                if let Some(project) = model.active_project_mut() {
                    if let Some(active_id) = project.active_tab {
                        if let Some(tab) = project.tab_mut_by_id(active_id) {
                            if let lazecode_core::TabKind::File(file) = &mut tab.kind {
                                let target_line = line.saturating_sub(1);
                                file.doc.cursor.line =
                                    target_line.min(file.doc.line_count().saturating_sub(1));
                                file.doc.cursor.col = 0;
                                file.doc.selection = None;
                                file.doc.scroll_line = target_line.saturating_sub(5) as f32;
                            }
                        }
                    }
                }
            }
            Command::OpenFileAt {
                path,
                line,
                character,
            } => {
                let index = model
                    .projects
                    .iter()
                    .position(|project| {
                        project
                            .worktrees
                            .iter()
                            .any(|worktree| path.starts_with(&worktree.path))
                    })
                    .or(model.active_project);
                let Some(index) = index else {
                    return;
                };
                let settings = model.settings.clone();
                match model.projects[index].open_file(path.clone(), &settings) {
                    Ok(tab_id) => {
                        model.active_project = Some(index);
                        if let Some(tab) = model.projects[index].tab_mut_by_id(tab_id) {
                            if let lazecode_core::TabKind::File(file) = &mut tab.kind {
                                let cursor = file.doc.lsp_cursor(line, character);
                                file.doc.set_cursor(cursor, false);
                                file.doc.scroll_line = cursor.line.saturating_sub(6) as f32;
                            }
                        }
                    }
                    Err(error) => {
                        let zh = model.settings.language.is_zh();
                        let message = if zh {
                            format!("无法打开 {}: {error}", path.display())
                        } else {
                            format!("Cannot open {}: {error}", path.display())
                        };
                        model.toast(message, ToastKind::Error);
                    }
                }
            }
            Command::ApplyWorkspaceEdit(edit) => {
                let zh = model.settings.language.is_zh();
                let mut changed = 0usize;
                let mut failed: Vec<String> = Vec::new();
                for (path, edits) in edit.files {
                    if edits.is_empty() {
                        continue;
                    }
                    let mut applied = false;
                    for project in &mut model.projects {
                        for tab in &mut project.tabs {
                            if let lazecode_core::TabKind::File(file) = &mut tab.kind {
                                if file.doc.path == path {
                                    if file.doc.apply_text_edits(&edits) {
                                        applied = true;
                                        changed += 1;
                                    }
                                    break;
                                }
                            }
                        }
                        if applied {
                            break;
                        }
                    }
                    if applied {
                        continue;
                    }
                    // Not open: apply to the file on disk, preserving its
                    // encoding, EOL style and BOM.
                    match lazecode_fs::file::read_text_file(&path) {
                        Ok(file) => {
                            match lazecode_text::lsp::apply_text_edits(&file.text, &edits) {
                                Ok(text) if text != file.text => {
                                    if let Err(error) = lazecode_fs::file::write_text_file(
                                        &path,
                                        &text,
                                        file.encoding,
                                        file.eol,
                                        file.had_bom,
                                    ) {
                                        failed.push(format!("{}: {error}", path.display()));
                                    } else {
                                        changed += 1;
                                    }
                                }
                                Ok(_) => {}
                                Err(error) => failed.push(format!("{}: {error}", path.display())),
                            }
                        }
                        Err(error) => failed.push(format!("{}: {error}", path.display())),
                    }
                }
                if let Some(error) = failed.first() {
                    let message = if zh {
                        format!("重命名部分失败: {error}")
                    } else {
                        format!("Rename partially failed: {error}")
                    };
                    model.toast(message, ToastKind::Error);
                } else if changed > 0 {
                    let message = if zh {
                        format!("已更新 {changed} 个文件")
                    } else {
                        format!("Updated {changed} file(s)")
                    };
                    model.toast(message, ToastKind::Success);
                }
            }
            Command::StashPush {
                message,
                include_untracked,
            } => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                let message = message.clone();
                self.spawn("stash-push", format!("stash:{root:?}"), move || {
                    let result = lazecode_git::stash_push(
                        &root,
                        message.as_deref(),
                        false,
                        include_untracked,
                    );
                    JobResult::OpDone {
                        root,
                        message: match &result {
                            Ok(_) => {
                                if zh {
                                    "已贮藏更改".to_string()
                                } else {
                                    "Changes stashed".to_string()
                                }
                            }
                            Err(err) => err.to_string(),
                        },
                        ok: result.is_ok(),
                        refresh: true,
                        reload_files: true,
                        clear_commit_message: false,
                    }
                });
            }
            Command::StashApply(stash) => {
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                let stash = stash.clone();
                self.spawn(
                    "stash-apply",
                    format!("stash-a:{root:?}:{stash}"),
                    move || {
                        let result = lazecode_git::stash_apply(&root, &stash);
                        JobResult::OpDone {
                            root,
                            message: match &result {
                                Ok(_) => format!("已恢复 {stash}"),
                                Err(err) => err.to_string(),
                            },
                            ok: result.is_ok(),
                            refresh: true,
                            reload_files: true,
                            clear_commit_message: false,
                        }
                    },
                );
            }
            Command::StashDrop(stash) => {
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                let stash = stash.clone();
                self.spawn(
                    "stash-drop",
                    format!("stash-d:{root:?}:{stash}"),
                    move || {
                        let result = lazecode_git::stash_drop(&root, &stash);
                        JobResult::OpDone {
                            root,
                            message: match &result {
                                Ok(_) => format!("已删除 {stash}"),
                                Err(err) => err.to_string(),
                            },
                            ok: result.is_ok(),
                            refresh: true,
                            reload_files: false,
                            clear_commit_message: false,
                        }
                    },
                );
            }
            Command::ShelfSave { name, paths } => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                let name = name.clone();
                self.spawn("shelf-save", format!("shelf:{root:?}:{name}"), move || {
                    // paths 空 = 全部已跟踪变更；否则只架藏给定路径
                    // （对齐稳定版「只架藏勾选文件」）。
                    let result = lazecode_git::shelf_save_paths(&root, &name, &paths);
                    JobResult::OpDone {
                        root,
                        message: match &result {
                            Ok(_) => {
                                if zh {
                                    format!("已贮藏为 {name}")
                                } else {
                                    format!("Shelved as {name}")
                                }
                            }
                            Err(err) => err.to_string(),
                        },
                        ok: result.is_ok(),
                        refresh: true,
                        reload_files: true,
                        clear_commit_message: false,
                    }
                });
            }
            Command::ShelfDelete(name) => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                let name = name.clone();
                self.spawn("shelf-delete", format!("shelf-del:{root:?}:{name}"), move || {
                    let result = lazecode_git::shelf_delete(&root, &name);
                    JobResult::OpDone {
                        root,
                        message: match &result {
                            Ok(_) => {
                                if zh {
                                    format!("已删除架藏 {name}（可在最近删除中恢复）")
                                } else {
                                    format!("Deleted shelf {name} (restorable from Recently Deleted)")
                                }
                            }
                            Err(err) => err.to_string(),
                        },
                        ok: result.is_ok(),
                        refresh: true,
                        reload_files: false,
                        clear_commit_message: false,
                    }
                });
            }
            Command::ShelfRename { from, to } => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                let (from, to) = (from.clone(), to.clone());
                self.spawn(
                    "shelf-rename",
                    format!("shelf-ren:{root:?}:{from}:{to}"),
                    move || {
                        let result = lazecode_git::shelf_rename(&root, &from, &to);
                        JobResult::OpDone {
                            root,
                            message: match &result {
                                Ok(_) => {
                                    if zh {
                                        format!("已重命名为 {to}")
                                    } else {
                                        format!("Renamed to {to}")
                                    }
                                }
                                Err(err) => err.to_string(),
                            },
                            ok: result.is_ok(),
                            refresh: false,
                            reload_files: false,
                            clear_commit_message: false,
                        }
                    },
                );
            }
            Command::ShelfRestoreDeleted(name) => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                let name = name.clone();
                self.spawn(
                    "shelf-restore-del",
                    format!("shelf-resdel:{root:?}:{name}"),
                    move || {
                        let result = lazecode_git::shelf_restore_deleted(&root, &name);
                        JobResult::OpDone {
                            root,
                            message: match &result {
                                Ok(_) => {
                                    if zh {
                                        format!("已恢复架藏 {name}")
                                    } else {
                                        format!("Restored shelf {name}")
                                    }
                                }
                                Err(err) => err.to_string(),
                            },
                            ok: result.is_ok(),
                            refresh: false,
                            reload_files: false,
                            clear_commit_message: false,
                        }
                    },
                );
            }
            Command::ShelfPurgeDeleted(name) => {
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                let name = name.clone();
                self.spawn(
                    "shelf-purge",
                    format!("shelf-purge:{root:?}:{name}"),
                    move || {
                        let result = lazecode_git::shelf_purge_deleted(&root, &name);
                        JobResult::OpDone {
                            root,
                            message: match &result {
                                Ok(_) => "Purged".to_string(),
                                Err(err) => err.to_string(),
                            },
                            ok: result.is_ok(),
                            refresh: false,
                            reload_files: false,
                            clear_commit_message: false,
                        }
                    },
                );
            }
            Command::ShelfRestore {
                name,
                paths,
                target_list,
                delete_after,
            } => {
                let root = model
                    .active_project()
                    .map(|project| project.root.clone())
                    .unwrap_or_default();
                let name = name.clone();
                let d_after = delete_after;
                self.spawn(
                    "shelf-restore",
                    format!("shelf-r:{root:?}:{name}"),
                    move || {
                        // paths=Some 走部分恢复（只应用勾选文件，对齐稳定
                        // 版 Unshelve）；target_list 在恢复成功后把文件归
                        // 入该变更列表（不存在则创建）。
                        let result = shelf_restore_scoped(
                            &root,
                            &name,
                            paths.as_deref(),
                            target_list.as_deref(),
                            d_after,
                        );
                        JobResult::OpDone {
                            root,
                            message: match &result {
                                Ok(_) => format!("已恢复 shelf {name}"),
                                Err(err) => err.to_string(),
                            },
                            ok: result.is_ok(),
                            refresh: true,
                            reload_files: true,
                            clear_commit_message: false,
                        }
                    },
                );
            }
            Command::ApplyPatch {
                patch,
                cached,
                reverse,
                root,
            } => {
                let zh = model.settings.language.is_zh();
                self.spawn(
                    "apply-patch",
                    format!("apply:{}", root.display()),
                    move || {
                        let result = lazecode_git::apply_patch(&root, &patch, cached, reverse);
                        match result {
                            Ok(()) => JobResult::OpDone {
                                root,
                                message: if zh {
                                    if cached {
                                        if reverse {
                                            "已从暂存区取消该块".to_string()
                                        } else {
                                            "已暂存该块".to_string()
                                        }
                                    } else {
                                        "已放弃该块的修改".to_string()
                                    }
                                } else if cached {
                                    if reverse {
                                        "Hunk unstaged".to_string()
                                    } else {
                                        "Hunk staged".to_string()
                                    }
                                } else {
                                    "Hunk rolled back".to_string()
                                },
                                ok: true,
                                refresh: true,
                                reload_files: true,
                                clear_commit_message: false,
                            },
                            Err(err) => JobResult::OpDone {
                                root,
                                message: err.to_string(),
                                ok: false,
                                refresh: false,
                                reload_files: false,
                                clear_commit_message: false,
                            },
                        }
                    },
                );
            }
            Command::CompareOpen { from, to } => {
                let Some(project) = model.active_project_mut() else {
                    return;
                };
                project.compare_session = Some(lazecode_core::model::CompareSession {
                    from: from.clone(),
                    to: to.clone(),
                    loading: true,
                    group_by_dir: true,
                    root_expanded: true,
                    ..Default::default()
                });
                let root = project.root.clone();
                model.ui.sidebar_view = lazecode_core::model::SidebarView::Compare;
                model.ui.sidebar_visible = true;
                self.spawn(
                    "compare-files",
                    format!("cmp:{root:?}:{from}:{to}"),
                    move || {
                        let result = lazecode_git::compare_commits(&root, &from, &to)
                            .map_err(|err| err.to_string());
                        JobResult::CmpSessionLoaded { root, result }
                    },
                );
            }
            Command::CompareSwap => {
                let Some(project) = model.active_project_mut() else {
                    return;
                };
                let Some(session) = project.compare_session.as_mut() else {
                    return;
                };
                let old_from = session.from.clone();
                let old_to = session.to.clone();
                std::mem::swap(&mut session.from, &mut session.to);
                session.selected = None;
                session.loading = true;
                let (from, to) = (session.from.clone(), session.to.clone());
                let root = project.root.clone();
                // 已打开的 diff 单例标签同步换向并重载。
                let open_diff = project.tabs.iter_mut().find_map(|tab| match &mut tab.kind {
                    TabKind::Diff(diff) => match &diff.source {
                        DiffSource::CompareCommits { from: f, to: t, .. }
                            if *f == old_from && *t == old_to =>
                        {
                            Some(diff.path.clone())
                        }
                        _ => None,
                    },
                    _ => None,
                });
                if let Some(path) = open_diff {
                    let source = DiffSource::CompareCommits {
                        from: to.clone(),
                        to: from.clone(),
                        path,
                    };
                    if let Some(project) = model.active_project_mut() {
                        project.open_diff_source(source.clone());
                    }
                    self.diff_source(root.clone(), source);
                }
                self.spawn(
                    "compare-files",
                    format!("cmp:{root:?}:{from}:{to}"),
                    move || {
                        let result = lazecode_git::compare_commits(&root, &from, &to)
                            .map_err(|err| err.to_string());
                        JobResult::CmpSessionLoaded { root, result }
                    },
                );
            }
            Command::CompareClose => {
                if let Some(project) = model.active_project_mut() {
                    project.compare_session = None;
                }
                model.ui.sidebar_view = lazecode_core::model::SidebarView::Git;
                model.ui.sidebar_visible = true;
            }
            Command::RefreshLog | Command::LogFilterChanged => {
                if let Some(project) = model.active_project_mut() {
                    project.log_loading = true;
                    project.log_paging = false;
                    project.log_reached_end = false;
                }
                if let Some(project) = model.active_project() {
                    let root = project.root.clone();
                    let query = project.log_query.clone();
                    self.refresh_log(root.clone(), query, 0);
                    self.refresh_branches(root);
                }
            }
            Command::LogLoadMore => {
                let Some(project) = model.active_project() else {
                    return;
                };
                if project.log_reached_end {
                    return;
                }
                let root = project.root.clone();
                let query = project.log_query.clone();
                let offset = project.log.len();
                if let Some(project) = model.active_project_mut() {
                    project.log_paging = true;
                }
                self.refresh_log(root, query, offset);
            }
            Command::SelectCommit(id) => {
                if let Some(project) = model.active_project_mut() {
                    if project
                        .commit_details
                        .as_ref()
                        .map(|details| details.info.id.as_str())
                        != Some(id.as_str())
                    {
                        let root = project.root.clone();
                        project.commit_details_attempted = None;
                        self.commit_details(root, id);
                    }
                }
            }
            Command::OpenDiff(path) => {
                if let Some(project) = model.active_project_mut() {
                    // 稳定版语义：diff 打开时该文件成为变更列表当前选中
                    // （selectedFile → 行选中同步）。
                    project.change_selection.clear();
                    project.change_selection.insert(path.clone());
                    project.change_anchor = Some(path.clone());
                    let source = DiffSource::Worktree { path };
                    project.open_diff_source(source.clone());
                    let root = project.root.clone();
                    self.diff_source(root, source);
                }
            }
            Command::OpenCommitDiff { commit, path } => {
                if let Some(project) = model.active_project_mut() {
                    let source = DiffSource::Commit { commit, path };
                    project.open_diff_source(source.clone());
                    let root = project.root.clone();
                    self.diff_source(root, source);
                }
            }
            Command::OpenCompareWithLocalDiff { commit, path } => {
                if let Some(project) = model.active_project_mut() {
                    let source = DiffSource::CompareWithLocal { commit, path };
                    project.open_diff_source(source.clone());
                    let root = project.root.clone();
                    self.diff_source(root, source);
                }
            }
            Command::OpenCompareCommitsDiff { from, to, path } => {
                if let Some(project) = model.active_project_mut() {
                    if let Some(session) = project.compare_session.as_mut() {
                        session.selected = Some(path.clone());
                        let parts: Vec<&str> = path.split('/').collect();
                        for i in 1..parts.len() {
                            let parent = parts[..i].join("/");
                            session.collapsed.remove(&parent);
                        }
                        session.root_expanded = true;
                    }
                    let source = DiffSource::CompareCommits { from, to, path };
                    project.open_diff_source(source.clone());
                    let root = project.root.clone();
                    self.diff_source(root, source);
                }
            }
            Command::OpenFilesDiff { old_path, new_path } => {
                if let Some(project) = model.active_project_mut() {
                    let source = DiffSource::Files { old_path, new_path };
                    project.open_diff_source(source.clone());
                    let root = project.root.clone();
                    self.diff_source(root, source);
                }
            }
            Command::ToggleDiffViewMode(tab_id) => {
                if let Some(project) = model.active_project_mut() {
                    if let Some(tab) = project.tab_mut_by_id(tab_id) {
                        if let TabKind::Diff(diff) = &mut tab.kind {
                            diff.view_mode = match diff.view_mode {
                                DiffViewMode::Unified => DiffViewMode::SideBySide,
                                DiffViewMode::SideBySide => DiffViewMode::Unified,
                            };
                        }
                    }
                }
            }
            Command::NextDiffHunk(tab_id) => {
                if let Some(project) = model.active_project_mut() {
                    if let Some(tab) = project.tab_mut_by_id(tab_id) {
                        if let TabKind::Diff(diff) = &mut tab.kind {
                            let total = diff.diff.as_ref().map(|d| d.diff.hunks.len()).unwrap_or(0);
                            if total > 0 {
                                diff.current_hunk = (diff.current_hunk + 1) % total;
                            }
                        }
                    }
                }
            }
            Command::PrevDiffHunk(tab_id) => {
                if let Some(project) = model.active_project_mut() {
                    if let Some(tab) = project.tab_mut_by_id(tab_id) {
                        if let TabKind::Diff(diff) = &mut tab.kind {
                            let total = diff.diff.as_ref().map(|d| d.diff.hunks.len()).unwrap_or(0);
                            if total > 0 {
                                diff.current_hunk = (diff.current_hunk + total - 1) % total;
                            }
                        }
                    }
                }
            }
            Command::Search {
                query,
                request_id,
                options,
            } => {
                if let Some(project) = model.active_project() {
                    let root = project.root.clone();
                    let key = format!("search:{}:{request_id}", root.display());
                    let job_root = root.clone();
                    let job_query = query.clone();
                    self.spawn("search", key, move || {
                        let result =
                            lazecode_fs::search_in_files(&job_root, &job_query, &options, None)
                                .map_err(|err| err.to_string());
                        JobResult::SearchDone {
                            root: job_root,
                            request_id,
                            query: job_query,
                            result,
                        }
                    });
                }
            }
            Command::ReplaceInFiles {
                query,
                replacement,
                options,
            } => {
                let zh = model.settings.language.is_zh();
                if let Some(project) = model.active_project() {
                    let root = project.root.clone();
                    let key = format!("replace-in-files:{}", root.display());
                    self.spawn("replace-in-files", key, move || {
                        let result = lazecode_fs::replace_in_files(
                            &root,
                            &query,
                            &replacement,
                            &options,
                            None,
                        );
                        let (msg, ok) = match result {
                            Ok(count) => (
                                if zh {
                                    format!("已完成全局替换：修改了 {count} 处匹配")
                                } else {
                                    format!("Replaced {count} occurrences across project")
                                },
                                true,
                            ),
                            Err(err) => (err.to_string(), false),
                        };
                        JobResult::OpDone {
                            root,
                            message: msg,
                            ok,
                            refresh: true,
                            reload_files: true,
                            clear_commit_message: false,
                        }
                    });
                }
            }
            Command::SaveTab(id) => {
                let Some(project) = model.active_project_mut() else {
                    return;
                };
                let root = project.root.clone();
                let Some(tab) = project.tab_mut_by_id(id) else {
                    return;
                };
                let TabKind::File(file) = &mut tab.kind else {
                    return;
                };
                if file.doc.read_only {
                    model.toast(
                        if model.settings.language.is_zh() {
                            "此文件为只读"
                        } else {
                            "This file is read-only"
                        },
                        ToastKind::Warning,
                    );
                    return;
                }
                if file.doc.lossy {
                    model.toast(
                        if model.settings.language.is_zh() {
                            "该文件解码时有替换字符，保存会损坏原文件"
                        } else {
                            "This file was decoded lossily; saving would corrupt it"
                        },
                        ToastKind::Warning,
                    );
                    return;
                }
                let path = file.doc.path.clone();
                let revision = file.doc.revision;
                let data = SaveData {
                    text: file.doc.text(),
                    encoding: file.doc.encoding,
                    eol: file.doc.eol,
                    bom: file.doc.had_bom,
                };
                let key = format!("save:{}", path.display());
                let job_root = root.clone();
                let job_path = path.clone();
                self.spawn("save", key, move || {
                    let result = lazecode_fs::file::write_text_file(
                        &job_path,
                        &data.text,
                        data.encoding,
                        data.eol,
                        data.bom,
                    )
                    .map_err(|err| err.to_string());
                    JobResult::Saved {
                        root: job_root,
                        path: job_path,
                        revision,
                        result,
                    }
                });
            }
            Command::SaveAll => {
                let ids: Vec<u64> = model
                    .active_project()
                    .map(|project| {
                        project
                            .tabs
                            .iter()
                            .filter(|tab| tab.is_dirty())
                            .map(|tab| tab.id)
                            .collect()
                    })
                    .unwrap_or_default();
                for id in ids {
                    self.execute(model, runtime, Command::SaveTab(id));
                }
            }
            Command::NewTerminal => {
                let settings = model.settings.clone();
                let notifier = model.notifier.clone();
                if let Some(project) = model.active_project_mut() {
                    match project.open_terminal(
                        None,
                        None,
                        lazecode_terminal::SessionKind::Shell,
                        &settings,
                        notifier,
                    ) {
                        Ok(id) => {
                            runtime.set_terminal_focus(id);
                        }
                        Err(err) => {
                            model.toast(err, ToastKind::Error);
                        }
                    }
                    model.ui.terminal_visible = true;
                    if model.ui.terminal_placement == lazecode_core::TerminalPlacement::Bottom {
                        model.ui.panel_visible = false;
                    }
                    model.ui.panel_tab = lazecode_core::PanelTab::Console;
                } else {
                    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
                    if let Ok(index) = model.open_project(home) {
                        model.active_project = Some(index);
                        if let Some(project) = model.active_project_mut() {
                            if let Ok(id) = project.open_terminal(
                                None,
                                None,
                                lazecode_terminal::SessionKind::Shell,
                                &settings,
                                notifier,
                            ) {
                                runtime.set_terminal_focus(id);
                            }
                        }
                        model.ui.terminal_visible = true;
                        if model.ui.terminal_placement == lazecode_core::TerminalPlacement::Bottom {
                            model.ui.panel_visible = false;
                        }
                        model.ui.panel_tab = lazecode_core::PanelTab::Console;
                    }
                }
            }
            Command::NewTerminalCommand { title, command } => {
                let settings = model.settings.clone();
                let notifier = model.notifier.clone();
                if let Some(project) = model.active_project_mut() {
                    match project.open_terminal(
                        Some(command),
                        Some(title),
                        lazecode_terminal::SessionKind::Agent,
                        &settings,
                        notifier,
                    ) {
                        Ok(id) => {
                            runtime.set_terminal_focus(id);
                        }
                        Err(err) => {
                            model.toast(err, ToastKind::Error);
                        }
                    }
                    model.ui.terminal_visible = true;
                    if model.ui.terminal_placement == lazecode_core::TerminalPlacement::Bottom {
                        model.ui.panel_visible = false;
                    }
                    model.ui.panel_tab = lazecode_core::PanelTab::Console;
                }
            }
            Command::RestartTerminal(id) => {
                let settings = model.settings.clone();
                let notifier = model.notifier.clone();
                if let Some(project) = model.active_project_mut() {
                    if let Err(err) = project.restart_terminal(id, &settings, notifier) {
                        model.toast(err, ToastKind::Error);
                    }
                }
            }
            Command::CloseTerminal(id) => {
                if let Some(project) = model.active_project_mut() {
                    project.close_terminal(id);
                }
                runtime.forget_terminal(id);
            }
            Command::CreateWorktree {
                name,
                branch,
                location,
            } => {
                let Some(project) = model.active_project() else {
                    return;
                };
                let project_root = project.root.clone();
                let destination = if location.as_os_str().is_empty() {
                    project_root.join(".worktree").join(&name)
                } else {
                    location
                };
                if destination.exists() {
                    model.toast(
                        if model.settings.language.is_zh() {
                            format!("路径已存在：{}", destination.display())
                        } else {
                            format!("Path already exists: {}", destination.display())
                        },
                        ToastKind::Error,
                    );
                    return;
                }
                // 默认在当前项目的 .worktree 下，并且自动添加到 .gitignore
                if destination.starts_with(&project_root) {
                    let gitignore_path = project_root.join(".gitignore");
                    let gitignore_entry = ".worktree/";
                    let already_ignored = if gitignore_path.exists() {
                        std::fs::read_to_string(&gitignore_path)
                            .map(|c| {
                                c.lines().any(|l| {
                                    let t = l.trim();
                                    t == ".worktree"
                                        || t == ".worktree/"
                                        || t == "/.worktree"
                                        || t == "/.worktree/"
                                })
                            })
                            .unwrap_or(false)
                    } else {
                        false
                    };
                    if !already_ignored {
                        use std::io::Write;
                        if let Ok(mut f) = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&gitignore_path)
                        {
                            let _ = writeln!(f, "\n# worktree\n{}", gitignore_entry);
                        }
                    }
                }
                let key = format!("worktree-add:{}", name);
                let job_name = name.clone();
                let branch_clone = branch.clone();
                self.spawn("worktree-add", key, move || {
                    let result = if branch_clone.trim().is_empty() {
                        lazecode_git::add_worktree_detached_at_head(&project_root, &destination)
                            .map_err(|err| err.to_string())
                    } else {
                        let b = branch_clone.trim();
                        let _ = lazecode_git::create_branch(&project_root, b, false);
                        lazecode_git::add_worktree(&project_root, &destination, b)
                            .map_err(|err| err.to_string())
                    };
                    JobResult::WorktreeAdded {
                        project_root,
                        name: job_name,
                        result,
                    }
                });
            }
            Command::SwitchWorktree(name) => {
                if let Some(project) = model.active_project_mut() {
                    match project.ensure_worktree(&name) {
                        Ok(Some(())) => {
                            // New worktree attached; snapshot comes via the job below.
                        }
                        Ok(None) => {}
                        Err(err) => {
                            model.toast(err, ToastKind::Error);
                        }
                    }
                }
            }
            Command::RemoveWorktree { name, force } => {
                let Some(project) = model.active_project() else {
                    return;
                };
                let project_root = project.root.clone();
                let worktree_path = project
                    .worktrees
                    .iter()
                    .find(|worktree| worktree.name == name)
                    .map(|worktree| worktree.path.clone());
                let Some(worktree_path) = worktree_path else {
                    return;
                };
                if project
                    .worktrees
                    .iter()
                    .find(|worktree| worktree.name == name)
                    .is_some_and(|worktree| worktree.is_main)
                {
                    model.toast(
                        if model.settings.language.is_zh() {
                            "主工作树不能被删除"
                        } else {
                            "The main worktree cannot be removed"
                        },
                        ToastKind::Error,
                    );
                    return;
                }
                // Drop consoles of the worktree being removed.
                let terminal_ids: Vec<u64> = project
                    .worktrees
                    .iter()
                    .find(|worktree| worktree.name == name)
                    .map(|worktree| {
                        worktree
                            .terminals
                            .iter()
                            .flat_map(|tab| tab.panes.iter())
                            .map(|pane| pane.id)
                            .collect()
                    })
                    .unwrap_or_default();
                if let Some(project) = model.active_project_mut() {
                    for id in terminal_ids {
                        project.close_terminal(id);
                        runtime.terminal_snapshots.remove(&id);
                        runtime.terminal_selections.remove(&id);
                        runtime.terminal_search.remove(&id);
                        if runtime.terminal_focus == Some(id) {
                            runtime.terminal_focus = None;
                        }
                    }
                }
                let key = format!("worktree-remove:{}", name);
                let job_name = name.clone();
                self.spawn("worktree-remove", key, move || {
                    let result =
                        lazecode_git::remove_worktree(&project_root, &worktree_path, force)
                            .map_err(|err| err.to_string());
                    JobResult::WorktreeRemoved {
                        project_root,
                        name: job_name,
                        result,
                    }
                });
            }
            Command::SaveWorkspace => model.save_workspace(),
            Command::Quit => model.should_quit = true,
        }
    }

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
            JobResult::CmpSessionLoaded { root, result } => {
                if let Some(project) = model
                    .projects
                    .iter_mut()
                    .find(|project| project.root == root)
                {
                    if let Some(session) = project.compare_session.as_mut() {
                        match result {
                            Ok(files) => {
                                session.nav_order = lazecode_core::model::compare_nav_order(&files);
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
            JobResult::GitRefreshed { root, result } => {
                if let Some(project) = model
                    .projects
                    .iter_mut()
                    .find(|project| project.root == root)
                {
                    match result {
                        Ok(snapshot) => {
                            project.git = Some(snapshot);
                            project.git_error = None;
                            project.prune_unchecked_public();
                            // Hide "checked" state for files that vanished.
                            let existing: HashSet<String> = project
                                .changes()
                                .iter()
                                .map(|change| change.path.clone())
                                .collect();
                            project.checked_paths.retain(|path| existing.contains(path));
                        }
                        Err(err) => project.git_error = Some(err),
                    }
                    project.git_loading = false;
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
                let Some(project) = model
                    .projects
                    .iter_mut()
                    .find(|project| project.root == root)
                else {
                    return;
                };
                // Stale completion (filters changed while the page was in
                // flight) is dropped; the fresh query already re-spawned.
                let stale = log_query_fingerprint(&project.log_query.clone())
                    != log_query_fingerprint(&query);
                if stale {
                    return;
                }
                match result {
                    Ok((mut commits, lanes)) => {
                        project.log_error = None;
                        if offset == 0 {
                            project.log_reached_end = commits.len() < LOG_PAGE_SIZE;
                            project.log = commits;
                        } else {
                            // Dedupe the page against everything already shown:
                            // the options always re-walk from the tips, so the
                            // page's commits repeat the already-loaded prefix.
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
                        project.log_loaded = project.log.len();
                        project.log_paging = false;
                        if !project
                            .log
                            .iter()
                            .any(|commit| Some(&commit.id) == project.selected_commit.as_ref())
                        {
                            project.selected_commit =
                                project.log.first().map(|commit| commit.id.clone());
                            project.commit_details = None;
                            project.commit_details_attempted = None;
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
            }
            JobResult::BranchesLoaded { root, result } => {
                if let Some(project) = model
                    .projects
                    .iter_mut()
                    .find(|project| project.root == root)
                {
                    project.branches = result;
                }
            }
            JobResult::CommitDetails { root, id, result } => {
                if let Some(project) = model
                    .projects
                    .iter_mut()
                    .find(|project| project.root == root)
                {
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
                                error = Some(err);
                            }
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
                if let Some(project) = model
                    .projects
                    .iter_mut()
                    .find(|project| project.root == root)
                {
                    project.set_diff_for_source(&source, result);
                }
            }
            JobResult::SearchDone {
                root,
                request_id,
                query,
                result,
            } => {
                if runtime.search.request_id == request_id
                    && runtime.search.query == query
                    && model
                        .active_project()
                        .is_some_and(|project| project.root == root)
                {
                    match result {
                        Ok(hits) => {
                            runtime.search.results = hits;
                            runtime.search.searched = true;
                            runtime.search.selected = 0;
                        }
                        Err(err) => {
                            runtime.search.results.clear();
                            runtime.search.searched = true;
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
                    if let Some(project) = model
                        .projects
                        .iter_mut()
                        .find(|project| project.root == root)
                    {
                        // 关闭已回滚文件（尤其是新增文件，磁盘上已删除）
                        // 的标签页：无论是否 dirty——文件已不存在，保留
                        // 标签只会让用户对着幽灵缓冲编辑。
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
                    let log_query = model
                        .projects
                        .iter()
                        .find(|project| project.root == root)
                        .map(|project| project.log_query.clone())
                        .unwrap_or_default();
                    self.refresh_git(root.clone(), None);
                    self.refresh_log(root.clone(), log_query, 0);
                    self.refresh_branches(root.clone());
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
                model.toast(
                    message,
                    if ok {
                        ToastKind::Success
                    } else {
                        ToastKind::Error
                    },
                );
                if refresh && !root.as_os_str().is_empty() {
                    let log_query = model
                        .projects
                        .iter()
                        .find(|project| project.root == root)
                        .map(|project| project.log_query.clone())
                        .expect("project exists");
                    self.refresh_git(root.clone(), None);
                    self.refresh_log(root.clone(), log_query, 0);
                    self.refresh_branches(root.clone());
                    // Sequencer/conflict state follows every git op.
                    if let Some(project) = model
                        .projects
                        .iter_mut()
                        .find(|project| project.root == root)
                    {
                        project.refresh_conflicts();
                    }
                    if let Some(project) = model
                        .projects
                        .iter_mut()
                        .find(|project| project.root == root)
                    {
                        project.commit_details = None;
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
                    }
                }
            }
            JobResult::Saved {
                root,
                path,
                revision,
                result,
            } => {
                if let Some(project) = model
                    .projects
                    .iter_mut()
                    .find(|project| project.root == root)
                {
                    if let Some(tab) = project.tabs.iter_mut().find(
                        |tab| matches!(&tab.kind, TabKind::File(file) if file.doc.path == path),
                    ) {
                        if let TabKind::File(file) = &mut tab.kind {
                            if let Err(err) = result {
                                model.toast(
                                    if model.settings.language.is_zh() {
                                        format!("保存失败：{err}")
                                    } else {
                                        format!("Save failed: {err}")
                                    },
                                    ToastKind::Error,
                                );
                            } else if file.doc.revision == revision {
                                file.doc.dirty = false;
                            }
                        }
                    }
                }
            }
            JobResult::WorktreeAdded {
                project_root,
                name,
                result,
            } => match result {
                Ok(path) => {
                    if let Some(project) = model
                        .projects
                        .iter_mut()
                        .find(|project| project.root == project_root)
                    {
                        project.attach_worktree(WorktreeState::load(
                            name.clone(),
                            path.clone(),
                            false,
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
                if let Some(project) = model
                    .projects
                    .iter_mut()
                    .find(|project| project.root == project_root)
                {
                    match result {
                        Ok(()) => {
                            project.detach_worktree(&name);
                            model.ui.sidebar_view = lazecode_core::SidebarView::Consoles;
                        }
                        Err(err) => {
                            model.toast(err, ToastKind::Error);
                        }
                    }
                }
            }
        }
    }
}

struct SaveData {
    text: String,
    encoding: Option<&'static encoding_rs::Encoding>,
    eol: lazecode_fs::Eol,
    bom: bool,
}

/// 恢复 shelf（整只或部分恢复）并把恢复出的文件归入目标变更列表。
/// `paths=Some` 只应用这些文件（部分恢复）；`target_list` 不存在时创建；
/// `delete_after` 在成功后删除 shelf 条目。
fn shelf_restore_scoped(
    root: &std::path::Path,
    name: &str,
    paths: Option<&[String]>,
    target_list: Option<&str>,
    delete_after: bool,
) -> anyhow::Result<()> {
    match paths {
        Some(selected) => lazecode_git::shelf_restore_partial(root, name, selected)?,
        None => lazecode_git::shelf_restore(root, name, false)?,
    }
    if let Some(list) = target_list {
        let mut lists = lazecode_git::Changelists::open(root);
        if !lists.lists().iter().any(|existing| existing == list) {
            lists.create_list(list)?;
        }
        let assigned = match paths {
            Some(selected) => selected.to_vec(),
            None => lazecode_git::shelf_files(root, name)?,
        };
        lists.assign_many(&assigned, list)?;
    }
    if delete_after {
        lazecode_git::shelf_delete(root, name)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazecode_core::UiState;

    struct NoopNotifier;

    impl Notifier for NoopNotifier {
        fn notify(&self) {}
    }

    fn empty_model() -> AppModel {
        AppModel {
            settings: Settings::default(),
            projects: Vec::new(),
            active_project: None,
            ui: UiState::default(),
            toasts: Vec::new(),
            recent: Vec::new(),
            commands: Vec::new(),
            notifier: None,
            lsp: lazecode_core::LspManager::new(true, None),
            should_quit: false,
            version: "test",
            favorite_branches: HashSet::new(),
            git_cmd_log: Vec::new(),
        }
    }

    fn run_and_apply(queue: &mut JobQueue, completion: JobCompletion) {
        let mut model = empty_model();
        let mut runtime = UiRuntime::default();
        queue.apply(&mut model, &mut runtime, completion);
    }

    #[test]
    fn completed_save_and_op_jobs_release_their_keys() {
        let mut queue = JobQueue::new(Arc::new(NoopNotifier));

        queue.spawn("test-save", "save:/tmp/a".into(), || JobResult::Saved {
            root: PathBuf::new(),
            path: PathBuf::from("/tmp/a"),
            revision: 0,
            result: Ok(()),
        });
        let completion = queue
            .rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        run_and_apply(&mut queue, completion);
        assert!(!queue.in_flight.contains("save:/tmp/a"));

        queue.spawn("test-op", "commit:/repo".into(), || JobResult::OpDone {
            root: PathBuf::new(),
            message: "ok".into(),
            ok: true,
            refresh: false,
            reload_files: false,
            clear_commit_message: false,
        });
        let completion = queue
            .rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        run_and_apply(&mut queue, completion);
        assert!(!queue.in_flight.contains("commit:/repo"));
    }

    #[test]
    fn panicking_job_releases_its_key() {
        let mut queue = JobQueue::new(Arc::new(NoopNotifier));
        queue.spawn("test-panic", "git:/repo".into(), || panic!("boom"));
        let completion = queue
            .rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        run_and_apply(&mut queue, completion);
        assert!(!queue.in_flight.contains("git:/repo"));
    }
    #[test]
    fn refresh_log_spawns_and_completes_job() {
        let mut queue = JobQueue::new(Arc::new(NoopNotifier));
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        queue.refresh_log(root.clone(), lazecode_core::LogQuery::default(), 0);
        let completion = queue
            .rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("refresh_log should produce completion");
        let key = completion.key.clone();
        let mut model = empty_model();
        let mut project = ProjectState::open(root, &Settings::default(), None).unwrap();
        project.log_paging = true;
        model.projects.push(project);
        model.active_project = Some(0);
        let mut runtime = UiRuntime::default();
        queue.apply(&mut model, &mut runtime, completion);
        assert!(!queue.in_flight.contains(&key));
        assert!(!model.projects[0].log.is_empty());
        assert!(!model.projects[0].log_paging);
    }
}
