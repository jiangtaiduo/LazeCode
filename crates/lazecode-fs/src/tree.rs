use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// A node in the project tree. Folder paths are relative to the project root.
#[derive(Clone, Debug)]
pub struct TreeNode {
    pub name: String,
    pub path: PathBuf,
    pub kind: NodeKind,
    pub children: Vec<TreeNode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    File,
    Folder,
}

/// A fully scanned project tree, respecting `.gitignore` and friends.
#[derive(Clone, Debug, Default)]
pub struct FileTree {
    pub nodes: Vec<TreeNode>,
    pub file_count: usize,
    pub truncated: bool,
}

impl FileTree {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

const DEFAULT_MAX_ENTRIES: usize = 20_000;

/// Scan `root` into a nested tree.
///
/// Hidden files and git-ignored files are skipped. `.git` is always skipped.
/// If the scan exceeds `max_entries` the result is marked `truncated`; the UI
/// surfaces that instead of silently pretending the project is complete.
pub fn load_tree(root: &Path) -> FileTree {
    load_tree_with_limit(root, DEFAULT_MAX_ENTRIES)
}

pub fn load_tree_with_limit(root: &Path, max_entries: usize) -> FileTree {
    use ignore::WalkBuilder;

    let mut tree = FileTree::default();
    if !root.is_dir() {
        return tree;
    }

    // Used both for building and for sorting: folders before files, then
    // case-insensitive by name.
    fn insert(nodes: &mut Vec<TreeNode>, components: &[PathBuf], rel: &Path, is_dir: bool) {
        let Some((first, rest)) = components.split_first() else {
            return;
        };
        let name = first
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let folder = !rest.is_empty() || is_dir;
        let pos = nodes
            .binary_search_by(|node| {
                let node_folder = node.kind == NodeKind::Folder;
                node_folder
                    .cmp(&folder)
                    .reverse()
                    .then_with(|| node.name.to_lowercase().cmp(&name.to_lowercase()))
                    .then_with(|| node.name.cmp(&name))
            })
            .unwrap_or_else(|pos| {
                nodes.insert(
                    pos,
                    TreeNode {
                        name,
                        path: rel.to_path_buf(),
                        kind: if folder {
                            NodeKind::Folder
                        } else {
                            NodeKind::File
                        },
                        children: Vec::new(),
                    },
                );
                pos
            });
        if !rest.is_empty() {
            insert(&mut nodes[pos].children, rest, rel, is_dir);
        }
    }

    let mut count = 0usize;
    let walker = WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .parents(false)
        .follow_links(false)
        .max_depth(Some(64))
        .filter_entry(|entry| !crate::is_default_ignored(entry.file_name()))
        .build();

    for entry in walker {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path == root {
            continue;
        }
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        if rel.components().count() > 64 {
            continue;
        }
        count += 1;
        if count > max_entries {
            tree.truncated = true;
            break;
        }
        let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
        let components: Vec<PathBuf> = rel
            .components()
            .map(|component| PathBuf::from(component.as_os_str()))
            .collect();
        insert(&mut tree.nodes, &components, rel, is_dir);
    }
    tree.file_count = crate::file_count(&tree);
    tree
}

/// Flatten the tree into the rows that are currently visible. Folders only
/// descend when their path is in `expanded`.
pub fn visible_rows(tree: &FileTree, expanded: &HashSet<PathBuf>) -> Vec<VisibleRow> {
    fn walk(
        nodes: &[TreeNode],
        depth: usize,
        expanded: &HashSet<PathBuf>,
        out: &mut Vec<VisibleRow>,
    ) {
        for node in nodes {
            let is_folder = node.kind == NodeKind::Folder;
            let open = is_folder && expanded.contains(&node.path);
            out.push(VisibleRow {
                path: node.path.clone(),
                name: node.name.clone(),
                kind: node.kind,
                depth,
                expanded: open,
            });
            if open {
                walk(&node.children, depth + 1, expanded, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(&tree.nodes, 0, expanded, &mut out);
    out
}

#[derive(Clone, Debug)]
pub struct VisibleRow {
    pub path: PathBuf,
    pub name: String,
    pub kind: NodeKind,
    pub depth: usize,
    pub expanded: bool,
}

/// Build a lookup from relative path to node for O(1) metadata access.
pub fn index_nodes(tree: &FileTree) -> BTreeMap<PathBuf, NodeKind> {
    let mut map = BTreeMap::new();
    fn walk(nodes: &[TreeNode], map: &mut BTreeMap<PathBuf, NodeKind>) {
        for node in nodes {
            map.insert(node.path.clone(), node.kind);
            walk(&node.children, map);
        }
    }
    walk(&tree.nodes, &mut map);
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_git_directory_and_sorts_folders_first() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.txt"), "b").unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".git/config"), "").unwrap();

        let tree = load_tree(dir.path());
        assert_eq!(tree.file_count, 3);
        assert_eq!(tree.nodes[0].name, "src");
        assert_eq!(tree.nodes[0].kind, NodeKind::Folder);
        assert_eq!(tree.nodes[1].name, "a.txt");
        assert!(tree.nodes.iter().all(|n| n.name != ".git"));
    }

    #[test]
    fn visible_rows_respect_expansion() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "").unwrap();
        let tree = load_tree(dir.path());
        let mut expanded = HashSet::new();
        assert_eq!(visible_rows(&tree, &expanded).len(), 1);
        expanded.insert(PathBuf::from("src"));
        let rows = visible_rows(&tree, &expanded);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].depth, 1);
    }
}
