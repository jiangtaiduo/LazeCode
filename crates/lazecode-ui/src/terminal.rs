//! Terminal pane rendering: cells, graphics, input and the search bar.

mod a11y;
mod colors;
mod graphics;
mod input;
mod prelude;
mod search;

use a11y::*;
use colors::*;
use graphics::*;
use input::*;
use search::*;

pub use graphics::GraphicsCache;

use egui::{Align2, Color32, Key, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use lazecode_terminal::{
    Session, Snapshot, ATTR_BOLD, ATTR_DIM, ATTR_HIDDEN, ATTR_INVERSE, ATTR_LEADING_SPACER,
    ATTR_SPACER, ATTR_STRIKEOUT, ATTR_UNDERLINE, ATTR_WIDE,
};

use crate::runtime::{TermResizeGate, TermSearchState, TermSelection};
use crate::theme::Theme;
use crate::widgets::char_width;

const PAD_X: f32 = 8.0;
const PAD_Y: f32 = 6.0;
const SCROLLBAR: f32 = 8.0;

#[allow(clippy::too_many_arguments)]
pub fn draw(
    ui: &mut Ui,
    session: &Session,
    snapshot: &mut Snapshot,
    selection: &mut Option<TermSelection>,
    search: &mut TermSearchState,
    resize: &mut TermResizeGate,
    focus: bool,
    preedit: &mut String,
    wheel_accum: &mut f32,
    graphics: &mut GraphicsCache,
    refresh: bool,
    theme: &Theme,
    rect: Rect,
    session_id: u64,
    font_size: f32,
) -> TerminalOutput {
    let p = theme.palette;
    let window_focused = ui.ctx().input(|input| input.focused);
    if focus {
        let blink = if window_focused { 500 } else { 1000 };
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(blink));
    } else {
        preedit.clear();
    }
    let refresh = refresh || snapshot.rows == 0;
    // Repaint only while output is recent: 30 fps for streamed output, 60 fps
    // for typing, slow when unfocused.
    if focus && session.last_output_ms().is_some_and(|age| age < 120) {
        let recent_input = session.last_input_ms().is_some_and(|age| age < 500);
        ui.ctx()
            .request_repaint_after(output_repaint_delay(window_focused, recent_input));
    }
    let line_h = (font_size * 1.36).round().max(12.0);
    let char_w = char_width(ui.ctx(), crate::theme::terminal_font(font_size)).max(5.0);
    let inner = Rect::from_min_max(
        Pos2::new(rect.left() + PAD_X, rect.top() + PAD_Y),
        Pos2::new(rect.right() - SCROLLBAR, rect.bottom() - PAD_Y),
    );
    let cols = ((inner.width() / char_w).floor() as usize).max(2);
    let rows = ((inner.height() / line_h).floor() as usize).max(2);
    let (session_cols, session_rows) = session.size();
    let resize_needed = cols as u16 != session_cols || rows as u16 != session_rows;
    let mut sync_hold = session.sync_update_hold();
    // Do not resize under a program that is midway through a synchronized
    // frame: rio-vt ends the update on resize, and the rest of the frame then
    // lands as an incoherent grid. Resize after ESU instead.
    if resize_needed && sync_hold.is_none() {
        let target = (cols as u16, rows as u16);
        if resize.should_apply(target) {
            session.resize(target.0, target.1);
            // Absolute selection endpoints cannot survive reflow.
            *selection = None;
            *wheel_accum = 0.0;
            sync_hold = session.sync_update_hold();
        } else {
            // Keep the previous width until the drag pauses: reflowing every
            // frame rewraps the whole scrollback and SIGWINCHes TUIs.
            ui.ctx()
                .request_repaint_after(resize.remaining() + std::time::Duration::from_millis(1));
        }
    }
    // Hold the last complete frame while the program is inside a synchronized
    // update (DEC 2026) and for a short settle window after a resize. omp
    // repaints whole screens inside BSU/ESU; snapshotting mid-frame paints a
    // garbled one-frame flash of mixed old/new rows before the final frame.
    const RESIZE_SETTLE_MS: u64 = 100;
    // With no previous frame there is nothing to hold; the first snapshot
    // (sync-held when the program is mid-frame) fills the pane immediately.
    let has_frame = snapshot.rows > 0;
    let resize_hold = if !has_frame {
        None
    } else {
        session
            .last_resize_ms()
            .filter(|age| *age < RESIZE_SETTLE_MS)
            .map(|age| std::time::Duration::from_millis(RESIZE_SETTLE_MS - age))
    };
    let (session_cols, session_rows) = session.size();
    let size_mismatch =
        session_cols as usize != snapshot.cols || session_rows as usize != snapshot.rows;
    let want_snapshot = refresh || snapshot.rows == 0 || size_mismatch;
    // Only a pane that already has a frame can afford to hold one. A fresh
    // pane (first draw after a project/tab switch, or a console started while
    // in the background) that holds while a TUI streams inside BSU/ESU paints
    // nothing at all — a black console until the program stops repainting.
    let hold = has_frame && (sync_hold.is_some() || resize_hold.is_some());
    let mut refresh_pending = false;
    if want_snapshot && !hold {
        session.snapshot_into(snapshot);
    }
    // Mid-update the snapshot above is a partial frame; retry after ESU so the
    // panel replaces it with the complete one instead of waiting for the next
    // output wake.
    if refresh && (hold || sync_hold.is_some()) {
        refresh_pending = true;
    }
    if let Some(delay) = sync_hold.into_iter().chain(resize_hold).min() {
        ui.ctx()
            .request_repaint_after(delay + std::time::Duration::from_millis(1));
    }
    let cols = snapshot.cols.max(1);
    let rows = snapshot.rows.max(1);
    // Kitty graphics needs the cell pixel size to size a placement.
    session.set_cell_size(char_w, line_h);

    let mut focus_requested = false;
    let input_rect = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top()),
        Pos2::new(rect.right() - SCROLLBAR, rect.bottom()),
    );
    let response = ui
        .interact(
            input_rect,
            ui.scope_id().with(("terminal", session_id)),
            Sense::click_and_drag(),
        )
        .on_hover_cursor(egui::CursorIcon::Text);
    if ui
        .input(|i| i.pointer.hover_pos())
        .is_some_and(|pos| input_rect.contains(pos))
    {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
    }
    if response.clicked() || response.dragged() {
        focus_requested = true;
    }
    // Screen readers see the visible viewport text, cursor and selection. The
    // mirror is bounded to the viewport, so no history is cloned per frame.
    if ui.ctx().options(|options| options.screen_reader) {
        let mirror = a11y_mirror(snapshot, selection.as_ref(), focus);
        response.widget_info(|| egui::WidgetInfo {
            role: egui::accesskit::Role::Terminal,
            enabled: true,
            label: Some(mirror.label.clone()),
            current_text_value: Some(mirror.text.clone()),
            ..egui::WidgetInfo::new(egui::accesskit::Role::Terminal)
        });
        if !mirror.selection.is_empty() {
            ui.ctx().accesskit_node_builder(response.id, |node| {
                node.set_description(mirror.selection.clone());
            });
        }
    }

    let modes = snapshot.modes;
    let pointer = response.interact_pointer_pos();
    let selection_end = |pos: Pos2| -> (i32, usize) {
        let row = ((pos.y - inner.top()) / line_h).floor().max(0.0) as usize;
        let row = row.min(rows.saturating_sub(1));
        let col = ((pos.x - inner.left()) / char_w).round().max(0.0) as usize;
        let col = col.min(cols.saturating_sub(1));
        (snapshot.absolute_line(row), col)
    };

    let mouse_reporting = modes.mouse_click || modes.mouse_drag || modes.mouse_motion;
    if let Some(pos) = pointer {
        let shift = ui.input(|input| input.modifiers.shift);
        if mouse_reporting && !shift {
            let (primary_pressed, primary_released, secondary_pressed, secondary_released) = ui
                .input(|input| {
                    (
                        input.pointer.primary_pressed(),
                        input.pointer.primary_released(),
                        input.pointer.secondary_pressed(),
                        input.pointer.secondary_released(),
                    )
                });
            let (middle_pressed, middle_released) = ui.input(|input| {
                (
                    input.pointer.button_pressed(egui::PointerButton::Middle),
                    input.pointer.button_released(egui::PointerButton::Middle),
                )
            });
            let sgr = snapshot.modes.sgr_mouse;
            if primary_pressed {
                send_mouse(session, snapshot, pos, inner, line_h, char_w, 0, true, sgr);
            }
            if primary_released {
                send_mouse(session, snapshot, pos, inner, line_h, char_w, 0, false, sgr);
            }
            if middle_pressed {
                send_mouse(session, snapshot, pos, inner, line_h, char_w, 1, true, sgr);
            }
            if middle_released {
                send_mouse(session, snapshot, pos, inner, line_h, char_w, 1, false, sgr);
            }
            if secondary_pressed {
                send_mouse(session, snapshot, pos, inner, line_h, char_w, 2, true, sgr);
            }
            if secondary_released {
                send_mouse(session, snapshot, pos, inner, line_h, char_w, 2, false, sgr);
            }
            // Drag motion uses button code 32 (motion with left held).
            if snapshot.modes.mouse_drag && response.dragged() {
                send_mouse(session, snapshot, pos, inner, line_h, char_w, 32, true, sgr);
            }
        } else if response.is_pointer_button_down_on()
            && ui.input(|input| input.pointer.primary_down())
        {
            // Start on press: `clicked` fires on release, so press-and-drag
            // never created an anchor and short selections felt dead.
            let point = selection_end(pos);
            let pressed = ui.input(|input| input.pointer.primary_pressed());
            if pressed || selection.is_none() {
                *selection = Some(TermSelection {
                    start: point,
                    end: point,
                });
            } else if let Some(sel) = selection.as_mut() {
                sel.end = point;
            }
        }
    }

    // A press without movement places focus; only a drag selects. Leaving the
    // press anchor behind painted a permanent one-cell blue block.
    clear_selection_on_click(selection, response.clicked());

    // Only a real text editor should take keys away from the console.
    // `egui_wants_keyboard_input()` is "any widget has focus": one Shift+Tab
    // makes egui's focus navigation focus some button, which used to mute all
    // terminal keys (Ctrl+C, Shift+Tab, …) for the rest of the session.
    if focus && !search.open && !ui.ctx().text_edit_focused() {
        handle_input(
            ui,
            session,
            snapshot,
            preedit,
            session_id,
            selection.as_ref(),
        );
        if ui.input(|input| input.modifiers.command && input.key_pressed(Key::F)) {
            search.open_bar();
        }
    }

    // Wheel: scroll history unless the program owns the mouse. Fractional
    // trackpad deltas are accumulated so slow gestures are not lost.
    if response.hovered() {
        let dy = ui.input(|input| input.smooth_scroll_delta.y);
        if dy.abs() > 0.0 {
            if mouse_reporting && !ui.input(|input| input.modifiers.shift) {
                let button = if dy > 0.0 { 64 } else { 65 };
                send_wheel(
                    session,
                    snapshot,
                    &inner,
                    line_h,
                    char_w,
                    pos_or(pointer, inner),
                    button,
                );
            } else {
                *wheel_accum += dy / line_h;
                let whole = wheel_accum.trunc() as i32;
                if whole != 0 {
                    *wheel_accum -= whole as f32;
                    session.scroll_delta(whole);
                    session.snapshot_into(snapshot);
                }
            }
        }
    }

    // Backgrounds, selection and text.
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, p.terminal_bg);
    let (sel_start, sel_end) = match selection_bounds(selection.as_ref()) {
        Some((start, end)) => (Some(start), Some(end)),
        None => (None, None),
    };
    let search_match = search.matches.get(search.index).copied();
    let default_fg = p.text;
    let default_bg = p.terminal_bg;

    for row in 0..rows {
        let abs_line = snapshot.absolute_line(row);
        let y = inner.top() + row as f32 * line_h;

        // Background pass first, so text is never painted over by a later
        // background run.
        for col in 0..cols {
            let cell = snapshot.cell(row, col);
            if cell.attrs & ATTR_SPACER != 0
                || cell.attrs & ATTR_LEADING_SPACER != 0
                || cell.ch == '\u{10EEEE}'
            {
                continue;
            }
            let (mut fg, mut bg) = (resolve(cell.fg, &p), resolve(cell.bg, &p));
            if cell.attrs & ATTR_INVERSE != 0 {
                std::mem::swap(&mut fg, &mut bg);
            }
            if bg != default_bg {
                let width = if cell.attrs & ATTR_WIDE != 0 {
                    2.0 * char_w
                } else {
                    char_w
                };
                painter.rect_filled(
                    Rect::from_min_size(
                        Pos2::new(inner.left() + col as f32 * char_w, y),
                        Vec2::new(
                            width.min(inner.right() - (inner.left() + col as f32 * char_w)),
                            line_h,
                        ),
                    ),
                    0.0,
                    bg,
                );
            }
        }

        // Selection and search are overlays on top of cell backgrounds.
        if let (Some(start), Some(end)) = (sel_start, sel_end) {
            if abs_line >= start.0 && abs_line <= end.0 {
                let first_col = if abs_line == start.0 { start.1 } else { 0 };
                let last_col = if abs_line == end.0 {
                    (end.1 + 1).min(cols)
                } else {
                    cols
                };
                if last_col > first_col {
                    painter.rect_filled(
                        Rect::from_min_size(
                            Pos2::new(inner.left() + first_col as f32 * char_w, y),
                            Vec2::new((last_col - first_col) as f32 * char_w, line_h),
                        ),
                        0.0,
                        p.selection,
                    );
                }
            }
        }
        if search.open {
            if let Some(hit) = search_match {
                if hit.line == abs_line {
                    painter.rect_filled(
                        Rect::from_min_size(
                            Pos2::new(inner.left() + hit.col as f32 * char_w, y),
                            Vec2::new(hit.len as f32 * char_w, line_h),
                        ),
                        0.0,
                        p.accent_soft,
                    );
                }
            }
        }

        let mut run_start = 0usize;
        let mut run_text = String::new();
        let mut run_color = default_fg;
        let mut run_attrs = 0u16;
        for col in 0..cols {
            let cell = snapshot.cell(row, col);
            let attrs = cell.attrs;
            if attrs & ATTR_SPACER != 0 || attrs & ATTR_LEADING_SPACER != 0 {
                continue;
            }
            // Placeholder cells carry combining diacritics; flush the text run
            // and let the image overlay paint that rectangle.
            if cell.ch == '\u{10EEEE}' {
                flush_run(
                    &painter,
                    &mut run_text,
                    run_start,
                    col,
                    run_color,
                    run_attrs,
                    y,
                    line_h,
                    inner,
                    char_w,
                    font_size,
                    &mut run_color,
                );
                run_start = col + 1;
                run_attrs = attrs;
                continue;
            }
            let (mut fg, mut bg) = (resolve(cell.fg, &p), resolve(cell.bg, &p));
            if attrs & ATTR_INVERSE != 0 {
                std::mem::swap(&mut fg, &mut bg);
            }
            if attrs & ATTR_DIM != 0 {
                fg = fg.gamma_multiply(0.62);
            }
            if attrs & ATTR_BOLD != 0 {
                fg = brighten(fg, cell.fg, &p);
            }
            if attrs & ATTR_HIDDEN != 0 {
                continue;
            }
            let wide = attrs & ATTR_WIDE != 0;
            let text = cell.text.as_deref();
            if wide || text.is_some() {
                flush_run(
                    &painter,
                    &mut run_text,
                    run_start,
                    col,
                    run_color,
                    run_attrs,
                    y,
                    line_h,
                    inner,
                    char_w,
                    font_size,
                    &mut run_color,
                );
                run_attrs = attrs;
                let display = text
                    .map(|text| text.to_string())
                    .unwrap_or_else(|| cell.ch.to_string());
                let cell_w = if wide { 2.0 * char_w } else { char_w };
                painter.text(
                    Pos2::new(inner.left() + col as f32 * char_w, y + line_h * 0.5),
                    Align2::LEFT_CENTER,
                    display,
                    crate::theme::terminal_font(font_size),
                    fg,
                );
                if attrs & ATTR_UNDERLINE != 0 {
                    painter.line_segment(
                        [
                            Pos2::new(inner.left() + col as f32 * char_w, y + line_h - 1.5),
                            Pos2::new(
                                inner.left() + col as f32 * char_w + cell_w,
                                y + line_h - 1.5,
                            ),
                        ],
                        Stroke::new(1.0, fg),
                    );
                }
                run_start = col + if wide { 2 } else { 1 };
                run_text.clear();
                run_color = fg;
                continue;
            }
            if run_color != fg || run_attrs != attrs {
                flush_run(
                    &painter,
                    &mut run_text,
                    run_start,
                    col,
                    run_color,
                    run_attrs,
                    y,
                    line_h,
                    inner,
                    char_w,
                    font_size,
                    &mut run_color,
                );
                run_start = col;
                run_color = fg;
                run_attrs = attrs;
            }
            run_text.push(cell.ch);
        }
        if !run_text.is_empty() {
            flush_run(
                &painter,
                &mut run_text,
                run_start,
                cols,
                run_color,
                run_attrs,
                y,
                line_h,
                inner,
                char_w,
                font_size,
                &mut run_color,
            );
        }
    }

    // Kitty graphics overlays sit on top of the cell grid and below the
    // cursor.
    graphics.sync(ui.ctx(), session, snapshot);
    graphics.paint(&painter, snapshot, inner, char_w, line_h, rows);

    // Cursor & IME activation.
    if focus {
        let (cx, cy) = if let Some(cursor) = snapshot.cursor {
            let x = inner.left() + cursor.col as f32 * char_w;
            let y = inner.top() + (cursor.row.min(rows.saturating_sub(1))) as f32 * line_h;
            (x, y)
        } else {
            (inner.left(), inner.top())
        };
        let cursor_rect = Rect::from_min_size(Pos2::new(cx, cy), Vec2::new(char_w, line_h));
        ui.ctx().output_mut(|o| {
            o.ime = Some(egui::output::IMEOutput {
                purpose: egui::IMEPurpose::Normal,
                rect: inner,
                cursor_rect,
                should_interrupt_composition: false,
            });
        });

        if let Some(cursor) = snapshot.cursor {
            if cursor.row < rows {
                let x = inner.left() + cursor.col as f32 * char_w;
                let y = inner.top() + cursor.row as f32 * line_h;
                let blink = ((ui.input(|input| input.time) * 1.6) as i64 % 2) == 0;
                if blink || !preedit.is_empty() {
                    match cursor.shape {
                        lazecode_terminal::CursorShape::Beam => {
                            painter.rect_filled(
                                Rect::from_min_size(Pos2::new(x, y), Vec2::new(1.6, line_h)),
                                0.0,
                                p.accent,
                            );
                        }
                        lazecode_terminal::CursorShape::Underline => {
                            painter.rect_filled(
                                Rect::from_min_size(
                                    Pos2::new(x, y + line_h - 2.0),
                                    Vec2::new(char_w, 2.0),
                                ),
                                0.0,
                                p.accent,
                            );
                        }
                        _ => {
                            painter.rect_filled(
                                Rect::from_min_size(Pos2::new(x, y), Vec2::new(char_w, line_h)),
                                0.0,
                                p.accent.gamma_multiply(0.65),
                            );
                            let cell = snapshot.cell(cursor.row, cursor.col);
                            let text = cell
                                .text
                                .as_deref()
                                .map(|text| text.to_string())
                                .unwrap_or_else(|| cell.ch.to_string());
                            painter.text(
                                Pos2::new(x + char_w * 0.5, y + line_h * 0.5),
                                Align2::CENTER_CENTER,
                                text,
                                crate::theme::terminal_font(font_size),
                                p.accent_on_fill,
                            );
                        }
                    }
                }
                if !preedit.is_empty() {
                    let width = char_w * preedit.chars().count() as f32;
                    painter.rect_filled(
                        Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, line_h)),
                        0.0,
                        p.terminal_bg,
                    );
                    painter.text(
                        Pos2::new(x, y + line_h * 0.5),
                        Align2::LEFT_CENTER,
                        preedit.as_str(),
                        crate::theme::terminal_font(font_size),
                        p.text,
                    );
                    painter.line_segment(
                        [
                            Pos2::new(x, y + line_h - 1.0),
                            Pos2::new(x + width, y + line_h - 1.0),
                        ],
                        Stroke::new(1.0, p.accent),
                    );
                }
            }
        }
    }

    // Scrollback.
    if snapshot.history_size > 0 {
        let track = Rect::from_min_max(
            Pos2::new(rect.right() - SCROLLBAR, rect.top()),
            Pos2::new(rect.right(), rect.bottom()),
        );
        let history = snapshot.history_size;
        let content_h = (history + rows) as f32 * line_h;
        let viewport_h = rows as f32 * line_h;
        let mut offset = (history.saturating_sub(snapshot.display_offset)) as f32 * line_h;
        if crate::widgets::scrollbar(ui, track, content_h, viewport_h, &mut offset, theme) {
            let target = (offset / line_h).round() as i32;
            let current = history.saturating_sub(snapshot.display_offset) as i32;
            // `Scroll::Delta` is positive-up: dragging the thumb up increases
            // display_offset (distance from the top).
            session.scroll_delta(current - target);
            session.snapshot_into(snapshot);
        }
    }

    // Terminal search overlay.
    if search.open {
        draw_search(ui, theme, rect, session, search);
    }

    TerminalOutput {
        focus_requested,
        refresh_pending,
    }
}

#[allow(clippy::too_many_arguments)]
fn flush_run(
    painter: &egui::Painter,
    run_text: &mut String,
    start: usize,
    end: usize,
    color: Color32,
    attrs: u16,
    y: f32,
    line_h: f32,
    inner: Rect,
    char_w: f32,
    font_size: f32,
    run_color: &mut Color32,
) {
    if run_text.is_empty() || end <= start {
        run_text.clear();
        *run_color = color;
        return;
    }
    painter.text(
        Pos2::new(inner.left() + start as f32 * char_w, y + line_h * 0.5),
        Align2::LEFT_CENTER,
        run_text.as_str(),
        crate::theme::terminal_font(font_size),
        color,
    );
    if attrs & ATTR_UNDERLINE != 0 {
        painter.line_segment(
            [
                Pos2::new(inner.left() + start as f32 * char_w, y + line_h - 1.5),
                Pos2::new(inner.left() + end as f32 * char_w, y + line_h - 1.5),
            ],
            Stroke::new(1.0, color),
        );
    }
    if attrs & ATTR_STRIKEOUT != 0 {
        painter.line_segment(
            [
                Pos2::new(inner.left() + start as f32 * char_w, y + line_h * 0.5),
                Pos2::new(inner.left() + end as f32 * char_w, y + line_h * 0.5),
            ],
            Stroke::new(1.0, color),
        );
    }
    run_text.clear();
    *run_color = color;
}

#[cfg(test)]
mod tests {
    use egui::Modifiers;
    use lazecode_terminal::{CellData, ImagePlacement, TerminalKey};

    use super::graphics::{source_uv, CachedImage};
    use super::input::key_to_stroke;
    use super::*;

    fn text_snapshot(rows: &[&str], cols: usize) -> Snapshot {
        let mut snapshot = Snapshot {
            cols,
            rows: rows.len(),
            ..Snapshot::default()
        };
        snapshot.cells = vec![CellData::default(); cols * rows.len()];
        for (row, line) in rows.iter().enumerate() {
            for (col, ch) in line.chars().enumerate().take(cols) {
                snapshot.cells[row * cols + col].ch = ch;
            }
        }
        snapshot
    }

    #[test]
    fn collapsed_terminal_selection_is_not_visible() {
        let snapshot = text_snapshot(&["$ ls"], 8);
        let collapsed = TermSelection {
            start: (0, 2),
            end: (0, 2),
        };
        assert!(selection_bounds(None).is_none());
        assert!(selection_bounds(Some(&collapsed)).is_none());
        assert!(selection_text(&snapshot, &collapsed).is_empty());

        let mut selection = Some(collapsed);
        clear_selection_on_click(&mut selection, false);
        assert!(selection.is_some(), "press anchor stays until release");
        clear_selection_on_click(&mut selection, true);
        assert!(selection.is_none(), "a completed click clears the anchor");

        let range = TermSelection {
            start: (0, 0),
            end: (0, 3),
        };
        assert_eq!(selection_bounds(Some(&range)), Some(((0, 0), (0, 3))));
        assert_eq!(selection_text(&snapshot, &range), "$ ls");
    }

    #[test]
    fn a11y_text_mirrors_the_visible_viewport_only() {
        let snapshot = text_snapshot(&["hello", "world"], 8);
        assert_eq!(a11y_text(&snapshot), "hello\nworld");

        // History size does not change the mirror size: only viewport cells
        // are exposed, so a screen reader never receives the whole scrollback.
        let mut scrolled = snapshot.clone();
        scrolled.history_size = 5_000;
        scrolled.display_offset = 120;
        scrolled.hot_history_size = 400;
        assert_eq!(a11y_text(&scrolled), "hello\nworld");
        assert_eq!(scrolled.cells.len(), scrolled.cols * scrolled.rows);
    }

    #[test]
    fn a11y_label_reports_cursor_scroll_and_focus() {
        let mut snapshot = text_snapshot(&["$ ls"], 8);
        snapshot.cursor = Some(lazecode_terminal::CursorView {
            row: 0,
            col: 2,
            shape: lazecode_terminal::CursorShape::Block,
            blinking: false,
        });
        snapshot.history_size = 40;
        snapshot.display_offset = 7;
        let mirror = a11y_mirror(&snapshot, None, true);
        assert!(
            mirror.label.contains("cursor line 1 column 3"),
            "{}",
            mirror.label
        );
        assert!(
            mirror.label.contains("scrolled back 7 of 40"),
            "{}",
            mirror.label
        );
        assert!(mirror.label.contains("focused"), "{}", mirror.label);
        assert!(mirror.selection.is_empty());

        // Absolute selection lines: the viewport starts at line -7.
        let selection = TermSelection {
            start: (-7, 0),
            end: (-7, 4),
        };
        let mirror = a11y_mirror(&snapshot, Some(&selection), false);
        assert_eq!(mirror.selection, "$ ls");
        assert!(mirror.label.contains("unfocused"));
    }

    #[test]
    fn kitty_source_rect_maps_to_texture_uv() {
        let placement = ImagePlacement {
            image_id: 1,
            col: 0,
            row: 0,
            columns: 1,
            rows: 1,
            source: (0, 0, 0, 0),
            z_index: 0,
        };
        let uv = source_uv(&placement, Vec2::new(10.0, 20.0));
        assert_eq!(uv, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)));

        let cropped = ImagePlacement {
            source: (5, 10, 5, 10),
            ..placement
        };
        let uv = source_uv(&cropped, Vec2::new(10.0, 20.0));
        assert_eq!(
            uv,
            Rect::from_min_max(Pos2::new(0.5, 0.5), Pos2::new(1.0, 1.0))
        );

        // A crop larger than the image clamps to the remaining pixels.
        let clamped = ImagePlacement {
            source: (5, 10, 500, 500),
            ..placement
        };
        let uv = source_uv(&clamped, Vec2::new(10.0, 20.0));
        assert_eq!(
            uv,
            Rect::from_min_max(Pos2::new(0.5, 0.5), Pos2::new(1.0, 1.0))
        );
    }

    #[test]
    fn graphics_cache_uploads_once_and_prunes_evicted_images() {
        let ctx = egui::Context::default();
        let mut cache = GraphicsCache::default();
        let image = lazecode_terminal::KittyImage {
            id: 7,
            width: 2,
            height: 2,
            rgba: std::sync::Arc::from(vec![0u8; 16].into_boxed_slice()),
            generation: 1,
        };
        cache.apply(&ctx, vec![image.clone()], &[7]);
        assert_eq!(cache.images.len(), 1);
        assert_eq!(cache.images[&7].generation, 1);

        // A stale upload is ignored; a new generation replaces the texture.
        cache.apply(&ctx, vec![image.clone()], &[7]);
        assert_eq!(cache.images[&7].generation, 1);
        let newer = lazecode_terminal::KittyImage {
            generation: 2,
            ..image
        };
        cache.apply(&ctx, vec![newer], &[7]);
        assert_eq!(cache.images[&7].generation, 2);

        // Eviction in the terminal drops the texture here too.
        cache.apply(&ctx, Vec::new(), &[]);
        assert!(cache.images.is_empty());
    }

    #[test]
    fn graphics_cache_paints_virtual_only_snapshots() {
        use egui::ColorImage;

        let ctx = egui::Context::default();
        let texture = ctx.load_texture(
            "virtual-test",
            ColorImage::new([1, 1], vec![Color32::WHITE]),
            egui::TextureOptions::NEAREST,
        );
        let mut cache = GraphicsCache::default();
        cache.images.insert(
            9,
            CachedImage {
                generation: 1,
                texture,
            },
        );
        let mut snapshot = Snapshot::default();
        snapshot
            .virtual_graphics
            .push(lazecode_terminal::VirtualImageRun {
                image_id: 9,
                col: 1.0,
                row: 1.0,
                columns: 2.0,
                rows: 1.0,
                source_uv: [0.0, 0.0, 1.0, 1.0],
            });

        let mut output = ctx.run_ui(Default::default(), |ui| {
            cache.paint(
                ui.painter(),
                &snapshot,
                Rect::from_min_size(Pos2::ZERO, Vec2::new(80.0, 80.0)),
                8.0,
                16.0,
                4,
            );
        });
        output.textures_delta.clear();
        assert!(
            output
                .shapes
                .iter()
                .any(|clipped| matches!(clipped.shape, egui::Shape::Mesh(_))),
            "virtual-only graphics must emit an image mesh"
        );
    }

    #[test]
    fn ctrl_and_alt_character_keys_become_strokes() {
        let ctrl = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        let stroke = key_to_stroke(Key::C, ctrl).expect("ctrl+c stroke");
        assert!(matches!(stroke.key, TerminalKey::Char('c')));
        assert!(stroke.ctrl);

        let alt = Modifiers {
            alt: true,
            ..Default::default()
        };
        let stroke = key_to_stroke(Key::B, alt).expect("alt+b stroke");
        assert!(matches!(stroke.key, TerminalKey::Char('b')));
        assert!(stroke.alt);

        assert!(key_to_stroke(Key::A, Modifiers::default()).is_none());
    }

    #[test]
    fn test_cjk_metrics() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx, &lazecode_core::Settings::default());
        let mut out = ctx.run_ui(Default::default(), |_| {});
        out.textures_delta.clear();
        let size = 13.0;
        let terminal_font = crate::theme::terminal_font(size);
        let char_w = crate::widgets::char_width(&ctx, terminal_font.clone());
        let ascii_galley = ctx.fonts_mut(|f| {
            f.layout_no_wrap("M".to_string(), terminal_font.clone(), egui::Color32::WHITE)
        });
        let cjk_galley = ctx.fonts_mut(|f| {
            f.layout_no_wrap("你好".to_string(), terminal_font, egui::Color32::WHITE)
        });
        assert!(cjk_galley.size().x > char_w);
        assert!(ascii_galley.size().x > 0.0);
        assert!(char_w > 0.0);
    }

    #[test]
    fn indexed_colors_cover_the_256_palette() {
        let palette = crate::theme::Theme::for_mode(lazecode_core::ThemeMode::Dark).palette;
        assert_eq!(
            indexed_color(196, &palette),
            egui::Color32::from_rgb(255, 0, 0)
        );
        assert_eq!(
            indexed_color(46, &palette),
            egui::Color32::from_rgb(0, 255, 0)
        );
        assert_eq!(
            indexed_color(232, &palette),
            egui::Color32::from_rgb(8, 8, 8)
        );
        assert_eq!(indexed_color(15, &palette), palette.ansi[15]);
    }

    #[test]
    fn light_terminal_keeps_indexed_colours_readable() {
        let palette = crate::theme::Theme::new(
            lazecode_core::ThemeMode::Light,
            lazecode_core::ThemeSkin::Graphite,
        )
        .palette;
        for index in [16, 46, 196, 231, 244, 250, 255] {
            let color = indexed_color(index, &palette);
            let ratio = contrast(color, palette.terminal_bg);
            assert!(
                ratio >= 3.0,
                "indexed {index} on light terminal is {ratio:.1}:1"
            );
        }
    }

    #[test]
    fn terminal_output_repaint_budget_is_adaptive() {
        use std::time::Duration;
        assert_eq!(
            output_repaint_delay(false, false),
            Duration::from_millis(100)
        );
        assert_eq!(
            output_repaint_delay(false, true),
            Duration::from_millis(100)
        );
        assert_eq!(output_repaint_delay(true, true), Duration::from_millis(16));
        assert_eq!(output_repaint_delay(true, false), Duration::from_millis(33));
    }
}
