//! Remote sync. Push, fetch, clone and fast-forward-only pull use native gix
//! transport (push speaks receive-pack directly); pull merge/rebase still
//! uses the `git` porcelain until the merge sequencer lands.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use anyhow::anyhow;
use gix::refs::transaction::{PreviousValue, RefEdit};
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

/// Push the current branch to `remote` using the native receive-pack
/// protocol. `force` bypasses the client-side fast-forward check; the command
/// still carries the last known remote oid, so a changed remote is rejected by
/// the server (force-with-lease semantics).
pub fn push(
    root: &Path,
    remote: &str,
    branch: &str,
    set_upstream: bool,
    force: bool,
) -> Result<String> {
    crate::push::push(root, remote, branch, set_upstream, force)
}

/// Fetch all remotes and prune stale remote-tracking branches.
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

fn prune_remote_tracking(
    repo: &gix::Repository,
    remote: &str,
    keep: &HashSet<String>,
) -> Result<usize> {
    let prefix = format!("refs/remotes/{remote}/");
    let mut stale = Vec::new();
    for reference in repo.references()?.all()? {
        let reference = reference?;
        let name = reference.name().as_bstr().to_string();
        if !name.starts_with(&prefix) || keep.contains(&name) {
            continue;
        }
        let Some(target) = reference.target().try_id().map(|id| id.to_owned()) else {
            continue;
        };
        stale.push((name, target));
    }
    let mut removed = 0;
    for (name, target) in stale {
        let edit = RefEdit::delete(
            name.as_str().try_into()?,
            PreviousValue::MustExistAndMatch(gix::refs::Target::Object(target)),
        );
        repo.edit_reference(edit)?;
        removed += 1;
    }
    Ok(removed)
}

fn fetch_remote(repo: &gix::Repository, name: &str) -> Result<()> {
    let remote = repo.find_remote(name)?;
    let connection = remote.connect(Direction::Fetch)?;
    let prepare = connection.prepare_fetch(
        gix::progress::Discard,
        gix::remote::ref_map::Options::default(),
    )?;
    let outcome = prepare.receive(gix::progress::Discard, &AtomicBool::new(false))?;
    let keep: HashSet<String> = outcome
        .ref_map
        .mappings
        .iter()
        .filter_map(|mapping| mapping.local.as_ref())
        .map(|local| local.to_string())
        .collect();
    let _ = prune_remote_tracking(repo, name, &keep)?;
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
