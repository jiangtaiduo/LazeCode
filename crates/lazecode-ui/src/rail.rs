use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use lazecode_core::{AppModel, Command};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::UiRuntime;
use crate::theme::{Theme, SMALL, TINY};
use crate::widgets::text_clipped;

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    let painter = ui.painter();
    let cx = rect.center().x;

    // Leave room for the native macOS traffic lights above the logo.
    let logo_rect = Rect::from_center_size(Pos2::new(cx, rect.top() + 50.0), Vec2::splat(34.0));
    let response = ui.interact(logo_rect, ui.scope_id().with("rail-logo"), Sense::click());
    let fill = if response.hovered() {
        p.accent.gamma_multiply(1.06)
    } else {
        p.accent
    };
    painter.rect_filled(logo_rect, egui::CornerRadius::same(10), fill);
    painter.text(
        logo_rect.center(),
        Align2::CENTER_CENTER,
        ">_",
        FontId::monospace(13.0),
        p.accent_on_fill,
    );
    if response.clicked() {
        rt.open_palette();
        model.ui.command_palette_open = true;
    }
    response.on_hover_text(crate::tr!(model, "Command palette · ⌘K", "命令面板 · ⌘K"));

    painter.hline(
        (cx - 11.0)..=(cx + 11.0),
        rect.top() + 76.0,
        Stroke::new(1.0, p.rail_border),
    );

    let mut y = rect.top() + 92.0;
    for index in 0..model.projects.len() {
        let (
            tone,
            active,
            running,
            initials,
            project_name,
            project_branch,
            root_display,
            terminals_len,
        ) = {
            let project = &model.projects[index];
            (
                Color32::from_rgb(project.tone.0, project.tone.1, project.tone.2),
                model.active_project == Some(index),
                project
                    .terminals
                    .iter()
                    .filter(|terminal| terminal.session.is_running())
                    .count(),
                project.initials.clone(),
                project.name.clone(),
                project.branch(),
                project.root.display().to_string(),
                project.terminals.len(),
            )
        };
        let tile_rect = Rect::from_center_size(Pos2::new(cx, y + 17.0), Vec2::splat(34.0));
        let response = ui.interact(
            tile_rect,
            ui.scope_id().with(("project-tile", index)),
            Sense::click(),
        );
        if active {
            painter.rect_filled(tile_rect, egui::CornerRadius::same(10), tone);
        } else {
            let fill = tone.gamma_multiply(if response.hovered() { 0.22 } else { 0.15 });
            painter.rect_filled(tile_rect, egui::CornerRadius::same(10), fill);
            painter.rect_stroke(
                tile_rect,
                egui::CornerRadius::same(10),
                Stroke::new(1.0, tone.gamma_multiply(0.35)),
                StrokeKind::Inside,
            );
        }
        let text_color = if active { p.accent_on_fill } else { tone };
        painter.text(
            tile_rect.center(),
            Align2::CENTER_CENTER,
            &initials,
            FontId::proportional(11.5),
            text_color,
        );
        if running > 0 {
            let badge = Rect::from_center_size(
                Pos2::new(tile_rect.right() - 2.0, tile_rect.bottom() - 2.0),
                Vec2::splat(15.0),
            );
            painter.circle_filled(badge.center(), 7.5, p.rail);
            painter.circle_filled(badge.center(), 6.0, p.running);
            painter.text(
                badge.center(),
                Align2::CENTER_CENTER,
                running.to_string(),
                FontId::monospace(8.0),
                Color32::from_rgb(0x08, 0x18, 0x10),
            );
        }
        if response.clicked() {
            model.switch_project(index);
        }
        response.on_hover_ui(|ui| {
            ui.set_max_width(280.0);
            ui.label(egui::RichText::new(&project_name).strong().size(12.5));
            ui.label(
                egui::RichText::new(format!("{} · {} terminals", project_branch, terminals_len))
                    .size(11.0)
                    .color(p.text3),
            );
            ui.label(
                egui::RichText::new(&root_display)
                    .size(10.5)
                    .monospace()
                    .color(p.text3),
            );
        });
        y += 51.0;
    }

    let add_rect = Rect::from_center_size(Pos2::new(cx, y + 16.0), Vec2::splat(30.0));
    let response = ui.interact(add_rect, ui.scope_id().with("rail-add"), Sense::click());
    if response.hovered() {
        painter.rect_filled(add_rect, egui::CornerRadius::same(9), p.hover);
    }
    paint_icon(ui.painter(), add_rect.shrink(8.0), Icon::Plus, p.text3);
    if response.clicked() {
        model.queue(Command::OpenProjectDialog);
    }
    response.on_hover_text(crate::tr!(model, "Open another project", "打开其他项目"));

    let settings_rect =
        Rect::from_center_size(Pos2::new(cx, rect.bottom() - 28.0), Vec2::splat(30.0));
    let response = ui.interact(
        settings_rect,
        ui.scope_id().with("rail-settings"),
        Sense::click(),
    );
    if response.hovered() || rt.settings_open {
        painter.rect_filled(
            settings_rect,
            egui::CornerRadius::same(9),
            if rt.settings_open { p.active } else { p.hover },
        );
    }
    paint_icon(
        ui.painter(),
        settings_rect.shrink(7.0),
        Icon::Settings,
        if rt.settings_open { p.text } else { p.text3 },
    );
    if response.clicked() {
        rt.settings_open = !rt.settings_open;
        rt.about_open = false;
    }
    response.on_hover_text(crate::tr!(model, "Settings · ⌘,", "设置 · ⌘,"));

    // Recent projects are reachable from the command palette, but the rail
    // footer shows how many are open so the count is never ambiguous.
    let count = model.projects.len();
    if count > 0 {
        let label = format!("{}", count);
        painter.text(
            Pos2::new(cx, rect.bottom() - 52.0),
            Align2::CENTER_CENTER,
            label,
            FontId::monospace(9.0),
            p.text3,
        );
    }
    let _ = (SMALL, TINY, text_clipped);
}
