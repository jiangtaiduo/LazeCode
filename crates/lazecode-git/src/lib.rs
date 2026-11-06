//! Pure-Rust Git integration built on `gix`.
//!
//! Read paths (status, log, branches, diffs) are implemented from Git
//! primitives; write paths (stage, unstage, commit, branch switch) respect the
//! index lock and explicitly refuse index formats whose extensions `gix-index`
//! cannot preserve.

pub mod changelists;
pub mod dag;
pub mod diff;
pub mod history;
pub mod introspect;
pub mod log;
pub mod model;
pub mod ops;
pub mod shelf;
pub mod status;
pub mod sync;
pub mod tree;
pub mod worktree;

pub use changelists::Changelists;
pub use dag::*;
pub use diff::{
    commit_details, commit_file_diff, diff_commits, diff_ref_worktree, diff_two_files, file_diff,
};
pub use history::{
    cherry_pick, cherry_pick_abort, cherry_pick_continue, conflict_list, drop_commit,
    mark_conflict_resolved, merge, merge_abort, merge_continue, merge_file_content, rebase,
    rebase_abort, rebase_continue, rebase_skip, require_clean, reset, resolve_take, revert,
    sequencer_state, squash_commits, ConflictEntry, MergeFile3Way, ResetMode, SequencerState,
};
pub use introspect::{
    blame, compare_branches, compare_commits, file_history, lfs_pull, lfs_status, lfs_track,
    submodule_list, BlameLine, CompareSide, HistoryEntry, LfsStatus, SubmoduleEntry,
};
pub use log::{branches, head_ancestors, log, log_ref, tags};
pub use model::*;
pub use ops::{
    apply_patch, checkout_branch, commit, commit_checked, create_branch, create_tag, delete_branch,
    delete_tag, rename_branch, stage, unstage,
};
pub use shelf::{
    shelf_delete, shelf_list, shelf_patch, shelf_restore, shelf_restore_partial, shelf_save,
    stash_apply, stash_drop, stash_files, stash_list, stash_pop, stash_push, ShelfEntry,
    StashEntry,
};
pub use status::snapshot;
pub use sync::{
    clone, fetch_all, pending_count, pull, push, remote_add, remote_list, remote_remove,
    remote_rename, remote_set_url, PullMode, RemoteInfo,
};
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
