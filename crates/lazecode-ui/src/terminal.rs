use egui::{
    Align2, Color32, Event, FontId, ImeEvent, Key, Modifiers, Pos2, Rect, Sense, Stroke,
    StrokeKind, TextEdit, Ui, Vec2,
};
use lazecode_terminal::{
    ColorSpec, Session, Snapshot, TerminalKey, ATTR_BOLD, ATTR_DIM, ATTR_HIDDEN, ATTR_INVERSE,
    ATTR_LEADING_SPACER, ATTR_SPACER, ATTR_STRIKEOUT, ATTR_UNDERLINE, ATTR_WIDE,
};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::{TermSearchState, TermSelection};
use crate::theme::Theme;
use crate::widgets::mono_char_width;

const PAD_X: f32 = 8.0;
const PAD_Y: f32 = 6.0;
const SCROLLBAR: f32 = 8.0;

pub struct TerminalOutput {
    pub focus_requested: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    ui: &mut Ui,
    session: &Session,
    snapshot: &mut Snapshot,
    selection: &mut Option<TermSelection>,
    search: &mut TermSearchState,
    focus: bool,
    preedit: &mut String,
    theme: &Theme,
    rect: Rect,
    session_id: u64,
    font_size: f32,
) -> TerminalOutput {
    let p = theme.palette;
    if focus {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
    }
    session.snapshot_into(snapshot);
    // Cap repaint rate for output-heavy sessions: the notifier wakes the UI
    // for every PTY batch; this repaint cap keeps the frame budget at 60fps
    // maximum so a `cat` of a 100MB file cannot starve input handling.
    if session.last_output_ms().is_some() && focus {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(16));
    }
    let line_h = (font_size * 1.36).round().max(12.0);
    let char_w = mono_char_width(ui.ctx(), font_size).max(5.0);
    let inner = Rect::from_min_max(
        Pos2::new(rect.left() + PAD_X, rect.top() + PAD_Y),
        Pos2::new(rect.right() - SCROLLBAR, rect.bottom() - PAD_Y),
    );
    let cols = ((inner.width() / char_w).floor() as usize).max(2);
    let rows = ((inner.height() / line_h).floor() as usize).max(2);
    if cols != snapshot.cols || rows != snapshot.rows {
        session.resize(cols as u16, rows as u16);
    }
    let cols = snapshot.cols.max(1);
    let rows = snapshot.rows.max(1);

    let mut focus_requested = false;
    let input_rect = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top()),
        Pos2::new(rect.right() - SCROLLBAR, rect.bottom()),
    );
    let response = ui.interact(
        input_rect,
        ui.scope_id().with(("terminal", session_id)),
        Sense::click_and_drag(),
    );
    if response.clicked() || response.dragged() {
        focus_requested = true;
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
        if response.clicked() {
            if mouse_reporting && !ui.input(|input| input.modifiers.shift) {
                send_mouse(
                    session,
                    snapshot,
                    pos,
                    inner,
                    line_h,
                    char_w,
                    0,
                    true,
                    snapshot.modes.sgr_mouse,
                );
            } else {
                let point = selection_end(pos);
                *selection = Some(TermSelection {
                    start: point,
                    end: point,
                });
            }
        } else if response.dragged() {
            if let Some(sel) = selection.as_mut() {
                sel.end = selection_end(pos);
            }
        }
    }

    if focus && !ui.ctx().egui_wants_keyboard_input() {
        handle_input(ui, session, snapshot, preedit, session_id);
        if let Some(sel) = selection.as_ref() {
            if ui.input(|input| input.modifiers.command && input.key_pressed(Key::C)) {
                let text = selection_text(snapshot, sel);
                if !text.is_empty() {
                    ui.ctx().copy_text(text);
                }
            }
        }
        if ui.input(|input| input.modifiers.command && input.key_pressed(Key::F)) {
            search.open = true;
        }
    }

    // Wheel: scroll history unless the program owns the mouse.
    if response.hovered() {
        let dy = ui.input(|input| input.smooth_scroll_delta.y);
        if dy.abs() > 0.5 {
            if mouse_reporting && !ui.input(|input| input.modifiers.shift) {
                let button = if dy > 0.0 { 64 } else { 65 };
                send_wheel(session, button, snapshot.modes.sgr_mouse);
            } else {
                let delta = (dy / line_h).round() as i32;
                session.scroll_delta(delta);
                session.snapshot_into(snapshot);
            }
        }
    }

    // Backgrounds, selection and text.
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, p.terminal_bg);
    let (sel_start, sel_end) = if let Some(sel) = selection.as_ref() {
        let (start, end) = sel.normalised();
        (Some(start), Some(end))
    } else {
        (None, None)
    };
    let search_match = search.matches.get(search.index).copied();
    let default_fg = p.text;
    let default_bg = p.terminal_bg;

    for row in 0..rows {
        let abs_line = snapshot.absolute_line(row);
        let y = inner.top() + row as f32 * line_h;
        // Selection band.
        if let (Some(start), Some(end)) = (sel_start, sel_end) {
            if abs_line >= start.0 && abs_line <= end.0 {
                let first_col = if abs_line == start.0 { start.1 } else { 0 };
                let last_col = if abs_line == end.0 { end.1 } else { cols };
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

        // Background pass first, so text is never painted over by a later
        // background run.
        for col in 0..cols {
            let cell = snapshot.cell(row, col);
            if cell.attrs & ATTR_SPACER != 0 || cell.attrs & ATTR_LEADING_SPACER != 0 {
                continue;
            }
            let (mut fg, mut bg) = (resolve(cell.fg, &p), resolve(cell.bg, &p));
            if cell.attrs & ATTR_INVERSE != 0 {
                std::mem::swap(&mut fg, &mut bg);
            }
            if bg != default_bg {
                painter.rect_filled(
                    Rect::from_min_size(
                        Pos2::new(inner.left() + col as f32 * char_w, y),
                        Vec2::new(char_w, line_h),
                    ),
                    0.0,
                    bg,
                );
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
                    FontId::monospace(font_size),
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
                                FontId::monospace(font_size),
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
                        FontId::monospace(font_size),
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
        let before = offset;
        if crate::widgets::scrollbar(ui, track, content_h, viewport_h, &mut offset, theme) {
            let target = (offset / line_h).round() as i32;
            let current = (history.saturating_sub(snapshot.display_offset)) as i32;
            session.scroll_delta(target - current);
        }
        let _ = before;
    }

    // Terminal search overlay.
    if search.open {
        draw_search(ui, theme, rect, session, search);
    }

    TerminalOutput { focus_requested }
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
        FontId::monospace(font_size),
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

fn resolve(color: ColorSpec, p: &crate::theme::Palette) -> Color32 {
    match color {
        ColorSpec::DefaultFg => p.text,
        ColorSpec::DefaultBg => p.terminal_bg,
        ColorSpec::Cursor => p.accent,
        ColorSpec::Indexed(index) => p.ansi[(index as usize).min(15)],
        ColorSpec::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    }
}

fn brighten(fg: Color32, spec: ColorSpec, p: &crate::theme::Palette) -> Color32 {
    if let ColorSpec::Indexed(index) = spec {
        if index < 8 {
            return p.ansi[index as usize + 8];
        }
    }
    fg.gamma_multiply(1.12)
}

fn handle_input(
    ui: &mut Ui,
    session: &Session,
    snapshot: &Snapshot,
    preedit: &mut String,
    _id: u64,
) {
    let events = ui.input(|input| input.events.clone());
    let alt_down = ui.input(|input| input.modifiers.alt);
    let mut bytes = Vec::new();
    for event in events {
        match event {
            Event::Text(text) => {
                // With Option-as-Alt, the Key event carries the meta prefix.
                // Do not also insert the composed/unmodified text.
                if !alt_down {
                    bytes.extend_from_slice(text.as_bytes());
                }
            }
            Event::Paste(text) => {
                bytes.extend(lazecode_terminal::encode_paste(
                    &text,
                    snapshot.modes.bracketed_paste,
                ));
            }
            Event::Ime(ImeEvent::Preedit { text, .. }) => {
                *preedit = text;
            }
            Event::Ime(ImeEvent::Commit(text)) => {
                preedit.clear();
                bytes.extend_from_slice(text.as_bytes());
            }
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                if let Some(stroke) = key_to_stroke(key, modifiers) {
                    if let Some(encoded) = encode_stroke_for(stroke, snapshot) {
                        bytes.extend_from_slice(&encoded);
                    }
                }
            }
            Event::Copy => {
                // Copy handled by the caller, which owns the selection.
            }
            _ => {}
        }
    }
    if !bytes.is_empty() {
        session.write(&bytes);
    }
}

fn encode_stroke_for(stroke: lazecode_terminal::KeyStroke, snapshot: &Snapshot) -> Option<Vec<u8>> {
    lazecode_terminal::encode_key(stroke, &snapshot.modes)
}

fn key_to_stroke(key: Key, modifiers: Modifiers) -> Option<lazecode_terminal::KeyStroke> {
    use lazecode_terminal::KeyStroke;
    if modifiers.command {
        return None;
    }
    if modifiers.ctrl || modifiers.alt {
        if let Some(mut ch) = key_char(key) {
            if modifiers.ctrl {
                ch = ch.to_ascii_lowercase();
            }
            return Some(KeyStroke {
                key: TerminalKey::Char(ch),
                ctrl: modifiers.ctrl,
                alt: modifiers.alt,
                shift: modifiers.shift,
            });
        }
    }
    let terminal_key = match key {
        Key::Enter => TerminalKey::Enter,
        Key::Tab => TerminalKey::Tab,
        Key::Backspace => TerminalKey::Backspace,
        Key::Escape => TerminalKey::Escape,
        Key::ArrowUp => TerminalKey::Up,
        Key::ArrowDown => TerminalKey::Down,
        Key::ArrowLeft => TerminalKey::Left,
        Key::ArrowRight => TerminalKey::Right,
        Key::Home => TerminalKey::Home,
        Key::End => TerminalKey::End,
        Key::PageUp => TerminalKey::PageUp,
        Key::PageDown => TerminalKey::PageDown,
        Key::Delete => TerminalKey::Delete,
        Key::Insert => TerminalKey::Insert,
        Key::F1 => TerminalKey::F(1),
        Key::F2 => TerminalKey::F(2),
        Key::F3 => TerminalKey::F(3),
        Key::F4 => TerminalKey::F(4),
        Key::F5 => TerminalKey::F(5),
        Key::F6 => TerminalKey::F(6),
        Key::F7 => TerminalKey::F(7),
        Key::F8 => TerminalKey::F(8),
        Key::F9 => TerminalKey::F(9),
        Key::F10 => TerminalKey::F(10),
        Key::F11 => TerminalKey::F(11),
        Key::F12 => TerminalKey::F(12),
        _ => return None,
    };
    if modifiers.command {
        return None;
    }
    Some(KeyStroke {
        key: terminal_key,
        ctrl: modifiers.ctrl,
        alt: modifiers.alt,
        shift: modifiers.shift,
    })
}

fn key_char(key: Key) -> Option<char> {
    Some(match key {
        Key::A => 'a',
        Key::B => 'b',
        Key::C => 'c',
        Key::D => 'd',
        Key::E => 'e',
        Key::F => 'f',
        Key::G => 'g',
        Key::H => 'h',
        Key::I => 'i',
        Key::J => 'j',
        Key::K => 'k',
        Key::L => 'l',
        Key::M => 'm',
        Key::N => 'n',
        Key::O => 'o',
        Key::P => 'p',
        Key::Q => 'q',
        Key::R => 'r',
        Key::S => 's',
        Key::T => 't',
        Key::U => 'u',
        Key::V => 'v',
        Key::W => 'w',
        Key::X => 'x',
        Key::Y => 'y',
        Key::Z => 'z',
        Key::Num0 => '0',
        Key::Num1 => '1',
        Key::Num2 => '2',
        Key::Num3 => '3',
        Key::Num4 => '4',
        Key::Num5 => '5',
        Key::Num6 => '6',
        Key::Num7 => '7',
        Key::Num8 => '8',
        Key::Num9 => '9',
        Key::Space => ' ',
        Key::Colon => ':',
        Key::Comma => ',',
        Key::Backslash => '\\',
        Key::Slash => '/',
        Key::Pipe => '|',
        Key::Questionmark => '?',
        Key::Exclamationmark => '!',
        Key::OpenBracket => '[',
        Key::CloseBracket => ']',
        Key::OpenCurlyBracket => '{',
        Key::CloseCurlyBracket => '}',
        Key::Backtick => '`',
        Key::Minus => '-',
        Key::Period => '.',
        Key::Plus => '+',
        Key::Equals => '=',
        Key::Semicolon => ';',
        Key::Quote => '\'',
        _ => return None,
    })
}

fn selection_text(snapshot: &Snapshot, selection: &TermSelection) -> String {
    let ((start_line, start_col), (end_line, end_col)) = selection.normalised();
    let mut out = String::new();
    for line in start_line..=end_line {
        let row = line - snapshot.absolute_line(0);
        if row < 0 || row as usize >= snapshot.rows {
            continue;
        }
        let row = row as usize;
        let first = if line == start_line { start_col } else { 0 };
        let last = if line == end_line {
            end_col.min(snapshot.cols)
        } else {
            snapshot.cols
        };
        let mut text = String::new();
        for col in first..last {
            let cell = snapshot.cell(row, col);
            if cell.attrs & ATTR_SPACER != 0 || cell.attrs & ATTR_LEADING_SPACER != 0 {
                continue;
            }
            text.push(cell.ch);
        }
        out.push_str(text.trim_end());
        if line != end_line {
            out.push('\n');
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn send_mouse(
    session: &Session,
    snapshot: &Snapshot,
    pos: Pos2,
    inner: Rect,
    line_h: f32,
    char_w: f32,
    button: u16,
    press: bool,
    sgr: bool,
) {
    let row = ((pos.y - inner.top()) / line_h).floor().max(0.0) as usize;
    let col = ((pos.x - inner.left()) / char_w).floor().max(0.0) as usize;
    let row = row.min(snapshot.rows.saturating_sub(1)) + 1;
    let col = col.min(snapshot.cols.saturating_sub(1)) + 1;
    if sgr {
        let suffix = if press { 'M' } else { 'm' };
        let sequence = format!("\x1b[<{button};{col};{row}{suffix}");
        session.write(sequence.as_bytes());
    } else {
        // Legacy X10 encoding: ESC [ M Cb Cx Cy with 32-biased bytes.
        let bytes = [
            0x1b,
            b'[',
            b'M',
            (32 + button).min(255) as u8,
            (32 + col).min(255) as u8,
            (32 + row).min(255) as u8,
        ];
        session.write(&bytes);
    }
}

fn send_wheel(session: &Session, button: u16, sgr: bool) {
    if sgr {
        let sequence = format!("\x1b[<{button};1;1M");
        session.write(sequence.as_bytes());
    } else {
        let bytes = [0x1b, b'[', b'M', (32 + button).min(255) as u8, 33, 33];
        session.write(&bytes);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_search(
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
            .hint_text("Find in terminal")
            .font(FontId::proportional(11.0)),
    );
    if state.focus_requested {
        response.request_focus();
        state.focus_requested = false;
    }
    if response.changed() {
        state.matches = session.search(&state.query);
        state.index = 0;
        if let Some(hit) = state.matches.first() {
            session.scroll_to_line(hit.line);
        }
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        crate::theme::install_fonts(&ctx);
        let mut out = ctx.run_ui(Default::default(), |_| {});
        out.textures_delta.clear();
        let size = 13.0;
        let char_w = crate::widgets::mono_char_width(&ctx, size);
        let ascii_galley = ctx.fonts_mut(|f| {
            f.layout_no_wrap(
                "M".to_string(),
                egui::FontId::monospace(size),
                egui::Color32::WHITE,
            )
        });
        let cjk_galley = ctx.fonts_mut(|f| {
            f.layout_no_wrap(
                "你好".to_string(),
                egui::FontId::monospace(size),
                egui::Color32::WHITE,
            )
        });
        assert!(cjk_galley.size().x > char_w);
        assert!(ascii_galley.size().x > 0.0);
        assert!(char_w > 0.0);
    }
}
