//! Dialog orchestration: chrome, the per-variant body dispatch, the shared
//! footer and the submit actions. Manager windows live in `managers`.

use egui::{Align2, Color32, FontId, Key, Pos2, Rect, Ui, Vec2};
use lazecode_core::{AppModel, Command, TabKind, ToastKind};

use crate::runtime::{DialogState, UiRuntime};
use crate::theme::Theme;
use crate::widgets::{primary_button, secondary_button};

use super::conflict_tool::draw_conflict_tool;
use super::frame;

mod actions;
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
    actions::finish_dialog(model, rt, dialog, confirm, confirm_add_remote, close);
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
