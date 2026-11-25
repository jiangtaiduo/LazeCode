//! Submit actions for the dialog footer: validate the input, queue the
//! command and write the dialog state back.

use lazecode_core::{AppModel, Command, TabKind, ToastKind};

use crate::runtime::{DialogState, UiRuntime};

use super::super::used_worktree_branches;
use super::delete_paths_to_trash;

/// Rename only accepts a single path component. Allowing `../other` here
/// would turn a rename into a silent move outside the current directory.
fn is_plain_name(name: &str) -> bool {
    let mut components = std::path::Path::new(name).components();
    matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
}

/// Apply the footer submit and write the dialog state back.
pub(super) fn finish_dialog(
    model: &mut AppModel,
    rt: &mut UiRuntime,
    mut dialog: DialogState,
    confirm: bool,
    confirm_add_remote: bool,
    close_from_footer: bool,
) {
    let mut close = close_from_footer;
    if confirm {
        match &dialog {
            DialogState::NewBranch {
                value,
                start_point,
                checkout,
                ..
            } => {
                let value = value.trim().to_string();
                if value.is_empty() {
                    rt.dialog = DialogState::NewBranch {
                        value,
                        start_point: start_point.clone(),
                        checkout: *checkout,
                        error: Some(
                            crate::tr!(model, "Enter a branch name", "请输入分支名").to_string(),
                        ),
                    };
                    close = true;
                } else {
                    model.queue(Command::CreateBranch {
                        name: value,
                        start_point: start_point.clone(),
                        checkout: *checkout,
                    });
                    close = true;
                }
            }
            DialogState::NewWorktree {
                name,
                branch,
                create_new,
                location,
                ..
            } => {
                let name = name.trim().to_string();
                let branch = branch.trim().to_string();
                let location_str = location.trim().to_string();

                let used_branches = model
                    .active_project()
                    .map(used_worktree_branches)
                    .unwrap_or_default();

                if name.is_empty() {
                    rt.dialog = DialogState::NewWorktree {
                        name,
                        branch,
                        create_new: *create_new,
                        location: location_str,
                        error: Some(
                            crate::tr!(model, "Enter a worktree name", "请输入工作树名称")
                                .to_string(),
                        ),
                    };
                } else if branch.is_empty() {
                    rt.dialog = DialogState::NewWorktree {
                        name,
                        branch,
                        create_new: *create_new,
                        location: location_str,
                        error: Some(if *create_new {
                            crate::tr!(model, "Enter a new branch name", "请输入新分支名称")
                                .to_string()
                        } else {
                            crate::tr!(model, "Select a branch", "请选择分支").to_string()
                        }),
                    };
                } else if let Some(wt_name) = used_branches.get(&branch) {
                    rt.dialog = DialogState::NewWorktree {
                        name,
                        branch: branch.clone(),
                        create_new: *create_new,
                        location: location_str,
                        error: Some(if model.settings.language.is_zh() {
                            format!("分支 '{branch}' 已在工作树 '{wt_name}' 检出，多个工作树不能使用相同分支")
                        } else {
                            format!("Branch '{branch}' is already checked out in '{wt_name}'")
                        }),
                    };
                } else {
                    let loc_path = if location_str.is_empty() {
                        let root = model
                            .active_project()
                            .map(|p| p.root.clone())
                            .unwrap_or_default();
                        root.join(".worktree").join(&name)
                    } else {
                        std::path::PathBuf::from(location_str)
                    };
                    model.queue(Command::CreateWorktree {
                        name,
                        branch,
                        location: loc_path,
                    });
                    close = true;
                }
            }
            DialogState::CheckoutRevision { value, .. } => {
                let value = value.trim().to_string();
                if value.is_empty() {
                    rt.dialog = DialogState::CheckoutRevision {
                        value,
                        error: Some(
                            crate::tr!(model, "Enter revision or branch", "请输入修订版本或分支")
                                .to_string(),
                        ),
                    };
                    close = true;
                } else {
                    model.queue(Command::CheckoutBranch(value));
                    close = true;
                }
            }
            DialogState::RemoveWorktree {
                name,
                dirty,
                worktree_path: _,
                changes: _,
            } => {
                model.queue(Command::RemoveWorktree {
                    name: name.clone(),
                    force: *dirty,
                });
                close = true;
            }
            DialogState::DeleteBranch {
                name,
                force,
                remote,
                ..
            } => {
                if *remote {
                    model.queue(Command::DeleteRemoteBranch(name.clone()));
                } else {
                    model.queue(Command::DeleteBranch {
                        name: name.clone(),
                        force: *force,
                    });
                }
                close = true;
            }
            DialogState::RenameBranch { from, value, .. } => {
                let value = value.trim().to_string();
                if value.is_empty() || value == *from {
                    rt.dialog = DialogState::RenameBranch {
                        from: from.clone(),
                        value,
                        error: Some(
                            crate::tr!(model, "Enter a new branch name", "请输入新的分支名")
                                .to_string(),
                        ),
                    };
                } else {
                    model.queue(Command::RenameBranch {
                        from: from.clone(),
                        to: value,
                    });
                    close = true;
                }
            }
            DialogState::CloseDirtyTab { tab_id, .. } => {
                if let Some(index) = model.active_project {
                    model.close_tab(index, *tab_id);
                }
                if rt.editor_focus == Some(*tab_id) {
                    rt.editor_focus = None;
                }
                close = true;
            }
            DialogState::ConfirmTerminalClose { action, .. } => {
                let action = action.clone();
                rt.apply_terminal_close(model, action);
                close = true;
            }
            DialogState::Quit { restart_update, .. } => {
                if *restart_update {
                    match rt.update.relaunch_installed() {
                        Ok(()) => model.should_quit = true,
                        Err(err) => {
                            rt.update.error = Some(err);
                            rt.update_open = true;
                        }
                    }
                } else {
                    model.should_quit = true;
                }
                close = true;
            }
            DialogState::Push {
                remote,
                branch,
                set_upstream,
                force,
                push_tags,
                ..
            } => {
                model.queue(Command::Push {
                    remote: remote.clone(),
                    branch: branch.clone(),
                    set_upstream: *set_upstream,
                    force: *force,
                    push_tags: *push_tags,
                });
                close = true;
            }
            DialogState::Pull { mode, .. } => {
                model.queue(Command::Pull { mode: *mode });
                close = true;
            }
            DialogState::Remotes {
                remotes,
                new_name,
                new_url,
                editing,
                ..
            } => {
                if confirm_add_remote {
                    let name = new_name.trim().to_string();
                    let url = new_url.trim().to_string();
                    if name.is_empty() || url.is_empty() {
                        rt.dialog = DialogState::Remotes {
                            remotes: remotes.clone(),
                            new_name: new_name.clone(),
                            new_url: new_url.clone(),
                            editing: editing.clone(),
                            error: Some(
                                crate::tr!(model, "Enter a name and a URL", "请输入名称和 URL")
                                    .to_string(),
                            ),
                        };
                    } else if let Some(from) = editing.clone() {
                        model.queue(Command::RemoteEdit { from, name, url });
                        close = true;
                    } else {
                        model.queue(Command::RemoteAdd { name, url });
                        close = true;
                    }
                } else {
                    close = true;
                }
            }
            DialogState::Reset { hash, mode } => {
                model.queue(Command::ResetTo {
                    hash: hash.clone(),
                    mode: *mode,
                });
                close = true;
            }
            DialogState::NewTag {
                commit: commit_hint,
                value,
                ..
            } => {
                let name = value.trim().to_string();
                if name.is_empty() {
                    rt.dialog = DialogState::NewTag {
                        commit: commit_hint.clone(),
                        value: value.clone(),
                        error: Some(
                            crate::tr!(model, "Enter a tag name", "请输入标签名").to_string(),
                        ),
                    };
                } else {
                    model.queue(Command::CreateTag {
                        name,
                        commit: commit_hint.clone(),
                    });
                    close = true;
                }
            }
            DialogState::NewFile { dir, value, .. } => {
                let name = value.trim();
                if name.is_empty() {
                    rt.dialog = DialogState::NewFile {
                        dir: dir.clone(),
                        value: value.clone(),
                        error: Some(
                            crate::tr!(model, "Name cannot be empty", "名称不能为空").to_string(),
                        ),
                    };
                } else if !is_plain_name(name) {
                    rt.dialog = DialogState::NewFile {
                        dir: dir.clone(),
                        value: value.clone(),
                        error: Some(
                            crate::tr!(
                                model,
                                "Enter a file name, not a path",
                                "请输入文件名，不要包含路径"
                            )
                            .to_string(),
                        ),
                    };
                } else {
                    let target = dir.join(name);
                    if target.exists() {
                        rt.dialog = DialogState::NewFile {
                            dir: dir.clone(),
                            value: value.clone(),
                            error: Some(
                                crate::tr!(model, "File already exists", "文件已存在").to_string(),
                            ),
                        };
                    } else if let Err(e) = std::fs::write(&target, "") {
                        rt.dialog = DialogState::NewFile {
                            dir: dir.clone(),
                            value: value.clone(),
                            error: Some(e.to_string()),
                        };
                    } else {
                        if let Some(active) = model.active_project {
                            let _ = model.open_file_in(active, target);
                        }
                        close = true;
                    }
                }
            }
            DialogState::NewDirectory { dir, value, .. } => {
                let name = value.trim();
                if name.is_empty() {
                    rt.dialog = DialogState::NewDirectory {
                        dir: dir.clone(),
                        value: value.clone(),
                        error: Some(
                            crate::tr!(model, "Name cannot be empty", "名称不能为空").to_string(),
                        ),
                    };
                } else if !is_plain_name(name) {
                    rt.dialog = DialogState::NewDirectory {
                        dir: dir.clone(),
                        value: value.clone(),
                        error: Some(
                            crate::tr!(
                                model,
                                "Enter a directory name, not a path",
                                "请输入目录名，不要包含路径"
                            )
                            .to_string(),
                        ),
                    };
                } else {
                    let target = dir.join(name);
                    if target.exists() {
                        rt.dialog = DialogState::NewDirectory {
                            dir: dir.clone(),
                            value: value.clone(),
                            error: Some(
                                crate::tr!(model, "Directory already exists", "目录已存在")
                                    .to_string(),
                            ),
                        };
                    } else if let Err(e) = std::fs::create_dir_all(&target) {
                        rt.dialog = DialogState::NewDirectory {
                            dir: dir.clone(),
                            value: value.clone(),
                            error: Some(e.to_string()),
                        };
                    } else {
                        close = true;
                    }
                }
            }
            DialogState::RenamePath {
                old_path, value, ..
            } => {
                let name = value.trim();
                if name.is_empty() {
                    rt.dialog = DialogState::RenamePath {
                        old_path: old_path.clone(),
                        value: value.clone(),
                        error: Some(
                            crate::tr!(model, "Name cannot be empty", "名称不能为空").to_string(),
                        ),
                    };
                } else if !is_plain_name(name) {
                    rt.dialog = DialogState::RenamePath {
                        old_path: old_path.clone(),
                        value: value.clone(),
                        error: Some(
                            crate::tr!(
                                model,
                                "Enter a file name, not a path",
                                "请输入文件名，不要包含路径"
                            )
                            .to_string(),
                        ),
                    };
                } else if old_path.file_name() == Some(std::ffi::OsStr::new(name)) {
                    rt.dialog = DialogState::RenamePath {
                        old_path: old_path.clone(),
                        value: value.clone(),
                        error: Some(crate::tr!(model, "Name unchanged", "名称未改变").to_string()),
                    };
                } else if let Some(parent) = old_path.parent() {
                    let target = parent.join(name);
                    match lazecode_fs::rename_no_overwrite(old_path, &target) {
                        Err(lazecode_fs::RenameError::TargetExists { is_dir: true }) => {
                            rt.dialog = DialogState::RenamePath {
                                old_path: old_path.clone(),
                                value: value.clone(),
                                error: Some(
                                    crate::tr!(model, "Directory already exists", "目录已存在")
                                        .to_string(),
                                ),
                            };
                        }
                        Err(lazecode_fs::RenameError::TargetExists { is_dir: false }) => {
                            rt.dialog = DialogState::RenamePath {
                                old_path: old_path.clone(),
                                value: value.clone(),
                                error: Some(
                                    crate::tr!(model, "File already exists", "文件已存在")
                                        .to_string(),
                                ),
                            };
                        }
                        Err(lazecode_fs::RenameError::Io(e)) => {
                            rt.dialog = DialogState::RenamePath {
                                old_path: old_path.clone(),
                                value: value.clone(),
                                error: Some(e.to_string()),
                            };
                        }
                        Ok(()) => {
                            model.retarget_path(old_path, &target);
                            // Recent files lists files; a renamed directory
                            // would open as an error.
                            if target.is_file() {
                                model.note_recent_file(target);
                            }
                            close = true;
                        }
                    }
                } else {
                    close = true;
                }
            }
            DialogState::DeletePath { path, .. } => {
                // 删除走废纸篓语义（稳定版 fs_delete），不再永久 unlink；
                // 关标签/提示/刷新统一在 delete_paths_to_trash 里收尾
                // （目录路径由助手连带关闭其下所有标签）。
                delete_paths_to_trash(model, std::slice::from_ref(path));
                close = true;
            }
            DialogState::ConfirmDeleteUntracked { paths } => {
                delete_paths_to_trash(model, paths);
                close = true;
            }
            DialogState::ConfirmDeleteShelf { name, purge } => {
                // 稳定版删除架藏前有确认；删除进入「最近删除」（可恢复），
                // purge = 彻底删除。确认后重新加载架藏数据。
                if let Some(project) = model.active_project() {
                    let root = project.root.clone();
                    let result = if *purge {
                        lazecode_git::shelf_purge_deleted(&root, name)
                    } else {
                        lazecode_git::shelf_delete(&root, name)
                    };
                    let zh = model.settings.language.is_zh();
                    model.toast(
                        match &result {
                            Ok(_) if !purge => {
                                if zh {
                                    format!("已删除架藏 {name}（可在最近删除中恢复）")
                                } else {
                                    format!("Deleted shelf {name} (restorable)")
                                }
                            }
                            Ok(_) => "Purged".to_string(),
                            Err(err) => err.to_string(),
                        },
                        if result.is_ok() {
                            ToastKind::Info
                        } else {
                            ToastKind::Error
                        },
                    );
                    rt.shelf_loaded = None;
                }
                if *purge {
                    rt.dialog = DialogState::None;
                } else {
                    rt.dialog = DialogState::ShelfManager {
                        shelves: Vec::new(),
                        selected: 0,
                        error: None,
                        loaded: false,
                    };
                }
                close = true;
            }
            DialogState::RenameShelf { from, value, .. } => {
                let name = value.trim().to_string();
                if name.is_empty() {
                    rt.dialog = DialogState::RenameShelf {
                        from: from.clone(),
                        value: name,
                        error: Some(
                            crate::tr!(model, "Enter a shelf name", "请输入架藏名称").to_string(),
                        ),
                    };
                    close = true;
                } else {
                    model.queue(Command::ShelfRename {
                        from: from.clone(),
                        to: name,
                    });
                    rt.shelf_loaded = None;
                    close = true;
                }
            }
            DialogState::NewShelf { value, paths, .. } => {
                let name = value.trim().to_string();
                if name.is_empty() {
                    rt.dialog = DialogState::NewShelf {
                        value: name,
                        error: Some(
                            crate::tr!(model, "Enter a shelf name", "请输入架藏名称").to_string(),
                        ),
                        paths: paths.clone(),
                    };
                    close = true;
                } else {
                    model.queue(Command::ShelfSave {
                        name,
                        paths: paths.clone(),
                    });
                    close = true;
                }
            }
            DialogState::GoToLine { value, .. } => {
                let text = value.trim();
                if let Ok(line) = text.parse::<usize>() {
                    model.queue(Command::GoToLine(line));
                    close = true;
                } else {
                    rt.dialog = DialogState::GoToLine {
                        value: value.clone(),
                        error: Some(
                            crate::tr!(model, "Enter a valid line number", "请输入有效的行号")
                                .to_string(),
                        ),
                    };
                    close = true;
                }
            }
            DialogState::RenameTerminal {
                terminal_id, value, ..
            } => {
                let name = value.trim().to_string();
                if !name.is_empty() {
                    if let Some(project) = model.active_project_mut() {
                        if let Some(term) = project.term_by_id_mut(*terminal_id) {
                            term.title = name;
                        }
                    }
                }
                close = true;
            }
            DialogState::RenameSymbol {
                tab_id,
                path,
                line,
                col,
                value,
                ..
            } => {
                let new_name = value.trim().to_string();
                if new_name.is_empty() {
                    rt.dialog = DialogState::RenameSymbol {
                        tab_id: *tab_id,
                        path: path.clone(),
                        line: *line,
                        col: *col,
                        value: new_name,
                        error: Some(
                            crate::tr!(model, "Enter a new name", "请输入新名称").to_string(),
                        ),
                    };
                } else {
                    let path_buf = std::path::PathBuf::from(path);
                    let position = model.projects.iter().find_map(|project| {
                        project.tabs.iter().find_map(|tab| match &tab.kind {
                            TabKind::File(file) if file.doc.path == path_buf => {
                                Some(file.doc.lsp_position(lazecode_text::Cursor {
                                    line: *line,
                                    col: *col,
                                }))
                            }
                            _ => None,
                        })
                    });
                    let message = match position {
                        Some((lsp_line, lsp_character)) => {
                            match model
                                .lsp
                                .rename(&path_buf, lsp_line, lsp_character, &new_name)
                            {
                                Some(id) => {
                                    rt.lsp.rename_pending = Some(id);
                                    close = true;
                                    None
                                }
                                None => Some(
                                    crate::tr!(
                                        model,
                                        "No language server is available",
                                        "没有可用的语言服务器"
                                    )
                                    .to_string(),
                                ),
                            }
                        }
                        None => Some(
                            crate::tr!(model, "The file is no longer open", "文件已不再打开")
                                .to_string(),
                        ),
                    };
                    if let Some(error) = message {
                        rt.dialog = DialogState::RenameSymbol {
                            tab_id: *tab_id,
                            path: path.clone(),
                            line: *line,
                            col: *col,
                            value: new_name,
                            error: Some(error),
                        };
                    }
                }
            }
            DialogState::NewChangelist { value, .. } => {
                let name = value.trim().to_string();
                if name.is_empty() {
                    rt.dialog = DialogState::NewChangelist {
                        value: name,
                        error: Some(
                            crate::tr!(model, "Enter a changelist name", "请输入变更列表名称")
                                .to_string(),
                        ),
                    };
                } else {
                    // 写入走后台任务，与 stage/commit 等路径一致（不再在
                    // UI 线程同步 IO）。
                    model.queue(Command::ChangelistCreate(name));
                    close = true;
                }
            }
            DialogState::StashCreate {
                message,
                include_untracked,
                ..
            } => {
                let message = message.trim().to_string();
                model.queue(Command::StashPush {
                    message: (!message.is_empty()).then_some(message),
                    include_untracked: *include_untracked,
                });
                close = true;
            }
            DialogState::Unstash {
                stash,
                as_new_branch,
                pop,
                reinstate_index,
                ..
            } => {
                let branch = as_new_branch.trim().to_string();
                model.queue(Command::Unstash {
                    stash: stash.clone(),
                    branch: (!branch.is_empty()).then_some(branch),
                    pop: *pop,
                    reinstate_index: *reinstate_index,
                });
                close = true;
            }
            DialogState::ConfirmDiscard { paths } => {
                let paths = paths.clone();
                if !paths.is_empty() {
                    model.queue(Command::DiscardFiles(paths));
                }
                close = true;
            }
            DialogState::DeleteChangelist { name, rollback, .. } => {
                model.queue(Command::ChangelistDelete {
                    name: name.clone(),
                    rollback: *rollback,
                });
                close = true;
            }
            DialogState::Unshelve {
                name,
                files,
                target,
                new_name,
                ..
            } => {
                let selected: Vec<String> = files
                    .iter()
                    .filter(|(_, selected)| *selected)
                    .map(|(path, _)| path.clone())
                    .collect();
                let mut target = target.clone();
                if target.is_none() {
                    let new_name = new_name.trim().to_string();
                    if new_name.is_empty() {
                        rt.dialog = DialogState::Unshelve {
                            name: name.clone(),
                            files: files.clone(),
                            target: None,
                            new_name,
                            error: Some(
                                crate::tr!(
                                    model,
                                    "Pick a changelist or enter a new name",
                                    "请选择目标变更列表或输入新名称"
                                )
                                .to_string(),
                            ),
                        };
                        close = true;
                    } else {
                        target = Some(new_name);
                    }
                }
                if let Some(target) = target {
                    if selected.is_empty() {
                        rt.dialog = DialogState::Unshelve {
                            name: name.clone(),
                            files: files.clone(),
                            target: Some(target),
                            new_name: String::new(),
                            error: Some(
                                crate::tr!(
                                    model,
                                    "Select at least one file to unshelve",
                                    "请至少勾选一个要恢复的文件"
                                )
                                .to_string(),
                            ),
                        };
                        close = true;
                    } else {
                        model.queue(Command::ShelfRestore {
                            name: name.clone(),
                            paths: Some(selected),
                            target_list: Some(target),
                            delete_after: false,
                        });
                        close = true;
                    }
                }
            }
            DialogState::CommitFiles { paths, value, .. }
            | DialogState::ExtractChanges { paths, value, .. } => {
                let message = value.trim().to_string();
                if message.is_empty() {
                    let dialog = match &dialog {
                        DialogState::ExtractChanges { paths, value, .. } => {
                            DialogState::ExtractChanges {
                                paths: paths.clone(),
                                value: value.clone(),
                                error: Some(
                                    crate::tr!(
                                        model,
                                        "Add a commit message first",
                                        "请先填写提交信息"
                                    )
                                    .to_string(),
                                ),
                            }
                        }
                        _ => DialogState::CommitFiles {
                            paths: paths.clone(),
                            value: value.clone(),
                            error: Some(
                                crate::tr!(model, "Add a commit message first", "请先填写提交信息")
                                    .to_string(),
                            ),
                        },
                    };
                    rt.dialog = dialog;
                    close = true;
                } else {
                    model.queue(Command::CommitPaths {
                        paths: paths.clone(),
                        message,
                    });
                    close = true;
                }
            }
            DialogState::GetFileFromRevision { path, revision, .. } => {
                let revision = revision.trim().to_string();
                if revision.is_empty() {
                    if let DialogState::GetFileFromRevision { error, .. } = &mut dialog {
                        *error =
                            Some(crate::tr!(model, "Enter a revision", "请输入版本").to_string());
                    }
                } else {
                    model.queue(Command::GetFileFromRevision {
                        path: path.clone(),
                        revision,
                    });
                    close = true;
                }
            }
            DialogState::CherryPickChanges { paths, target, .. } => {
                let target = target.trim().to_string();
                if target.is_empty() {
                    if let DialogState::CherryPickChanges { error, .. } = &mut dialog {
                        *error = Some(
                            crate::tr!(model, "Enter a target branch", "请输入目标分支")
                                .to_string(),
                        );
                    }
                } else {
                    model.queue(Command::CherryPickChanges {
                        paths: paths.clone(),
                        target,
                    });
                    close = true;
                }
            }
            _ => {}
        }
    }
    if !close {
        // Confirmation branches replace `rt.dialog` with an error-bearing
        // variant and leave the dialog open. Only restore the pre-draw state
        // when no branch produced a replacement; unconditionally assigning
        // `dialog` here would silently discard every validation message.
        if matches!(&rt.dialog, DialogState::None) {
            rt.dialog = dialog;
        }
    } else {
        rt.branch_create_error = None;
    }
    let _ = (model, ToastKind::Info);
}
