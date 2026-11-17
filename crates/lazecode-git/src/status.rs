use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

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
    let (ignored_files, ignored_dirs) = ignored_entries_cached(&workdir).unwrap_or_default();
    Ok(RepoSnapshot {
        root: workdir,
        branch,
        detached,
        unborn,
        ahead,
        behind,
        changes,
        ignored_files,
        ignored_dirs,
        head: info,
    })
}

#[derive(Clone)]
struct IgnoredCacheEntry {
    at: Instant,
    files: Vec<String>,
    dirs: Vec<String>,
}

static IGNORED_CACHE: OnceLock<parking_lot::Mutex<HashMap<PathBuf, IgnoredCacheEntry>>> =
    OnceLock::new();

fn ignored_cache_key(root: &Path) -> PathBuf {
    std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())
}

/// Drop the ignored-file cache for a worktree. Call this after watcher events.
pub fn invalidate_ignored_cache(root: &Path) {
    if let Some(cache) = IGNORED_CACHE.get() {
        cache.lock().remove(&ignored_cache_key(root));
    }
}

fn ignored_entries_cached(root: &Path) -> Result<(Vec<String>, Vec<String>)> {
    let key = ignored_cache_key(root);
    let cache = IGNORED_CACHE.get_or_init(|| parking_lot::Mutex::new(HashMap::new()));
    if let Some(entry) = cache.lock().get(&key) {
        if entry.at.elapsed() < std::time::Duration::from_secs(2) {
            return Ok((entry.files.clone(), entry.dirs.clone()));
        }
    }
    let (files, dirs) = ignored_entries(root)?;
    let mut cache = cache.lock();
    if cache.len() >= 256 {
        cache.clear();
    }
    cache.insert(
        key,
        IgnoredCacheEntry {
            at: Instant::now(),
            files: files.clone(),
            dirs: dirs.clone(),
        },
    );
    Ok((files, dirs))
}

/// Ignored entries, mirroring `git status --ignored=matching`: ignored
/// directories are reported as `dir/`. Returns `(all_ignored, ignored_dirs)`.
pub fn ignored_entries(root: &Path) -> Result<(Vec<String>, Vec<String>)> {
    let repo = discover(root)?;
    let index = repo.index_or_empty()?;
    let options = repo
        .dirwalk_options()?
        .emit_ignored(Some(gix::dir::walk::EmissionMode::CollapseDirectory))
        .emit_tracked(false)
        .emit_pruned(false)
        .emit_empty_directories(false)
        .emit_collapsed(Some(gix::dir::walk::CollapsedEntriesEmissionMode::All));
    let mut collect = gix::dir::walk::delegate::Collect::default();
    let interrupt = std::sync::atomic::AtomicBool::new(false);
    repo.dirwalk(
        &index,
        std::iter::empty::<gix::bstr::BString>(),
        &interrupt,
        options,
        &mut collect,
    )?;
    let mut ignored_all = Vec::new();
    let mut ignored_dirs = Vec::new();
    for (entry, _) in collect.into_entries_by_path() {
        if !matches!(entry.status, gix::dir::entry::Status::Ignored(_)) {
            continue;
        }
        let path = String::from_utf8_lossy(entry.rela_path.as_ref())
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_string();
        if path.is_empty() {
            continue;
        }
        if entry.disk_kind == Some(gix::dir::entry::Kind::Directory) {
            ignored_dirs.push(path.clone());
        }
        ignored_all.push(path);
    }
    ignored_all.sort();
    ignored_dirs.sort();
    Ok((ignored_all, ignored_dirs))
}

/// Query files ignored by `.gitignore` rules.
pub fn ignored_files(root: &Path) -> Result<Vec<String>> {
    let (all, _) = ignored_entries(root)?;
    Ok(all)
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

pub(crate) fn collect_changes(
    repo: &gix::Repository,
    workdir: &Path,
    head_map: &TreeMap,
) -> Result<Vec<Change>> {
    let index = repo.index_or_empty()?;
    let index_entries = index.entries();
    let mut index_map: BTreeMap<String, (ObjectId, u32, Stat)> = BTreeMap::new();
    let mut conflicted: HashSet<String> = HashSet::new();
    for entry in index_entries {
        if entry.stage() != gix::index::entry::Stage::Unconflicted {
            // Unmerged stages are conflicts, not untracked files; keep them out
            // of the unstaged walks so the UI can show Resolve.
            conflicted.insert(entry.path(&index).to_string());
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
            conflicted: false,
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
        if staged.is_none() && unstaged.is_none() {
            continue;
        }
        // JetBrains parity: the panel lists the **net** HEAD ↔ worktree
        // difference, not raw `git status`. When the index diverges from HEAD
        // while the worktree still matches HEAD (e.g. stale content was staged
        // and the disk file was later restored), the staged and unstaged
        // halves cancel out; listing the path would open a diff that reports
        // "no uncommitted changes".
        let net_dirty = match (head_entry, unstaged) {
            // Disk stat-matches the index, so disk content equals the index
            // blob; the index differs from HEAD ⇒ net change.
            (Some(_), None) => true,
            // Added in the index only: nets to a change while it exists on
            // disk, nothing once the disk file is gone too.
            (None, _) => workdir.join(path).symlink_metadata().is_ok(),
            // Diverges on both sides: compare the disk file against HEAD.
            (Some(head), Some(_)) => {
                net_differs_from_head(repo, workdir, path, head.oid, head.mode)?
            }
        };
        if net_dirty {
            add(path.clone(), staged, unstaged, false);
        }
    }

    for (path, head) in head_map.iter() {
        if conflicted.contains(path) {
            continue;
        }
        // Deleted in the index only: hidden while the disk file still matches
        // HEAD (the deletion never reached the worktree).
        if !index_map.contains_key(path)
            && net_differs_from_head(repo, workdir, path, head.oid, head.mode)?
        {
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
        // Paths present in HEAD are owned by the staged-deletion scan above
        // (with its net HEAD ↔ worktree check); walking them here too would
        // resurrect index-only phantoms as untracked entries.
        if index_map.contains_key(&path)
            || conflicted.contains(&path)
            || head_map.contains_key(&path)
        {
            continue;
        }
        add(path, None, None, true);
    }

    // Conflicted index entries are shown once, under Merge Conflicts.
    for path in &conflicted {
        add(path.clone(), None, None, false);
    }
    for path in &conflicted {
        if let Some(change) = acc.get_mut(path) {
            change.conflicted = true;
        }
    }

    let mut changes: Vec<Change> = acc.into_values().collect();
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(changes)
}

/// Lightweight change scan for stash/shelf: paths and staged/unstaged state
/// only. Skips per-file diff stats, ignored entries and ahead/behind walking.
pub fn changed_paths(root: &Path) -> Result<Vec<Change>> {
    let repo = discover(root)?;
    let workdir = workdir(&repo)?;
    let head_map = head_tree_map(&repo)?;
    collect_changes(&repo, &workdir, &head_map)
}

pub(crate) fn changes(repo: &gix::Repository, workdir: &Path) -> Result<Vec<Change>> {
    let head_map = head_tree_map(repo)?;
    let mut changes = collect_changes(repo, workdir, &head_map)?;
    let mut filters = crate::filter::FilterContext::new(repo)?;
    for change in &mut changes {
        let (old, new) = file_sides(repo, workdir, &change.path, &head_map, &mut filters)?;
        let (additions, deletions, binary) = compute_stats(&old, &new);
        change.additions = additions;
        change.deletions = deletions;
        change.binary = binary;
    }
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
    let stat_matches = gix::index::fs::Metadata::from_path_no_follow(&abs)
        .ok()
        .and_then(|meta| Stat::from_fs(&meta).ok())
        .is_some_and(|stat| stats_equal(&stat, index_stat));
    if stat_matches {
        return Ok(WorktreeStatus::Clean);
    }
    if index_stat.size != 0 && meta.len() != index_stat.size as u64 {
        return Ok(WorktreeStatus::Modified);
    }
    let bytes = match std::fs::read(&abs) {
        Ok(b) => b,
        Err(_) => return Ok(WorktreeStatus::Modified),
    };
    let recorded = match blob_bytes(repo, index_oid) {
        Ok(b) => b,
        Err(_) => return Ok(WorktreeStatus::Modified),
    };
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

/// Net HEAD ↔ worktree check for paths the index-based scan already flagged:
/// does the disk file differ from the HEAD blob? Only the dirty set reaches
/// this, so the extra blob reads stay bounded.
fn net_differs_from_head(
    repo: &gix::Repository,
    workdir: &Path,
    path: &str,
    head_oid: ObjectId,
    head_mode: u32,
) -> Result<bool> {
    let abs = workdir.join(path);
    let Ok(meta) = std::fs::symlink_metadata(&abs) else {
        // Present in HEAD, gone from the worktree.
        return Ok(true);
    };
    if meta.is_dir() {
        // A tracked file replaced by a directory is a type change.
        return Ok(true);
    }
    if !meta.is_file() && !meta.file_type().is_symlink() {
        // FIFOs and device nodes can block forever in std::fs::read.
        return Ok(true);
    }
    let is_symlink = meta.file_type().is_symlink();
    let head_is_symlink = head_mode & 0o170000 == 0o120000;
    if is_symlink != head_is_symlink {
        return Ok(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let worktree_executable = meta.permissions().mode() & 0o111 != 0;
        let head_executable = head_mode & 0o111 != 0;
        if worktree_executable != head_executable {
            return Ok(true);
        }
    }
    if is_symlink {
        let current = std::fs::read_link(&abs)?.to_string_lossy().into_owned();
        let recorded = String::from_utf8_lossy(&blob_bytes(repo, head_oid)?).into_owned();
        return Ok(current != recorded);
    }
    let bytes = std::fs::read(&abs)?;
    Ok(bytes != blob_bytes(repo, head_oid)?)
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
    filters: &mut crate::filter::FilterContext<'_>,
) -> Result<(Vec<u8>, Vec<u8>)> {
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
            let data = std::fs::read(&worktree).unwrap_or_default();
            if data == old {
                data
            } else {
                filters.clean(path, data)?
            }
        }
        Ok(metadata)
            if metadata.is_file() && filters.filter_driver(path)?.as_deref() == Some("lfs") =>
        {
            filters.clean_file(path, &worktree)?
        }
        _ => Vec::new(),
    };
    Ok((old, new))
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

type CountKey = (PathBuf, ObjectId, ObjectId);

static COUNT_CACHE: OnceLock<parking_lot::Mutex<HashMap<CountKey, (usize, usize)>>> =
    OnceLock::new();

fn cached_counts(key: &CountKey) -> Option<(usize, usize)> {
    COUNT_CACHE
        .get_or_init(|| parking_lot::Mutex::new(HashMap::new()))
        .lock()
        .get(key)
        .copied()
}

fn store_counts(key: CountKey, counts: (usize, usize)) {
    let cache = COUNT_CACHE.get_or_init(|| parking_lot::Mutex::new(HashMap::new()));
    let mut cache = cache.lock();
    if cache.len() >= 512 {
        cache.clear();
    }
    cache.insert(key, counts);
}

fn counts_until(
    repo: &gix::Repository,
    workdir: &Path,
    head: ObjectId,
    upstream: ObjectId,
    base: ObjectId,
) -> Result<(usize, usize)> {
    let key = (workdir.to_path_buf(), head, upstream);
    if let Some(cached) = cached_counts(&key) {
        return Ok(cached);
    }
    let counts = (
        count_until(repo, head, base)?,
        count_until(repo, upstream, base)?,
    );
    store_counts(key, counts);
    Ok(counts)
}

fn upstream_ref(repo: &gix::Repository, branch: &str) -> Option<String> {
    let config = repo.config_snapshot();
    let remote = config
        .string(&format!("branch.{branch}.remote"))?
        .to_string();
    let merge = config
        .string(&format!("branch.{branch}.merge"))?
        .to_string();
    let leaf = merge.strip_prefix("refs/heads/").unwrap_or(&merge);
    if remote == "." {
        Some(format!("refs/heads/{leaf}"))
    } else {
        Some(format!("refs/remotes/{remote}/{leaf}"))
    }
}

fn ahead_behind(repo: &gix::Repository, head: ObjectId, branch: &str) -> Result<(usize, usize)> {
    let mut candidates = Vec::new();
    if let Some(upstream) = upstream_ref(repo, branch) {
        candidates.push(upstream);
    }
    // Stable fallback: `origin/<branch>`. A configured upstream (upstream/*,
    // non-origin remotes) is resolved above.
    let origin = format!("refs/remotes/origin/{branch}");
    if !candidates.contains(&origin) {
        candidates.push(origin);
    }
    for upstream in candidates {
        let upstream_id = repo
            .find_reference(upstream.as_str())
            .ok()
            .and_then(|reference| reference.target().try_id().map(|id| id.to_owned()));
        let Some(upstream_id) = upstream_id else {
            continue;
        };
        let Some(base) = repo.merge_base(head, upstream_id)? else {
            continue;
        };
        let workdir = workdir(repo)?;
        return counts_until(repo, &workdir, head, upstream_id, base.detach());
    }
    Ok((0, 0))
}

/// Count commits on `branch` that are not yet in its configured upstream
/// (falling back to `origin/<branch>`), without invoking `git rev-list`.
pub fn pending_count(root: &Path, branch: &str) -> Result<usize> {
    let repo = discover(root)?;
    let branch_id = repo
        .rev_parse_single(gix::bstr::BStr::new(&format!("refs/heads/{branch}")))
        .ok()
        .map(|id| id.detach());
    let Some(branch_id) = branch_id else {
        return Ok(0);
    };
    let candidates = upstream_ref(&repo, branch)
        .into_iter()
        .chain(std::iter::once(format!("refs/remotes/origin/{branch}")));
    for upstream in candidates {
        let Some(upstream_id) = repo
            .find_reference(upstream.as_str())
            .ok()
            .and_then(|reference| reference.target().try_id().map(|id| id.to_owned()))
        else {
            continue;
        };
        let Some(base) = repo.merge_base(branch_id, upstream_id)? else {
            continue;
        };
        let workdir = workdir(&repo)?;
        let counts = counts_until(&repo, &workdir, branch_id, upstream_id, base.detach())?;
        return Ok(counts.0);
    }
    Ok(0)
}

fn count_until(repo: &gix::Repository, from: ObjectId, stop: ObjectId) -> Result<usize> {
    let mut count = 0usize;
    let walk = repo.rev_walk([from]).with_hidden([stop]).all()?;
    for commit in walk {
        let _ = commit?;
        count += 1;
        if count >= 20_000 {
            break;
        }
    }
    Ok(count)
}
