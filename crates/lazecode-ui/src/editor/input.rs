//! Keyboard and pointer input handling: editor keys, completion and
//! symbol events, and word-at-cursor.

use super::prelude::*;

pub(super) fn handle_editor_input(
    ui: &mut Ui,
    doc: &mut Document,
    pending: &mut Vec<Command>,
    tab_id: u64,
    rt: &mut UiRuntime,
) {
    let events = ui.input(|input| input.events.clone());
    for event in events {
        // Read-only snapshots (repository version tabs) allow navigation and
        // copy but no mutation, so the buffer never diverges from the blob.
        if doc.read_only {
            match &event {
                Event::Copy => {
                    if let Some(text) = doc.selected_text() {
                        ui.ctx().copy_text(text);
                    }
                }
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let navigates = matches!(
                        key,
                        Key::ArrowUp
                            | Key::ArrowDown
                            | Key::ArrowLeft
                            | Key::ArrowRight
                            | Key::Home
                            | Key::End
                            | Key::PageUp
                            | Key::PageDown
                    ) || (modifiers.command && matches!(key, Key::A | Key::C));
                    if navigates {
                        let _ = handle_editor_key(ui, doc, pending, tab_id, rt, *key, *modifiers);
                    }
                }
                _ => {}
            }
            continue;
        }
        if handle_completion_event(doc, rt, tab_id, &event) {
            continue;
        }
        if handle_symbol_event(doc, rt, tab_id, &event) {
            continue;
        }
        match event {
            Event::Text(text) => {
                doc.insert_text(&text);
                if crate::lsp_ui::is_completion_trigger(&text)
                    && lazecode_core::language_id_for_path(&doc.path).is_some()
                {
                    rt.lsp.completion_due = Some((tab_id, std::time::Instant::now()));
                }
                if rt.lsp.completion_due.is_some() {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(160));
                }
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
            } if handle_editor_key(ui, doc, pending, tab_id, rt, key, modifiers) => {
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

/// Keys while the completion popup is open; `true` consumes the event.
pub(super) fn handle_completion_event(
    doc: &mut Document,
    rt: &mut UiRuntime,
    tab_id: u64,
    event: &Event,
) -> bool {
    let Some(mut state) = rt.lsp.completion.take() else {
        return false;
    };
    if state.tab_id != tab_id {
        rt.lsp.completion = Some(state);
        return false;
    }
    let mut keep = true;
    let mut consumed = false;
    match event {
        Event::Key {
            key, pressed: true, ..
        } => match key {
            Key::ArrowDown => {
                if !state.items.is_empty() {
                    state.selected = (state.selected + 1) % state.items.len();
                }
                consumed = true;
            }
            Key::ArrowUp => {
                if !state.items.is_empty() {
                    state.selected = if state.selected == 0 {
                        state.items.len() - 1
                    } else {
                        state.selected - 1
                    };
                }
                consumed = true;
            }
            Key::Enter | Key::Tab => {
                crate::lsp_ui::accept_completion(doc, &state);
                keep = false;
                consumed = true;
            }
            Key::Escape => {
                keep = false;
                consumed = true;
            }
            Key::ArrowLeft | Key::ArrowRight | Key::Delete => {
                keep = false;
            }
            Key::Backspace => {
                keep = false;
                rt.lsp.completion_due = Some((tab_id, std::time::Instant::now()));
            }
            _ => {}
        },
        Event::Text(text) => {
            keep = false;
            // Keep filtering: re-request after the character is inserted.
            if text.chars().all(|ch| ch.is_alphanumeric() || ch == '_') {
                rt.lsp.completion_due = Some((tab_id, std::time::Instant::now()));
            }
        }
        Event::Paste(_) => keep = false,
        _ => {}
    }
    if keep {
        rt.lsp.completion = Some(state);
    }
    consumed
}

/// The symbol picker is modal-ish: it swallows typing so a stray character
/// cannot edit the buffer behind it.
pub(super) fn handle_symbol_event(
    doc: &mut Document,
    rt: &mut UiRuntime,
    tab_id: u64,
    event: &Event,
) -> bool {
    let Some(mut state) = rt.lsp.symbols.take() else {
        return false;
    };
    if state.tab_id != tab_id {
        rt.lsp.symbols = Some(state);
        return false;
    }
    let mut keep = true;
    match event {
        Event::Key {
            key, pressed: true, ..
        } => match key {
            Key::ArrowDown => {
                if !state.items.is_empty() {
                    state.selected = (state.selected + 1) % state.items.len();
                }
            }
            Key::ArrowUp => {
                if !state.items.is_empty() {
                    state.selected = if state.selected == 0 {
                        state.items.len() - 1
                    } else {
                        state.selected - 1
                    };
                }
            }
            Key::Enter | Key::Tab => {
                crate::lsp_ui::accept_symbol(doc, &state);
                keep = false;
            }
            Key::Escape => keep = false,
            _ => {}
        },
        Event::Text(_) | Event::Paste(_) => keep = false,
        _ => {
            rt.lsp.symbols = Some(state);
            return false;
        }
    }
    if keep {
        rt.lsp.symbols = Some(state);
    }
    true
}

pub(crate) fn word_at(doc: &Document, cursor: Cursor) -> String {
    let line = doc.line_content(cursor.line);
    let chars: Vec<char> = line.chars().collect();
    let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
    let mut start = cursor.col.min(chars.len());
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let mut end = cursor.col.min(chars.len());
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    chars[start..end].iter().collect()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_editor_key(
    ui: &mut Ui,
    doc: &mut Document,
    pending: &mut Vec<Command>,
    tab_id: u64,
    rt: &mut UiRuntime,
    key: Key,
    modifiers: Modifiers,
) -> bool {
    let cmd = modifiers.command;
    if cmd && modifiers.alt && key == Key::L {
        rt.lsp.format_request = Some(tab_id);
        return true;
    }
    if cmd && modifiers.alt && key == Key::O {
        rt.lsp.symbols_request = Some(tab_id);
        return true;
    }
    if modifiers.ctrl && key == Key::Space {
        rt.lsp.completion_due = Some((tab_id, std::time::Instant::now()));
        return true;
    }
    if key == Key::F12 || (cmd && !modifiers.shift && !modifiers.alt && key == Key::B) {
        rt.lsp.goto_request = Some((tab_id, doc.cursor));
        return true;
    }
    if key == Key::F2 {
        let word = word_at(doc, doc.cursor);
        rt.dialog = DialogState::RenameSymbol {
            tab_id,
            path: doc.path.to_string_lossy().to_string(),
            line: doc.cursor.line,
            col: doc.cursor.col,
            value: word,
            error: None,
        };
        return true;
    }
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
