//! Stable project rail: the narrow column under the titlebar lists every open
//! project. The tool buttons that used to live here now live in the titlebar;
//! the rail keeps its only footer action (open another project) plus the
//! per-project console-activity ring.
//!
//! The rail is intentionally independent from the file/commit drawer: it is
//! the fast project switcher, while the top-bar project card remains the
//! affordance for the active project's menu (worktrees, close, open).

use egui::{pos2, vec2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use lazecode_core::{AppModel, Command};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::UiRuntime;
use crate::theme::Theme;

const TILE: f32 = 34.0;
const GAP: f32 = 6.0;
const PAD: f32 = 8.0;
const PLUS: f32 = 32.0;

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    let painter = ui.painter().with_clip_rect(rect);
    let cx = rect.center().x;

    // Footer action stays fixed while the project list can scroll.
    let plus_center = Pos2::new(cx, rect.bottom() - PAD - PLUS * 0.5);
    let plus_rect = Rect::from_center_size(plus_center, Vec2::splat(PLUS));
    let viewport = Rect::from_min_max(
        pos2(rect.left() + 2.0, rect.top() + GAP),
        pos2(rect.right() - 2.0, plus_rect.top() - GAP),
    );

    let count = model.projects.len();
    let content_height = if count == 0 {
        0.0
    } else {
        count as f32 * TILE + count.saturating_sub(1) as f32 * GAP
    };
    let max_scroll = (content_height - viewport.height()).max(0.0);

    // Trackpad/wheel scrolling is scoped to the rail so the editor and
    // console keep their own scroll behaviour.
    if max_scroll > 0.0
        && ui
            .ctx()
            .pointer_hover_pos()
            .is_some_and(|pos| viewport.contains(pos))
    {
        let dy = ui.ctx().input(|input| input.smooth_scroll_delta.y);
        if dy != 0.0 {
            rt.project_rail_scroll -= dy;
        }
    }
    rt.project_rail_scroll = rt.project_rail_scroll.clamp(0.0, max_scroll);
    let scroll = rt.project_rail_scroll;

    for index in 0..count {
        let y = viewport.top() + index as f32 * (TILE + GAP) - scroll;
        let tile = Rect::from_min_size(Pos2::new(cx - TILE * 0.5, y), Vec2::splat(TILE));
        if !tile.intersects(viewport) {
            continue;
        }
        let (tone, active, activity, initials, name, branch) = {
            let project = &model.projects[index];
            (
                Color32::from_rgb(project.tone.0, project.tone.1, project.tone.2),
                model.active_project == Some(index),
                rt.project_busy(project),
                project.initials.clone(),
                project.name.clone(),
                project.branch(),
            )
        };
        let hit = tile.intersect(viewport);
        let response = ui.interact(
            hit,
            ui.scope_id().with(("rail-project", index)),
            Sense::click(),
        );
        if active {
            painter.rect_filled(tile, CornerRadius::same(8), tone);
        } else {
            painter.rect_filled(
                tile,
                CornerRadius::same(8),
                tone.gamma_multiply(if response.hovered() { 0.24 } else { 0.15 }),
            );
            painter.rect_stroke(
                tile,
                CornerRadius::same(8),
                Stroke::new(1.0, tone.gamma_multiply(0.35)),
                StrokeKind::Inside,
            );
        }
        painter.text(
            tile.center(),
            egui::Align2::CENTER_CENTER,
            &initials,
            egui::FontId::proportional(12.0),
            if active { p.accent_on_fill } else { tone },
        );
        paint_activity_border(ui, &painter, tile, 8, &activity, p);
        if active {
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(rect.left() + 2.0, tile.center().y - 8.0),
                    Vec2::new(3.0, 16.0),
                ),
                CornerRadius {
                    nw: 0,
                    ne: 2,
                    se: 2,
                    sw: 0,
                },
                p.accent,
            );
        }

        let mut tooltip = format!("{name} · {branch}");
        if activity.busy {
            tooltip.push('\n');
            tooltip.push_str(&format!(
                "{} {}",
                crate::tr!(model, "Console:", "控制台:"),
                activity.commands.join(", ")
            ));
        }
        let clicked = response.clicked();
        response.on_hover_text(tooltip);
        if clicked {
            model.switch_project(index);
            rt.clear_terminal_cache();
        }
    }

    // A thin scrollbar is enough to signal that more projects exist; the
    // plus button remains fixed at the bottom.
    if max_scroll > 0.0 {
        let track = Rect::from_min_size(
            pos2(rect.right() - 4.0, viewport.top()),
            vec2(2.0, viewport.height()),
        );
        let thumb_h =
            (viewport.height() / content_height * track.height()).clamp(20.0, track.height());
        let t = (scroll / max_scroll).clamp(0.0, 1.0);
        let thumb = Rect::from_min_size(
            pos2(track.left(), track.top() + (track.height() - thumb_h) * t),
            vec2(track.width(), thumb_h),
        );
        painter.rect_filled(track, 1.0, p.border_soft);
        painter.rect_filled(thumb, 1.0, p.text3);
    }

    let plus = ui.interact(
        plus_rect,
        ui.scope_id().with("rail-open-project"),
        Sense::click(),
    );
    if plus.hovered() {
        painter.rect_filled(plus_rect, CornerRadius::same(8), p.hover);
    }
    paint_icon(
        &painter,
        plus_rect.shrink(8.0),
        Icon::Plus,
        if plus.hovered() { p.text } else { p.text3 },
    );
    if plus.clicked() {
        model.queue(Command::OpenProjectDialog);
    }
    plus.on_hover_text(crate::tr!(model, "Open another project", "打开其他项目"));
}

fn paint_activity_border(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    radius: u8,
    activity: &crate::runtime::TerminalBusyState,
    p: crate::theme::Palette,
) {
    if !activity.busy {
        return;
    }
    let color = if activity.outputting {
        p.rose
    } else {
        p.running
    };
    painter.rect_stroke(
        rect,
        CornerRadius::same(radius),
        Stroke::new(1.25, color),
        StrokeKind::Inside,
    );
    if !activity.outputting {
        return;
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(16));
    let time = ui.ctx().input(|input| input.time) as f32;
    let w = (rect.width() - 2.0).max(1.0);
    let h = (rect.height() - 2.0).max(1.0);
    let perimeter = 2.0 * (w + h);
    let at = |distance: f32| {
        let mut d = distance % perimeter;
        if d < w {
            return Pos2::new(rect.left() + 1.0 + d, rect.top() + 1.0);
        }
        d -= w;
        if d < h {
            return Pos2::new(rect.right() - 1.0, rect.top() + 1.0 + d);
        }
        d -= h;
        if d < w {
            return Pos2::new(rect.right() - 1.0 - d, rect.bottom() - 1.0);
        }
        d -= w;
        Pos2::new(rect.left() + 1.0, rect.bottom() - 1.0 - d)
    };
    let start = (time * 70.0) % perimeter;
    let len = 12.0f32.min(perimeter * 0.35);
    painter.line_segment([at(start), at(start + len)], Stroke::new(2.0, p.rose));
}
