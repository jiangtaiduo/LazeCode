//! Filesystem primitives: project trees, text files, search and change
//! notification. No UI or Git knowledge.

pub mod file;
pub mod search;
pub mod tree;
pub mod watch;

pub use file::{move_to_trash, Eol, TextFile, MAX_FILE_BYTES, MAX_FORCE_FILE_BYTES};
pub use search::{compile_regex, replace_in_files, search_in_files, SearchHit, SearchOptions};
pub use tree::{
    load_tree, load_tree_filtered, load_tree_with_expanded, FileTree, NodeKind, TreeNode,
};
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

/// Match an entry against ignored patterns (e.g. from settings).
pub fn is_ignored_pattern(name: &str, is_dir: bool, custom_ignored: Option<&[String]>) -> bool {
    const DEFAULT_IGNORED: &[&str] = &[
        ".git",
        ".idea",
        ".DS_Store",
        "node_modules",
        ".venv",
        "__pycache__",
    ];

    let patterns: Vec<&str> = match custom_ignored {
        Some(list) if !list.is_empty() => list
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => DEFAULT_IGNORED.to_vec(),
    };

    for pat in patterns {
        if let Some(suffix) = pat.strip_prefix('*') {
            if name.ends_with(suffix) {
                return true;
            }
        } else if let Some(prefix) = pat.strip_suffix('*') {
            if name.starts_with(prefix) {
                return true;
            }
        } else if pat.ends_with('/') {
            let dir_pat = pat.trim_end_matches('/');
            if is_dir && name == dir_pat {
                return true;
            }
        } else if name == pat {
            return true;
        }
    }

    false
}

pub fn is_default_ignored(name: &std::ffi::OsStr) -> bool {
    is_ignored_pattern(name.to_str().unwrap_or_default(), true, None)
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
