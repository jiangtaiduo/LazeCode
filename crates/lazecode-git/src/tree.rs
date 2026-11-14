use std::collections::BTreeMap;

use gix::hash::ObjectId;

use crate::ops::safe_worktree_join;
use crate::{workdir, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeEntry {
    pub oid: ObjectId,
    /// Git's octal file mode, e.g. `0o100644`.
    pub mode: u32,
}

pub type TreeMap = BTreeMap<String, TreeEntry>;

/// Recursively collect every blob below `tree`.
#[allow(clippy::only_used_in_recursion)]
pub fn collect_tree(
    repo: &gix::Repository,
    tree: gix::Tree<'_>,
    prefix: &str,
    out: &mut TreeMap,
) -> Result<()> {
    for entry in tree.iter() {
        let entry = entry?;
        let name = entry.filename().to_string();
        if name.is_empty() || name == "." || name == ".." || name.contains('\0') {
            anyhow::bail!("refusing unsafe tree entry name {name:?}");
        }
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if entry.mode().is_tree() {
            let subtree = entry.object()?.into_tree();
            collect_tree(repo, subtree, &path, out)?;
        } else if entry.mode().is_blob() || entry.mode().is_link() {
            out.insert(
                path,
                TreeEntry {
                    oid: entry.oid().to_owned(),
                    mode: entry.mode().value() as u32,
                },
            );
        }
        // Submodules (commit entries) are intentionally not materialised.
    }
    Ok(())
}

/// True when the tree contains a submodule gitlink. Submodules are not
/// materialised by this crate, so write paths must refuse rather than silently
/// dropping them.
#[allow(clippy::only_used_in_recursion)]
pub fn tree_has_gitlinks(repo: &gix::Repository, tree: gix::Tree<'_>) -> Result<bool> {
    for entry in tree.iter() {
        let entry = entry?;
        if entry.mode().is_commit() {
            return Ok(true);
        }
        if entry.mode().is_tree() {
            let subtree = entry.object()?.into_tree();
            if tree_has_gitlinks(repo, subtree)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub fn head_tree_map(repo: &gix::Repository) -> Result<TreeMap> {
    let mut map = TreeMap::new();
    if repo.head().map(|head| head.is_unborn()).unwrap_or(true) {
        return Ok(map);
    }
    let tree = repo.head_tree()?;
    collect_tree(repo, tree, "", &mut map)?;
    Ok(map)
}

pub fn tree_map(repo: &gix::Repository, tree: ObjectId) -> Result<TreeMap> {
    let tree = repo.find_tree(tree)?;
    let mut map = TreeMap::new();
    collect_tree(repo, tree, "", &mut map)?;
    Ok(map)
}

pub fn commit_tree_map(repo: &gix::Repository, commit: ObjectId) -> Result<TreeMap> {
    let commit = repo.find_commit(commit)?;
    let tree = commit.tree()?;
    let mut map = TreeMap::new();
    collect_tree(repo, tree, "", &mut map)?;
    Ok(map)
}

pub fn index_map(state: &gix::index::State) -> BTreeMap<String, TreeEntry> {
    let mut map = BTreeMap::new();
    for entry in state.entries() {
        if entry.stage() != gix::index::entry::Stage::Unconflicted {
            continue;
        }
        if entry.mode == gix::index::entry::Mode::COMMIT {
            continue;
        }
        let path = entry.path(state).to_string();
        map.insert(
            path,
            TreeEntry {
                oid: entry.id,
                mode: entry.mode.bits(),
            },
        );
    }
    map
}

/// Write the index contents as a tree hierarchy and return the root tree id.
pub fn write_tree_from_index(
    repo: &gix::Repository,
    state: &gix::index::State,
) -> Result<ObjectId> {
    #[derive(Default)]
    struct Dir {
        dirs: BTreeMap<String, Dir>,
        files: BTreeMap<String, TreeEntry>,
    }

    let mut root = Dir::default();
    for entry in state.entries() {
        if entry.stage() != gix::index::entry::Stage::Unconflicted {
            continue;
        }
        if entry.mode == gix::index::entry::Mode::COMMIT {
            continue;
        }
        let path = entry.path(state).to_string();
        let mut components = path.split('/').peekable();
        let mut dir = &mut root;
        while let Some(component) = components.next() {
            if components.peek().is_some() {
                dir = dir.dirs.entry(component.to_string()).or_default();
            } else {
                dir.files.insert(
                    component.to_string(),
                    TreeEntry {
                        oid: entry.id,
                        mode: entry.mode.bits(),
                    },
                );
            }
        }
    }

    fn write_dir(repo: &gix::Repository, dir: &mut Dir) -> Result<ObjectId> {
        let mut entries = Vec::with_capacity(dir.files.len() + dir.dirs.len());
        for (name, file) in &dir.files {
            let mode = gix::objs::tree::EntryMode::try_from(file.mode)
                .map_err(|mode| anyhow::anyhow!("unsupported tree mode {mode:o} for {name}"))?;
            entries.push(gix::objs::tree::Entry {
                mode,
                filename: name.as_str().into(),
                oid: file.oid,
            });
        }
        for (name, child) in &mut dir.dirs {
            let oid = write_dir(repo, child)?;
            entries.push(gix::objs::tree::Entry {
                mode: gix::objs::tree::EntryMode::from(gix::objs::tree::EntryKind::Tree),
                filename: name.as_str().into(),
                oid,
            });
        }
        entries.sort();
        let tree = gix::objs::Tree { entries };
        Ok(repo.write_object(&tree)?.detach())
    }

    write_dir(repo, &mut root)
}

/// Write an index state matching `map` (used by branch checkout).
pub fn index_state_from_tree(repo: &gix::Repository, map: &TreeMap) -> gix::index::State {
    let mut state = gix::index::State::new(repo.object_hash());
    for (path, entry) in map {
        let mode = gix::index::entry::Mode::from_bits_truncate(entry.mode);
        state.dangerously_push_entry(
            gix::index::entry::Stat::default(),
            entry.oid,
            gix::index::entry::Flags::empty(),
            mode,
            path.as_str().into(),
        );
    }
    state.sort_entries();
    state
}

/// Read a blob's bytes.
pub fn blob_bytes(repo: &gix::Repository, oid: ObjectId) -> Result<Vec<u8>> {
    let mut blob = repo.find_blob(oid)?;
    Ok(blob.take_data())
}

/// Materialise a tree map into the worktree (only files, no submodules).
pub fn materialize_tree(repo: &gix::Repository, map: &TreeMap) -> Result<()> {
    let workdir = workdir(repo)?;
    for (path, entry) in map {
        let target = safe_worktree_join(&workdir, path)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = blob_bytes(repo, entry.oid)?;
        match entry.mode {
            0o120000 => {
                if target.symlink_metadata().is_ok() {
                    let _ = std::fs::remove_file(&target);
                }
                let target_path = String::from_utf8_lossy(&data).to_string();
                std::os::unix::fs::symlink(target_path, &target)?;
            }
            mode => {
                let streamed = crate::filter::smudge_lfs_to_file_in(repo, path, &data, &target)?;
                let data = if streamed {
                    Vec::new()
                } else {
                    crate::filter::smudge_in(repo, path, &entry.oid.to_string(), data)?
                };
                let existing = if streamed {
                    None
                } else {
                    match std::fs::symlink_metadata(&target) {
                        Ok(metadata) if metadata.is_file() => std::fs::read(&target).ok(),
                        _ => None,
                    }
                };
                if !streamed && existing.as_deref() != Some(data.as_slice()) {
                    lazecode_fs::file::write_atomic(&target, &data)?;
                }
                use std::os::unix::fs::PermissionsExt;
                let perms = if mode == 0o100755 { 0o755 } else { 0o644 };
                let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(perms));
            }
        }
    }
    Ok(())
}
