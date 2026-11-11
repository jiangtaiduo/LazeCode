//! Rope-backed documents, incremental syntax highlighting and diffing.
//!
//! The crate is UI-agnostic: it yields line text, styled spans, cursors and
//! hunks, leaving colour decisions to `lazecode-ui`.

pub mod diff;
pub mod document;
pub mod lsp;
pub mod syntax;

pub use diff::{
    change_stats, count_change_groups, diff_text, diff_text_with_context, inline_diff,
    unified_patch, DiffHunk, DiffLine, DiffLineKind, InlineChunk, InlineChunkKind, TextDiff,
};
pub use document::{Cursor, Document, MatchOptions, Motion, Selection};
pub use lazecode_fs::{Eol, TextFile};
pub use lsp::LspTextEdit;
pub use syntax::{HighlightSpan, SyntaxHighlighter, TokenKind};
