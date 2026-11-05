use std::path::{Component, Path, PathBuf};

use gix::bstr::ByteSlice;
use gix::hash::ObjectId;
use gix::index::entry::{Flags, Mode, Stage, Stat};
use gix::refs::transaction::{PreviousValue, RefEdit};
use gix::refs::{FullName, Target};

use crate::tree::{
    blob_bytes, commit_tree_map, head_tree_map, index_map, index_state_from_tree, materialize_tree,
    write_tree_from_index,
};
use crate::{discover, workdir, Result};

fn open_index(repo: &gix::Repository) -> Result<gix::index::File> {
    let file = if repo.index_path().exists() {
        repo.open_index()?
    } else {
        gix::index::File::from_state(
            gix::index::State::new(repo.object_hash()),
            repo.index_path(),
        )
    };
    if file.version() == gix::index::Version::V4 {
        anyhow::bail!(
            "this repository uses index v4; LazeCode refuses to rewrite it because that would drop \
             its extensions. Run `git update-index --index-version 2` first."
        );
    }
    if file.link().is_some() {
        anyhow::bail!("split index is not supported yet; refusing to rewrite .git/index");
    }
    if file.untracked().is_some() {
        anyhow::bail!("untracked cache extension is present; refusing to rewrite .git/index");
    }
    if file.fs_monitor().is_some() {
        anyhow::bail!("FSMonitor extension is present; refusing to rewrite .git/index");
    }
    if file.resolve_undo().is_some() {
        anyhow::bail!("resolve-undo extension is present; refusing to rewrite .git/index");
    }
    if file
        .entries()
        .iter()
        .any(|entry| entry.stage() != Stage::Unconflicted)
    {
        anyhow::bail!(
            "the index has unresolved conflicts; resolve them before using LazeCode's Git write \
             operations"
        );
    }
    Ok(file)
}

/// Reject absolute paths, `..`, and other non-normal components before joining
/// them onto the worktree. Git tree/index paths are always `/`-separated. On
/// Unix a backslash is a legal filename character, so it is intentionally not
/// treated as a separator here.
pub(crate) fn validate_repo_path(path: &str) -> Result<PathBuf> {
    let relative = Path::new(path);
    if relative.as_os_str().is_empty() || relative.is_absolute() {
        anyhow::bail!("refusing empty or absolute repository path {path:?}");
    }
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        anyhow::bail!("refusing non-normal repository path {path:?}");
    }
    Ok(relative.to_path_buf())
}

/// Verify every existing component below `workdir` is a real directory, not a
/// symlink. This prevents a tracked or untracked symlink from redirecting
/// checkout writes outside the worktree.
pub(crate) fn safe_worktree_join(workdir: &Path, path: &str) -> Result<PathBuf> {
    let relative = validate_repo_path(path)?;
    let mut current = workdir.to_path_buf();
    let components: Vec<_> = relative.components().collect();
    for (index, component) in components.iter().enumerate() {
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if index + 1 < components.len() {
                    if metadata.file_type().is_symlink() {
                        anyhow::bail!(
                            "refusing to write through symlink {} while checking out {path}",
                            current.display()
                        );
                    }
                    if !metadata.is_dir() {
                        anyhow::bail!(
                            "{} is not a directory while checking out {path}",
                            current.display()
                        );
                    }
                } else if metadata.is_dir() {
                    anyhow::bail!(
                        "refusing to replace directory {} with a file",
                        current.display()
                    );
                } else if !metadata.is_file() && !metadata.file_type().is_symlink() {
                    anyhow::bail!("refusing to replace non-regular file {}", current.display());
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                anyhow::bail!("cannot inspect {}: {err}", current.display());
            }
        }
    }
    Ok(workdir.join(relative))
}

fn ensure_safe_ancestors(workdir: &Path, path: &str) -> Result<()> {
    let relative = validate_repo_path(path)?;
    let mut current = workdir.to_path_buf();
    let components: Vec<_> = relative.components().collect();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    anyhow::bail!(
                        "refusing to write through symlink {} while checking out {path}",
                        current.display()
                    );
                }
                if !metadata.is_dir() {
                    anyhow::bail!(
                        "{} is not a directory while checking out {path}",
                        current.display()
                    );
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => anyhow::bail!("cannot inspect {}: {err}", current.display()),
        }
    }
    Ok(())
}

fn remove_empty_parents(workdir: &Path, relative: &Path) {
    let mut parent = relative.parent();
    while let Some(path) = parent {
        if path.as_os_str().is_empty() {
            break;
        }
        if std::fs::remove_dir(workdir.join(path)).is_err() {
            break;
        }
        parent = path.parent();
    }
}

fn write_index(mut file: gix::index::File) -> Result<()> {
    file.write(gix::index::write::Options::default())?;
    Ok(())
}

/// Stage the given worktree-relative paths.
pub fn stage(root: &Path, paths: &[String]) -> Result<()> {
    let repo = discover(root)?;
    let workdir = workdir(&repo)?;
    let file = open_index(&repo)?;
    let index_path = file.path().to_path_buf();
    let (mut state, _) = file.into_parts();
    for path in paths {
        validate_repo_path(path)?;
        let abs = safe_worktree_join(&workdir, path)?;
        let meta = match std::fs::symlink_metadata(&abs) {
            Ok(meta) => meta,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                // Staging a deletion is only valid for a path that is actually
                // tracked. A missing untracked path must not silently succeed.
                if state.entry_by_path(path.as_str().into()).is_none() {
                    anyhow::bail!("cannot stage {path}: no such file and it is not tracked");
                }
                state.remove_entries(|_, candidate, _| {
                    candidate.to_str().ok() == Some(path.as_str())
                });
                continue;
            }
            Err(err) => return Err(anyhow::anyhow!("cannot stat {path}: {err}")),
        };
        let (data, mode) = if meta.file_type().is_symlink() {
            let target = std::fs::read_link(&abs)?;
            (
                target.to_string_lossy().into_owned().into_bytes(),
                Mode::SYMLINK,
            )
        } else if meta.is_dir() {
            anyhow::bail!(
                "{path} is a directory; submodules and directory staging are not supported"
            );
        } else if !meta.is_file() {
            anyhow::bail!("{path} is not a regular file; refusing to stage it");
        } else {
            if meta.len() > crate::MAX_STAGE_BYTES {
                anyhow::bail!(
                    "{path} is too large to stage ({:.1} MiB, limit {:.0} MiB)",
                    meta.len() as f64 / (1024.0 * 1024.0),
                    crate::MAX_STAGE_BYTES as f64 / (1024.0 * 1024.0)
                );
            }
            use std::os::unix::fs::PermissionsExt;
            let mode = if meta.permissions().mode() & 0o111 != 0 {
                Mode::FILE_EXECUTABLE
            } else {
                Mode::FILE
            };
            (std::fs::read(&abs)?, mode)
        };
        let id = repo.write_blob(&data)?.detach();
        let stat = gix::index::fs::Metadata::from_path_no_follow(&abs)
            .ok()
            .and_then(|meta| Stat::from_fs(&meta).ok())
            .unwrap_or_default();
        state.remove_entries(|_, candidate, _| candidate.to_str().ok() == Some(path.as_str()));
        state.dangerously_push_entry(stat, id, Flags::empty(), mode, path.as_str().into());
    }
    state.remove_tree();
    state.sort_entries();
    write_index(gix::index::File::from_state(state, index_path))?;
    Ok(())
}

/// Reset the index entries for `paths` back to HEAD.
pub fn unstage(root: &Path, paths: &[String]) -> Result<()> {
    let repo = discover(root)?;
    let head_map = head_tree_map(&repo)?;
    let file = open_index(&repo)?;
    let index_path = file.path().to_path_buf();
    let (mut state, _) = file.into_parts();
    for path in paths {
        validate_repo_path(path)?;
        if state.entries().iter().any(|entry| {
            entry.mode == Mode::COMMIT && entry.path(&state).as_bytes() == path.as_bytes()
        }) {
            anyhow::bail!("cannot unstage submodule {path}: submodule support is not implemented");
        }
        match head_map.get(path) {
            Some(entry) => {
                let mode = Mode::from_bits_truncate(entry.mode);
                if let Some(current) =
                    state.entry_mut_by_path_and_stage(path.as_str().into(), Stage::Unconflicted)
                {
                    current.id = entry.oid;
                    current.mode = mode;
                    current.stat = Stat::default();
                } else {
                    state.dangerously_push_entry(
                        Stat::default(),
                        entry.oid,
                        Flags::empty(),
                        mode,
                        path.as_str().into(),
                    );
                }
            }
            None => {
                state.remove_entries(|_, candidate, _| {
                    candidate.to_str().ok() == Some(path.as_str())
                });
            }
        }
    }
    state.remove_tree();
    state.sort_entries();
    write_index(gix::index::File::from_state(state, index_path))?;
    Ok(())
}

/// Stage the checked paths, reset the explicitly unchecked ones out of the
/// index, then commit. Git always records the whole index, so this is what
/// makes the UI's per-file checkboxes the exact commit set.
pub fn commit_checked(
    root: &Path,
    paths: &[String],
    excluded: &[String],
    message: &str,
    amend: bool,
) -> Result<String> {
    stage(root, paths)?;
    if !excluded.is_empty() {
        unstage(root, excluded)?;
    }
    commit(root, message, amend)
}

/// Commit the index (optionally amending HEAD).
pub fn commit(root: &Path, message: &str, amend: bool) -> Result<String> {
    if message.trim().is_empty() {
        anyhow::bail!("commit message is empty");
    }
    let repo = discover(root)?;
    for marker in ["MERGE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD"] {
        if repo.git_dir().join(marker).exists() {
            anyhow::bail!(
                "cannot commit while {marker} exists: merge/cherry-pick/revert sequencing is not \
                 implemented"
            );
        }
    }
    for marker in ["rebase-merge", "rebase-apply"] {
        if repo.git_dir().join(marker).exists() {
            anyhow::bail!("cannot commit during a rebase: rebase is not implemented");
        }
    }
    let file = open_index(&repo)?;
    let (state, _) = file.into_parts();
    if state
        .entries()
        .iter()
        .any(|entry| entry.mode == Mode::COMMIT)
    {
        anyhow::bail!(
            "cannot commit while submodules are present: submodule support is not implemented"
        );
    }
    let tree = write_tree_from_index(&repo, &state)?;
    let head = repo.head_id().ok().map(|id| id.detach());
    let parents: Vec<ObjectId> = if amend {
        match head {
            Some(head) => repo
                .find_commit(head)?
                .parent_ids()
                .map(|parent| parent.detach())
                .collect(),
            None => anyhow::bail!("cannot amend: repository has no commits"),
        }
    } else {
        head.into_iter().collect()
    };
    let subject: String = message.lines().next().unwrap_or("").trim().to_string();
    let new_commit = repo
        .new_commit(message, tree, parents.clone())
        .map_err(|err| {
            anyhow::anyhow!("cannot create commit: {err}. Check user.name/user.email in Git config")
        })?;
    let new_id = new_commit.id().detach();
    let expected = if amend {
        PreviousValue::MustExistAndMatch(Target::Object(
            head.ok_or_else(|| anyhow::anyhow!("cannot amend without HEAD"))?,
        ))
    } else {
        match head {
            Some(head) => PreviousValue::MustExistAndMatch(Target::Object(head)),
            None => PreviousValue::MustNotExist,
        }
    };
    let name: FullName = "HEAD".try_into()?;
    let mut edit = RefEdit::update(
        name,
        new_id,
        expected,
        format!("commit{}: {subject}", if amend { " (amend)" } else { "" }),
    );
    edit.deref = true;
    repo.edit_reference(edit)?;
    Ok(new_id.to_string())
}

fn head_branch_name(repo: &gix::Repository) -> Option<String> {
    repo.head()
        .ok()
        .and_then(|head| head.referent_name().map(|name| name.shorten().to_string()))
}

pub fn create_branch(root: &Path, name: &str, checkout: bool) -> Result<()> {
    let name = name.trim();
    let repo = discover(root)?;
    let head = repo.head_id()?.detach();
    let full = format!("refs/heads/{name}");
    let full_name: FullName = full.as_str().try_into()?;
    let edit = RefEdit::update(
        full_name,
        head,
        PreviousValue::MustNotExist,
        "branch: Created from HEAD",
    );
    repo.edit_reference(edit)?;
    if checkout {
        checkout_branch(root, name)?;
    }
    Ok(())
}

pub fn delete_branch(root: &Path, name: &str) -> Result<()> {
    let name = name.trim();
    let repo = discover(root)?;
    if head_branch_name(&repo).as_deref() == Some(name) {
        anyhow::bail!("cannot delete the current branch");
    }
    if name.is_empty() {
        anyhow::bail!("branch name is empty");
    }
    let full = format!("refs/heads/{name}");
    let full_name: FullName = full.as_str().try_into()?;
    repo.edit_reference(RefEdit::delete(full_name, PreviousValue::MustExist))?;
    Ok(())
}

/// Switch branches by materialising the target tree on a clean worktree, then
/// updating the index and HEAD. Refuses to run on a dirty worktree.
pub fn checkout_branch(root: &Path, name: &str) -> Result<()> {
    let name = name.trim();
    let repo = discover(root)?;
    let workdir = workdir(&repo)?;
    let full = format!("refs/heads/{name}");
    let target_ref = repo
        .find_reference(full.as_str())
        .map_err(|_| anyhow::anyhow!("branch '{name}' does not exist"))?;
    let target_id = target_ref
        .target()
        .try_id()
        .map(|id| id.to_owned())
        .ok_or_else(|| anyhow::anyhow!("branch '{name}' has no target"))?;
    let snapshot = crate::status::snapshot(&workdir)?;
    if snapshot.has_tracked_changes() {
        anyhow::bail!("cannot switch branches: the worktree has staged or unstaged changes");
    }
    let target_tree = repo.find_commit(target_id)?.tree()?;
    if crate::tree::tree_has_gitlinks(&repo, target_tree)? {
        anyhow::bail!("cannot switch branches: submodules are not supported");
    }
    if let Ok(head) = repo.head() {
        if !head.is_unborn() && crate::tree::tree_has_gitlinks(&repo, repo.head_tree()?)? {
            anyhow::bail!("cannot switch branches: submodules are not supported");
        }
    }
    let target_map = commit_tree_map(&repo, target_id)?;
    let head_map = head_tree_map(&repo)?;
    let old_index = repo.open_index()?;
    if old_index
        .entries()
        .iter()
        .any(|entry| entry.mode == Mode::COMMIT)
    {
        anyhow::bail!("cannot switch branches: submodules are not supported");
    }
    let old_map = index_map(&old_index);

    // Validate every path before mutating anything. This rejects `..`,
    // absolute paths, and symlinked ancestors that would escape the worktree.
    for path in target_map.keys().chain(old_map.keys()) {
        ensure_safe_ancestors(&workdir, path)?;
    }
    for path in target_map.keys() {
        let relative = validate_repo_path(path)?;
        let abs = workdir.join(&relative);
        if std::fs::symlink_metadata(&abs).is_ok()
            && !old_map.contains_key(path)
            && !head_map.contains_key(path)
        {
            anyhow::bail!("untracked file '{path}' would be overwritten by checkout");
        }
    }

    // Delete stale paths first, then prune empty directories. This handles
    // file↔directory type changes without ever removing untracked content.
    for path in old_map.keys() {
        if target_map.contains_key(path) {
            continue;
        }
        let relative = validate_repo_path(path)?;
        let abs = workdir.join(&relative);
        match std::fs::symlink_metadata(&abs) {
            Ok(metadata) if metadata.is_dir() => {
                anyhow::bail!(
                    "refusing to remove directory {} while switching branches",
                    abs.display()
                );
            }
            Ok(_) => {
                std::fs::remove_file(&abs)?;
                remove_empty_parents(&workdir, &relative);
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => anyhow::bail!("cannot remove {}: {err}", abs.display()),
        }
    }
    for path in target_map.keys() {
        safe_worktree_join(&workdir, path)?;
    }
    materialize_tree(&repo, &target_map)?;
    let state = index_state_from_tree(&repo, &target_map);
    let mut index_file = gix::index::File::from_state(state, repo.index_path());
    index_file.write(gix::index::write::Options::default())?;

    let name: FullName = full.as_str().try_into()?;
    let message = format!("checkout: moving to {name}");
    let mut edit = RefEdit::update(
        "HEAD".try_into()?,
        Target::Symbolic(name),
        PreviousValue::Any,
        message,
    );
    edit.deref = false;
    repo.edit_reference(edit)?;
    Ok(())
}

#[allow(dead_code)]
fn _blob(repo: &gix::Repository, id: ObjectId) -> Vec<u8> {
    blob_bytes(repo, id).unwrap_or_default()
}

/// Rename a local branch in place (`branch -m`). Refuses if the target exists.
pub fn rename_branch(root: &Path, from: &str, to: &str) -> Result<String> {
    let from = from.trim();
    let to = to.trim();
    if to.is_empty() {
        anyhow::bail!("new branch name is empty");
    }
    let from_full: FullName = format!("refs/heads/{from}").as_str().try_into()?;
    let to_full: FullName = format!("refs/heads/{to}").as_str().try_into()?;
    let repo = discover(root)?;
    let reference = repo
        .find_reference(from_full.as_bstr())
        .map_err(|_| anyhow::anyhow!("branch '{from}' does not exist"))?;
    if repo.try_find_reference(to_full.as_bstr())?.is_some() {
        anyhow::bail!("branch '{to}' already exists");
    }
    let target = reference
        .target()
        .try_id()
        .map(|id| id.to_owned())
        .ok_or_else(|| anyhow::anyhow!("branch '{from}' has no target"))?;
    let log = format!("branch: renamed {from} to {to}");
    repo.edit_reference(RefEdit::update(
        to_full.clone(),
        target,
        PreviousValue::MustNotExist,
        log.as_str(),
    ))?;
    repo.edit_reference(RefEdit::delete(from_full, PreviousValue::MustExist))?;
    // HEAD points at the old branch name; retarget it when we renamed the
    // currently checked-out branch.
    if head_branch_name(&repo).as_deref() == Some(from) {
        let head_ref: FullName = "HEAD".try_into()?;
        let mut edit = RefEdit::update(
            head_ref,
            Target::Symbolic(to_full),
            PreviousValue::Any,
            format!("branch: updated HEAD to {to}").as_str(),
        );
        edit.deref = false;
        repo.edit_reference(edit)?;
    }
    Ok(to.to_string())
}

/// Create a tag at `commit` (HEAD when empty). Only lightweight tags are
/// written in this pass; annotated tags are a M6 item.
pub fn create_tag(root: &Path, name: &str, commit: &str, _message: Option<&str>) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        anyhow::bail!("tag name is empty");
    }
    if name.contains("..") || name.starts_with('-') {
        anyhow::bail!("unsuitable tag name '{name}'");
    }
    let repo = discover(root)?;
    let target = if commit.trim().is_empty() {
        repo.head_id()?.detach()
    } else {
        crate::resolve_commit(&repo, commit.trim())?
    };
    let full: FullName = format!("refs/tags/{name}").as_str().try_into()?;
    if repo.try_find_reference(full.as_bstr())?.is_some() {
        anyhow::bail!("tag '{name}' already exists");
    }
    repo.edit_reference(RefEdit::update(
        full,
        target,
        PreviousValue::MustNotExist,
        "tag: created (lightweight)",
    ))?;
    Ok(())
}

/// Delete a tag ref in the local repository.
pub fn delete_tag(root: &Path, name: &str) -> Result<()> {
    let name = name.trim();
    let full: FullName = format!("refs/tags/{name}").as_str().try_into()?;
    let repo = discover(root)?;
    repo.edit_reference(RefEdit::delete(full, PreviousValue::MustExist))?;
    Ok(())
}

/// Apply a unified patch to the worktree (or the index with `cached`).
/// Runs `git apply --check` first so a dry-run failure never half-applies.
/// The porcelain binary is the compatibility surface for `add -p`-style
/// hunk staging, which gix does not expose.
pub fn apply_patch(root: &Path, patch: &str, cached: bool, reverse: bool) -> Result<()> {
    if patch.trim().is_empty() {
        anyhow::bail!("empty patch");
    }
    if patch.len() > 16 * 1024 * 1024 {
        anyhow::bail!("patch larger than 16 MiB");
    }
    // The porcelain runner trims stdout; `git apply` needs the trailing
    // newline to close the final hunk, so restore it if it was stripped.
    let patch = if patch.ends_with('\n') {
        patch.to_string()
    } else {
        format!("{patch}\n")
    };
    let repo = discover(root)?;
    let worktree = workdir(&repo)?;
    let mut base = vec!["apply".to_string(), "--whitespace=nowarn".to_string()];
    if cached {
        base.push("--cached".into());
    }
    if reverse {
        base.push("--reverse".into());
    }
    let run = |extra: &[&str]| -> Result<std::process::Output> {
        let mut cmd = std::process::Command::new("git");
        cmd.current_dir(&worktree).args(&base);
        for arg in extra {
            cmd.arg(arg);
        }
        let temp = std::env::temp_dir().join(format!(
            "lazecode-apply-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&temp, patch.as_bytes())?;
        cmd.arg(&temp);
        let result = cmd
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .map_err(|err| anyhow::anyhow!("git apply failed: {err}"));
        let _ = std::fs::remove_file(&temp);
        result
    };
    let check = run(&["--check"])?;
    if !check.status.success() {
        anyhow::bail!(
            "patch check failed: {}",
            String::from_utf8_lossy(&check.stderr).trim()
        );
    }
    let output = run(&[])?;
    if !output.status.success() {
        anyhow::bail!(
            "git apply failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}
