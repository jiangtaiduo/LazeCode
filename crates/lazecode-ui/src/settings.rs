use egui::{
    Align2, Color32, FontId, Pos2, Rect, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2,
};
use lazecode_core::{AppModel, Language, ThemeMode, ThemeSkin};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::UiRuntime;
use crate::theme::Theme;
use crate::widgets::{
    checkbox, icon_button, primary_button, secondary_button, secondary_button_enabled, text_clipped,
};

const SECTIONS_EN: [&str; 8] = [
    "Appearance",
    "Editor",
    "Terminal",
    "Coding Agents",
    "Files",
    "Shortcuts",
    "Updates",
    "About",
];
const SECTIONS_ZH: [&str; 8] = [
    "外观",
    "编辑器",
    "终端",
    "编码智能体",
    "文件",
    "快捷键",
    "更新",
    "关于",
];

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
    let sections: &[&str; 8] = if model.settings.language.is_zh() {
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
        1 => editor(ui, model, rt, theme, content),
        2 => terminal(ui, model, rt, theme, content),
        3 => agents(ui, model, theme, content),
        4 => files(ui, model, rt, theme, content),
        5 => keybindings_section(ui, model, rt, theme, content),
        6 => updates(ui, model, rt, theme, content),
        _ => about(ui, model, theme, content),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FontTarget {
    Ui,
    Editor,
    Terminal,
}

struct FontPicker<'a> {
    rect: Rect,
    y: f32,
    label: &'a str,
    target: FontTarget,
    monospaced_only: bool,
}

/// Family picker used by Appearance / Editor / Terminal. The choice applies
/// on the next frame through `UiRuntime::fonts_dirty`.
fn font_picker(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    picker: FontPicker<'_>,
) {
    let FontPicker {
        rect,
        y,
        label,
        target,
        monospaced_only,
    } = picker;
    let p = theme.palette;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(11.5),
        p.text2,
    );
    let current = match target {
        FontTarget::Ui => model.settings.ui_font.clone(),
        FontTarget::Editor => model.settings.editor.font_family.clone(),
        FontTarget::Terminal => model.settings.terminal.font_family.clone(),
    };
    let default_label = match target {
        FontTarget::Ui => crate::tr!(model, "System default", "系统默认"),
        FontTarget::Editor => crate::tr!(model, "System monospace", "系统等宽"),
        FontTarget::Terminal => crate::tr!(model, "Match editor", "跟随编辑器"),
    };
    let selected_text = if current.is_empty() {
        if target == FontTarget::Terminal && !model.settings.editor.font_family.is_empty() {
            format!("{} ({})", default_label, model.settings.editor.font_family)
        } else {
            default_label.to_string()
        }
    } else {
        current.clone()
    };

    let field = Rect::from_min_size(
        Pos2::new(rect.left() + 160.0, y - 13.0),
        Vec2::new(260.0, 26.0),
    );
    let id = match target {
        FontTarget::Ui => "settings-ui-font",
        FontTarget::Editor => "settings-editor-font",
        FontTarget::Terminal => "settings-terminal-font",
    };
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(field)
            .id_salt(id)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let filter_id = ui.make_persistent_id((id, "filter"));
    let mut filter = ui.data(|data| data.get_temp::<String>(filter_id).unwrap_or_default());
    let mut chosen: Option<String> = None;

    egui::ComboBox::from_id_salt(id)
        .width(field.width())
        .selected_text(egui::RichText::new(selected_text).color(p.text))
        .show_ui(&mut child, |ui| {
            ui.set_min_width(260.0);
            ui.add(
                egui::TextEdit::singleline(&mut filter)
                    .align(egui::Align2::LEFT_CENTER)
                    .hint_text(crate::tr!(model, "Search fonts", "搜索字体"))
                    .desired_width(f32::INFINITY),
            );
            let needle = filter.trim().to_lowercase();
            if ui
                .selectable_label(current.is_empty(), default_label)
                .clicked()
            {
                chosen = Some(String::new());
            }
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt((id, "list"))
                .max_height(260.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut any = false;
                    for family in crate::fonts::families(monospaced_only) {
                        if !needle.is_empty() && !family.to_lowercase().contains(&needle) {
                            continue;
                        }
                        any = true;
                        if ui.selectable_label(current == family, family).clicked() {
                            chosen = Some(family.to_string());
                        }
                    }
                    if !any {
                        ui.weak(crate::tr!(model, "No matching fonts", "没有匹配的字体"));
                    }
                });
        });
    ui.data_mut(|data| data.insert_temp(filter_id, filter));

    if let Some(font) = chosen {
        match target {
            FontTarget::Ui => model.settings.ui_font = font,
            FontTarget::Editor => model.settings.editor.font_family = font,
            FontTarget::Terminal => model.settings.terminal.font_family = font,
        }
        model.save_settings();
        rt.fonts_dirty = true;
    }
}

fn option_button(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    selected: bool,
    icon: Option<Icon>,
    theme: &Theme,
) -> egui::Response {
    if selected {
        primary_button(ui, rect, label, icon, true, theme)
    } else {
        secondary_button(ui, rect, label, icon, theme)
    }
}

fn scrollback_options(current: usize) -> Vec<usize> {
    let mut options = vec![5_000usize, 10_000, 20_000, 50_000];
    if !options.contains(&current) {
        options.push(current);
        options.sort_unstable();
    }
    options
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

/// Miniature of one theme variant used by the paired picker cards.
fn skin_card_preview(painter: &egui::Painter, rect: Rect, palette: &crate::theme::Palette) {
    let p = *palette;
    painter.rect_filled(rect, egui::CornerRadius::same(4), p.bg);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(4),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    let header = Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.top() + 8.0));
    painter.rect_filled(
        header,
        egui::CornerRadius {
            nw: 4,
            ne: 4,
            sw: 0,
            se: 0,
        },
        p.elev,
    );
    let sidebar = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top() + 8.0),
        Pos2::new(rect.left() + rect.width() * 0.26, rect.bottom()),
    );
    painter.rect_filled(sidebar, egui::CornerRadius::same(0), p.panel);
    let editor = Rect::from_min_max(
        Pos2::new(sidebar.right() + 2.0, rect.top() + 12.0),
        Pos2::new(rect.right() - 3.0, rect.bottom() - 3.0),
    );
    painter.rect_filled(editor, egui::CornerRadius::same(2), p.editor);
    let lines = [
        (0.10, 0.55, p.text2),
        (0.28, 0.38, p.accent),
        (0.10, 0.72, p.text),
        (0.32, 0.30, p.tok_str),
        (0.10, 0.44, p.tok_kw),
    ];
    for (index, (indent, width, color)) in lines.iter().enumerate() {
        let top = editor.top() + 3.0 + index as f32 * 5.0;
        if top + 2.0 > editor.bottom() {
            break;
        }
        let left = editor.left() + 3.0 + editor.width() * indent;
        let right = (left + editor.width() * width).min(editor.right() - 3.0);
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(left, top), Pos2::new(right, top + 2.0)),
            egui::CornerRadius::same(1),
            *color,
        );
    }
    painter.rect_filled(
        Rect::from_min_max(
            Pos2::new(rect.right() - 18.0, rect.top() + 2.5),
            Pos2::new(rect.right() - 5.0, rect.top() + 6.5),
        ),
        egui::CornerRadius::same(1),
        p.accent,
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
    let is_dark = model.settings.theme == ThemeMode::Dark;
    if option_button(
        ui,
        dark_rect,
        crate::tr!(model, "Dark", "深色"),
        is_dark,
        Some(Icon::Moon),
        theme,
    )
    .clicked()
        && !is_dark
    {
        model.settings.theme = ThemeMode::Dark;
        rt.applied_theme = None;
        model.save_settings();
    }
    if option_button(
        ui,
        light_rect,
        crate::tr!(model, "Light", "浅色"),
        !is_dark,
        Some(Icon::Sun),
        theme,
    )
    .clicked()
        && is_dark
    {
        model.settings.theme = ThemeMode::Light;
        rt.applied_theme = None;
        model.save_settings();
    }
    y += 42.0;
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Theme pair", "主题配色"),
        FontId::proportional(11.5),
        p.text2,
    );
    ui.painter().text(
        Pos2::new(rect.left() + 160.0, y),
        Align2::LEFT_CENTER,
        crate::tr!(
            model,
            "One selection applies to dark and light.",
            "一次选择同时应用于深色与浅色。"
        ),
        FontId::proportional(10.0),
        p.text3,
    );
    let current_skin = model.settings.skin();
    let zh = model.settings.language.is_zh();
    let gap = 8.0;
    let cols = 3usize;
    let card_w = ((rect.width() - gap * (cols as f32 - 1.0)) / cols as f32).max(120.0);
    let card_h = 78.0;
    let grid_top = y + 10.0;
    let mut chosen_skin = None;
    for (index, skin) in ThemeSkin::ALL.into_iter().enumerate() {
        let col = index % cols;
        let row = index / cols;
        let card = Rect::from_min_size(
            Pos2::new(
                rect.left() + col as f32 * (card_w + gap),
                grid_top + row as f32 * (card_h + gap),
            ),
            Vec2::new(card_w, card_h),
        );
        let response = ui.interact(
            card,
            ui.scope_id().with(("theme-card", skin.id())),
            Sense::click(),
        );
        let active = skin == current_skin;
        ui.painter().rect_filled(
            card,
            egui::CornerRadius::same(6),
            if active { p.selected_row } else { p.elev },
        );
        ui.painter().rect_stroke(
            card,
            egui::CornerRadius::same(6),
            Stroke::new(
                if active { 1.5 } else { 1.0 },
                if active { p.accent } else { p.border },
            ),
            StrokeKind::Inside,
        );
        crate::widgets::text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(card.left() + 8.0, card.top() + 2.0),
                Pos2::new(card.right() - 14.0, card.top() + 20.0),
            ),
            crate::skins::display_name(skin, zh),
            FontId::proportional(11.0),
            if active { p.text } else { p.text2 },
            Align2::LEFT_CENTER,
        );
        if active {
            ui.painter().circle_filled(
                Pos2::new(card.right() - 9.0, card.top() + 10.0),
                3.0,
                p.accent,
            );
        }
        let preview_w = ((card.width() - 24.0) / 2.0).max(1.0);
        let preview_h = (card.height() - 30.0).max(1.0);
        let preview_y = card.top() + 24.0;
        skin_card_preview(
            ui.painter(),
            Rect::from_min_size(
                Pos2::new(card.left() + 8.0, preview_y),
                Vec2::new(preview_w, preview_h),
            ),
            &crate::skins::palette(skin, true),
        );
        skin_card_preview(
            ui.painter(),
            Rect::from_min_size(
                Pos2::new(card.left() + 16.0 + preview_w, preview_y),
                Vec2::new(preview_w, preview_h),
            ),
            &crate::skins::palette(skin, false),
        );
        if response.clicked() {
            chosen_skin = Some(skin);
        }
    }
    if let Some(skin) = chosen_skin {
        model.settings.set_skin(skin);
        rt.applied_theme = None;
        model.save_settings();
    }
    let rows = ThemeSkin::ALL.len().div_ceil(cols);
    y = grid_top + rows as f32 * (card_h + gap) + 8.0;
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
    let is_english = model.settings.language == Language::English;
    if option_button(ui, en_rect, "English", is_english, None, theme).clicked() && !is_english {
        model.set_language(Language::English);
        rt.fonts_dirty = true;
    }
    if option_button(ui, zh_rect, "中文", !is_english, None, theme).clicked() && is_english {
        model.set_language(Language::Chinese);
        rt.fonts_dirty = true;
    }
    y += 42.0;
    font_picker(
        ui,
        model,
        rt,
        theme,
        FontPicker {
            rect,
            y,
            label: crate::tr!(model, "Interface font", "界面字体"),
            target: FontTarget::Ui,
            monospaced_only: false,
        },
    );
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

fn editor(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
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

fn terminal(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
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

fn agents(ui: &mut Ui, model: &mut AppModel, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    heading(
        ui,
        theme,
        rect,
        crate::tr!(model, "Coding Agents", "编码智能体"),
        crate::tr!(
            model,
            "Entries shown in the Coding Agent menu. An empty command opens a login shell.",
            "编码智能体菜单中的选项；命令留空时打开登录 Shell。"
        ),
    );

    let header_y = rect.top() + 66.0;
    ui.painter().text(
        Pos2::new(rect.left(), header_y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Name", "名称"),
        FontId::proportional(10.5),
        p.text3,
    );
    ui.painter().text(
        Pos2::new(rect.left() + 186.0, header_y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Command / script", "命令 / 脚本"),
        FontId::proportional(10.5),
        p.text3,
    );

    let list = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top() + 80.0),
        Pos2::new(rect.right(), rect.bottom() - 48.0),
    );
    let mut remove = None;
    let mut save = false;
    super::app_ui::region(ui, list, "settings-agents", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("settings-agents-list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for (index, option) in model.settings.agents.options.iter_mut().enumerate() {
                    let (row, _) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 38.0), Sense::hover());
                    let name_rect = Rect::from_min_size(
                        Pos2::new(row.left(), row.top() + 5.0),
                        Vec2::new(174.0, 28.0),
                    );
                    ui.painter()
                        .rect_filled(name_rect, egui::CornerRadius::same(4), p.panel2);
                    let command_rect = Rect::from_min_size(
                        Pos2::new(row.left() + 182.0, row.top() + 5.0),
                        Vec2::new((row.width() - 214.0).max(80.0), 28.0),
                    );
                    ui.painter()
                        .rect_filled(command_rect, egui::CornerRadius::same(4), p.panel2);
                    let name_response = ui.put(
                        name_rect,
                        TextEdit::singleline(&mut option.name)
                            .frame(egui::Frame::NONE)
                            .align(egui::Align2::LEFT_CENTER)
                            .font(FontId::proportional(11.5))
                            .hint_text(crate::tr!(model, "Agent name", "智能体名称")),
                    );
                    ui.painter().rect_stroke(
                        name_rect,
                        egui::CornerRadius::same(4),
                        Stroke::new(1.0, p.border_soft),
                        StrokeKind::Inside,
                    );
                    let command_response = ui.put(
                        command_rect,
                        TextEdit::singleline(&mut option.command)
                            .frame(egui::Frame::NONE)
                            .align(egui::Align2::LEFT_CENTER)
                            .font(FontId::monospace(11.0))
                            .hint_text(crate::tr!(model, "login shell", "登录 Shell")),
                    );
                    ui.painter().rect_stroke(
                        command_rect,
                        egui::CornerRadius::same(4),
                        Stroke::new(1.0, p.border_soft),
                        StrokeKind::Inside,
                    );
                    if name_response.lost_focus() || command_response.lost_focus() {
                        save = true;
                    }
                    let delete_rect = Rect::from_center_size(
                        Pos2::new(row.right() - 12.0, row.center().y),
                        Vec2::splat(22.0),
                    );
                    if icon_button(ui, delete_rect, Icon::X, false, theme)
                        .on_hover_text(crate::tr!(model, "Remove", "移除"))
                        .clicked()
                    {
                        remove = Some(index);
                    }
                }
            });
    });
    if let Some(index) = remove {
        if index < model.settings.agents.options.len() {
            model.settings.agents.options.remove(index);
            model.save_settings();
        }
    } else if save {
        model.save_settings();
    }

    let add = Rect::from_min_size(
        Pos2::new(rect.left(), rect.bottom() - 34.0),
        Vec2::new(104.0, 28.0),
    );
    if secondary_button(
        ui,
        add,
        crate::tr!(model, "Add agent", "添加智能体"),
        Some(Icon::Plus),
        theme,
    )
    .clicked()
    {
        model
            .settings
            .agents
            .options
            .push(lazecode_core::settings::AgentProfile {
                name: crate::tr!(model, "New agent", "新智能体").to_string(),
                description: String::new(),
                command: String::new(),
            });
        model.save_settings();
    }
    let reset = Rect::from_min_size(
        Pos2::new(add.right() + 8.0, rect.bottom() - 34.0),
        Vec2::new(132.0, 28.0),
    );
    if secondary_button(
        ui,
        reset,
        crate::tr!(model, "Restore defaults", "恢复默认"),
        None,
        theme,
    )
    .clicked()
    {
        model.settings.agents = lazecode_core::settings::AgentSettings::default();
        model.save_settings();
    }
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
            .align(egui::Align2::LEFT_CENTER)
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
            .align(egui::Align2::LEFT_CENTER)
            .hint_text(hint)
            .font(FontId::monospace(10.5)),
    );
    let entered = response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    response.lost_focus() || entered
}

/// 设置页「快捷键」分区：列出全部可改键的动作，点击「修改」进入录入态
/// （下一次按键即生效，Esc 取消），冲突会被拒绝并弹 toast。
fn keybindings_section(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
) {
    use crate::keybindings;

    let p = theme.palette;
    heading(
        ui,
        theme,
        rect,
        crate::tr!(model, "Shortcuts", "快捷键"),
        crate::tr!(
            model,
            "Click Change, then press the new combo. Esc cancels; duplicates are rejected.",
            "点击「修改」后按下新的组合键；Esc 取消，重复的键会被拒绝。"
        ),
    );
    let reset_all = Rect::from_min_size(
        Pos2::new(rect.right() - 130.0, rect.top() + 8.0),
        Vec2::new(130.0, 24.0),
    );
    if secondary_button(
        ui,
        reset_all,
        crate::tr!(model, "Reset all", "全部恢复默认"),
        None,
        theme,
    )
    .clicked()
    {
        model.settings.keybindings.clear();
        model.save_settings();
    }

    let mut y = rect.top() + 74.0;
    if let Some(action) = rt.keybind_capture {
        ui.painter().text(
            Pos2::new(rect.left(), y),
            Align2::LEFT_CENTER,
            format!(
                "{}{}",
                crate::tr!(
                    model,
                    "Recording: press the new combo for ",
                    "录入中：请为「"
                ),
                {
                    let name = keybindings::label(model, action);
                    if model.settings.language.is_zh() {
                        format!("{name}」")
                    } else {
                        name
                    }
                }
            ),
            FontId::proportional(11.5),
            p.accent,
        );
        y += 30.0;
    }

    // 分区都是绝对坐标绘制；把滚动区钉在 content 里剩余的空间上。
    let list_rect = Rect::from_min_max(
        Pos2::new(rect.left(), y),
        Pos2::new(rect.right(), rect.bottom()),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(list_rect)
            .id_salt("settings-keybindings"),
    );
    ScrollArea::vertical()
        .id_salt("settings-keybindings-scroll")
        .auto_shrink([false, false])
        .show(&mut child, |ui| {
            ui.add_space(4.0);
            for action in keybindings::ALL_ACTIONS {
                let (row, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::hover());
                let recording = rt.keybind_capture == Some(action);
                if recording {
                    ui.painter().rect_filled(
                        row,
                        egui::CornerRadius::same(5),
                        p.accent.gamma_multiply(0.12),
                    );
                } else if ui
                    .interact(
                        row,
                        ui.scope_id().with(("kb-row", action.id())),
                        Sense::hover(),
                    )
                    .hovered()
                {
                    ui.painter()
                        .rect_filled(row, egui::CornerRadius::same(5), p.hover);
                }
                ui.painter().text(
                    Pos2::new(row.left() + 6.0, row.center().y),
                    Align2::LEFT_CENTER,
                    keybindings::label(model, action),
                    FontId::proportional(11.5),
                    p.text,
                );
                let display = keybindings::binding_display(keybindings::resolved(model, action));
                let empty = display.is_empty();
                ui.painter().text(
                    Pos2::new(row.right() - 190.0, row.center().y),
                    Align2::RIGHT_CENTER,
                    if empty {
                        crate::tr!(model, "unbound", "未设置").to_string()
                    } else {
                        display
                    },
                    FontId::monospace(11.5),
                    if empty { p.text3 } else { p.text2 },
                );
                let change_rect = Rect::from_min_size(
                    Pos2::new(row.right() - 128.0, row.center().y - 10.0),
                    Vec2::new(58.0, 20.0),
                );
                let change_label = if recording {
                    crate::tr!(model, "Cancel", "取消")
                } else {
                    crate::tr!(model, "Change", "修改")
                };
                if secondary_button(ui, change_rect, change_label, None, theme).clicked() {
                    rt.keybind_capture = if recording { None } else { Some(action) };
                }
                let has_override = model.settings.keybindings.contains_key(action.id());
                let reset_rect = Rect::from_min_size(
                    Pos2::new(row.right() - 62.0, row.center().y - 10.0),
                    Vec2::new(58.0, 20.0),
                );
                if secondary_button_enabled(
                    ui,
                    reset_rect,
                    crate::tr!(model, "Default", "默认"),
                    None,
                    has_override,
                    theme,
                )
                .clicked()
                {
                    model.settings.keybindings.remove(action.id());
                    model.save_settings();
                }
            }
        });
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
    let auto_label = crate::tr!(model, "Check automatically", "自动检查更新");
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
            "Five seconds after launch, once per launch (no download starts).",
            "启动 5 秒后检查，每次启动一次（不会自动下载）。"
        ),
        FontId::proportional(10.5),
        p.text3,
    );

    y += 30.0;
    let checked = if model.settings.last_update_check_time > 0 {
        crate::update_ui::checked_label(
            Some(model.settings.last_update_check_time),
            model.settings.language.is_zh(),
        )
    } else {
        crate::tr!(model, "never", "从未").to_string()
    };
    ui.painter().text(
        Pos2::new(rect.left(), y),
        Align2::LEFT_CENTER,
        if model.settings.language.is_zh() {
            format!("上次检查：{checked}")
        } else {
            format!("Last checked: {checked}")
        },
        FontId::proportional(10.5),
        p.text3,
    );

    if !model.settings.ignored_update_version.is_empty() {
        y += 24.0;
        let ignored = model.settings.ignored_update_version.clone();
        ui.painter().text(
            Pos2::new(rect.left(), y),
            Align2::LEFT_CENTER,
            if model.settings.language.is_zh() {
                format!("已忽略版本：v{ignored}")
            } else {
                format!("Ignored version: v{ignored}")
            },
            FontId::proportional(10.5),
            p.text3,
        );
        let clear_rect = Rect::from_min_size(
            Pos2::new(rect.left() + 160.0, y - 11.0),
            Vec2::new(56.0, 22.0),
        );
        if secondary_button(
            ui,
            clear_rect,
            crate::tr!(model, "Clear", "清除"),
            None,
            theme,
        )
        .clicked()
        {
            model.settings.ignored_update_version.clear();
            rt.update.ignore_checked = false;
            model.save_settings();
        }
    }

    y += 36.0;
    let open_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 160.0, y - 13.0),
        Vec2::new(170.0, 26.0),
    );
    if secondary_button_enabled(
        ui,
        open_rect,
        crate::tr!(model, "Check for updates…", "检查更新…"),
        None,
        !rt.update.busy(),
        theme,
    )
    .clicked()
    {
        rt.update_open = true;
        let version = model.version;
        if rt.update.begin_check(&mut model.settings, version, false) {
            model.save_settings();
        }
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
        let note = crate::tr!(
            model,
            "Updates stay disabled until both fields are filled in. Only HTTPS/HTTP, file:// URLs and local paths are accepted; artifacts are SHA-256 checked and Ed25519 verified.",
            "两项都填写后才会启用更新。仅接受 HTTPS/HTTP、file:// 或本地路径；下载后会校验 SHA-256 与 Ed25519 签名。"
        );
        let galley = ui.painter().layout(
            note.to_string(),
            FontId::proportional(10.5),
            p.amber,
            rect.width(),
        );
        ui.painter()
            .with_clip_rect(rect)
            .galley(Pos2::new(rect.left(), y - 7.0), galley, p.amber);
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

#[cfg(test)]
mod skin_ui_tests {
    use super::*;
    use lazecode_core::Settings;

    #[test]
    fn appearance_screen_renders_for_every_skin() {
        let mut model = AppModel::new(Settings::default(), None);
        model.ui.settings_section = 0;
        let mut rt = UiRuntime::default();
        for mode in [ThemeMode::Dark, ThemeMode::Light] {
            for skin in ThemeSkin::ALL {
                model.settings.theme = mode;
                model.settings.set_skin(skin);
                let theme = Theme::new(mode, skin);
                let ctx = egui::Context::default();
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 700.0))),
                    ..Default::default()
                };
                let mut output = ctx.run_ui(input, |ui| {
                    draw(ui, &mut model, &mut rt, &theme);
                });
                let shapes = output.shapes.len();
                output.textures_delta.clear();
                assert!(
                    shapes > 60,
                    "{} {:?}: settings screen drew {} shapes",
                    skin.id(),
                    mode,
                    shapes
                );
            }
        }
    }

    #[test]
    fn scrollback_options_keep_default_and_custom_value_selectable() {
        let defaults = scrollback_options(10_000);
        assert_eq!(defaults, vec![5_000, 10_000, 20_000, 50_000]);
        let custom = scrollback_options(100_000);
        assert_eq!(custom, vec![5_000, 10_000, 20_000, 50_000, 100_000]);
        let odd = scrollback_options(1_500);
        assert_eq!(odd, vec![1_500, 5_000, 10_000, 20_000, 50_000]);
    }
}
