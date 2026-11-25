//! Command dispatch: one arm per `Command`, each spawning the matching
//! background job.

use super::*;

impl JobQueue {
    pub fn execute(&mut self, model: &mut AppModel, runtime: &mut UiRuntime, command: Command) {
        if model
            .active_project()
            .is_some_and(|project| project.is_single_file)
            && matches!(
                &command,
                Command::Search { .. } | Command::ReplaceInFiles { .. }
            )
        {
            model.toast(
                if model.settings.language.is_zh() {
                    "单文件模式没有项目根目录；请从文件所在项目中使用全局搜索"
                } else {
                    "Single files has no project root; open a folder to search across files"
                },
                ToastKind::Warning,
            );
            return;
        }
        match command {
            Command::OpenProjectDialog => {
                let mut directories = Vec::new();
                let mut files = Vec::new();
                for path in pick_open_paths() {
                    if path.is_dir() {
                        directories.push(path);
                    } else if path.is_file() {
                        files.push(path);
                    }
                }
                for directory in directories {
                    self.load_project(directory, model.settings.clone(), model.notifier.clone());
                }
                if !files.is_empty() {
                    match model.open_files(files) {
                        Ok(_) => runtime.clear_terminal_cache(),
                        Err(err) => model.toast(err, ToastKind::Error),
                    }
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
                    .map(|project| project.active_root().to_path_buf())
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
                if path.is_file() {
                    match model.open_files(vec![path]) {
                        Ok(_) => runtime.clear_terminal_cache(),
                        Err(err) => model.toast(err, ToastKind::Error),
                    }
                } else {
                    self.load_project(path, model.settings.clone(), model.notifier.clone());
                }
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
                    // Busy state is probed off-thread; the project closes (or a
                    // confirmation opens) on a later frame.
                    runtime.request_terminal_close(
                        model,
                        lazecode_ui::runtime::TerminalCloseAction::Project(index),
                    );
                }
            }
            Command::ReloadGit => {
                if let Some(project) = model.active_project() {
                    let root = project.active_root().to_path_buf();
                    self.refresh_git(root, model.notifier.clone());
                }
            }
            Command::ReloadTree => {
                let settings = model.settings.clone();
                if let Some(project) = model.active_project_mut() {
                    if !project.is_single_file {
                        // Watchers cache the ignore list; a settings edit must
                        // rebuild them before the tree scan starts.
                        project.rebuild_watchers(&settings);
                        let root = project.active_root().to_path_buf();
                        self.refresh_tree(root, Arc::new(settings.ignored_patterns.clone()));
                    }
                }
            }
            Command::Stage(paths) => {
                if let Some(project) = model.active_project() {
                    let root = project.active_root().to_path_buf();
                    let zh = model.settings.language.is_zh();
                    let key = format!("stage:{}:{}", root.display(), paths.join("\u{1}"));
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
                    let root = project.active_root().to_path_buf();
                    let zh = model.settings.language.is_zh();
                    let key = format!("unstage:{}:{}", root.display(), paths.join("\u{1}"));
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
                let root = project.active_root().to_path_buf();
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
                let root = project.active_root().to_path_buf();
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                let root = project.active_root().to_path_buf();
                let zh = model.settings.language.is_zh();
                let key = format!("checkout:{}:{name}", root.display());
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
            Command::CreateBranch {
                name,
                start_point,
                checkout,
            } => {
                let Some(project) = model.active_project() else {
                    return;
                };
                let root = project.active_root().to_path_buf();
                let zh = model.settings.language.is_zh();
                let key = format!(
                    "branch:{}:{name}:{}:{checkout}",
                    root.display(),
                    start_point.as_deref().unwrap_or("")
                );
                let job_root = root.clone();
                self.spawn("branch", key, move || {
                    let result = lazecode_git::create_branch_from(
                        &job_root,
                        &name,
                        start_point.as_deref(),
                        checkout,
                    );
                    match result {
                        Ok(()) => JobResult::OpDone {
                            root: job_root,
                            message: if zh {
                                if let Some(start) = start_point.as_deref() {
                                    format!("已从 {start} 创建分支 {name}")
                                } else {
                                    format!("已创建分支 {name}")
                                }
                            } else if let Some(start) = start_point.as_deref() {
                                format!("Created branch {name} from {start}")
                            } else {
                                format!("Created branch {name}")
                            },
                            ok: true,
                            refresh: true,
                            reload_files: checkout,
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
            Command::DeleteBranch { name, force } => {
                let Some(project) = model.active_project() else {
                    return;
                };
                let root = project.active_root().to_path_buf();
                let zh = model.settings.language.is_zh();
                let key = format!("delete-branch:{}:{name}", root.display());
                let job_root = root.clone();
                self.spawn("delete-branch", key, move || {
                    let result = if force {
                        lazecode_git::force_delete_branch(&job_root, &name)
                    } else {
                        lazecode_git::delete_branch(&job_root, &name)
                    };
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
                        Err(err) if !force && err.to_string().contains("not fully merged") => {
                            JobResult::BranchDeleteNeedsForce {
                                root: job_root,
                                name,
                            }
                        }
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
            Command::DeleteRemoteBranch(name) => {
                let Some(project) = model.active_project() else {
                    return;
                };
                let root = project.active_root().to_path_buf();
                let key = format!("delete-remote-branch:{}:{name}", root.display());
                let job_root = root.clone();
                self.spawn("delete-remote-branch", key, move || {
                    let result = lazecode_git::delete_remote_branch(&job_root, &name);
                    JobResult::OpDone {
                        root: job_root,
                        message: match &result {
                            Ok(message) => message.clone(),
                            Err(err) => err.to_string(),
                        },
                        ok: result.is_ok(),
                        refresh: result.is_ok(),
                        reload_files: false,
                        clear_commit_message: false,
                    }
                });
            }
            Command::RenameBranch { from, to } => {
                let Some(project) = model.active_project() else {
                    return;
                };
                let root = project.active_root().to_path_buf();
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
                let root = project.active_root().to_path_buf();
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
                let root = project.active_root().to_path_buf();
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
                let Some(project) = model.active_project() else {
                    model.toast(
                        if model.settings.language.is_zh() {
                            "没有活动项目"
                        } else {
                            "No active project"
                        },
                        ToastKind::Warning,
                    );
                    return;
                };
                let root = project.active_root().to_path_buf();
                let zh = model.settings.language.is_zh();
                self.spawn("fetch-all", format!("fetch-all:{root:?}"), move || {
                    let result = lazecode_git::fetch_all(&root);
                    JobResult::OpDone {
                        root: root.clone(),
                        message: match &result {
                            Ok(_) => {
                                if zh {
                                    "已获取全部远程更新".to_string()
                                } else {
                                    "Fetched all remotes".to_string()
                                }
                            }
                            Err(err) => err.to_string(),
                        },
                        ok: result.is_ok(),
                        refresh: result.is_ok(),
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
                push_tags,
            } => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.active_root().to_path_buf())
                    .unwrap_or_default();
                self.spawn(
                    "push",
                    format!("push:{root:?}:{remote}:{branch}:{set_upstream}:{force}:{push_tags}"),
                    move || {
                        let branch_result =
                            lazecode_git::push(&root, &remote, &branch, set_upstream, force);
                        let tags_result = if branch_result.is_ok() && push_tags {
                            Some(lazecode_git::push_tags(&root, &remote))
                        } else {
                            None
                        };
                        let result = match (branch_result, tags_result) {
                            (Ok(_text), Some(Err(err))) => {
                                Err(anyhow::anyhow!("branch pushed, but tags failed: {err}"))
                            }
                            (Ok(text), _) => Ok(text),
                            (Err(err), _) => Err(err),
                        };
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
                                // A branch push may have succeeded before tag
                                // pushing failed; refresh anyway.
                                refresh: true,
                                reload_files: false,
                                clear_commit_message: false,
                            },
                        }
                    },
                );
            }
            Command::PushTag { remote, tag } => {
                let root = model
                    .active_project()
                    .map(|project| project.active_root().to_path_buf())
                    .unwrap_or_default();
                self.spawn(
                    "push-tag",
                    format!("push-tag:{root:?}:{remote}:{tag}"),
                    move || {
                        let result = lazecode_git::push_tag(&root, &remote, &tag);
                        JobResult::OpDone {
                            root: root.clone(),
                            message: match &result {
                                Ok(message) => message.clone(),
                                Err(err) => err.to_string(),
                            },
                            ok: result.is_ok(),
                            refresh: result.is_ok(),
                            reload_files: false,
                            clear_commit_message: false,
                        }
                    },
                );
            }
            Command::Pull { mode } => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
                    .unwrap_or_default();
                self.spawn(
                    "remote-rename",
                    format!("remote-rn:{root:?}:{from}:{to}"),
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
                    .map(|project| project.active_root().to_path_buf())
                    .unwrap_or_default();
                self.spawn(
                    "remote-url",
                    format!("remote-url:{root:?}:{name}:{url}"),
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
            Command::RemoteEdit { from, name, url } => {
                let root = model
                    .active_project()
                    .map(|project| project.active_root().to_path_buf())
                    .unwrap_or_default();
                self.spawn(
                    "remote-edit",
                    format!("remote-edit:{root:?}:{from}:{name}"),
                    move || {
                        let result = (|| -> anyhow::Result<()> {
                            if from != name {
                                lazecode_git::remote_rename(&root, &from, &name)?;
                            }
                            lazecode_git::remote_set_url(&root, &name, &url)?;
                            Ok(())
                        })();
                        JobResult::OpDone {
                            root,
                            message: match &result {
                                Ok(()) => format!("Updated remote {name}"),
                                Err(err) => err.to_string(),
                            },
                            ok: result.is_ok(),
                            refresh: result.is_ok(),
                            reload_files: false,
                            clear_commit_message: false,
                        }
                    },
                );
            }
            Command::Merge { ref_name } => {
                let root = model
                    .active_project()
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
            Command::RevertContinue => {
                let root = model
                    .active_project()
                    .map(|project| project.active_root().to_path_buf())
                    .unwrap_or_default();
                self.spawn("revert-continue", format!("rvc:{root:?}"), move || {
                    let result = lazecode_git::revert_continue(&root);
                    JobResult::OpDone {
                        root,
                        message: match &result {
                            Ok(_) => "revert continuing".to_string(),
                            Err(err) => err.to_string(),
                        },
                        ok: result.is_ok(),
                        refresh: true,
                        reload_files: true,
                        clear_commit_message: false,
                    }
                });
            }
            Command::RevertAbort => {
                let root = model
                    .active_project()
                    .map(|project| project.active_root().to_path_buf())
                    .unwrap_or_default();
                self.spawn("revert-abort", format!("rva:{root:?}"), move || {
                    let result = lazecode_git::revert_abort(&root);
                    JobResult::OpDone {
                        root,
                        message: match &result {
                            Ok(_) => "revert aborted".to_string(),
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
                    .unwrap_or_default();
                let hashes = hashes.clone();
                let message = message.clone();
                self.spawn(
                    "squash",
                    format!("sq:{root:?}:{}", payload_hash(&hashes)),
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
                    .unwrap_or_default();
                let paths = paths.clone();
                self.spawn("discard", format!("disc:{root:?}:{paths:?}"), move || {
                    // Roll back per file; newly added files are gone, so their
                    // tabs must be closed too.
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
                    .map(|project| project.active_root().to_path_buf())
                    .unwrap_or_default();
                let revision = revision.clone();
                self.spawn(
                    "checkout_rev",
                    format!("corev:{root:?}:{revision}"),
                    move || {
                        let result = lazecode_git::checkout_ref(&root, &revision);
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                match model.open_file_in(index, path.clone()) {
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
            Command::StashPop(stash) => {
                let root = model
                    .active_project()
                    .map(|project| project.active_root().to_path_buf())
                    .unwrap_or_default();
                let stash = stash.clone();
                self.spawn(
                    "stash-pop",
                    format!("stash-p:{root:?}:{stash}"),
                    move || {
                        let result = lazecode_git::stash_pop(&root, &stash);
                        JobResult::OpDone {
                            root,
                            message: match &result {
                                Ok(_) => format!("已弹出 {stash}"),
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
            Command::StashClear => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.active_root().to_path_buf())
                    .unwrap_or_default();
                self.spawn("stash-clear", format!("stash-c:{root:?}"), move || {
                    // 丢弃后索引前移；先查列表，空列表是成功终点，而不是
                    // 把 stash_drop 的「no entries」错误当成失败。
                    let mut dropped = 0usize;
                    let mut error = None;
                    loop {
                        match lazecode_git::stash_list(&root) {
                            Ok(list) if list.is_empty() => break,
                            Ok(_) => {}
                            Err(err) => {
                                error = Some(err.to_string());
                                break;
                            }
                        }
                        match lazecode_git::stash_drop(&root, "stash@{0}") {
                            Ok(_) => dropped += 1,
                            Err(err) => {
                                error = Some(err.to_string());
                                break;
                            }
                        }
                    }
                    let ok = error.is_none();
                    JobResult::OpDone {
                        root,
                        message: match error {
                            Some(err) => err,
                            None if zh => format!("已清空 {dropped} 条贮藏"),
                            None => format!("Cleared {dropped} stash entr(y/ies)"),
                        },
                        ok,
                        refresh: true,
                        reload_files: false,
                        clear_commit_message: false,
                    }
                });
            }
            Command::ShelfSave { name, paths } => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.active_root().to_path_buf())
                    .unwrap_or_default();
                let name = name.clone();
                self.spawn(
                    "shelf-save",
                    format!("shelf:{root:?}:{name}:{}", payload_hash(&paths)),
                    move || {
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
                    },
                );
            }
            Command::ShelfDelete(name) => {
                let zh = model.settings.language.is_zh();
                let root = model
                    .active_project()
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
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
                    .map(|project| project.active_root().to_path_buf())
                    .unwrap_or_default();
                let name = name.clone();
                let d_after = delete_after;
                self.spawn(
                    "shelf-restore",
                    format!(
                        "shelf-r:{root:?}:{name}:{}:{}:{d_after}",
                        payload_hash(&paths),
                        payload_hash(&target_list)
                    ),
                    move || {
                        // `paths` restores only selected files; `target_list`
                        // assigns them to that changelist (created if absent).
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
                    format!(
                        "apply:{}:{}",
                        root.display(),
                        payload_hash(&(&patch, cached, reverse))
                    ),
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
                let root = project.active_root().to_path_buf();
                model.ui.sidebar_view = lazecode_core::model::SidebarView::Compare;
                model.ui.sidebar_visible = true;
                self.spawn(
                    "compare-files",
                    format!("cmp:{root:?}:{from}:{to}"),
                    move || {
                        let result = lazecode_git::compare_commits(&root, &from, &to)
                            .map_err(|err| err.to_string());
                        JobResult::CmpSessionLoaded {
                            root,
                            from,
                            to,
                            local: false,
                            result,
                        }
                    },
                );
            }
            Command::OpenCompareWithLocal { commit } => {
                let Some(project) = model.active_project_mut() else {
                    return;
                };
                project.compare_session = Some(lazecode_core::model::CompareSession {
                    from: commit.clone(),
                    to: String::new(),
                    local: true,
                    loading: true,
                    group_by_dir: true,
                    root_expanded: true,
                    ..Default::default()
                });
                let root = project.active_root().to_path_buf();
                model.ui.sidebar_view = lazecode_core::model::SidebarView::Compare;
                model.ui.sidebar_visible = true;
                self.load_compare_local(root, commit);
            }
            Command::CompareSwap => {
                let Some(project) = model.active_project_mut() else {
                    return;
                };
                let Some(session) = project.compare_session.as_mut() else {
                    return;
                };
                if session.local {
                    return;
                }
                let old_from = session.from.clone();
                let old_to = session.to.clone();
                std::mem::swap(&mut session.from, &mut session.to);
                session.selected = None;
                session.loading = true;
                let (from, to) = (session.from.clone(), session.to.clone());
                let root = project.active_root().to_path_buf();
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
                        JobResult::CmpSessionLoaded {
                            root,
                            from,
                            to,
                            local: false,
                            result,
                        }
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
                    let root = project.active_root().to_path_buf();
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
                let root = project.active_root().to_path_buf();
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
                        let root = project.active_root().to_path_buf();
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
                    let root = project.active_root().to_path_buf();
                    self.diff_source(root, source);
                }
            }
            Command::OpenDiffNewTab(path) => {
                if let Some(project) = model.active_project_mut() {
                    project.change_selection.clear();
                    project.change_selection.insert(path.clone());
                    project.change_anchor = Some(path.clone());
                    let source = DiffSource::Worktree { path };
                    project.open_diff_source_new_tab(source.clone());
                    let root = project.active_root().to_path_buf();
                    self.diff_source(root, source);
                }
            }
            Command::OpenIndexDiff(path) => {
                if let Some(project) = model.active_project_mut() {
                    project.change_selection.clear();
                    project.change_selection.insert(path.clone());
                    project.change_anchor = Some(path.clone());
                    let source = DiffSource::Index { path };
                    project.open_diff_source(source.clone());
                    let root = project.active_root().to_path_buf();
                    self.diff_source(root, source);
                }
            }
            Command::GetFileFromRevision { path, revision } => {
                let root = model
                    .active_project()
                    .map(|project| project.active_root().to_path_buf())
                    .unwrap_or_default();
                let zh = model.settings.language.is_zh();
                self.spawn(
                    "get-from-revision",
                    format!("get-rev:{root:?}:{revision}:{path}"),
                    move || {
                        let result = lazecode_git::checkout_revision_path(&root, &revision, &path);
                        JobResult::OpDone {
                            root,
                            message: match &result {
                                Ok(()) => {
                                    if zh {
                                        format!("已从 {revision} 恢复 {path}")
                                    } else {
                                        format!("Restored {path} from {revision}")
                                    }
                                }
                                Err(err) => err.to_string(),
                            },
                            ok: result.is_ok(),
                            // A failed checkout can still have touched paths
                            // (for example, an untracked-file collision after
                            // earlier writes). Refresh even on error.
                            refresh: true,
                            reload_files: true,
                            clear_commit_message: false,
                        }
                    },
                );
            }
            Command::CherryPickChanges { paths, target } => {
                let root = model
                    .active_project()
                    .map(|project| project.active_root().to_path_buf())
                    .unwrap_or_default();
                let zh = model.settings.language.is_zh();
                let key_paths = paths.clone();
                self.spawn(
                    "cherry-pick-changes",
                    format!(
                        "cherry-changes:{root:?}:{target}:{}",
                        payload_hash(&key_paths)
                    ),
                    move || {
                        let result = cherry_pick_selected_changes(&root, &key_paths, &target);
                        JobResult::OpDone {
                            root,
                            message: match &result {
                                Ok(_) if zh => {
                                    format!("已将选中更改移动到 {target}（未提交）")
                                }
                                Ok(message) => message.clone(),
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
            Command::OpenCommitDiff { commit, path } => {
                if let Some(project) = model.active_project_mut() {
                    let source = DiffSource::Commit { commit, path };
                    project.open_diff_source(source.clone());
                    let root = project.active_root().to_path_buf();
                    self.diff_source(root, source);
                }
            }
            Command::OpenStashDiff { stash, path } => {
                if let Some(project) = model.active_project_mut() {
                    let source = DiffSource::Stash {
                        stash: stash.clone(),
                        path: path.clone(),
                    };
                    project.open_diff_source(source.clone());
                    let root = project.active_root().to_path_buf();
                    self.diff_source(root, source);
                }
            }
            Command::OpenStashDiffNewTab { stash, path } => {
                if let Some(project) = model.active_project_mut() {
                    let source = DiffSource::Stash {
                        stash: stash.clone(),
                        path: path.clone(),
                    };
                    project.open_diff_source_new_tab(source.clone());
                    let root = project.active_root().to_path_buf();
                    self.diff_source(root, source);
                }
            }
            Command::OpenStashBeforeLocalDiff { stash, path } => {
                if let Some(project) = model.active_project_mut() {
                    let source = DiffSource::StashBeforeLocal {
                        stash: stash.clone(),
                        path: path.clone(),
                    };
                    project.open_diff_source(source.clone());
                    let root = project.active_root().to_path_buf();
                    self.diff_source(root, source);
                }
            }
            Command::OpenRepositoryVersion { path } => {
                let Some(project) = model.active_project_mut() else {
                    return;
                };
                let root = project.active_root().to_path_buf();
                let relative = path.clone();
                let label = std::path::Path::new(&relative)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| relative.clone());
                let settings = model.settings.clone();
                match lazecode_git::head_file_bytes(&root, &relative) {
                    Ok(bytes) => {
                        if let Some(project) = model.active_project_mut() {
                            let label = format!("{label} (HEAD)");
                            if let Err(err) = project.open_revision_snapshot(
                                &root.join(&relative),
                                &bytes,
                                label,
                                &settings,
                            ) {
                                model.toast(err, ToastKind::Error);
                            }
                        }
                    }
                    Err(err) => model.toast(err.to_string(), ToastKind::Error),
                }
            }
            Command::CherryPickStashPaths { stash, paths } => {
                let root = model
                    .active_project()
                    .map(|project| project.active_root().to_path_buf())
                    .unwrap_or_default();
                let stash = stash.clone();
                let paths = paths.clone();
                self.spawn(
                    "stash-apply-paths",
                    format!("stash-ap:{root:?}:{stash}:{paths:?}"),
                    move || {
                        let result = lazecode_git::stash_apply_paths(&root, &stash, &paths);
                        JobResult::OpDone {
                            root,
                            message: match &result {
                                Ok(_) => {
                                    if paths.len() == 1 {
                                        format!("已拣选 {}", paths[0])
                                    } else {
                                        format!("已拣选 {} 个文件", paths.len())
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
            Command::Unstash {
                stash,
                branch,
                pop,
                reinstate_index,
            } => {
                let root = model
                    .active_project()
                    .map(|project| project.active_root().to_path_buf())
                    .unwrap_or_default();
                let stash = stash.clone();
                let branch = branch.clone();
                self.spawn(
                    "stash-unstash",
                    format!("stash-u:{root:?}:{stash}:{branch:?}:{pop}:{reinstate_index}"),
                    move || {
                        let result = (|| -> Result<String, String> {
                            if let Some(branch) = branch.as_deref() {
                                lazecode_git::create_branch(&root, branch, true)
                                    .map_err(|err| err.to_string())?;
                            }
                            lazecode_git::stash_apply_with(
                                &root,
                                &stash,
                                &lazecode_git::StashApplyOptions {
                                    paths: None,
                                    reinstate_index,
                                },
                            )
                            .map_err(|err| err.to_string())?;
                            if pop {
                                lazecode_git::stash_drop(&root, &stash)
                                    .map_err(|err| err.to_string())?;
                            }
                            Ok(match branch {
                                Some(branch) => format!("已在 {branch} 上恢复 {stash}"),
                                None => format!("已恢复 {stash}"),
                            })
                        })();
                        let ok = result.is_ok();
                        let message = match result {
                            Ok(message) => message,
                            Err(err) => err,
                        };
                        JobResult::OpDone {
                            root,
                            message,
                            ok,
                            refresh: true,
                            reload_files: true,
                            clear_commit_message: false,
                        }
                    },
                );
            }
            Command::OpenCompareWithLocalDiff { commit, path } => {
                if let Some(project) = model.active_project_mut() {
                    let source = DiffSource::CompareWithLocal { commit, path };
                    project.open_diff_source(source.clone());
                    let root = project.active_root().to_path_buf();
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
                    let root = project.active_root().to_path_buf();
                    self.diff_source(root, source);
                }
            }
            Command::OpenFilesDiff { old_path, new_path } => {
                if let Some(project) = model.active_project_mut() {
                    let source = DiffSource::Files { old_path, new_path };
                    project.open_diff_source(source.clone());
                    let root = project.active_root().to_path_buf();
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
                    let root = project.active_root().to_path_buf();
                    // Revoke the previous scan; `search_in_files` polls the flag
                    // between files and unwinds quickly.
                    if let Some((_, stale)) = self.search_cancels.remove(&root) {
                        stale.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    let cancel = Arc::new(AtomicBool::new(false));
                    self.search_cancels
                        .insert(root.clone(), (request_id, cancel.clone()));
                    let key = format!("search:{}:{request_id}", root.display());
                    let job_root = root.clone();
                    let job_query = query.clone();
                    self.spawn("search", key, move || {
                        let result = lazecode_fs::search_in_files(
                            &job_root,
                            &job_query,
                            &options,
                            Some(&cancel),
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
            Command::ReplaceInFiles {
                query,
                replacement,
                options,
            } => {
                let zh = model.settings.language.is_zh();
                if let Some(project) = model.active_project() {
                    let root = project.active_root().to_path_buf();
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
                let root = project.active_root().to_path_buf();
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
                // With nothing open the console belongs to the hidden virtual
                // "Single files" checkout; it must not register HOME.
                let index = model
                    .active_project
                    .unwrap_or_else(|| model.ensure_single_files());
                if let Some(project) = model.projects.get_mut(index) {
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
                }
                model.active_project = Some(index);
                model.ui.terminal_visible = true;
                if model.ui.terminal_placement == lazecode_core::TerminalPlacement::Bottom {
                    model.ui.panel_visible = false;
                }
                model.ui.panel_tab = lazecode_core::PanelTab::Console;
            }
            Command::NewTerminalCommand { title, command } => {
                let settings = model.settings.clone();
                let notifier = model.notifier.clone();
                match model.open_agent_console(title, command, &settings, notifier) {
                    Ok(id) => {
                        runtime.set_terminal_focus(id);
                        model.ui.terminal_visible = true;
                        if model.ui.terminal_placement == lazecode_core::TerminalPlacement::Bottom {
                            model.ui.panel_visible = false;
                        }
                        model.ui.panel_tab = lazecode_core::PanelTab::Console;
                    }
                    Err(err) => {
                        model.toast(err, ToastKind::Warning);
                    }
                }
            }
            Command::CloseTerminal(id) => {
                runtime.request_terminal_close(
                    model,
                    lazecode_ui::runtime::TerminalCloseAction::Pane(id),
                );
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
                let key = format!("worktree-add:{}:{name}", project_root.display());
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
                let mut switched = None;
                let settings = model.settings.clone();
                if let Some(project) = model.active_project_mut() {
                    match project.switch_worktree(&name, &settings) {
                        Ok(_) => {
                            switched = Some((
                                project.active_root().to_path_buf(),
                                project.log_query.clone(),
                                project.root.clone(),
                            ));
                        }
                        Err(err) => {
                            model.toast(err, ToastKind::Error);
                        }
                    }
                }
                if let Some((active_root, query, project_root)) = switched {
                    self.refresh_git(active_root.clone(), None);
                    self.refresh_log(active_root, query, 0);
                    self.refresh_branches(project_root);
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
                // Recheck right before removal; turn the failure into a
                // force-confirmation prompt instead of a toast.
                if !force {
                    if let Ok(snapshot) = lazecode_git::snapshot(&worktree_path) {
                        if !snapshot.changes.is_empty() {
                            runtime.dialog = lazecode_ui::runtime::DialogState::RemoveWorktree {
                                name: name.clone(),
                                worktree_path,
                                dirty: true,
                                changes: snapshot.changes.len(),
                            };
                            return;
                        }
                    }
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
                        // `forget_terminal` covers every per-pane map and the
                        // focus; hand-maintained subsets drift (kitty texture
                        // caches used to leak here).
                        runtime.forget_terminal(id);
                    }
                }
                let key = format!("worktree-remove:{}:{name}", project_root.display());
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
}
