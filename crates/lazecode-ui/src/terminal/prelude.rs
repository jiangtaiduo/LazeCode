//! Shared imports for the terminal-pane submodules.

pub(super) use std::collections::HashMap;

pub(super) use egui::{
    Align2, Color32, Event, FontId, ImeEvent, Key, Modifiers, Pos2, Rect, Sense, Stroke,
    StrokeKind, TextEdit, Ui, Vec2,
};
pub(super) use lazecode_terminal::{
    ColorSpec, ImagePlacement, KeyEvent, Session, Snapshot, TerminalKey, ATTR_LEADING_SPACER,
    ATTR_SPACER,
};

pub(super) use crate::icons::{paint as paint_icon, Icon};
pub(super) use crate::runtime::{TermSearchState, TermSelection};
pub(super) use crate::theme::Theme;
