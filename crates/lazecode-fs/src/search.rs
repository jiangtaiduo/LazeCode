use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{FsError, Result};

#[derive(Clone, Debug)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
    /// Optional glob restricting the file name (e.g. `*.rs`).
    pub include: Option<String>,
    pub max_results: usize,
    pub max_file_bytes: u64,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            case_sensitive: false,
            whole_word: false,
            regex: false,
            include: None,
            max_results: 500,
            max_file_bytes: 2 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub path: PathBuf,
    pub line: usize,
    /// Byte range inside `line_text`.
    pub start: usize,
    pub end: usize,
    pub line_text: String,
}

/// Search file contents under `root`. The walk respects `.gitignore`.
///
/// Cancellation is cooperative: pass `Some(&flag)` and set it to true to stop.
pub fn search_in_files(
    root: &Path,
    query: &str,
    options: &SearchOptions,
    cancel: Option<&AtomicBool>,
) -> Result<Vec<SearchHit>> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let escaped = regex::escape(query);
    let pattern = if options.regex {
        query.to_string()
    } else {
        escaped
    };
    let pattern = if options.whole_word && !options.regex {
        format!(r"\b{pattern}\b")
    } else {
        pattern
    };
    let matcher = regex::RegexBuilder::new(&pattern)
        .case_insensitive(!options.case_sensitive)
        .size_limit(4 * 1024 * 1024)
        .build()
        .map_err(|err| FsError::Pattern(err.to_string()))?;

    let include = match &options.include {
        Some(glob) if !glob.trim().is_empty() => {
            let glob = glob.trim();
            Some(
                globset::Glob::new(glob)
                    .map_err(|err| FsError::Pattern(err.to_string()))?
                    .compile_matcher(),
            )
        }
        _ => None,
    };

    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .follow_links(false)
        .max_depth(Some(64))
        .filter_entry(|entry| !crate::is_default_ignored(entry.file_name()))
        .build();

    let mut hits = Vec::new();
    'files: for entry in walker {
        if cancel.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            break;
        }
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_some_and(|ty| ty.is_file()) {
            continue;
        }
        let Some(name) = entry.file_name().to_str() else {
            continue;
        };
        if let Some(include) = &include {
            if !include.is_match(name) {
                continue;
            }
        }
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.len() > options.max_file_bytes {
            continue;
        }
        let Ok(bytes) = std::fs::read(entry.path()) else {
            continue;
        };
        if bytes.iter().take(8192).any(|b| *b == 0) {
            continue;
        }
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        for (line_idx, line) in text.split_inclusive('\n').enumerate() {
            for found in matcher.find_iter(line) {
                hits.push(SearchHit {
                    path: entry
                        .path()
                        .strip_prefix(root)
                        .unwrap_or(entry.path())
                        .to_path_buf(),
                    line: line_idx + 1,
                    start: found.start(),
                    end: found.end(),
                    line_text: line.trim_end_matches(['\r', '\n']).to_string(),
                });
                if hits.len() >= options.max_results {
                    break 'files;
                }
            }
        }
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_matches_and_respects_include() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn main() {}\nlet x = 1;\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "fn main() {}\n").unwrap();
        let options = SearchOptions {
            include: Some("*.rs".into()),
            ..Default::default()
        };
        let hits = search_in_files(dir.path(), "main", &options, None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, PathBuf::from("a.rs"));
        assert_eq!(hits[0].line, 1);
        assert_eq!(&hits[0].line_text[hits[0].start..hits[0].end], "main");
    }

    #[test]
    fn whole_word() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "main domain\n").unwrap();
        let options = SearchOptions {
            whole_word: true,
            ..Default::default()
        };
        let hits = search_in_files(dir.path(), "main", &options, None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].start, 0);
    }
}
