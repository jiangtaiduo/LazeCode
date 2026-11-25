//! Keybindings section: rebindable actions with capture mode.

use super::prelude::*;
use super::widgets::*;

/// 设置页「快捷键」分区：列出全部可改键的动作，点击「修改」进入录入态
/// （下一次按键即生效，Esc 取消），冲突会被拒绝并弹 toast。
pub(super) fn keybindings_section(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
) {
    use crate::keybindings;

    let p = theme.palette;
    heading(
        ui,
        theme,
        rect,
        crate::tr!(model, "Shortcuts", "快捷键"),
        crate::tr!(
            model,
            "Click Change, then press the new combo. Esc cancels; duplicates are rejected.",
            "点击「修改」后按下新的组合键；Esc 取消，重复的键会被拒绝。"
        ),
    );
    let reset_all = Rect::from_min_size(
        Pos2::new(rect.right() - 130.0, rect.top() + 8.0),
        Vec2::new(130.0, 24.0),
    );
    if secondary_button(
        ui,
        reset_all,
        crate::tr!(model, "Reset all", "全部恢复默认"),
        None,
        theme,
    )
    .clicked()
    {
        model.settings.keybindings.clear();
        model.save_settings();
    }

    let mut y = rect.top() + 74.0;
    if let Some(action) = rt.keybind_capture {
        ui.painter().text(
            Pos2::new(rect.left(), y),
            Align2::LEFT_CENTER,
            format!(
                "{}{}",
                crate::tr!(
                    model,
                    "Recording: press the new combo for ",
                    "录入中：请为「"
                ),
                {
                    let name = keybindings::label(model, action);
                    if model.settings.language.is_zh() {
                        format!("{name}」")
                    } else {
                        name
                    }
                }
            ),
            FontId::proportional(11.5),
            p.accent,
        );
        y += 30.0;
    }

    // 分区都是绝对坐标绘制；把滚动区钉在 content 里剩余的空间上。
    let list_rect = Rect::from_min_max(
        Pos2::new(rect.left(), y),
        Pos2::new(rect.right(), rect.bottom()),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(list_rect)
            .id_salt("settings-keybindings"),
    );
    ScrollArea::vertical()
        .id_salt("settings-keybindings-scroll")
        .auto_shrink([false, false])
        .show(&mut child, |ui| {
            ui.add_space(4.0);
            for action in keybindings::ALL_ACTIONS {
                let (row, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::hover());
                let recording = rt.keybind_capture == Some(action);
                if recording {
                    ui.painter().rect_filled(
                        row,
                        egui::CornerRadius::same(5),
                        p.accent.gamma_multiply(0.12),
                    );
                } else if ui
                    .interact(
                        row,
                        ui.scope_id().with(("kb-row", action.id())),
                        Sense::hover(),
                    )
                    .hovered()
                {
                    ui.painter()
                        .rect_filled(row, egui::CornerRadius::same(5), p.hover);
                }
                ui.painter().text(
                    Pos2::new(row.left() + 6.0, row.center().y),
                    Align2::LEFT_CENTER,
                    keybindings::label(model, action),
                    FontId::proportional(11.5),
                    p.text,
                );
                let display = keybindings::binding_display(keybindings::resolved(model, action));
                let empty = display.is_empty();
                ui.painter().text(
                    Pos2::new(row.right() - 190.0, row.center().y),
                    Align2::RIGHT_CENTER,
                    if empty {
                        crate::tr!(model, "unbound", "未设置").to_string()
                    } else {
                        display
                    },
                    FontId::monospace(11.5),
                    if empty { p.text3 } else { p.text2 },
                );
                let change_rect = Rect::from_min_size(
                    Pos2::new(row.right() - 128.0, row.center().y - 10.0),
                    Vec2::new(58.0, 20.0),
                );
                let change_label = if recording {
                    crate::tr!(model, "Cancel", "取消")
                } else {
                    crate::tr!(model, "Change", "修改")
                };
                if secondary_button(ui, change_rect, change_label, None, theme).clicked() {
                    rt.keybind_capture = if recording { None } else { Some(action) };
                }
                let has_override = model.settings.keybindings.contains_key(action.id());
                let reset_rect = Rect::from_min_size(
                    Pos2::new(row.right() - 62.0, row.center().y - 10.0),
                    Vec2::new(58.0, 20.0),
                );
                if secondary_button_enabled(
                    ui,
                    reset_rect,
                    crate::tr!(model, "Default", "默认"),
                    None,
                    has_override,
                    theme,
                )
                .clicked()
                {
                    model.settings.keybindings.remove(action.id());
                    model.save_settings();
                }
            }
        });
}
