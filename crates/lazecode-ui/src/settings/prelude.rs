//! Shared imports for the settings submodules.

pub(super) use egui::{
    Align2, FontId, Pos2, Rect, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2,
};
pub(super) use lazecode_core::{AppModel, Language, ThemeMode, ThemeSkin};

pub(super) use crate::icons::Icon;
pub(super) use crate::runtime::UiRuntime;
pub(super) use crate::theme::Theme;
pub(super) use crate::widgets::{
    checkbox, icon_button, primary_button, secondary_button, secondary_button_enabled, text_clipped,
};
