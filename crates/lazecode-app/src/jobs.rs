use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};
use lazecode_core::{AppModel, Command, ProjectState, Settings, TabKind, ToastKind, WorktreeState};
use lazecode_fs::{SearchHit, SearchOptions};
use lazecode_git::{BranchInfo, CommitDetails, CommitInfo, FileDiff, LogOptions, RepoSnapshot};
use lazecode_terminal::Notifier;
use lazecode_ui::UiRuntime;

pub enum JobResult {
    ProjectLoaded {
        path: PathBuf,
        result: Result<Box<ProjectState>, String>,
    },
    GitRefreshed {
        root: PathBuf,
        result: Result<RepoSnapshot, String>,
    },
    LogLoaded {
        root: PathBuf,
        branch: Option<String>,
        result: Result<Vec<CommitInfo>, String>,
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
        path: String,
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
    pending_log: HashSet<(PathBuf, Option<String>)>,
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
            pending_log: HashSet::new(),
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

    pub fn refresh_log(&mut self, root: PathBuf, branch: Option<String>) {
        let key = format!("log:{}:{}", root.display(), branch.as_deref().unwrap_or(""));
        if self.in_flight.contains(&key) {
            self.pending_log.insert((root, branch));
            return;
        }
        let job_root = root.clone();
        let job_branch = branch.clone();
        self.spawn("log", key, move || {
            let options = LogOptions {
                limit: 300,
                first_parent: false,
            };
            let result = match &job_branch {
                Some(name) => lazecode_git::log_ref(&job_root, name, options),
                None => lazecode_git::log(&job_root, options),
            }
            .map_err(|err| err.to_string());
            JobResult::LogLoaded {
                root: job_root,
                branch: job_branch,
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

    pub fn file_diff(&mut self, root: PathBuf, path: String) {
        let key = format!("diff:{}:{path}", root.display());
        let job_root = root.clone();
        let job_path = path.clone();
        self.spawn("diff", key, move || {
            let result =
                lazecode_git::file_diff(&job_root, &job_path).map_err(|err| err.to_string());
            JobResult::DiffLoaded {
                root: job_root,
                path: job_path,
                result,
            }
        });
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
                if let Some(project) = model.active_project_mut() {
                    project.tree = lazecode_fs::tree::load_tree(&project.root);
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
            Command::RefreshLog => {
                if let Some(project) = model.active_project() {
                    let root = project.root.clone();
                    let branch = project.log_branch.clone();
                    self.refresh_log(root.clone(), branch);
                    self.refresh_branches(root);
                }
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
                    project.open_diff(path.clone());
                    let root = project.root.clone();
                    self.file_diff(root, path);
                }
            }
            Command::Search { query, request_id } => {
                if let Some(project) = model.active_project() {
                    let root = project.root.clone();
                    let key = format!("search:{}:{request_id}", root.display());
                    let job_root = root.clone();
                    let job_query = query.clone();
                    self.spawn("search", key, move || {
                        let result = lazecode_fs::search_in_files(
                            &job_root,
                            &job_query,
                            &SearchOptions {
                                max_results: 500,
                                ..Default::default()
                            },
                            None,
                        )
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
                    if let Err(err) = project.open_terminal(
                        None,
                        None,
                        lazecode_terminal::SessionKind::Shell,
                        &settings,
                        notifier,
                    ) {
                        model.toast(err, ToastKind::Error);
                    }
                    model.ui.panel_visible = true;
                    model.ui.panel_tab = lazecode_core::PanelTab::Console;
                }
            }
            Command::NewTerminalCommand { title, command } => {
                let settings = model.settings.clone();
                let notifier = model.notifier.clone();
                if let Some(project) = model.active_project_mut() {
                    if let Err(err) = project.open_terminal(
                        Some(command),
                        Some(title),
                        lazecode_terminal::SessionKind::Agent,
                        &settings,
                        notifier,
                    ) {
                        model.toast(err, ToastKind::Error);
                    }
                    model.ui.panel_visible = true;
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
                runtime.terminal_snapshots.remove(&id);
                runtime.terminal_selections.remove(&id);
                runtime.terminal_search.remove(&id);
            }
            Command::CreateWorktree { name } => {
                let Some(project) = model.active_project() else {
                    return;
                };
                let project_root = project.root.clone();
                let destination = project_root.parent().unwrap_or(&project_root).join(&name);
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
                let key = format!("worktree-add:{}", name);
                let job_name = name.clone();
                self.spawn("worktree-add", key, move || {
                    let result =
                        lazecode_git::add_worktree_detached_at_head(&project_root, &destination)
                            .map_err(|err| err.to_string());
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
                    .map(|worktree| worktree.terminals.iter().map(|t| t.id).collect())
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
                            project
                                .unchecked_paths
                                .retain(|path| existing.contains(path));
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
                result,
            } => {
                if let Some(project) = model
                    .projects
                    .iter_mut()
                    .find(|project| project.root == root)
                {
                    if project.log_branch == branch {
                        match result {
                            Ok(log) => {
                                project.log = log;
                                project.log_error = None;
                            }
                            Err(err) => project.log_error = Some(err),
                        }
                        project.log_loading = false;
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
                }
                if self.pending_log.remove(&(root.clone(), branch.clone())) {
                    self.refresh_log(root, branch);
                }
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
            JobResult::DiffLoaded { root, path, result } => {
                if let Some(project) = model
                    .projects
                    .iter_mut()
                    .find(|project| project.root == root)
                {
                    project.set_diff(&path, result);
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
                    let log_branch = model
                        .projects
                        .iter()
                        .find(|project| project.root == root)
                        .and_then(|project| project.log_branch.clone());
                    self.refresh_git(root.clone(), None);
                    self.refresh_log(root.clone(), log_branch);
                    self.refresh_branches(root.clone());
                    if let Some(project) = model
                        .projects
                        .iter_mut()
                        .find(|project| project.root == root)
                    {
                        project.commit_details = None;
                        if clear_commit_message {
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
                        self.refresh_git(path.clone(), None);
                        self.refresh_log(path.clone(), None);
                        self.refresh_branches(project_root.clone());
                    }
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
            should_quit: false,
            version: "test",
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
}
