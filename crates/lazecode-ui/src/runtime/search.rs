//! Find/search/palette/LSP UI state and the palette command model.

use std::time::Instant;

use lazecode_fs::SearchHit;
use lazecode_text::Cursor;

use crate::icons::Icon;

#[derive(Default)]
pub struct FindState {
    pub open: bool,
    pub last_revision: u64,
    pub last_signature: String,
    pub query: String,
    pub replacement: String,
    pub case_sensitive: bool,
    pub regex: bool,
    pub whole_word: bool,
    pub matches: Vec<(Cursor, Cursor)>,
    pub index: usize,
    pub focus_requested: bool,
    pub opened_frame: u64,
}

#[derive(Default)]
pub struct SearchState {
    pub open: bool,
    pub query: String,
    pub replacement: String,
    pub replace_mode: bool,
    pub file_mask: String,
    pub case_sensitive: bool,
    pub regex: bool,
    pub whole_word: bool,
    pub results: Vec<SearchHit>,
    pub selected: usize,
    pub searched: bool,
    pub focus_requested: bool,
    pub request_id: u64,
    pub opened_frame: u64,
    /// When the query/flags last changed; drives the short debounce before
    /// the async filesystem search starts.
    pub dirty_since: Option<f64>,
    /// Query+flags signature represented by the current results.
    pub signature: String,
    pub searching: bool,
}

#[derive(Default)]
pub struct PaletteState {
    pub query: String,
    pub selected: usize,
    pub focus_requested: bool,
    pub opened_frame: u64,
}

#[derive(Default)]
pub struct LspHoverState {
    pub tab_id: u64,
    pub line: usize,
    pub col: usize,
    pub pointer: egui::Pos2,
    /// When the pointer settled on this character.
    pub since: Option<Instant>,
    pub request: Option<i64>,
    pub requested_at: Option<Instant>,
    pub result: Option<lazecode_core::lsp::Hover>,
}

#[derive(Default)]
pub struct LspCompletionState {
    pub tab_id: u64,
    pub request: Option<i64>,
    pub requested_at: Option<Instant>,
    pub items: Vec<lazecode_core::lsp::CompletionItem>,
    pub selected: usize,
    /// Start of the word prefix that `insert_text` replaces.
    pub prefix_start: Cursor,
    /// Cursor and revision when the request was sent; a response that arrives
    /// after the user moved or typed is stale and must not be inserted.
    pub request_cursor: Cursor,
    pub revision: u64,
    /// Caret screen position, refreshed by the editor while the popup is open.
    pub anchor: egui::Pos2,
}

#[derive(Default)]
pub struct LspSymbolState {
    pub tab_id: u64,
    pub request: Option<i64>,
    pub requested_at: Option<Instant>,
    pub items: Vec<lazecode_core::lsp::DocumentSymbol>,
    pub selected: usize,
}

/// Cross-frame state for the optional language-server features. Requests are
/// ids into `AppModel::lsp`; completed responses are polled here each frame.
#[derive(Default)]
pub struct LspUiState {
    pub hover: Option<LspHoverState>,
    pub completion: Option<LspCompletionState>,
    pub symbols: Option<LspSymbolState>,
    /// Set by the editor after a completion trigger character.
    pub completion_due: Option<(u64, Instant)>,
    /// Set by Cmd+Click / F12; sent after the current input batch.
    pub goto_request: Option<(u64, Cursor)>,
    /// Set by Option+Cmd+L.
    pub format_request: Option<u64>,
    /// Set by Cmd+Shift+O.
    pub symbols_request: Option<u64>,
    pub goto_pending: Option<i64>,
    pub format_pending: Option<(u64, i64)>,
    pub rename_pending: Option<i64>,
}

pub struct PaletteItem {
    pub icon: Icon,
    pub title: String,
    pub subtitle: String,
    pub shortcut: Option<String>,
    pub action: PaletteAction,
}

#[derive(Clone, Debug)]
pub enum PaletteAction {
    OpenFile(std::path::PathBuf),
    ToggleSidebar,
    ToggleConsole,
    OpenSettings,
    CheckUpdates,
    ToggleTheme,
    OpenProject,
    NewTerminal,
    SaveFile,
    CloseTab,
    OpenGitLog,
    RefreshGit,
    OpenSearch,
    OpenPush,
    OpenPull,
    OpenRemotes,
    FetchAll,
    StashChanges,
    ShelvedChanges,
    About,
    SwitchProject(usize),
    SwitchWorktree { project: usize, name: String },
    CloneRepo,
    StashManager,
    ShelfManager,
    GitTools,
    CompareBranches,
    GoToLine,
    FormatDocument,
    FormatJson,
    MinifyJson,
    GoToSymbol,
    RenameSymbol,
    GoToDefinition,
    Quit,
}
