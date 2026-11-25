//! Shared keyboard navigation for list surfaces (sidebar lists, the log
//! table and manager dialogs).
//!
//! Only plain Up/Down/Home/End count: modified arrows stay with shortcuts, and
//! a focused text field wins so filters/editors keep their caret keys. Main
//! window lists additionally require the pointer over the list, which is the
//! only unambiguous "this list is the target" signal without a focus stack.

use egui::{Key, Rect, Ui};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ListMove {
    Up,
    Down,
    Home,
    End,
}

/// A move for a list in the main window: needs the pointer over `area`.
pub(crate) fn hovered_move(ui: &Ui, area: Rect) -> Option<ListMove> {
    if ui.ctx().text_edit_focused() || !ui.rect_contains_pointer(area) {
        return None;
    }
    read(ui, false)
}

/// A move for a modal dialog list: the dialog owns the keyboard, so only a
/// focused text field suppresses it.
pub(crate) fn modal_move(ui: &Ui) -> Option<ListMove> {
    if ui.ctx().text_edit_focused() {
        return None;
    }
    read(ui, false)
}

/// Vertical moves for a panel whose own single-line query field owns the
/// keyboard: ↑/↓ step the results, while Home/End stay with the caret.
pub(crate) fn panel_move(ui: &Ui) -> Option<ListMove> {
    read(ui, true)
}

fn read(ui: &Ui, vertical_only: bool) -> Option<ListMove> {
    ui.input(|input| {
        let plain = !input.modifiers.command && !input.modifiers.ctrl && !input.modifiers.alt;
        if !plain {
            return None;
        }
        if input.key_pressed(Key::ArrowUp) {
            Some(ListMove::Up)
        } else if input.key_pressed(Key::ArrowDown) {
            Some(ListMove::Down)
        } else if !vertical_only && input.key_pressed(Key::Home) {
            Some(ListMove::Home)
        } else if !vertical_only && input.key_pressed(Key::End) {
            Some(ListMove::End)
        } else {
            None
        }
    })
}

/// Target row after a move. `None` means nothing was selected yet; Up and
/// Down both land on the first row, then clamp at the ends (no wrap, so the
/// list never jumps across the whole viewport).
pub(crate) fn stepped(current: Option<usize>, len: usize, mv: ListMove) -> Option<usize> {
    let last = len.checked_sub(1)?;
    Some(match mv {
        ListMove::Up => current.map_or(0, |index| index.min(last).saturating_sub(1)),
        ListMove::Down => current.map_or(0, |index| (index + 1).min(last)),
        ListMove::Home => 0,
        ListMove::End => last,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_clamp_at_both_ends() {
        assert_eq!(stepped(None, 0, ListMove::Down), None);
        assert_eq!(stepped(None, 3, ListMove::Down), Some(0));
        assert_eq!(stepped(None, 3, ListMove::Up), Some(0));
        assert_eq!(stepped(Some(0), 3, ListMove::Up), Some(0));
        assert_eq!(stepped(Some(2), 3, ListMove::Down), Some(2));
        assert_eq!(stepped(Some(1), 3, ListMove::Down), Some(2));
        assert_eq!(stepped(Some(1), 3, ListMove::Up), Some(0));
        assert_eq!(stepped(Some(1), 3, ListMove::Home), Some(0));
        assert_eq!(stepped(Some(1), 3, ListMove::End), Some(2));
    }

    #[test]
    fn stale_index_outside_the_list_is_clamped() {
        // A filter can shrink the list under a selection; the next move must
        // land on a real row, not stay out of bounds. ↑ clamps to the last row
        // and then steps above it, ↓ clamps onto the last row.
        assert_eq!(stepped(Some(9), 3, ListMove::Up), Some(1));
        assert_eq!(stepped(Some(9), 3, ListMove::Down), Some(2));
    }
}
