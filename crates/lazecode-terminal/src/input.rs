use crate::session::{KittyKeyboard, Modes};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalKey {
    Char(char),
    Enter,
    Tab,
    BackTab,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    Insert,
    F(u8),
}

/// Key event kind. Terminals could only report presses until the kitty
/// keyboard protocol (and win32-input-mode) added repeat/release reporting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeyEvent {
    #[default]
    Press,
    Repeat,
    Release,
}

#[derive(Clone, Copy, Debug)]
pub struct KeyStroke {
    pub key: TerminalKey,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub event: KeyEvent,
}

impl KeyStroke {
    pub fn plain(key: TerminalKey) -> Self {
        Self {
            key,
            ctrl: false,
            alt: false,
            shift: false,
            event: KeyEvent::Press,
        }
    }

    pub fn with_event(mut self, event: KeyEvent) -> Self {
        self.event = event;
        self
    }
}

fn modifier_code(stroke: KeyStroke) -> u8 {
    1 + u8::from(stroke.shift) + 2 * u8::from(stroke.alt) + 4 * u8::from(stroke.ctrl)
}

fn cursor_seq(final_byte: char, modes: &Modes, stroke: KeyStroke) -> String {
    let modifier = modifier_code(stroke);
    if modifier != 1 {
        format!("\x1b[1;{modifier}{final_byte}")
    } else if modes.app_cursor {
        format!("\x1bO{final_byte}")
    } else {
        format!("\x1b[{final_byte}")
    }
}

fn tilde_seq(number: u8, stroke: KeyStroke) -> String {
    let modifier = modifier_code(stroke);
    if modifier == 1 {
        format!("\x1b[{number}~")
    } else {
        format!("\x1b[{number};{modifier}~")
    }
}

fn function_seq(final_byte: char, stroke: KeyStroke) -> String {
    let modifier = modifier_code(stroke);
    if modifier == 1 {
        format!("\x1bO{final_byte}")
    } else {
        format!("\x1b[1;{modifier}{final_byte}")
    }
}

/// Kitty functional key codes (the spec's `57344` private-use block).
fn kitty_key_code(key: TerminalKey) -> Option<u32> {
    Some(match key {
        TerminalKey::Char(ch) => ch as u32,
        TerminalKey::Enter => 13,
        TerminalKey::Tab | TerminalKey::BackTab => 9,
        TerminalKey::Backspace => 127,
        TerminalKey::Escape => 27,
        TerminalKey::Insert => 57348,
        TerminalKey::Delete => 57349,
        TerminalKey::Left => 57350,
        TerminalKey::Right => 57351,
        TerminalKey::Up => 57352,
        TerminalKey::Down => 57353,
        TerminalKey::PageUp => 57354,
        TerminalKey::PageDown => 57355,
        TerminalKey::Home => 57356,
        TerminalKey::End => 57357,
        TerminalKey::F(number) if (1..=35).contains(&number) => 57363 + u32::from(number),
        TerminalKey::F(_) => return None,
    })
}

/// Does this stroke need the kitty `CSI … u` form instead of the legacy bytes?
fn uses_kitty(stroke: KeyStroke, flags: KittyKeyboard) -> bool {
    if !flags.is_active() {
        return false;
    }
    if flags.contains(KittyKeyboard::REPORT_ALL_KEYS_AS_ESC) {
        return true;
    }
    if stroke.event != KeyEvent::Press && flags.contains(KittyKeyboard::REPORT_EVENT_TYPES) {
        return true;
    }
    if stroke.ctrl || stroke.alt {
        return true;
    }
    // Shift makes the legacy encoding of the control keys ambiguous
    // (`\r` for Enter is the same with and without shift).
    stroke.shift
        && matches!(
            stroke.key,
            TerminalKey::Enter
                | TerminalKey::Tab
                | TerminalKey::BackTab
                | TerminalKey::Backspace
                | TerminalKey::Escape
        )
}

fn kitty_event_code(event: KeyEvent) -> char {
    match event {
        KeyEvent::Press => '1',
        KeyEvent::Repeat => '2',
        KeyEvent::Release => '3',
    }
}

/// Text produced by the key, as kitty's `REPORT_ASSOCIATED_TEXT` codepoints.
fn associated_text(stroke: KeyStroke) -> Option<u32> {
    let TerminalKey::Char(ch) = stroke.key else {
        return None;
    };
    if stroke.ctrl || stroke.alt {
        return None;
    }
    let ch = if stroke.shift && ch.is_ascii_alphabetic() {
        ch.to_ascii_uppercase()
    } else {
        ch
    };
    Some(ch as u32)
}

fn kitty_sequence(stroke: KeyStroke, flags: KittyKeyboard) -> Option<String> {
    let code = kitty_key_code(stroke.key)?;
    let modifiers = modifier_code(stroke);
    let event = flags
        .contains(KittyKeyboard::REPORT_EVENT_TYPES)
        .then_some(kitty_event_code(stroke.event));
    let text = if stroke.event == KeyEvent::Release {
        None
    } else if flags.contains(KittyKeyboard::REPORT_ASSOCIATED_TEXT) {
        associated_text(stroke)
    } else {
        None
    };
    let mut out = format!("\x1b[{code}");
    if modifiers != 1 || event.is_some() || text.is_some() {
        out.push(';');
        out.push_str(&modifiers.to_string());
        if let Some(event) = event {
            out.push(':');
            out.push(event);
        }
    }
    if let Some(text) = text {
        out.push(';');
        out.push_str(&text.to_string());
    }
    out.push('u');
    Some(out)
}

/// Does this key already have an unambiguous legacy control byte?
fn legacy_control_code(ch: char) -> bool {
    matches!(
        ch.to_ascii_lowercase(),
        'a'..='z' | '[' | '\\' | ']' | '^' | '_' | '@' | ' ' | '?'
    )
}

fn modify_other_keys_code(key: TerminalKey) -> Option<u32> {
    Some(match key {
        TerminalKey::Char(ch) => ch as u32,
        TerminalKey::Enter => 13,
        TerminalKey::Tab | TerminalKey::BackTab => 9,
        TerminalKey::Backspace => 127,
        TerminalKey::Escape => 27,
        _ => return None,
    })
}

/// xterm `modifyOtherKeys` encoding: `CSI 27 ; modifier ; key ~`.
fn modify_keys_sequence(stroke: KeyStroke, level: u8) -> Option<String> {
    if level == 0 || stroke.event != KeyEvent::Press {
        return None;
    }
    let modified = stroke.ctrl || stroke.alt || stroke.shift;
    if !modified {
        return None;
    }
    let apply = match level {
        // Level 1 only disambiguates keys whose legacy encoding would lose
        // information: the control keys, and ctrl+keys with no control byte.
        1 => match stroke.key {
            TerminalKey::Enter
            | TerminalKey::Tab
            | TerminalKey::BackTab
            | TerminalKey::Backspace
            | TerminalKey::Escape => true,
            TerminalKey::Char(ch) => stroke.ctrl && !legacy_control_code(ch),
            _ => false,
        },
        // Level 2 reports every modified key.
        _ => true,
    };
    if !apply {
        return None;
    }
    let code = modify_other_keys_code(stroke.key)?;
    Some(format!("\x1b[27;{};{code}~", modifier_code(stroke)))
}

/// Encode a key press for the PTY (kitty protocol when enabled, else legacy
/// xterm bytes). Command/meta combos never reach the terminal.
pub fn encode_key(stroke: KeyStroke, modes: &Modes) -> Option<Vec<u8>> {
    if uses_kitty(stroke, modes.kitty) {
        return kitty_sequence(stroke, modes.kitty).map(String::into_bytes);
    }
    // A legacy terminal cannot report *releases*; a repeat is still just
    // another press, so holding Backspace/arrows must keep producing input.
    if stroke.event == KeyEvent::Release {
        return None;
    }
    if let Some(sequence) = modify_keys_sequence(stroke, modes.modify_other_keys) {
        return Some(sequence.into_bytes());
    }
    let mut out = String::new();
    match stroke.key {
        TerminalKey::Char(ch) => {
            if stroke.ctrl {
                let lower = ch.to_ascii_lowercase();
                let code = match lower {
                    'a'..='z' => Some((lower as u8) - b'a' + 1),
                    '[' => Some(27),
                    '\\' => Some(28),
                    ']' => Some(29),
                    '^' => Some(30),
                    '_' => Some(31),
                    '@' | ' ' => Some(0),
                    '?' => Some(127),
                    _ => None,
                };
                if let Some(code) = code {
                    if stroke.alt {
                        out.push('\x1b');
                    }
                    out.push(code as char);
                } else if stroke.alt {
                    out.push('\x1b');
                    out.push(ch);
                } else {
                    out.push(ch);
                }
            } else if stroke.alt {
                out.push('\x1b');
                out.push(ch);
            } else {
                out.push(ch);
            }
        }
        TerminalKey::Enter => {
            if stroke.alt {
                out.push('\x1b');
            }
            out.push('\r');
        }
        TerminalKey::Tab => {
            if stroke.shift {
                out.push_str("\x1b[Z");
            } else {
                out.push('\t');
            }
        }
        TerminalKey::BackTab => out.push_str("\x1b[Z"),
        TerminalKey::Backspace => {
            if stroke.alt {
                out.push('\x1b');
            }
            out.push('\x7f');
        }
        TerminalKey::Escape => {
            if stroke.alt {
                out.push('\x1b');
            }
            out.push('\x1b');
        }
        TerminalKey::Up => out.push_str(&cursor_seq('A', modes, stroke)),
        TerminalKey::Down => out.push_str(&cursor_seq('B', modes, stroke)),
        TerminalKey::Right => out.push_str(&cursor_seq('C', modes, stroke)),
        TerminalKey::Left => out.push_str(&cursor_seq('D', modes, stroke)),
        TerminalKey::Home => out.push_str(&cursor_seq('H', modes, stroke)),
        TerminalKey::End => out.push_str(&cursor_seq('F', modes, stroke)),
        TerminalKey::PageUp => out.push_str(&tilde_seq(5, stroke)),
        TerminalKey::PageDown => out.push_str(&tilde_seq(6, stroke)),
        TerminalKey::Insert => out.push_str(&tilde_seq(2, stroke)),
        TerminalKey::Delete => out.push_str(&tilde_seq(3, stroke)),
        TerminalKey::F(n) => match n {
            1 => out.push_str(&function_seq('P', stroke)),
            2 => out.push_str(&function_seq('Q', stroke)),
            3 => out.push_str(&function_seq('R', stroke)),
            4 => out.push_str(&function_seq('S', stroke)),
            5 => out.push_str(&tilde_seq(15, stroke)),
            6 => out.push_str(&tilde_seq(17, stroke)),
            7 => out.push_str(&tilde_seq(18, stroke)),
            8 => out.push_str(&tilde_seq(19, stroke)),
            9 => out.push_str(&tilde_seq(20, stroke)),
            10 => out.push_str(&tilde_seq(21, stroke)),
            11 => out.push_str(&tilde_seq(23, stroke)),
            12 => out.push_str(&tilde_seq(24, stroke)),
            _ => return None,
        },
    }
    if out.is_empty() {
        None
    } else {
        Some(out.into_bytes())
    }
}

/// Paste encoding honours DEC bracketed paste; control bytes are neutralised so
/// a pasted `ESC[201~` cannot end the paste and inject keys.
pub fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    let text = sanitize_paste(text, bracketed);
    let mut out = String::with_capacity(text.len() + 16);
    if bracketed {
        out.push_str("\x1b[200~");
        out.push_str(&text);
        out.push_str("\x1b[201~");
    } else {
        out.push_str(&text);
    }
    out.into_bytes()
}

fn sanitize_paste(text: &str, bracketed: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\n' if !bracketed => out.push('\r'),
            '\n' | '\r' | '\t' => out.push(ch),
            ch if ch.is_control() => out.push(' '),
            ch => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modes() -> Modes {
        Modes::default()
    }

    fn kitty(flags: KittyKeyboard) -> Modes {
        Modes {
            kitty: flags,
            kitty_keyboard: flags.is_active(),
            ..Modes::default()
        }
    }

    fn stroke(key: TerminalKey, ctrl: bool, alt: bool, shift: bool) -> KeyStroke {
        KeyStroke {
            key,
            ctrl,
            alt,
            shift,
            event: KeyEvent::Press,
        }
    }

    fn encoded(stroke: KeyStroke, modes: &Modes) -> String {
        String::from_utf8(encode_key(stroke, modes).expect("encoded")).expect("utf8")
    }

    #[test]
    fn ctrl_c_is_etx() {
        let bytes =
            encode_key(stroke(TerminalKey::Char('c'), true, false, false), &modes()).unwrap();
        assert_eq!(bytes, vec![3]);
    }

    #[test]
    fn arrows_follow_application_mode() {
        let mut modes = modes();
        assert_eq!(
            encode_key(KeyStroke::plain(TerminalKey::Up), &modes).unwrap(),
            b"\x1b[A"
        );
        modes.app_cursor = true;
        assert_eq!(
            encode_key(KeyStroke::plain(TerminalKey::Up), &modes).unwrap(),
            b"\x1bOA"
        );
    }

    #[test]
    fn navigation_keys_carry_xterm_modifiers() {
        assert_eq!(
            encode_key(stroke(TerminalKey::Left, true, false, false), &modes()).unwrap(),
            b"\x1b[1;5D".to_vec()
        );
        assert_eq!(
            encode_key(stroke(TerminalKey::Up, false, false, true), &modes()).unwrap(),
            b"\x1b[1;2A".to_vec()
        );
    }

    #[test]
    fn bracketed_paste() {
        assert_eq!(encode_paste("ls", true), b"\x1b[200~ls\x1b[201~");
    }

    #[test]
    fn bracket_paste_cannot_be_terminated_by_pasted_escape() {
        let encoded = String::from_utf8(encode_paste("a\x1b[201~rm -rf /", true)).unwrap();
        assert_eq!(encoded.matches("\x1b[201~").count(), 1);
        assert!(encoded.starts_with("\x1b[200~"));
        assert!(encoded.ends_with("\x1b[201~"));
        assert!(!encoded.contains("a\x1b[201~"));
    }

    #[test]
    fn plain_paste_normalises_newlines_and_control_bytes() {
        let encoded = String::from_utf8(encode_paste("a\nb\x00c\x07", false)).unwrap();
        assert_eq!(encoded, "a\rb c ");
    }

    #[test]
    fn ctrl_alt_letter_keeps_the_meta_prefix() {
        assert_eq!(
            encode_key(stroke(TerminalKey::Char('a'), true, true, false), &modes()).unwrap(),
            vec![0x1b, 0x01]
        );
    }

    #[test]
    fn kitty_disambiguates_modified_keys_only() {
        let modes = kitty(KittyKeyboard::DISAMBIGUATE_ESC_CODES);
        // Plain text keeps the legacy encoding.
        assert_eq!(
            encoded(stroke(TerminalKey::Char('a'), false, false, false), &modes),
            "a"
        );
        assert_eq!(
            encoded(stroke(TerminalKey::Char('a'), true, false, false), &modes),
            "\x1b[97;5u"
        );
        assert_eq!(
            encoded(stroke(TerminalKey::Char('a'), false, true, false), &modes),
            "\x1b[97;3u"
        );
        assert_eq!(
            encoded(stroke(TerminalKey::Char('a'), true, true, true), &modes),
            "\x1b[97;8u"
        );
        // Shift+Enter is ambiguous in legacy mode.
        assert_eq!(
            encoded(stroke(TerminalKey::Enter, false, false, true), &modes),
            "\x1b[13;2u"
        );
        // Plain Enter still uses the control byte.
        assert_eq!(
            encoded(stroke(TerminalKey::Enter, false, false, false), &modes),
            "\r"
        );
        // Arrows with control become functional codepoints.
        assert_eq!(
            encoded(stroke(TerminalKey::Up, true, false, false), &modes),
            "\x1b[57352;5u"
        );
        assert_eq!(
            encoded(stroke(TerminalKey::F(1), false, false, false), &modes),
            "\x1bOP"
        );
    }

    #[test]
    fn legacy_repeat_keeps_producing_bytes() {
        let modes = modes();
        let press = KeyStroke::plain(TerminalKey::Backspace);
        assert_eq!(
            encode_key(press.with_event(KeyEvent::Repeat), &modes).unwrap(),
            b"\x7f"
        );
        assert!(encode_key(press.with_event(KeyEvent::Release), &modes).is_none());
    }

    #[test]
    fn kitty_reports_events_and_releases() {
        let flags = KittyKeyboard::DISAMBIGUATE_ESC_CODES | KittyKeyboard::REPORT_EVENT_TYPES;
        let modes = kitty(flags);
        let press = stroke(TerminalKey::Char('c'), true, false, false);
        assert_eq!(encoded(press, &modes), "\x1b[99;5:1u");
        assert_eq!(
            encoded(press.with_event(KeyEvent::Repeat), &modes),
            "\x1b[99;5:2u"
        );
        assert_eq!(
            encoded(press.with_event(KeyEvent::Release), &modes),
            "\x1b[99;5:3u"
        );
        // A released unmodified key is still reported (that is the point of
        // the event-types flag), with the default modifier parameter.
        assert_eq!(
            encoded(
                stroke(TerminalKey::Char('a'), false, false, false).with_event(KeyEvent::Release),
                &modes
            ),
            "\x1b[97;1:3u"
        );
    }

    #[test]
    fn kitty_release_without_event_flag_stays_legacy_silent() {
        let modes = kitty(KittyKeyboard::DISAMBIGUATE_ESC_CODES);
        assert!(encode_key(
            stroke(TerminalKey::Char('a'), false, false, false).with_event(KeyEvent::Release),
            &modes
        )
        .is_none());
        assert!(encode_key(
            stroke(TerminalKey::Up, false, false, false).with_event(KeyEvent::Release),
            &modes
        )
        .is_none());
        // A ctrl-modified release is disambiguated even without flag 2.
        assert_eq!(
            encoded(
                stroke(TerminalKey::Char('a'), true, false, false).with_event(KeyEvent::Release),
                &modes
            ),
            "\x1b[97;5u"
        );
    }

    #[test]
    fn kitty_all_keys_and_associated_text() {
        let modes =
            kitty(KittyKeyboard::REPORT_ALL_KEYS_AS_ESC | KittyKeyboard::REPORT_ASSOCIATED_TEXT);
        assert_eq!(
            encoded(stroke(TerminalKey::Char('a'), false, false, false), &modes),
            "\x1b[97;1;97u"
        );
        assert_eq!(
            encoded(stroke(TerminalKey::Char('a'), false, false, true), &modes),
            "\x1b[97;2;65u"
        );
        assert_eq!(
            encoded(stroke(TerminalKey::F(5), false, false, false), &modes),
            "\x1b[57368u"
        );
    }

    #[test]
    fn modify_other_keys_level_two_reports_every_modified_key() {
        let mut modes = modes();
        modes.modify_other_keys = 2;
        assert_eq!(
            encoded(stroke(TerminalKey::Enter, true, false, false), &modes),
            "\x1b[27;5;13~"
        );
        assert_eq!(
            encoded(stroke(TerminalKey::Tab, false, false, true), &modes),
            "\x1b[27;2;9~"
        );
        assert_eq!(
            encoded(stroke(TerminalKey::Char('a'), true, false, false), &modes),
            "\x1b[27;5;97~"
        );
        // Unmodified keys are untouched.
        assert_eq!(
            encoded(stroke(TerminalKey::Char('a'), false, false, false), &modes),
            "a"
        );
    }

    #[test]
    fn modify_other_keys_level_one_only_disambiguates() {
        let mut modes = modes();
        modes.modify_other_keys = 1;
        // ctrl+a already has a control byte: leave it alone.
        assert_eq!(
            encode_key(stroke(TerminalKey::Char('a'), true, false, false), &modes).unwrap(),
            vec![1]
        );
        // ctrl+enter has no legacy byte.
        assert_eq!(
            encoded(stroke(TerminalKey::Enter, true, false, false), &modes),
            "\x1b[27;5;13~"
        );
        // Plain alt+letter keeps its ESC prefix.
        assert_eq!(
            encode_key(stroke(TerminalKey::Char('b'), false, true, false), &modes).unwrap(),
            b"\x1bb".to_vec()
        );
    }

    #[test]
    fn kitty_protocol_wins_over_modify_other_keys() {
        let mut modes = kitty(KittyKeyboard::DISAMBIGUATE_ESC_CODES);
        modes.modify_other_keys = 2;
        assert_eq!(
            encoded(stroke(TerminalKey::Enter, true, false, false), &modes),
            "\x1b[13;5u"
        );
    }

    #[test]
    fn disabling_the_protocol_restores_legacy_bytes() {
        let mut modes = kitty(KittyKeyboard::DISAMBIGUATE_ESC_CODES);
        modes.kitty = KittyKeyboard::default();
        assert_eq!(
            encode_key(stroke(TerminalKey::Char('c'), true, false, false), &modes).unwrap(),
            vec![3]
        );
        assert_eq!(
            encode_key(KeyStroke::plain(TerminalKey::Up), &modes).unwrap(),
            b"\x1b[A"
        );
    }
}
