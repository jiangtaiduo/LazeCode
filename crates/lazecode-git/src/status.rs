use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use gix::hash::ObjectId;
use gix::index::entry::Stat;

use crate::model::{Change, ChangeKind, HeadInfo, RepoSnapshot};
use crate::tree::{blob_bytes, head_tree_map, TreeEntry, TreeMap};
use crate::{discover, short_id, workdir, Result};

const MAX_STAT_BYTES: usize = 1024 * 1024;

/// Read the complete repository state shown by the source-control panel.
pub fn snapshot(root: &Path) -> Result<RepoSnapshot> {
    let repo = discover(root)?;
    let workdir = workdir(&repo)?;
    let head = repo.head()?;
    let unborn = head.is_unborn();
    let detached = head.is_detached();
    let branch = head.referent_name().map(|name| name.shorten().to_string());
    let head_id = if unborn {
        None
    } else {
        repo.head_id().ok().map(|id| id.detach())
    };
    let info = head_id.map(|id| head_info(&repo, id)).transpose()?;
    let (ahead, behind) = match (head_id, branch.as_deref()) {
        (Some(head), Some(branch)) => ahead_behind(&repo, head, branch)?,
        _ => (0, 0),
    };
    let changes = changes(&repo, &workdir)?;
    Ok(RepoSnapshot {
        root: workdir,
        branch,
        detached,
        unborn,
        ahead,
        behind,
        changes,
        head: info,
    })
}

fn head_info(repo: &gix::Repository, id: ObjectId) -> Result<HeadInfo> {
    let commit = repo.find_commit(id)?;
    let message = commit.message_raw()?.to_string();
    let subject = message.lines().next().unwrap_or("").trim().to_string();
    let author = commit
        .author()
        .ok()
        .map(|sig| sig.name.to_string())
        .unwrap_or_default();
    let time = commit
        .author()
        .ok()
        .and_then(|sig| sig.time.split_whitespace().next()?.parse::<i64>().ok())
        .unwrap_or(0);
    Ok(HeadInfo {
        id: id.to_string(),
        short: short_id(&id),
        message: subject,
        author,
        time,
    })
}

pub(crate) fn changes(repo: &gix::Repository, workdir: &Path) -> Result<Vec<Change>> {
    let head_map = head_tree_map(repo)?;
    let index = repo.index_or_empty()?;
    let index_entries = index.entries();
    let mut index_map: BTreeMap<String, (ObjectId, u32, Stat)> = BTreeMap::new();
    for entry in index_entries {
        if entry.stage() != gix::index::entry::Stage::Unconflicted {
            continue;
        }
        // Submodules are not materialised by this crate; showing every gitlink
        // as a phantom staged addition would make normal commits impossible.
        if entry.mode == gix::index::entry::Mode::COMMIT {
            continue;
        }
        let path = entry.path(&index).to_string();
        index_map.insert(path, (entry.id, entry.mode.bits(), entry.stat));
    }

    let mut acc: BTreeMap<String, Change> = BTreeMap::new();
    let mut add = |path: String,
                   staged: Option<ChangeKind>,
                   unstaged: Option<ChangeKind>,
                   untracked: bool| {
        let entry = acc.entry(path.clone()).or_insert_with(|| Change {
            path,
            staged: None,
            unstaged: None,
            untracked: false,
            additions: 0,
            deletions: 0,
            binary: false,
        });
        if staged.is_some() {
            entry.staged = staged;
        }
        if unstaged.is_some() {
            entry.unstaged = unstaged;
        }
        entry.untracked |= untracked;
    };

    for (path, (index_oid, index_mode, index_stat)) in &index_map {
        let head_entry = head_map.get(path);
        let staged = match head_entry {
            None => Some(ChangeKind::Added),
            Some(head) if head.oid != *index_oid || head.mode != *index_mode => {
                Some(ChangeKind::Modified)
            }
            Some(_) => None,
        };
        let unstaged =
            match worktree_status(repo, workdir, path, *index_oid, *index_mode, index_stat)? {
                WorktreeStatus::Clean => None,
                WorktreeStatus::Deleted => Some(ChangeKind::Deleted),
                WorktreeStatus::Modified => Some(ChangeKind::Modified),
            };
        if staged.is_some() || unstaged.is_some() {
            add(path.clone(), staged, unstaged, false);
        }
    }

    for path in head_map.keys() {
        if !index_map.contains_key(path) {
            add(path.clone(), Some(ChangeKind::Deleted), None, false);
        }
    }

    // Untracked files. Ignored files stay out of the list, matching `git status`.
    let walker = ignore::WalkBuilder::new(workdir)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .follow_links(false)
        .max_depth(Some(64))
        .filter_entry(|entry| !lazecode_fs::is_default_ignored(entry.file_name()))
        .build();
    for entry in walker {
        let Ok(entry) = entry else { continue };
        if !entry
            .file_type()
            .is_some_and(|ty| ty.is_file() || ty.is_symlink())
        {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(workdir) else {
            continue;
        };
        // On Unix a backslash is a normal filename character; translating it
        // to a separator can turn a harmless filename into a path escape.
        let path = rel.to_string_lossy().into_owned();
        if index_map.contains_key(&path) {
            continue;
        }
        add(path, None, None, true);
    }

    let mut changes: Vec<Change> = acc.into_values().collect();
    for change in &mut changes {
        let (old, new) = file_sides(repo, workdir, &change.path, &head_map);
        let (additions, deletions, binary) = compute_stats(&old, &new);
        change.additions = additions;
        change.deletions = deletions;
        change.binary = binary;
    }
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(changes)
}

enum WorktreeStatus {
    Clean,
    Deleted,
    Modified,
}

fn worktree_status(
    repo: &gix::Repository,
    workdir: &Path,
    path: &str,
    index_oid: ObjectId,
    index_mode: u32,
    index_stat: &Stat,
) -> Result<WorktreeStatus> {
    if index_mode == 0o160000 {
        // Submodule: report changed only when the recorded commit differs.
        return Ok(WorktreeStatus::Clean);
    }
    let abs = workdir.join(path);
    let Ok(meta) = std::fs::symlink_metadata(&abs) else {
        return Ok(WorktreeStatus::Deleted);
    };
    if meta.is_dir() {
        // A tracked file replaced by a directory is a type change, not a
        // clean file. Never let checkout delete a directory it did not track.
        return Ok(WorktreeStatus::Modified);
    }
    if !meta.is_file() && !meta.file_type().is_symlink() {
        // FIFOs and device nodes can block forever in std::fs::read.
        return Ok(WorktreeStatus::Modified);
    }
    let is_symlink = meta.file_type().is_symlink();
    let index_is_symlink = index_mode & 0o170000 == 0o120000;
    if is_symlink != index_is_symlink {
        return Ok(WorktreeStatus::Modified);
    }
    if is_symlink {
        let target = std::fs::read_link(&abs)?;
        let current = target.to_string_lossy().into_owned();
        let recorded = String::from_utf8_lossy(&blob_bytes(repo, index_oid)?).into_owned();
        return Ok(if current == recorded {
            WorktreeStatus::Clean
        } else {
            WorktreeStatus::Modified
        });
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let worktree_executable = meta.permissions().mode() & 0o111 != 0;
        let index_executable = index_mode & 0o111 != 0;
        if worktree_executable != index_executable {
            return Ok(WorktreeStatus::Modified);
        }
    }
    if meta.len() > MAX_STAT_BYTES as u64 {
        return Ok(WorktreeStatus::Modified);
    }
    let stat_matches = gix::index::fs::Metadata::from_path_no_follow(&abs)
        .ok()
        .and_then(|meta| Stat::from_fs(&meta).ok())
        .is_some_and(|stat| stats_equal(&stat, index_stat));
    if stat_matches {
        return Ok(WorktreeStatus::Clean);
    }
    let bytes = std::fs::read(&abs)?;
    if bytes.iter().take(8000).any(|byte| *byte == 0) {
        // Binary: compare by hash-equivalent bytes anyway.
        let recorded = blob_bytes(repo, index_oid)?;
        return Ok(if bytes == recorded {
            WorktreeStatus::Clean
        } else {
            WorktreeStatus::Modified
        });
    }
    let recorded = blob_bytes(repo, index_oid)?;
    Ok(if bytes == recorded {
        WorktreeStatus::Clean
    } else {
        WorktreeStatus::Modified
    })
}

fn stats_equal(a: &Stat, b: &Stat) -> bool {
    a.mtime.secs == b.mtime.secs
        && a.mtime.nsecs == b.mtime.nsecs
        && a.size == b.size
        && a.mtime.secs != 0
}

fn blob_bytes_capped(repo: &gix::Repository, oid: gix::hash::ObjectId) -> Option<Vec<u8>> {
    let header = repo.find_header(oid).ok()?;
    if header.size() > MAX_STAT_BYTES as u64 {
        return None;
    }
    blob_bytes(repo, oid).ok()
}

fn file_sides(
    repo: &gix::Repository,
    workdir: &Path,
    path: &str,
    head_map: &TreeMap,
) -> (Vec<u8>, Vec<u8>) {
    let old = head_map
        .get(path)
        .and_then(|entry: &TreeEntry| blob_bytes_capped(repo, entry.oid))
        .unwrap_or_default();
    let worktree = workdir.join(path);
    let new = match std::fs::symlink_metadata(&worktree) {
        Ok(metadata) if metadata.file_type().is_symlink() => std::fs::read_link(&worktree)
            .ok()
            .map(|target| target.to_string_lossy().into_owned().into_bytes())
            .unwrap_or_default(),
        Ok(metadata) if metadata.is_file() && metadata.len() <= MAX_STAT_BYTES as u64 => {
            std::fs::read(&worktree).unwrap_or_default()
        }
        _ => Vec::new(),
    };
    (old, new)
}

pub(crate) fn compute_stats(old: &[u8], new: &[u8]) -> (u32, u32, bool) {
    let binary = old.iter().take(8000).any(|b| *b == 0) || new.iter().take(8000).any(|b| *b == 0);
    if binary || old.len() > MAX_STAT_BYTES || new.len() > MAX_STAT_BYTES {
        return (0, 0, binary);
    }
    let old_text = String::from_utf8_lossy(old);
    let new_text = String::from_utf8_lossy(new);
    let diff = lazecode_text::diff::diff_text(&old_text, &new_text);
    (diff.additions, diff.deletions, false)
}

fn ahead_behind(repo: &gix::Repository, head: ObjectId, branch: &str) -> Result<(usize, usize)> {
    let upstream = format!("refs/remotes/origin/{branch}");
    let upstream_id = repo
        .find_reference(upstream.as_str())
        .ok()
        .and_then(|reference| reference.target().try_id().map(|id| id.to_owned()));
    let Some(upstream_id) = upstream_id else {
        return Ok((0, 0));
    };
    let Some(base) = repo.merge_base(head, upstream_id)? else {
        return Ok((0, 0));
    };
    let base = base.detach();
    Ok((
        count_until(repo, head, base).unwrap_or(0),
        count_until(repo, upstream_id, base).unwrap_or(0),
    ))
}

fn count_until(repo: &gix::Repository, from: ObjectId, stop: ObjectId) -> Result<usize> {
    let mut seen = HashSet::new();
    let mut queue = vec![from];
    let mut count = 0usize;
    while let Some(id) = queue.pop() {
        if id == stop || !seen.insert(id) {
            continue;
        }
        count += 1;
        if count > 20_000 {
            break;
        }
        if let Ok(commit) = repo.find_commit(id) {
            for parent in commit.parent_ids() {
                queue.push(parent.detach());
            }
        }
    }
    Ok(count)
}
