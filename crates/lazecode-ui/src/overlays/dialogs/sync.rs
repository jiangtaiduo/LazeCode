//! Push dialog body: remote picker, branch row, force/upstream/tags options
//! and the pending-commit list.

use egui::{Align2, FontId, Key, Pos2, Rect, Sense, Ui, Vec2};
use lazecode_core::AppModel;

use crate::runtime::DialogState;
use crate::theme::Theme;
use crate::widgets::{checkbox, text_clipped};

use super::super::remotes_width;

/// Draw the body of `DialogState::Push`. Returns `(close, confirm)`.
pub(super) fn draw_push(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    dialog: &mut DialogState,
) -> (bool, bool) {
    let DialogState::Push {
        remote,
        branch,
        set_upstream,
        force,
        push_tags,
        remotes,
        pending,
        status,
    } = dialog
    else {
        return (false, false);
    };
    let p = theme.palette;
    let close = false;
    let mut confirm = false;

    ui.painter().text(
        Pos2::new(rect.left() + 22.0, rect.top() + 92.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Remote", "远程"),
        FontId::proportional(9.5),
        p.text3,
    );
    let remote_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 90.0, rect.top() + 78.0),
        Vec2::new(210.0, 26.0),
    );
    if remotes.is_empty() {
        ui.painter().text(
            Pos2::new(remote_rect.left() + 8.0, remote_rect.center().y),
            Align2::LEFT_CENTER,
            remote.as_str(),
            FontId::monospace(11.0),
            p.text,
        );
    } else {
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(remote_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                egui::ComboBox::from_id_salt("push-remote")
                    .width(remotes_width(remotes, remote_rect.width()))
                    .selected_text(egui::RichText::new(remote.as_str()).color(p.text))
                    .show_ui(ui, |ui| {
                        for candidate in remotes.iter() {
                            if ui
                                .selectable_label(remote == &candidate.name, &candidate.name)
                                .clicked()
                            {
                                *remote = candidate.name.clone();
                            }
                        }
                    });
            },
        );
    }
    ui.painter().text(
        Pos2::new(rect.left() + 22.0, rect.top() + 124.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Branch", "分支"),
        FontId::proportional(9.5),
        p.text3,
    );
    ui.painter().text(
        Pos2::new(rect.left() + 90.0, rect.top() + 124.0),
        Align2::LEFT_CENTER,
        branch,
        FontId::monospace(11.0),
        p.text,
    );
    let option_y = rect.top() + 158.0;
    let mut option = |x: f32, value: &mut bool, label: &str| {
        let check = Rect::from_center_size(
            Pos2::new(rect.left() + x + 8.0, option_y),
            Vec2::splat(16.0),
        );
        if checkbox(ui, check, *value, theme).clicked() {
            *value = !*value;
        }
        ui.painter().text(
            Pos2::new(check.right() + 6.0, option_y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(10.5),
            p.text2,
        );
    };
    option(
        22.0,
        set_upstream,
        crate::tr!(model, "Set upstream", "建立 upstream"),
    );
    option(
        190.0,
        force,
        crate::tr!(model, "Force (with lease)", "强制（with lease）"),
    );
    option(382.0, push_tags, crate::tr!(model, "Push tags", "推送标签"));

    ui.painter().text(
        Pos2::new(rect.left() + 22.0, rect.top() + 196.0),
        Align2::LEFT_CENTER,
        format!(
            "{} {}",
            crate::tr!(model, "Commits to push:", "待推送提交："),
            pending.len()
        ),
        FontId::proportional(10.5),
        p.text3,
    );
    let list_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 18.0, rect.top() + 208.0),
        Pos2::new(rect.right() - 18.0, rect.bottom() - 72.0),
    );
    crate::app_ui::region(ui, list_rect, "push-pending", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("push-pending-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if pending.is_empty() {
                    ui.label(
                        egui::RichText::new(crate::tr!(
                            model,
                            "Everything is already pushed.",
                            "没有待推送的提交。"
                        ))
                        .color(p.text3)
                        .size(10.5),
                    );
                }
                for commit in pending.iter() {
                    let (row, _) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::hover());
                    ui.painter().text(
                        Pos2::new(row.left() + 4.0, row.center().y),
                        Align2::LEFT_CENTER,
                        &commit.short,
                        FontId::monospace(9.5),
                        p.accent,
                    );
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(row.left() + 66.0, row.top()),
                            Pos2::new(row.right() - 160.0, row.bottom()),
                        ),
                        &commit.message,
                        FontId::proportional(10.5),
                        p.text,
                        Align2::LEFT_CENTER,
                    );
                    ui.painter().text(
                        Pos2::new(row.right() - 4.0, row.center().y),
                        Align2::RIGHT_CENTER,
                        &commit.author,
                        FontId::proportional(9.0),
                        p.text3,
                    );
                }
            });
    });

    if let Some(status) = status {
        ui.painter().text(
            Pos2::new(rect.left() + 22.0, rect.bottom() - 58.0),
            Align2::LEFT_CENTER,
            status,
            FontId::proportional(9.5),
            p.rose,
        );
    }
    if ui.input(|input| input.key_pressed(Key::Enter)) {
        confirm = true;
    }

    (close, confirm)
}
