//! Branch push: a local fast-forward guard plus the native `git push`
//! transport.
//!
//! The previous implementation hand-built a pack from every object reachable
//! from the tip, compressed each object independently (no deltas) and uploaded
//! it without knowing what the remote already had. On real repositories that
//! uploads the entire history on every push and is orders of magnitude slower
//! than git's negotiated, delta-compressed transfer — so transfers now go
//! through the `git` binary like pull/merge/tags already do.

use std::path::Path;

use anyhow::anyhow;
use gix::hash::ObjectId;

use crate::{discover, Result};

pub(crate) fn write_upstream(repo: &gix::Repository, branch: &str, remote: &str) -> Result<()> {
    let path = repo.common_dir().join("config");
    let mut config =
        gix::config::File::from_path_no_includes(path.clone(), gix::config::Source::Local)?;
    config.set_raw_value_by(
        "branch",
        Some(gix::bstr::BStr::new(branch)),
        "remote",
        remote,
    )?;
    config.set_raw_value_by(
        "branch",
        Some(gix::bstr::BStr::new(branch)),
        "merge",
        format!("refs/heads/{branch}"),
    )?;
    let mut bytes = Vec::new();
    config.write_to(&mut bytes)?;
    lazecode_fs::file::write_atomic(&path, &bytes)?;
    Ok(())
}

pub fn push(
    root: &Path,
    remote_name: &str,
    branch: &str,
    set_upstream: bool,
    force: bool,
) -> Result<String> {
    // 本地快进检查（与稳定版 push 对话框一致，先给出可读的拒绝原因）。
    let repo = discover(root)?;
    let new: ObjectId = repo
        .find_reference(format!("refs/heads/{branch}").as_str())
        .map_err(|_| anyhow!("branch '{branch}' does not exist"))?
        .target()
        .try_id()
        .map(|id| id.to_owned())
        .ok_or_else(|| anyhow!("branch '{branch}' has no target"))?;
    let tracking = format!("refs/remotes/{remote_name}/{branch}");
    let old = repo
        .find_reference(tracking.as_str())
        .ok()
        .and_then(|reference| reference.target().try_id().map(|id| id.to_owned()));
    if !force {
        if let Some(old) = old {
            if repo.merge_base(old, new)?.map(|id| id.detach()) != Some(old) {
                anyhow::bail!("push rejected: non-fast-forward (fetch or force first)");
            }
        }
    }
    drop(repo);

    // `--force-with-lease` keeps the "expected old oid" semantics the
    // hand-written transport had (it leased the last known tracking ref).
    let mut args: Vec<String> = vec!["push".into()];
    if set_upstream {
        args.push("--set-upstream".into());
    }
    if force {
        args.push("--force-with-lease".into());
    }
    args.push(remote_name.to_string());
    args.push(format!("refs/heads/{branch}:refs/heads/{branch}"));
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    crate::sync::run_git(root, &argv)?;
    Ok(format!("pushed {branch} to {remote_name}"))
}
