//! Editor settings section.

use super::prelude::*;
use super::widgets::*;

pub(super) fn editor(
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
        crate::tr!(model, "Editor", "编辑器"),
        crate::tr!(
            model,
            "Typing, indentation and syntax highlighting.",
            "输入、缩进与语法高亮。"
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
            target: FontTarget::Editor,
            monospaced_only: true,
        },
    );
    y += 38.0;
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
        model.settings.editor.font_size = (model.settings.editor.font_size - 0.5).max(9.0);
        model.save_settings();
    }
    ui.painter().text(
        Pos2::new(minus.right() + 18.0, y),
        Align2::CENTER_CENTER,
        format!("{:.0}", model.settings.editor.font_size),
        FontId::monospace(11.0),
        p.text,
    );
    let plus = Rect::from_min_size(
        Pos2::new(minus.right() + 46.0, y - 13.0),
        Vec2::new(28.0, 26.0),
    );
    if secondary_button(ui, plus, "+", None, theme).clicked() {
        model.settings.editor.font_size = (model.settings.editor.font_size + 0.5).min(24.0);
        model.save_settings();
    }
    y += 38.0;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Tab width", "制表宽度"),
        FontId::proportional(11.5),
        p.text2,
    );
    let minus = Rect::from_min_size(
        Pos2::new(rect.left() + 160.0, y - 13.0),
        Vec2::new(28.0, 26.0),
    );
    if secondary_button(ui, minus, "−", None, theme).clicked() {
        model.settings.editor.tab_width = model.settings.editor.tab_width.saturating_sub(1).max(1);
        model.save_settings();
    }
    ui.painter().text(
        Pos2::new(minus.right() + 18.0, y),
        Align2::CENTER_CENTER,
        model.settings.editor.tab_width.to_string(),
        FontId::monospace(11.0),
        p.text,
    );
    let plus = Rect::from_min_size(
        Pos2::new(minus.right() + 46.0, y - 13.0),
        Vec2::new(28.0, 26.0),
    );
    if secondary_button(ui, plus, "+", None, theme).clicked() {
        model.settings.editor.tab_width = (model.settings.editor.tab_width + 1).min(8);
        model.save_settings();
    }
    y += 38.0;
    let toggles = [
        (
            crate::tr!(model, "Insert spaces", "使用空格"),
            model.settings.editor.use_spaces,
        ),
        (
            crate::tr!(model, "Line numbers", "行号"),
            model.settings.editor.show_line_numbers,
        ),
        (
            crate::tr!(model, "Highlight current line", "高亮当前行"),
            model.settings.editor.highlight_current_line,
        ),
        (
            crate::tr!(model, "Auto save", "自动保存"),
            model.settings.editor.auto_save,
        ),
        (
            crate::tr!(
                model,
                "Language servers (experimental)",
                "语言服务器（实验性）"
            ),
            model.settings.lsp_enabled,
        ),
    ];
    for (index, (label, current)) in toggles.into_iter().enumerate() {
        ui.painter().text(
            Pos2::new(rect.left(), y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(11.5),
            p.text2,
        );
        let check = Rect::from_center_size(Pos2::new(rect.left() + 170.0, y), Vec2::splat(18.0));
        if checkbox(ui, check, current, theme).clicked() {
            match index {
                0 => model.settings.editor.use_spaces = !current,
                1 => model.settings.editor.show_line_numbers = !current,
                2 => model.settings.editor.highlight_current_line = !current,
                3 => model.settings.editor.auto_save = !current,
                _ => model.settings.lsp_enabled = !current,
            }
            model.lsp.set_enabled(model.settings.lsp_enabled);
            model.save_settings();
        }
        y += 34.0;
    }
}
