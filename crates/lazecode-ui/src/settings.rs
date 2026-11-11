use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2};
use lazecode_core::{AppModel, Language, ThemeMode};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::UiRuntime;
use crate::theme::Theme;
use crate::widgets::{checkbox, primary_button, secondary_button, text_clipped};

const SECTIONS_EN: [&str; 6] = [
    "Appearance",
    "Editor",
    "Terminal",
    "Files",
    "Updates",
    "About",
];
const SECTIONS_ZH: [&str; 6] = ["外观", "编辑器", "终端", "文件", "更新", "关于"];

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme) {
    let p = theme.palette;
    let screen = ui.max_rect();
    ui.painter()
        .rect_filled(screen, 0.0, Color32::from_black_alpha(120));
    let panel = Rect::from_center_size(
        screen.center(),
        Vec2::new(
            screen.width().min(880.0) - 80.0,
            screen.height().min(640.0) - 80.0,
        ),
    );
    ui.painter()
        .rect_filled(panel, egui::CornerRadius::same(12), p.elev);
    ui.painter().rect_stroke(
        panel,
        egui::CornerRadius::same(12),
        Stroke::new(1.0, p.border_strong),
        StrokeKind::Inside,
    );
    let nav = Rect::from_min_max(panel.min, Pos2::new(panel.left() + 190.0, panel.bottom()));
    ui.painter().rect_filled(
        nav,
        egui::CornerRadius {
            nw: 12,
            sw: 12,
            ne: 0,
            se: 0,
        },
        p.panel,
    );
    ui.painter().text(
        Pos2::new(nav.left() + 22.0, nav.top() + 34.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Settings", "设置"),
        FontId::proportional(18.0),
        p.text,
    );
    let mut y = nav.top() + 66.0;
    let sections: &[&str; 6] = if model.settings.language.is_zh() {
        &SECTIONS_ZH
    } else {
        &SECTIONS_EN
    };
    for (index, label) in sections.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(nav.left() + 10.0, y),
            Vec2::new(nav.width() - 20.0, 32.0),
        );
        let response = ui.interact(
            row,
            ui.scope_id().with(("settings-section", index)),
            Sense::click(),
        );
        if model.ui.settings_section == index {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(5), p.active);
            ui.painter().rect_filled(
                Rect::from_min_size(Pos2::new(row.left(), row.top() + 6.0), Vec2::new(2.0, 20.0)),
                egui::CornerRadius::same(1),
                p.accent,
            );
        } else if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(5), p.hover);
        }
        ui.painter().text(
            Pos2::new(row.left() + 16.0, row.center().y),
            Align2::LEFT_CENTER,
            *label,
            FontId::proportional(12.0),
            if model.ui.settings_section == index {
                p.text
            } else {
                p.text2
            },
        );
        if response.clicked() {
            model.ui.settings_section = index;
        }
        y += 35.0;
    }
    ui.painter().text(
        Pos2::new(nav.left() + 22.0, nav.bottom() - 24.0),
        Align2::LEFT_CENTER,
        format!("LazeCode {}", model.version),
        FontId::proportional(9.5),
        p.text3,
    );

    let content = Rect::from_min_max(
        Pos2::new(panel.left() + 214.0, panel.top() + 30.0),
        Pos2::new(panel.right() - 24.0, panel.bottom() - 24.0),
    );
    let close = Rect::from_center_size(
        Pos2::new(panel.right() - 20.0, panel.top() + 18.0),
        Vec2::splat(22.0),
    );
    let close_response = ui.interact(close, ui.scope_id().with("settings-close"), Sense::click());
    paint_icon(
        ui.painter(),
        close.shrink(4.0),
        Icon::X,
        if close_response.hovered() {
            p.text
        } else {
            p.text3
        },
    );
    if close_response.clicked() {
        rt.settings_open = false;
        model.save_settings();
    }

    match model.ui.settings_section {
        0 => appearance(ui, model, rt, theme, content),
        1 => editor(ui, model, theme, content),
        2 => terminal(ui, model, theme, content),
        3 => files(ui, model, rt, theme, content),
        4 => updates(ui, model, rt, theme, content),
        _ => about(ui, model, theme, content),
    }
}

fn heading(ui: &mut Ui, theme: &Theme, rect: Rect, title: &str, subtitle: &str) {
    let p = theme.palette;
    ui.painter().text(
        Pos2::new(rect.left(), rect.top() + 8.0),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(15.0),
        p.text,
    );
    ui.painter().text(
        Pos2::new(rect.left(), rect.top() + 28.0),
        Align2::LEFT_CENTER,
        subtitle,
        FontId::proportional(10.5),
        p.text3,
    );
    ui.painter().hline(
        rect.x_range(),
        rect.top() + 44.5,
        Stroke::new(1.0, p.border_soft),
    );
}

fn appearance(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    heading(
        ui,
        theme,
        rect,
        crate::tr!(model, "Appearance", "外观"),
        crate::tr!(
            model,
            "Theme, language and interface density.",
            "主题、语言与界面密度。"
        ),
    );
    let mut y = rect.top() + 62.0;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Theme", "主题"),
        FontId::proportional(11.5),
        p.text2,
    );
    let dark_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 160.0, y - 13.0),
        Vec2::new(96.0, 26.0),
    );
    let light_rect = Rect::from_min_size(
        Pos2::new(dark_rect.right() + 8.0, y - 13.0),
        Vec2::new(96.0, 26.0),
    );
    if secondary_button(
        ui,
        dark_rect,
        crate::tr!(model, "Dark", "深色"),
        Some(Icon::Moon),
        theme,
    )
    .clicked()
        && model.settings.theme != ThemeMode::Dark
    {
        model.settings.theme = ThemeMode::Dark;
        rt.applied_theme = None;
        model.save_settings();
    }
    if secondary_button(
        ui,
        light_rect,
        crate::tr!(model, "Light", "浅色"),
        Some(Icon::Sun),
        theme,
    )
    .clicked()
        && model.settings.theme != ThemeMode::Light
    {
        model.settings.theme = ThemeMode::Light;
        rt.applied_theme = None;
        model.save_settings();
    }
    y += 42.0;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Language", "语言"),
        FontId::proportional(11.5),
        p.text2,
    );
    let en_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 160.0, y - 13.0),
        Vec2::new(70.0, 26.0),
    );
    let zh_rect = Rect::from_min_size(
        Pos2::new(en_rect.right() + 8.0, y - 13.0),
        Vec2::new(70.0, 26.0),
    );
    if secondary_button(ui, en_rect, "English", None, theme).clicked() {
        model.set_language(Language::English);
    }
    if secondary_button(ui, zh_rect, "中文", None, theme).clicked() {
        model.set_language(Language::Chinese);
    }
    y += 42.0;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Minimap", "缩略图"),
        FontId::proportional(11.5),
        p.text2,
    );
    let check = Rect::from_center_size(Pos2::new(rect.left() + 170.0, y), Vec2::splat(18.0));
    if checkbox(ui, check, model.ui.show_minimap, theme).clicked() {
        model.ui.show_minimap = !model.ui.show_minimap;
        model.settings.editor.show_minimap = model.ui.show_minimap;
        model.save_settings();
    }
}

fn editor(ui: &mut Ui, model: &mut AppModel, theme: &Theme, rect: Rect) {
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

fn terminal(ui: &mut Ui, model: &mut AppModel, theme: &Theme, rect: Rect) {
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
    for value in [5_000usize, 10_000, 20_000, 50_000] {
        let button = Rect::from_min_size(Pos2::new(x, y - 13.0), Vec2::new(60.0, 26.0));
        if secondary_button(ui, button, &format!("{}k", value / 1000), None, theme).clicked() {
            model.settings.terminal.scrollback = value;
            model.save_settings();
        }
        x += 65.0;
    }
    y += 40.0;
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
    let b_resp = if is_bottom {
        primary_button(
            ui,
            bottom_btn,
            crate::tr!(model, "Bottom", "底部"),
            None,
            true,
            theme,
        )
    } else {
        secondary_button(
            ui,
            bottom_btn,
            crate::tr!(model, "Bottom", "底部"),
            None,
            theme,
        )
    };
    if b_resp.clicked() {
        model.ui.terminal_placement = lazecode_core::TerminalPlacement::Bottom;
        model.save_workspace();
    }
    let right_btn = Rect::from_min_size(
        Pos2::new(bottom_btn.right() + 8.0, y - 13.0),
        Vec2::new(96.0, 26.0),
    );
    let r_resp = if !is_bottom {
        primary_button(
            ui,
            right_btn,
            crate::tr!(model, "Right", "右侧"),
            None,
            true,
            theme,
        )
    } else {
        secondary_button(
            ui,
            right_btn,
            crate::tr!(model, "Right", "右侧"),
            None,
            theme,
        )
    };
    if r_resp.clicked() {
        model.ui.terminal_placement = lazecode_core::TerminalPlacement::Right;
        model.save_workspace();
    }
    y += 44.0;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        "Sessions run in their own threads; the grid holds up to the selected scrollback in RAM.",
        FontId::proportional(10.0),
        p.text3,
    );
}

fn files(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    heading(
        ui,
        theme,
        rect,
        crate::tr!(model, "Files & Excludes", "文件与忽略规则"),
        crate::tr!(
            model,
            "Patterns excluded from file exploration and indexing.",
            "在资源管理器与索引中排除的忽略模式。"
        ),
    );
    let mut y = rect.top() + 64.0;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Add ignore pattern", "添加忽略模式"),
        FontId::proportional(11.5),
        p.text2,
    );

    let input_field = Rect::from_min_size(
        Pos2::new(rect.left() + 160.0, y - 13.0),
        Vec2::new(200.0, 26.0),
    );
    ui.painter()
        .rect_filled(input_field, egui::CornerRadius::same(5), p.panel2);
    ui.painter().rect_stroke(
        input_field,
        egui::CornerRadius::same(5),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    let text_resp = ui.put(
        input_field.shrink2(Vec2::new(7.0, 3.0)),
        TextEdit::singleline(&mut rt.new_ignore_pattern)
            .frame(egui::Frame::NONE)
            .hint_text("e.g. .venv, build, *.log")
            .font(FontId::monospace(10.5)),
    );
    let enter_pressed = text_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let add_btn = Rect::from_min_size(
        Pos2::new(input_field.right() + 8.0, y - 13.0),
        Vec2::new(60.0, 26.0),
    );
    let mut add_clicked =
        secondary_button(ui, add_btn, crate::tr!(model, "Add", "添加"), None, theme).clicked();
    if enter_pressed {
        add_clicked = true;
    }
    if add_clicked {
        let pattern = rt.new_ignore_pattern.trim();
        if !pattern.is_empty() && !model.settings.ignored_patterns.iter().any(|s| s == pattern) {
            model.settings.ignored_patterns.push(pattern.to_string());
            model.save_settings();
            model.queue(lazecode_core::Command::ReloadTree);
            rt.new_ignore_pattern.clear();
        }
    }

    let reset_btn = Rect::from_min_size(
        Pos2::new(add_btn.right() + 8.0, y - 13.0),
        Vec2::new(100.0, 26.0),
    );
    if secondary_button(
        ui,
        reset_btn,
        crate::tr!(model, "Reset defaults", "重置默认"),
        None,
        theme,
    )
    .clicked()
    {
        model.settings.ignored_patterns = vec![
            ".git".into(),
            ".idea".into(),
            ".DS_Store".into(),
            "node_modules".into(),
            ".venv".into(),
            "__pycache__".into(),
            "target".into(),
        ];
        model.save_settings();
        model.queue(lazecode_core::Command::ReloadTree);
    }

    y += 44.0;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Ignored patterns", "已忽略的模式"),
        FontId::proportional(11.5),
        p.text2,
    );

    let list_area = Rect::from_min_max(
        Pos2::new(rect.left() + 160.0, y - 13.0),
        Pos2::new(rect.right() - 20.0, rect.bottom() - 20.0),
    );
    let mut remove_index: Option<usize> = None;
    super::app_ui::region(ui, list_area, "settings-ignore-list", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("ignored-patterns-scroll")
            .show(ui, |ui| {
                for (idx, pat) in model.settings.ignored_patterns.iter().enumerate() {
                    let (row, _) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::hover());
                    ui.painter()
                        .rect_filled(row, egui::CornerRadius::same(4), p.panel2);
                    ui.painter().text(
                        Pos2::new(row.left() + 10.0, row.center().y),
                        Align2::LEFT_CENTER,
                        pat,
                        FontId::monospace(11.0),
                        p.text,
                    );
                    let del_btn = Rect::from_center_size(
                        Pos2::new(row.right() - 14.0, row.center().y),
                        Vec2::splat(16.0),
                    );
                    let del_resp = ui.interact(
                        del_btn,
                        ui.scope_id().with(("del-pat", idx)),
                        Sense::click(),
                    );
                    if del_resp.hovered() {
                        ui.painter().circle_filled(del_btn.center(), 8.0, p.hover);
                    }
                    ui.painter().text(
                        del_btn.center(),
                        Align2::CENTER_CENTER,
                        "×",
                        FontId::proportional(11.0),
                        if del_resp.hovered() {
                            p.text_bright
                        } else {
                            p.text3
                        },
                    );
                    if del_resp.clicked() {
                        remove_index = Some(idx);
                    }
                }
            });
    });
    if let Some(idx) = remove_index {
        model.settings.ignored_patterns.remove(idx);
        model.save_settings();
        model.queue(lazecode_core::Command::ReloadTree);
    }
}

/// Single-line labelled settings input. Returns `true` when the user
/// committed a change (focus lost or Enter).
fn labelled_input(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    y: f32,
    label: &str,
    hint: &str,
    value: &mut String,
) -> bool {
    let p = theme.palette;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(11.5),
        p.text2,
    );
    let input = Rect::from_min_size(
        Pos2::new(rect.left() + 160.0, y - 13.0),
        Vec2::new((rect.width() - 170.0).max(160.0), 26.0),
    );
    ui.painter()
        .rect_filled(input, egui::CornerRadius::same(5), p.panel2);
    ui.painter().rect_stroke(
        input,
        egui::CornerRadius::same(5),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    let response = ui.put(
        input.shrink2(Vec2::new(7.0, 3.0)),
        TextEdit::singleline(value)
            .frame(egui::Frame::NONE)
            .hint_text(hint)
            .font(FontId::monospace(10.5)),
    );
    let entered = response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    response.lost_focus() || entered
}

fn updates(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    heading(
        ui,
        theme,
        rect,
        crate::tr!(model, "Updates", "更新"),
        crate::tr!(
            model,
            "Signed release manifest: HTTPS endpoint plus the pinned Ed25519 public key.",
            "签名发布清单：更新地址与固定的 Ed25519 公钥。"
        ),
    );
    let endpoint_label = crate::tr!(model, "Endpoint", "更新地址");
    let key_label = crate::tr!(model, "Public key", "公钥");
    let auto_label = crate::tr!(model, "Check at startup", "启动时检查");
    let mut changed = false;
    let mut y = rect.top() + 74.0;
    changed |= labelled_input(
        ui,
        theme,
        rect,
        y,
        endpoint_label,
        "https://updates.example.com/manifest.json",
        &mut model.settings.update_endpoint,
    );
    y += 42.0;
    changed |= labelled_input(
        ui,
        theme,
        rect,
        y,
        key_label,
        "base64 or hex Ed25519 public key",
        &mut model.settings.update_public_key,
    );
    y += 42.0;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        auto_label,
        FontId::proportional(11.5),
        p.text2,
    );
    let check_rect =
        Rect::from_min_size(Pos2::new(rect.left() + 160.0, y - 9.0), Vec2::splat(18.0));
    if checkbox(ui, check_rect, model.settings.auto_check_updates, theme).clicked() {
        model.settings.auto_check_updates = !model.settings.auto_check_updates;
        changed = true;
    }
    ui.painter().text(
        Pos2::new(rect.left() + 186.0, y),
        Align2::LEFT_CENTER,
        crate::tr!(
            model,
            "Fetch the manifest once shortly after launch (no download starts).",
            "启动后自动获取一次清单（不会自动下载）。"
        ),
        FontId::proportional(10.5),
        p.text3,
    );

    y += 44.0;
    let open_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 160.0, y - 13.0),
        Vec2::new(150.0, 26.0),
    );
    if secondary_button(
        ui,
        open_rect,
        crate::tr!(model, "Open update dialog", "打开更新窗口"),
        None,
        theme,
    )
    .clicked()
    {
        rt.update_open = true;
    }

    y += 34.0;
    if model.settings.updates_configured() {
        ui.painter().text(
            Pos2::new(rect.left(), y),
            Align2::LEFT_CENTER,
            format!(
                "{} {}",
                crate::tr!(model, "Signing key:", "签名公钥："),
                match model.settings.update_public_key.len() {
                    0 => crate::tr!(model, "not set", "未设置").to_string(),
                    len => format!("{len} chars"),
                }
            ),
            FontId::proportional(10.5),
            p.text3,
        );
    } else {
        ui.painter().text(
            Pos2::new(rect.left(), y),
            Align2::LEFT_CENTER,
            crate::tr!(
                model,
                "Updates stay disabled until both fields are filled in. Only HTTPS, file:// URLs and local paths are accepted; artifacts are SHA-256 checked and Ed25519 verified.",
                "两项都填写后才会启用更新。仅接受 HTTPS、file:// 或本地路径；下载后会校验 SHA-256 与 Ed25519 签名。"
            ),
            FontId::proportional(10.5),
            p.amber,
        );
    }

    if changed {
        model.save_settings();
    }
}

fn about(ui: &mut Ui, model: &AppModel, theme: &Theme, rect: Rect) {
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
