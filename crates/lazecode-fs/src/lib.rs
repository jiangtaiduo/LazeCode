//! Filesystem primitives for LazeCode: project trees, text files, search and
//! change notification.
//!
//! The crate deliberately has no UI or Git knowledge; callers compose it with
//! `lazecode-git` and `lazecode-text`.

pub mod file;
pub mod search;
pub mod tree;
pub mod watch;

pub use file::{move_to_trash, Eol, TextFile, MAX_FILE_BYTES, MAX_FORCE_FILE_BYTES};
pub use search::{compile_regex, replace_in_files, search_in_files, SearchHit, SearchOptions};
pub use tree::{FileTree, NodeKind, TreeNode};
pub use watch::{watch_project, ProjectWatcher, WatchBatch};

/// Errors produced by filesystem operations.
#[derive(Debug, thiserror::Error)]
pub enum FsError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot write {path}: {source}")]
    Write {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is not a regular file")]
    NotAFile { path: std::path::PathBuf },
    #[error("{path} looks like a binary file")]
    Binary { path: std::path::PathBuf },
    #[error("{path} is too large ({size} bytes, limit {limit})")]
    TooLarge {
        path: std::path::PathBuf,
        size: u64,
        limit: u64,
    },
    #[error("invalid search pattern: {0}")]
    Pattern(String),
    #[error("cannot represent all characters in {path} using {encoding}")]
    Unmappable {
        path: std::path::PathBuf,
        encoding: &'static str,
    },
}

pub type Result<T, E = FsError> = std::result::Result<T, E>;

/// Directories that are never interesting in a project tree or a status list.
/// Git's own ignore rules are still applied; this list only removes build and
/// dependency output that would otherwise flood the UI.
pub fn is_default_ignored(name: &std::ffi::OsStr) -> bool {
    matches!(
        name.to_str(),
        Some(".git")
            | Some(".DS_Store")
            | Some("target")
            | Some("node_modules")
            | Some("dist")
            | Some(".next")
            | Some(".nuxt")
            | Some(".venv")
            | Some("__pycache__")
            | Some(".gradle")
    )
}

/// Normalise a path for display: replace the home directory with `~`.
pub fn display_path(path: &std::path::Path) -> String {
    if let Some(home) = dirs_home() {
        if let Ok(rest) = path.strip_prefix(&home) {
            let rest = rest.to_string_lossy();
            return if rest.is_empty() {
                "~".to_string()
            } else {
                format!("~/{rest}")
            };
        }
    }
    path.to_string_lossy().into_owned()
}

fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

/// Total number of files below `tree` (folders excluded).
pub fn file_count(tree: &FileTree) -> usize {
    fn count(nodes: &[TreeNode]) -> usize {
        nodes
            .iter()
            .map(|node| match node.kind {
                NodeKind::File => 1,
                NodeKind::Folder => count(&node.children),
            })
            .sum()
    }
    count(&tree.nodes)
}
