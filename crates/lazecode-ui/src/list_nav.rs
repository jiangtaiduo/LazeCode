//! Shared keyboard navigation for list surfaces (sidebar lists, the log
//! table and manager dialogs).
//!
//! Only plain Up/Down/Home/End count: modified arrows stay with shortcuts, and
//! a focused text field wins so filters/editors keep their caret keys. Main
//! window lists additionally require the pointer over the list, which is the
//! only unambiguous "this list is the target" signal without a focus stack.

use egui::{Key, Rect, Ui};
use lazecode_core::AppModel;

use crate::runtime::UiRuntime;

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
/// `own_input_focused` must be the panel's own field, not just any text edit.
pub(crate) fn panel_move(ui: &Ui, own_input_focused: bool) -> Option<ListMove> {
    if !own_input_focused {
        return None;
    }
    read(ui, true)
}

/// A main-window list only answers arrows when no overlay owns input and no
/// editor/console text surface is the focused pane.
pub(crate) fn keys_free(ui: &Ui, model: &AppModel, rt: &UiRuntime) -> bool {
    !rt.blocks_input(model) && !rt.typing_surface_owns_keys(model, ui.ctx())
}

/// Scroll offset that centers row `row` in a virtualized list of `row_h`-high
/// rows inside a `viewport_h`-high viewport.
pub(crate) fn centered_scroll(row: usize, row_h: f32, viewport_h: f32) -> f32 {
    (row as f32 * row_h - viewport_h * 0.5 + row_h * 0.5).max(0.0)
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
/// list never jumps across the whole viewport). A selection index left behind
/// by a filter/delete is re-anchored to the last row before stepping.
pub(crate) fn stepped(current: Option<usize>, len: usize, mv: ListMove) -> Option<usize> {
    let last = len.checked_sub(1)?;
    let stale = current.is_some_and(|index| index > last);
    let clamped = current.map(|index| index.min(last));
    Some(match mv {
        // No row corresponds to a stale index: re-anchor instead of stepping
        // past a row the user never saw.
        ListMove::Up if stale => last,
        ListMove::Up => clamped.map_or(0, |index| index.saturating_sub(1)),
        ListMove::Down => clamped.map_or(0, |index| (index + 1).min(last)),
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
        // A filter can shrink the list under a selection; the first arrow must
        // re-anchor on a real row. The next Up then steps from that row.
        assert_eq!(stepped(Some(9), 3, ListMove::Up), Some(2));
        assert_eq!(stepped(Some(9), 3, ListMove::Down), Some(2));
        assert_eq!(stepped(Some(2), 3, ListMove::Up), Some(1));
    }

    #[test]
    fn centered_scroll_clamps_at_the_top() {
        assert_eq!(centered_scroll(0, 21.0, 42.0), 0.0);
        assert_eq!(centered_scroll(2, 21.0, 42.0), 31.5);
    }
}
