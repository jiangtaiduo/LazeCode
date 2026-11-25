//! Find bar: query/options UI, match navigation and jump-to-match.

use super::prelude::*;

pub(super) fn draw_find(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    project: &mut ProjectState,
    tab_id: u64,
    rt: &mut UiRuntime,
    lang: lazecode_core::Language,
) {
    let p = theme.palette;
    let Some(tab) = project.tab_mut_by_id(tab_id) else {
        return;
    };
    let TabKind::File(file_tab) = &mut tab.kind else {
        return;
    };
    let doc = &mut file_tab.doc;
    let panel = Rect::from_min_size(
        Pos2::new(rect.right() - 446.0, rect.top() + 8.0),
        Vec2::new(434.0, 106.0),
    );
    ui.painter()
        .rect_filled(panel, egui::CornerRadius::same(8), p.elev);
    ui.painter().rect_stroke(
        panel,
        egui::CornerRadius::same(8),
        Stroke::new(1.0, p.border_strong),
        StrokeKind::Inside,
    );

    // Row 1: Find input + count + close button
    let input_rect = Rect::from_min_size(
        Pos2::new(panel.left() + 10.0, panel.top() + 8.0),
        Vec2::new(panel.width() - 44.0, 24.0),
    );
    ui.painter()
        .rect_filled(input_rect, egui::CornerRadius::same(5), p.panel2);
    let query_rect = Rect::from_min_max(
        Pos2::new(input_rect.left() + 8.0, input_rect.top() + 2.0),
        Pos2::new(input_rect.right() - 70.0, input_rect.bottom() - 2.0),
    );
    let response = ui.put(
        query_rect,
        TextEdit::singleline(&mut rt.find.query)
            .frame(egui::Frame::NONE)
            .align(egui::Align2::LEFT_CENTER)
            .hint_text(crate::i18n::t(lang, "Find", "查找"))
            .font(FontId::proportional(12.5)),
    );
    if rt.find.focus_requested {
        response.request_focus();
        rt.find.focus_requested = false;
    }
    let signature = format!(
        "{}|{}|{}|{}|{}",
        rt.find.query, rt.find.case_sensitive, rt.find.regex, rt.find.whole_word, doc.revision
    );
    if signature != rt.find.last_signature {
        rt.find.last_signature = signature;
        rt.find.matches = doc.find_all(
            &rt.find.query,
            MatchOptions {
                case_sensitive: rt.find.case_sensitive,
                regex: rt.find.regex,
                whole_word: rt.find.whole_word,
            },
        );
        rt.find.index = 0;
    }
    let count = rt.find.matches.len();
    ui.painter().text(
        Pos2::new(input_rect.right() - 60.0, input_rect.center().y),
        Align2::LEFT_CENTER,
        if count > 0 {
            format!("{} / {}", (rt.find.index % count) + 1, count)
        } else if rt.find.query.is_empty() {
            String::new()
        } else {
            "0".to_string()
        },
        FontId::monospace(11.5),
        p.text3,
    );
    let close = Rect::from_center_size(
        Pos2::new(panel.right() - 18.0, panel.top() + 20.0),
        Vec2::splat(18.0),
    );
    let close_response = ui.interact(close, ui.scope_id().with("find-close"), Sense::click());
    paint_icon(ui.painter(), close.shrink(3.0), Icon::X, p.text3);
    if close_response.clicked() {
        rt.find.open = false;
    }

    // Row 2: Replace input + [Replace] + [Replace All]
    let replace_rect = Rect::from_min_size(
        Pos2::new(panel.left() + 10.0, panel.top() + 38.0),
        Vec2::new(panel.width() - 176.0, 24.0),
    );
    ui.painter()
        .rect_filled(replace_rect, egui::CornerRadius::same(5), p.panel2);
    let field = Rect::from_min_max(
        Pos2::new(replace_rect.left() + 8.0, replace_rect.top() + 2.0),
        Pos2::new(replace_rect.right() - 8.0, replace_rect.bottom() - 2.0),
    );
    ui.put(
        field,
        TextEdit::singleline(&mut rt.find.replacement)
            .frame(egui::Frame::NONE)
            .align(egui::Align2::LEFT_CENTER)
            .hint_text(crate::i18n::t(lang, "Replace", "替换为"))
            .font(FontId::proportional(12.5)),
    );

    let replace_btn_rect = Rect::from_min_size(
        Pos2::new(replace_rect.right() + 6.0, panel.top() + 38.0),
        Vec2::new(72.0, 24.0),
    );
    if secondary_button(
        ui,
        replace_btn_rect,
        crate::i18n::t(lang, "Replace", "替换"),
        None,
        theme,
    )
    .clicked()
        && count > 0
    {
        if let Some((start, end)) = rt.find.matches.get(rt.find.index).copied() {
            doc.replace_match(start, end, &rt.find.replacement);
            rt.find.last_signature.clear();
        }
    }

    let replace_all_rect = Rect::from_min_size(
        Pos2::new(replace_btn_rect.right() + 6.0, panel.top() + 38.0),
        Vec2::new(82.0, 24.0),
    );
    if secondary_button(
        ui,
        replace_all_rect,
        crate::i18n::t(lang, "Replace all", "全部替换"),
        None,
        theme,
    )
    .clicked()
    {
        doc.replace_all(
            &rt.find.query,
            &rt.find.replacement,
            MatchOptions {
                case_sensitive: rt.find.case_sensitive,
                regex: rt.find.regex,
                whole_word: rt.find.whole_word,
            },
        );
        rt.find.last_signature.clear();
    }

    // Row 3: Toggles (Aa, ab, .*) + Prev (↑) / Next (↓)
    let mut x = input_rect.left();
    for (label, active, id) in [
        ("Aa", rt.find.case_sensitive, "case"),
        ("ab", rt.find.whole_word, "word"),
        (".*", rt.find.regex, "regex"),
    ] {
        let toggle = Rect::from_min_size(Pos2::new(x, panel.top() + 72.0), Vec2::new(28.0, 22.0));
        x += 32.0;
        let response = ui.interact(
            toggle,
            ui.scope_id().with(("find-toggle", id)),
            Sense::click(),
        );
        ui.painter().rect_filled(
            toggle,
            egui::CornerRadius::same(4),
            if active {
                p.accent_soft
            } else {
                Color32::TRANSPARENT
            },
        );
        ui.painter().text(
            toggle.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::monospace(11.5),
            if active { p.accent } else { p.text3 },
        );
        if response.clicked() {
            match id {
                "case" => rt.find.case_sensitive = !rt.find.case_sensitive,
                "word" => rt.find.whole_word = !rt.find.whole_word,
                _ => rt.find.regex = !rt.find.regex,
            }
            rt.find.last_signature.clear();
        }
    }
    let nav_rect = Rect::from_min_size(
        Pos2::new(panel.left() + 116.0, panel.top() + 72.0),
        Vec2::new(28.0, 22.0),
    );
    if secondary_button(ui, nav_rect, "↑", None, theme).clicked() && count > 0 {
        rt.find.index = (rt.find.index + count - 1) % count;
        jump_to_match(doc, rt);
    }
    let nav_rect2 = Rect::from_min_size(
        Pos2::new(nav_rect.right() + 4.0, panel.top() + 72.0),
        Vec2::new(28.0, 22.0),
    );
    if secondary_button(ui, nav_rect2, "↓", None, theme).clicked() && count > 0 {
        rt.find.index = (rt.find.index + 1) % count;
        jump_to_match(doc, rt);
    }
    let _ = (rect, tab_id);
}

pub(super) fn jump_to_match(doc: &mut Document, rt: &mut UiRuntime) {
    if let Some((start, end)) = rt.find.matches.get(rt.find.index).copied() {
        doc.selection = Some(Selection {
            anchor: start,
            head: end,
        });
        doc.cursor = end;
        if start.line < doc.scroll_line as usize || start.line > doc.scroll_line as usize + 30 {
            doc.scroll_line = (start.line.saturating_sub(6)) as f32;
        }
    }
}
