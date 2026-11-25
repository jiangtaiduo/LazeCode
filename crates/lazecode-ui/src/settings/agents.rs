//! Agents settings section.

use super::prelude::*;
use super::widgets::*;

pub(super) fn agents(ui: &mut Ui, model: &mut AppModel, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    heading(
        ui,
        theme,
        rect,
        crate::tr!(model, "Coding Agents", "编码智能体"),
        crate::tr!(
            model,
            "Entries shown in the Coding Agent menu. An empty command opens a login shell.",
            "编码智能体菜单中的选项；命令留空时打开登录 Shell。"
        ),
    );

    let header_y = rect.top() + 66.0;
    ui.painter().text(
        Pos2::new(rect.left(), header_y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Name", "名称"),
        FontId::proportional(10.5),
        p.text3,
    );
    ui.painter().text(
        Pos2::new(rect.left() + 186.0, header_y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Command / script", "命令 / 脚本"),
        FontId::proportional(10.5),
        p.text3,
    );

    let list = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top() + 80.0),
        Pos2::new(rect.right(), rect.bottom() - 48.0),
    );
    let mut remove = None;
    let mut save = false;
    crate::app_ui::region(ui, list, "settings-agents", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("settings-agents-list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for (index, option) in model.settings.agents.options.iter_mut().enumerate() {
                    let (row, _) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 38.0), Sense::hover());
                    let name_rect = Rect::from_min_size(
                        Pos2::new(row.left(), row.top() + 5.0),
                        Vec2::new(174.0, 28.0),
                    );
                    ui.painter()
                        .rect_filled(name_rect, egui::CornerRadius::same(4), p.panel2);
                    let command_rect = Rect::from_min_size(
                        Pos2::new(row.left() + 182.0, row.top() + 5.0),
                        Vec2::new((row.width() - 214.0).max(80.0), 28.0),
                    );
                    ui.painter()
                        .rect_filled(command_rect, egui::CornerRadius::same(4), p.panel2);
                    let name_response = ui.put(
                        name_rect,
                        TextEdit::singleline(&mut option.name)
                            .frame(egui::Frame::NONE)
                            .align(egui::Align2::LEFT_CENTER)
                            .font(FontId::proportional(11.5))
                            .hint_text(crate::tr!(model, "Agent name", "智能体名称")),
                    );
                    ui.painter().rect_stroke(
                        name_rect,
                        egui::CornerRadius::same(4),
                        Stroke::new(1.0, p.border_soft),
                        StrokeKind::Inside,
                    );
                    let command_response = ui.put(
                        command_rect,
                        TextEdit::singleline(&mut option.command)
                            .frame(egui::Frame::NONE)
                            .align(egui::Align2::LEFT_CENTER)
                            .font(FontId::monospace(11.0))
                            .hint_text(crate::tr!(model, "login shell", "登录 Shell")),
                    );
                    ui.painter().rect_stroke(
                        command_rect,
                        egui::CornerRadius::same(4),
                        Stroke::new(1.0, p.border_soft),
                        StrokeKind::Inside,
                    );
                    if name_response.lost_focus() || command_response.lost_focus() {
                        save = true;
                    }
                    let delete_rect = Rect::from_center_size(
                        Pos2::new(row.right() - 12.0, row.center().y),
                        Vec2::splat(22.0),
                    );
                    if icon_button(ui, delete_rect, Icon::X, false, theme)
                        .on_hover_text(crate::tr!(model, "Remove", "移除"))
                        .clicked()
                    {
                        remove = Some(index);
                    }
                }
            });
    });
    if let Some(index) = remove {
        if index < model.settings.agents.options.len() {
            model.settings.agents.options.remove(index);
            model.save_settings();
        }
    } else if save {
        model.save_settings();
    }

    let add = Rect::from_min_size(
        Pos2::new(rect.left(), rect.bottom() - 34.0),
        Vec2::new(104.0, 28.0),
    );
    if secondary_button(
        ui,
        add,
        crate::tr!(model, "Add agent", "添加智能体"),
        Some(Icon::Plus),
        theme,
    )
    .clicked()
    {
        model
            .settings
            .agents
            .options
            .push(lazecode_core::settings::AgentProfile {
                name: crate::tr!(model, "New agent", "新智能体").to_string(),
                description: String::new(),
                command: String::new(),
            });
        model.save_settings();
    }
    let reset = Rect::from_min_size(
        Pos2::new(add.right() + 8.0, rect.bottom() - 34.0),
        Vec2::new(132.0, 28.0),
    );
    if secondary_button(
        ui,
        reset,
        crate::tr!(model, "Restore defaults", "恢复默认"),
        None,
        theme,
    )
    .clicked()
    {
        model.settings.agents = lazecode_core::settings::AgentSettings::default();
        model.save_settings();
    }
}
