//! Shared imports for the editor submodules.
//!
//! The editor is split across sibling modules that all need the same egui,
//! model and widget vocabulary. Keeping it in one `pub(super) use` prelude
//! avoids both a copy in every module and "unused import" churn in the
//! parent when only a child uses a name.

pub(super) use std::path::PathBuf;

pub(super) use egui::{
    Align2, Color32, Event, FontId, ImeEvent, Key, Modifiers, Pos2, Rect, Sense, Stroke,
    StrokeKind, TextEdit, Ui, Vec2,
};
pub(super) use lazecode_core::{
    AppModel, Command, DiffSource, DiffTab, DiffViewMode, ProjectState, TabKind,
};
pub(super) use lazecode_text::{Cursor, Document, MatchOptions, Motion, Selection, TokenKind};

pub(super) use crate::icons::{paint as paint_icon, Icon};
pub(super) use crate::runtime::{DialogState, UiRuntime};
pub(super) use crate::theme::{Theme, MONO};
pub(super) use crate::widgets::{mono_char_width, secondary_button, text_clipped, text_width};
