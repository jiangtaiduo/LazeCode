//! Stash and shelves ("贮藏架"). Stash rides the porcelain (`git stash`);
//! shelves are LazeCode-owned patch files under `.git/lazecode-shelves/`,
//! applicable later (or partially) via `git apply`.

use std::path::Path;

use anyhow::anyhow;

use crate::{discover, Result};

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

// ---------------- stash ----------------

#[derive(Clone, Debug)]
pub struct StashEntry {
    /// e.g. `stash@{0}`.
    pub name: String,
    pub subject: String,
}

pub fn stash_list(root: &Path) -> Result<Vec<StashEntry>> {
    let output = run(root, &["stash", "list", "--format=%gd%x09%gs"])?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let (name, subject) = line.split_once('\t')?;
            Some(StashEntry {
                name: name.to_string(),
                subject: subject.to_string(),
            })
        })
        .collect())
}

pub fn stash_push(
    root: &Path,
    message: Option<&str>,
    keep_index: bool,
    include_untracked: bool,
) -> Result<String> {
    let mut args: Vec<&str> = vec!["stash", "push"];
    if keep_index {
        args.push("--keep-index");
    }
    if include_untracked {
        // `git stash -u`：未跟踪文件一并入栈，对齐 IDE「Include untracked
        // files」选项；stash push 默认不触碰 untracked。
        args.push("-u");
    }
    if let Some(message) = message.map(str::trim).filter(|m| !m.is_empty()) {
        args.push("-m");
        args.push(message);
    }
    run(root, &args)
}

/// Files touched by a stash entry, for previewing before applying.
/// `--include-untracked` 让 `-u` 贮藏的未跟踪文件也出现在清单里。
pub fn stash_files(root: &Path, stash: &str) -> Result<Vec<String>> {
    let output = run(
        root,
        &[
            "stash",
            "show",
            "--include-untracked",
            "--name-only",
            "--format=",
            stash,
        ],
    )?;
    Ok(output.lines().map(str::to_string).collect())
}

pub fn stash_apply(root: &Path, stash: &str) -> Result<String> {
    run(root, &["stash", "apply", stash])
}

pub fn stash_pop(root: &Path, stash: &str) -> Result<String> {
    run(root, &["stash", "pop", stash])
}

pub fn stash_drop(root: &Path, stash: &str) -> Result<String> {
    run(root, &["stash", "drop", stash])
}

// ---------------- shelves ----------------

pub const SHELVES_DIR: &str = "lazecode-shelves";

#[derive(Clone, Debug)]
pub struct ShelfEntry {
    pub name: String,
    /// The unified patch text of the shelf.
    pub patch: String,
    pub created: u64,
}

fn shelves_dir(root: &Path) -> std::path::PathBuf {
    match discover(root) {
        Ok(repo) => repo.git_dir().join(SHELVES_DIR),
        Err(_) => Path::new(".git").join(SHELVES_DIR),
    }
}

pub fn shelf_list(root: &Path) -> Result<Vec<ShelfEntry>> {
    let dir = shelves_dir(root);
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(out),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("patch") {
            continue;
        }
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let patch = std::fs::read_to_string(&path).unwrap_or_default();
        let created = entry
            .metadata()
            .ok()
            .and_then(|meta| meta.created().ok())
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        out.push(ShelfEntry {
            name,
            patch,
            created,
        });
    }
    out.sort_by_key(|entry| std::cmp::Reverse(entry.created));
    Ok(out)
}

/// Shelve the current tracked/untracked changes as a named patch in the
/// repo's private store; equivalent of IDE "shelve changes".
///
/// `paths` 为空时等价于「架藏全部已跟踪变更」（旧 `shelf_save` 语义）；
/// 否则只生成并撤回这些路径的变更（含未跟踪文件，对齐稳定版「只架藏
/// 勾选文件」的行为）。撤回复用 `discard_file_changes`：它对 HEAD 中存在
/// 的文件走 `checkout HEAD --`（索引+工作区同时复位，避免只 `checkout --`
/// 时暂存区残留），对新增文件删盘并清索引。
pub fn shelf_save(root: &Path, name: &str) -> Result<()> {
    shelf_save_paths(root, name, &[])
}

pub fn shelf_save_paths(root: &Path, name: &str, paths: &[String]) -> Result<()> {
    let name = name.trim();
    if name.is_empty() || name.contains("..") || name.contains('/') {
        anyhow::bail!("unsuitable shelf name '{name}'");
    }
    let output = if paths.is_empty() {
        // 全树架藏：`git diff HEAD` 只覆盖已跟踪变更，未跟踪新文件必须
        // 单独以 --no-index 补进补丁（否则架藏后它们仍散落在工作区）。
        let tracked = run(root, &["-c", "core.quotepath=false", "diff", "HEAD"])?;
        let mut all = patch_paths(&tracked);
        all.extend(list_untracked(root));
        if all.is_empty() {
            anyhow::bail!("nothing to shelve (no tracked changes)");
        }
        diff_patch_paths(root, &all)?
    } else {
        diff_patch_paths(root, paths)?
    };
    if output.trim().is_empty() {
        anyhow::bail!("nothing to shelve (no tracked changes)");
    }
    let mut output = output;
    if !output.ends_with('\n') {
        output.push('\n');
    }
    let dir = shelves_dir(root);
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(format!("{name}.patch"));
    if file.exists() {
        anyhow::bail!("shelf '{name}' already exists");
    }
    lazecode_fs::file::write_atomic(&file, output.as_bytes())
        .map_err(|err| anyhow!("cannot write shelf: {err}"))?;
    // Retract exactly the shelved paths (patch paths when shelving all) so
    // the worktree no longer shows them. Reuse the single-file discard so
    // staged-new files are removed from disk *and* index.
    let retract: Vec<String> = if paths.is_empty() {
        patch_paths(&output)
    } else {
        paths.to_vec()
    };
    let mut first_error: Option<anyhow::Error> = None;
    for path in &retract {
        if let Err(err) = crate::ops::discard_file_changes(root, path) {
            // 部分撤回失败时不中断其余路径（与稳定版逐文件回滚一致），
            // 但必须把首错误暴露给调用方，避免「看起来架藏成功」。
            if first_error.is_none() {
                first_error = Some(err);
            }
        }
    }
    if let Some(err) = first_error {
        return Err(err);
    }
    Ok(())
}

/// Unified patch limited to `paths`: `git diff HEAD -- paths` for tracked
/// entries plus a `--no-index` pseudo-diff against /dev/null for each
/// untracked file (they never appear in `git diff`). Port of the stable
/// version's patch builder used by shelve/copy-patch flows.
pub fn diff_patch_paths(root: &Path, paths: &[String]) -> Result<String> {
    let cleaned: Vec<&str> = paths
        .iter()
        .map(|path| path.trim())
        .filter(|path| !path.is_empty())
        .collect();
    if cleaned.is_empty() {
        return Ok(String::new());
    }
    let mut tracked_args: Vec<&str> = vec![
        "-c",
        "core.quotepath=false",
        "diff",
        "--no-color",
        "--no-ext-diff",
        "HEAD",
        "--",
    ];
    tracked_args.extend(cleaned.iter().copied());
    let output = run(root, &tracked_args)?;
    let mut patch = output;

    // 未跟踪文件：一次 `git ls-files -z` 批量判定已跟踪集合（稳定版的
    // 优化：避免逐文件 spawn），差集中的普通文件逐个生成 /dev/null 补丁。
    let mut ls_args: Vec<&str> = vec!["-c", "core.quotepath=false", "ls-files", "-z", "--"];
    ls_args.extend(cleaned.iter().copied());
    let tracked_out = run_raw(root, &ls_args)?;
    let tracked: std::collections::HashSet<String> = tracked_out
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            entry
                .trim_start_matches('"')
                .trim_end_matches('"')
                .to_string()
        })
        .collect();
    for path in cleaned {
        if tracked.contains(path) || !root.join(path).is_file() {
            continue;
        }
        // --no-index 有差异时退出码为 1，属正常输出路径。
        if let Ok(text) = run_diff_allow_one(
            root,
            &[
                "-c",
                "core.quotepath=false",
                "diff",
                "--no-color",
                "--no-ext-diff",
                "--no-index",
                "--",
                "/dev/null",
                path,
            ],
        ) {
            // `run` 会 trim 尾部换行；补丁节之间必须以换行分隔，
            // 否则 git apply 报 "corrupt patch"。
            if !patch.is_empty() && !patch.ends_with('\n') {
                patch.push('\n');
            }
            patch.push_str(&text);
        }
    }
    if !patch.is_empty() && !patch.ends_with('\n') {
        patch.push('\n');
    }
    Ok(patch)
}

/// Paths touched by a shelf patch (the `b/` side of each `diff --git`
/// header), used by the unshelve dialog for per-file selection.
pub fn shelf_files(root: &Path, name: &str) -> Result<Vec<String>> {
    let patch = shelf_patch(root, name)?;
    Ok(patch_paths(&patch))
}

/// Extract the new-side path of every file section from a unified patch.
/// `--no-index` creation sections carry `--- /dev/null`; the `diff --git`
/// header still names the real path on both sides, so the b-side parse
/// covers them uniformly.
fn patch_paths(patch: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("diff --git a/") {
            if let Some((_, right)) = rest.split_once(" b/") {
                paths.push(right.to_string());
            }
        }
    }
    paths
}

fn run_raw(root: &Path, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|err| anyhow!("git spawn failed: {err}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `--no-index` 差异比较以退出码 1 表示「有差异」，属预期结果；
/// 只有其他码才算失败（对齐稳定版 run_git_diff 的语义）。
fn run_diff_allow_one(root: &Path, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|err| anyhow!("git spawn failed: {err}"))?;
    let code = output.status.code();
    if !output.status.success() && code != Some(1) {
        return Err(anyhow!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// 未跟踪文件清单（respect gitignore），供全树架藏补齐 diff 看不到的
/// 新文件。
fn list_untracked(root: &Path) -> Vec<String> {
    match run_raw(
        root,
        &[
            "-c",
            "core.quotepath=false",
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
        ],
    ) {
        Ok(out) => out
            .split('\0')
            .filter(|entry| !entry.is_empty() && root.join(entry).is_file())
            .map(str::to_string)
            .collect(),
        Err(_) => Vec::new(),
    }
}

pub fn shelf_delete(root: &Path, name: &str) -> Result<()> {
    let file = shelves_dir(root).join(format!("{}.patch", name.trim()));
    std::fs::remove_file(&file).map_err(|err| anyhow!("cannot delete shelf: {err}"))
}

pub fn shelf_patch(root: &Path, name: &str) -> Result<String> {
    let file = shelves_dir(root).join(format!("{}.patch", name.trim()));
    std::fs::read_to_string(&file).map_err(|err| anyhow!("cannot read shelf: {err}"))
}

/// Apply the full (or index-only) patch of a shelf back onto the worktree.
pub fn shelf_restore(root: &Path, name: &str, delete_after: bool) -> Result<()> {
    let patch = shelf_patch(root, name)?;
    crate::apply_patch(root, &patch, false, false)?;
    if delete_after {
        shelf_delete(root, name)?;
    }
    Ok(())
}

/// Apply only the hunks of the shelf that touch `paths` (partial apply).
pub fn shelf_restore_partial(root: &Path, name: &str, paths: &[String]) -> Result<()> {
    let mut patch = shelf_patch(root, name)?;
    if !patch.ends_with('\n') {
        patch.push('\n');
    }
    // git apply supports path selection below `--include`/`--exclude`.
    let mut args: Vec<&str> = vec!["apply", "--whitespace=nowarn", "--check"];
    for path in paths {
        args.push("--include");
        args.push(path);
    }
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(root).args(&args).arg("-");
    use std::io::Write;
    let mut child = cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .expect("piped")
        .write_all(patch.as_bytes())?;
    let check = child.wait_with_output()?;
    if !check.status.success() {
        return Err(anyhow!(
            "partial apply does not match: {}",
            String::from_utf8_lossy(&check.stderr).trim()
        ));
    }
    // Checked, now really apply.
    let mut args: Vec<&str> = vec!["apply", "--whitespace=nowarn"];
    for path in paths {
        args.push("--include");
        args.push(path);
    }
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(root).args(&args).arg("-");
    let mut child = cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .expect("piped")
        .write_all(patch.as_bytes())?;
    let applied = child.wait_with_output()?;
    if !applied.status.success() {
        return Err(anyhow!(
            "partial apply failed: {}",
            String::from_utf8_lossy(&applied.stderr).trim()
        ));
    }
    Ok(())
}
