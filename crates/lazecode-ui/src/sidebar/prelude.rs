//! Shared imports for the sidebar submodules.

pub(super) use std::collections::{HashMap, HashSet};

pub(super) use egui::{
    Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2,
};
pub(super) use lazecode_core::{AppModel, Command, ProjectState, SidebarView, ToastKind};
pub(super) use lazecode_fs::tree::{visible_rows, NodeKind, VisibleRow};

pub(super) use crate::icons::{paint as paint_icon, Icon};
pub(super) use crate::runtime::{CommitTab, UiRuntime};
pub(super) use crate::theme::Theme;
pub(super) use crate::widgets::{
    checkbox, checkbox_tri, icon_button, live_dot, primary_button, secondary_button_enabled,
    text_clipped, text_width, TriState,
};
