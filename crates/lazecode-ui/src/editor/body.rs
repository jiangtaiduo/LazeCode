//! File-tab rendering: lines, gutter, cursor/selection painting, minimap.

use super::*;

pub(super) fn draw_file(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    index: usize,
    tab_id: u64,
) {
    let p = theme.palette;
    let (is_md, md_mode, annotate, blame) = {
        let Some(tab) = model.projects[index].tab_by_id(tab_id) else {
            return;
        };
        let TabKind::File(file) = &tab.kind else {
            return;
        };
        let is_md = file
            .doc
            .path
            .extension()
            .is_some_and(|ext| ext == "md" || ext == "markdown");
        (is_md, file.md_mode, file.annotate, file.blame.clone())
    };

    if is_md && md_mode == lazecode_core::MarkdownViewMode::Preview {
        let Some(tab) = model.projects[index].tab_by_id(tab_id) else {
            return;
        };
        let TabKind::File(file) = &tab.kind else {
            return;
        };
        let text = file.doc.rope.to_string();
        draw_markdown_view(ui, rt, p, theme, rect, &text, tab_id);
        return;
    }

    if is_md && md_mode == lazecode_core::MarkdownViewMode::Split {
        let half_w = ((rect.width() - 1.0) / 2.0).floor();
        let left_rect = Rect::from_min_size(rect.min, Vec2::new(half_w, rect.height()));
        let right_rect = Rect::from_min_size(
            Pos2::new(rect.min.x + half_w + 1.0, rect.min.y),
            Vec2::new(half_w, rect.height()),
        );
        ui.painter().vline(
            rect.min.x + half_w + 0.5,
            rect.y_range(),
            Stroke::new(1.0, p.border_soft),
        );

        draw_file_inner(
            ui, model, rt, theme, left_rect, index, tab_id, annotate, &blame,
        );

        let Some(tab) = model.projects[index].tab_by_id(tab_id) else {
            return;
        };
        let TabKind::File(file) = &tab.kind else {
            return;
        };
        let text = file.doc.rope.to_string();
        draw_markdown_view(ui, rt, p, theme, right_rect, &text, tab_id);
        return;
    }

    draw_file_inner(ui, model, rt, theme, rect, index, tab_id, annotate, &blame);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_file_inner(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    index: usize,
    tab_id: u64,
    annotate: bool,
    blame: &Option<Vec<lazecode_git::BlameLine>>,
) {
    let p = theme.palette;
    let settings = model.settings.editor.clone();
    let Some(path) = model
        .projects
        .get(index)
        .and_then(|project| project.tab_by_id(tab_id))
        .and_then(|tab| match &tab.kind {
            TabKind::File(file) => Some(file.doc.path.clone()),
            _ => None,
        })
    else {
        return;
    };
    if rt
        .lsp
        .completion
        .as_ref()
        .is_some_and(|state| state.tab_id != tab_id)
    {
        if let Some(state) = rt.lsp.completion.take() {
            if let Some(id) = state.request {
                model.lsp.cancel(id);
            }
        }
    }
    if rt
        .lsp
        .symbols
        .as_ref()
        .is_some_and(|state| state.tab_id != tab_id)
    {
        if let Some(state) = rt.lsp.symbols.take() {
            if let Some(id) = state.request {
                model.lsp.cancel(id);
            }
        }
    }
    let diagnostics = model.lsp.diagnostics(&path);
    let (font_size, tab_width, line_numbers, current_line, minimap) = (
        settings.font_size,
        settings.tab_width,
        settings.show_line_numbers,
        settings.highlight_current_line,
        model.ui.show_minimap,
    );
    let line_h = (font_size * 1.54).round().max(18.0);
    let char_w = mono_char_width(ui.ctx(), font_size);
    let font = FontId::monospace(font_size);

    let annotate_w = if annotate { 130.0 } else { 0.0 };
    let gutter_w = (if line_numbers { GUTTER } else { 10.0 }) + annotate_w;
    let minimap_w = if minimap { 38.0 } else { 0.0 };
    let content = Rect::from_min_max(
        Pos2::new(rect.left() + gutter_w, rect.top()),
        Pos2::new(rect.right() - SCROLLBAR_W - minimap_w, rect.bottom()),
    );
    let gutter = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top()),
        Pos2::new(rect.left() + gutter_w, rect.bottom()),
    );
    let minimap_rect = Rect::from_min_max(
        Pos2::new(rect.right() - SCROLLBAR_W - minimap_w, rect.top()),
        Pos2::new(rect.right() - SCROLLBAR_W, rect.bottom()),
    );
    ui.painter().rect_filled(gutter, 0.0, p.editor);
    ui.painter().vline(
        gutter.right() - 0.5,
        gutter.y_range(),
        Stroke::new(1.0, p.hairline),
    );
    if minimap {
        ui.painter()
            .rect_filled(minimap_rect, 0.0, p.editor.gamma_multiply(0.98));
        ui.painter().vline(
            minimap_rect.left() + 0.5,
            minimap_rect.y_range(),
            Stroke::new(1.0, p.border_soft),
        );
    }

    let focused = rt.editor_focus == Some(tab_id);
    if focused {
        // Keep the caret blink alive without repainting the whole app when
        // nothing else is happening.
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
    }
    // A modal surface owns the keyboard even without a text field; otherwise
    // typing would edit the focused buffer behind it.
    let overlays_open = rt.blocks_input(model);
    let mut pending: Vec<Command> = Vec::new();
    let mut hover_cursor: Option<Cursor> = None;
    let mut diag_tooltip: Option<(String, lazecode_core::lsp::DiagnosticSeverity)> = None;

    {
        let project = &mut model.projects[index];
        let Some(tab) = project.tab_mut_by_id(tab_id) else {
            return;
        };
        let TabKind::File(file_tab) = &mut tab.kind else {
            return;
        };
        let doc = &mut file_tab.doc;
        let lines = doc.line_count();
        let view_h = content.height();
        let visible_rows = (view_h / line_h).ceil() as usize + 2;
        let scroll_offset = doc.scroll_line * line_h;
        let max_scroll = ((lines as f32 * line_h) - view_h).max(0.0);
        doc.scroll_line = (scroll_offset / line_h).clamp(0.0, max_scroll / line_h);
        let first = doc.scroll_line.floor() as usize;
        let last = (first + visible_rows).min(lines.saturating_sub(1));
        doc.ensure_highlight(
            first.saturating_sub(20),
            (last + 20).min(lines.saturating_sub(1)),
        );

        let input_rect = Rect::from_min_max(
            Pos2::new(gutter.left(), rect.top()),
            Pos2::new(minimap_rect.left(), rect.bottom()),
        );
        let response = ui
            .interact(
                input_rect,
                ui.scope_id().with(("editor", tab_id)),
                Sense::click_and_drag(),
            )
            .on_hover_cursor(egui::CursorIcon::Text);
        if !overlays_open
            && ui
                .input(|i| i.pointer.hover_pos())
                .is_some_and(|pos| input_rect.contains(pos))
        {
            // `Response::hovered` can be shadowed by a transparent interaction
            // layer; containment decides the I-beam cursor.
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        }
        if response.clicked() || response.dragged() {
            rt.set_editor_focus(tab_id);
        }
        let pointer = response.interact_pointer_pos();
        let command_click = response.clicked() && ui.input(|input| input.modifiers.command);
        if let Some(pos) = pointer {
            if response.clicked() || response.dragged() {
                let cursor = cursor_from_pos(pos, content, doc, char_w, line_h, tab_width);
                let extend = response.dragged() || ui.input(|input| input.modifiers.shift);
                doc.set_cursor(cursor, extend);
                if command_click {
                    rt.lsp.goto_request = Some((tab_id, cursor));
                }
                if response.dragged() {
                    rt.editor_drag = true;
                }
            }
        }
        if response.double_clicked() {
            if let Some(pos) = pointer {
                let cursor = cursor_from_pos(pos, content, doc, char_w, line_h, tab_width);
                select_word(doc, cursor);
            }
        }
        if response.secondary_clicked() {
            let path_str = doc.path.to_string_lossy().to_string();
            let anchor = ui
                .input(|i| i.pointer.hover_pos())
                .map(|p| Rect::from_min_size(p, Vec2::splat(1.0)))
                .unwrap_or(input_rect);
            rt.open_menu(
                crate::runtime::MenuKind::EditorContext { project: index },
                anchor,
                Some(path_str),
            );
        }
        // Wheel scrolling.
        if response.hovered() {
            let (dy, dx, shift) = ui.input(|input| {
                (
                    input.smooth_scroll_delta.y,
                    input.smooth_scroll_delta.x,
                    input.modifiers.shift,
                )
            });
            if dy.abs() > 0.0 || dx.abs() > 0.0 {
                if shift {
                    doc.scroll_x = (doc.scroll_x - dy).max(0.0);
                } else {
                    doc.scroll_line =
                        (doc.scroll_line - dy / line_h).clamp(0.0, (max_scroll / line_h).max(0.0));
                    doc.scroll_x = (doc.scroll_x - dx).max(0.0);
                }
            }
        }

        // `text_edit_focused` (not `egui_wants_keyboard_input`): egui's Tab
        // navigation moving focus onto some button must not mute the editor.
        if focused && !overlays_open && !ui.ctx().text_edit_focused() {
            handle_editor_input(ui, doc, &mut pending, tab_id, rt);
        }

        let scroll_rect =
            Rect::from_min_max(Pos2::new(rect.right() - SCROLLBAR_W, rect.top()), rect.max);
        let mut offset = doc.scroll_line * line_h;
        if crate::widgets::scrollbar(
            ui,
            scroll_rect,
            lines as f32 * line_h,
            view_h,
            &mut offset,
            theme,
        ) {
            doc.scroll_line = (offset / line_h).clamp(0.0, (max_scroll / line_h).max(0.0));
        }

        // Text is clipped to the content column; the gutter needs its own clip
        // because it lives to the left of `content`.
        let painter = ui.painter().with_clip_rect(content);
        let gutter_painter = ui.painter().with_clip_rect(gutter);
        let sel = doc.selection_char_range();
        let cursor = doc.cursor;
        let scroll_line = doc.scroll_line;
        let scroll_x = doc.scroll_x;
        let tab_w = tab_width.max(1);
        let mut line_chars: Vec<char> = Vec::new();
        let mut line_kinds: Vec<TokenKind> = Vec::new();
        for line_index in first..=last {
            let y = content.top() + (line_index as f32 - scroll_line) * line_h;
            if line_index == first && scroll_line.fract() > 0.0 {
                // still draw; clipping handles the partial row
            }
            if current_line && line_index == cursor.line && focused {
                painter.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(gutter.left(), y),
                        Pos2::new(rect.right() - SCROLLBAR_W, y + line_h),
                    ),
                    0.0,
                    p.current_line,
                );
            }
            if let Some((start, end)) = sel {
                let line_start = doc.rope.line_to_char(line_index);
                let line_len = doc.line_content_len(line_index);
                let a = start.max(line_start);
                let b = end.min(line_start + line_len);
                if a < b {
                    let line_text = doc.line_content(line_index);
                    let v0 = Document::visual_col(&line_text, a - line_start, tab_w);
                    let v1 = Document::visual_col(&line_text, b - line_start, tab_w);
                    painter.rect_filled(
                        Rect::from_min_size(
                            Pos2::new(content.left() + v0 as f32 * char_w - scroll_x, y),
                            Vec2::new(((v1 - v0) as f32 * char_w).max(2.0), line_h),
                        ),
                        0.0,
                        p.selection,
                    );
                }
            }
            if annotate {
                if let Some(blame_lines) = blame {
                    if let Some(bl) = blame_lines.get(line_index) {
                        gutter_painter.text(
                            Pos2::new(gutter.left() + 6.0, y + line_h * 0.5),
                            Align2::LEFT_CENTER,
                            format!("{} {}", bl.short, bl.author),
                            FontId::monospace(11.0),
                            p.accent,
                        );
                    }
                }
            }
            if line_numbers {
                gutter_painter.text(
                    Pos2::new(gutter.right() - 10.0, y + line_h * 0.5),
                    Align2::RIGHT_CENTER,
                    (line_index + 1).to_string(),
                    FontId::monospace((font_size - 1.5).max(11.5)),
                    if line_index == cursor.line && focused {
                        p.text3
                    } else {
                        p.text3.gamma_multiply(0.72)
                    },
                );
            }
            let text = doc.line_content(line_index);
            let spans = doc.highlighter.spans(line_index);
            draw_line_runs(
                &painter,
                &text,
                spans,
                content.left() - scroll_x,
                y,
                line_h,
                char_w,
                tab_w,
                &font,
                theme,
                &mut line_chars,
                &mut line_kinds,
            );
            // LSP diagnostics: one underline per diagnostic per visible line.
            // Multi-line ranges clip to the current row.
            if !diagnostics.is_empty() {
                let text_len = text.chars().count();
                let line_u32 = line_index as u32;
                for diagnostic in diagnostics {
                    let start = diagnostic.range.start;
                    let end = diagnostic.range.end;
                    if line_u32 < start.line || line_u32 > end.line {
                        continue;
                    }
                    let start_col = if line_u32 == start.line {
                        lazecode_text::lsp::char_col_for_utf16(&text, start.character)
                    } else {
                        0
                    }
                    .min(text_len);
                    let mut end_col = if line_u32 == end.line {
                        lazecode_text::lsp::char_col_for_utf16(&text, end.character)
                    } else {
                        text_len
                    }
                    .min(text_len);
                    if end_col <= start_col {
                        if start.line != end.line {
                            continue;
                        }
                        end_col = (start_col + 1).min(text_len);
                    }
                    let v0 = Document::visual_col(&text, start_col, tab_w);
                    let v1 = Document::visual_col(&text, end_col, tab_w);
                    let x0 = (content.left() + v0 as f32 * char_w - scroll_x).max(content.left());
                    let x1 = (content.left() + v1 as f32 * char_w - scroll_x)
                        .max(x0 + 2.0)
                        .min(content.right());
                    if x1 <= content.left() || x0 >= content.right() {
                        continue;
                    }
                    let y_line = y + line_h - 2.0;
                    painter.line_segment(
                        [Pos2::new(x0, y_line), Pos2::new(x1, y_line)],
                        Stroke::new(
                            1.5,
                            crate::lsp_ui::diagnostic_color(theme, diagnostic.severity),
                        ),
                    );
                }
            }
            let leading_spaces = text.chars().take_while(|c| *c == ' ').count();
            let leading_tabs = text.chars().take_while(|c| *c == '\t').count();
            let total_indent_cols = leading_tabs * tab_w + leading_spaces;
            if total_indent_cols >= tab_w {
                for col in (tab_w..total_indent_cols).step_by(tab_w) {
                    let gx = content.left() + col as f32 * char_w - scroll_x;
                    if gx >= content.left() && gx <= content.right() {
                        painter.vline(
                            gx,
                            egui::Rangef::new(y, y + line_h),
                            Stroke::new(1.0, p.hairline),
                        );
                    }
                }
            }
        }

        // Cursor + preedit + IME activation.
        if focused && !overlays_open {
            let line = cursor.line.min(lines.saturating_sub(1));
            let line_text = doc.line_content(line);
            let vcol = Document::visual_col(&line_text, cursor.col, tab_w);
            let x = content.left() + vcol as f32 * char_w - doc.scroll_x;
            let y = content.top() + (line as f32 - doc.scroll_line) * line_h;
            let cursor_rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(char_w, line_h));
            if let Some(state) = rt.lsp.completion.as_mut() {
                if state.tab_id == tab_id {
                    state.anchor = Pos2::new(x, y);
                }
            }
            ui.ctx().output_mut(|o| {
                o.ime = Some(egui::output::IMEOutput {
                    purpose: egui::IMEPurpose::Normal,
                    rect: content,
                    cursor_rect,
                    should_interrupt_composition: false,
                });
            });
            if y + line_h > content.top() && y < content.bottom() && x >= content.left() - 1.0 {
                let time = ui.input(|input| input.time);
                let blink = ((time * 1.4) as i64 % 2) == 0;
                if blink || !rt.ime_preedit.is_empty() {
                    ui.painter().rect_filled(
                        Rect::from_min_size(Pos2::new(x, y + 1.0), Vec2::new(1.8, line_h - 2.0)),
                        0.0,
                        p.accent,
                    );
                }
                if !rt.ime_preedit.is_empty() {
                    let preedit_rect = Rect::from_min_size(
                        Pos2::new(x, y),
                        Vec2::new(char_w * rt.ime_preedit.chars().count() as f32, line_h),
                    );
                    ui.painter().text(
                        Pos2::new(x, y + line_h * 0.5),
                        Align2::LEFT_CENTER,
                        &rt.ime_preedit,
                        font.clone(),
                        p.text,
                    );
                    ui.painter().line_segment(
                        [
                            Pos2::new(preedit_rect.left(), preedit_rect.bottom() - 1.0),
                            Pos2::new(preedit_rect.right(), preedit_rect.bottom() - 1.0),
                        ],
                        Stroke::new(1.0, p.accent),
                    );
                }
            }
        }

        if minimap {
            draw_minimap(ui, doc, theme, minimap_rect, view_h, line_h);
        }
        if !overlays_open {
            hover_cursor = ui
                .input(|input| input.pointer.hover_pos())
                .filter(|pos| content.contains(*pos) && !ui.input(|input| input.pointer.any_down()))
                .map(|pos| cursor_from_pos(pos, content, doc, char_w, line_h, tab_width));
        }
        if let Some(cursor) = hover_cursor {
            diag_tooltip = crate::lsp_ui::diagnostic_at(doc, diagnostics, cursor)
                .map(|diagnostic| (diagnostic.message.clone(), diagnostic.severity));
        }
        let _ = MONO;
    }

    for command in pending {
        model.queue(command);
    }
    crate::lsp_ui::update_hover(ui.ctx(), model, rt, tab_id, hover_cursor);
    if rt
        .lsp
        .hover
        .as_ref()
        .and_then(|state| state.result.as_ref())
        .is_none()
    {
        if let Some((message, severity)) = diag_tooltip {
            let color = crate::lsp_ui::diagnostic_color(theme, severity);
            egui::Tooltip::always_open(
                ui.ctx().clone(),
                ui.layer_id(),
                egui::Id::unique(("lsp-diagnostic", tab_id)),
                egui::PopupAnchor::Pointer,
            )
            .show(|ui| {
                ui.set_max_width(520.0);
                ui.label(egui::RichText::new(message).size(12.0).color(color));
            });
        }
    }
    let _ = (p, rect, tab_id);
    // Find overlay sits above the text.
    if rt.find.open {
        draw_find(
            ui,
            theme,
            rect,
            &mut model.projects[index],
            tab_id,
            rt,
            model.settings.language,
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_line_runs(
    painter: &egui::Painter,
    text: &str,
    spans: &[lazecode_text::HighlightSpan],
    origin_x: f32,
    y: f32,
    line_h: f32,
    char_w: f32,
    tab_width: usize,
    font: &FontId,
    theme: &Theme,
    chars: &mut Vec<char>,
    kinds: &mut Vec<TokenKind>,
) {
    if text.is_empty() {
        return;
    }
    chars.clear();
    chars.extend(text.chars());
    kinds.clear();
    kinds.resize(chars.len(), TokenKind::Text);
    for span in spans {
        for kind in kinds
            .iter_mut()
            .take(span.end.min(chars.len()))
            .skip(span.start)
        {
            *kind = span.kind;
        }
    }
    let mut visual = 0usize;
    let mut index = 0usize;
    while index < chars.len() {
        let kind = kinds[index];
        let start_visual = visual;
        let mut run = String::new();
        while index < chars.len() && kinds[index] == kind {
            let ch = chars[index];
            if ch == '\t' {
                let width = tab_width - (visual % tab_width);
                for _ in 0..width {
                    run.push(' ');
                    visual += 1;
                }
            } else {
                let width = unicode_width::UnicodeWidthChar::width(ch)
                    .unwrap_or(1)
                    .max(1);
                run.push(ch);
                visual += width;
            }
            index += 1;
        }
        if !run.is_empty() {
            painter.text(
                Pos2::new(origin_x + start_visual as f32 * char_w, y + line_h * 0.5),
                Align2::LEFT_CENTER,
                run,
                font.clone(),
                token_color(kind, theme),
            );
        }
    }
}

pub(super) fn cursor_from_pos(
    pos: Pos2,
    content: Rect,
    doc: &Document,
    char_w: f32,
    line_h: f32,
    tab_width: usize,
) -> Cursor {
    let line = ((pos.y - content.top()) / line_h + doc.scroll_line).floor();
    let line = (line.max(0.0) as usize).min(doc.line_count().saturating_sub(1));
    let visual_col = (((pos.x - content.left() + doc.scroll_x) / char_w).round()).max(0.0) as usize;
    let text = doc.line_content(line);
    let mut visual = 0usize;
    let mut col = 0usize;
    for ch in text.chars() {
        if visual >= visual_col {
            break;
        }
        if ch == '\t' {
            let width = tab_width.max(1) - (visual % tab_width.max(1));
            visual += width;
        } else {
            visual += unicode_width::UnicodeWidthChar::width(ch)
                .unwrap_or(1)
                .max(1);
        }
        col += 1;
    }
    Cursor { line, col }
}

pub(super) fn select_word(doc: &mut Document, cursor: Cursor) {
    let text = doc.line_content(cursor.line);
    let chars: Vec<char> = text.chars().collect();
    let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
    let mut start = cursor.col.min(chars.len());
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let mut end = cursor.col.min(chars.len());
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    if start == end && end < chars.len() {
        end += 1;
    }
    doc.selection = Some(Selection {
        anchor: Cursor {
            line: cursor.line,
            col: start,
        },
        head: Cursor {
            line: cursor.line,
            col: end,
        },
    });
    doc.cursor = Cursor {
        line: cursor.line,
        col: end,
    };
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_minimap(
    ui: &mut Ui,
    doc: &mut Document,
    theme: &Theme,
    rect: Rect,
    view_h: f32,
    line_h: f32,
) {
    let p = theme.palette;
    let total = doc.line_count().max(1);
    let visible_lines = (view_h / line_h).max(1.0);
    let painter = ui.painter().with_clip_rect(rect);

    // 对齐 Monaco Editor Minimap 核心机制：
    // 每行固定行高 2.0px（Monaco scale: 1 默认值），缩进与代码宽度等比投影。
    const MINIMAP_LINE_H: f32 = 2.0;
    let minimap_content_h = total as f32 * MINIMAP_LINE_H;
    let max_editor_scroll = ((total as f32 * line_h) - view_h).max(0.0) / line_h;
    let scroll_ratio = if max_editor_scroll > 0.0 {
        (doc.scroll_line / max_editor_scroll).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let (minimap_scroll_y, slider_top, slider_h) = if minimap_content_h <= rect.height() {
        // 短文件：Minimap 不滚动，代码从顶部画起；滑块高度与当前可见行严格同比例覆盖
        let s_top = rect.top() + doc.scroll_line * MINIMAP_LINE_H;
        let s_h = (visible_lines * MINIMAP_LINE_H).clamp(18.0, minimap_content_h.max(18.0));
        let s_top = s_top.clamp(
            rect.top(),
            (rect.top() + minimap_content_h - s_h).max(rect.top()),
        );
        (0.0, s_top, s_h)
    } else {
        // 长文件：Minimap 随编辑器滚动同步平滑滚动（Monaco 滚动映射）
        let max_minimap_scroll = minimap_content_h - rect.height();
        let m_scroll = scroll_ratio * max_minimap_scroll;
        let s_h = (visible_lines * MINIMAP_LINE_H).clamp(20.0, rect.height());
        let s_top = rect.top() + doc.scroll_line * MINIMAP_LINE_H - m_scroll;
        let s_top = s_top.clamp(rect.top(), (rect.bottom() - s_h).max(rect.top()));
        (m_scroll, s_top, s_h)
    };

    // 绘制代码微缩行（带缩进投影，与编辑器代码轮廓一比一对应）
    for line_index in 0..total {
        let y = rect.top() + line_index as f32 * MINIMAP_LINE_H - minimap_scroll_y;
        if y + MINIMAP_LINE_H < rect.top() {
            continue;
        }
        if y > rect.bottom() {
            break;
        }
        let text = doc.line_content(line_index);
        let trimmed = text.trim_start();
        let indent = text.len() - trimmed.len();
        let indent_x = (indent as f32 * 0.8).min(rect.width() * 0.35);
        let width = (trimmed.trim_end().chars().count() as f32 * 0.65)
            .clamp(0.0, rect.width() - 8.0 - indent_x);
        if width > 1.0 {
            let color = if trimmed.starts_with("//") || trimmed.starts_with('#') {
                p.tok_com
            } else {
                p.text3
            };
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(rect.left() + 4.0 + indent_x, y),
                    Vec2::new(width, 1.5),
                ),
                0.0,
                color.gamma_multiply(0.32),
            );
        }
    }

    let slider_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 1.0, slider_top),
        Vec2::new(rect.width() - 2.0, slider_h),
    );

    // 交互：点击或拖拽 minimap 滚动编辑器
    let response = ui.interact(rect, ui.scope_id().with("minimap"), Sense::click_and_drag());
    if (response.clicked() || response.dragged()) && max_editor_scroll > 0.0 {
        if let Some(pos) = response.interact_pointer_pos() {
            if minimap_content_h <= rect.height() {
                let clicked_line = ((pos.y - rect.top()) / MINIMAP_LINE_H).max(0.0);
                let target_line = clicked_line - visible_lines * 0.5;
                doc.scroll_line = target_line.clamp(0.0, max_editor_scroll);
            } else {
                let rel_y = ((pos.y - rect.top() - slider_h * 0.5)
                    / (rect.height() - slider_h).max(1.0))
                .clamp(0.0, 1.0);
                doc.scroll_line = (rel_y * max_editor_scroll).clamp(0.0, max_editor_scroll);
            }
        }
    }

    let slider_bg = if response.dragged() {
        Color32::from_rgba_unmultiplied(255, 255, 255, 36)
    } else if response.hovered() {
        Color32::from_rgba_unmultiplied(255, 255, 255, 26)
    } else {
        Color32::from_rgba_unmultiplied(255, 255, 255, 16)
    };
    ui.painter()
        .rect_filled(slider_rect, egui::CornerRadius::same(3), slider_bg);
    ui.painter().rect_stroke(
        slider_rect,
        egui::CornerRadius::same(3),
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 45)),
        StrokeKind::Inside,
    );
}
