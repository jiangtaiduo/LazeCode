//! Dialog orchestration: chrome, the per-variant body dispatch, the shared
//! footer and the submit actions. Manager windows live in `managers`.

use egui::{Align2, Color32, FontId, Key, Pos2, Rect, Ui, Vec2};
use lazecode_core::{AppModel, Command, TabKind, ToastKind};

use crate::runtime::{DialogState, UiRuntime};
use crate::theme::Theme;
use crate::widgets::{primary_button, secondary_button};

use super::conflict_tool::draw_conflict_tool;
use super::{frame, used_worktree_branches};

mod branch;
mod chrome;
mod files;
mod managers;
mod stash;
mod sync;
mod worktree;

use managers::{
    draw_clone_repo_dialog, draw_file_history_dialog, draw_git_tools_dialog,
    draw_interactive_rebase_dialog, draw_shelf_manager_dialog, draw_squash_dialog,
    draw_stash_manager_dialog,
};

/// Rename only accepts a single path component. Allowing `../other` here
/// would turn a rename into a silent move outside the current directory.
fn is_plain_name(name: &str) -> bool {
    let mut components = std::path::Path::new(name).components();
    matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
}

/// 把一批路径移入系统废纸篓并收尾（对齐稳定版 deletePathsToTrash）：
/// 逐个 trash、关闭已删除路径下的编辑器标签、按删除数量/失败名单提示，
/// 最后刷新活动项目快照让变更列表收窄。目录路径会连带关闭其下所有标签。
fn delete_paths_to_trash(model: &mut AppModel, paths: &[std::path::PathBuf]) {
    let mut failed: Vec<String> = Vec::new();
    for path in paths {
        if let Err(err) = lazecode_fs::move_to_trash(path) {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
            log::warn!("trash delete failed for {name}: {err}");
            failed.push(name);
        }
    }
    // 文件已不存在：关闭其编辑器标签，避免留下幽灵缓冲。
    for project in model.projects.iter_mut() {
        let mut closed: Vec<u64> = Vec::new();
        for path in paths {
            for tab in &project.tabs {
                if let TabKind::File(file) = &tab.kind {
                    let under = path.is_dir() && file.doc.path.starts_with(path);
                    if file.doc.path == *path || under {
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
    if failed.is_empty() {
        let zh = model.settings.language.is_zh();
        let message = if paths.len() == 1 {
            let name = paths[0]
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            if zh {
                format!("已删除 {name}（已移入废纸篓）")
            } else {
                format!("Deleted {name} (moved to the trash)")
            }
        } else if zh {
            format!("已删除 {} 个文件（已移入废纸篓）", paths.len())
        } else {
            format!("Deleted {} files (moved to the trash)", paths.len())
        };
        model.toast(message, ToastKind::Info);
    } else {
        let joined = failed.join(", ");
        model.toast(
            if model.settings.language.is_zh() {
                format!("删除失败：{joined}")
            } else {
                format!("Delete failed: {joined}")
            },
            ToastKind::Error,
        );
    }
    model.queue(Command::ReloadGit);
}

pub(super) fn draw_dialog(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
) {
    let p = theme.palette;
    let mut dialog = std::mem::replace(&mut rt.dialog, DialogState::None);
    // Esc 关闭对话框（稳定版行为）；冲突解决编辑器除外——里面是用户
    // 手工合并的内容，误按 Esc 直接丢弃太危险。
    let esc_closes = !matches!(dialog, DialogState::ConflictResolve { .. });
    if esc_closes && ui.input(|input| input.key_pressed(Key::Escape)) {
        return;
    }
    if let DialogState::ConflictResolve {
        mut path,
        mut ours,
        mut theirs,
        mut worktree,
    } = dialog
    {
        ui.painter()
            .rect_filled(screen, 0.0, Color32::from_black_alpha(150));
        let width = (screen.width() - 40.0).min(1140.0);
        let height = (screen.height() - 40.0).min(760.0);
        let rect = Rect::from_center_size(screen.center(), Vec2::new(width, height));
        frame(ui, rect, theme);
        let mut close = false;
        draw_conflict_tool(
            ui,
            model,
            theme,
            rect,
            &mut path,
            &mut ours,
            &mut theirs,
            &mut worktree,
            &mut close,
        );
        if !close {
            rt.dialog = DialogState::ConflictResolve {
                path,
                ours,
                theirs,
                worktree,
            };
        }
        return;
    }

    if let DialogState::Squash {
        commits,
        mut value,
        mut error,
    } = dialog
    {
        let mut close = false;
        draw_squash_dialog(
            ui, model, theme, screen, &commits, &mut value, &mut error, &mut close,
        );
        if !close {
            rt.dialog = DialogState::Squash {
                commits,
                value,
                error,
            };
        }
        return;
    }

    if let DialogState::StashManager {
        mut stashes,
        mut selected,
        mut error,
        mut loaded,
    } = dialog
    {
        let mut close = false;
        draw_stash_manager_dialog(
            ui,
            model,
            theme,
            screen,
            &mut stashes,
            &mut selected,
            &mut error,
            &mut loaded,
            &mut close,
        );
        if !close {
            rt.dialog = DialogState::StashManager {
                stashes,
                selected,
                error,
                loaded,
            };
        }
        return;
    }

    if let DialogState::ShelfManager {
        mut shelves,
        mut selected,
        mut error,
        mut loaded,
    } = dialog
    {
        let mut close = false;
        draw_shelf_manager_dialog(
            ui,
            model,
            rt,
            theme,
            screen,
            &mut shelves,
            &mut selected,
            &mut error,
            &mut loaded,
            &mut close,
        );
        if !close {
            rt.dialog = DialogState::ShelfManager {
                shelves,
                selected,
                error,
                loaded,
            };
        }
        return;
    }

    if let DialogState::FileHistory {
        path,
        mut entries,
        mut selected,
        mut loaded,
    } = dialog
    {
        let mut close = false;
        draw_file_history_dialog(
            ui,
            model,
            theme,
            screen,
            &path,
            &mut entries,
            &mut selected,
            &mut loaded,
            &mut close,
        );
        if !close {
            rt.dialog = DialogState::FileHistory {
                path,
                entries,
                selected,
                loaded,
            };
        }
        return;
    }

    if let DialogState::GitTools {
        mut tab,
        mut submodules,
        mut lfs_status,
        mut loaded,
    } = dialog
    {
        let mut close = false;
        draw_git_tools_dialog(
            ui,
            model,
            theme,
            screen,
            &mut tab,
            &mut submodules,
            &mut lfs_status,
            &mut loaded,
            &mut close,
        );
        if !close {
            rt.dialog = DialogState::GitTools {
                tab,
                submodules,
                lfs_status,
                loaded,
            };
        }
        return;
    }

    if let DialogState::CloneRepo {
        mut url,
        mut dest,
        mut error,
    } = dialog
    {
        let mut close = false;
        draw_clone_repo_dialog(
            ui, model, theme, screen, &mut url, &mut dest, &mut error, &mut close,
        );
        if !close {
            rt.dialog = DialogState::CloneRepo { url, dest, error };
        }
        return;
    }

    if let DialogState::InteractiveRebase { onto, mut todos } = dialog {
        let mut close = false;
        draw_interactive_rebase_dialog(ui, model, theme, screen, &onto, &mut todos, &mut close);
        if !close {
            rt.dialog = DialogState::InteractiveRebase { onto, todos };
        }
        return;
    }
    let mut confirm_add_remote = false;
    let Some((title, subtitle, width, height)) = chrome::dialog_chrome(model, &dialog) else {
        return;
    };
    ui.painter()
        .rect_filled(screen, 0.0, Color32::from_black_alpha(140));
    let rect = Rect::from_center_size(screen.center(), Vec2::new(width, height));
    frame(ui, rect, theme);
    ui.painter().text(
        Pos2::new(rect.left() + 22.0, rect.top() + 28.0),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(15.0),
        p.text,
    );
    paint_wrapped_text(
        ui,
        Rect::from_min_max(
            Pos2::new(rect.left() + 22.0, rect.top() + 46.0),
            Pos2::new(rect.right() - 22.0, rect.top() + 86.0),
        ),
        &subtitle,
        FontId::proportional(10.5),
        p.text3,
    );
    let mut close = false;
    let mut confirm = false;
    // Dialogs extracted into their own modules run first; their arms were
    // removed from the match below so the two never both fire.
    if matches!(&dialog, DialogState::NewWorktree { .. }) {
        let (arm_close, arm_confirm) =
            worktree::draw_new_worktree(ui, model, theme, rect, &mut dialog);
        close |= arm_close;
        confirm |= arm_confirm;
    }
    if matches!(&dialog, DialogState::Push { .. }) {
        let (arm_close, arm_confirm) = sync::draw_push(ui, model, theme, rect, &mut dialog);
        close |= arm_close;
        confirm |= arm_confirm;
    }
    if matches!(&dialog, DialogState::Pull { .. }) {
        let (arm_close, arm_confirm) = sync::draw_pull(ui, model, theme, rect, &mut dialog);
        close |= arm_close;
        confirm |= arm_confirm;
    }
    if matches!(&dialog, DialogState::Remotes { .. }) {
        let (arm_close, arm_confirm) =
            sync::draw_remotes(ui, model, theme, rect, &mut dialog, &mut confirm_add_remote);
        close |= arm_close;
        confirm |= arm_confirm;
    }
    if matches!(&dialog, DialogState::Reset { .. }) {
        let (arm_close, arm_confirm) = sync::draw_reset(ui, model, theme, rect, &mut dialog);
        close |= arm_close;
        confirm |= arm_confirm;
    }
    if matches!(&dialog, DialogState::NewTag { .. }) {
        let (arm_close, arm_confirm) = sync::draw_new_tag(ui, theme, rect, &mut dialog);
        close |= arm_close;
        confirm |= arm_confirm;
    }
    if matches!(&dialog, DialogState::StashCreate { .. }) {
        let (arm_close, arm_confirm) =
            stash::draw_stash_create(ui, model, theme, rect, &mut dialog);
        close |= arm_close;
        confirm |= arm_confirm;
    }
    if matches!(&dialog, DialogState::Unstash { .. }) {
        let (arm_close, arm_confirm) = stash::draw_unstash(ui, model, theme, rect, &mut dialog);
        close |= arm_close;
        confirm |= arm_confirm;
    }
    if matches!(&dialog, DialogState::ConfirmDiscard { .. }) {
        let (arm_close, arm_confirm) = stash::draw_confirm_discard(ui, theme, rect, &mut dialog);
        close |= arm_close;
        confirm |= arm_confirm;
    }
    if matches!(&dialog, DialogState::DeleteChangelist { .. }) {
        let (arm_close, arm_confirm) =
            stash::draw_delete_changelist(ui, model, theme, rect, &mut dialog);
        close |= arm_close;
        confirm |= arm_confirm;
    }
    if matches!(&dialog, DialogState::Unshelve { .. }) {
        let (arm_close, arm_confirm) = stash::draw_unshelve(ui, model, theme, rect, &mut dialog);
        close |= arm_close;
        confirm |= arm_confirm;
    }
    if matches!(&dialog, DialogState::NewBranch { .. }) {
        let (arm_close, arm_confirm) =
            branch::draw_new_branch(ui, model, rt, theme, rect, &mut dialog);
        close |= arm_close;
        confirm |= arm_confirm;
    }
    if matches!(&dialog, DialogState::CheckoutRevision { .. }) {
        let (arm_close, arm_confirm) = branch::draw_checkout_revision(ui, theme, rect, &mut dialog);
        close |= arm_close;
        confirm |= arm_confirm;
    }
    if matches!(&dialog, DialogState::DeleteBranch { .. }) {
        let (arm_close, arm_confirm) =
            branch::draw_delete_branch(ui, model, theme, rect, &mut dialog);
        close |= arm_close;
        confirm |= arm_confirm;
    }
    if matches!(&dialog, DialogState::RenameBranch { .. }) {
        let (arm_close, arm_confirm) = branch::draw_rename_branch(ui, theme, rect, &mut dialog);
        close |= arm_close;
        confirm |= arm_confirm;
    }
    if matches!(&dialog, DialogState::RemoveWorktree { .. }) {
        let (arm_close, arm_confirm) =
            branch::draw_remove_worktree(ui, model, theme, rect, &mut dialog);
        close |= arm_close;
        confirm |= arm_confirm;
    }
    if matches!(&dialog, DialogState::CloseDirtyTab { .. }) {
        let (arm_close, arm_confirm) = branch::draw_close_dirty_tab(ui, theme, rect, &mut dialog);
        close |= arm_close;
        confirm |= arm_confirm;
    }
    if matches!(
        &dialog,
        DialogState::NewFile { .. }
            | DialogState::NewDirectory { .. }
            | DialogState::RenamePath { .. }
            | DialogState::NewShelf { .. }
            | DialogState::GoToLine { .. }
            | DialogState::RenameSymbol { .. }
            | DialogState::RenameTerminal { .. }
            | DialogState::NewChangelist { .. }
    ) {
        let (arm_close, arm_confirm) = files::draw_path_input_dialogs(ui, theme, rect, &mut dialog);
        close |= arm_close;
        confirm |= arm_confirm;
    }
    if matches!(&dialog, DialogState::DeletePath { .. }) {
        let (arm_close, arm_confirm) = files::draw_delete_path(ui, model, theme, rect, &mut dialog);
        close |= arm_close;
        confirm |= arm_confirm;
    }
    if matches!(&dialog, DialogState::ConfirmDeleteUntracked { .. }) {
        let (arm_close, arm_confirm) =
            files::draw_delete_untracked(ui, model, theme, rect, &mut dialog);
        close |= arm_close;
        confirm |= arm_confirm;
    }
    if matches!(
        &dialog,
        DialogState::CommitFiles { .. } | DialogState::ExtractChanges { .. }
    ) {
        let (arm_close, arm_confirm) =
            files::draw_commit_files(ui, model, theme, rect, &mut dialog);
        close |= arm_close;
        confirm |= arm_confirm;
    }
    if matches!(&dialog, DialogState::GetFileFromRevision { .. }) {
        let (arm_close, arm_confirm) =
            files::draw_get_file_from_revision(ui, model, theme, rect, &mut dialog);
        close |= arm_close;
        confirm |= arm_confirm;
    }
    if matches!(&dialog, DialogState::CherryPickChanges { .. }) {
        let (arm_close, arm_confirm) =
            files::draw_cherry_pick_changes(ui, model, theme, rect, &mut dialog);
        close |= arm_close;
        confirm |= arm_confirm;
    }
    let cancel = Rect::from_min_size(
        Pos2::new(rect.right() - 208.0, rect.bottom() - 48.0),
        Vec2::new(88.0, 30.0),
    );
    if secondary_button(ui, cancel, crate::tr!(model, "Cancel", "取消"), None, theme).clicked() {
        close = true;
    }
    let confirm_rect = Rect::from_min_size(
        Pos2::new(rect.right() - 112.0, rect.bottom() - 48.0),
        Vec2::new(90.0, 30.0),
    );
    let confirm_label = match &dialog {
        DialogState::NewBranch { .. } => crate::tr!(model, "Create", "创建"),
        DialogState::NewWorktree { .. } => crate::tr!(model, "Create", "创建"),
        DialogState::CheckoutRevision { .. } => crate::tr!(model, "Checkout", "检出"),
        DialogState::RemoveWorktree { dirty: true, .. } => {
            crate::tr!(model, "Force remove", "强制删除")
        }
        DialogState::RemoveWorktree { dirty: false, .. } => {
            crate::tr!(model, "Remove", "删除")
        }
        DialogState::DeleteBranch { force, .. } => {
            if *force {
                crate::tr!(model, "Force delete", "强制删除")
            } else {
                crate::tr!(model, "Delete", "删除")
            }
        }
        DialogState::RenameBranch { .. } => crate::tr!(model, "Rename", "重命名"),
        DialogState::Push { .. } => crate::tr!(model, "Push", "推送"),
        DialogState::Pull { .. } => crate::tr!(model, "Pull", "拉取"),
        DialogState::Remotes { .. } => crate::tr!(model, "Done", "完成"),
        DialogState::Reset { .. } => crate::tr!(model, "Reset", "重置"),
        DialogState::NewTag { .. } => crate::tr!(model, "Create", "创建"),
        DialogState::Quit { restart_update, .. } => {
            if *restart_update {
                crate::tr!(model, "Restart now", "立即重启")
            } else {
                crate::tr!(model, "Quit", "退出")
            }
        }
        DialogState::CloseDirtyTab { .. } => crate::tr!(model, "Discard", "放弃修改"),
        DialogState::ConfirmTerminalClose { .. } => {
            crate::tr!(model, "Close anyway", "确定关闭")
        }
        DialogState::NewFile { .. } | DialogState::NewDirectory { .. } => {
            crate::tr!(model, "Create", "创建")
        }
        DialogState::RenamePath { .. } | DialogState::RenameShelf { .. } => {
            crate::tr!(model, "Rename", "重命名")
        }
        DialogState::DeletePath { .. }
        | DialogState::ConfirmDeleteUntracked { .. }
        | DialogState::ConfirmDeleteShelf { .. } => {
            crate::tr!(model, "Delete", "删除")
        }
        DialogState::NewShelf { .. } => crate::tr!(model, "Save", "保存"),
        DialogState::GoToLine { .. } => crate::tr!(model, "Go", "跳转"),
        DialogState::RenameSymbol { .. } => crate::tr!(model, "Rename", "重命名"),
        DialogState::RenameTerminal { .. } => crate::tr!(model, "Rename", "重命名"),
        DialogState::NewChangelist { .. } => crate::tr!(model, "Create", "创建"),
        DialogState::StashCreate { .. } => crate::tr!(model, "Stash", "贮藏"),
        DialogState::Unstash { .. } => crate::tr!(model, "Apply Stash", "应用贮藏"),
        DialogState::ConfirmDiscard { .. } => crate::tr!(model, "Rollback", "回滚"),
        DialogState::DeleteChangelist { .. } => crate::tr!(model, "Delete", "删除"),
        DialogState::Unshelve { .. } => crate::tr!(model, "Unshelve", "恢复"),
        DialogState::CommitFiles { .. } => crate::tr!(model, "Commit", "提交"),
        DialogState::ExtractChanges { .. } => crate::tr!(model, "Extract", "提取"),
        DialogState::GetFileFromRevision { .. } => crate::tr!(model, "Restore", "恢复"),
        DialogState::CherryPickChanges { .. } => crate::tr!(model, "Cherry-pick", "拣选"),
        _ => "Discard",
    };
    if primary_button(ui, confirm_rect, confirm_label, None, true, theme).clicked() {
        confirm = true;
    }
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

fn paint_wrapped_text(ui: &Ui, rect: Rect, text: &str, font: FontId, color: Color32) {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = rect.width();
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: font,
            color,
            ..Default::default()
        },
    );
    let galley = ui.ctx().fonts_mut(|fonts| fonts.layout_job(job));
    ui.painter().galley(rect.left_top(), galley, color);
}
