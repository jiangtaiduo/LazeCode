//! Stash / unshelve / discard / changelist dialog bodies.

use egui::{Align2, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2};
use lazecode_core::AppModel;

use crate::runtime::DialogState;
use crate::theme::Theme;
use crate::widgets::{checkbox, text_clipped};

/// Draw the body of `DialogState::StashCreate`. Returns `(close, confirm)`.
pub(super) fn draw_stash_create(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    dialog: &mut DialogState,
) -> (bool, bool) {
    let DialogState::StashCreate {
        message,
        include_untracked,
        error,
    } = dialog
    else {
        return (false, false);
    };
    let p = theme.palette;
    let close = false;
    let confirm = false;

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
        TextEdit::singleline(message)
            .frame(egui::Frame::NONE)
            .align(egui::Align2::LEFT_CENTER)
            .hint_text(crate::tr!(
                model,
                "Stash message (optional)",
                "贮藏说明（可选）"
            ))
            .font(FontId::monospace(11.0)),
    );
    response.request_focus();
    let check_rect = Rect::from_center_size(
        Pos2::new(rect.left() + 30.0, input.bottom() + 22.0),
        Vec2::splat(16.0),
    );
    if checkbox(ui, check_rect, *include_untracked, theme).clicked() {
        *include_untracked = !*include_untracked;
    }
    ui.painter().text(
        Pos2::new(rect.left() + 42.0, input.bottom() + 22.0),
        Align2::LEFT_CENTER,
        crate::tr!(
            model,
            "Include untracked files (git stash -u)",
            "包含未跟踪文件（git stash -u）"
        ),
        FontId::proportional(11.0),
        p.text2,
    );
    if let Some(err) = error {
        ui.painter().text(
            Pos2::new(input.left(), input.bottom() + 44.0),
            Align2::LEFT_CENTER,
            err,
            FontId::proportional(9.5),
            p.rose,
        );
    }

    (close, confirm)
}

/// Draw the body of `DialogState::Unstash`. Returns `(close, confirm)`.
pub(super) fn draw_unstash(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    dialog: &mut DialogState,
) -> (bool, bool) {
    let DialogState::Unstash {
        branch,
        as_new_branch,
        pop,
        reinstate_index,
        error,
        ..
    } = dialog
    else {
        return (false, false);
    };
    let p = theme.palette;
    let close = false;
    let confirm = false;

    ui.painter().text(
        Pos2::new(rect.left() + 22.0, rect.top() + 92.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Current branch:", "当前分支:"),
        FontId::proportional(11.5),
        p.text3,
    );
    ui.painter().text(
        Pos2::new(rect.left() + 130.0, rect.top() + 92.0),
        Align2::LEFT_CENTER,
        if branch.is_empty() {
            "—"
        } else {
            branch.as_str()
        },
        FontId::proportional(11.5),
        p.text,
    );
    ui.painter().text(
        Pos2::new(rect.left() + 22.0, rect.top() + 128.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "As new branch:", "作为新分支:"),
        FontId::proportional(11.5),
        p.text3,
    );
    let input = Rect::from_min_size(
        Pos2::new(rect.left() + 130.0, rect.top() + 113.0),
        Vec2::new(rect.width() - 152.0, 28.0),
    );
    ui.painter()
        .rect_filled(input, egui::CornerRadius::same(4), p.panel2);
    ui.painter().rect_stroke(
        input,
        egui::CornerRadius::same(4),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    ui.put(
        input.shrink2(Vec2::new(8.0, 4.0)),
        TextEdit::singleline(as_new_branch)
            .frame(egui::Frame::NONE)
            .align(egui::Align2::LEFT_CENTER)
            .hint_text(crate::tr!(
                model,
                "Leave empty to stay on the current branch",
                "留空则停留在当前分支"
            ))
            .font(FontId::monospace(11.0)),
    );
    let pop_check = Rect::from_center_size(
        Pos2::new(rect.left() + 30.0, rect.top() + 170.0),
        Vec2::splat(16.0),
    );
    if checkbox(ui, pop_check, *pop, theme).clicked() {
        *pop = !*pop;
    }
    ui.painter().text(
        Pos2::new(rect.left() + 42.0, rect.top() + 170.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Pop stash", "弹出贮藏"),
        FontId::proportional(11.0),
        p.text2,
    );
    let index_check = Rect::from_center_size(
        Pos2::new(rect.left() + 30.0, rect.top() + 198.0),
        Vec2::splat(16.0),
    );
    if checkbox(ui, index_check, *reinstate_index, theme).clicked() {
        *reinstate_index = !*reinstate_index;
    }
    ui.painter().text(
        Pos2::new(rect.left() + 42.0, rect.top() + 198.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Reinstate index", "恢复索引"),
        FontId::proportional(11.0),
        p.text2,
    );
    if let Some(err) = error {
        ui.painter().text(
            Pos2::new(rect.left() + 22.0, rect.top() + 228.0),
            Align2::LEFT_CENTER,
            err,
            FontId::proportional(9.5),
            p.rose,
        );
    }

    (close, confirm)
}

/// Draw the body of `DialogState::ConfirmDiscard`. Returns `(close, confirm)`.
pub(super) fn draw_confirm_discard(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    dialog: &mut DialogState,
) -> (bool, bool) {
    let DialogState::ConfirmDiscard { paths } = dialog else {
        return (false, false);
    };
    let p = theme.palette;
    let close = false;
    let confirm = false;

    // 预览前若干路径，超出部分以计数收尾。
    let show = paths.len().min(4);
    for (index, path) in paths.iter().take(show).enumerate() {
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(rect.left() + 22.0, rect.top() + 96.0 + index as f32 * 18.0),
                Pos2::new(
                    rect.right() - 22.0,
                    rect.top() + 112.0 + index as f32 * 18.0,
                ),
            ),
            path,
            FontId::monospace(10.5),
            p.text2,
            Align2::LEFT_CENTER,
        );
    }
    if paths.len() > show {
        ui.painter().text(
            Pos2::new(rect.left() + 22.0, rect.top() + 96.0 + show as f32 * 18.0),
            Align2::LEFT_CENTER,
            format!("… +{}", paths.len() - show),
            FontId::monospace(10.5),
            p.text3,
        );
    }

    (close, confirm)
}

/// Draw the body of `DialogState::DeleteChangelist`. Returns `(close, confirm)`.
pub(super) fn draw_delete_changelist(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    dialog: &mut DialogState,
) -> (bool, bool) {
    let DialogState::DeleteChangelist {
        name,
        count,
        rollback,
    } = dialog
    else {
        return (false, false);
    };
    let p = theme.palette;
    let close = false;
    let confirm = false;

    ui.painter().text(
        Pos2::new(rect.left() + 22.0, rect.top() + 100.0),
        Align2::LEFT_CENTER,
        if model.settings.language.is_zh() {
            format!("「{name}」中有 {count} 个文件，删除后这些文件：")
        } else {
            format!("\"{name}\" holds {count} file(s). After deleting it, these files will:")
        },
        FontId::proportional(11.5),
        p.text2,
    );
    for (index, (label, target)) in [
        (
            crate::tr!(
                model,
                "Move into the default changelist (recommended)",
                "移入默认变更列表（推荐）"
            ),
            false,
        ),
        (
            crate::tr!(
                model,
                "Rollback (discard the uncommitted changes)",
                "回滚（放弃这些文件的未提交更改）"
            ),
            true,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 22.0, rect.top() + 126.0 + index as f32 * 24.0),
            Vec2::new(rect.width() - 44.0, 22.0),
        );
        let check = Rect::from_center_size(
            Pos2::new(row.left() + 8.0, row.center().y),
            Vec2::splat(16.0),
        );
        if checkbox(ui, check, *rollback == target, theme).clicked() {
            *rollback = target;
        }
        ui.painter().text(
            Pos2::new(row.left() + 22.0, row.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(11.0),
            if target { p.rose } else { p.text2 },
        );
    }

    (close, confirm)
}

/// Draw the body of `DialogState::Unshelve`. Returns `(close, confirm)`.
pub(super) fn draw_unshelve(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    dialog: &mut DialogState,
) -> (bool, bool) {
    let DialogState::Unshelve {
        files,
        target,
        new_name,
        error,
        ..
    } = dialog
    else {
        return (false, false);
    };
    let p = theme.palette;
    let close = false;
    let confirm = false;

    // 文件勾选清单（默认全选；Unshelve 支持部分恢复）。
    let list_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 22.0, rect.top() + 94.0),
        Pos2::new(rect.right() - 22.0, rect.top() + 214.0),
    );
    ui.painter()
        .rect_filled(list_rect, egui::CornerRadius::same(5), p.panel);
    let row_h = 22.0;
    let mut list_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(list_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    egui::ScrollArea::vertical()
        .id_salt("unshelve-files")
        .auto_shrink([false, false])
        .show(&mut list_ui, |ui| {
            ui.set_width(list_rect.width());
            for (path, selected) in files.iter_mut() {
                let (row, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::hover());
                let check = Rect::from_center_size(
                    Pos2::new(row.left() + 12.0, row.center().y),
                    Vec2::splat(14.0),
                );
                if checkbox(ui, check, *selected, theme).clicked() {
                    *selected = !*selected;
                }
                text_clipped(
                    ui.painter(),
                    Rect::from_min_max(
                        Pos2::new(row.left() + 26.0, row.top()),
                        Pos2::new(row.right() - 4.0, row.bottom()),
                    ),
                    path,
                    FontId::monospace(10.5),
                    p.text2,
                    Align2::LEFT_CENTER,
                );
            }
        });
    // 目标 changelist：现有列表单选 + 新建输入（项目可能已关闭，
    // 不 unwrap；无项目时只剩新建输入）。
    let existing_lists: Vec<String> = model
        .active_project()
        .map(|project| {
            lazecode_git::Changelists::open(project.active_root())
                .lists()
                .to_vec()
        })
        .unwrap_or_default();
    let mut y = rect.top() + 224.0;
    for name in existing_lists.iter() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 22.0, y),
            Vec2::new((rect.width() - 44.0) * 0.5, 20.0),
        );
        let check = Rect::from_center_size(
            Pos2::new(row.left() + 8.0, row.center().y),
            Vec2::splat(14.0),
        );
        let is_target = *target == Some(name.clone());
        if checkbox(ui, check, is_target, theme).clicked() {
            *target = Some(name.clone());
        }
        ui.painter().text(
            Pos2::new(row.left() + 20.0, row.center().y),
            Align2::LEFT_CENTER,
            name,
            FontId::proportional(10.5),
            p.text2,
        );
        y += 22.0;
    }
    let new_row = Rect::from_min_size(
        Pos2::new(rect.left() + 22.0, y),
        Vec2::new((rect.width() - 44.0) * 0.5, 22.0),
    );
    let check = Rect::from_center_size(
        Pos2::new(new_row.left() + 8.0, new_row.center().y),
        Vec2::splat(14.0),
    );
    let new_is_target = target.is_none();
    if checkbox(ui, check, new_is_target, theme).clicked() && !new_is_target {
        *target = None;
    }
    ui.painter().text(
        Pos2::new(new_row.left() + 20.0, new_row.center().y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "New changelist:", "新建变更列表："),
        FontId::proportional(10.5),
        p.text2,
    );
    let name_input = Rect::from_min_size(
        Pos2::new(rect.left() + 150.0, y - 2.0),
        Vec2::new(rect.right() - rect.left() - 172.0, 24.0),
    );
    ui.painter()
        .rect_filled(name_input, egui::CornerRadius::same(4), p.panel2);
    ui.painter().rect_stroke(
        name_input,
        egui::CornerRadius::same(4),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    let response = ui.put(
        name_input.shrink2(Vec2::new(6.0, 3.0)),
        TextEdit::singleline(new_name)
            .frame(egui::Frame::NONE)
            .align(egui::Align2::LEFT_CENTER)
            .hint_text(crate::tr!(model, "Name", "名称"))
            .font(FontId::monospace(10.5)),
    );
    if response.changed() && !new_name.trim().is_empty() {
        *target = None;
    }
    if let Some(err) = error {
        ui.painter().text(
            Pos2::new(rect.left() + 22.0, rect.bottom() - 58.0),
            Align2::LEFT_CENTER,
            err,
            FontId::proportional(9.5),
            p.rose,
        );
    }

    (close, confirm)
}
