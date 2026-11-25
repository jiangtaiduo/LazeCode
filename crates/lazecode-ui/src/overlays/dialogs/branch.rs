//! Branch / worktree / tab dialog bodies: create, checkout, delete, rename,
//! remove worktree and close-dirty-tab.

use egui::{Align2, FontId, Key, Pos2, Rect, Stroke, StrokeKind, TextEdit, Ui, Vec2};
use lazecode_core::AppModel;

use crate::runtime::{DialogState, UiRuntime};
use crate::theme::Theme;
use crate::widgets::{checkbox, text_clipped};

/// Draw the body of `DialogState::NewBranch`. Returns `(close, confirm)`.
pub(super) fn draw_new_branch(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    dialog: &mut DialogState,
) -> (bool, bool) {
    let DialogState::NewBranch {
        value,
        start_point,
        checkout,
        error,
    } = dialog
    else {
        return (false, false);
    };
    let p = theme.palette;
    let close = false;
    let mut confirm = false;

    let start_label = start_point.as_deref().unwrap_or("HEAD");
    ui.painter().text(
        Pos2::new(rect.left() + 22.0, rect.top() + 92.0),
        Align2::LEFT_CENTER,
        format!(
            "{} {}",
            crate::tr!(model, "Start point:", "起点："),
            start_label
        ),
        FontId::monospace(10.5),
        p.text3,
    );
    let input = Rect::from_min_size(
        Pos2::new(rect.left() + 22.0, rect.top() + 112.0),
        Vec2::new(rect.width() - 44.0, 30.0),
    );
    ui.painter()
        .rect_filled(input, egui::CornerRadius::same(5), p.panel2);
    ui.painter().rect_stroke(
        input,
        egui::CornerRadius::same(5),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    let response = ui.put(
        input.shrink2(Vec2::new(8.0, 4.0)),
        TextEdit::singleline(value)
            .frame(egui::Frame::NONE)
            .align(egui::Align2::LEFT_CENTER)
            .hint_text("feat/new-branch")
            .font(FontId::monospace(11.0)),
    );
    if rt.branch_create_error.is_none() {
        response.request_focus();
    }
    let check = Rect::from_center_size(
        Pos2::new(rect.left() + 30.0, rect.top() + 166.0),
        Vec2::splat(16.0),
    );
    if checkbox(ui, check, *checkout, theme).clicked() {
        *checkout = !*checkout;
    }
    ui.painter().text(
        Pos2::new(rect.left() + 44.0, rect.top() + 166.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Checkout after create", "创建后检出"),
        FontId::proportional(10.5),
        p.text2,
    );
    if let Some(err) = error {
        ui.painter().text(
            Pos2::new(rect.left() + 22.0, rect.top() + 196.0),
            Align2::LEFT_CENTER,
            err,
            FontId::proportional(9.5),
            p.rose,
        );
    }
    if ui.input(|input| input.key_pressed(Key::Enter)) {
        confirm = true;
    }

    (close, confirm)
}

/// Draw the body of `DialogState::CheckoutRevision`.
pub(super) fn draw_checkout_revision(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    dialog: &mut DialogState,
) -> (bool, bool) {
    let DialogState::CheckoutRevision { value, error } = dialog else {
        return (false, false);
    };
    let p = theme.palette;
    let close = false;
    let mut confirm = false;

    let input = Rect::from_min_size(
        Pos2::new(rect.left() + 22.0, rect.top() + 94.0),
        Vec2::new(rect.width() - 44.0, 30.0),
    );
    ui.painter()
        .rect_filled(input, egui::CornerRadius::same(5), p.panel2);
    ui.painter().rect_stroke(
        input,
        egui::CornerRadius::same(5),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    let response = ui.put(
        input.shrink2(Vec2::new(8.0, 4.0)),
        TextEdit::singleline(value)
            .frame(egui::Frame::NONE)
            .align(egui::Align2::LEFT_CENTER)
            .hint_text("v1.0.0 or commit hash or branch")
            .font(FontId::monospace(11.0)),
    );
    response.request_focus();
    if let Some(err) = error {
        ui.painter().text(
            Pos2::new(input.left(), input.bottom() + 12.0),
            Align2::LEFT_CENTER,
            err,
            FontId::proportional(9.5),
            p.rose,
        );
    }
    if ui.input(|input| input.key_pressed(Key::Enter)) {
        confirm = true;
    }

    (close, confirm)
}

/// Draw the body of `DialogState::DeleteBranch`.
pub(super) fn draw_delete_branch(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    dialog: &mut DialogState,
) -> (bool, bool) {
    let DialogState::DeleteBranch {
        name,
        force,
        remote,
        error,
    } = dialog
    else {
        return (false, false);
    };
    let p = theme.palette;
    let close = false;
    let confirm = false;

    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(rect.left() + 22.0, rect.top() + 96.0),
            Pos2::new(rect.right() - 22.0, rect.top() + 118.0),
        ),
        name,
        FontId::monospace(11.0),
        p.amber,
        Align2::LEFT_CENTER,
    );
    if *remote {
        ui.painter().text(
            Pos2::new(rect.left() + 22.0, rect.top() + 142.0),
            Align2::LEFT_CENTER,
            crate::tr!(
                model,
                "Remote deletion uses push --delete.",
                "远程删除使用 push --delete。"
            ),
            FontId::proportional(9.5),
            p.text3,
        );
    } else if *force {
        ui.painter().text(
            Pos2::new(rect.left() + 22.0, rect.top() + 142.0),
            Align2::LEFT_CENTER,
            crate::tr!(
                model,
                "This bypasses the merged check.",
                "此操作会跳过合并检查。"
            ),
            FontId::proportional(9.5),
            p.rose,
        );
    }
    if let Some(err) = error {
        ui.painter().text(
            Pos2::new(rect.left() + 22.0, rect.top() + 166.0),
            Align2::LEFT_CENTER,
            err,
            FontId::proportional(9.5),
            p.rose,
        );
    }

    (close, confirm)
}

/// Draw the body of `DialogState::RenameBranch`.
pub(super) fn draw_rename_branch(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    dialog: &mut DialogState,
) -> (bool, bool) {
    let DialogState::RenameBranch { from, value, error } = dialog else {
        return (false, false);
    };
    let p = theme.palette;
    let close = false;
    let mut confirm = false;

    let input = Rect::from_min_size(
        Pos2::new(rect.left() + 22.0, rect.top() + 90.0),
        Vec2::new(rect.width() - 44.0, 30.0),
    );
    ui.painter()
        .rect_filled(input, egui::CornerRadius::same(5), p.panel2);
    ui.painter().rect_stroke(
        input,
        egui::CornerRadius::same(5),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    let response = ui.put(
        input.shrink2(Vec2::new(8.0, 4.0)),
        TextEdit::singleline(value)
            .frame(egui::Frame::NONE)
            .align(egui::Align2::LEFT_CENTER)
            .hint_text(from.clone())
            .font(FontId::monospace(11.0)),
    );
    response.request_focus();
    if let Some(err) = error {
        ui.painter().text(
            Pos2::new(input.left(), input.bottom() + 12.0),
            Align2::LEFT_CENTER,
            err,
            FontId::proportional(9.5),
            p.rose,
        );
    }
    if ui.input(|input| input.key_pressed(Key::Enter)) {
        confirm = true;
    }

    (close, confirm)
}

/// Draw the body of `DialogState::RemoveWorktree`.
pub(super) fn draw_remove_worktree(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    dialog: &mut DialogState,
) -> (bool, bool) {
    let DialogState::RemoveWorktree {
        name,
        worktree_path,
        dirty,
        changes,
    } = dialog
    else {
        return (false, false);
    };
    let p = theme.palette;
    let close = false;
    let confirm = false;

    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(rect.left() + 22.0, rect.top() + 96.0),
            Pos2::new(rect.right() - 22.0, rect.top() + 112.0),
        ),
        name,
        FontId::monospace(11.0),
        p.amber,
        Align2::LEFT_CENTER,
    );
    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(rect.left() + 22.0, rect.top() + 114.0),
            Pos2::new(rect.right() - 22.0, rect.top() + 130.0),
        ),
        &worktree_path.display().to_string(),
        FontId::monospace(10.0),
        p.text3,
        Align2::LEFT_CENTER,
    );
    if *dirty {
        let detail = if *changes > 0 {
            if model.settings.language.is_zh() {
                format!("{changes} 个文件有未提交的更改，将一并丢弃")
            } else {
                format!(
                    "{changes} uncommitted change{} will be discarded",
                    if *changes == 1 { "" } else { "s" }
                )
            }
        } else if model.settings.language.is_zh() {
            "检测到未提交的更改，将一并丢弃".to_string()
        } else {
            "Uncommitted changes detected and will be discarded".to_string()
        };
        ui.painter().text(
            Pos2::new(rect.left() + 22.0, rect.top() + 150.0),
            Align2::LEFT_CENTER,
            detail,
            FontId::proportional(10.5),
            p.amber,
        );
    }

    (close, confirm)
}

/// Draw the body of `DialogState::CloseDirtyTab`.
pub(super) fn draw_close_dirty_tab(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    dialog: &mut DialogState,
) -> (bool, bool) {
    let DialogState::CloseDirtyTab { name, .. } = dialog else {
        return (false, false);
    };
    let p = theme.palette;
    let close = false;
    let confirm = false;

    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(rect.left() + 22.0, rect.top() + 96.0),
            Pos2::new(rect.right() - 22.0, rect.top() + 118.0),
        ),
        name,
        FontId::monospace(11.0),
        p.amber,
        Align2::LEFT_CENTER,
    );

    (close, confirm)
}
