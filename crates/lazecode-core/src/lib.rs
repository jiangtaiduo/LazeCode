//! Application model for LazeCode: projects, open tabs, terminals, settings
//! and Git snapshots. This crate contains no UI and no winit/egui types.

pub mod commands;
pub mod gitref;
pub mod lsp;
pub mod model;
pub mod settings;
pub mod updater;

pub use commands::Command;
pub use gitref::{FlatRefRow, RefTreeEntry, RefTreeNode};
pub use lsp::{
    language_id_for_path, Diagnostic, DiagnosticSeverity, DocumentSymbol, LspManager, LspPosition,
    LspRange, ServerKey, WorkspaceEdit,
};
pub use model::{
    aggregate_commit_files, is_json_path, is_snapshot_ignored, selected_commit_range, AppModel,
    CommitRangeDetails, CompareBranchesSideView, CompareBranchesState, CompareDateFilter,
    CompareDetail, CompareSession, DatePreset, DiffSource, DiffTab, DiffViewMode, FileTab,
    GitSubtab, ImageTab, LogQuery, MarkdownViewMode, PaneSplit, PanelTab, ProjectState,
    SidebarView, Tab, TabKind, TerminalPane, TerminalPlacement, TerminalTab, Toast, ToastKind,
    UiState, WorktreeState, MAX_PANES_PER_TAB,
};
pub use settings::{Language, Settings, ThemeMode, ThemeSkin, WorkspaceFile};
