//! Shared imports for the bottom-panel submodules.

pub(super) use std::collections::HashSet;

pub(super) use egui::{
    Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2,
};
pub(super) use lazecode_core::{AppModel, Command};
pub(super) use lazecode_git::{
    lane_color, lane_x, CommitInfo, GraphBreak, GraphRowLayout, GRAPH_BREAK_GAP, GRAPH_H, LANE_W,
    NODE_R,
};

pub(super) use crate::icons::{paint as paint_icon, Icon};
pub(super) use crate::runtime::{GitLogCache, UiRuntime};
pub(super) use crate::theme::Theme;
pub(super) use crate::widgets::{icon_button, text_clipped};
