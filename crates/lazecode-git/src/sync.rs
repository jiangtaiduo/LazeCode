//! Remote sync. Fetch, clone and fast-forward-only pull use gix's blocking
//! transport; push and pull merge/rebase still use the `git` porcelain until
//! gix grows a send-pack driver and a merge sequencer.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use anyhow::anyhow;
use gix::remote::Direction;

use crate::{discover, Result};

fn run(root: &Path, args: &[&str]) -> Result<String> {
    let output = crate::porcelain(root)
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
    let output = crate::porcelain(root)
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

/// Fetch all remotes. Native gix blocking transport; stale remote-tracking
/// branches are left in place (prune is not part of the gix fetch API).
pub fn fetch_all(root: &Path) -> Result<String> {
    let repo = discover(root)?;
    let names: Vec<String> = repo
        .remote_names()
        .into_iter()
        .map(|name| name.to_string())
        .collect();
    for name in &names {
        fetch_remote(&repo, name)?;
    }
    Ok(names
        .iter()
        .map(|name| format!("{name}: fetched"))
        .collect::<Vec<_>>()
        .join("\n"))
}

fn fetch_remote(repo: &gix::Repository, name: &str) -> Result<()> {
    let remote = repo.find_remote(name)?;
    let connection = remote.connect(Direction::Fetch)?;
    let prepare = connection.prepare_fetch(
        gix::progress::Discard,
        gix::remote::ref_map::Options::default(),
    )?;
    let _outcome = prepare.receive(gix::progress::Discard, &AtomicBool::new(false))?;
    Ok(())
}

/// Pull the current branch's upstream using the requested mode.
pub fn pull(root: &Path, mode: PullMode) -> Result<String> {
    if mode == PullMode::FastForwardOnly {
        return pull_fast_forward(root);
    }
    let mut args: Vec<String> = vec!["pull".into()];
    args.push(match mode {
        PullMode::Rebase => "--rebase".into(),
        PullMode::Merge => "--no-rebase".into(),
        PullMode::FastForwardOnly => unreachable!(),
    });
    let output = crate::porcelain(root)
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

fn pull_fast_forward(root: &Path) -> Result<String> {
    let repo = discover(root)?;
    let head = repo.head()?;
    if head.is_unborn() || head.is_detached() {
        anyhow::bail!("fast-forward pull requires a checked-out branch");
    }
    let branch = head
        .referent_name()
        .map(|name| name.shorten().to_string())
        .unwrap_or_default();
    let config = repo.config_snapshot();
    let remote = config
        .string(&format!("branch.{branch}.remote"))
        .map(|value| value.to_string())
        .ok_or_else(|| anyhow!("branch '{branch}' has no upstream remote"))?;
    let merge = config
        .string(&format!("branch.{branch}.merge"))
        .map(|value| value.to_string())
        .ok_or_else(|| anyhow!("branch '{branch}' has no upstream branch"))?;
    let leaf = merge.strip_prefix("refs/heads/").unwrap_or(&merge);
    fetch_remote(&repo, &remote)?;
    let upstream_ref = format!("refs/remotes/{remote}/{leaf}");
    let upstream = repo
        .find_reference(upstream_ref.as_str())
        .ok()
        .and_then(|reference| reference.target().try_id().map(|id| id.to_owned()))
        .ok_or_else(|| anyhow!("remote-tracking ref {upstream_ref} is missing"))?;
    let head_id = repo.head_id()?.detach();
    let base = repo
        .merge_base(head_id, upstream)?
        .ok_or_else(|| anyhow!("no common ancestor with upstream"))?
        .detach();
    if base != head_id {
        anyhow::bail!("cannot fast-forward: local branch has diverged");
    }
    crate::history::reset(root, &upstream.to_string(), crate::history::ResetMode::Hard)?;
    Ok("fast-forwarded".to_string())
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
    let mut prepare = gix::prepare_clone(url, destination)?
        .fetch_then_checkout(gix::progress::Discard, &AtomicBool::new(false))?
        .0;
    let _repo = prepare.main_worktree(gix::progress::Discard, &AtomicBool::new(false))?;
    Ok(destination.to_path_buf())
}

/// Commits on the current branch that no remote-tracking branch contains
/// (rough `git log @{u}..` estimate — falls back to counting commits not on
/// origin/<branch>).
pub fn pending_count(root: &Path, branch: &str) -> Result<usize> {
    crate::status::pending_count(root, branch)
}
