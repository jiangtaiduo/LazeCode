//! Shared imports for the compare-page submodules.

pub(super) use egui::{
    Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2,
};
pub(super) use lazecode_core::{AppModel, Command};

pub(super) use crate::icons::{paint as paint_icon, Icon};
pub(super) use crate::runtime::UiRuntime;
pub(super) use crate::theme::Theme;
pub(super) use crate::widgets::{secondary_button, text_clipped};
