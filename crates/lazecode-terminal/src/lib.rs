//! PTY-backed terminal sessions.
//!
//! `alacritty_terminal` owns the VT grid and scrollback; this crate adds the
//! PTY lifecycle, a reusable screen snapshot for the UI, key/paste encoding and
//! scrollback search. Parsing runs on a dedicated reader thread while the UI
//! only copies the visible grid.

pub mod input;
pub mod session;

pub use alacritty_terminal::vte::ansi::CursorShape;
pub use input::{encode_key, encode_paste, KeyStroke, TerminalKey};
pub use session::{
    spawn, CellData, ColorSpec, CursorView, Modes, Notifier, Session, SessionKind, Snapshot,
    SpawnOptions, TerminalMatch, ATTR_BOLD, ATTR_DIM, ATTR_HIDDEN, ATTR_INVERSE, ATTR_ITALIC,
    ATTR_LEADING_SPACER, ATTR_SPACER, ATTR_STRIKEOUT, ATTR_UNDERLINE, ATTR_WIDE,
};

use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}
