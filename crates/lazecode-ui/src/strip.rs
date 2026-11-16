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

    let visible: Vec<usize> = model
        .projects
        .iter()
        .enumerate()
        .filter(|(_, project)| !(project.is_single_file && project.tabs.is_empty()))
        .map(|(index, _)| index)
        .collect();
    let count = visible.len();
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

    for (slot, &index) in visible.iter().enumerate() {
        let y = viewport.top() + slot as f32 * (TILE + GAP) - scroll;
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
    const RING_STROKE: f32 = 2.25;
    let ring_color = if activity.outputting {
        // Stable draws a dim red frame while outputting; the moving dash is
        // the only bright red element. A full-bright frame hides the dash and
        // makes any path error look like a spike.
        p.rose.gamma_multiply(0.45)
    } else {
        p.running
    };
    painter.rect_stroke(
        rect,
        CornerRadius::same(radius),
        Stroke::new(RING_STROKE, ring_color),
        StrokeKind::Inside,
    );
    if !activity.outputting {
        return;
    }
    // The ring is decorative; cap it at 30 fps so a busy console does not
    // force the whole window onto the compositor's native refresh cadence.
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(33));
    let time = ui.ctx().input(|input| input.time) as f32;

    // The ring is stroked inside the rect. Put the dash centerline on the
    // same centerline, then trace the rounded rectangle by arc length.
    let inset = RING_STROKE * 0.5;
    let path_rect = rect.shrink(inset);
    let path_radius =
        (radius as f32 - inset).clamp(0.5, path_rect.width().min(path_rect.height()) * 0.5);
    let perimeter = rounded_rect_perimeter(path_rect, path_radius);
    if perimeter <= 0.0 {
        return;
    }
    // Stable marquee: a 9%-perimeter dash with round caps, one revolution
    // every 2.4 s, no trailing copies.
    let dash_len = perimeter * 0.09;
    let start = (time / 2.4).rem_euclid(1.0) * perimeter;
    let steps = ((dash_len * 3.0).ceil() as usize).clamp(10, 96);
    let points: Vec<Pos2> = (0..=steps)
        .map(|step| {
            let d = start + dash_len * step as f32 / steps as f32;
            rounded_rect_point(path_rect, path_radius, d)
        })
        .collect();
    let dash = Stroke::new(2.0, p.rose);
    painter.add(egui::Shape::line(points, dash));
    // Caps make the ends circular, exactly like SVG stroke-linecap="round".
    painter.circle_filled(
        rounded_rect_point(path_rect, path_radius, start),
        1.0,
        p.rose,
    );
    painter.circle_filled(
        rounded_rect_point(path_rect, path_radius, start + dash_len),
        1.0,
        p.rose,
    );
}

fn rounded_rect_perimeter(rect: Rect, radius: f32) -> f32 {
    let r = radius.clamp(0.0, rect.width().min(rect.height()) * 0.5);
    2.0 * (rect.width() - 2.0 * r).max(0.0)
        + 2.0 * (rect.height() - 2.0 * r).max(0.0)
        + std::f32::consts::TAU * r
}

/// Point at `distance` along a clockwise rounded-rectangle outline.
fn rounded_rect_point(rect: Rect, radius: f32, distance: f32) -> Pos2 {
    use std::f32::consts::{FRAC_PI_2, PI};
    let r = radius.clamp(0.0, rect.width().min(rect.height()) * 0.5);
    let left = rect.left();
    let top = rect.top();
    let right = rect.right();
    let bottom = rect.bottom();
    let straight_w = (rect.width() - 2.0 * r).max(0.0);
    let straight_h = (rect.height() - 2.0 * r).max(0.0);
    let arc = FRAC_PI_2 * r;
    let perimeter = rounded_rect_perimeter(rect, r);
    let mut d = distance.rem_euclid(perimeter);

    // Top edge, left to right.
    if d <= straight_w {
        return Pos2::new(left + r + d, top);
    }
    d -= straight_w;

    // Top-right corner.
    if d <= arc {
        let angle = -FRAC_PI_2 + if r > 0.0 { d / r } else { 0.0 };
        return Pos2::new(right - r + r * angle.cos(), top + r + r * angle.sin());
    }
    d -= arc;

    // Right edge, top to bottom.
    if d <= straight_h {
        return Pos2::new(right, top + r + d);
    }
    d -= straight_h;

    // Bottom-right corner.
    if d <= arc {
        let angle = if r > 0.0 { d / r } else { 0.0 };
        return Pos2::new(right - r + r * angle.cos(), bottom - r + r * angle.sin());
    }
    d -= arc;

    // Bottom edge, right to left.
    if d <= straight_w {
        return Pos2::new(right - r - d, bottom);
    }
    d -= straight_w;

    // Bottom-left corner.
    if d <= arc {
        let angle = FRAC_PI_2 + if r > 0.0 { d / r } else { 0.0 };
        return Pos2::new(left + r + r * angle.cos(), bottom - r + r * angle.sin());
    }
    d -= arc;

    // Left edge, bottom to top.
    if d <= straight_h {
        return Pos2::new(left, bottom - r - d);
    }
    d -= straight_h;

    // Top-left corner.
    let angle = PI + if r > 0.0 { d / r } else { 0.0 };
    Pos2::new(left + r + r * angle.cos(), top + r + r * angle.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounded_perimeter_matches_css_rect_path() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(34.0));
        let r = 6.875;
        let expected = 2.0 * (34.0 - 2.0 * r) * 2.0 + std::f32::consts::TAU * r;
        assert!((rounded_rect_perimeter(rect, r) - expected).abs() < 0.001);
    }

    #[test]
    fn rounded_point_traverses_corners_on_the_arc() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(34.0));
        let r = 6.875;
        let straight = 34.0 - 2.0 * r;
        // Midpoint of the top-right quarter arc: 45 degrees from vertical.
        let d = straight + std::f32::consts::FRAC_PI_2 * r * 0.5;
        let point = rounded_rect_point(rect, r, d);
        let center = Pos2::new(34.0 - r, r);
        let distance = (point - center).length();
        assert!((distance - r).abs() < 0.01);
        assert!(point.x > center.x);
        assert!(point.y < center.y);
    }

    #[test]
    fn sampled_perimeter_has_no_chord_jumps() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(34.0));
        let r = 6.875;
        let perimeter = rounded_rect_perimeter(rect, r);
        let step = 0.25;
        let mut previous = rounded_rect_point(rect, r, 0.0);
        let mut distance = step;
        while distance <= perimeter + step {
            let point = rounded_rect_point(rect, r, distance);
            // A dense sample of the true outline never jumps by a chord:
            // this is the regression test for the square-corner marquee.
            assert!(
                (point - previous).length() <= step * 1.5,
                "seam jump at {distance}: {:?} -> {:?}",
                previous,
                point
            );
            previous = point;
            distance += step;
        }
    }

    #[test]
    fn rounded_point_wraps_without_discontinuity() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(34.0));
        let r = 6.875;
        let perimeter = rounded_rect_perimeter(rect, r);
        let a = rounded_rect_point(rect, r, 1.0);
        let b = rounded_rect_point(rect, r, perimeter + 1.0);
        assert!((a - b).length() < 0.001);
    }
}
