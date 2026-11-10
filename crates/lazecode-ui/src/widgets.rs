use egui::{
    Align2, Color32, FontId, Painter, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2,
};

use crate::icons::{paint as paint_icon, Icon};
use crate::theme::Theme;

/// Draw text clipped to `rect`, adding an ellipsis when it doesn't fit.
pub fn text_clipped(
    painter: &Painter,
    rect: Rect,
    text: &str,
    font: FontId,
    color: Color32,
    align: Align2,
) {
    if text.is_empty() || rect.width() <= 1.0 {
        return;
    }
    let galley = painter.layout_no_wrap(text.to_owned(), font.clone(), color);
    if galley.size().x <= rect.width() {
        painter.galley(
            align.align_size_within_rect(galley.size(), rect).min,
            galley,
            color,
        );
        return;
    }
    let ellipsis = painter.layout_no_wrap("…".to_owned(), font.clone(), color);
    let available = (rect.width() - ellipsis.size().x).max(1.0);
    let mut fitted = String::new();
    let mut width = 0.0;
    for ch in text.chars() {
        let next = painter.layout_no_wrap(ch.to_string(), font.clone(), color);
        if width + next.size().x > available {
            break;
        }
        width += next.size().x;
        fitted.push(ch);
    }
    fitted.push('…');
    let galley = painter.layout_no_wrap(fitted, font, color);
    painter.galley(
        align.align_size_within_rect(galley.size(), rect).min,
        galley,
        color,
    );
}

pub fn label(painter: &Painter, pos: Pos2, text: &str, size: f32, color: Color32) {
    painter.text(
        pos,
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(size),
        color,
    );
}

pub fn mono(painter: &Painter, pos: Pos2, text: &str, size: f32, color: Color32) {
    painter.text(
        pos,
        Align2::LEFT_CENTER,
        text,
        FontId::monospace(size),
        color,
    );
}

pub fn text_width(painter: &Painter, text: &str, font: FontId) -> f32 {
    painter
        .layout_no_wrap(text.to_owned(), font, Color32::WHITE)
        .size()
        .x
}

pub fn mono_char_width(ctx: &egui::Context, size: f32) -> f32 {
    ctx.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap("M".to_owned(), FontId::monospace(size), Color32::WHITE)
            .size()
            .x
    })
}

/// Thin keyboard hint, matching the design's `.kbd`.
pub fn kbd(painter: &Painter, rect: Rect, label: &str, theme: &Theme) {
    let p = theme.palette;
    painter.rect_filled(rect, egui::CornerRadius::same(4), p.panel2);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(4),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    text_clipped(
        painter,
        rect.shrink2(Vec2::new(3.0, 0.0)),
        label,
        FontId::monospace(11.0),
        p.text2,
        Align2::CENTER_CENTER,
    );
}

/// A small dot whose glow conveys "process running".
pub fn live_dot(painter: &Painter, center: Pos2, on: bool, color: Color32) {
    if on {
        painter.circle_filled(center, 4.5, color.gamma_multiply(0.16));
        painter.circle_filled(center, 2.6, color);
    } else {
        painter.circle_filled(center, 2.4, color);
    }
}

pub fn hover_rect(ui: &Ui, rect: Rect, theme: &Theme) -> Response {
    let response = ui.interact(
        rect,
        ui.scope_id()
            .with(("hover", rect.min.x as i32, rect.min.y as i32)),
        Sense::hover(),
    );
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(5), theme.palette.hover);
    }
    response
}

pub fn row_response(
    ui: &mut Ui,
    rect: Rect,
    id: impl std::hash::Hash + std::fmt::Debug,
    selected: bool,
    theme: &Theme,
) -> Response {
    let response = ui.interact(rect, ui.scope_id().with(id), Sense::click());
    let p = theme.palette;
    if selected {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(5), p.active);
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(5), p.hover);
    }
    response
}

/// Custom checkbox used by the commit dock and settings.
pub fn checkbox(ui: &mut Ui, rect: Rect, checked: bool, theme: &Theme) -> Response {
    let response = ui.interact(
        rect,
        ui.scope_id()
            .with(("chk", rect.min.x as i32, rect.min.y as i32)),
        Sense::click(),
    );
    let p = theme.palette;
    let box_rect = Rect::from_center_size(rect.center(), Vec2::splat(14.0));
    if checked {
        ui.painter()
            .rect_filled(box_rect, egui::CornerRadius::same(4), p.accent);
        paint_icon(
            ui.painter(),
            box_rect.shrink(2.0),
            Icon::Check,
            p.accent_on_fill,
        );
    } else {
        ui.painter()
            .rect_filled(box_rect, egui::CornerRadius::same(4), p.panel2);
        ui.painter().rect_stroke(
            box_rect,
            egui::CornerRadius::same(4),
            Stroke::new(
                1.0,
                if response.hovered() {
                    p.border_strong
                } else {
                    p.border
                },
            ),
            StrokeKind::Inside,
        );
    }
    response
}

/// 组头三态勾选框状态：全勾 / 全不勾 / 半选（组内勾选不一致）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriState {
    Checked,
    Unchecked,
    Partial,
}

/// Tri-state checkbox for changelist group headers. 半选态画一条短横线
/// （JetBrains 惯例），点击行为由调用方决定（有未勾项→全勾，否则全不勾）。
pub fn checkbox_tri(ui: &mut Ui, rect: Rect, state: TriState, theme: &Theme) -> Response {
    let response = ui.interact(
        rect,
        ui.scope_id()
            .with(("chktri", rect.min.x as i32, rect.min.y as i32)),
        Sense::click(),
    );
    let p = theme.palette;
    let box_rect = Rect::from_center_size(rect.center(), Vec2::splat(14.0));
    match state {
        TriState::Checked => {
            ui.painter()
                .rect_filled(box_rect, egui::CornerRadius::same(4), p.accent);
            paint_icon(
                ui.painter(),
                box_rect.shrink(2.0),
                Icon::Check,
                p.accent_on_fill,
            );
        }
        TriState::Partial => {
            ui.painter()
                .rect_filled(box_rect, egui::CornerRadius::same(4), p.accent);
            let bar = Rect::from_min_max(
                Pos2::new(box_rect.left() + 3.5, box_rect.center().y - 1.0),
                Pos2::new(box_rect.right() - 3.5, box_rect.center().y + 1.0),
            );
            ui.painter()
                .rect_filled(bar, egui::CornerRadius::same(1), p.accent_on_fill);
        }
        TriState::Unchecked => {
            ui.painter()
                .rect_filled(box_rect, egui::CornerRadius::same(4), p.panel2);
            ui.painter().rect_stroke(
                box_rect,
                egui::CornerRadius::same(4),
                Stroke::new(
                    1.0,
                    if response.hovered() {
                        p.border_strong
                    } else {
                        p.border
                    },
                ),
                StrokeKind::Inside,
            );
        }
    }
    response
}

/// A filled primary button (Commit, Open project).
pub fn primary_button(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    icon: Option<Icon>,
    enabled: bool,
    theme: &Theme,
) -> Response {
    let p = theme.palette;
    let response = ui.interact(
        rect,
        ui.scope_id()
            .with(("primary", rect.min.x as i32, rect.min.y as i32, label)),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let fill = if !enabled {
        p.panel2
    } else if response.hovered() {
        p.accent.gamma_multiply(1.06)
    } else {
        p.accent
    };
    let text_color = if enabled { p.accent_on_fill } else { p.text3 };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(6), fill);
    let mut text_rect = rect;
    if let Some(icon) = icon {
        let icon_rect = Rect::from_center_size(
            Pos2::new(rect.left() + 14.0, rect.center().y),
            Vec2::splat(14.0),
        );
        paint_icon(ui.painter(), icon_rect, icon, text_color);
        text_rect.min.x += 24.0;
    }
    text_clipped(
        ui.painter(),
        text_rect,
        label,
        FontId::proportional(13.0),
        text_color,
        Align2::CENTER_CENTER,
    );
    response
}

/// A translucent secondary button.
pub fn secondary_button(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    icon: Option<Icon>,
    theme: &Theme,
) -> Response {
    let p = theme.palette;
    let response = ui.interact(
        rect,
        ui.scope_id()
            .with(("secondary", rect.min.x as i32, rect.min.y as i32, label)),
        Sense::click(),
    );
    let fill = if response.hovered() {
        p.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(6), fill);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(6),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    let mut text_rect = rect;
    if let Some(icon) = icon {
        let icon_rect = Rect::from_center_size(
            Pos2::new(rect.left() + 14.0, rect.center().y),
            Vec2::splat(13.0),
        );
        paint_icon(ui.painter(), icon_rect, icon, p.text2);
        text_rect.min.x += 22.0;
    }
    text_clipped(
        ui.painter(),
        text_rect,
        label,
        FontId::proportional(12.5),
        p.text2,
        Align2::CENTER_CENTER,
    );
    response
}

/// `secondary_button` 的可禁用版本（Shelve 等需要空态禁用的按钮）。
pub fn secondary_button_enabled(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    icon: Option<Icon>,
    enabled: bool,
    theme: &Theme,
) -> Response {
    let p = theme.palette;
    let response = ui.interact(
        rect,
        ui.scope_id()
            .with(("secondarye", rect.min.x as i32, rect.min.y as i32, label)),
        Sense::click(),
    );
    let hovered = enabled && response.hovered();
    let fill = if hovered {
        p.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(6), fill);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(6),
        Stroke::new(1.0, if enabled { p.border } else { p.border_soft }),
        StrokeKind::Inside,
    );
    let mut text_rect = rect;
    if let Some(icon) = icon {
        let icon_rect = Rect::from_center_size(
            Pos2::new(rect.left() + 14.0, rect.center().y),
            Vec2::splat(13.0),
        );
        paint_icon(
            ui.painter(),
            icon_rect,
            icon,
            if enabled { p.text2 } else { p.text3 },
        );
        text_rect.min.x += 22.0;
    }
    text_clipped(
        ui.painter(),
        text_rect,
        label,
        FontId::proportional(12.5),
        if enabled { p.text2 } else { p.text3 },
        Align2::CENTER_CENTER,
    );
    response
}

/// Icon-only chrome button.
pub fn icon_button(ui: &mut Ui, rect: Rect, icon: Icon, active: bool, theme: &Theme) -> Response {
    let p = theme.palette;
    let response = ui.interact(
        rect,
        ui.scope_id()
            .with(("icon", rect.min.x as i32, rect.min.y as i32, icon as u8)),
        Sense::click(),
    );
    if active || response.hovered() {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(6),
            if active { p.active } else { p.hover },
        );
    }
    let color = if active { p.text } else { p.text2 };
    let icon_rect =
        Rect::from_center_size(rect.center(), Vec2::splat(14.0_f32.min(rect.width() - 4.0)));
    paint_icon(ui.painter(), icon_rect, icon, color);
    response
}

/// Draw a vertical scrollbar for custom scrolling surfaces and handle dragging.
pub fn scrollbar(
    ui: &mut Ui,
    track: Rect,
    content_height: f32,
    viewport_height: f32,
    offset: &mut f32,
    theme: &Theme,
) -> bool {
    let p = theme.palette;
    if content_height <= viewport_height + 0.5 {
        return false;
    }
    let max_offset = (content_height - viewport_height).max(1.0);
    let thumb_height =
        (viewport_height / content_height * track.height()).clamp(24.0, track.height());
    let travel = track.height() - thumb_height;
    let t = ((*offset) / max_offset).clamp(0.0, 1.0);
    let thumb = Rect::from_min_size(
        Pos2::new(track.min.x + 2.0, track.min.y + travel * t),
        Vec2::new(track.width() - 4.0, thumb_height),
    );
    let response = ui.interact(
        thumb,
        ui.scope_id()
            .with(("scrollbar", track.min.x as i32, track.min.y as i32)),
        Sense::click_and_drag(),
    );
    let color = if response.hovered() || response.dragged() {
        p.scrollbar_hover
    } else {
        p.scrollbar
    };
    ui.painter()
        .rect_filled(thumb, egui::CornerRadius::same(4), color);
    if response.dragged() {
        let delta = response.drag_delta().y;
        *offset = (*offset + delta / travel * max_offset).clamp(0.0, max_offset);
        true
    } else {
        false
    }
}

/// A section heading inside popovers and dialogs.
pub fn section_label(painter: &Painter, pos: Pos2, text: &str, theme: &Theme) {
    painter.text(
        pos,
        Align2::LEFT_CENTER,
        text.to_uppercase(),
        FontId::proportional(11.0),
        theme.palette.text3,
    );
}
