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

/// A project tree scanned under the application's configured ignore patterns.
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

/// Scan `root` into a nested tree (ignored entries skipped). Siblings are read
/// before any subtree, so a large directory cannot hide later entries.
/// Exceeding `max_entries` marks the result `truncated`.
pub fn load_tree(root: &Path) -> FileTree {
    load_tree_filtered(root, DEFAULT_MAX_ENTRIES, None)
}

pub fn load_tree_with_limit(root: &Path, max_entries: usize) -> FileTree {
    load_tree_filtered(root, max_entries, None)
}

pub fn load_tree_filtered(
    root: &Path,
    max_entries: usize,
    custom_ignored: Option<&[String]>,
) -> FileTree {
    load_tree_with_expanded(root, None, max_entries, custom_ignored)
}

pub fn load_tree_with_expanded(
    root: &Path,
    expanded: Option<&HashSet<PathBuf>>,
    max_entries: usize,
    custom_ignored: Option<&[String]>,
) -> FileTree {
    let mut tree = FileTree::default();
    if !root.is_dir() {
        return tree;
    }

    fn scan_dir_recursive(
        root: &Path,
        rel: &Path,
        expanded: Option<&HashSet<PathBuf>>,
        custom_ignored: Option<&[String]>,
        budget: &mut usize,
        max_entries: usize,
    ) -> Vec<TreeNode> {
        let full_dir = if rel.as_os_str().is_empty() {
            root.to_path_buf()
        } else {
            root.join(rel)
        };
        let Ok(read_dir) = std::fs::read_dir(&full_dir) else {
            return Vec::new();
        };

        let mut children = Vec::new();
        let mut descend: Vec<(usize, PathBuf)> = Vec::new();
        for entry in read_dir.flatten() {
            if *budget >= max_entries {
                break;
            }
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if crate::is_ignored_pattern(&name, is_dir, custom_ignored) {
                continue;
            }
            *budget += 1;
            let entry_rel = if rel.as_os_str().is_empty() {
                PathBuf::from(&*name)
            } else {
                rel.join(&*name)
            };
            if is_dir {
                let should_descend = match expanded {
                    Some(exp) => exp.contains(&entry_rel),
                    None => name != "target" && name != "dist" && rel.components().count() < 16,
                };
                children.push(TreeNode {
                    name: name.into_owned(),
                    path: entry_rel.clone(),
                    kind: NodeKind::Folder,
                    children: Vec::new(),
                });
                if should_descend {
                    descend.push((children.len() - 1, entry_rel));
                }
            } else {
                children.push(TreeNode {
                    name: name.into_owned(),
                    path: entry_rel,
                    kind: NodeKind::File,
                    children: Vec::new(),
                });
            }
        }
        // Descend only after every sibling is in `children`: a deep subtree
        // must not consume the shared budget before the next entry is read.
        for (index, entry_rel) in descend {
            children[index].children = scan_dir_recursive(
                root,
                &entry_rel,
                expanded,
                custom_ignored,
                budget,
                max_entries,
            );
        }

        children.sort_by(|a, b| {
            let a_folder = a.kind == NodeKind::Folder;
            let b_folder = b.kind == NodeKind::Folder;
            b_folder
                .cmp(&a_folder)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.name.cmp(&b.name))
        });

        children
    }

    let mut budget = 0usize;
    tree.nodes = scan_dir_recursive(
        root,
        Path::new(""),
        expanded,
        custom_ignored,
        &mut budget,
        max_entries,
    );
    if budget >= max_entries {
        tree.truncated = true;
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
    #[test]
    fn loads_all_root_entries_without_target_truncation() {
        // Build a synthetic repository: the real one lives at an arbitrary
        // path (and a CI checkout has no `target/` before the build), so the
        // test must not depend on the developer's working copy layout.
        let repo = tempfile::tempdir().unwrap();
        let root = repo.path();
        for name in ["Cargo.lock", "README.md", ".gitignore"] {
            std::fs::write(root.join(name), "").unwrap();
        }
        std::fs::create_dir(root.join(".github")).unwrap();
        // A `target/` directory is what used to truncate the scan on big
        // real trees; it must be listed like any other folder.
        std::fs::create_dir(root.join("target")).unwrap();
        std::fs::write(root.join("target/artifact.bin"), "").unwrap();

        let tree = load_tree_with_expanded(root, Some(&HashSet::new()), 20_000, None);
        assert!(!tree.truncated);
        let names: Vec<&str> = tree.nodes.iter().map(|n| n.name.as_str()).collect();
        assert!(names.contains(&".gitignore"));
        assert!(names.contains(&"Cargo.lock"));
        assert!(names.contains(&"README.md"));
        assert!(names.contains(&"target"));
        assert!(names.contains(&".github"));
    }

    #[test]
    fn budget_exhaustion_keeps_every_top_level_entry() {
        // A directory scanned early must not eat the budget before the
        // remaining root entries are even read (readdir order is arbitrary).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for name in ["first.txt", "sentinel.txt", "last.txt"] {
            std::fs::write(root.join(name), "").unwrap();
        }
        for folder in ["a_big", "b_big"] {
            std::fs::create_dir(root.join(folder)).unwrap();
            for index in 0..100 {
                std::fs::write(root.join(folder).join(format!("{index}.bin")), "").unwrap();
            }
        }

        let tree = load_tree_filtered(root, 64, None);
        assert!(tree.truncated, "fixture should exhaust the scan budget");
        let names: Vec<&str> = tree.nodes.iter().map(|node| node.name.as_str()).collect();
        for expected in ["first.txt", "sentinel.txt", "last.txt", "a_big", "b_big"] {
            assert!(
                names.contains(&expected),
                "missing top-level entry {expected}: {names:?}"
            );
        }
    }
}
