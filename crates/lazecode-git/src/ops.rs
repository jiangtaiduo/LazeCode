use std::path::{Component, Path, PathBuf};

use gix::bstr::ByteSlice;
use gix::hash::ObjectId;
use gix::index::entry::{Flags, Mode, Stage, Stat};
use gix::refs::transaction::{PreviousValue, RefEdit};
use gix::refs::{FullName, Target};

use crate::tree::{
    blob_bytes, commit_tree_map, head_tree_map, index_map, index_state_from_tree, materialize_tree,
    write_tree_from_index, TreeEntry, TreeMap,
};
use crate::{discover, workdir, Result};

pub(crate) fn open_index(repo: &gix::Repository) -> Result<gix::index::File> {
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

pub(crate) fn remove_empty_parents(workdir: &Path, relative: &Path) {
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

pub(crate) fn write_index(mut file: gix::index::File) -> Result<()> {
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
    let mut filters = crate::filter::FilterContext::new(&repo)?;
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
            let lfs = filters.filter_driver(path)?.as_deref() == Some("lfs");
            if !lfs && meta.len() > crate::MAX_STAGE_BYTES {
                anyhow::bail!(
                    "{path} is too large to stage ({:.1} MiB, limit {:.0} MiB); configure filter=lfs for large files",
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
            (filters.clean_file(root, path, &abs)?, mode)
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
    _excluded: &[String],
    message: &str,
    amend: bool,
) -> Result<String> {
    run_commit_hooks(root, message, amend, |message| {
        commit_checked_inner(root, paths, message, amend)
    })
}

fn commit_checked_inner(
    root: &Path,
    paths: &[String],
    message: &str,
    amend: bool,
) -> Result<String> {
    if message.trim().is_empty() {
        anyhow::bail!("commit message is empty");
    }
    let repo = discover(root)?;
    let workdir = workdir(&repo)?;
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
    let index_file = open_index(&repo)?;
    let index_path = index_file.path().to_path_buf();
    let (mut real_state, _) = index_file.into_parts();
    if real_state
        .entries()
        .iter()
        .any(|entry| entry.mode == Mode::COMMIT)
    {
        anyhow::bail!(
            "cannot commit while submodules are present: submodule support is not implemented"
        );
    }

    // Build the commit tree from HEAD plus only the checked worktree paths.
    // The real index is never staged/un-staged, so previously staged files
    // outside the checked set keep their index state (stable partial commit).
    let mut tree_map = head_tree_map(&repo)?;
    let mut filters = crate::filter::FilterContext::new(&repo)?;
    for path in paths {
        validate_repo_path(path)?;
        let abs = safe_worktree_join(&workdir, path)?;
        let meta = match std::fs::symlink_metadata(&abs) {
            Ok(meta) => meta,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                tree_map.remove(path);
                continue;
            }
            Err(err) => return Err(anyhow::anyhow!("cannot stat {path}: {err}")),
        };
        let (data, mode) = if meta.file_type().is_symlink() {
            let target = std::fs::read_link(&abs)?;
            (target.to_string_lossy().into_owned().into_bytes(), 0o120000)
        } else if meta.is_dir() {
            anyhow::bail!("{path} is a directory; directory commits are not supported");
        } else if !meta.is_file() {
            anyhow::bail!("{path} is not a regular file; refusing to commit it");
        } else {
            let lfs = filters.filter_driver(path)?.as_deref() == Some("lfs");
            if !lfs && meta.len() > crate::MAX_STAGE_BYTES {
                anyhow::bail!(
                    "{path} is too large to commit ({:.1} MiB, limit {:.0} MiB); configure filter=lfs for large files",
                    meta.len() as f64 / (1024.0 * 1024.0),
                    crate::MAX_STAGE_BYTES as f64 / (1024.0 * 1024.0)
                );
            }
            use std::os::unix::fs::PermissionsExt;
            let mode = if meta.permissions().mode() & 0o111 != 0 {
                0o100755
            } else {
                0o100644
            };
            (filters.clean_file(root, path, &abs)?, mode)
        };
        let oid = repo.write_blob(&data)?.detach();
        tree_map.insert(path.clone(), TreeEntry { oid, mode });
    }
    let temp_state = index_state_from_tree(&repo, &tree_map);
    let tree = write_tree_from_index(&repo, &temp_state)?;

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
    let new_commit = repo.new_commit(message, tree, parents).map_err(|err| {
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

    // Make the checked paths clean relative to the new HEAD while leaving
    // every other index entry (including staged files) exactly as it was.
    for path in paths {
        validate_repo_path(path)?;
        let committed = tree_map.get(path);
        let existing =
            real_state.entry_index_by_path_and_stage(path.as_str().into(), Stage::Unconflicted);
        match (committed, existing) {
            (Some(entry), Some(index)) => {
                let current = &mut real_state.entries_mut()[index];
                current.id = entry.oid;
                current.mode = Mode::from_bits_truncate(entry.mode);
                current.stat = Stat::default();
            }
            (Some(entry), None) => {
                real_state.dangerously_push_entry(
                    Stat::default(),
                    entry.oid,
                    Flags::empty(),
                    Mode::from_bits_truncate(entry.mode),
                    path.as_str().into(),
                );
            }
            (None, Some(index)) => {
                real_state.remove_entry_at_index(index);
            }
            (None, None) => {}
        }
    }
    real_state.remove_tree();
    real_state.sort_entries();
    write_index(gix::index::File::from_state(real_state, index_path))?;
    Ok(new_id.to_string())
}

/// Commit the index (optionally amending HEAD), running the standard commit
/// hooks in git's order.
pub fn commit(root: &Path, message: &str, amend: bool) -> Result<String> {
    run_commit_hooks(root, message, amend, |message| {
        commit_inner(root, message, amend)
    })
}

fn run_commit_hooks<T>(
    root: &Path,
    message: &str,
    amend: bool,
    create: impl FnOnce(&str) -> Result<T>,
) -> Result<T> {
    if message.trim().is_empty() {
        anyhow::bail!("commit message is empty");
    }
    let repo = discover(root)?;
    let git_dir = repo.git_dir().to_path_buf();
    let head = repo.head_id().ok().map(|id| id.detach());
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos())
        .unwrap_or(0);
    let message_path = git_dir.join(format!(
        ".lazecode-commit-msg-{}-{nanos}",
        std::process::id()
    ));
    std::fs::write(&message_path, message)?;
    let message_arg = message_path.to_string_lossy().into_owned();
    let result = (|| {
        crate::hooks::run(root, "pre-commit", &[], None)?;
        let mut prepare_args: Vec<String> = vec![message_arg.clone()];
        if amend {
            prepare_args.push("commit".to_string());
            if let Some(head) = head {
                prepare_args.push(head.to_string());
            }
        } else {
            prepare_args.push("message".to_string());
        }
        let prepare_refs: Vec<&str> = prepare_args.iter().map(String::as_str).collect();
        crate::hooks::run(root, "prepare-commit-msg", &prepare_refs, None)?;
        crate::hooks::run(root, "commit-msg", &[message_arg.as_str()], None)?;
        let edited = std::fs::read_to_string(&message_path)?;
        if edited.trim().is_empty() {
            anyhow::bail!("commit message is empty after commit-msg hook");
        }
        let created = create(&edited)?;
        if let Err(error) = crate::hooks::run(root, "post-commit", &[], None) {
            log::warn!("post-commit hook reported an error: {error}");
        }
        Ok(created)
    })();
    let _ = std::fs::remove_file(&message_path);
    result
}

/// Commit the index (optionally amending HEAD).
fn commit_inner(root: &Path, message: &str, amend: bool) -> Result<String> {
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
    if let Ok(worktrees) = crate::worktree::list_worktrees(root) {
        for wt in worktrees {
            if let Some(ref b) = wt.branch {
                if b == name {
                    anyhow::bail!(
                        "cannot delete branch '{name}': checked out in worktree '{}'",
                        wt.name
                    );
                }
            }
        }
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
enum BackupKind {
    Missing,
    File { copy: PathBuf, mode: u32 },
    Symlink(PathBuf),
    Directory,
}

pub(crate) struct CheckoutBackup {
    workdir: PathBuf,
    dir: PathBuf,
    entries: Vec<(PathBuf, BackupKind)>,
}

impl CheckoutBackup {
    pub(crate) fn capture<'a>(
        repo: &gix::Repository,
        workdir: &Path,
        paths: impl Iterator<Item = &'a String>,
    ) -> Result<Self> {
        let unique: std::collections::BTreeSet<&String> = paths.collect();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let dir = repo
            .git_dir()
            .join(format!("lazecode-checkout-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let mut entries = Vec::with_capacity(unique.len());
        for (index, path) in unique.into_iter().enumerate() {
            let abs = safe_worktree_join(workdir, path)?;
            let kind = match std::fs::symlink_metadata(&abs) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    BackupKind::Symlink(std::fs::read_link(&abs)?)
                }
                Ok(metadata) if metadata.is_file() => {
                    use std::os::unix::fs::PermissionsExt;
                    let copy = dir.join(index.to_string());
                    std::fs::copy(&abs, &copy)?;
                    BackupKind::File {
                        copy,
                        mode: metadata.permissions().mode(),
                    }
                }
                Ok(_) => BackupKind::Directory,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => BackupKind::Missing,
                Err(error) => anyhow::bail!("cannot stat '{path}': {error}"),
            };
            entries.push((PathBuf::from(path), kind));
        }
        Ok(Self {
            workdir: workdir.to_path_buf(),
            dir,
            entries,
        })
    }

    pub(crate) fn restore(&self) -> Result<()> {
        for (path, kind) in self.entries.iter().rev() {
            let abs = safe_worktree_join(&self.workdir, &path.to_string_lossy())?;
            match kind {
                BackupKind::Missing => {
                    if let Ok(metadata) = std::fs::symlink_metadata(&abs) {
                        if metadata.is_dir() {
                            let _ = std::fs::remove_dir(&abs);
                        } else {
                            let _ = std::fs::remove_file(&abs);
                        }
                    }
                }
                BackupKind::File { copy, mode } => {
                    if let Some(parent) = abs.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    if abs.is_dir() {
                        let _ = std::fs::remove_dir(&abs);
                    }
                    std::fs::copy(copy, &abs)?;
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&abs, std::fs::Permissions::from_mode(*mode))?;
                }
                BackupKind::Symlink(target) => {
                    let _ = std::fs::remove_file(&abs);
                    std::os::unix::fs::symlink(target, &abs)?;
                }
                BackupKind::Directory => {}
            }
        }
        Ok(())
    }

    pub(crate) fn discard(self) {
        let _ = std::fs::remove_dir_all(self.dir);
    }
}

pub fn checkout_branch(root: &Path, name: &str) -> Result<()> {
    let name = name.trim();
    let repo = discover(root)?;
    let workdir = workdir(&repo)?;
    if let Ok(worktrees) = crate::worktree::list_worktrees(root) {
        let current_workdir = workdir
            .canonicalize()
            .unwrap_or_else(|_| workdir.to_path_buf());
        for wt in worktrees {
            if wt.path != current_workdir {
                if let Some(ref b) = wt.branch {
                    if b == name {
                        anyhow::bail!(
                            "cannot checkout '{name}': already checked out in worktree '{}' ({})",
                            wt.name,
                            wt.path.display()
                        );
                    }
                }
            }
        }
    }
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
    // Snapshot every worktree path the checkout can touch so a mid-checkout
    // failure restores the pre-checkout tree instead of leaving a hybrid.
    let backup = CheckoutBackup::capture(&repo, &workdir, old_map.keys().chain(target_map.keys()))?;
    let old_index_bytes = std::fs::read(repo.index_path()).ok();
    let old_head = repo
        .head_id()
        .ok()
        .map(|id| id.to_string())
        .unwrap_or_else(|| "0".repeat(40));
    let checkout_name: FullName = full.as_str().try_into()?;
    let result = (|| -> Result<()> {
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

        let message = format!("checkout: moving to {checkout_name}");
        let mut edit = RefEdit::update(
            "HEAD".try_into()?,
            Target::Symbolic(checkout_name.clone()),
            PreviousValue::Any,
            message,
        );
        edit.deref = false;
        repo.edit_reference(edit)?;
        Ok(())
    })();
    if let Err(error) = result {
        if let Err(restore_error) = backup.restore() {
            log::error!("checkout rollback failed: {restore_error}");
        }
        match old_index_bytes {
            Some(bytes) => {
                if let Err(restore_error) =
                    lazecode_fs::file::write_atomic(&repo.index_path(), &bytes)
                {
                    log::error!("checkout index rollback failed: {restore_error}");
                }
            }
            None => {
                let _ = std::fs::remove_file(repo.index_path());
            }
        }
        return Err(error);
    }
    backup.discard();
    let new_head = target_id.to_string();
    if let Err(error) = crate::hooks::run(root, "post-checkout", &[&old_head, &new_head, "1"], None)
    {
        log::warn!("post-checkout hook reported an error: {error}");
    }
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
        anyhow::bail!("patch is empty");
    }
    if patch.len() > 16 * 1024 * 1024 {
        anyhow::bail!("patch is too large (limit 16 MiB)");
    }
    let patch = if patch.ends_with('\n') {
        patch.to_string()
    } else {
        format!("{patch}\n")
    };
    let repo = discover(root)?;
    let worktree = workdir(&repo)?;
    let files = crate::patch::parse(&patch)?;

    enum Update {
        Delete,
        Write { data: Vec<u8>, mode: u32 },
    }
    struct Planned {
        path: String,
        update: Update,
    }
    let mut planned = Vec::with_capacity(files.len());

    if cached {
        let index = open_index(&repo)?;
        for file in &files {
            let (path, create, delete) = file.operation(reverse)?;
            let relative = validate_repo_path(path)?;
            let path = relative.to_string_lossy().to_string();
            let entry = index.entry_by_path(path.as_str().into());
            if create && entry.is_some() {
                anyhow::bail!("patch creates '{path}', which already exists in the index");
            }
            if !create && entry.is_none() {
                anyhow::bail!("patch modifies '{path}', which is not in the index");
            }
            let old = entry
                .map(|entry| blob_bytes(&repo, entry.id))
                .transpose()?
                .unwrap_or_default();
            let data = crate::patch::apply(&old, file, reverse)?;
            if delete && !data.is_empty() {
                anyhow::bail!("deletion patch for '{path}' does not remove the whole file");
            }
            let mode = if create {
                file.mode(reverse).unwrap_or(0o100644)
            } else {
                entry.map(|entry| entry.mode.bits()).unwrap_or(0o100644)
            };
            planned.push(Planned {
                path,
                update: if delete {
                    Update::Delete
                } else {
                    Update::Write { data, mode }
                },
            });
        }
        let file = open_index(&repo)?;
        let index_path = file.path().to_path_buf();
        let (mut state, _) = file.into_parts();
        for item in planned {
            state.remove_entries(|_, candidate, _| {
                candidate
                    .to_str()
                    .map(|path| path == item.path)
                    .unwrap_or(false)
            });
            if let Update::Write { data, mode } = item.update {
                let oid = repo.write_blob(&data)?.detach();
                state.dangerously_push_entry(
                    Stat::default(),
                    oid,
                    Flags::empty(),
                    Mode::from_bits_truncate(mode),
                    item.path.as_str().into(),
                );
            }
        }
        state.remove_tree();
        state.sort_entries();
        write_index(gix::index::File::from_state(state, index_path))?;
        return Ok(());
    }

    for file in &files {
        let (path, create, delete) = file.operation(reverse)?;
        let relative = validate_repo_path(path)?;
        let path = relative.to_string_lossy().to_string();
        let abs = worktree.join(&relative);
        let metadata = std::fs::symlink_metadata(&abs);
        if create && metadata.is_ok() {
            anyhow::bail!("patch creates '{path}', which already exists");
        }
        if !create && metadata.is_err() {
            anyhow::bail!("patch modifies '{path}', which does not exist");
        }
        let (old, old_mode) = match &metadata {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let target = std::fs::read_link(&abs)?;
                (target.to_string_lossy().into_owned().into_bytes(), 0o120000)
            }
            Ok(metadata) if metadata.is_file() => {
                use std::os::unix::fs::PermissionsExt;
                (
                    std::fs::read(&abs)?,
                    if metadata.permissions().mode() & 0o111 != 0 {
                        0o100755
                    } else {
                        0o100644
                    },
                )
            }
            Ok(_) => anyhow::bail!("patch target '{path}' is not a regular file"),
            Err(_) if create => (Vec::new(), 0o100644),
            Err(error) => anyhow::bail!("cannot stat '{path}': {error}"),
        };
        let data = crate::patch::apply(&old, file, reverse)?;
        if delete && !data.is_empty() {
            anyhow::bail!("deletion patch for '{path}' does not remove the whole file");
        }
        planned.push(Planned {
            path,
            update: if delete {
                Update::Delete
            } else {
                Update::Write {
                    data,
                    mode: file.mode(reverse).unwrap_or(old_mode),
                }
            },
        });
    }

    let paths: Vec<String> = planned.iter().map(|item| item.path.clone()).collect();
    let backup = CheckoutBackup::capture(&repo, &worktree, paths.iter())?;
    let result = (|| -> Result<()> {
        for item in planned {
            let abs = safe_worktree_join(&worktree, &item.path)?;
            match item.update {
                Update::Delete => {
                    std::fs::remove_file(&abs)?;
                    remove_empty_parents(&worktree, Path::new(&item.path));
                }
                Update::Write { data, mode } => {
                    if let Some(parent) = abs.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    if mode == 0o120000 {
                        let _ = std::fs::remove_file(&abs);
                        std::os::unix::fs::symlink(
                            String::from_utf8_lossy(&data).to_string(),
                            &abs,
                        )?;
                        continue;
                    }
                    lazecode_fs::file::write_atomic(&abs, &data)?;
                    use std::os::unix::fs::PermissionsExt;
                    let perms = if mode & 0o111 != 0 { 0o755 } else { 0o644 };
                    std::fs::set_permissions(&abs, std::fs::Permissions::from_mode(perms))?;
                }
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        if let Err(restore_error) = backup.restore() {
            log::error!("patch rollback failed: {restore_error}");
        }
        return Err(error);
    }
    backup.discard();
    Ok(())
}

/// Initialize a new Git repository at `root`.
pub fn git_init(root: &Path) -> Result<()> {
    std::fs::create_dir_all(root)?;
    gix::init(root).map_err(|err| anyhow::anyhow!("git init failed: {err}"))?;
    Ok(())
}

/// Discard changes for a file or directory (rollback to HEAD or delete untracked).
pub fn discard_file_changes(root: &Path, rel_path: &str) -> Result<()> {
    let repo = discover(root)?;
    let workdir = workdir(&repo)?;
    let relative = validate_repo_path(rel_path)?;
    let rel_path = relative.to_string_lossy().to_string();
    let head_map = head_tree_map(&repo)?;
    if let Some(entry) = head_map.get(rel_path.as_str()) {
        let mut map = TreeMap::new();
        map.insert(rel_path.clone(), *entry);
        materialize_tree(&repo, &map)?;
        stage(root, &[rel_path])?;
    } else {
        ensure_safe_ancestors(&workdir, &rel_path)?;
        let full = workdir.join(&relative);
        if full.is_dir() {
            std::fs::remove_dir_all(&full)?;
        } else if full.exists() || std::fs::symlink_metadata(&full).is_ok() {
            std::fs::remove_file(&full)?;
        }
        let file = open_index(&repo)?;
        let index_path = file.path().to_path_buf();
        let (mut state, _) = file.into_parts();
        let prefix = format!("{rel_path}/");
        state.remove_entries(|_, candidate, _| {
            candidate
                .to_str()
                .map(|candidate| candidate == rel_path || candidate.starts_with(&prefix))
                .unwrap_or(false)
        });
        state.remove_tree();
        state.sort_entries();
        write_index(gix::index::File::from_state(state, index_path))?;
    }
    Ok(())
}
