//! Remote sync: push, fetch, clone and fast-forward-only pull use native gix
//! transport; pull merge/rebase still uses `git`.

use std::path::{Path, PathBuf};

use anyhow::anyhow;
use gix::refs::transaction::{PreviousValue, RefEdit};

use crate::{discover, Result};

/// Run the `git` binary in `root` (non-interactive) and return trimmed
/// stdout. Shared with the push/fetch transports.
pub(crate) fn run_git(root: &Path, args: &[&str]) -> Result<String> {
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
    let output = run_git(root, &["remote", "-v"])?;
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
    run_git(root, &["remote", "add", name, url]).map(drop)
}

pub fn remote_remove(root: &Path, name: &str) -> Result<()> {
    run_git(root, &["remote", "remove", name]).map(drop)
}

pub fn remote_rename(root: &Path, from: &str, to: &str) -> Result<()> {
    if to.contains(' ') || to.starts_with('-') {
        anyhow::bail!("unsuitable remote name '{to}'");
    }
    run_git(root, &["remote", "rename", from, to]).map(drop)
}

pub fn remote_set_url(root: &Path, name: &str, url: &str) -> Result<()> {
    run_git(root, &["remote", "set-url", name, url]).map(drop)
}

// ---------------- sync operations ----------------

/// Push the current branch via native receive-pack. `force` skips the local
/// fast-forward check but still sends the last known remote oid
/// (force-with-lease).
pub fn push(
    root: &Path,
    remote: &str,
    branch: &str,
    set_upstream: bool,
    force: bool,
) -> Result<String> {
    crate::push::push(root, remote, branch, set_upstream, force)
}

/// Commits on `branch` after `upstream` (or the newest commits when no
/// upstream is configured), used by the Push dialog.
pub fn pending_commits(
    root: &Path,
    upstream: Option<&str>,
    branch: &str,
    limit: usize,
) -> Result<Vec<crate::CommitInfo>> {
    use crate::LogOptions;
    let range = upstream
        .filter(|upstream| !upstream.trim().is_empty())
        .map(|upstream| format!("{upstream}..{branch}"))
        .unwrap_or_else(|| branch.to_string());
    crate::log(
        root,
        LogOptions {
            limit,
            branch: Some(range),
            topo_order: false,
            ..Default::default()
        },
    )
}

/// Delete a remote branch (`full_name` is e.g. `origin/docs-only`) and its
/// tracking ref after the server confirms.
pub fn delete_remote_branch(root: &Path, full_name: &str) -> Result<String> {
    let (remote, branch) = full_name
        .split_once('/')
        .ok_or_else(|| anyhow!("remote branch must be '<remote>/<branch>'"))?;
    if remote.is_empty()
        || branch.is_empty()
        || remote.starts_with('-')
        || branch.starts_with('-')
        || remote.chars().any(char::is_whitespace)
        || branch.chars().any(char::is_whitespace)
    {
        anyhow::bail!("unsuitable remote branch name '{full_name}'");
    }
    let output = crate::porcelain(root)
        .args(["push", remote, "--delete", branch])
        .output()
        .map_err(|err| anyhow!("git push --delete failed to spawn: {err}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let repo = discover(root)?;
    let tracking = format!("refs/remotes/{full_name}");
    if let Ok(reference) = repo.find_reference(tracking.as_str()) {
        if let Some(target) = reference.target().try_id().map(|id| id.to_owned()) {
            let edit = RefEdit::delete(
                tracking.as_str().try_into()?,
                PreviousValue::MustExistAndMatch(gix::refs::Target::Object(target)),
            );
            let _ = repo.edit_reference(edit);
        }
    }
    Ok(format!("Deleted remote branch {full_name}"))
}

/// Push every local tag to `remote`.
pub fn push_tags(root: &Path, remote: &str) -> Result<String> {
    if remote.is_empty() || remote.starts_with('-') || remote.chars().any(char::is_whitespace) {
        anyhow::bail!("unsuitable remote name '{remote}'");
    }
    run_git(root, &["push", remote, "--tags"])
}

/// Push one local tag to `remote`.
pub fn push_tag(root: &Path, remote: &str, tag: &str) -> Result<String> {
    if remote.is_empty() || remote.starts_with('-') || remote.chars().any(char::is_whitespace) {
        anyhow::bail!("unsuitable remote name '{remote}'");
    }
    if tag.is_empty() || tag.starts_with('-') || tag.chars().any(char::is_whitespace) {
        anyhow::bail!("unsuitable tag name '{tag}'");
    }
    run_git(root, &["push", remote, &format!("refs/tags/{tag}")])
}

/// Fetch all remotes and prune stale remote-tracking branches.
///
/// Uses the `git` binary: gix's fetch is single-threaded while indexing the
/// received pack on this build (no `gix` "parallel" feature), which is much
/// slower than native git on large repositories.
pub fn fetch_all(root: &Path) -> Result<String> {
    run_git(root, &["fetch", "--all", "--prune"])?;
    let names = discover(root)?
        .remote_names()
        .into_iter()
        .map(|name| name.to_string())
        .collect::<Vec<_>>();
    Ok(names
        .iter()
        .map(|name| format!("{name}: fetched"))
        .collect::<Vec<_>>()
        .join("\n"))
}

/// Fetch one remote through the native transport (used by fast-forward pull,
/// which then re-reads the refs the git process just updated).
fn fetch_remote(root: &Path, name: &str) -> Result<()> {
    if name.is_empty() || name.starts_with('-') || name.chars().any(char::is_whitespace) {
        anyhow::bail!("unsuitable remote name '{name}'");
    }
    run_git(root, &["fetch", "--prune", name]).map(drop)
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
    fetch_remote(root, &remote)?;
    // The git process updated the refs; drop the handle so the ref cache
    // below reflects what it wrote.
    let repo = discover(root)?;
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
///
/// Native git: the same pack-indexing cost as fetch made gix clones of large
/// repositories much slower than the `git` binary.
pub fn clone(url: &str, destination: &Path) -> Result<PathBuf> {
    if destination.exists() {
        let empty = std::fs::read_dir(destination)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        if !empty {
            anyhow::bail!("{} already exists and is not empty", destination.display());
        }
    }
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    let target = destination
        .file_name()
        .ok_or_else(|| anyhow!("clone destination has no directory name"))?;
    let output = crate::porcelain(parent)
        .args(["clone", "--"])
        .arg(url)
        .arg(target)
        .output()
        .map_err(|err| anyhow!("git clone failed to spawn: {err}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(destination.to_path_buf())
}

/// Commits not on any remote-tracking branch (`git log @{u}..` estimate).
pub fn pending_count(root: &Path, branch: &str) -> Result<usize> {
    crate::status::pending_count(root, branch)
}
