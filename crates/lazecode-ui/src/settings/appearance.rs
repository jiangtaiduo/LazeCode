//! Appearance section: theme pairs and skin cards.

use super::prelude::*;
use super::widgets::*;

/// Miniature of one theme variant used by the paired picker cards.
pub(super) fn skin_card_preview(
    painter: &egui::Painter,
    rect: Rect,
    palette: &crate::theme::Palette,
) {
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

pub(super) fn appearance(
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
