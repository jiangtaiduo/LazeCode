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

/// Fold every `/` segment into a directory node, matching the file tree. A
/// branch `origin/dependabot/cargo/x` becomes three collapsible groups with
/// `x` as the leaf; the full ref name stays available on the row's tooltip.
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
    sort_entries(&mut root);
    root
}

fn sort_entries(node: &mut RefTreeNode) {
    node.entries.sort_by(|a, b| {
        let key = |entry: &RefTreeEntry| match entry {
            RefTreeEntry::Dir(dir) => (0u8, dir.name.clone()),
            RefTreeEntry::Ref(branch) => (1, branch.name.clone()),
            RefTreeEntry::Tag { name, .. } => (2, name.clone()),
        };
        key(a).cmp(&key(b))
    });
    for entry in &mut node.entries {
        if let RefTreeEntry::Dir(child) = entry {
            sort_entries(child);
        }
    }
}

fn ref_label(branch: &BranchInfo, depth: usize) -> String {
    if depth > 0 {
        branch
            .name
            .rsplit('/')
            .next()
            .unwrap_or(&branch.name)
            .to_string()
    } else {
        branch.name.clone()
    }
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
/// paths, matching the UI's set). Entries are sorted by the tree builder:
/// groups first, then refs and tags, each alphabetically.
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
                label: ref_label(branch, depth),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn branch(name: &str) -> BranchInfo {
        BranchInfo {
            name: name.into(),
            full_name: format!("refs/remotes/{name}"),
            remote: true,
            current: false,
            target: String::new(),
            tracking: None,
            ahead: 0,
            behind: 0,
        }
    }

    #[test]
    fn deep_remote_paths_fold_into_collapsible_dirs() {
        let refs = [
            branch("origin/dependabot/cargo/notify-8.2.2"),
            branch("origin/dependabot/cargo/png-0.18.1"),
            branch("origin/feat-ui-redesign"),
            branch("upstream/actions/checkout-7"),
        ];
        let tree = build_ref_tree(&refs);
        assert_eq!(tree.entries.len(), 2, "one group per remote");
        assert!(matches!(&tree.entries[0], RefTreeEntry::Dir(dir) if dir.name == "origin"));

        let expanded = [
            "origin",
            "origin/dependabot",
            "origin/dependabot/cargo",
            "upstream",
            "upstream/actions",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        let rows = flatten_ref_tree(&tree, &expanded);
        let labels: Vec<_> = rows.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "origin",
                "dependabot",
                "cargo",
                "notify-8.2.2",
                "png-0.18.1",
                "feat-ui-redesign",
                "upstream",
                "actions",
                "checkout-7",
            ]
        );
        let depths: Vec<_> = rows.iter().map(|row| row.depth).collect();
        assert_eq!(depths, [0, 1, 2, 3, 3, 1, 0, 1, 2]);

        // A collapsed directory still shows as a row but hides its children.
        let collapsed = ["origin".to_string()].into_iter().collect();
        let rows = flatten_ref_tree(&tree, &collapsed);
        let labels: Vec<_> = rows.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(
            labels,
            ["origin", "dependabot", "feat-ui-redesign", "upstream"]
        );

        let fully_collapsed = std::collections::HashSet::new();
        let rows = flatten_ref_tree(&tree, &fully_collapsed);
        let labels: Vec<_> = rows.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(labels, ["origin", "upstream"]);
    }

    #[test]
    fn groups_and_leaves_are_sorted() {
        let refs = [
            branch("origin/zebra"),
            branch("origin/alpha"),
            branch("upstream/beta"),
        ];
        let tree = build_ref_tree(&refs);
        let expanded = ["origin".into(), "upstream".into()].into_iter().collect();
        let rows = flatten_ref_tree(&tree, &expanded);
        let paths: Vec<_> = rows.iter().map(|row| row.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "origin",
                "origin/alpha",
                "origin/zebra",
                "upstream",
                "upstream/beta",
            ]
        );
    }
}
