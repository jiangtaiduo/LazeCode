//! In-terminal search bar.

use super::prelude::*;

pub(super) fn draw_search(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    session: &Session,
    state: &mut TermSearchState,
) {
    let p = theme.palette;
    let panel = Rect::from_min_size(
        Pos2::new(rect.right() - 316.0, rect.top() + 6.0),
        Vec2::new(304.0, 34.0),
    );
    // 稳定版 `.terminal-find-bar`：悬浮右上，bg-header + 6px 圆角。
    ui.painter()
        .rect_filled(panel, egui::CornerRadius::same(6), p.titlebar);
    ui.painter().rect_stroke(
        panel,
        egui::CornerRadius::same(6),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(panel.left() + 15.0, panel.center().y),
            Vec2::splat(12.0),
        ),
        Icon::Search,
        p.text3,
    );
    let field = Rect::from_min_max(
        Pos2::new(panel.left() + 26.0, panel.top() + 4.0),
        Pos2::new(panel.right() - 96.0, panel.bottom() - 4.0),
    );
    let response = ui.put(
        field,
        TextEdit::singleline(&mut state.query)
            .frame(egui::Frame::NONE)
            .align(egui::Align2::LEFT_CENTER)
            .hint_text("Find in terminal")
            .font(FontId::proportional(11.0)),
    );
    if state.focus_requested {
        response.request_focus();
        state.focus_requested = false;
    }
    if response.changed() {
        state.query_changed();
    }
    if state.pump_search(session, ui.ctx()) {
        if let Some(hit) = state.matches.first() {
            session.scroll_to_line(hit.line);
        }
    }
    if ui.input(|input| input.key_pressed(Key::Escape)) {
        state.close_bar();
        return;
    }
    if ui.input(|input| input.key_pressed(Key::Enter)) && !state.matches.is_empty() {
        state.index = (state.index + 1) % state.matches.len();
        session.scroll_to_line(state.matches[state.index].line);
    }
    // ↑ ↓ 匹配导航（稳定版 find 按钮）。
    let nav = |ui: &mut Ui, rect: Rect, id: &'static str, icon: Icon| -> bool {
        let response = ui.interact(rect, ui.scope_id().with(id), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(4), p.hover);
        }
        paint_icon(ui.painter(), rect.shrink(4.0), icon, p.text2);
        response.clicked()
    };
    let prev_rect = Rect::from_center_size(
        Pos2::new(panel.right() - 62.0, panel.center().y),
        Vec2::splat(20.0),
    );
    let next_rect = Rect::from_center_size(
        Pos2::new(panel.right() - 40.0, panel.center().y),
        Vec2::splat(20.0),
    );
    let mut jump: i64 = 0;
    if nav(ui, prev_rect, "term-find-prev", Icon::ChevronUp) && !state.matches.is_empty() {
        jump = -1;
    }
    if nav(ui, next_rect, "term-find-next", Icon::ChevronDown) && !state.matches.is_empty() {
        jump = 1;
    }
    if jump != 0 {
        let total = state.matches.len() as i64;
        state.index = ((state.index as i64 + jump).rem_euclid(total)) as usize;
        session.scroll_to_line(state.matches[state.index].line);
    }
    ui.painter().text(
        Pos2::new(panel.right() - 90.0, panel.center().y),
        Align2::LEFT_CENTER,
        if state.matches.is_empty() {
            if state.query.is_empty() {
                String::new()
            } else {
                "none".to_string()
            }
        } else {
            format!("{} / {}", state.index + 1, state.matches.len())
        },
        FontId::monospace(9.0),
        p.text3,
    );
    let close = Rect::from_center_size(
        Pos2::new(panel.right() - 14.0, panel.center().y),
        Vec2::splat(18.0),
    );
    let close_response = ui.interact(
        close,
        ui.scope_id().with("term-search-close"),
        Sense::click(),
    );
    paint_icon(ui.painter(), close.shrink(3.0), Icon::X, p.text3);
    if close_response.clicked() {
        state.open = false;
        state.matches.clear();
    }
}
