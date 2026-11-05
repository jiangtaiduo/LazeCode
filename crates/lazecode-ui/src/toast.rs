use egui::{Align2, FontId, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2};
use lazecode_core::{AppModel, ToastKind};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::UiRuntime;
use crate::theme::Theme;
use crate::widgets::text_clipped;

pub fn draw(ui: &mut Ui, model: &AppModel, _rt: &mut UiRuntime, theme: &Theme, status: Rect) {
    if !model.toasts.is_empty() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    }
    let p = theme.palette;
    let mut y = status.top() - 12.0;
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
        let width = (toast.message.chars().count() as f32 * 6.2 + 68.0).clamp(180.0, 460.0);
        let rect = Rect::from_center_size(
            Pos2::new(status.center().x, y - 16.0),
            Vec2::new(width, 34.0),
        );
        let fill = p.elev.gamma_multiply(alpha);
        let border = color.gamma_multiply(0.35 * alpha);
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(8), fill);
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(8),
            Stroke::new(1.0, border),
            StrokeKind::Inside,
        );
        paint_icon(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(rect.left() + 18.0, rect.center().y),
                Vec2::splat(14.0),
            ),
            icon,
            color.gamma_multiply(alpha),
        );
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(rect.left() + 32.0, rect.top()),
                Pos2::new(rect.right() - 12.0, rect.bottom()),
            ),
            &toast.message,
            FontId::proportional(11.5),
            p.text.gamma_multiply(alpha),
            Align2::LEFT_CENTER,
        );
        y -= 40.0;
    }
}
