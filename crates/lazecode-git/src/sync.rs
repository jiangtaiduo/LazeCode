//! Remote sync over the `git` porcelain binary. gix 0.90 has no push/merge
//! drivers; the plumbing stays the same, only the transport/outcome differs.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::anyhow;

use crate::Result;

fn run(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|err| anyhow!("git {:?} failed to spawn: {err}", args.first()))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

// ---------------- remote management ----------------

#[derive(Clone, Debug)]
pub struct RemoteInfo {
    pub name: String,
    /// Fetch URL (first of many).
    pub url: String,
}

pub fn remote_list(root: &Path) -> Result<Vec<RemoteInfo>> {
    let output = run(root, &["remote", "-v"])?;
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for line in output.lines() {
        // "<name>\t<url> (fetch)"
        let Some((name, rest)) = line.split_once('\t') else {
            continue;
        };
        let url = rest.trim().trim_end_matches(" (fetch)").trim();
        if seen.insert(name.to_string()) {
            out.push(RemoteInfo {
                name: name.to_string(),
                url: url.to_string(),
            });
        }
    }
    Ok(out)
}

pub fn remote_add(root: &Path, name: &str, url: &str) -> Result<()> {
    if name.contains(' ') || name.starts_with('-') {
        anyhow::bail!("unsuitable remote name '{name}'");
    }
    run(root, &["remote", "add", name, url]).map(drop)
}

pub fn remote_remove(root: &Path, name: &str) -> Result<()> {
    run(root, &["remote", "remove", name]).map(drop)
}

pub fn remote_rename(root: &Path, from: &str, to: &str) -> Result<()> {
    if to.contains(' ') || to.starts_with('-') {
        anyhow::bail!("unsuitable remote name '{to}'");
    }
    run(root, &["remote", "rename", from, to]).map(drop)
}

pub fn remote_set_url(root: &Path, name: &str, url: &str) -> Result<()> {
    run(root, &["remote", "set-url", name, url]).map(drop)
}

// ---------------- sync operations ----------------

/// Push the current branch to `remote`, creating the upstream ref when
/// `set_upstream` is set. Returns the command's stderr text (for refush).
pub fn push(
    root: &Path,
    remote: &str,
    branch: &str,
    set_upstream: bool,
    force: bool,
) -> Result<String> {
    let mut args = vec!["push".to_string()];
    if force {
        args.push("--force-with-lease".into());
    }
    if set_upstream {
        args.push("--set-upstream".into());
    }
    args.push(remote.to_string());
    args.push(format!("refs/heads/{branch}:refs/heads/{branch}"));
    let output = Command::new("git")
        .current_dir(root)
        .args(&args)
        .output()
        .map_err(|err| anyhow!("git push spawn failed: {err}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stderr).trim().to_string())
}

/// Fetch all remotes and prune stale remote-tracking branches.
pub fn fetch_all(root: &Path) -> Result<String> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["fetch", "--all", "--prune"])
        .output()
        .map_err(|err| anyhow!("git fetch spawn failed: {err}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stderr).trim().to_string())
}

/// Pull the current branch's upstream using the requested mode.
pub fn pull(root: &Path, mode: PullMode) -> Result<String> {
    let mut args: Vec<String> = vec!["pull".into()];
    args.push(match mode {
        PullMode::Rebase => "--rebase".into(),
        PullMode::Merge => "--no-rebase".into(),
        PullMode::FastForwardOnly => "--ff-only".into(),
    });
    let output = Command::new("git")
        .current_dir(root)
        .args(&args)
        .output()
        .map_err(|err| anyhow!("git pull spawn failed: {err}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stderr).trim().to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PullMode {
    Merge,
    Rebase,
    FastForwardOnly,
}

/// Clone `url` into `destination` (directory must not exist or be empty).
pub fn clone(url: &str, destination: &Path) -> Result<PathBuf> {
    if destination.exists() {
        let empty = std::fs::read_dir(destination)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        if !empty {
            anyhow::bail!("{} already exists and is not empty", destination.display());
        }
    }
    let output = Command::new("git")
        .arg("clone")
        .arg(url)
        .arg(destination)
        .output()
        .map_err(|err| anyhow!("git clone spawn failed: {err}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(destination.to_path_buf())
}

/// Commits on the current branch that no remote-tracking branch contains
/// (rough `git log @{u}..` estimate — falls back to counting commits not on
/// origin/<branch>).
pub fn pending_count(root: &Path, branch: &str) -> Result<usize> {
    let upstream = format!("origin/{branch}");
    run(
        root,
        &[
            "rev-list",
            "--count",
            &format!("{upstream}..{branch}"),
            "--",
        ],
    )
    .and_then(|text| {
        text.parse::<usize>()
            .map_err(|err| anyhow!("pending count: {err}"))
    })
}
