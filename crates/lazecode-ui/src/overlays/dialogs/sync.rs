//! Push / pull / remotes / reset / tag dialog bodies.

use egui::{Align2, FontId, Key, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2};
use lazecode_core::{AppModel, Command};

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

/// Draw the body of `DialogState::Pull`. Returns `(close, confirm)`.
pub(super) fn draw_pull(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    dialog: &mut DialogState,
) -> (bool, bool) {
    let DialogState::Pull { mode, status, .. } = dialog else {
        return (false, false);
    };
    let p = theme.palette;
    let close = false;
    let confirm = false;

    let modes = [
        (
            lazecode_git::PullMode::Merge,
            crate::tr!(model, "Merge", "合并"),
        ),
        (
            lazecode_git::PullMode::Rebase,
            crate::tr!(model, "Rebase", "变基"),
        ),
        (
            lazecode_git::PullMode::FastForwardOnly,
            crate::tr!(model, "Fast-forward only", "仅快进"),
        ),
    ];
    for (candidate, label) in modes {
        let row = Rect::from_min_size(
            Pos2::new(
                rect.left() + 22.0,
                rect.top() + 96.0 + (candidate as usize as f32) * 28.0,
            ),
            Vec2::new(rect.width() - 44.0, 26.0),
        );
        if ui
            .interact(
                row,
                ui.scope_id().with(("pullmode", candidate as u8)),
                Sense::click(),
            )
            .clicked()
        {
            *mode = candidate;
        }
        let selected = *mode == candidate;
        ui.painter().circle_stroke(
            Pos2::new(row.left() + 10.0, row.center().y),
            5.0,
            Stroke::new(1.3, if selected { p.accent } else { p.text3 }),
        );
        if selected {
            ui.painter()
                .circle_filled(Pos2::new(row.left() + 10.0, row.center().y), 2.4, p.accent);
        }
        ui.painter().text(
            Pos2::new(row.left() + 22.0, row.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(10.5),
            p.text,
        );
    }
    if let Some(status) = status {
        ui.painter().text(
            Pos2::new(rect.left() + 22.0, rect.bottom() - 66.0),
            Align2::LEFT_CENTER,
            status,
            FontId::proportional(9.5),
            p.rose,
        );
    }

    (close, confirm)
}

/// Draw the body of `DialogState::Remotes`. Returns `(close, confirm)` and
/// arms `add_remote` when Add/Save was clicked so the footer applies it.
pub(super) fn draw_remotes(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    dialog: &mut DialogState,
    add_remote: &mut bool,
) -> (bool, bool) {
    let DialogState::Remotes {
        remotes,
        new_name,
        new_url,
        editing,
        error,
    } = dialog
    else {
        return (false, false);
    };
    let p = theme.palette;
    let mut close = false;
    let mut confirm = false;

    let rows = Rect::from_min_max(
        Pos2::new(rect.left() + 10.0, rect.top() + 88.0),
        Pos2::new(rect.right() - 10.0, rect.bottom() - 96.0),
    );
    ui.painter().text(
        Pos2::new(rows.left(), rect.top() + 78.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Existing remotes", "现有远程"),
        FontId::proportional(9.5),
        p.text3,
    );
    let mut edit_remote: Option<(String, String)> = None;
    let mut remove_remote: Option<String> = None;
    crate::app_ui::region(ui, rows, "remotes-rows", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("remotes-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for list_remote in remotes.iter() {
                    let (row, _) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::hover());
                    ui.painter().text(
                        Pos2::new(row.left() + 6.0, row.center().y - 6.0),
                        Align2::LEFT_CENTER,
                        &list_remote.name,
                        FontId::proportional(10.5),
                        p.text,
                    );
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(row.left() + 6.0, row.center().y + 3.0),
                            Pos2::new(row.right() - 92.0, row.bottom()),
                        ),
                        &list_remote.url,
                        FontId::monospace(8.5),
                        p.text3,
                        Align2::LEFT_CENTER,
                    );
                    let edit_rect = Rect::from_center_size(
                        Pos2::new(row.right() - 74.0, row.center().y),
                        Vec2::new(38.0, 20.0),
                    );
                    let delete_rect = Rect::from_center_size(
                        Pos2::new(row.right() - 34.0, row.center().y),
                        Vec2::new(38.0, 20.0),
                    );
                    let edit_resp = ui.interact(
                        edit_rect,
                        ui.scope_id().with(("edit-remote", &list_remote.name)),
                        Sense::click(),
                    );
                    let delete_resp = ui.interact(
                        delete_rect,
                        ui.scope_id().with(("delete-remote", &list_remote.name)),
                        Sense::click(),
                    );
                    for (rect, hovered, label, color) in [
                        (
                            edit_rect,
                            edit_resp.hovered(),
                            crate::tr!(model, "Edit", "编辑"),
                            p.accent,
                        ),
                        (
                            delete_rect,
                            delete_resp.hovered(),
                            crate::tr!(model, "Delete", "删除"),
                            p.rose,
                        ),
                    ] {
                        if hovered {
                            ui.painter()
                                .rect_filled(rect, egui::CornerRadius::same(3), p.hover);
                        }
                        ui.painter().text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            label,
                            FontId::proportional(9.0),
                            color,
                        );
                    }
                    if edit_resp.clicked() {
                        edit_remote = Some((list_remote.name.clone(), list_remote.url.clone()));
                    } else if delete_resp.clicked() {
                        remove_remote = Some(list_remote.name.clone());
                    }
                }
            });
    });
    if let Some((name, url)) = edit_remote {
        *editing = Some(name.clone());
        *new_name = name;
        *new_url = url;
    }
    if let Some(name) = remove_remote {
        model.queue(Command::RemoteRemove(name));
        close = true;
    }
    let name_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 12.0, rect.bottom() - 92.0),
        Vec2::new(120.0, 26.0),
    );
    ui.painter()
        .rect_filled(name_rect, egui::CornerRadius::same(4), p.panel2);
    let _ = ui.put(
        name_rect.shrink2(Vec2::new(6.0, 3.0)),
        TextEdit::singleline(new_name)
            .frame(egui::Frame::NONE)
            .align(egui::Align2::LEFT_CENTER)
            .hint_text(crate::tr!(model, "name", "名称"))
            .font(FontId::monospace(10.0)),
    );
    let url_width = rect.width() - 12.0 - 120.0 - 8.0 - 74.0;
    let url_rect = Rect::from_min_size(
        Pos2::new(name_rect.right() + 8.0, name_rect.top()),
        Vec2::new(url_width, 26.0),
    );
    ui.painter()
        .rect_filled(url_rect, egui::CornerRadius::same(4), p.panel2);
    let _ = ui.put(
        url_rect.shrink2(Vec2::new(6.0, 3.0)),
        TextEdit::singleline(new_url)
            .frame(egui::Frame::NONE)
            .align(egui::Align2::LEFT_CENTER)
            .hint_text("https://github.com/user/repo.git")
            .font(FontId::monospace(9.5)),
    );
    let add_rect = Rect::from_min_size(
        Pos2::new(url_rect.right() + 8.0, url_rect.top()),
        Vec2::new(64.0, 26.0),
    );
    let form_label = if editing.is_some() {
        crate::tr!(model, "Save", "保存")
    } else {
        crate::tr!(model, "Add", "添加")
    };
    if crate::widgets::primary_button(ui, add_rect, form_label, None, true, theme).clicked() {
        *add_remote = true;
        // The footer's `if confirm` match applies the add; without also
        // setting `confirm`, the Add/Save click only armed a flag that was
        // then dropped at the end of the frame (the button looked alive but
        // did nothing).
        confirm = true;
    }
    if let Some(err) = error {
        ui.painter().text(
            Pos2::new(rect.left() + 22.0, rect.bottom() - 66.0),
            Align2::LEFT_CENTER,
            err,
            FontId::proportional(9.5),
            p.rose,
        );
    }

    (close, confirm)
}

/// Draw the body of `DialogState::Reset`. Returns `(close, confirm)`.
pub(super) fn draw_reset(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    dialog: &mut DialogState,
) -> (bool, bool) {
    let DialogState::Reset { hash, mode } = dialog else {
        return (false, false);
    };
    let p = theme.palette;
    let close = false;
    let confirm = false;

    ui.painter().text(
        Pos2::new(rect.left() + 22.0, rect.top() + 92.0),
        Align2::LEFT_CENTER,
        hash,
        FontId::monospace(11.0),
        p.text,
    );
    let modes = [
        (
            lazecode_git::ResetMode::Soft,
            crate::tr!(
                model,
                "Soft — keep changes staged",
                "Soft — 保留更改（已暂存）"
            ),
        ),
        (
            lazecode_git::ResetMode::Mixed,
            crate::tr!(model, "Mixed — unstage changes", "Mixed — 取消暂存"),
        ),
        (
            lazecode_git::ResetMode::Hard,
            crate::tr!(model, "Hard — discard all changes", "Hard — 丢弃全部更改"),
        ),
    ];
    for (candidate, label) in modes {
        let row = Rect::from_min_size(
            Pos2::new(
                rect.left() + 22.0,
                rect.top() + 120.0 + (candidate as usize as f32) * 28.0,
            ),
            Vec2::new(rect.width() - 44.0, 26.0),
        );
        if ui
            .interact(
                row,
                ui.scope_id().with(("resetmode", candidate as u8)),
                Sense::click(),
            )
            .clicked()
        {
            *mode = candidate;
        }
        let selected = *mode == candidate;
        ui.painter().circle_stroke(
            Pos2::new(row.left() + 10.0, row.center().y),
            5.0,
            Stroke::new(1.3, if selected { p.accent } else { p.text3 }),
        );
        if selected {
            ui.painter()
                .circle_filled(Pos2::new(row.left() + 10.0, row.center().y), 2.4, p.accent);
        }
        ui.painter().text(
            Pos2::new(row.left() + 22.0, row.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(10.5),
            p.text,
        );
    }

    (close, confirm)
}

/// Draw the body of `DialogState::NewTag`. Returns `(close, confirm)`.
pub(super) fn draw_new_tag(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    dialog: &mut DialogState,
) -> (bool, bool) {
    let DialogState::NewTag {
        commit: commit_hint,
        value,
        error,
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
        commit_hint,
        FontId::monospace(10.5),
        p.accent,
    );
    let input = Rect::from_min_size(
        Pos2::new(rect.left() + 22.0, rect.top() + 118.0),
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
            .hint_text("v0.1.0")
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
