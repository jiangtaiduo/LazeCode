//! Pure-Rust Git integration built on `gix`.
//!
//! Read paths (status, log, branches, diffs) are implemented from Git
//! primitives; write paths (stage, unstage, commit, branch switch) respect the
//! index lock and explicitly refuse index formats whose extensions `gix-index`
//! cannot preserve.

pub mod diff;
pub mod log;
pub mod model;
pub mod ops;
pub mod status;
pub mod tree;
pub mod worktree;

pub use diff::{commit_details, file_diff};
pub use log::{branches, log, log_ref, tags};
pub use model::*;
pub use ops::{
    checkout_branch, commit, commit_checked, create_branch, delete_branch, stage, unstage,
};
pub use status::snapshot;
pub use worktree::{
    add_worktree, add_worktree_detached_at_head, is_linked_worktree, list_worktrees, main_worktree,
    prune_worktrees, remove_worktree,
};

use std::path::{Path, PathBuf};

pub type Result<T, E = anyhow::Error> = std::result::Result<T, E>;

/// Upper bound for a single blob read by the porcelain layer. Larger files are
/// reported as binary/size-unavailable instead of being loaded into memory.
pub const MAX_STAGE_BYTES: u64 = 256 * 1024 * 1024;

/// Open the repository containing `root`.
pub fn discover(root: &Path) -> Result<gix::Repository> {
    gix::discover(root).map_err(|err| anyhow::anyhow!("not a Git repository: {err}"))
}

pub fn workdir(repo: &gix::Repository) -> Result<PathBuf> {
    repo.workdir()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow::anyhow!("bare repositories are not supported"))
}

pub(crate) fn short_id(id: &gix::hash::ObjectId) -> String {
    let hex = id.to_string();
    hex.chars().take(8).collect()
}

/// Parse a user supplied object id or revspec (branch, tag, short hash).
pub(crate) fn resolve_commit(repo: &gix::Repository, spec: &str) -> Result<gix::ObjectId> {
    let id = if let Ok(id) = repo.rev_parse_single(spec) {
        id.detach()
    } else {
        // Fall back to an exact object id lookup for full hex ids.
        gix::hash::ObjectId::from_hex(spec.as_bytes())
            .map_err(|_| anyhow::anyhow!("unknown revision '{spec}'"))?
    };
    // Annotated tags resolve to the tag object; peel to the commit they name.
    let object = repo.find_object(id)?;
    Ok(object.peel_to_kind(gix::objs::Kind::Commit)?.id)
}
