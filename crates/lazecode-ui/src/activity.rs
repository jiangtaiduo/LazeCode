use egui::{Align2, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use lazecode_core::{AppModel, Command, PanelTab, SidebarView};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::UiRuntime;
use crate::theme::Theme;

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    let cx = rect.center().x;
    let mut y = rect.top() + 10.0;
    let changes = model
        .active_project()
        .map(|project| project.changes().len())
        .unwrap_or(0);
    let running = model.total_running();

    let mut button = |ui: &mut Ui,
                      rt: &mut UiRuntime,
                      icon: Icon,
                      active: bool,
                      badge: Option<(usize, egui::Color32)>,
                      tooltip: &str,
                      id: &'static str|
     -> bool {
        let button_rect = Rect::from_center_size(Pos2::new(cx, y + 16.0), Vec2::splat(34.0));
        y += 42.0;
        let response = ui.interact(button_rect, ui.scope_id().with(id), Sense::click());
        if active || response.hovered() {
            ui.painter().rect_filled(
                button_rect,
                egui::CornerRadius::same(8),
                if active { p.active } else { p.hover },
            );
        }
        if active {
            ui.painter().rect_filled(
                Rect::from_min_size(
                    Pos2::new(rect.left() + 2.0, button_rect.center().y - 8.0),
                    Vec2::new(2.0, 16.0),
                ),
                egui::CornerRadius::same(1),
                p.accent,
            );
        }
        paint_icon(
            ui.painter(),
            button_rect.shrink(9.0),
            icon,
            if active { p.text } else { p.text3 },
        );
        if let Some((count, color)) = badge {
            if count > 0 {
                let label = if count > 99 {
                    "99+".to_string()
                } else {
                    count.to_string()
                };
                let badge_width = (label.chars().count() as f32 * 5.2 + 6.0).max(15.0);
                let badge_rect = Rect::from_center_size(
                    Pos2::new(
                        button_rect.right() - badge_width * 0.5 + 1.0,
                        button_rect.top() + 8.0,
                    ),
                    Vec2::new(badge_width, 14.0),
                );
                ui.painter()
                    .rect_filled(badge_rect, egui::CornerRadius::same(7), color);
                ui.painter().text(
                    badge_rect.center(),
                    Align2::CENTER_CENTER,
                    label,
                    FontId::monospace(8.5),
                    egui::Color32::from_rgb(0x07, 0x0a, 0x08),
                );
            }
        }
        let _ = rt;
        response.on_hover_text(tooltip).clicked()
    };

    let files_active = model.ui.sidebar_visible && model.ui.sidebar_view == SidebarView::Files;
    if button(
        ui,
        rt,
        Icon::Files,
        files_active,
        None,
        crate::tr!(model, "Explorer · ⌘B", "资源管理器 · ⌘B"),
        "act-files",
    ) {
        model.ui.sidebar_view = SidebarView::Files;
        model.ui.sidebar_visible = true;
    }
    let git_active = model.ui.sidebar_visible && model.ui.sidebar_view == SidebarView::Git;
    if button(
        ui,
        rt,
        Icon::Changes,
        git_active,
        Some((changes, p.orange)),
        crate::tr!(model, "Source control", "版本控制"),
        "act-git",
    ) {
        model.ui.sidebar_view = SidebarView::Git;
        model.ui.sidebar_visible = true;
    }
    let consoles_active =
        model.ui.sidebar_visible && model.ui.sidebar_view == SidebarView::Consoles;
    if button(
        ui,
        rt,
        Icon::Terminal,
        consoles_active,
        Some((running, p.running)),
        crate::tr!(model, "Consoles", "控制台"),
        "act-consoles",
    ) {
        model.ui.sidebar_view = SidebarView::Consoles;
        model.ui.sidebar_visible = true;
        model.ui.panel_visible = true;
        model.ui.panel_tab = PanelTab::Console;
    }
    let log_active = model.ui.panel_visible && model.ui.panel_tab == PanelTab::Git;
    if button(
        ui,
        rt,
        Icon::GitLog,
        log_active,
        None,
        crate::tr!(model, "Git log · ⌘⇧G", "Git 日志 · ⌘⇧G"),
        "act-log",
    ) {
        if log_active {
            model.ui.panel_visible = false;
        } else {
            model.ui.panel_visible = true;
            model.ui.panel_tab = PanelTab::Git;
            model.queue(Command::RefreshLog);
        }
    }

    // Theme toggle pinned near the bottom of the activity bar.
    let bottom = rect.bottom() - 34.0;
    let icon = if theme.dark { Icon::Moon } else { Icon::Sun };
    let toggle_rect = Rect::from_center_size(Pos2::new(cx, bottom), Vec2::splat(30.0));
    let response = ui.interact(toggle_rect, ui.scope_id().with("act-theme"), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(toggle_rect, egui::CornerRadius::same(8), p.hover);
    }
    paint_icon(ui.painter(), toggle_rect.shrink(8.0), icon, p.text3);
    if response.clicked() {
        model.toggle_theme();
        rt.applied_theme = None;
    }
    response.on_hover_text(crate::tr!(model, "Toggle theme", "切换主题"));

    ui.painter().vline(
        rect.right() - 0.5,
        rect.y_range(),
        Stroke::new(1.0, p.border_soft),
    );
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::ZERO,
        Stroke::new(0.0, p.border_soft),
        StrokeKind::Inside,
    );
}
