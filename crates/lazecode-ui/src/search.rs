use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2};
use lazecode_core::{AppModel, Command, TabKind};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::{SearchState, UiRuntime};
use crate::theme::Theme;
use crate::widgets::secondary_button;

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme) {
    let p = theme.palette;
    let now = ui.input(|input| input.time);
    let enter_pressed = ui.input(|input| input.key_pressed(egui::Key::Enter));
    let screen = ui.max_rect();
    ui.painter()
        .rect_filled(screen, 0.0, Color32::from_black_alpha(120));
    let panel = Rect::from_center_size(
        screen.center(),
        Vec2::new(
            screen.width().min(760.0) - 80.0,
            screen.height().min(600.0) - 100.0,
        ),
    );
    ui.painter()
        .rect_filled(panel, egui::CornerRadius::same(10), p.elev);
    ui.painter().rect_stroke(
        panel,
        egui::CornerRadius::same(10),
        Stroke::new(1.0, p.border_strong),
        StrokeKind::Inside,
    );
    let state = &mut rt.search;
    let replace_open = state.replace_mode;
    let bar_height = if replace_open { 74.0 } else { 36.0 };
    let input = Rect::from_min_size(
        Pos2::new(panel.left() + 16.0, panel.top() + 14.0),
        Vec2::new(panel.width() - 32.0, bar_height),
    );
    ui.painter()
        .rect_filled(input, egui::CornerRadius::same(6), p.panel2);
    ui.painter().rect_stroke(
        input,
        egui::CornerRadius::same(6),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );

    // Row 1: Search icon + query field + flags (Aa, ab, .*) + replace toggle + close
    let row1 = Rect::from_min_size(input.min, Vec2::new(input.width(), 34.0));
    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(row1.left() + 16.0, row1.center().y),
            Vec2::splat(14.0),
        ),
        Icon::Search,
        p.text3,
    );
    let field = Rect::from_min_max(
        Pos2::new(row1.left() + 32.0, row1.top() + 4.0),
        Pos2::new(row1.right() - 210.0, row1.bottom() - 4.0),
    );
    let response = ui.put(
        field,
        TextEdit::singleline(&mut state.query)
            .frame(egui::Frame::NONE)
            .align(egui::Align2::LEFT_CENTER)
            .hint_text(crate::tr!(
                model,
                "Search file contents across the project",
                "在项目中搜索文件内容"
            ))
            .font(FontId::proportional(13.0)),
    );
    if state.focus_requested {
        response.request_focus();
        state.focus_requested = false;
    }

    // Flags: Aa, ab, .*
    let mut flag_x = row1.right() - 200.0;
    for (label, active, id) in [
        ("Aa", state.case_sensitive, "case"),
        ("ab", state.whole_word, "word"),
        (".*", state.regex, "regex"),
    ] {
        let toggle = Rect::from_min_size(
            Pos2::new(flag_x, row1.center().y - 11.0),
            Vec2::new(26.0, 22.0),
        );
        flag_x += 28.0;
        let t_resp = ui.interact(
            toggle,
            ui.scope_id().with(("search-flag", id)),
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
            FontId::monospace(9.5),
            if active { p.accent } else { p.text3 },
        );
        if t_resp.clicked() {
            match id {
                "case" => state.case_sensitive = !state.case_sensitive,
                "word" => state.whole_word = !state.whole_word,
                _ => state.regex = !state.regex,
            }
        }
    }

    // Replace mode toggle button
    let rep_btn_rect = Rect::from_min_size(
        Pos2::new(flag_x + 6.0, row1.center().y - 11.0),
        Vec2::new(76.0, 22.0),
    );
    let rep_btn_label = if state.replace_mode {
        crate::tr!(model, "▲ Hide", "▲ 隐藏替换")
    } else {
        crate::tr!(model, "⇄ Replace", "⇄ 替换")
    };
    if secondary_button(ui, rep_btn_rect, rep_btn_label, None, theme).clicked() {
        state.replace_mode = !state.replace_mode;
    }

    // Close button
    let close = Rect::from_center_size(
        Pos2::new(row1.right() - 14.0, row1.center().y),
        Vec2::splat(18.0),
    );
    let close_response = ui.interact(close, ui.scope_id().with("search-close"), Sense::click());
    paint_icon(ui.painter(), close.shrink(3.0), Icon::X, p.text3);
    if close_response.clicked() {
        state.open = false;
    }

    // Row 2 (if replace_mode): Replace input + File mask + [Replace all] button
    if replace_open {
        let row2 = Rect::from_min_size(
            Pos2::new(input.left(), input.top() + 38.0),
            Vec2::new(input.width(), 32.0),
        );
        ui.painter()
            .hline(input.x_range(), row2.top(), Stroke::new(1.0, p.border_soft));

        let rep_field_rect = Rect::from_min_max(
            Pos2::new(row2.left() + 14.0, row2.top() + 4.0),
            Pos2::new(row2.left() + 240.0, row2.bottom() - 4.0),
        );
        ui.put(
            rep_field_rect,
            TextEdit::singleline(&mut state.replacement)
                .frame(egui::Frame::NONE)
                .align(egui::Align2::LEFT_CENTER)
                .hint_text(crate::tr!(model, "Replace with…", "替换为…"))
                .font(FontId::proportional(12.5)),
        );

        let mask_field_rect = Rect::from_min_max(
            Pos2::new(row2.left() + 250.0, row2.top() + 4.0),
            Pos2::new(row2.right() - 110.0, row2.bottom() - 4.0),
        );
        let mask_resp = ui.put(
            mask_field_rect,
            TextEdit::singleline(&mut state.file_mask)
                .frame(egui::Frame::NONE)
                .align(egui::Align2::LEFT_CENTER)
                .hint_text(crate::tr!(
                    model,
                    "File mask (e.g. *.rs, *.ts)",
                    "文件掩码 (如 *.rs)"
                ))
                .font(FontId::monospace(10.5)),
        );
        let _ = mask_resp;

        let rep_all_rect = Rect::from_min_size(
            Pos2::new(row2.right() - 98.0, row2.center().y - 11.0),
            Vec2::new(90.0, 22.0),
        );
        if secondary_button(
            ui,
            rep_all_rect,
            crate::tr!(model, "Replace all", "全部替换"),
            None,
            theme,
        )
        .clicked()
        {
            let options = lazecode_fs::SearchOptions {
                case_sensitive: state.case_sensitive,
                whole_word: state.whole_word,
                regex: state.regex,
                include: (!state.file_mask.trim().is_empty())
                    .then(|| state.file_mask.trim().to_string()),
                max_results: 5000,
                ..Default::default()
            };
            model.queue(Command::ReplaceInFiles {
                query: state.query.clone(),
                replacement: state.replacement.clone(),
                options,
            });
        }
    }

    // Find in Files searches while you type (160ms debounce); invalidate old
    // results as soon as the query or a flag changes.
    let project_root = model
        .active_project()
        .map(|project| project.active_root().display().to_string())
        .unwrap_or_default();
    let signature = format!(
        "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
        project_root,
        state.query,
        state.case_sensitive,
        state.whole_word,
        state.regex,
        state.file_mask
    );
    if signature != state.signature {
        state.signature = signature;
        state.request_id = state.request_id.wrapping_add(1);
        state.results.clear();
        state.selected = 0;
        state.searched = false;
        state.searching = !state.query.trim().is_empty();
        state.dirty_since = Some(now);
    }
    if state.query.trim().is_empty() {
        state.results.clear();
        state.searched = false;
        state.searching = false;
        state.dirty_since = None;
    } else if enter_pressed
        || state
            .dirty_since
            .is_some_and(|since| (now - since).max(0.0) >= 0.16)
    {
        run(model, state);
    } else if let Some(since) = state.dirty_since {
        let wait_ms = ((0.16 - (now - since).max(0.0)) * 1000.0).max(16.0);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(wait_ms as u64));
    }
    let result_area = Rect::from_min_max(
        Pos2::new(panel.left() + 10.0, input.bottom() + 8.0),
        Pos2::new(panel.right() - 10.0, panel.bottom() - 10.0),
    );
    if state.results.is_empty() {
        let prompt_pos = Pos2::new(panel.center().x, panel.top() + 84.0);
        let (message, position) = if state.searching {
            (crate::tr!(model, "Searching…", "正在搜索…"), prompt_pos)
        } else if state.searched {
            (crate::tr!(model, "No matches", "没有匹配"), panel.center())
        } else {
            (
                crate::tr!(
                    model,
                    "Type to search across the project",
                    "输入内容以搜索项目"
                ),
                prompt_pos,
            )
        };
        ui.painter().text(
            position,
            Align2::CENTER_CENTER,
            message,
            FontId::proportional(12.5),
            p.text3,
        );
    } else {
        ui.painter().text(
            Pos2::new(panel.left() + 18.0, input.bottom() + 2.0),
            Align2::LEFT_CENTER,
            if model.settings.language.is_zh() {
                format!("{} 个匹配", state.results.len())
            } else {
                format!("{} matches", state.results.len())
            },
            FontId::proportional(11.5),
            p.text3,
        );
    }
    let mut activate: Option<usize> = None;
    let results = state.results.clone();

    // Keyboard: ↑/↓ step the matches while the query field owns the keyboard
    // (its caret keys are single-line left/right, so vertical arrows are
    // free); the selected row scrolls into view.
    let mut nav_scroll: Option<f32> = None;
    if !results.is_empty() {
        if let Some(mv) = crate::list_nav::panel_move(ui) {
            let current = Some(state.selected.min(results.len() - 1));
            if let Some(next) = crate::list_nav::stepped(current, results.len(), mv) {
                state.selected = next;
                nav_scroll =
                    Some((next as f32 * 34.0 - result_area.height() * 0.5 + 17.0).max(0.0));
            }
        }
    }

    super::app_ui::region(ui, result_area, "search-results", |ui| {
        let mut area = egui::ScrollArea::vertical()
            .id_salt("search-results-scroll")
            .auto_shrink([false, false]);
        if let Some(offset) = nav_scroll {
            area = area.vertical_scroll_offset(offset);
        }
        area.show_rows(ui, 34.0, results.len(), |ui, range| {
            for index in range {
                let hit = &results[index];
                let (row, response) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::click());
                if index == state.selected || response.hovered() {
                    ui.painter().rect_filled(
                        row,
                        egui::CornerRadius::same(4),
                        if index == state.selected {
                            p.active
                        } else {
                            p.hover
                        },
                    );
                }
                let (icon, tone) = file_icon(hit.path.to_str().unwrap_or_default(), theme);
                paint_icon(
                    ui.painter(),
                    Rect::from_center_size(
                        Pos2::new(row.left() + 14.0, row.top() + 12.0),
                        Vec2::splat(12.0),
                    ),
                    icon,
                    tone,
                );
                let location = format!("{}:{}", hit.path.display(), hit.line);
                ui.painter().text(
                    Pos2::new(row.left() + 26.0, row.top() + 11.0),
                    Align2::LEFT_CENTER,
                    &location,
                    FontId::proportional(11.5),
                    p.text3,
                );
                ui.painter().text(
                    Pos2::new(row.left() + 26.0, row.top() + 24.0),
                    Align2::LEFT_CENTER,
                    &hit.line_text,
                    FontId::monospace(12.0),
                    p.text2,
                );
                if response.clicked() {
                    activate = Some(index);
                }
            }
        });
    });
    if let Some(index) = activate {
        if let Some(hit) = results.get(index) {
            if let Some(active) = model.active_project {
                let path = model.projects[active].active_root().join(&hit.path);
                if let Ok(id) = model.open_file_in(active, path) {
                    let _ = id;
                    let project = &mut model.projects[active];
                    if let Some(tab_id) = project.active_tab {
                        if let Some(tab) = project.tab_mut_by_id(tab_id) {
                            if let TabKind::File(file) = &mut tab.kind {
                                file.doc.set_cursor(
                                    lazecode_text::Cursor {
                                        line: hit.line.saturating_sub(1),
                                        col: 0,
                                    },
                                    false,
                                );
                                file.doc.scroll_line = hit.line.saturating_sub(6) as f32;
                            }
                        }
                    }
                }
            }
            rt.search.open = false;
        }
    }
    if ui.input(|input| input.key_pressed(egui::Key::Escape))
        || (rt.search.opened_frame != rt.frame_count
            && ui.input(|input| {
                input.pointer.any_click()
                    && input
                        .pointer
                        .interact_pos()
                        .is_some_and(|pos| !panel.contains(pos))
            }))
    {
        rt.search.open = false;
    }
}

fn run(model: &mut AppModel, state: &mut SearchState) {
    if model.active_project().is_none() {
        return;
    }
    state.request_id = state.request_id.wrapping_add(1);
    state.results.clear();
    state.selected = 0;
    state.searched = false;
    state.searching = true;
    state.dirty_since = None;
    let options = lazecode_fs::SearchOptions {
        case_sensitive: state.case_sensitive,
        whole_word: state.whole_word,
        regex: state.regex,
        include: (!state.file_mask.trim().is_empty()).then(|| state.file_mask.trim().to_string()),
        max_results: 500,
        ..Default::default()
    };
    model.queue(Command::Search {
        query: state.query.clone(),
        request_id: state.request_id,
        options,
    });
}

fn file_icon(path: &str, theme: &Theme) -> (Icon, Color32) {
    let p = theme.palette;
    let color = match path.rsplit('.').next().unwrap_or_default() {
        "rs" => p.orange,
        "ts" | "tsx" => p.blue,
        "js" | "jsx" => p.amber,
        "json" => p.amber,
        "md" => p.text2,
        _ => p.text3,
    };
    (Icon::File, color)
}
