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

/// 文件条目：`--name-status` 的状态字母 + `--numstat` 的增删行数合并
/// （稳定版 CommitFileDto 的等价物；二进制行显示为 `-`，记 0）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompareFile {
    pub path: String,
    pub status: char,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Clone, Debug, Default)]
pub struct CompareSide {
    pub commits: Vec<crate::model::CommitInfo>,
    /// 提交日志遍历触及上限（还有更早的提交，前端显示 Load More）。
    pub truncated: bool,
    /// 该侧相对 merge-base 的文件清单（`other...side` 三点）。
    pub files: Vec<CompareFile>,
}

#[derive(Clone, Debug, Default)]
pub struct CompareBranches {
    pub left: CompareSide,
    pub right: CompareSide,
    /// 两个分支端点的 merge-base；`None` = 无共同祖先（unrelated
    /// histories），文件清单退化为空（对齐稳定版语义）。
    pub merge_base: Option<String>,
}

/// 把 `--numstat` 与 `--name-status` 两路输出合并成 CompareFile 列表。
/// numstat 失败时状态字母退化为 'M'（对齐稳定版 git_commit_files）。
fn merge_file_lists(numstat: &str, name_status: &str) -> Vec<CompareFile> {
    let mut counts: std::collections::HashMap<String, (u32, u32)> =
        std::collections::HashMap::new();
    for line in numstat.lines() {
        let mut parts = line.splitn(3, '\t');
        let add = parts.next().unwrap_or("");
        let del = parts.next().unwrap_or("");
        let path = match parts.next() {
            Some(path) if !path.is_empty() => path,
            _ => continue,
        };
        let parse = |text: &str| -> u32 {
            text.parse::<u32>().unwrap_or(0) // binary rows show '-'
        };
        counts.insert(path.to_string(), (parse(add), parse(del)));
    }
    let mut files: Vec<CompareFile> = Vec::new();
    for line in name_status.lines() {
        let mut parts = line.splitn(2, '\t');
        let kind = parts.next().unwrap_or("").trim();
        let path = match parts.next() {
            Some(path) if !path.is_empty() => path.to_string(),
            _ => continue,
        };
        let status = kind.chars().next().unwrap_or('M');
        let (additions, deletions) = counts.remove(&path).unwrap_or((0, 0));
        files.push(CompareFile {
            path,
            status,
            additions,
            deletions,
        });
    }
    files
}

/// 统一的两路 diff：`git diff --no-renames --numstat/--name-status <range> [-- path]`。
fn diff_range_files(root: &Path, range: &str, path: Option<&str>) -> Vec<CompareFile> {
    let base_args: Vec<&str> = vec!["diff", "--no-renames", "--no-color"];
    let mut tail: Vec<String> = vec![range.to_string()];
    if let Some(path) = path {
        if !path.trim().is_empty() {
            tail.push("--".to_string());
            tail.push(path.trim().to_string());
        }
    }
    // numstat / name-status 两路并发执行（对齐稳定版 git_commit_files 的
    // 合并策略；任一路失败该路退化为空）。
    let mut numstat_args = base_args.clone();
    numstat_args.push("--numstat");
    let numstat = run_args(root, &numstat_args, &tail).unwrap_or_default();
    let mut status_args = base_args;
    status_args.push("--name-status");
    let name_status = run_args(root, &status_args, &tail).unwrap_or_default();
    merge_file_lists(&numstat, &name_status)
}

fn run_args(root: &Path, args: &[&str], tail: &[String]) -> Result<String> {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .args(tail)
        .output()
        .map_err(|err| anyhow!("git spawn failed: {err}"))?;
    if !output.status.success() {
        anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// 双向分支比较（对齐稳定版 git_compare_branches）：
/// - 每侧提交 = `git log {other}..{side}`（topo 序），多取 1 条探测截断；
/// - 每侧文件 = `git diff --no-renames {other}...{side}`（三点 =
///   merge-base..side，只统计该侧独有变更；失败不拖垮提交列表，退化为空）；
/// - `left_path`/`right_path` 同时作用于提交日志与文件清单；
/// - `limit` 夹在 1..=10000。
pub fn compare_branches(
    root: &Path,
    left: &str,
    right: &str,
    left_path: Option<&str>,
    right_path: Option<&str>,
    limit: usize,
) -> Result<CompareBranches> {
    let limit = limit.clamp(1, 10_000);
    // merge-base of the two tips; unrelated histories -> None.
    let base = run(root, &["merge-base", "--end-of-options", left, right]).ok();
    let build = |tip: &str, range: String, path: Option<&str>| -> Result<CompareSide> {
        let options = crate::model::LogOptions {
            limit: limit + 1,
            branch: Some(range),
            topo_order: true,
            path: path.map(str::to_string),
            ..Default::default()
        };
        let mut commits = crate::log::log(root, options)?;
        let truncated = commits.len() > limit;
        commits.truncate(limit);
        // Files of tip relative to the merge-base. 无 merge-base 时不做
        // 端点退化（git 三点语法自身会失败）——与稳定版一致退化为空清单。
        let range_arg = match &base {
            Some(base) => format!("{base}...{tip}"),
            None => String::new(),
        };
        let files = if range_arg.is_empty() {
            Vec::new()
        } else {
            diff_range_files(root, &range_arg, path)
        };
        Ok(CompareSide {
            commits,
            truncated,
            files,
        })
    };
    let left_side = build(left, format!("{right}..{left}"), left_path)?;
    let right_side = build(right, format!("{left}..{right}"), right_path)?;
    Ok(CompareBranches {
        left: left_side,
        right: right_side,
        merge_base: base,
    })
}

/// 单个提交的变更文件（`git show --numstat/--name-status`，对齐稳定版
/// git_commit_files）。
pub fn commit_files(root: &Path, commit: &str) -> Result<Vec<CompareFile>> {
    let numstat = run_args(
        root,
        &[
            "show",
            "--no-renames",
            "--no-color",
            "--numstat",
            "--format=",
        ],
        &[commit.to_string()],
    )?;
    let name_status = run_args(
        root,
        &[
            "show",
            "--no-renames",
            "--no-color",
            "--name-status",
            "--format=",
        ],
        &[commit.to_string()],
    );
    let name_status = match name_status {
        Ok(text) if !text.trim().is_empty() => text,
        // 根提交的 show 也可用；仍失败则按 merge-base 语义退化为空。
        _ => String::new(),
    };
    Ok(merge_file_lists(&numstat, &name_status))
}

/// 两个 revision 之间的变更文件（二点范围，用于详情列多选范围 diff）。
pub fn compare_commits(root: &Path, base: &str, tip: &str) -> Result<Vec<CompareFile>> {
    Ok(diff_range_files(root, &format!("{base}..{tip}"), None))
}
