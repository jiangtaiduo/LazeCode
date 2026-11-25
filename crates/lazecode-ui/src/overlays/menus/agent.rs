//! Agent picker menu.

use super::prelude::*;

pub(super) fn draw_agent_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    anchor: Rect,
) {
    let p = theme.palette;
    let options = model.settings.agents.options.clone();
    let visible_rows = options.len().min(9);
    let height = 52.0 + visible_rows as f32 * 46.0 + 24.0;
    let rect = popover_rect(rt, screen, anchor, 320.0, height);
    menu_frame(ui, rect, theme);
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, rect.top() + 20.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Start agent", "启动 agent"),
        FontId::proportional(11.0),
        p.text3,
    );
    let rows = Rect::from_min_max(
        Pos2::new(rect.left() + 6.0, rect.top() + 34.0),
        Pos2::new(rect.right() - 6.0, rect.bottom() - 10.0),
    );
    let mut launch: Option<(String, String)> = None;
    let mut close = false;
    crate::app_ui::region(ui, rows, "agent-menu", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("agent-menu-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for option in &options {
                    let (row, response) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 44.0), Sense::click());
                    if response.hovered() {
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::same(5), p.hover);
                    }
                    let icon_rect = Rect::from_center_size(
                        Pos2::new(row.left() + 20.0, row.center().y),
                        Vec2::splat(24.0),
                    );
                    ui.painter()
                        .rect_filled(icon_rect, egui::CornerRadius::same(6), p.accent_soft);
                    paint_icon(ui.painter(), icon_rect.shrink(5.0), Icon::Spark, p.accent);
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(row.left() + 38.0, row.top() + 5.0),
                            Pos2::new(row.right() - 12.0, row.center().y + 3.0),
                        ),
                        &option.name,
                        FontId::proportional(11.5),
                        p.text,
                        Align2::LEFT_CENTER,
                    );
                    let description = if option.description.is_empty() {
                        if option.command.is_empty() {
                            crate::tr!(model, "Start a plain login shell", "启动登录 Shell")
                                .to_string()
                        } else {
                            format!("Run {}", option.command)
                        }
                    } else {
                        option.description.clone()
                    };
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(row.left() + 38.0, row.center().y + 3.0),
                            Pos2::new(row.right() - 12.0, row.bottom() - 3.0),
                        ),
                        &description,
                        FontId::proportional(9.5),
                        p.text3,
                        Align2::LEFT_CENTER,
                    );
                    if !option.command.is_empty() {
                        text_clipped(
                            ui.painter(),
                            Rect::from_min_max(
                                Pos2::new(row.right() - 120.0, row.top()),
                                Pos2::new(row.right() - 12.0, row.center().y - 2.0),
                            ),
                            &option.command,
                            FontId::monospace(8.5),
                            p.text3,
                            Align2::RIGHT_CENTER,
                        );
                    }
                    if response.clicked() {
                        launch = Some((option.name.clone(), option.command.clone()));
                        close = true;
                    }
                }
            });
    });
    if let Some((title, command)) = launch {
        if command.is_empty() {
            model.queue(Command::NewTerminal);
        } else {
            model.queue(Command::NewTerminalCommand { title, command });
        }
    }
    if should_dismiss(ui, rect, rt.agent_menu_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.agent_menu = Some(anchor);
    }
}
