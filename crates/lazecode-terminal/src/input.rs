use crate::session::Modes;

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

#[derive(Clone, Copy, Debug)]
pub struct KeyStroke {
    pub key: TerminalKey,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl KeyStroke {
    pub fn plain(key: TerminalKey) -> Self {
        Self {
            key,
            ctrl: false,
            alt: false,
            shift: false,
        }
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

/// Encode a key press for the PTY. Command (meta) combinations are handled by
/// the application shell and never reach the terminal.
pub fn encode_key(stroke: KeyStroke, modes: &Modes) -> Option<Vec<u8>> {
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

/// Paste encoding honours DEC bracketed paste so shells do not execute the
/// pasted text. Control bytes are neutralised so a pasted `ESC[201~` cannot
/// terminate the paste state machine early and inject keystrokes.
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

    #[test]
    fn ctrl_c_is_etx() {
        let bytes = encode_key(
            KeyStroke {
                key: TerminalKey::Char('c'),
                ctrl: true,
                alt: false,
                shift: false,
            },
            &modes(),
        )
        .unwrap();
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
        let stroke = KeyStroke {
            key: TerminalKey::Left,
            ctrl: true,
            alt: false,
            shift: false,
        };
        assert_eq!(encode_key(stroke, &modes()).unwrap(), b"\x1b[1;5D".to_vec());
        let stroke = KeyStroke {
            key: TerminalKey::Up,
            ctrl: false,
            alt: false,
            shift: true,
        };
        assert_eq!(encode_key(stroke, &modes()).unwrap(), b"\x1b[1;2A".to_vec());
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
        let stroke = KeyStroke {
            key: TerminalKey::Char('a'),
            ctrl: true,
            alt: true,
            shift: false,
        };
        assert_eq!(encode_key(stroke, &modes()).unwrap(), vec![0x1b, 0x01]);
    }
}
