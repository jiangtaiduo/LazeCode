//! About section.

use super::prelude::*;
use super::widgets::*;

pub(super) fn about(ui: &mut Ui, model: &AppModel, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    heading(
        ui,
        theme,
        rect,
        crate::tr!(model, "About LazeCode", "关于 LazeCode"),
        crate::tr!(
            model,
            "A native, pure-Rust workspace for code, Git and terminals.",
            "纯 Rust 的代码、Git 与终端工作空间。"
        ),
    );
    let mut y = rect.top() + 72.0;
    let rows = [
        (
            crate::tr!(model, "Version", "版本"),
            model.version.to_string(),
        ),
        (
            crate::tr!(model, "UI", "界面"),
            "winit + egui + wgpu (Metal)".to_string(),
        ),
        (
            crate::tr!(model, "Terminal", "终端"),
            "alacritty_terminal + portable-pty".to_string(),
        ),
        ("Git", "gix (gitoxide)".to_string()),
        (
            crate::tr!(model, "Editor", "编辑器"),
            "ropey + syntect".to_string(),
        ),
        (crate::tr!(model, "License", "许可证"), "MIT".to_string()),
    ];
    for (label, value) in rows {
        ui.painter().text(
            Pos2::new(rect.left(), y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(11.0),
            p.text3,
        );
        ui.painter().text(
            Pos2::new(rect.left() + 130.0, y),
            Align2::LEFT_CENTER,
            value,
            FontId::monospace(10.5),
            p.text2,
        );
        y += 28.0;
    }
    y += 16.0;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        crate::tr!(
            model,
            "No WebView, no bundled JavaScript, no telemetry.",
            "无 WebView、无内置 JavaScript、无遥测。"
        ),
        FontId::proportional(11.0),
        p.text2,
    );
    y += 22.0;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        "github.com/jiangtaiduo/LazeCode",
        FontId::proportional(10.5),
        p.blue,
    );
    let _ = (primary_button, text_clipped);
}
