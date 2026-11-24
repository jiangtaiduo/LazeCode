use egui::{Align2, FontId, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2};
use lazecode_core::{AppModel, ToastKind};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::UiRuntime;
use crate::theme::Theme;
use crate::widgets::text_clipped;

const TOAST_H: f32 = 38.0;
const TOAST_GAP: f32 = 8.0;
const TOAST_MAX_W: f32 = 560.0;
const PAD_L: f32 = 12.0;
const PAD_R: f32 = 14.0;
const ICON_BOX: f32 = 24.0;
const ICON_GAP: f32 = 10.0;

/// Global transient notifications, anchored just below the titlebar and
/// stacked downward (newest first). Contextual LSP popups and modal dialogs
/// keep their own anchors.
pub fn draw(
    ui: &mut Ui,
    model: &AppModel,
    _rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    top: f32,
) {
    if !model.toasts.is_empty() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    }
    let p = theme.palette;
    let font = FontId::proportional(12.5);
    let max_w = (screen.width() - 32.0).clamp(240.0, TOAST_MAX_W);
    let mut y = top;
    for toast in model.toasts.iter().rev() {
        let remaining = toast
            .expires
            .saturating_duration_since(std::time::Instant::now())
            .as_secs_f32();
        let alpha = (remaining / 0.6).min(1.0);
        let (color, icon) = match toast.kind {
            ToastKind::Info => (p.blue, Icon::Info),
            ToastKind::Success => (p.green, Icon::Check),
            ToastKind::Warning => (p.amber, Icon::Alert),
            ToastKind::Error => (p.rose, Icon::Error),
        };
        let galley = ui
            .painter()
            .layout_no_wrap(toast.message.clone(), font.clone(), p.text);
        let width = (galley.size().x + PAD_L + ICON_BOX + ICON_GAP + PAD_R).clamp(220.0, max_w);
        let rect = Rect::from_min_size(
            Pos2::new(screen.center().x - width * 0.5, y),
            Vec2::new(width, TOAST_H),
        );

        crate::overlays::dialog_shadow(
            ui.painter(),
            rect,
            theme,
            10.0,
            5.0,
            18.0,
            (52.0 * alpha) as u8,
        );
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(10),
            p.dialog.gamma_multiply(alpha),
        );
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(10),
            Stroke::new(1.0, color.gamma_multiply(0.5 * alpha)),
            StrokeKind::Inside,
        );

        let icon_box = Rect::from_center_size(
            Pos2::new(rect.left() + PAD_L + ICON_BOX * 0.5, rect.center().y),
            Vec2::splat(ICON_BOX),
        );
        ui.painter().rect_filled(
            icon_box,
            egui::CornerRadius::same(6),
            color.gamma_multiply(0.14 * alpha),
        );
        paint_icon(
            ui.painter(),
            Rect::from_center_size(icon_box.center(), Vec2::splat(14.0)),
            icon,
            color.gamma_multiply(alpha),
        );
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(icon_box.right() + ICON_GAP, rect.top()),
                Pos2::new(rect.right() - PAD_R, rect.bottom()),
            ),
            &toast.message,
            font.clone(),
            p.text.gamma_multiply(alpha),
            Align2::LEFT_CENTER,
        );
        y += TOAST_H + TOAST_GAP;
    }
}
