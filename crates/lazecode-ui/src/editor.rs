use std::path::PathBuf;

use egui::{
    Align2, Color32, Event, FontId, ImeEvent, Key, Modifiers, Pos2, Rect, Sense, Stroke,
    StrokeKind, TextEdit, Ui, Vec2,
};
use lazecode_core::{AppModel, Command, ProjectState, TabKind, ToastKind};
use lazecode_text::{Cursor, Document, MatchOptions, Motion, Selection, TokenKind};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::{DialogState, UiRuntime};
use crate::theme::{Theme, MONO};
use crate::widgets::{mono_char_width, primary_button, secondary_button, text_clipped};

const TABBAR_H: f32 = 35.0;
const CRUMBS_H: f32 = 26.0;
const GUTTER: f32 = 52.0;
const SCROLLBAR_W: f32 = 9.0;

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.editor);
    let Some(index) = model.active_project else {
        draw_welcome(ui, model, rt, theme, rect, None);
        return;
    };
    let tab_count = model.projects[index].tabs.len();
    if tab_count == 0 {
        draw_welcome(ui, model, rt, theme, rect, Some(index));
        return;
    }

    let tabbar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), TABBAR_H));
    draw_tabbar(ui, model, rt, theme, tabbar, index);

    let active = model.projects[index].active_tab;
    let Some(active) = active else {
        draw_welcome(ui, model, rt, theme, rect, Some(index));
        return;
    };
    let is_file = model.projects[index]
        .tab_by_id(active)
        .map(|tab| matches!(tab.kind, TabKind::File(_)))
        .unwrap_or(false);
    let content = Rect::from_min_max(Pos2::new(rect.left(), tabbar.bottom()), rect.max);
    if is_file {
        let crumbs = Rect::from_min_size(content.min, Vec2::new(content.width(), CRUMBS_H));
        draw_crumbs(ui, model, theme, crumbs, index, active);
        let body = Rect::from_min_max(Pos2::new(content.left(), crumbs.bottom()), content.max);
        draw_file(ui, model, rt, theme, body, index, active);
    } else {
        draw_diff(ui, model, rt, theme, content, index, active);
    }
}

fn tab_icon(path: &str, theme: &Theme) -> (Icon, Color32) {
    let p = theme.palette;
    let color = match path.rsplit('.').next().unwrap_or_default() {
        "rs" => p.orange,
        "ts" | "tsx" => p.blue,
        "js" | "jsx" => p.amber,
        "json" => p.amber,
        "css" | "scss" => p.violet,
        "html" => p.orange,
        "md" => p.text2,
        "py" => p.blue,
        "go" => p.green,
        "sh" | "zsh" => p.green,
        _ => p.text3,
    };
    (Icon::File, color)
}

fn draw_tabbar(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    index: usize,
) {
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.tabstrip);
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, p.border),
    );
    let project = &model.projects[index];
    let tabs: Vec<(u64, String, Option<String>, bool, bool)> = project
        .tabs
        .iter()
        .map(|tab| {
            let path = tab.path().map(|path| path.to_string());
            let (is_diff, external) = match &tab.kind {
                TabKind::File(file) => (false, file.external),
                TabKind::Diff(_) => (true, false),
            };
            (
                tab.id,
                tab.title(),
                path,
                tab.is_dirty(),
                is_diff || external,
            )
        })
        .collect();
    let active = project.active_tab;
    let mut x = rect.left();
    let mut pending: Option<Command> = None;
    let mut close: Option<u64> = None;
    for (id, title, path, dirty, badge) in tabs {
        let width = (title.chars().count() as f32 * 6.6 + 62.0).clamp(96.0, 196.0);
        let tab_rect =
            Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(width, rect.height()));
        if tab_rect.right() > rect.right() - 4.0 {
            break;
        }
        let is_active = active == Some(id);
        let body = Rect::from_min_max(
            tab_rect.min,
            Pos2::new(tab_rect.right() - 24.0, tab_rect.bottom()),
        );
        let close_rect = Rect::from_min_max(
            Pos2::new(tab_rect.right() - 24.0, tab_rect.top()),
            tab_rect.max,
        );
        let response = ui.interact(body, ui.scope_id().with(("tab", id)), Sense::click());
        let close_response = ui.interact(
            close_rect,
            ui.scope_id().with(("tab-close", id)),
            Sense::click(),
        );
        if is_active {
            ui.painter()
                .rect_filled(tab_rect, egui::CornerRadius::ZERO, p.tabstrip);
            ui.painter()
                .rect_filled(tab_rect, egui::CornerRadius::ZERO, p.editor);
            ui.painter().rect_filled(
                Rect::from_min_size(tab_rect.min, Vec2::new(tab_rect.width(), 2.0)),
                egui::CornerRadius::ZERO,
                p.accent,
            );
        } else if response.hovered() || close_response.hovered() {
            ui.painter()
                .rect_filled(tab_rect, egui::CornerRadius::ZERO, p.hover);
        } else {
            ui.painter().rect_filled(
                tab_rect,
                egui::CornerRadius::ZERO,
                p.tabstrip.gamma_multiply(0.98),
            );
        }
        let (icon, tone) = tab_icon(path.as_deref().unwrap_or_default(), theme);
        paint_icon(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(tab_rect.left() + 14.0, tab_rect.center().y),
                Vec2::splat(13.0),
            ),
            icon,
            if is_active {
                tone
            } else {
                tone.gamma_multiply(0.8)
            },
        );
        let text_rect = Rect::from_min_max(
            Pos2::new(tab_rect.left() + 24.0, tab_rect.top()),
            Pos2::new(close_rect.left() - 4.0, tab_rect.bottom()),
        );
        text_clipped(
            ui.painter(),
            text_rect,
            &title,
            FontId::proportional(11.5),
            if is_active { p.text } else { p.text2 },
            Align2::LEFT_CENTER,
        );
        if badge {
            let badge_rect = Rect::from_center_size(
                Pos2::new(close_rect.left() - 30.0, tab_rect.center().y),
                Vec2::new(30.0, 14.0),
            );
            ui.painter()
                .rect_filled(badge_rect, egui::CornerRadius::same(3), p.accent_soft);
            ui.painter().text(
                badge_rect.center(),
                Align2::CENTER_CENTER,
                "diff",
                FontId::monospace(8.0),
                p.accent,
            );
        } else if dirty {
            ui.painter().circle_filled(
                Pos2::new(close_rect.left() - 30.0, tab_rect.center().y),
                2.6,
                p.accent,
            );
        }
        if close_response.hovered() || (is_active && !dirty) {
            paint_icon(
                ui.painter(),
                Rect::from_center_size(close_rect.center(), Vec2::splat(12.0)),
                Icon::X,
                if close_response.hovered() {
                    p.text
                } else {
                    p.text3
                },
            );
        }
        if close_response.clicked() {
            close = Some(id);
        } else if response.clicked() {
            model.projects[index].active_tab = Some(id);
            rt.set_editor_focus(id);
        }
        x = tab_rect.right() + 1.0;
    }
    // Keep the rest of the bar draggable / empty.
    let rest = Rect::from_min_max(Pos2::new(x, rect.top()), rect.max);
    if rest.width() > 4.0 {
        let drag = ui.interact(
            rest,
            ui.scope_id().with("tabbar-drag"),
            Sense::click_and_drag(),
        );
        if drag.dragged() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
    }
    if let Some(id) = close {
        if let Some(tab) = model.projects[index].tab_by_id(id) {
            if tab.is_dirty() {
                rt.dialog = crate::runtime::DialogState::CloseDirtyTab {
                    tab_id: id,
                    name: tab.title(),
                };
            } else {
                model.projects[index].close_tab(id);
                if rt.editor_focus == Some(id) {
                    rt.editor_focus = None;
                }
            }
        }
    }
    if let Some(command) = pending.take() {
        model.queue(command);
    }
}

fn draw_crumbs(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    index: usize,
    tab_id: u64,
) {
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.editor);
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    let Some(tab) = model.projects[index].tab_by_id(tab_id) else {
        return;
    };
    let path = tab.path().unwrap_or_default().to_string();
    let root = &model.projects[index].root;
    let relative = std::path::Path::new(&path)
        .strip_prefix(root)
        .map(|rel| rel.to_string_lossy().into_owned())
        .unwrap_or(path);
    let parts: Vec<&str> = relative
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let mut x = rect.left() + 14.0;
    for (part_index, part) in parts.iter().enumerate() {
        let last = part_index + 1 == parts.len();
        let width = part.chars().count() as f32 * 6.5 + 4.0;
        if x + width > rect.right() - 30.0 {
            break;
        }
        ui.painter().text(
            Pos2::new(x, rect.center().y),
            Align2::LEFT_CENTER,
            *part,
            FontId::proportional(10.5),
            if last { p.text2 } else { p.text3 },
        );
        x += width;
        if !last {
            paint_icon(
                ui.painter(),
                Rect::from_center_size(Pos2::new(x + 4.0, rect.center().y), Vec2::splat(9.0)),
                Icon::ChevronRight,
                p.text3,
            );
            x += 12.0;
        }
    }
    let toggle_rect = Rect::from_center_size(
        Pos2::new(rect.right() - 18.0, rect.center().y),
        Vec2::splat(20.0),
    );
    let response = ui.interact(
        toggle_rect,
        ui.scope_id().with("minimap-toggle"),
        Sense::click(),
    );
    if response.hovered() {
        ui.painter()
            .rect_filled(toggle_rect, egui::CornerRadius::same(4), p.hover);
    }
    paint_icon(
        ui.painter(),
        toggle_rect.shrink(5.0),
        Icon::Layers,
        if model.ui.show_minimap {
            p.text2
        } else {
            p.text3
        },
    );
    if response.clicked() {
        model.ui.show_minimap = !model.ui.show_minimap;
        model.settings.editor.show_minimap = model.ui.show_minimap;
        model.save_settings();
    }
    response.on_hover_text("Toggle minimap");
}

fn token_color(kind: TokenKind, theme: &Theme) -> Color32 {
    let p = theme.palette;
    match kind {
        TokenKind::Text => p.text,
        TokenKind::Keyword => p.tok_kw,
        TokenKind::Type => p.tok_type,
        TokenKind::String => p.tok_str,
        TokenKind::Number => p.tok_num,
        TokenKind::Function => p.tok_fn,
        TokenKind::Comment => p.tok_com,
        TokenKind::Punctuation => p.tok_punc,
        TokenKind::Literal => p.tok_lit,
        TokenKind::Variable => p.tok_var,
        TokenKind::Property => p.tok_prop,
        TokenKind::Attribute => p.tok_attr,
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_file(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    index: usize,
    tab_id: u64,
) {
    let p = theme.palette;
    let settings = model.settings.editor.clone();
    let (font_size, tab_width, line_numbers, current_line, minimap) = (
        settings.font_size,
        settings.tab_width,
        settings.show_line_numbers,
        settings.highlight_current_line,
        model.ui.show_minimap,
    );
    let line_h = (font_size * 1.62).round().max(16.0);
    let char_w = mono_char_width(ui.ctx(), font_size);
    let font = FontId::monospace(font_size);

    let gutter_w = if line_numbers { GUTTER } else { 10.0 };
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
    // A modal surface owns the keyboard even when it has no text field (e.g.
    // settings or a confirmation dialog). Otherwise typing behind it would
    // silently edit the focused buffer.
    let overlays_open = rt.settings_open
        || rt.about_open
        || rt.search.open
        || model.ui.command_palette_open
        || rt.menu.is_some()
        || rt.agent_menu.is_some()
        || !matches!(rt.dialog, DialogState::None);
    let mut pending: Vec<Command> = Vec::new();

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
        let response = ui.interact(
            input_rect,
            ui.scope_id().with(("editor", tab_id)),
            Sense::click_and_drag(),
        );
        if response.clicked() || response.dragged() {
            rt.set_editor_focus(tab_id);
        }
        let pointer = response.interact_pointer_pos();
        if let Some(pos) = pointer {
            if response.clicked() || response.dragged() {
                let cursor = cursor_from_pos(pos, content, doc, char_w, line_h, tab_width);
                let extend = response.dragged() || ui.input(|input| input.modifiers.shift);
                doc.set_cursor(cursor, extend);
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

        if focused && !overlays_open && !ui.ctx().egui_wants_keyboard_input() {
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

        // Render visible lines.
        let painter = ui.painter().with_clip_rect(content);
        let sel = doc.selection_char_range();
        let cursor = doc.cursor;
        let scroll_line = doc.scroll_line;
        let scroll_x = doc.scroll_x;
        let tab_w = tab_width.max(1);
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
            if line_numbers {
                painter.text(
                    Pos2::new(gutter.right() - 10.0, y + line_h * 0.5),
                    Align2::RIGHT_CENTER,
                    (line_index + 1).to_string(),
                    FontId::monospace(font_size - 1.5),
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
            );
        }

        // Cursor + preedit.
        if focused {
            let line = cursor.line.min(lines.saturating_sub(1));
            let line_text = doc.line_content(line);
            let vcol = Document::visual_col(&line_text, cursor.col, tab_w);
            let x = content.left() + vcol as f32 * char_w - doc.scroll_x;
            let y = content.top() + (line as f32 - doc.scroll_line) * line_h;
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
            draw_minimap(ui, doc, theme, minimap_rect);
        }
        let _ = MONO;
    }

    for command in pending {
        model.queue(command);
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
fn draw_line_runs(
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
) {
    if text.is_empty() {
        return;
    }
    let chars: Vec<char> = text.chars().collect();
    let mut kinds = vec![TokenKind::Text; chars.len()];
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

fn cursor_from_pos(
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

fn select_word(doc: &mut Document, cursor: Cursor) {
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

fn handle_editor_input(
    ui: &mut Ui,
    doc: &mut Document,
    pending: &mut Vec<Command>,
    tab_id: u64,
    rt: &mut UiRuntime,
) {
    let events = ui.input(|input| input.events.clone());
    for event in events {
        match event {
            Event::Text(text) => {
                doc.insert_text(&text);
            }
            Event::Paste(text) => {
                doc.insert_text(&text);
            }
            Event::Copy => {
                if let Some(text) = doc.selected_text() {
                    ui.ctx().copy_text(text);
                }
            }
            Event::Cut => {
                if let Some(text) = doc.selected_text() {
                    ui.ctx().copy_text(text);
                    doc.delete_selection();
                }
            }
            Event::Ime(ImeEvent::Preedit { text, .. }) => {
                rt.ime_preedit = text;
            }
            Event::Ime(ImeEvent::Commit(text)) => {
                rt.ime_preedit.clear();
                doc.insert_text(&text);
            }
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } if handle_editor_key(ui, doc, pending, key, modifiers) => {
                ui.ctx().request_repaint();
            }
            _ => {}
        }
    }
    if doc.dirty && doc.highlighter.is_enabled() {
        ui.ctx().request_repaint();
    }
    let _ = (tab_id, pending);
}

fn handle_editor_key(
    ui: &mut Ui,
    doc: &mut Document,
    pending: &mut Vec<Command>,
    key: Key,
    modifiers: Modifiers,
) -> bool {
    let cmd = modifiers.command;
    if cmd {
        match key {
            Key::Z => {
                if modifiers.shift {
                    doc.redo();
                } else {
                    doc.undo();
                }
                return true;
            }
            Key::Y => {
                doc.redo();
                return true;
            }
            Key::A => {
                doc.select_all();
                return true;
            }
            Key::C => {
                if let Some(text) = doc.selected_text() {
                    ui.ctx().copy_text(text);
                }
                return true;
            }
            Key::X => {
                if let Some(text) = doc.selected_text() {
                    ui.ctx().copy_text(text);
                    doc.delete_selection();
                }
                return true;
            }
            Key::S => {
                return false;
            }
            _ => {}
        }
    }
    match key {
        Key::Enter => doc.newline(),
        Key::Backspace => doc.backspace(),
        Key::Delete => doc.delete_forward(),
        Key::Tab => {
            if modifiers.shift {
                doc.outdent_selection();
            } else {
                doc.indent_selection();
            }
        }
        Key::ArrowLeft => doc.move_cursor(
            if cmd {
                Motion::LineStart
            } else if modifiers.alt {
                Motion::WordLeft
            } else {
                Motion::Left
            },
            modifiers.shift,
        ),
        Key::ArrowRight => doc.move_cursor(
            if cmd {
                Motion::LineEnd
            } else if modifiers.alt {
                Motion::WordRight
            } else {
                Motion::Right
            },
            modifiers.shift,
        ),
        Key::ArrowUp => doc.move_cursor(
            if cmd { Motion::DocStart } else { Motion::Up },
            modifiers.shift,
        ),
        Key::ArrowDown => doc.move_cursor(
            if cmd { Motion::DocEnd } else { Motion::Down },
            modifiers.shift,
        ),
        Key::Home => doc.move_cursor(Motion::Home, modifiers.shift),
        Key::End => doc.move_cursor(Motion::End, modifiers.shift),
        Key::PageUp => doc.move_cursor(Motion::PageUp(24), modifiers.shift),
        Key::PageDown => doc.move_cursor(Motion::PageDown(24), modifiers.shift),
        Key::Escape => {
            doc.selection = None;
        }
        _ => return false,
    }
    let _ = pending;
    true
}

fn draw_minimap(ui: &mut Ui, doc: &Document, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    let total = doc.line_count().max(1);
    let mini_h = (rect.height() / total as f32).clamp(0.8, 3.0);
    let lines_to_draw = ((rect.height() / mini_h).floor() as usize).min(total);
    let painter = ui.painter().with_clip_rect(rect);
    for index in 0..lines_to_draw {
        let line_index =
            index + (doc.scroll_line as usize * total / total.max(1)).min(total.saturating_sub(1));
        if line_index >= total {
            break;
        }
        let text = doc.line_content(line_index);
        let width = (text.trim().chars().count() as f32 * 0.7).clamp(0.0, rect.width() - 8.0);
        if width > 1.0 {
            let y = rect.top() + index as f32 * mini_h;
            let color = if text.trim_start().starts_with("//") || text.trim_start().starts_with('#')
            {
                p.tok_com
            } else {
                p.text3
            };
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(rect.left() + 4.0, y),
                    Vec2::new(width, mini_h.max(1.0)),
                ),
                0.0,
                color.gamma_multiply(0.32),
            );
        }
    }
    let view_top = doc.scroll_line / total as f32 * rect.height();
    let view_h = (rect.height() / total as f32 * rect.height()).max(12.0);
    ui.painter().rect_stroke(
        Rect::from_min_size(
            Pos2::new(rect.left() + 1.0, rect.top() + view_top),
            Vec2::new(rect.width() - 2.0, view_h),
        ),
        egui::CornerRadius::same(2),
        Stroke::new(1.0, p.border_strong),
        StrokeKind::Inside,
    );
}

fn draw_diff(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    index: usize,
    tab_id: u64,
) {
    let p = theme.palette;
    let Some(tab) = model.projects[index].tab_by_id(tab_id) else {
        return;
    };
    let TabKind::Diff(diff) = &tab.kind else {
        return;
    };
    let path = diff.path.clone();
    let status = diff.diff.as_ref().map(|diff| diff.kind);
    let additions = diff
        .diff
        .as_ref()
        .map(|diff| diff.diff.additions)
        .unwrap_or(0);
    let deletions = diff
        .diff
        .as_ref()
        .map(|diff| diff.diff.deletions)
        .unwrap_or(0);
    let error = diff.error.clone();
    let hunks = diff.diff.as_ref().map(|diff| diff.diff.hunks.clone());
    let binary = diff.diff.as_ref().map(|diff| diff.binary).unwrap_or(false);
    let loading = diff.loading;

    let toolbar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 34.0));
    ui.painter().rect_filled(toolbar, 0.0, p.tabstrip);
    ui.painter().hline(
        toolbar.x_range(),
        toolbar.bottom() - 0.5,
        Stroke::new(1.0, p.border),
    );
    let (icon, tone) = tab_icon(&path, theme);
    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(toolbar.left() + 18.0, toolbar.center().y),
            Vec2::splat(14.0),
        ),
        icon,
        tone,
    );
    let name = path.rsplit('/').next().unwrap_or(&path);
    ui.painter().text(
        Pos2::new(toolbar.left() + 30.0, toolbar.center().y),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(11.5),
        p.text,
    );
    if let Some(kind) = status {
        ui.painter().text(
            Pos2::new(
                toolbar.left() + 30.0 + name.chars().count() as f32 * 6.5 + 10.0,
                toolbar.center().y,
            ),
            Align2::LEFT_CENTER,
            kind.label(),
            FontId::proportional(9.5),
            p.text3,
        );
    }
    ui.painter().text(
        Pos2::new(toolbar.center().x, toolbar.center().y),
        Align2::CENTER_CENTER,
        if model.settings.language.is_zh() {
            format!("HEAD → 工作区 · +{additions} −{deletions}")
        } else {
            format!("HEAD → Working tree · +{additions} −{deletions}")
        },
        FontId::monospace(9.5),
        p.text3,
    );
    let open_rect = Rect::from_center_size(
        Pos2::new(toolbar.right() - 48.0, toolbar.center().y),
        Vec2::new(78.0, 22.0),
    );
    if secondary_button(ui, open_rect, "Open file", None, theme).clicked() {
        let settings = model.settings.clone();
        let path = model.projects[index].root.join(&path);
        if let Err(err) = model.projects[index].open_file(path, &settings) {
            model.toast(err, ToastKind::Error);
        }
    }

    let body = Rect::from_min_max(Pos2::new(rect.left(), toolbar.bottom()), rect.max);
    if loading {
        ui.painter().text(
            body.center(),
            Align2::CENTER_CENTER,
            crate::tr!(model, "Loading diff…", "正在加载差异…"),
            FontId::proportional(11.5),
            p.text3,
        );
        return;
    }
    if let Some(error) = error {
        ui.painter().text(
            Pos2::new(body.left() + 20.0, body.top() + 30.0),
            Align2::LEFT_CENTER,
            error,
            FontId::proportional(11.0),
            p.rose,
        );
        return;
    }
    if binary {
        ui.painter().text(
            body.center(),
            Align2::CENTER_CENTER,
            crate::tr!(
                model,
                "Binary file — no textual diff",
                "二进制文件，无文本差异"
            ),
            FontId::proportional(11.5),
            p.text3,
        );
        return;
    }
    let Some(hunks) = hunks else {
        ui.painter().text(
            body.center(),
            Align2::CENTER_CENTER,
            crate::tr!(model, "No changes to display", "没有可显示的变更"),
            FontId::proportional(11.5),
            p.text3,
        );
        return;
    };
    let line_h = 18.0;
    let font = FontId::monospace(11.5);
    super::app_ui::region(ui, body, "diff-scroll", |ui| {
        egui::ScrollArea::vertical()
            .id_salt(("diff", tab_id))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for hunk in &hunks {
                    let (head_rect, _) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
                    ui.painter().rect_filled(head_rect, 0.0, p.panel2);
                    ui.painter().text(
                        Pos2::new(head_rect.left() + 12.0, head_rect.center().y),
                        Align2::LEFT_CENTER,
                        &hunk.header,
                        FontId::monospace(10.0),
                        p.text3,
                    );
                    for line in &hunk.lines {
                        let (row, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), line_h),
                            Sense::hover(),
                        );
                        match line.kind {
                            lazecode_text::DiffLineKind::Add => {
                                ui.painter().rect_filled(row, 0.0, p.diff_add);
                            }
                            lazecode_text::DiffLineKind::Delete => {
                                ui.painter().rect_filled(row, 0.0, p.diff_del);
                            }
                            _ => {}
                        }
                        let old = line.old_lineno.map(|n| n.to_string()).unwrap_or_default();
                        let new = line.new_lineno.map(|n| n.to_string()).unwrap_or_default();
                        ui.painter().text(
                            Pos2::new(row.left() + 46.0, row.center().y),
                            Align2::RIGHT_CENTER,
                            old,
                            FontId::monospace(9.5),
                            p.text3,
                        );
                        ui.painter().text(
                            Pos2::new(row.left() + 92.0, row.center().y),
                            Align2::RIGHT_CENTER,
                            new,
                            FontId::monospace(9.5),
                            p.text3,
                        );
                        let sign = match line.kind {
                            lazecode_text::DiffLineKind::Add => "+",
                            lazecode_text::DiffLineKind::Delete => "−",
                            _ => " ",
                        };
                        ui.painter().text(
                            Pos2::new(row.left() + 106.0, row.center().y),
                            Align2::LEFT_CENTER,
                            sign,
                            FontId::monospace(11.0),
                            match line.kind {
                                lazecode_text::DiffLineKind::Add => p.green,
                                lazecode_text::DiffLineKind::Delete => p.rose,
                                _ => p.text3,
                            },
                        );
                        text_clipped(
                            ui.painter(),
                            Rect::from_min_max(
                                Pos2::new(row.left() + 120.0, row.top()),
                                Pos2::new(row.right() - 12.0, row.bottom()),
                            ),
                            &line.text,
                            font.clone(),
                            if line.kind == lazecode_text::DiffLineKind::Context {
                                p.text2
                            } else {
                                p.text
                            },
                            Align2::LEFT_CENTER,
                        );
                    }
                }
                ui.add_space(40.0);
            });
    });
    let _ = rt;
}

fn draw_find(
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
        Pos2::new(rect.right() - 436.0, rect.top() + 8.0),
        Vec2::new(424.0, 82.0),
    );
    ui.painter()
        .rect_filled(panel, egui::CornerRadius::same(8), p.elev);
    ui.painter().rect_stroke(
        panel,
        egui::CornerRadius::same(8),
        Stroke::new(1.0, p.border_strong),
        StrokeKind::Inside,
    );
    let input_rect = Rect::from_min_size(
        Pos2::new(panel.left() + 10.0, panel.top() + 9.0),
        Vec2::new(panel.width() - 20.0, 24.0),
    );
    ui.painter()
        .rect_filled(input_rect, egui::CornerRadius::same(5), p.panel2);
    let query_rect = Rect::from_min_max(
        Pos2::new(input_rect.left() + 8.0, input_rect.top() + 2.0),
        Pos2::new(input_rect.right() - 130.0, input_rect.bottom() - 2.0),
    );
    let response = ui.put(
        query_rect,
        TextEdit::singleline(&mut rt.find.query)
            .frame(egui::Frame::NONE)
            .hint_text(crate::i18n::t(lang, "Find", "查找"))
            .font(FontId::proportional(11.0)),
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
        Pos2::new(input_rect.right() - 120.0, input_rect.center().y),
        Align2::LEFT_CENTER,
        if count > 0 {
            format!("{} / {}", (rt.find.index % count) + 1, count)
        } else if rt.find.query.is_empty() {
            String::new()
        } else {
            "no results".to_string()
        },
        FontId::monospace(9.0),
        p.text3,
    );
    // Toggles.
    let mut x = input_rect.left();
    for (label, active, id) in [
        ("Aa", rt.find.case_sensitive, "case"),
        ("ab", rt.find.whole_word, "word"),
        (".*", rt.find.regex, "regex"),
    ] {
        let toggle = Rect::from_min_size(Pos2::new(x, panel.top() + 44.0), Vec2::new(28.0, 20.0));
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
            FontId::monospace(9.5),
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
        Pos2::new(panel.left() + 120.0, panel.top() + 44.0),
        Vec2::new(28.0, 20.0),
    );
    if secondary_button(ui, nav_rect, "↑", None, theme).clicked() && count > 0 {
        rt.find.index = (rt.find.index + count - 1) % count;
        jump_to_match(doc, rt);
    }
    let nav_rect2 = Rect::from_min_size(
        Pos2::new(nav_rect.right() + 4.0, panel.top() + 44.0),
        Vec2::new(28.0, 20.0),
    );
    if secondary_button(ui, nav_rect2, "↓", None, theme).clicked() && count > 0 {
        rt.find.index = (rt.find.index + 1) % count;
        jump_to_match(doc, rt);
    }
    let close = Rect::from_center_size(
        Pos2::new(panel.right() - 16.0, panel.top() + 20.0),
        Vec2::splat(18.0),
    );
    let close_response = ui.interact(close, ui.scope_id().with("find-close"), Sense::click());
    paint_icon(ui.painter(), close.shrink(3.0), Icon::X, p.text3);
    if close_response.clicked() {
        rt.find.open = false;
    }

    // Replace row.
    let replace_rect = Rect::from_min_size(
        Pos2::new(panel.left() + 10.0, panel.top() + 50.0),
        Vec2::new(panel.width() - 20.0, 24.0),
    );
    ui.painter()
        .rect_filled(replace_rect, egui::CornerRadius::same(5), p.panel2);
    let field = Rect::from_min_max(
        Pos2::new(replace_rect.left() + 8.0, replace_rect.top() + 2.0),
        Pos2::new(replace_rect.right() - 100.0, replace_rect.bottom() - 2.0),
    );
    ui.put(
        field,
        TextEdit::singleline(&mut rt.find.replacement)
            .frame(egui::Frame::NONE)
            .hint_text(crate::i18n::t(lang, "Replace with", "替换为"))
            .font(FontId::proportional(11.0)),
    );
    let replace_all = Rect::from_center_size(
        Pos2::new(replace_rect.right() - 44.0, replace_rect.center().y),
        Vec2::new(82.0, 20.0),
    );
    if secondary_button(
        ui,
        replace_all,
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
    let _ = (rect, tab_id);
}

fn jump_to_match(doc: &mut Document, rt: &mut UiRuntime) {
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

#[allow(clippy::too_many_arguments)]
fn draw_welcome(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    index: Option<usize>,
) {
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.editor);
    let card = Rect::from_center_size(
        rect.center(),
        Vec2::new(
            720.0_f32.min(rect.width() - 60.0),
            420.0_f32.min(rect.height() - 60.0),
        ),
    );
    let left = card.left() + 34.0;
    let mut y = card.top() + 40.0;

    // Logo mark.
    let mark = Rect::from_min_size(Pos2::new(left, y), Vec2::splat(42.0));
    ui.painter()
        .rect_filled(mark, egui::CornerRadius::same(11), p.accent);
    ui.painter().text(
        mark.center(),
        Align2::CENTER_CENTER,
        ">_",
        FontId::monospace(16.0),
        p.accent_on_fill,
    );
    y += 58.0;
    let name = index
        .and_then(|index| model.projects.get(index))
        .map(|project| project.name.clone())
        .unwrap_or_else(|| "LazeCode".to_string());
    ui.painter().text(
        Pos2::new(left, y),
        Align2::LEFT_CENTER,
        &name,
        FontId::proportional(24.0),
        p.text,
    );
    y += 28.0;
    let subtitle = if index.is_some() {
        if model.settings.language.is_zh() {
            format!("{} 个项目已打开。所有进程一目了然。", model.projects.len())
        } else {
            format!(
                "{} projects open. Every process in view.",
                model.projects.len()
            )
        }
    } else {
        crate::tr!(
            model,
            "A quiet place for many projects. Open one to begin.",
            "一个安静的工作空间。打开项目即可开始。"
        )
        .to_string()
    };
    ui.painter().text(
        Pos2::new(left, y),
        Align2::LEFT_CENTER,
        subtitle,
        FontId::proportional(12.0),
        p.text3,
    );
    y += 30.0;

    let actions = [
        (
            crate::tr!(model, "Open project", "打开项目"),
            Icon::FolderOpen,
            "open-project",
        ),
        (
            crate::tr!(model, "New console", "新建控制台"),
            Icon::Plus,
            "new-console",
        ),
        (
            crate::tr!(model, "Git log", "Git 日志"),
            Icon::GitLog,
            "git-log",
        ),
        (
            crate::tr!(model, "Find in files", "在文件中查找"),
            Icon::Search,
            "search",
        ),
    ];
    let mut x = left;
    for (label, icon, id) in actions {
        let width = 104.0;
        let button = Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, 30.0));
        let primary = id == "open-project";
        let response = if primary {
            primary_button(ui, button, label, Some(icon), true, theme)
        } else {
            secondary_button(ui, button, label, Some(icon), theme)
        };
        if response.clicked() {
            match id {
                "open-project" => model.queue(Command::OpenProjectDialog),
                "new-console" => model.queue(Command::NewTerminal),
                "git-log" => {
                    model.ui.panel_visible = true;
                    model.ui.panel_tab = lazecode_core::PanelTab::Git;
                    model.queue(Command::RefreshLog);
                }
                _ => {
                    rt.search.open = true;
                    rt.search.focus_requested = true;
                }
            }
        }
        x += width + 8.0;
    }
    y += 52.0;

    // Recent / open files and shortcuts, two columns when there is room.
    let column_w = (card.width() - 88.0) * 0.56;
    let recent_rect = Rect::from_min_size(Pos2::new(left, y), Vec2::new(column_w, 168.0));
    ui.painter()
        .rect_filled(recent_rect, egui::CornerRadius::same(8), p.panel);
    ui.painter().text(
        Pos2::new(recent_rect.left() + 14.0, recent_rect.top() + 18.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Open files", "打开的文件"),
        FontId::proportional(10.0),
        p.text3,
    );
    let items: Vec<(PathBuf, bool)> = if let Some(index) = index {
        model.projects[index]
            .tabs
            .iter()
            .filter_map(|tab| match &tab.kind {
                TabKind::File(file) => Some((file.doc.path.clone(), false)),
                _ => None,
            })
            .take(5)
            .collect()
    } else {
        model
            .recent
            .iter()
            .take(5)
            .cloned()
            .map(|path| (path, true))
            .collect()
    };
    ui.painter().text(
        Pos2::new(recent_rect.left() + 14.0, recent_rect.top() + 18.0),
        Align2::LEFT_CENTER,
        if index.is_some() {
            crate::tr!(model, "Open files", "打开的文件")
        } else {
            crate::tr!(model, "Recent projects", "最近项目")
        },
        FontId::proportional(10.0),
        p.text3,
    );
    if items.is_empty() {
        ui.painter().text(
            Pos2::new(recent_rect.left() + 14.0, recent_rect.top() + 44.0),
            Align2::LEFT_CENTER,
            if index.is_some() {
                crate::tr!(
                    model,
                    "Click a file in the explorer",
                    "在资源管理器中点击文件"
                )
            } else {
                crate::tr!(model, "No recent projects", "暂无最近项目")
            },
            FontId::proportional(11.0),
            p.text3,
        );
    }
    let mut row_y = recent_rect.top() + 32.0;
    for (path, is_project) in items {
        let row = Rect::from_min_size(
            Pos2::new(recent_rect.left() + 8.0, row_y),
            Vec2::new(recent_rect.width() - 16.0, 24.0),
        );
        let response = ui.interact(
            row,
            ui.scope_id().with(("recent", path.clone())),
            Sense::click(),
        );
        if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(4), p.hover);
        }
        let (icon, tone) = if is_project {
            (Icon::Folder, p.accent)
        } else {
            tab_icon(path.to_str().unwrap_or_default(), theme)
        };
        paint_icon(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(row.left() + 10.0, row.center().y),
                Vec2::splat(12.0),
            ),
            icon,
            tone,
        );
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(row.left() + 22.0, row.top()),
                Pos2::new(row.right() - 8.0, row.bottom()),
            ),
            &name,
            FontId::proportional(11.0),
            p.text2,
            Align2::LEFT_CENTER,
        );
        if response.clicked() {
            if is_project {
                model.queue(Command::OpenProject(path));
            } else {
                let settings = model.settings.clone();
                if let Some(project) = model.active_project_mut() {
                    let _ = project.open_file(path, &settings);
                }
            }
        }
        row_y += 26.0;
    }

    let shortcuts = Rect::from_min_size(
        Pos2::new(recent_rect.right() + 16.0, y),
        Vec2::new(card.right() - 34.0 - (recent_rect.right() + 16.0), 168.0),
    );
    ui.painter()
        .rect_filled(shortcuts, egui::CornerRadius::same(8), p.panel);
    ui.painter().text(
        Pos2::new(shortcuts.left() + 14.0, shortcuts.top() + 18.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Keyboard", "快捷键"),
        FontId::proportional(10.0),
        p.text3,
    );
    let entries = [
        (crate::tr!(model, "Command palette", "命令面板"), "⌘K"),
        (crate::tr!(model, "Toggle explorer", "切换资源管理器"), "⌘B"),
        (crate::tr!(model, "Toggle console", "切换控制台"), "⌘J"),
        (crate::tr!(model, "Open Git log", "打开 Git 日志"), "⌘⇧G"),
        (crate::tr!(model, "Find in files", "在文件中查找"), "⌘⇧F"),
    ];
    let mut row_y = shortcuts.top() + 36.0;
    for (label, key) in entries {
        ui.painter().text(
            Pos2::new(shortcuts.left() + 14.0, row_y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(10.5),
            p.text3,
        );
        let kbd_rect = Rect::from_center_size(
            Pos2::new(shortcuts.right() - 26.0, row_y),
            Vec2::new(key.chars().count() as f32 * 9.0 + 8.0, 17.0),
        );
        crate::widgets::kbd(ui.painter(), kbd_rect, key, theme);
        row_y += 24.0;
    }

    let foot_y = card.bottom() - 20.0;
    ui.painter().text(
        Pos2::new(left, foot_y),
        Align2::LEFT_CENTER,
        if model.settings.language.is_zh() {
            format!(
                "{} 个运行中 · {} 个项目",
                model.total_running(),
                model.projects.len()
            )
        } else {
            format!(
                "{} running across {} projects",
                model.total_running(),
                model.projects.len()
            )
        },
        FontId::proportional(10.0),
        p.text3,
    );
    if let Some(index) = index {
        ui.painter().text(
            Pos2::new(card.right() - 34.0, foot_y),
            Align2::RIGHT_CENTER,
            format!(
                "{} · {}",
                model.projects[index].name,
                model.projects[index].branch()
            ),
            FontId::proportional(10.0),
            p.text3,
        );
    }
    let _ = rt;
}
