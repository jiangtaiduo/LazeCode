//! Single-file blame and file history, via the `git` porcelain (gix's blame
//! implementation is still primitive in 0.90), plus submodule inventory and
//! LFS status/track passthrough.

use std::path::Path;

use anyhow::anyhow;

use crate::Result;

fn run(root: &Path, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|err| anyhow!("git spawn failed: {err}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(anyhow!("{}{}", stdout.trim(), stderr.trim()));
    }
    Ok(stdout.trim().to_string())
}

// ---------------- blame ----------------

#[derive(Clone, Debug)]
pub struct BlameLine {
    pub hash: String,
    pub short: String,
    pub author: String,
    pub time: i64,
    /// 1-based line number this row came from (renames resolved by git).
    pub line: usize,
    pub content: String,
}

pub fn blame(root: &Path, path: &str) -> Result<Vec<BlameLine>> {
    let output = run(root, &["blame", "--porcelain", "--", path])?;
    // Passing structure: "<hash> <line> <n>" header; metadata until "summary";
    // entry then "	" content line; subsequent rows of the same commit print
    // only the hash header (no metadata).
    struct Pending {
        hash: String,
        line_no: usize,
        author: String,
        time: i64,
    }
    let mut pending: Option<Pending> = None;
    let mut entries: Vec<BlameLine> = Vec::new();
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix('\t') {
            if let Some(state) = pending.take() {
                entries.push(BlameLine {
                    hash: state.hash.chars().take(40).collect(),
                    short: state.hash.chars().take(8).collect(),
                    author: state.author,
                    time: state.time,
                    line: state.line_no,
                    content: rest.to_string(),
                });
            }
            continue;
        }
        // Commit header starts with a 40-hex hash.
        if line.len() >= 40 && line.chars().take(40).all(|c| c.is_ascii_hexdigit()) {
            let mut parts = line.splitn(4, ' ');
            let hash = parts.next().unwrap_or("").to_string();
            let line_no: usize = parts.next().unwrap_or("").parse().unwrap_or(0);
            // The FIRST header row of every commit carries metadata rows
            // optionally; repeat rows don't re-declare author. Track which
            // hash we saw metadata for.
            let known = entries.iter().any(|entry| entry.hash == hash)
                || pending.as_ref().is_some_and(|state| state.hash == hash);
            pending = Some(Pending {
                hash,
                line_no,
                author: String::new(),
                time: 0,
            });
            if !known {
                continue;
            }
            let _ = known;
            continue;
        }
        if let Some(state) = pending.as_mut() {
            if let Some(rest) = line.strip_prefix("author ") {
                state.author = rest.to_string();
            } else if let Some(rest) = line.strip_prefix("author-time ") {
                state.time = rest.parse().unwrap_or(state.time);
            } else if let Some(rest) = line.strip_prefix("previous ") {
                // Not used.
                let _ = rest;
            }
        }
    }
    Ok(entries)
}

// ---------------- file history ----------------

#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub hash: String,
    pub short: String,
    pub subject: String,
    pub author: String,
    pub time: i64,
}

pub fn file_history(root: &Path, path: &str, limit: usize) -> Result<Vec<HistoryEntry>> {
    let limit_arg = format!("-n={limit}");
    let _ = limit_arg;
    let output = run(
        root,
        &[
            "log",
            "--pretty=format:%H%x09%h%x09%an%x09%at%x09%s",
            "-n",
            &limit.to_string(),
            "--",
            path,
        ],
    )?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let hash = parts.next()?.to_string();
            let short = parts.next()?.to_string();
            let author = parts.next()?.to_string();
            let time = parts.next()?.parse().ok()?;
            let subject = parts.next().unwrap_or("").to_string();
            Some(HistoryEntry {
                hash,
                short,
                author,
                time,
                subject,
            })
        })
        .collect())
}

// ---------------- submodules ----------------

#[derive(Clone, Debug)]
pub struct SubmoduleEntry {
    pub path: String,
    pub url: String,
    pub status: String,
}

pub fn submodule_list(root: &Path) -> Result<Vec<SubmoduleEntry>> {
    let output = run(root, &["submodule", "status", "--recursive"])?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let status = line.chars().next()?.to_string();
            let rest = line.get(1..)?.trim_start();
            let (path, url) = match rest.split_once(' ') {
                Some((path, url)) => (
                    path.to_string(),
                    url.trim_matches(|c| c == '(' || c == ')').to_string(),
                ),
                None => (rest.to_string(), String::new()),
            };
            Some(SubmoduleEntry {
                path,
                url,
                status: match status.as_str() {
                    " " => "clean".to_string(),
                    "-" => "not initialized".to_string(),
                    "+" => "different commit".to_string(),
                    "U" => "merge conflicts".to_string(),
                    _ => status,
                },
            })
        })
        .collect())
}

// ---------------- LFS ----------------

#[derive(Clone, Debug, Default)]
pub struct LfsStatus {
    pub installed: bool,
    /// Glob patterns currently tracked by LFS.
    pub patterns: Vec<String>,
}

pub fn lfs_status(root: &Path) -> LfsStatus {
    let installed = std::process::Command::new("git")
        .current_dir(root)
        .args(["lfs", "version"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    let patterns = std::fs::read_to_string(root.join(".gitattributes"))
        .map(|text| {
            text.lines()
                .filter_map(|line| {
                    // "<glob> filter=lfs diff=lfs merge=lfs -text"
                    let mut parts = line.split_whitespace();
                    let glob = parts.next()?.to_string();
                    (parts.any(|part| part == "filter=lfs")).then_some(glob)
                })
                .collect()
        })
        .unwrap_or_default();
    LfsStatus {
        installed,
        patterns,
    }
}

pub fn lfs_track(root: &Path, pattern: &str) -> Result<String> {
    if pattern.contains(' ') || pattern.contains(';') {
        anyhow::bail!("pattern must be a single glob");
    }
    run(root, &["lfs", "track", pattern])
}

pub fn lfs_pull(root: &Path) -> Result<String> {
    run(root, &["lfs", "pull"])
}

// ---------------- compare ----------------

#[derive(Clone, Debug)]
pub struct CompareSide {
    pub commits: Vec<crate::model::CommitInfo>,
    /// Files changed between the tips' merge-base..tip.
    pub files: Vec<(String, String)>, // (path, kind A/M/D)
}

pub fn compare_branches(
    root: &Path,
    left: &str,
    right: &str,
) -> Result<(CompareSide, CompareSide)> {
    // merge-base of the two tips; unrelated histories fall back to empty diff.
    let base = run(root, &["merge-base", left, right]).unwrap_or_default();
    let build = |tip: &str, range: String| -> Result<CompareSide> {
        let options = crate::model::LogOptions {
            limit: 500,
            branch: Some(range),
            topo_order: true,
            ..Default::default()
        };
        let commits = crate::log::log(root, options)?;
        // Files of tip relative to the merge-base (or an empty tree).
        let range_arg = if base.is_empty() {
            tip.to_string()
        } else {
            format!("{base}..{tip}")
        };
        let name_only = run(
            root,
            &["diff", "--name-status", "--no-color", &range_arg, "--"],
        )
        .unwrap_or_default();
        let files: Vec<(String, String)> = name_only
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(2, '\t');
                let kind = parts.next()?.to_string();
                let path = parts.next()?.to_string();
                if path.is_empty() {
                    None
                } else {
                    Some((path, kind))
                }
            })
            .collect();
        Ok(CompareSide { commits, files })
    };
    let left_range = format!("{right}..{left}");
    let right_range = format!("{left}..{right}");
    let left_side = build(left, left_range)?;
    let right_side = build(right, right_range)?;
    Ok((left_side, right_side))
}

/// Files changed between any two revisions.
pub fn compare_commits(root: &Path, base: &str, tip: &str) -> Result<Vec<(String, String)>> {
    let output = run(
        root,
        &["diff", "--name-status", "--no-color", base, tip, "--"],
    )?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, '\t');
            let kind = parts.next()?.to_string();
            let path = parts.next()?.to_string();
            Some((path, kind))
        })
        .collect())
}
