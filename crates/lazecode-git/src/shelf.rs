//! Stash (native commits/reflog) and shelves (patch files under
//! `.git/lazecode-shelves/`, full or path-filtered restore).

use std::path::Path;

use anyhow::anyhow;
use gix::bstr::ByteSlice;
use gix::hash::ObjectId;
use gix::index::entry::{Flags, Mode, Stage, Stat};
use gix::refs::transaction::{LogChange, PreviousValue, RefEdit, RefLog};
use gix::refs::Target;

use crate::ops::{
    open_index, remove_empty_parents, safe_worktree_join, write_index, CheckoutBackup,
};
use crate::tree::{
    blob_bytes, commit_tree_map, head_tree_map, index_map, index_state_from_tree, materialize_tree,
    write_tree_from_index, TreeEntry, TreeMap,
};
use crate::{discover, workdir, Result};

// ---------------- stash ----------------

#[derive(Clone, Debug)]
pub struct StashEntry {
    /// e.g. `stash@{0}`.
    pub name: String,
    /// Raw reflog message: `On <branch>: <message>` or
    /// `WIP on <branch>: <sha> <subject>`.
    pub subject: String,
    /// Branch the stash was created on, parsed from the reflog message.
    pub branch: Option<String>,
}

impl StashEntry {
    /// Reflog message without git's `On <branch>:` / `WIP on <branch>: <sha>`
    /// prefix — the text the user typed when creating the stash.
    pub fn message(&self) -> String {
        stash_message_parts(&self.subject).1
    }
}

/// Split a stash reflog message into `(branch, message)`.
fn stash_message_parts(subject: &str) -> (Option<String>, String) {
    let subject = subject.trim();
    if let Some(rest) = subject.strip_prefix("On ") {
        if let Some((branch, message)) = rest.split_once(": ") {
            return (Some(branch.to_string()), message.trim().to_string());
        }
    }
    if let Some(rest) = subject.strip_prefix("WIP on ") {
        if let Some((branch, message)) = rest.split_once(": ") {
            // Drop the abbreviated commit id git puts in front of the subject.
            let message = message.split_once(' ').map(|(_, tail)| tail).unwrap_or(message);
            return (Some(branch.to_string()), message.trim().to_string());
        }
    }
    (None, subject.to_string())
}

pub fn stash_list(root: &Path) -> Result<Vec<StashEntry>> {
    let repo = discover(root)?;
    let Ok(reference) = repo.find_reference("refs/stash") else {
        return Ok(Vec::new());
    };
    let mut platform = reference.log_iter();
    let Some(lines) = platform
        .rev()
        .map_err(|err| anyhow!("cannot read stash reflog: {err}"))?
    else {
        return Ok(Vec::new());
    };
    let mut entries = Vec::new();
    for (index, line) in lines.enumerate() {
        let line = line.map_err(|err| anyhow!("cannot parse stash reflog: {err}"))?;
        let subject = String::from_utf8_lossy(line.message.as_ref()).to_string();
        entries.push(StashEntry {
            name: format!("stash@{{{index}}}"),
            branch: stash_message_parts(&subject).0,
            subject,
        });
    }
    Ok(entries)
}

fn parse_stash_index(spec: &str) -> Option<usize> {
    let inner = spec.strip_prefix("stash@{")?.strip_suffix('}')?;
    inner.parse().ok()
}

fn stash_oid(repo: &gix::Repository, spec: &str) -> Result<ObjectId> {
    if let Some(index) = parse_stash_index(spec) {
        let reference = repo
            .find_reference("refs/stash")
            .map_err(|_| anyhow!("no stash entries"))?;
        let mut platform = reference.log_iter();
        let Some(lines) = platform
            .rev()
            .map_err(|err| anyhow!("cannot read stash reflog: {err}"))?
        else {
            anyhow::bail!("no stash entries");
        };
        for (current, line) in lines.enumerate() {
            if current == index {
                return Ok(line
                    .map_err(|err| anyhow!("cannot parse stash reflog: {err}"))?
                    .new_oid);
            }
        }
        anyhow::bail!("stash entry {spec} does not exist");
    }
    Ok(repo
        .rev_parse_single(gix::bstr::BStr::new(spec))
        .map_err(|err| anyhow!("cannot resolve stash '{spec}': {err}"))?
        .detach())
}

fn write_map_tree(repo: &gix::Repository, map: &TreeMap) -> Result<ObjectId> {
    write_tree_from_index(repo, &index_state_from_tree(repo, map))
}

fn worktree_entry(
    repo: &gix::Repository,
    path: &str,
    absolute: &Path,
    filters: &mut crate::filter::FilterContext<'_>,
) -> Result<Option<TreeEntry>> {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::symlink_metadata(absolute) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let target = std::fs::read_link(absolute)?;
            let data = target.to_string_lossy().into_owned().into_bytes();
            Ok(Some(TreeEntry {
                oid: repo.write_blob(&data)?.detach(),
                mode: 0o120000,
            }))
        }
        Ok(metadata) if metadata.is_file() => {
            let data = filters.clean_file(path, absolute)?;
            Ok(Some(TreeEntry {
                oid: repo.write_blob(&data)?.detach(),
                mode: if metadata.permissions().mode() & 0o111 != 0 {
                    0o100755
                } else {
                    0o100644
                },
            }))
        }
        Ok(_) => anyhow::bail!("{path} is not a regular file"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn stash_push(
    root: &Path,
    message: Option<&str>,
    keep_index: bool,
    include_untracked: bool,
) -> Result<String> {
    let repo = discover(root)?;
    let workdir = workdir(&repo)?;
    let head = repo.head()?;
    if head.is_unborn() {
        anyhow::bail!("cannot stash without a commit");
    }
    let head_id = repo.head_id()?.detach();
    let head_map = head_tree_map(&repo)?;
    let index_file = open_index(&repo)?;
    let index_map = index_map(&index_file);
    drop(index_file);

    let snapshot = crate::status::changed_paths(root)?;
    let mut work_map = head_map.clone();
    let mut untracked_map = TreeMap::new();
    let mut filters = crate::filter::FilterContext::new(&repo)?;
    for change in &snapshot {
        if change.conflicted {
            anyhow::bail!("cannot stash unresolved conflicts");
        }
        if change.untracked && !include_untracked {
            continue;
        }
        let absolute = safe_worktree_join(&workdir, &change.path)?;
        let entry = worktree_entry(&repo, &change.path, &absolute, &mut filters)?;
        if change.untracked {
            if let Some(entry) = entry {
                untracked_map.insert(change.path.clone(), entry);
            }
            continue;
        }
        match entry {
            Some(entry) => {
                work_map.insert(change.path.clone(), entry);
            }
            None => {
                work_map.remove(&change.path);
            }
        }
    }
    if work_map == head_map && index_map == head_map && untracked_map.is_empty() {
        anyhow::bail!("No local changes to save");
    }

    let branch = head
        .referent_name()
        .map(|name| name.shorten().to_string())
        .unwrap_or_else(|| "HEAD".to_string());
    let head_commit = repo.find_commit(head_id)?;
    let message_text = head_commit.message_raw()?.to_string();
    let subject = message_text.lines().next().unwrap_or("").trim().to_string();
    let short = crate::short_id(&head_id);
    let stash_message = match message.map(str::trim).filter(|text| !text.is_empty()) {
        Some(text) => format!("On {branch}: {text}"),
        None => format!("WIP on {branch}: {short} {subject}"),
    };

    let index_tree = write_map_tree(&repo, &index_map)?;
    let index_commit = repo
        .new_commit(
            format!("index on {branch}: {short} {subject}"),
            index_tree,
            [head_id],
        )?
        .id()
        .detach();
    let untracked_commit = if untracked_map.is_empty() {
        None
    } else {
        let tree = write_map_tree(&repo, &untracked_map)?;
        Some(
            repo.new_commit(
                format!("untracked files on {branch}: {short} {subject}"),
                tree,
                std::iter::empty::<ObjectId>(),
            )?
            .id()
            .detach(),
        )
    };
    let work_tree = write_map_tree(&repo, &work_map)?;
    let mut parents = vec![head_id, index_commit];
    if let Some(untracked) = untracked_commit {
        parents.push(untracked);
    }
    let stash_id = repo
        .new_commit(&stash_message, work_tree, parents)?
        .id()
        .detach();
    let edit = RefEdit::update_with_log(
        "refs/stash".try_into()?,
        Target::Object(stash_id),
        PreviousValue::Any,
        LogChange {
            mode: RefLog::AndReference,
            force_create_reflog: true,
            message: stash_message.clone().into(),
        },
    );
    repo.edit_reference(edit)?;

    if keep_index {
        switch_worktree(&repo, &workdir, &index_map, &head_map)?;
    } else {
        crate::history::reset(root, "HEAD", crate::history::ResetMode::Hard)?;
    }
    if include_untracked {
        for path in untracked_map.keys() {
            let absolute = safe_worktree_join(&workdir, path)?;
            let _ = std::fs::remove_file(&absolute);
            remove_empty_parents(&workdir, Path::new(path));
        }
    }
    Ok(format!(
        "Saved working directory and index state {stash_message}"
    ))
}

/// Apply `target` to the worktree, deleting tracked paths that are no longer
/// present. Used by `stash --keep-index`; the index itself is untouched.
fn switch_worktree(
    repo: &gix::Repository,
    workdir: &Path,
    target: &TreeMap,
    old: &TreeMap,
) -> Result<()> {
    for path in old.keys().chain(target.keys()) {
        safe_worktree_join(workdir, path)?;
    }
    let backup = CheckoutBackup::capture(repo, workdir, old.keys().chain(target.keys()))?;
    let result = (|| -> Result<()> {
        for path in old.keys() {
            if target.contains_key(path) {
                continue;
            }
            let absolute = safe_worktree_join(workdir, path)?;
            match std::fs::symlink_metadata(&absolute) {
                Ok(metadata) if metadata.is_dir() => {
                    anyhow::bail!("refusing to remove directory {}", absolute.display());
                }
                Ok(_) => {
                    std::fs::remove_file(&absolute)?;
                    remove_empty_parents(workdir, Path::new(path));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        materialize_tree(repo, target)
    })();
    if let Err(error) = result {
        if let Err(restore_error) = backup.restore() {
            log::error!("stash worktree rollback failed: {restore_error}");
        }
        return Err(error);
    }
    backup.discard();
    Ok(())
}

fn stash_trees(
    repo: &gix::Repository,
    stash: ObjectId,
) -> Result<(ObjectId, TreeMap, TreeMap, TreeMap)> {
    let commit = repo.find_commit(stash)?;
    let parents: Vec<ObjectId> = commit.parent_ids().map(|id| id.detach()).collect();
    let base_id = *parents
        .first()
        .ok_or_else(|| anyhow!("stash has no base commit"))?;
    let base = commit_tree_map(repo, base_id)?;
    let index = match parents.get(1) {
        Some(index) => commit_tree_map(repo, *index)?,
        None => base.clone(),
    };
    let mut new = commit_tree_map(repo, stash)?;
    if let Some(untracked) = parents.get(2) {
        new.extend(commit_tree_map(repo, *untracked)?);
    }
    Ok((base_id, base, index, new))
}

pub fn stash_files(root: &Path, stash: &str) -> Result<Vec<String>> {
    let repo = discover(root)?;
    let stash = stash_oid(&repo, stash)?;
    let (_, base, _, new) = stash_trees(&repo, stash)?;
    let mut paths = Vec::new();
    for path in new.keys().chain(base.keys()) {
        if new.get(path) != base.get(path) {
            paths.push(path.clone());
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// 贮藏内容按 PyCharm 的两个树根拆分：已跟踪改动 + 未跟踪文件快照。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StashDetails {
    /// Paths whose tracked content differs between the stash base and the
    /// stashed worktree state (sorted).
    pub tracked: Vec<String>,
    /// Paths captured by `git stash push --include-untracked` (sorted).
    pub untracked: Vec<String>,
}

pub fn stash_details(root: &Path, stash: &str) -> Result<StashDetails> {
    let repo = discover(root)?;
    let stash = stash_oid(&repo, stash)?;
    let commit = repo.find_commit(stash)?;
    let mut parents = commit.parent_ids().map(|id| id.detach());
    let base_id = parents
        .next()
        .ok_or_else(|| anyhow!("stash has no base commit"))?;
    let base = commit_tree_map(&repo, base_id)?;
    let stashed = commit_tree_map(&repo, stash)?;
    let mut tracked = Vec::new();
    for path in stashed.keys().chain(base.keys()) {
        if stashed.get(path) != base.get(path) {
            tracked.push(path.clone());
        }
    }
    tracked.sort();
    tracked.dedup();
    let _index_parent = parents.next();
    let mut untracked: Vec<String> = match parents.next() {
        Some(id) => commit_tree_map(&repo, id)?.keys().cloned().collect(),
        None => Vec::new(),
    };
    untracked.sort();
    untracked.dedup();
    Ok(StashDetails { tracked, untracked })
}

/// 单个文件在贮藏中的差异：基线与贮藏内容（含未跟踪快照）对比。
pub fn stash_file_diff(root: &Path, stash: &str, path: &str) -> Result<crate::model::FileDiff> {
    use crate::diff::{build_file_diff, load_blob_capped};
    let repo = discover(root)?;
    let spec = stash_oid(&repo, stash)?;
    let (_, base, _, stashed) = stash_trees(&repo, spec)?;
    crate::ops::validate_repo_path(path)?;
    let old_entry = base.get(path);
    let new_entry = stashed.get(path);
    let old = old_entry
        .map(|entry| load_blob_capped(&repo, entry.oid))
        .unwrap_or_default();
    let new = new_entry
        .map(|entry| load_blob_capped(&repo, entry.oid))
        .unwrap_or_default();
    Ok(build_file_diff(
        path.to_string(),
        old,
        new,
        old_entry.is_some(),
        new_entry.is_some(),
    ))
}

fn read_worktree(path: &Path) -> Result<Option<(Vec<u8>, u32)>> {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Ok(Some((
            std::fs::read_link(path)?
                .to_string_lossy()
                .into_owned()
                .into_bytes(),
            0o120000,
        ))),
        Ok(metadata) if metadata.is_file() => Ok(Some((
            std::fs::read(path)?,
            if metadata.permissions().mode() & 0o111 != 0 {
                0o100755
            } else {
                0o100644
            },
        ))),
        Ok(_) => anyhow::bail!("{} is not a regular file", path.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn entry_worktree(
    repo: &gix::Repository,
    path: &str,
    entry: &TreeEntry,
    filters: &mut crate::filter::FilterContext<'_>,
) -> Result<(Vec<u8>, u32)> {
    let data = blob_bytes(repo, entry.oid)?;
    if entry.mode == 0o120000 {
        Ok((data, entry.mode))
    } else {
        let data = filters.smudge(path, &entry.oid.to_string(), data)?;
        Ok((data, entry.mode))
    }
}

enum StashAction {
    Write { mode: u32, payload: WritePayload },
    WriteLfs { mode: u32, pointer: Vec<u8> },
    Delete,
}

/// Plain writes are smudged at write time so a stash touching
/// `.gitattributes` applies the new attributes to the remaining files.
enum WritePayload {
    Blob(gix::hash::ObjectId),
    Merged(Vec<u8>),
}

fn lfs_entry(
    repo: &gix::Repository,
    filters: &mut crate::filter::FilterContext<'_>,
    path: &str,
    entry: &TreeEntry,
) -> Result<Option<(Vec<u8>, String, u64)>> {
    if entry.mode == 0o120000 {
        return Ok(None);
    }
    let data = blob_bytes(repo, entry.oid)?;
    let Some((oid, size)) = filters.lfs_pointer_info(path, &data)? else {
        return Ok(None);
    };
    if filters.lfs_object_path(&oid).is_file() {
        Ok(Some((data, oid, size)))
    } else {
        Ok(None)
    }
}

fn file_matches_lfs(path: &Path, oid: &str, size: u64) -> bool {
    use std::io::Read;
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if !metadata.is_file() || metadata.len() != size {
        return false;
    }
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut hasher = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buffer = [0u8; 64 * 1024];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => hasher.update(&buffer[..read]),
            Err(_) => return false,
        }
    }
    let digest = hasher.finish();
    let mut actual = String::with_capacity(64);
    for byte in digest.as_ref() {
        use std::fmt::Write as _;
        let _ = write!(actual, "{byte:02x}");
    }
    actual == oid
}

fn current_matches_entry(
    repo: &gix::Repository,
    workdir: &Path,
    path: &str,
    entry: Option<&TreeEntry>,
    filters: &mut crate::filter::FilterContext<'_>,
) -> Result<bool> {
    let absolute = safe_worktree_join(workdir, path)?;
    let Some(entry) = entry else {
        return Ok(std::fs::symlink_metadata(&absolute).is_err());
    };
    if let Some((_, oid, size)) = lfs_entry(repo, filters, path, entry)? {
        return Ok(file_matches_lfs(&absolute, &oid, size));
    }
    let expected = entry_worktree(repo, path, entry, filters)?;
    Ok(read_worktree(&absolute)? == Some(expected))
}

struct StashConflict {
    path: String,
    base: Option<TreeEntry>,
    ours: Option<TreeEntry>,
    theirs: Option<TreeEntry>,
}

fn ours_entry(
    repo: &gix::Repository,
    index: &TreeMap,
    path: &str,
    current: Option<&(Vec<u8>, u32)>,
) -> Result<Option<TreeEntry>> {
    if let Some(entry) = index.get(path) {
        return Ok(Some(*entry));
    }
    match current {
        Some((data, mode)) => Ok(Some(TreeEntry {
            oid: repo.write_blob(data)?.detach(),
            mode: *mode,
        })),
        None => Ok(None),
    }
}

fn write_conflict_index(repo: &gix::Repository, conflicts: &[StashConflict]) -> Result<()> {
    let file = open_index(repo)?;
    let index_path = file.path().to_path_buf();
    let (mut state, _) = file.into_parts();
    for conflict in conflicts {
        state.remove_entries(|_, candidate, _| {
            candidate
                .to_str()
                .map(|path| path == conflict.path)
                .unwrap_or(false)
        });
        for (stage, entry) in [
            (Stage::Base, conflict.base),
            (Stage::Ours, conflict.ours),
            (Stage::Theirs, conflict.theirs),
        ] {
            if let Some(entry) = entry {
                state.dangerously_push_entry(
                    Stat::default(),
                    entry.oid,
                    Flags::from_stage(stage),
                    Mode::from_bits_truncate(entry.mode),
                    conflict.path.as_str().into(),
                );
            }
        }
    }
    state.remove_tree();
    state.sort_entries();
    write_index(gix::index::File::from_state(state, index_path))
}

fn merge_contents(ancestor: &[u8], current: &[u8], other: &[u8]) -> Option<(Vec<u8>, bool)> {
    let binary = |data: &[u8]| data.iter().take(8000).any(|byte| *byte == 0);
    if binary(ancestor) || binary(current) || binary(other) {
        return None;
    }
    let mut input = imara_diff::InternedInput::new(&[][..], &[]);
    let mut output = Vec::new();
    let labels = gix::merge::blob::builtin_driver::text::Labels {
        ancestor: Some(gix::bstr::BStr::new("base")),
        current: Some(gix::bstr::BStr::new("Updated upstream")),
        other: Some(gix::bstr::BStr::new("Stashed changes")),
    };
    let resolution = gix::merge::blob::builtin_driver::text(
        &mut output,
        &mut input,
        labels,
        current,
        ancestor,
        other,
        gix::merge::blob::builtin_driver::text::Options::default(),
    );
    Some((output, resolution == gix::merge::blob::Resolution::Conflict))
}

/// Stash apply switches: restrict the restore to a path subset
/// ("Cherry-Pick Selected Changes") and optionally re-create the staged
/// state recorded by the stash (`Reinstate index`).
#[derive(Clone, Debug, Default)]
pub struct StashApplyOptions {
    /// Exact repo-relative paths to restore; `None` = the whole stash.
    pub paths: Option<Vec<String>>,
    /// Write the stash's index version of the applied files back to the
    /// real index (`git stash apply --index` semantics).
    pub reinstate_index: bool,
}

pub fn stash_apply(root: &Path, stash: &str) -> Result<String> {
    stash_apply_with(root, stash, &StashApplyOptions::default())
}

/// Restore only `paths` from the stash (subtree lists are expanded by the
/// caller); no index side effects.
pub fn stash_apply_paths(root: &Path, stash: &str, paths: &[String]) -> Result<String> {
    stash_apply_with(
        root,
        stash,
        &StashApplyOptions {
            paths: Some(paths.to_vec()),
            reinstate_index: false,
        },
    )
}

pub fn stash_apply_with(root: &Path, stash: &str, options: &StashApplyOptions) -> Result<String> {
    let repo = discover(root)?;
    let workdir = workdir(&repo)?;
    let stash_id = stash_oid(&repo, stash)?;
    let (_, base, index, new) = stash_trees(&repo, stash_id)?;
    let mut filters = crate::filter::FilterContext::new(&repo)?;
    let mut actions: Vec<(String, StashAction)> = Vec::new();
    let mut conflicts: Vec<StashConflict> = Vec::new();
    for path in new.keys().chain(base.keys()) {
        if options
            .paths
            .as_ref()
            .is_some_and(|paths| !paths.iter().any(|wanted| wanted == path))
        {
            continue;
        }
        let old = base.get(path);
        let new_entry = new.get(path);
        if old == new_entry {
            continue;
        }
        let absolute = safe_worktree_join(&workdir, path)?;
        if let Some((pointer, oid, size)) = new_entry
            .map(|entry| lfs_entry(&repo, &mut filters, path, entry))
            .transpose()?
            .flatten()
        {
            if file_matches_lfs(&absolute, &oid, size) {
                continue;
            }
            let old_matches = current_matches_entry(&repo, &workdir, path, old, &mut filters)?;
            let index_matches =
                current_matches_entry(&repo, &workdir, path, index.get(path), &mut filters)?;
            if old_matches || index_matches {
                actions.push((
                    path.clone(),
                    StashAction::WriteLfs {
                        mode: new_entry.expect("new entry").mode,
                        pointer,
                    },
                ));
            } else {
                conflicts.push(StashConflict {
                    path: path.clone(),
                    base: old.copied(),
                    ours: index.get(path).copied(),
                    theirs: new_entry.copied(),
                });
            }
            continue;
        }
        let current = read_worktree(&absolute)?;
        let old_data = old
            .map(|entry| entry_worktree(&repo, path, entry, &mut filters))
            .transpose()?;
        let index_data = index
            .get(path)
            .map(|entry| entry_worktree(&repo, path, entry, &mut filters))
            .transpose()?;
        let new_data = new_entry
            .map(|entry| entry_worktree(&repo, path, entry, &mut filters))
            .transpose()?;
        if current == new_data {
            continue;
        }
        if current == old_data || current == index_data {
            match new_entry {
                Some(entry) => actions.push((
                    path.clone(),
                    StashAction::Write {
                        mode: entry.mode,
                        payload: WritePayload::Blob(entry.oid),
                    },
                )),
                None => actions.push((path.clone(), StashAction::Delete)),
            }
            continue;
        }

        // Modified on both sides: perform a real three-way merge and write
        // conflict markers when gix cannot resolve it automatically.
        let merge_base = old_data
            .as_ref()
            .map(|(data, _)| data.as_slice())
            .unwrap_or(&[]);
        let current_data = current
            .as_ref()
            .map(|(data, _)| data.as_slice())
            .unwrap_or(&[]);
        if let Some((data, conflict)) = new_entry
            .and_then(|entry| new_data.as_ref().map(|(data, _)| (entry, data)))
            .and_then(|(entry, data)| {
                merge_contents(merge_base, current_data, data).map(|(merged, conflict)| {
                    (
                        StashAction::Write {
                            mode: entry.mode,
                            payload: WritePayload::Merged(merged),
                        },
                        conflict,
                    )
                })
            })
        {
            actions.push((path.clone(), data));
            if conflict {
                conflicts.push(StashConflict {
                    path: path.clone(),
                    base: old.copied(),
                    ours: ours_entry(&repo, &index, path, current.as_ref())?,
                    theirs: new_entry.copied(),
                });
            }
            continue;
        }

        // Delete/modify or binary add/add: keep the current side and report a
        // conflict; Git leaves the index unmerged in this case.
        conflicts.push(StashConflict {
            path: path.clone(),
            base: old.copied(),
            ours: ours_entry(&repo, &index, path, current.as_ref())?,
            theirs: new_entry.copied(),
        });
    }
    if actions.is_empty() && conflicts.is_empty() {
        return Ok("stash applied".to_string());
    }
    let applied_paths: Vec<String> = actions.iter().map(|(path, _)| path.clone()).collect();
    let backup = CheckoutBackup::capture(&repo, &workdir, actions.iter().map(|(path, _)| path))?;
    let result = (|| -> Result<()> {
        let mut filters = crate::filter::FilterContext::new(&repo)?;
        for (path, action) in actions {
            let absolute = safe_worktree_join(&workdir, &path)?;
            match action {
                StashAction::Delete => {
                    std::fs::remove_file(&absolute)?;
                    remove_empty_parents(&workdir, Path::new(&path));
                }
                StashAction::WriteLfs { mode, pointer } => {
                    if let Some(parent) = absolute.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    if !crate::filter::smudge_lfs_to_file_in(&repo, &path, &pointer, &absolute)? {
                        lazecode_fs::file::write_atomic(&absolute, &pointer)?;
                    }
                    use std::os::unix::fs::PermissionsExt;
                    let perms = if mode & 0o111 != 0 { 0o755 } else { 0o644 };
                    std::fs::set_permissions(&absolute, std::fs::Permissions::from_mode(perms))?;
                }
                StashAction::Write { mode, payload } => {
                    let data = match payload {
                        WritePayload::Blob(oid) => {
                            // Smudge with current attributes so a
                            // `.gitattributes` updated earlier in this stash
                            // governs the remaining files, like checkout.
                            let blob = blob_bytes(&repo, oid)?;
                            filters.smudge(&path, &oid.to_string(), blob)?
                        }
                        WritePayload::Merged(data) => data,
                    };
                    if let Some(parent) = absolute.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    if mode == 0o120000 {
                        let _ = std::fs::remove_file(&absolute);
                        std::os::unix::fs::symlink(
                            String::from_utf8_lossy(&data).to_string(),
                            &absolute,
                        )?;
                    } else {
                        lazecode_fs::file::write_atomic(&absolute, &data)?;
                        use std::os::unix::fs::PermissionsExt;
                        let perms = if mode & 0o111 != 0 { 0o755 } else { 0o644 };
                        std::fs::set_permissions(
                            &absolute,
                            std::fs::Permissions::from_mode(perms),
                        )?;
                    }
                }
            }
            // Materialising `.gitattributes` changes later files' semantics;
            // rebuild the shared attribute session.
            if path == ".gitattributes" || path.ends_with("/.gitattributes") {
                filters = crate::filter::FilterContext::new(&repo)?;
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        if let Err(restore_error) = backup.restore() {
            log::error!("stash apply rollback failed: {restore_error}");
        }
        return Err(error);
    }
    backup.discard();
    let conflicted: Vec<String> = conflicts.iter().map(|c| c.path.clone()).collect();
    if !conflicts.is_empty() {
        write_conflict_index(&repo, &conflicts)?;
        conflicts.sort_by(|a, b| a.path.cmp(&b.path));
    }
    if options.reinstate_index {
        let reinstate: Vec<String> = applied_paths
            .into_iter()
            .filter(|path| !conflicted.contains(path))
            .collect();
        write_stash_index(&repo, &base, &index, &reinstate)?;
    }
    if !conflicts.is_empty() {
        return Err(anyhow!(
            "stash applied with conflicts: {}",
            conflicts
                .iter()
                .map(|conflict| conflict.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok("stash applied".to_string())
}

/// Restore the staged state the stash recorded: for each applied path whose
/// stash-index entry differs from the stash base, write that entry (or its
/// removal) into the real index. Paths without staged changes are left alone,
/// so the user's current staging is preserved — `git stash apply --index`.
fn write_stash_index(
    repo: &gix::Repository,
    base: &TreeMap,
    index: &TreeMap,
    paths: &[String],
) -> Result<()> {
    let relevant: Vec<&String> = paths
        .iter()
        .filter(|path| base.get(*path) != index.get(*path))
        .collect();
    if relevant.is_empty() {
        return Ok(());
    }
    let file = open_index(repo)?;
    let index_path = file.path().to_path_buf();
    let (mut state, _) = file.into_parts();
    for path in relevant {
        state.remove_entries(|_, candidate, _| candidate.to_str().ok() == Some(path.as_str()));
        if let Some(entry) = index.get(path) {
            state.dangerously_push_entry(
                Stat::default(),
                entry.oid,
                Flags::empty(),
                Mode::from_bits_truncate(entry.mode),
                path.as_str().into(),
            );
        }
    }
    state.remove_tree();
    state.sort_entries();
    write_index(gix::index::File::from_state(state, index_path))
}

/// A stashed file's "before" version (the stash base blob) against the
/// current working-tree file — the reference UX's "Compare Before with Local".
pub fn stash_before_local_diff(root: &Path, stash: &str, path: &str) -> Result<crate::model::FileDiff> {
    use crate::diff::{build_file_diff, load_blob_capped};
    let repo = discover(root)?;
    let workdir = workdir(&repo)?;
    let spec = stash_oid(&repo, stash)?;
    let (_, base, _, _) = stash_trees(&repo, spec)?;
    crate::ops::validate_repo_path(path)?;
    let old_entry = base.get(path);
    let old = old_entry
        .map(|entry| load_blob_capped(&repo, entry.oid))
        .unwrap_or_default();
    let abs = safe_worktree_join(&workdir, path)?;
    let (new, new_exists) = match read_worktree(&abs)? {
        Some((data, _)) => (data, true),
        None => (Vec::new(), false),
    };
    Ok(build_file_diff(
        path.to_string(),
        old,
        new,
        old_entry.is_some(),
        new_exists,
    ))
}

/// File content as committed at HEAD — the read-only "Open Repository
/// Version" tab. Raw blob bytes: line endings/filters stay repository-side.
pub fn head_file_bytes(root: &Path, path: &str) -> Result<Vec<u8>> {
    let repo = discover(root)?;
    crate::ops::validate_repo_path(path)?;
    let map = head_tree_map(&repo)?;
    let entry = map
        .get(path)
        .ok_or_else(|| anyhow!("{path} is not tracked in HEAD"))?;
    blob_bytes(&repo, entry.oid)
}

pub fn stash_pop(root: &Path, stash: &str) -> Result<String> {
    let result = stash_apply(root, stash)?;
    stash_drop(root, stash)?;
    Ok(result)
}

pub fn stash_drop(root: &Path, stash: &str) -> Result<String> {
    let index = parse_stash_index(stash).ok_or_else(|| anyhow!("invalid stash name '{stash}'"))?;
    let repo = discover(root)?;
    let common = repo.common_dir();
    let log_path = common.join("logs/refs/stash");
    let content =
        std::fs::read_to_string(&log_path).map_err(|_| anyhow!("no stash reflog entries"))?;
    let mut lines: Vec<&str> = content.lines().collect();
    if index >= lines.len() {
        anyhow::bail!("stash entry {stash} does not exist");
    }
    lines.remove(lines.len() - 1 - index);
    let ref_path = common.join("refs/stash");
    if lines.is_empty() {
        // Delete through gix first so a packed refs/stash cannot reappear
        // when only the loose file is removed.
        if let Ok(reference) = repo.find_reference("refs/stash") {
            let _ = reference.delete();
        }
        let _ = std::fs::remove_file(&log_path);
        let _ = std::fs::remove_file(&ref_path);
        return Ok(format!("Dropped {stash}"));
    }
    let mut log = lines.join("\n");
    log.push('\n');
    lazecode_fs::file::write_atomic(&log_path, log.as_bytes())?;
    let top = lines
        .last()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| anyhow!("corrupt stash reflog"))?;
    lazecode_fs::file::write_atomic(&ref_path, format!("{top}\n").as_bytes())?;
    Ok(format!("Dropped {stash}"))
}

// ---------------- shelves ----------------

pub const SHELVES_DIR: &str = "lazecode-shelves";

#[derive(Clone, Debug)]
pub struct ShelfEntry {
    pub name: String,
    /// The unified patch text of the shelf.
    pub patch: String,
    pub created: u64,
}

fn shelves_dir(root: &Path) -> std::path::PathBuf {
    match discover(root) {
        Ok(repo) => repo.git_dir().join(SHELVES_DIR),
        Err(_) => Path::new(".git").join(SHELVES_DIR),
    }
}

/// Shelf names become file names under `.git/lazecode-shelves`; reject
/// separators, `..`, control chars and `.`/`..`.
fn sanitize_shelf_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains("..")
        || name.contains('/')
        || name.contains('\\')
        || name.contains(':')
        || name.chars().any(char::is_control)
    {
        anyhow::bail!("unsuitable shelf name '{name}'");
    }
    Ok(name.to_string())
}

fn shelf_path(root: &Path, name: &str) -> Result<std::path::PathBuf> {
    Ok(shelves_dir(root).join(format!("{}.patch", sanitize_shelf_name(name)?)))
}

pub fn shelf_list(root: &Path) -> Result<Vec<ShelfEntry>> {
    let dir = shelves_dir(root);
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(out),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("patch") {
            continue;
        }
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let patch = std::fs::read_to_string(&path).unwrap_or_default();
        let created = entry
            .metadata()
            .ok()
            .and_then(|meta| meta.created().ok())
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        out.push(ShelfEntry {
            name,
            patch,
            created,
        });
    }
    out.sort_by_key(|entry| std::cmp::Reverse(entry.created));
    Ok(out)
}

/// Shelve tracked/untracked changes as a named patch; empty `paths` means all
/// tracked changes. Restore reuses `discard_file_changes` (index + worktree).
pub fn shelf_save(root: &Path, name: &str) -> Result<()> {
    shelf_save_paths(root, name, &[])
}

pub fn shelf_save_paths(root: &Path, name: &str, paths: &[String]) -> Result<()> {
    let name = sanitize_shelf_name(name)?;
    let name = name.as_str();
    let selected: Vec<String> = if paths.is_empty() {
        crate::status::changed_paths(root)?
            .into_iter()
            .map(|change| change.path)
            .collect()
    } else {
        paths.to_vec()
    };
    let output = diff_patch_paths(root, &selected)?;
    if output.trim().is_empty() {
        anyhow::bail!("nothing to shelve (no tracked changes)");
    }
    let mut output = output;
    if !output.ends_with('\n') {
        output.push('\n');
    }
    let dir = shelves_dir(root);
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(format!("{name}.patch"));
    if file.exists() {
        anyhow::bail!("shelf '{name}' already exists");
    }
    lazecode_fs::file::write_atomic(&file, output.as_bytes())
        .map_err(|err| anyhow!("cannot write shelf: {err}"))?;
    // Retract exactly the shelved paths; reuse single-file discard so
    // staged-new files leave both disk and index.
    let retract: Vec<String> = if paths.is_empty() {
        patch_paths(&output)
    } else {
        paths.to_vec()
    };
    let mut first_error: Option<anyhow::Error> = None;
    for path in &retract {
        if let Err(err) = crate::ops::discard_file_changes(root, path) {
            // 部分撤回失败时不中断其余路径（与稳定版逐文件回滚一致），
            // 但必须把首错误暴露给调用方，避免「看起来架藏成功」。
            if first_error.is_none() {
                first_error = Some(err);
            }
        }
    }
    if let Some(err) = first_error {
        return Err(err);
    }
    Ok(())
}

/// Unified patch limited to `paths`: `git diff HEAD` for tracked files plus a
/// `/dev/null` pseudo-diff per untracked file.
pub fn diff_patch_paths(root: &Path, paths: &[String]) -> Result<String> {
    if paths.is_empty() {
        return Ok(String::new());
    }
    let repo = discover(root)?;
    let workdir = workdir(&repo)?;
    let head_map = head_tree_map(&repo)?;
    let mut filters = crate::filter::FilterContext::new(&repo)?;
    let mut output = String::new();
    for raw in paths {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let relative = crate::ops::validate_repo_path(raw)?;
        let path = relative.to_string_lossy().replace('\\', "/");
        let absolute = safe_worktree_join(&workdir, &path)?;
        let old = head_map.get(&path).copied();
        let new = worktree_entry(&repo, &path, &absolute, &mut filters)?;
        if old == new {
            continue;
        }
        output.push_str(&render_patch_section(&repo, &path, old, new)?);
    }
    Ok(output)
}

fn render_patch_section(
    repo: &gix::Repository,
    path: &str,
    old: Option<TreeEntry>,
    new: Option<TreeEntry>,
) -> Result<String> {
    let mut out = String::new();
    out.push_str(&format!("diff --git a/{path} b/{path}\n"));
    match (old, new) {
        (None, Some(entry)) => out.push_str(&format!("new file mode {:06o}\n", entry.mode)),
        (Some(entry), None) => out.push_str(&format!("deleted file mode {:06o}\n", entry.mode)),
        (Some(old), Some(new)) if old.mode != new.mode => {
            out.push_str(&format!(
                "old mode {:06o}\nnew mode {:06o}\n",
                old.mode, new.mode
            ));
        }
        _ => {}
    }
    match old {
        Some(_) => out.push_str(&format!("--- a/{path}\n")),
        None => out.push_str("--- /dev/null\n"),
    }
    match new {
        Some(_) => out.push_str(&format!("+++ b/{path}\n")),
        None => out.push_str("+++ /dev/null\n"),
    }

    let old_data = old
        .map(|entry| blob_bytes(repo, entry.oid))
        .transpose()?
        .unwrap_or_default();
    let new_data = new
        .map(|entry| blob_bytes(repo, entry.oid))
        .transpose()?
        .unwrap_or_default();
    if old_data == new_data {
        return Ok(out);
    }
    let is_binary = |bytes: &[u8]| bytes.iter().take(8000).any(|byte| *byte == 0);
    if is_binary(&old_data) || is_binary(&new_data) {
        anyhow::bail!("binary files are not supported in shelf patches");
    }
    let old_text = std::str::from_utf8(&old_data)
        .map_err(|_| anyhow!("non-UTF8 files are not supported in shelf patches"))?;
    let new_text = std::str::from_utf8(&new_data)
        .map_err(|_| anyhow!("non-UTF8 files are not supported in shelf patches"))?;
    let diff = lazecode_text::diff::diff_text_with_context(old_text, new_text, 3);
    let old_total = text_line_count(old_text);
    let new_total = text_line_count(new_text);
    let old_no_newline = !old_text.is_empty() && !old_text.ends_with('\n');
    let new_no_newline = !new_text.is_empty() && !new_text.ends_with('\n');
    for hunk in &diff.hunks {
        out.push_str(&hunk.header);
        out.push('\n');
        for line in &hunk.lines {
            let sign = match line.kind {
                lazecode_text::diff::DiffLineKind::Context => ' ',
                lazecode_text::diff::DiffLineKind::Add => '+',
                lazecode_text::diff::DiffLineKind::Delete => '-',
            };
            out.push(sign);
            out.push_str(&line.text);
            out.push('\n');
            let no_newline = match line.kind {
                lazecode_text::diff::DiffLineKind::Context => {
                    (old_no_newline && line.old_lineno == Some(old_total as u32))
                        || (new_no_newline && line.new_lineno == Some(new_total as u32))
                }
                lazecode_text::diff::DiffLineKind::Delete => {
                    old_no_newline && line.old_lineno == Some(old_total as u32)
                }
                lazecode_text::diff::DiffLineKind::Add => {
                    new_no_newline && line.new_lineno == Some(new_total as u32)
                }
            };
            if no_newline {
                out.push_str("\\ No newline at end of file\n");
            }
        }
    }
    Ok(out)
}

fn text_line_count(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.matches('\n').count() + usize::from(!text.ends_with('\n'))
    }
}

/// Paths touched by a shelf patch (the `b/` side of each `diff --git`
/// header), used by the unshelve dialog for per-file selection.
pub fn shelf_files(root: &Path, name: &str) -> Result<Vec<String>> {
    let patch = shelf_patch(root, name)?;
    Ok(patch_paths(&patch))
}

/// New-side path of every file section; `--no-index` creation sections still
/// name the real path in the `diff --git` header.
fn patch_paths(patch: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("diff --git a/") {
            if let Some((_, right)) = rest.split_once(" b/") {
                paths.push(right.to_string());
            }
        }
    }
    paths
}

pub fn shelf_delete(root: &Path, name: &str) -> Result<()> {
    // 稳定版语义：删除先进入「Recently Deleted」，可恢复、可彻底清除。
    let name = sanitize_shelf_name(name)?;
    let dir = shelves_dir(root);
    let file = dir.join(format!("{name}.patch"));
    let deleted_dir = dir.join("deleted");
    std::fs::create_dir_all(&deleted_dir)
        .map_err(|err| anyhow!("cannot create deleted dir: {err}"))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let target = deleted_dir.join(format!("{name}.{stamp}.patch"));
    std::fs::rename(&file, &target).map_err(|err| anyhow!("cannot delete shelf: {err}"))
}

/// 「Recently Deleted」列表：`deleted/` 下的补丁（同名取最新一次删除）。
pub fn shelf_list_deleted(root: &Path) -> Result<Vec<ShelfEntry>> {
    let deleted_dir = shelves_dir(root).join("deleted");
    let mut entries: Vec<ShelfEntry> = Vec::new();
    let Ok(read) = std::fs::read_dir(&deleted_dir) else {
        return Ok(entries);
    };
    for item in read.flatten() {
        let file_name = item.file_name().to_string_lossy().to_string();
        let Some(base) = file_name.strip_suffix(".patch") else {
            continue;
        };
        // `{name}.{stamp}.patch` → name + stamp 作为删除时间。
        let Some((name, stamp)) = base.rsplit_once('.') else {
            continue;
        };
        let created = stamp.parse::<u64>().unwrap_or(0);
        if let Ok(patch) = std::fs::read_to_string(item.path()) {
            entries.push(ShelfEntry {
                name: name.to_string(),
                patch,
                created,
            });
        }
    }
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.created));
    entries.dedup_by(|a, b| a.name == b.name);
    Ok(entries)
}

/// 从「Recently Deleted」恢复架藏（重命名回主目录）。
pub fn shelf_restore_deleted(root: &Path, name: &str) -> Result<()> {
    let name = sanitize_shelf_name(name)?;
    let dir = shelves_dir(root);
    let deleted_dir = dir.join("deleted");
    let mut best: Option<(std::path::PathBuf, u64)> = None;
    if let Ok(read) = std::fs::read_dir(&deleted_dir) {
        for item in read.flatten() {
            let file_name = item.file_name().to_string_lossy().to_string();
            let Some(base) = file_name.strip_suffix(".patch") else {
                continue;
            };
            if let Some((deleted_name, stamp)) = base.rsplit_once('.') {
                if deleted_name == name {
                    let stamp = stamp.parse::<u64>().unwrap_or(0);
                    if best
                        .as_ref()
                        .is_none_or(|(_, best_stamp)| stamp > *best_stamp)
                    {
                        best = Some((item.path(), stamp));
                    }
                }
            }
        }
    }
    let Some((path, _)) = best else {
        return Err(anyhow!("no deleted shelf named {name}"));
    };
    let target = dir.join(format!("{name}.patch"));
    if target.exists() {
        return Err(anyhow!("a shelf named {name} already exists"));
    }
    std::fs::rename(&path, &target).map_err(|err| anyhow!("cannot restore shelf: {err}"))
}

/// 彻底删除「最近删除」里的架藏（不可恢复）。
pub fn shelf_purge_deleted(root: &Path, name: &str) -> Result<()> {
    let name = sanitize_shelf_name(name)?;
    let deleted_dir = shelves_dir(root).join("deleted");
    let mut removed = false;
    if let Ok(read) = std::fs::read_dir(&deleted_dir) {
        for item in read.flatten() {
            let file_name = item.file_name().to_string_lossy().to_string();
            let Some(base) = file_name.strip_suffix(".patch") else {
                continue;
            };
            if base
                .rsplit_once('.')
                .is_some_and(|(deleted_name, _)| deleted_name == name)
            {
                let _ = std::fs::remove_file(item.path());
                removed = true;
            }
        }
    }
    if removed {
        Ok(())
    } else {
        Err(anyhow!("no deleted shelf named {name}"))
    }
}

/// 重命名架藏（重命名主目录下的补丁文件）。
pub fn shelf_rename(root: &Path, from: &str, to: &str) -> Result<()> {
    let from = shelf_path(root, from)?;
    let to = shelf_path(root, to)?;
    if !from.exists() {
        return Err(anyhow!("no shelf to rename"));
    }
    if to.exists() {
        return Err(anyhow!("a shelf named {} already exists", to.display()));
    }
    std::fs::rename(&from, &to).map_err(|err| anyhow!("cannot rename shelf: {err}"))
}

pub fn shelf_patch(root: &Path, name: &str) -> Result<String> {
    let file = shelf_path(root, name)?;
    std::fs::read_to_string(&file).map_err(|err| anyhow!("cannot read shelf: {err}"))
}

/// Apply the full (or index-only) patch of a shelf back onto the worktree.
pub fn shelf_restore(root: &Path, name: &str, delete_after: bool) -> Result<()> {
    let patch = shelf_patch(root, name)?;
    crate::apply_patch(root, &patch, false, false)?;
    if delete_after {
        shelf_delete(root, name)?;
    }
    Ok(())
}

/// Apply only the hunks of the shelf that touch `paths` (partial apply).
pub fn shelf_restore_partial(root: &Path, name: &str, paths: &[String]) -> Result<()> {
    let patch = shelf_patch(root, name)?;
    let selected: std::collections::HashSet<&str> = paths
        .iter()
        .map(|path| path.trim())
        .filter(|path| !path.is_empty())
        .collect();
    if selected.is_empty() {
        return Ok(());
    }
    let mut filtered = String::new();
    let mut starts = Vec::new();
    let mut offset = 0usize;
    for line in patch.split_inclusive('\n') {
        if line.starts_with("diff --git ") {
            starts.push(offset);
        }
        offset += line.len();
    }
    starts.push(patch.len());
    if starts.len() == 1 {
        // No `diff --git` headers: treat the whole patch as one section.
        if patch_paths(&patch)
            .iter()
            .any(|path| selected.contains(path.as_str()))
        {
            filtered.push_str(&patch);
        }
    } else {
        for window in starts.windows(2) {
            let section = &patch[window[0]..window[1]];
            if patch_paths(section)
                .iter()
                .any(|path| selected.contains(path.as_str()))
            {
                if !filtered.is_empty() && !filtered.ends_with('\n') {
                    filtered.push('\n');
                }
                filtered.push_str(section);
            }
        }
    }
    if filtered.trim().is_empty() {
        return Ok(());
    }
    crate::apply_patch(root, &filtered, false, false)
}

/// Import an external `.patch` file into the shelf repository.
pub fn shelf_import_patch(root: &Path, patch_path: &Path, name: Option<&str>) -> Result<String> {
    if !patch_path.exists() {
        anyhow::bail!("patch file does not exist");
    }
    let content =
        std::fs::read_to_string(patch_path).map_err(|e| anyhow!("failed to read patch: {e}"))?;
    if content.trim().is_empty() {
        anyhow::bail!("patch file is empty");
    }
    let shelf_name = match name.filter(|n| !n.trim().is_empty()) {
        Some(n) => sanitize_shelf_name(n)?,
        None => sanitize_shelf_name(
            &patch_path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Imported Patch".to_string()),
        )?,
    };
    let dir = shelves_dir(root);
    std::fs::create_dir_all(&dir)?;
    let target = dir.join(format!("{shelf_name}.patch"));
    std::fs::write(&target, content)?;
    Ok(shelf_name)
}
