use std::collections::BTreeSet;
use std::path::Path;

use gix::hash::ObjectId;

use crate::model::{ChangeKind, CommitDetails, FileDiff, FileStat};
use crate::ops::{safe_worktree_join, validate_repo_path};
use crate::status::compute_stats;
use crate::tree::{blob_bytes, commit_tree_map, head_tree_map, TreeEntry};
use crate::{discover, resolve_commit, short_id, workdir, Result};

const MAX_DIFF_BYTES: usize = 4 * 1024 * 1024;

fn load_blob_capped(repo: &gix::Repository, oid: ObjectId) -> Vec<u8> {
    let Ok(header) = repo.find_header(oid) else {
        return Vec::new();
    };
    if header.size() > MAX_DIFF_BYTES as u64 {
        return Vec::new();
    }
    blob_bytes(repo, oid).unwrap_or_default()
}

pub fn file_diff(root: &Path, path: &str) -> Result<FileDiff> {
    validate_repo_path(path)?;
    let repo = discover(root)?;
    let workdir = workdir(&repo)?;
    let head_map = head_tree_map(&repo)?;
    let old = head_map
        .get(path)
        .map(|entry| load_blob_capped(&repo, entry.oid))
        .unwrap_or_default();
    let abs = safe_worktree_join(&workdir, path)?;
    let new = match std::fs::symlink_metadata(&abs) {
        Ok(metadata) if metadata.file_type().is_symlink() => std::fs::read_link(&abs)
            .map(|target| target.to_string_lossy().into_owned().into_bytes())
            .unwrap_or_default(),
        Ok(metadata) if metadata.is_file() && metadata.len() <= MAX_DIFF_BYTES as u64 => {
            std::fs::read(&abs).unwrap_or_default()
        }
        _ => Vec::new(),
    };
    let binary = old.iter().take(8000).any(|b| *b == 0) || new.iter().take(8000).any(|b| *b == 0);
    let exists = std::fs::symlink_metadata(&abs).is_ok();
    let kind = match (head_map.contains_key(path), exists) {
        (false, true) => ChangeKind::Added,
        (true, false) => ChangeKind::Deleted,
        _ => ChangeKind::Modified,
    };
    if binary || old.len() > MAX_DIFF_BYTES || new.len() > MAX_DIFF_BYTES {
        return Ok(FileDiff {
            path: path.to_string(),
            kind,
            old_text: String::new(),
            new_text: String::new(),
            diff: Default::default(),
            binary,
        });
    }
    let old_text = String::from_utf8_lossy(&old).into_owned();
    let new_text = String::from_utf8_lossy(&new).into_owned();
    let diff = lazecode_text::diff::diff_text(&old_text, &new_text);
    Ok(FileDiff {
        path: path.to_string(),
        kind,
        old_text,
        new_text,
        diff,
        binary,
    })
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
