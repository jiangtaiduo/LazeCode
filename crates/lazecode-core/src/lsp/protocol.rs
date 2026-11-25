//! LSP protocol types plus the server key and URI helpers.

use std::path::{Path, PathBuf};

use super::parse::hex_value;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

impl DiagnosticSeverity {
    pub(super) fn from_lsp(value: Option<u64>) -> Self {
        match value {
            Some(1) => Self::Error,
            Some(2) => Self::Warning,
            Some(3) => Self::Information,
            _ => Self::Hint,
        }
    }

    pub fn is_problem(self) -> bool {
        matches!(self, Self::Error | Self::Warning)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub range: LspRange,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub source: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Hover {
    pub text: String,
    pub range: Option<LspRange>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    pub path: PathBuf,
    pub range: LspRange,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    pub detail: Option<String>,
    pub documentation: Option<String>,
    pub insert_text: String,
    pub kind: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkspaceEdit {
    pub files: Vec<(PathBuf, Vec<lazecode_text::LspTextEdit>)>,
}

impl WorkspaceEdit {
    pub fn is_empty(&self) -> bool {
        self.files.iter().all(|(_, edits)| edits.is_empty())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentSymbol {
    pub name: String,
    pub detail: Option<String>,
    pub kind: u64,
    pub selection: LspRange,
    pub depth: usize,
}

pub use lazecode_text::LspTextEdit as TextEdit;

/// A server instance is scoped by workspace root and language. Worktrees and
/// separate projects therefore never share a server unless they share a root.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct ServerKey {
    pub root: PathBuf,
    pub language: String,
}

impl ServerKey {
    pub fn new(root: impl Into<PathBuf>, language: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            language: language.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// URI conversion
// ---------------------------------------------------------------------------

/// Convert an absolute path to a `file://` URI. Non-ASCII bytes and reserved
/// characters are percent-encoded so servers see the same document identity
/// that they report back.
pub fn path_to_uri(path: &Path) -> String {
    let text = path.to_string_lossy();
    let mut uri = String::with_capacity(text.len() + 8);
    uri.push_str("file://");
    if !text.starts_with('/') {
        uri.push('/');
    }
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => {
                uri.push(*byte as char)
            }
            _ => {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                uri.push('%');
                uri.push(HEX[(byte >> 4) as usize] as char);
                uri.push(HEX[(byte & 0x0F) as usize] as char);
            }
        }
    }
    uri
}

/// Convert a `file://` URI back to a path. Non-file URIs (untitled buffers)
/// are ignored; the client only tracks real files.
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let bytes = rest.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = hex_value(bytes[index + 1])?;
            let low = hex_value(bytes[index + 2])?;
            decoded.push(high << 4 | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok().map(PathBuf::from)
}
