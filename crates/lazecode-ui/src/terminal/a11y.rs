//! Accessibility mirror of the visible viewport.

use super::input::selection_text;
use super::prelude::*;

/// Bounded accessibility mirror of the visible viewport.
pub struct A11yMirror {
    /// Accessible name: geometry, cursor and scroll/focus state.
    pub label: String,
    /// Visible grid text, one line per row, trailing blanks trimmed.
    pub text: String,
    /// Selected text (empty when there is no selection).
    pub selection: String,
}

/// Screen-reader view of the visible viewport (no scrollback clone).
pub fn a11y_mirror(
    snapshot: &Snapshot,
    selection: Option<&TermSelection>,
    focused: bool,
) -> A11yMirror {
    let text = a11y_text(snapshot);
    let mut label = format!(
        "Terminal, {} columns by {} rows",
        snapshot.cols, snapshot.rows
    );
    if let Some(cursor) = snapshot.cursor {
        label.push_str(&format!(
            ", cursor line {} column {}",
            cursor.row + 1,
            cursor.col + 1
        ));
    }
    if snapshot.display_offset > 0 {
        label.push_str(&format!(
            ", scrolled back {} of {} history lines",
            snapshot.display_offset, snapshot.history_size
        ));
    }
    label.push_str(if focused { ", focused" } else { ", unfocused" });

    let selection = selection
        .map(|selection| selection_text(snapshot, selection))
        .unwrap_or_default();
    A11yMirror {
        label,
        text,
        selection,
    }
}

/// Visible grid text with trailing blanks trimmed per row.
pub fn a11y_text(snapshot: &Snapshot) -> String {
    let mut out = String::with_capacity(snapshot.rows * (snapshot.cols + 1));
    for row in 0..snapshot.rows {
        for col in 0..snapshot.cols {
            let cell = snapshot.cell(row, col);
            if cell.attrs & (ATTR_SPACER | ATTR_LEADING_SPACER) != 0 {
                continue;
            }
            match &cell.text {
                Some(cluster) => out.push_str(cluster),
                None if cell.ch != '\0' => out.push(cell.ch),
                None => out.push(' '),
            }
        }
        // Trim the blank tail so screen readers do not announce padding.
        while out.ends_with(' ') {
            out.pop();
        }
        if row + 1 < snapshot.rows {
            out.push('\n');
        }
    }
    out
}

/// Repaint budget for an output-producing terminal: 30 fps when visible,
/// slower when the window is not focused.
pub(crate) fn output_repaint_delay(
    window_focused: bool,
    recent_input: bool,
) -> std::time::Duration {
    if !window_focused {
        std::time::Duration::from_millis(100)
    } else if recent_input {
        std::time::Duration::from_millis(16)
    } else {
        std::time::Duration::from_millis(33)
    }
}
