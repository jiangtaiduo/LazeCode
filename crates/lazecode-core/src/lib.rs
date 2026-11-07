//! Application model for LazeCode: projects, open tabs, terminals, settings
//! and Git snapshots. This crate contains no UI and no winit/egui types.

pub mod commands;
pub mod gitref;
pub mod model;
pub mod settings;

pub use commands::Command;
pub use gitref::{FlatRefRow, RefTreeEntry, RefTreeNode};
pub use model::{
    AppModel, CompareBranchesSideView, CompareBranchesState, CompareDateFilter, CompareDetail,
    DatePreset, DiffSource, DiffTab, DiffViewMode, FileTab, LogQuery, MarkdownViewMode, PanelTab,
    ProjectState, SidebarView, Tab, TabKind, TerminalTab, Toast, ToastKind, UiState, WorktreeState,
};
pub use settings::{Language, Settings, ThemeMode, WorkspaceFile};
