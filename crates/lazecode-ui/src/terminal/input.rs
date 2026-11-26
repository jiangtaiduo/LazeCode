//! Keyboard/mouse input: key strokes, selection text, mouse and wheel reports.

use super::prelude::*;

pub(super) fn handle_input(
    ui: &mut Ui,
    session: &Session,
    snapshot: &Snapshot,
    preedit: &mut String,
    _id: u64,
    selection: Option<&TermSelection>,
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
                pressed,
                repeat,
                modifiers,
                ..
            } => {
                if let Some(mut stroke) = key_to_stroke(key, modifiers) {
                    stroke.event = match (pressed, repeat) {
                        (true, true) => KeyEvent::Repeat,
                        (true, false) => KeyEvent::Press,
                        (false, _) => KeyEvent::Release,
                    };
                    if let Some(encoded) = encode_stroke_for(stroke, snapshot) {
                        bytes.extend_from_slice(&encoded);
                    }
                }
            }
            Event::Copy => {
                // egui-winit maps Cmd+C to `Event::Copy` instead of a
                // `Key::C` event, so copying must happen here.
                if let Some(sel) = selection {
                    let text = selection_text(snapshot, sel);
                    if !text.is_empty() {
                        ui.ctx().copy_text(text);
                    }
                }
            }
            _ => {}
        }
    }
    if !bytes.is_empty() {
        session.write(&bytes);
    }
}

pub(super) fn encode_stroke_for(
    stroke: lazecode_terminal::KeyStroke,
    snapshot: &Snapshot,
) -> Option<Vec<u8>> {
    lazecode_terminal::encode_key(stroke, &snapshot.modes)
}

pub(super) fn key_to_stroke(
    key: Key,
    modifiers: Modifiers,
) -> Option<lazecode_terminal::KeyStroke> {
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
                event: KeyEvent::Press,
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
        event: KeyEvent::Press,
    })
}

pub(super) fn key_char(key: Key) -> Option<char> {
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

pub(super) fn clear_selection_on_click(selection: &mut Option<TermSelection>, clicked: bool) {
    if clicked {
        *selection = None;
    }
}

/// A press anchor is not a selection until the pointer moves to another cell.
/// This keeps a plain click from painting a one-cell block.
pub(super) fn selection_bounds(
    selection: Option<&TermSelection>,
) -> Option<((i32, usize), (i32, usize))> {
    let (start, end) = selection?.normalised();
    (start != end).then_some((start, end))
}

pub(super) fn selection_text(snapshot: &Snapshot, selection: &TermSelection) -> String {
    let Some(((start_line, start_col), (end_line, end_col))) = selection_bounds(Some(selection))
    else {
        return String::new();
    };
    let mut out = String::new();
    for line in start_line..=end_line {
        let row = line - snapshot.absolute_line(0);
        if row < 0 || row as usize >= snapshot.rows {
            continue;
        }
        let row = row as usize;
        let first = if line == start_line { start_col } else { 0 };
        let last = if line == end_line {
            (end_col + 1).min(snapshot.cols)
        } else {
            snapshot.cols
        };
        let mut text = String::new();
        for col in first..last {
            let cell = snapshot.cell(row, col);
            if cell.attrs & ATTR_SPACER != 0 || cell.attrs & ATTR_LEADING_SPACER != 0 {
                continue;
            }
            if let Some(cluster) = &cell.text {
                text.push_str(cluster);
            } else if cell.ch != '\0' {
                text.push(cell.ch);
            }
        }
        out.push_str(text.trim_end());
        if line != end_line {
            out.push('\n');
        }
    }
    out
}

/// X10 reports encode each coordinate as a 32-biased byte, so 223 is the
/// largest representable 1-based column/row. Clamping wider coordinates to
/// 255 would silently report the wrong cell, so skip the report instead;
/// SGR reports have no such limit and are still sent unchanged.
fn x10_report(button: u16, col: usize, row: usize, press: bool) -> Option<[u8; 6]> {
    const MAX_X10: usize = u8::MAX as usize - 32;
    if col > MAX_X10 || row > MAX_X10 || button as usize > MAX_X10 {
        return None;
    }
    // A release is button 3 regardless of which button was held.
    let legacy_button = if press { button } else { 3 };
    Some([
        0x1b,
        b'[',
        b'M',
        (32 + legacy_button) as u8,
        (32 + col) as u8,
        (32 + row) as u8,
    ])
}

#[allow(clippy::too_many_arguments)]
pub(super) fn send_mouse(
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
    } else if let Some(bytes) = x10_report(button, col, row, press) {
        session.write(&bytes);
    }
}

pub(super) fn send_wheel(
    session: &Session,
    snapshot: &Snapshot,
    inner: &Rect,
    line_h: f32,
    char_w: f32,
    pos: Pos2,
    button: u16,
) {
    let row = (((pos.y - inner.top()) / line_h).floor().max(0.0) as usize)
        .min(snapshot.rows.saturating_sub(1))
        + 1;
    let col = (((pos.x - inner.left()) / char_w).floor().max(0.0) as usize)
        .min(snapshot.cols.saturating_sub(1))
        + 1;
    if snapshot.modes.sgr_mouse {
        let sequence = format!("\x1b[<{button};{col};{row}M");
        session.write(sequence.as_bytes());
    } else if let Some(bytes) = x10_report(button, col, row, true) {
        session.write(&bytes);
    }
}

pub(super) fn pos_or(pos: Option<Pos2>, inner: Rect) -> Pos2 {
    pos.unwrap_or_else(|| inner.center())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x10_reports_stop_at_the_largest_representable_coordinate() {
        // Column 223 is the last cell an X10 byte can express (32 + 223 =
        // 255). Column 224 must not be reported as a saturated 255, which
        // programs would decode as column 223.
        assert_eq!(
            x10_report(0, 223, 10, true),
            Some([0x1b, b'[', b'M', 32, 255, 42])
        );
        assert_eq!(x10_report(0, 224, 10, true), None);
        assert_eq!(x10_report(0, 10, 224, true), None);
        assert_eq!(x10_report(224, 10, 10, true), None);
    }

    #[test]
    fn x10_release_uses_button_three_and_press_keeps_the_button() {
        assert_eq!(
            x10_report(2, 5, 7, false),
            Some([0x1b, b'[', b'M', 35, 37, 39])
        );
        assert_eq!(
            x10_report(2, 5, 7, true),
            Some([0x1b, b'[', b'M', 34, 37, 39])
        );
    }
}
