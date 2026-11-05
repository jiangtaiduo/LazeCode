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

pub fn stash_push(root: &Path, message: Option<&str>, keep_index: bool) -> Result<String> {
    let mut args: Vec<&str> = vec!["stash", "push"];
    if keep_index {
        args.push("--keep-index");
    }
    if let Some(message) = message.map(str::trim).filter(|m| !m.is_empty()) {
        args.push("-m");
        args.push(message);
    }
    run(root, &args)
}

/// Files touched by a stash entry, for previewing before applying.
pub fn stash_files(root: &Path, stash: &str) -> Result<Vec<String>> {
    let output = run(root, &["stash", "show", "--name-only", "--format=", stash])?;
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
pub fn shelf_save(root: &Path, name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() || name.contains("..") || name.contains('/') {
        anyhow::bail!("unsuitable shelf name '{name}'");
    }
    // Generate a unified patch from the worktree against HEAD (tracked files).
    let output = run(root, &["diff", "HEAD"])?;
    if output.trim().is_empty() {
        anyhow::bail!("nothing to shelve (no tracked changes)");
    }
    let dir = shelves_dir(root);
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(format!("{name}.patch"));
    if file.exists() {
        anyhow::bail!("shelf '{name}' already exists");
    }
    lazecode_fs::file::write_atomic(&file, output.as_bytes())
        .map_err(|err| anyhow!("cannot write shelf: {err}"))?;
    // Detect new-file sections (`--- /dev/null`) so their paths can be
    // removed below; `git checkout --` cannot revert untracked files.
    let new_paths: Vec<String> = {
        let mut paths = Vec::new();
        let mut current: Option<String> = None;
        let mut current_is_new = false;
        for line in output.lines() {
            if let Some(rest) = line.strip_prefix("diff --git a/") {
                if current_is_new {
                    if let Some(path) = &current {
                        paths.push(path.clone());
                    }
                }
                current = rest.split_once(" b/").map(|(_, right)| right.to_string());
                current_is_new = false;
            } else if line.starts_with("new file mode") {
                current_is_new = true;
            }
        }
        if current_is_new {
            if let Some(path) = current {
                paths.push(path);
            }
        }
        paths
    };
    // Roll the changes back out of the worktree so the tree is clean.
    run(root, &["checkout", "--", "."])?;
    for path in new_paths {
        let absolute = root.join(&path);
        if let Err(err) = std::fs::remove_file(&absolute) {
            if err.kind() != std::io::ErrorKind::NotFound {
                return Err(anyhow!("cannot remove shelved new file {path}: {err}"));
            }
        }
    }
    Ok(())
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
    eprintln!(
        "SHELF_RESTORE {name}: patch bytes={};\n{patch}\n===END===",
        patch.len()
    );
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
