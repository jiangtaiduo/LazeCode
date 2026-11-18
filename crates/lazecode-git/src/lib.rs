//! Pure-Rust Git integration on `gix`. Write paths respect the index lock and
//! refuse index formats gix cannot preserve.

pub mod changelists;
pub mod dag;
pub mod diff;
pub mod filter;
pub mod history;
pub mod hooks;
pub mod introspect;
pub mod log;
pub mod model;
pub mod ops;
pub mod patch;
pub mod push;
pub mod shelf;
pub mod status;
pub mod sync;
pub mod tree;
pub mod worktree;

/// A `git` process with the GUI-safety environment applied: no interactive
/// credential/host-key prompt, no editor, and its own process group so a
/// timeout/quit can terminate hooks and transports as a unit.
pub(crate) fn porcelain(root: &std::path::Path) -> std::process::Command {
    let mut command = std::process::Command::new("git");
    command
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("GIT_EDITOR", "true")
        .env("GIT_SEQUENCE_EDITOR", "true")
        .env("GIT_SSH_COMMAND", "ssh -oBatchMode=yes");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
}

pub use changelists::Changelists;
pub use dag::*;
pub use diff::{
    commit_details, commit_file_diff, diff_commits, diff_ref_worktree, diff_two_files, file_diff,
};
pub use history::{
    amend_commit_message, checkout_ref, checkout_revision, checkout_revision_path, cherry_pick,
    cherry_pick_abort, cherry_pick_continue, commit_patch, conflict_list, drop_commit,
    mark_conflict_resolved, merge, merge_abort, merge_continue, merge_file_content, rebase,
    rebase_abort, rebase_continue, rebase_interactive, rebase_skip, require_clean, reset,
    resolve_take, revert, revert_abort, revert_continue, sequencer_state, squash_commits,
    undo_commit_head, ConflictEntry, MergeFile3Way, RebaseStep, ResetMode, SequencerState,
};
pub use introspect::{
    blame, commit_files, compare_branches, compare_commits, file_history, lfs_pull, lfs_status,
    lfs_track, submodule_list, BlameLine, CompareBranches, CompareFile, CompareSide, HistoryEntry,
    LfsStatus, SubmoduleEntry,
};
pub use log::{branches, head_ancestors, log, log_ref, tags};
pub use model::*;
pub use ops::{
    apply_patch, checkout_branch, commit, commit_checked, create_branch, create_branch_from,
    create_tag, delete_branch, delete_tag, discard_file_changes, force_delete_branch, git_init,
    rename_branch, stage, unstage,
};
pub use shelf::{
    diff_patch_paths, shelf_delete, shelf_files, shelf_import_patch, shelf_list,
    shelf_list_deleted, shelf_patch, shelf_purge_deleted, shelf_rename, shelf_restore,
    shelf_restore_deleted, shelf_restore_partial, shelf_save, shelf_save_paths, stash_apply,
    stash_details, stash_drop, stash_file_diff, stash_files, stash_list, stash_pop, stash_push,
    ShelfEntry, StashDetails, StashEntry,
};
pub use status::{ignored_entries, ignored_files, invalidate_ignored_cache, snapshot};
pub use sync::{
    clone, delete_remote_branch, fetch_all, pending_commits, pending_count, pull, push, push_tag,
    push_tags, remote_add, remote_list, remote_remove, remote_rename, remote_set_url, PullMode,
    RemoteInfo,
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

/// Open options shared by every repository: an in-memory committer fallback so
/// clone/fetch work without `user.name` (real config still wins).
pub(crate) fn open_options() -> gix::open::Options {
    gix::open::Options::default().config_overrides([
        "gitoxide.committer.nameFallback=LazeCode",
        "gitoxide.committer.emailFallback=lazecode@localhost",
    ])
}

/// Open the repository containing `root`.
pub fn discover(root: &Path) -> Result<gix::Repository> {
    gix::discover_opts(root, Default::default(), open_options())
        .map_err(|err| anyhow::anyhow!("not a Git repository: {err}"))
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
