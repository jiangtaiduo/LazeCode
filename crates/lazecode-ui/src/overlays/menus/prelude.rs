//! Shared imports for the context-menu modules.

pub(super) use std::path::PathBuf;

pub(super) use egui::{
    Align2, Color32, FontId, Key, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2,
};
pub(super) use lazecode_core::{AppModel, Command, ToastKind};

pub(super) use crate::icons::{paint as paint_icon, Icon};
pub(super) use crate::runtime::{DialogState, MenuKind, MenuState, UiRuntime};
pub(super) use crate::theme::Theme;
pub(super) use crate::widgets::{primary_button, secondary_button, text_clipped};

pub(super) use super::super::{
    menu_frame, menu_row, popover_rect, push_dialog_state, should_dismiss, used_worktree_branches,
    MENU_ROW_H,
};
