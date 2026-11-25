//! Terminal settings section (shell, fonts, scrollback, graphics).

use super::prelude::*;
use super::widgets::*;

pub(super) fn scrollback_options(current: usize) -> Vec<usize> {
    let mut options = vec![5_000usize, 10_000, 20_000, 50_000];
    if !options.contains(&current) {
        options.push(current);
        options.sort_unstable();
    }
    options
}

pub(super) fn terminal(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
) {
    let p = theme.palette;
    heading(
        ui,
        theme,
        rect,
        crate::tr!(model, "Terminal", "终端"),
        crate::tr!(
            model,
            "PTY sessions, scrollback and the shell used for consoles.",
            "PTY 会话、滚动历史与控制台 shell。"
        ),
    );
    let mut y = rect.top() + 64.0;
    font_picker(
        ui,
        model,
        rt,
        theme,
        FontPicker {
            rect,
            y,
            label: crate::tr!(model, "Font", "字体"),
            target: FontTarget::Terminal,
            monospaced_only: true,
        },
    );
    y += 40.0;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Font size", "字号"),
        FontId::proportional(11.5),
        p.text2,
    );
    let minus = Rect::from_min_size(
        Pos2::new(rect.left() + 160.0, y - 13.0),
        Vec2::new(28.0, 26.0),
    );
    if secondary_button(ui, minus, "−", None, theme).clicked() {
        model.settings.terminal.font_size = (model.settings.terminal.font_size - 0.5).max(9.0);
        model.save_settings();
    }
    ui.painter().text(
        Pos2::new(minus.right() + 18.0, y),
        Align2::CENTER_CENTER,
        format!("{:.0}", model.settings.terminal.font_size),
        FontId::monospace(11.0),
        p.text,
    );
    let plus = Rect::from_min_size(
        Pos2::new(minus.right() + 46.0, y - 13.0),
        Vec2::new(28.0, 26.0),
    );
    if secondary_button(ui, plus, "+", None, theme).clicked() {
        model.settings.terminal.font_size = (model.settings.terminal.font_size + 0.5).min(24.0);
        model.save_settings();
    }
    y += 40.0;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Scrollback", "滚动历史"),
        FontId::proportional(11.5),
        p.text2,
    );
    let mut x = rect.left() + 160.0;
    let current_scrollback = model.settings.terminal.scrollback;
    for value in scrollback_options(current_scrollback) {
        let selected = current_scrollback == value;
        let label = if value % 1000 == 0 {
            format!("{}k", value / 1000)
        } else {
            format!("{:.1}k", value as f64 / 1000.0)
        };
        let button = Rect::from_min_size(Pos2::new(x, y - 13.0), Vec2::new(60.0, 26.0));
        if option_button(ui, button, &label, selected, None, theme).clicked() && !selected {
            model.settings.terminal.scrollback = value;
            model.save_settings();
        }
        x += 65.0;
    }
    y += 40.0;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Image cache", "图像缓存"),
        FontId::proportional(11.5),
        p.text2,
    );
    let mut x = rect.left() + 160.0;
    for value in [32usize, 64, 128, 256] {
        let button = Rect::from_min_size(Pos2::new(x, y - 13.0), Vec2::new(60.0, 26.0));
        let selected = model.settings.terminal.graphics_cache_mb == value;
        let response = option_button(ui, button, &format!("{value} MB"), selected, None, theme);
        if response.clicked() && !selected {
            model.settings.terminal.graphics_cache_mb = value;
            model.save_settings();
        }
        x += 65.0;
    }
    ui.painter().text(
        Pos2::new(rect.left(), y + 20.0),
        Align2::LEFT_CENTER,
        crate::tr!(
            model,
            "Soft per-console limit; visible images may temporarily use up to 4×.",
            "每个控制台的软上限；可见图片最多可临时占用 4 倍。"
        ),
        FontId::proportional(10.0),
        p.text3,
    );
    y += 54.0;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Shell", "Shell"),
        FontId::proportional(11.5),
        p.text2,
    );
    let field = Rect::from_min_size(
        Pos2::new(rect.left() + 160.0, y - 13.0),
        Vec2::new(260.0, 26.0),
    );
    ui.painter()
        .rect_filled(field, egui::CornerRadius::same(5), p.panel2);
    ui.painter().rect_stroke(
        field,
        egui::CornerRadius::same(5),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    let mut shell = model.settings.terminal.shell.clone().unwrap_or_default();
    let response = ui.put(
        field.shrink2(Vec2::new(7.0, 3.0)),
        TextEdit::singleline(&mut shell)
            .frame(egui::Frame::NONE)
            .align(egui::Align2::LEFT_CENTER)
            .hint_text("$SHELL")
            .font(FontId::monospace(10.5)),
    );
    if response.changed() {
        model.settings.terminal.shell = if shell.trim().is_empty() {
            None
        } else {
            Some(shell.trim().to_string())
        };
        model.save_settings();
    }
    y += 40.0;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Placement", "停靠位置"),
        FontId::proportional(11.5),
        p.text2,
    );
    let is_bottom = model.ui.terminal_placement == lazecode_core::TerminalPlacement::Bottom;
    let bottom_btn = Rect::from_min_size(
        Pos2::new(rect.left() + 160.0, y - 13.0),
        Vec2::new(96.0, 26.0),
    );
    let b_resp = option_button(
        ui,
        bottom_btn,
        crate::tr!(model, "Bottom", "底部"),
        is_bottom,
        None,
        theme,
    );
    if b_resp.clicked() && !is_bottom {
        model.ui.terminal_placement = lazecode_core::TerminalPlacement::Bottom;
        model.save_workspace();
    }
    let right_btn = Rect::from_min_size(
        Pos2::new(bottom_btn.right() + 8.0, y - 13.0),
        Vec2::new(96.0, 26.0),
    );
    let r_resp = option_button(
        ui,
        right_btn,
        crate::tr!(model, "Right", "右侧"),
        !is_bottom,
        None,
        theme,
    );
    if r_resp.clicked() && is_bottom {
        model.ui.terminal_placement = lazecode_core::TerminalPlacement::Right;
        model.save_workspace();
    }
    y += 44.0;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        crate::tr!(
            model,
            "RAM holds the selected scrollback; older rows spill to disk (8× lines / 64 MB) and evict oldest first.",
            "内存保留所选行数；更早的行写入磁盘（8 倍行数 / 64 MB），超出后淘汰最旧行。"
        ),
        FontId::proportional(10.0),
        p.text3,
    );
}
