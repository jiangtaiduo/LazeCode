//! Language detection and the per-language server binaries.

use std::path::Path;

// ---------------------------------------------------------------------------
// Language table
// ---------------------------------------------------------------------------

/// Languages with an optional language-server integration. This is the stable
/// list from `src/services/lsp.ts`; TS/JS/JSON/CSS/HTML deliberately stay
/// syntax-highlighting only (no Node runtime, no external server).
pub const LSP_LANGUAGES: [&str; 11] = [
    "python", "go", "rust", "c", "cpp", "java", "ruby", "php", "kotlin", "swift", "lua",
];

#[derive(Clone, Copy)]
pub(super) struct Candidate {
    pub(super) command: &'static str,
    pub(super) args: &'static [&'static str],
}

const PYTHON_CANDIDATES: &[Candidate] = &[
    Candidate {
        command: "pyright-langserver",
        args: &["--stdio"],
    },
    Candidate {
        command: "basedpyright-langserver",
        args: &["--stdio"],
    },
    Candidate {
        command: "pylsp",
        args: &[],
    },
];
const GO_CANDIDATES: &[Candidate] = &[Candidate {
    command: "gopls",
    args: &[],
}];
const RUST_CANDIDATES: &[Candidate] = &[Candidate {
    command: "rust-analyzer",
    args: &[],
}];
const CLANGD: &[Candidate] = &[Candidate {
    command: "clangd",
    args: &[],
}];
const JAVA_CANDIDATES: &[Candidate] = &[Candidate {
    command: "jdtls",
    args: &[],
}];
const RUBY_CANDIDATES: &[Candidate] = &[Candidate {
    command: "solargraph",
    args: &["stdio"],
}];
const PHP_CANDIDATES: &[Candidate] = &[Candidate {
    command: "intelephense",
    args: &["--stdio"],
}];
const KOTLIN_CANDIDATES: &[Candidate] = &[Candidate {
    command: "kotlin-language-server",
    args: &[],
}];
const SWIFT_CANDIDATES: &[Candidate] = &[Candidate {
    command: "sourcekit-lsp",
    args: &[],
}];
const LUA_CANDIDATES: &[Candidate] = &[Candidate {
    command: "lua-language-server",
    args: &[],
}];

pub(super) fn candidates(language: &str) -> &'static [Candidate] {
    match language {
        "python" => PYTHON_CANDIDATES,
        "go" => GO_CANDIDATES,
        "rust" => RUST_CANDIDATES,
        "c" | "cpp" => CLANGD,
        "java" => JAVA_CANDIDATES,
        "ruby" => RUBY_CANDIDATES,
        "php" => PHP_CANDIDATES,
        "kotlin" => KOTLIN_CANDIDATES,
        "swift" => SWIFT_CANDIDATES,
        "lua" => LUA_CANDIDATES,
        _ => &[],
    }
}

/// Map a file path to one of [`LSP_LANGUAGES`]. `None` means the file keeps
/// syntax highlighting only; the editor must not start a server for it.
pub fn language_id_for_path(path: &Path) -> Option<&'static str> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_ascii_lowercase())?;
    if matches!(file_name.as_str(), "gemfile" | "rakefile") {
        return Some("ruby");
    }
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())?;
    Some(match extension.as_str() {
        "py" | "pyi" => "python",
        "go" => "go",
        "rs" => "rust",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
        "java" => "java",
        "rb" | "rake" => "ruby",
        "php" => "php",
        "kt" | "kts" => "kotlin",
        "swift" => "swift",
        "lua" => "lua",
        _ => return None,
    })
}
