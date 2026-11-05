//! Git ref tree: fold branch/tag names by `/` into a directory tree, PyCharm
//! style. Ported from main's `src/services/refTree.ts`.

use lazecode_git::BranchInfo;

/// Folded node of the ref tree; `path` is the full prefix for keys.
#[derive(Clone, Debug, Default)]
pub struct RefTreeNode {
    pub name: String,
    pub path: String,
    pub entries: Vec<RefTreeEntry>,
}

#[derive(Clone, Debug)]
pub enum RefTreeEntry {
    Dir(RefTreeNode),
    Ref(BranchInfo),
    /// Tags are BranchInfo-shaped rows with `remote` unused.
    Tag {
        name: String,
        target: String,
    },
}

#[derive(Clone, Debug)]
pub struct FlatRefRow {
    /// 0 = root level, 1 = one directory deep...
    pub depth: usize,
    pub dir: bool,
    pub path: String,
    pub label: String,
    pub branch: Option<BranchInfo>,
    pub tag: Option<String>,
    /// Whether the row is a tag row (styles its chip amber).
    pub is_tag: bool,
}

/// Build the tree from branch/tag rows. Order of input is preserved within
/// each directory level; directories appear where their first member sits.
pub fn build_ref_tree(refs: &[BranchInfo]) -> RefTreeNode {
    let mut root = RefTreeNode::default();
    for branch in refs {
        let parts: Vec<&str> = branch
            .name
            .split('/')
            .filter(|part| !part.is_empty())
            .collect();
        if parts.is_empty() {
            continue;
        }
        let mut current = &mut root;
        let mut path = String::new();
        for part in &parts[..parts.len() - 1] {
            path = if path.is_empty() {
                part.to_string()
            } else {
                format!("{path}/{part}")
            };
            let position = current
                .entries
                .iter()
                .position(|entry| matches!(entry, RefTreeEntry::Dir(node) if node.name == *part));
            let index = match position {
                Some(index) => index,
                None => {
                    current.entries.push(RefTreeEntry::Dir(RefTreeNode {
                        name: part.to_string(),
                        path: path.clone(),
                        entries: Vec::new(),
                    }));
                    current.entries.len() - 1
                }
            };
            match &mut current.entries[index] {
                RefTreeEntry::Dir(node) => current = node,
                _ => unreachable!("just matched a dir"),
            }
        }
        current.entries.push(RefTreeEntry::Ref(branch.clone()));
    }
    root
}

/// Push tag rows into the tree root as individual entries (PyCharm shows tags
/// in their own section, not folded by directory).
pub fn push_tags(root: &mut RefTreeNode, tags: &[String], targets: &[(String, String)]) {
    for tag in tags {
        let target = targets
            .iter()
            .find(|(name, _)| name == tag)
            .map(|(_, target)| target.clone())
            .unwrap_or_default();
        root.entries.push(RefTreeEntry::Tag {
            name: tag.clone(),
            target,
        });
    }
}

/// Flatten for rendering, expanding only directories in `expanded` (full
/// paths, matching the UI's set). Favourite branches always surface first at
/// the root, then current, then alphabetically (PyCharm's local ordering).
pub fn flatten_ref_tree(
    root: &RefTreeNode,
    expanded: &std::collections::HashSet<String>,
) -> Vec<FlatRefRow> {
    let mut out = Vec::new();
    walk(root, 0, expanded, &mut out);
    out
}

fn walk(
    node: &RefTreeNode,
    depth: usize,
    expanded: &std::collections::HashSet<String>,
    out: &mut Vec<FlatRefRow>,
) {
    for entry in &node.entries {
        match entry {
            RefTreeEntry::Dir(child) => {
                out.push(FlatRefRow {
                    depth,
                    dir: true,
                    path: child.path.clone(),
                    label: child.name.clone(),
                    branch: None,
                    tag: None,
                    is_tag: false,
                });
                if expanded.contains(&child.path) {
                    walk(child, depth + 1, expanded, out);
                }
            }
            RefTreeEntry::Ref(branch) => out.push(FlatRefRow {
                depth,
                dir: false,
                path: branch.name.clone(),
                label: branch
                    .name
                    .rsplit('/')
                    .next()
                    .unwrap_or(&branch.name)
                    .to_string(),
                branch: Some(branch.clone()),
                tag: None,
                is_tag: false,
            }),
            RefTreeEntry::Tag { name, target } => out.push(FlatRefRow {
                depth,
                dir: false,
                path: name.clone(),
                label: name.clone(),
                branch: None,
                tag: Some(target.clone()),
                is_tag: true,
            }),
        }
    }
}

/// Local branch sort: current first, then favourite, then name.
pub fn sort_local_branches(
    branches: &mut [BranchInfo],
    current_branch: &str,
    favorites: &std::collections::HashSet<String>,
) {
    let is_current = |branch: &BranchInfo| branch.current || branch.name == current_branch;
    branches.sort_by(|a, b| {
        let (cur_a, cur_b) = (is_current(a), is_current(b));
        cur_b
            .cmp(&cur_a)
            .then_with(|| {
                favorites
                    .contains(&b.name)
                    .cmp(&favorites.contains(&a.name))
            })
            .then_with(|| a.name.cmp(&b.name))
    });
}
