use std::collections::BTreeSet;
use std::path::Path;

use gix::hash::ObjectId;

use crate::model::{ChangeKind, CommitDetails, FileDiff, FileStat};
use crate::ops::{safe_worktree_join, validate_repo_path};
use crate::status::compute_stats;
use crate::tree::{blob_bytes, commit_tree_map, head_tree_map, TreeEntry};
use crate::{discover, resolve_commit, short_id, workdir, Result};

const MAX_DIFF_BYTES: usize = 4 * 1024 * 1024;

pub(crate) fn load_blob_capped(repo: &gix::Repository, oid: ObjectId) -> Vec<u8> {
    let Ok(header) = repo.find_header(oid) else {
        return Vec::new();
    };
    if header.size() > MAX_DIFF_BYTES as u64 {
        return Vec::new();
    };
    blob_bytes(repo, oid).unwrap_or_default()
}

pub(crate) fn build_file_diff(
    path: String,
    old: Vec<u8>,
    new: Vec<u8>,
    old_exists: bool,
    new_exists: bool,
) -> FileDiff {
    let binary = old.iter().take(8000).any(|b| *b == 0) || new.iter().take(8000).any(|b| *b == 0);
    let kind = match (old_exists, new_exists) {
        (false, true) => ChangeKind::Added,
        (true, false) => ChangeKind::Deleted,
        _ => ChangeKind::Modified,
    };
    if binary || old.len() > MAX_DIFF_BYTES || new.len() > MAX_DIFF_BYTES {
        return FileDiff {
            path,
            kind,
            old_text: String::new(),
            new_text: String::new(),
            diff: Default::default(),
            binary,
        };
    }
    let old_text = String::from_utf8_lossy(&old).into_owned();
    let new_text = String::from_utf8_lossy(&new).into_owned();
    let diff = lazecode_text::diff::diff_text(&old_text, &new_text);
    FileDiff {
        path,
        kind,
        old_text,
        new_text,
        diff,
        binary,
    }
}

pub fn file_diff(root: &Path, path: &str) -> Result<FileDiff> {
    validate_repo_path(path)?;
    let repo = discover(root)?;
    let workdir = workdir(&repo)?;
    let head_map = head_tree_map(&repo)?;
    let old_entry = head_map.get(path);
    let old = old_entry
        .map(|entry| load_blob_capped(&repo, entry.oid))
        .unwrap_or_default();
    let abs = safe_worktree_join(&workdir, path)?;
    let exists = std::fs::symlink_metadata(&abs).is_ok();
    let new = match std::fs::symlink_metadata(&abs) {
        Ok(metadata) if metadata.file_type().is_symlink() => std::fs::read_link(&abs)
            .map(|target| target.to_string_lossy().into_owned().into_bytes())
            .unwrap_or_default(),
        Ok(metadata) if metadata.is_file() && metadata.len() <= MAX_DIFF_BYTES as u64 => {
            std::fs::read(&abs).unwrap_or_default()
        }
        _ => Vec::new(),
    };
    Ok(build_file_diff(
        path.to_string(),
        old,
        new,
        old_entry.is_some(),
        exists,
    ))
}

pub fn commit_file_diff(root: &Path, commit_spec: &str, path: &str) -> Result<FileDiff> {
    let repo = discover(root)?;
    let id = resolve_commit(&repo, commit_spec)?;
    let commit = repo.find_commit(id)?;
    let parent_id = commit.parent_ids().next().map(|id| id.detach());
    let parent_map = if let Some(parent_id) = parent_id {
        commit_tree_map(&repo, parent_id)?
    } else {
        Default::default()
    };
    let current_map = commit_tree_map(&repo, id)?;

    let target_path = if path.is_empty() {
        let mut diff_path = None;
        for (p, entry) in &current_map {
            if parent_map.get(p) != Some(entry) {
                diff_path = Some(p.clone());
                break;
            }
        }
        if diff_path.is_none() {
            for p in parent_map.keys() {
                if !current_map.contains_key(p) {
                    diff_path = Some(p.clone());
                    break;
                }
            }
        }
        diff_path.unwrap_or_default()
    } else {
        validate_repo_path(path)?;
        path.to_string()
    };

    if target_path.is_empty() {
        return Ok(build_file_diff(
            String::new(),
            Vec::new(),
            Vec::new(),
            false,
            false,
        ));
    }

    let old_entry = parent_map.get(&target_path);
    let new_entry = current_map.get(&target_path);
    let old = old_entry
        .map(|entry| load_blob_capped(&repo, entry.oid))
        .unwrap_or_default();
    let new = new_entry
        .map(|entry| load_blob_capped(&repo, entry.oid))
        .unwrap_or_default();
    Ok(build_file_diff(
        target_path,
        old,
        new,
        old_entry.is_some(),
        new_entry.is_some(),
    ))
}

pub fn diff_commits(root: &Path, from_spec: &str, to_spec: &str, path: &str) -> Result<FileDiff> {
    let repo = discover(root)?;
    let from_id = resolve_commit(&repo, from_spec)?;
    let to_id = resolve_commit(&repo, to_spec)?;
    let from_map = commit_tree_map(&repo, from_id)?;
    let to_map = commit_tree_map(&repo, to_id)?;

    let target_path = if path.is_empty() {
        let mut diff_path = None;
        for (p, entry) in &to_map {
            if from_map.get(p) != Some(entry) {
                diff_path = Some(p.clone());
                break;
            }
        }
        if diff_path.is_none() {
            for p in from_map.keys() {
                if !to_map.contains_key(p) {
                    diff_path = Some(p.clone());
                    break;
                }
            }
        }
        diff_path.unwrap_or_default()
    } else {
        validate_repo_path(path)?;
        path.to_string()
    };

    if target_path.is_empty() {
        return Ok(build_file_diff(
            String::new(),
            Vec::new(),
            Vec::new(),
            false,
            false,
        ));
    }

    let old_entry = from_map.get(&target_path);
    let new_entry = to_map.get(&target_path);
    let old = old_entry
        .map(|entry| load_blob_capped(&repo, entry.oid))
        .unwrap_or_default();
    let new = new_entry
        .map(|entry| load_blob_capped(&repo, entry.oid))
        .unwrap_or_default();
    Ok(build_file_diff(
        target_path,
        old,
        new,
        old_entry.is_some(),
        new_entry.is_some(),
    ))
}

pub fn diff_ref_worktree(root: &Path, ref_spec: &str, path: &str) -> Result<FileDiff> {
    let repo = discover(root)?;
    let workdir = workdir(&repo)?;
    let ref_id = resolve_commit(&repo, ref_spec)?;
    let ref_map = commit_tree_map(&repo, ref_id)?;

    let target_path = if path.is_empty() {
        let snap = crate::status::snapshot(root)?;
        snap.changes
            .first()
            .map(|c| c.path.clone())
            .unwrap_or_default()
    } else {
        validate_repo_path(path)?;
        path.to_string()
    };
    if target_path.is_empty() {
        return Ok(build_file_diff(
            String::new(),
            Vec::new(),
            Vec::new(),
            false,
            false,
        ));
    }
    let old_entry = ref_map.get(&target_path);
    let old = old_entry
        .map(|entry| load_blob_capped(&repo, entry.oid))
        .unwrap_or_default();
    let abs = safe_worktree_join(&workdir, &target_path)?;
    let exists = std::fs::symlink_metadata(&abs).is_ok();
    let new = match std::fs::symlink_metadata(&abs) {
        Ok(metadata) if metadata.file_type().is_symlink() => std::fs::read_link(&abs)
            .map(|target| target.to_string_lossy().into_owned().into_bytes())
            .unwrap_or_default(),
        Ok(metadata) if metadata.is_file() && metadata.len() <= MAX_DIFF_BYTES as u64 => {
            std::fs::read(&abs).unwrap_or_default()
        }
        _ => Vec::new(),
    };
    Ok(build_file_diff(
        target_path,
        old,
        new,
        old_entry.is_some(),
        exists,
    ))
}

pub fn diff_two_files(old_path: &Path, new_path: &Path) -> Result<FileDiff> {
    let old = if old_path.is_file() {
        std::fs::read(old_path).unwrap_or_default()
    } else {
        Vec::new()
    };
    let new = if new_path.is_file() {
        std::fs::read(new_path).unwrap_or_default()
    } else {
        Vec::new()
    };
    let display_name = format!(
        "{} ↔ {}",
        old_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("left"),
        new_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("right"),
    );
    Ok(build_file_diff(
        display_name,
        old,
        new,
        old_path.is_file(),
        new_path.is_file(),
    ))
}

pub fn commit_details(root: &Path, spec: &str) -> Result<CommitDetails> {
    let repo = discover(root)?;
    let id = resolve_commit(&repo, spec)?;
    let commit = repo.find_commit(id)?;
    let raw = commit
        .message_raw()
        .map(|msg| msg.to_string())
        .unwrap_or_default();
    let mut lines = raw.lines();
    let message = lines.next().unwrap_or("").trim().to_string();
    let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    let author = commit.author().ok();
    let author_name = author
        .as_ref()
        .map(|sig| sig.name.to_string())
        .unwrap_or_default();
    let email = author
        .as_ref()
        .map(|sig| sig.email.to_string())
        .unwrap_or_default();
    let time = author
        .as_ref()
        .and_then(|sig| sig.time.split_whitespace().next()?.parse::<i64>().ok())
        .unwrap_or(0);
    let parents: Vec<String> = commit
        .parent_ids()
        .map(|id| id.detach().to_string())
        .collect();
    let parent_map = if let Some(parent) = parents.first() {
        let parent_id = resolve_commit(&repo, parent)?;
        commit_tree_map(&repo, parent_id)?
    } else {
        Default::default()
    };
    let current_map = commit_tree_map(&repo, id)?;
    let paths: BTreeSet<&String> = parent_map.keys().chain(current_map.keys()).collect();
    let mut files = Vec::new();
    for path in paths.into_iter().take(300) {
        let old_entry: Option<&TreeEntry> = parent_map.get(path);
        let new_entry: Option<&TreeEntry> = current_map.get(path);
        let unchanged = match (old_entry, new_entry) {
            (Some(old), Some(new)) => old.oid == new.oid && old.mode == new.mode,
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            continue;
        }
        let old = old_entry
            .map(|entry| load_blob_capped(&repo, entry.oid))
            .unwrap_or_default();
        let new = new_entry
            .map(|entry| load_blob_capped(&repo, entry.oid))
            .unwrap_or_default();
        let kind = match (
            parent_map.contains_key(path),
            current_map.contains_key(path),
        ) {
            (false, true) => ChangeKind::Added,
            (true, false) => ChangeKind::Deleted,
            _ => ChangeKind::Modified,
        };
        let (additions, deletions, binary) = compute_stats(&old, &new);
        files.push(FileStat {
            path: path.clone(),
            kind,
            additions,
            deletions,
            binary,
        });
    }
    let info = crate::model::CommitInfo {
        id: id.to_string(),
        short: short_id(&id),
        message,
        body,
        author: author_name,
        email,
        time,
        parents,
        in_head: None,
        refs: Vec::new(),
        lane: 0,
        lane_count: 1,
        edges: Vec::new(),
    };
    Ok(CommitDetails { info, files })
}

#[allow(dead_code)]
fn _object_id(id: ObjectId) -> ObjectId {
    id
}
