//! Rope-backed documents, incremental syntax highlighting and diffing.
//!
//! The crate is UI-agnostic: it yields line text, styled spans, cursors and
//! hunks, leaving colour decisions to `lazecode-ui`.

pub mod diff;
pub mod document;
pub mod syntax;

pub use diff::{DiffHunk, DiffLine, DiffLineKind, TextDiff};
pub use document::{Cursor, Document, MatchOptions, Motion, Selection};
pub use lazecode_fs::{Eol, TextFile};
pub use syntax::{HighlightSpan, SyntaxHighlighter, TokenKind};
