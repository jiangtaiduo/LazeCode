//! Git DAG lane layout and long-span folding: deterministic prefix allocation,
//! merge curves, jump arrows.

use std::collections::{HashMap, HashSet};

use crate::model::CommitInfo;

pub const GRAPH_BREAK_MIN_SPAN: usize = 25;
pub const GRAPH_BREAK_GAP: f32 = 7.0;
pub const LANE_W: f32 = 16.0;
pub const GRAPH_H: f32 = 26.0;
pub const NODE_R: f32 = 4.0;

/// PyCharm-style 8-color palette for branch lanes.
pub const GRAPH_PALETTE: [(u8, u8, u8); 8] = [
    (0x3f, 0xb9, 0x50), // #3fb950 - Green (main)
    (0xa3, 0x71, 0xf7), // #a371f7 - Purple
    (0xf0, 0x88, 0x3e), // #f0883e - Orange
    (0x39, 0xc5, 0xcf), // #39c5cf - Cyan
    (0xe3, 0xb3, 0x41), // #e3b341 - Yellow
    (0x58, 0xa6, 0xff), // #58a6ff - Blue
    (0xf8, 0x51, 0x49), // #f85149 - Red
    (0xd2, 0xa8, 0xff), // #d2a8ff - Light Purple
];

#[inline]
pub fn lane_color(lane: usize) -> (u8, u8, u8) {
    GRAPH_PALETTE[lane % GRAPH_PALETTE.len()]
}

#[inline]
pub fn lane_x(lane: usize) -> f32 {
    10.0 + lane as f32 * LANE_W
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrowDir {
    Up,
    Down,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldArrowMark {
    pub lane: usize,
    pub dir: ArrowDir,
    pub peer_id: String,
    pub peer_short: String,
    pub peer_msg: String,
    pub span: usize,
    pub tooltip: String,
    pub pending: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphBreak {
    pub above: Vec<FoldArrowMark>,
    pub below: Vec<FoldArrowMark>,
    pub omitted: HashSet<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphRowLayout {
    pub node: usize,
    pub has_in_node: bool,
    pub ins: Vec<usize>,
    pub straight: Vec<usize>,
    pub outs: Vec<(usize, usize)>, // (from_lane, to_lane)
}

#[derive(Clone, Debug, Default)]
pub struct DagLayout {
    pub rows: HashMap<String, GraphRowLayout>,
    pub breaks: HashMap<String, GraphBreak>,
    pub max_lane: usize,
}

/// Compute prefix-deterministic DAG layout for all loaded commits.
pub fn compute_graph_layout(commits: &[CommitInfo]) -> (HashMap<String, GraphRowLayout>, usize) {
    let mut lanes: Vec<Option<String>> = Vec::new();
    let mut map = HashMap::with_capacity(commits.len());
    let mut max_lane = 0;

    for commit in commits {
        let h = &commit.id;
        let mut has_in_node = false;
        let node = if let Some(idx) = lanes.iter().position(|l| l.as_deref() == Some(h.as_str())) {
            has_in_node = true;
            idx
        } else if let Some(empty_idx) = lanes.iter().position(|l| l.is_none()) {
            empty_idx
        } else {
            lanes.push(None);
            lanes.len() - 1
        };

        if node > max_lane {
            max_lane = node;
        }

        let mut ins = Vec::new();
        let mut straight = Vec::new();
        for (idx, lane) in lanes.iter().enumerate() {
            if idx == node {
                continue;
            }
            match lane.as_deref() {
                Some(existing) if existing == h.as_str() => {
                    ins.push(idx);
                    if idx > max_lane {
                        max_lane = idx;
                    }
                }
                Some(_) => {
                    straight.push(idx);
                    if idx > max_lane {
                        max_lane = idx;
                    }
                }
                None => {}
            }
        }

        let mut next_lanes = lanes.clone();
        if node < next_lanes.len() {
            next_lanes[node] = None;
        }
        for &src in &ins {
            if src < next_lanes.len() {
                next_lanes[src] = None;
            }
        }

        let mut outs = Vec::new();
        let parents = &commit.parents;
        for (p_idx, parent) in parents.iter().enumerate() {
            if p_idx == 0 {
                if node >= next_lanes.len() {
                    next_lanes.resize(node + 1, None);
                }
                next_lanes[node] = Some(parent.clone());
                outs.push((node, node));
            } else if let Some(to_idx) = next_lanes
                .iter()
                .position(|l| l.as_deref() == Some(parent.as_str()))
            {
                outs.push((node, to_idx));
                if to_idx > max_lane {
                    max_lane = to_idx;
                }
            } else {
                let target_lane =
                    if let Some(empty_idx) = next_lanes.iter().position(|l| l.is_none()) {
                        empty_idx
                    } else {
                        next_lanes.push(None);
                        next_lanes.len() - 1
                    };
                next_lanes[target_lane] = Some(parent.clone());
                outs.push((node, target_lane));
                if target_lane > max_lane {
                    max_lane = target_lane;
                }
            }
        }

        lanes = next_lanes;
        map.insert(
            h.clone(),
            GraphRowLayout {
                node,
                has_in_node,
                ins,
                straight,
                outs,
            },
        );
    }

    (map, max_lane)
}

fn side_interaction(lay: Option<&GraphRowLayout>, lane: usize) -> bool {
    let Some(lay) = lay else { return false };
    if lay.ins.contains(&lane) {
        return true;
    }
    lay.outs
        .iter()
        .any(|&(from, to)| to == lane && from != lane)
}

/// Compute long-span folding break arrows and omitted straight line spans.
pub fn compute_graph_breaks(
    commits: &[CommitInfo],
    layout: &HashMap<String, GraphRowLayout>,
    min_span: usize,
) -> HashMap<String, GraphBreak> {
    let mut out: HashMap<String, GraphBreak> = HashMap::new();
    if commits.len() < min_span {
        return out;
    }

    let straight_of = |idx: usize, lane: usize| -> bool {
        layout
            .get(&commits[idx].id)
            .map(|lay| lay.straight.contains(&lane))
            .unwrap_or(false)
    };

    let interact_at =
        |idx: usize, lane: usize| -> bool { side_interaction(layout.get(&commits[idx].id), lane) };

    let mut max_lane = 0;
    for c in commits {
        if let Some(lay) = layout.get(&c.id) {
            max_lane = max_lane.max(lay.node);
            for &s in &lay.straight {
                max_lane = max_lane.max(s);
            }
        }
    }

    for lane in 0..=max_lane {
        let mut i = 0;
        while i < commits.len() {
            if !straight_of(i, lane) || interact_at(i, lane) {
                i += 1;
                continue;
            }
            let mut j = i;
            let mut boundary: Option<usize> = None;
            while j + 1 < commits.len() && straight_of(j + 1, lane) && !interact_at(j + 1, lane) {
                j += 1;
            }
            if j + 1 < commits.len() && straight_of(j + 1, lane) && interact_at(j + 1, lane) {
                boundary = Some(j + 1);
            }
            let len = j - i + 1;

            if len >= min_span {
                let mut cr = i as isize - 1;
                while cr >= 0 && straight_of(cr as usize, lane) {
                    cr -= 1;
                }
                let child = &commits[cr.max(0) as usize];

                let mut p = j + 1;
                while p < commits.len() && straight_of(p, lane) {
                    p += 1;
                }
                let parent = commits.get(p);
                let closed_right = boundary.is_some() || parent.is_some();

                let down_tooltip = if let Some(parent) = parent {
                    let msg_preview: String = parent.message.chars().take(30).collect();
                    format!(
                        "↓ 跳转到父提交: {} \"{}\" (自本行起 {} 行前)",
                        parent.short,
                        msg_preview,
                        p - i
                    )
                } else {
                    "↓ 该分支延伸至更早提交，父提交尚未加载".to_string()
                };

                let target = parent.unwrap_or(child);
                out.entry(commits[i].id.clone())
                    .or_default()
                    .below
                    .push(FoldArrowMark {
                        lane,
                        dir: ArrowDir::Down,
                        peer_id: target.id.clone(),
                        peer_short: target.short.clone(),
                        peer_msg: target.message.clone(),
                        span: len,
                        tooltip: down_tooltip,
                        pending: parent.is_none(),
                    });

                let cut = if closed_right { j.saturating_sub(1) } else { j };
                for commit in commits.iter().take(cut + 1).skip(i + 1) {
                    out.entry(commit.id.clone())
                        .or_default()
                        .omitted
                        .insert(lane);
                }

                if closed_right {
                    let msg_preview: String = child.message.chars().take(30).collect();
                    let up_tooltip = format!(
                        "↑ 跳转到子提交: {} \"{}\" (自本行起 {} 行后)",
                        child.short, msg_preview, len
                    );
                    out.entry(commits[j].id.clone())
                        .or_default()
                        .above
                        .push(FoldArrowMark {
                            lane,
                            dir: ArrowDir::Up,
                            peer_id: child.id.clone(),
                            peer_short: child.short.clone(),
                            peer_msg: child.message.clone(),
                            span: len,
                            tooltip: up_tooltip,
                            pending: false,
                        });
                }
            }

            i = boundary.unwrap_or(j) + 1;
        }
    }

    out
}

/// Merge previous page graph breaks into fresh recalculation for prefix stability.
pub fn merge_graph_breaks(
    prev_commits: &[CommitInfo],
    prev_breaks: &HashMap<String, GraphBreak>,
    next_commits: &[CommitInfo],
    mut fresh: HashMap<String, GraphBreak>,
) -> HashMap<String, GraphBreak> {
    if prev_commits.is_empty() || prev_breaks.is_empty() {
        return fresh;
    }
    if prev_commits.len() > next_commits.len() {
        return fresh;
    }
    for (k, prev) in prev_commits.iter().enumerate() {
        if next_commits[k].id != prev.id {
            return fresh;
        }
    }

    for (hash, old) in prev_breaks {
        let Some(fe) = fresh.get_mut(hash) else {
            fresh.insert(hash.clone(), old.clone());
            continue;
        };

        let mut fresh_arrow_lanes = HashSet::new();
        for m in &fe.above {
            fresh_arrow_lanes.insert(m.lane);
        }
        for m in &fe.below {
            fresh_arrow_lanes.insert(m.lane);
        }

        for &lane in &old.omitted {
            if !fresh_arrow_lanes.contains(&lane) {
                fe.omitted.insert(lane);
            }
        }

        for old_arrow in &old.above {
            if !fresh_arrow_lanes.contains(&old_arrow.lane) {
                fe.above.push(old_arrow.clone());
            }
        }
        for old_arrow in &old.below {
            if !fresh_arrow_lanes.contains(&old_arrow.lane) {
                fe.below.push(old_arrow.clone());
            }
        }
    }

    fresh
}

/// Compute complete DAG layout with folding.
pub fn compute_dag(commits: &[CommitInfo]) -> DagLayout {
    let (rows, max_lane) = compute_graph_layout(commits);
    let breaks = compute_graph_breaks(commits, &rows, GRAPH_BREAK_MIN_SPAN);
    DagLayout {
        rows,
        breaks,
        max_lane,
    }
}

use crate::model::{BranchInfo, FileStat};

#[derive(Clone, Debug, Default)]
pub struct BranchTreeNode {
    pub name: String,
    pub path: String,
    pub subdirs: Vec<BranchTreeNode>,
    pub branches: Vec<BranchInfo>,
}

#[derive(Clone, Debug)]
pub struct FlatBranchRow {
    pub is_dir: bool,
    pub depth: usize,
    pub name: String,
    pub path: String,
    pub branch: Option<BranchInfo>,
}

pub fn build_branch_tree(branches: &[BranchInfo]) -> BranchTreeNode {
    let mut root = BranchTreeNode::default();
    for branch in branches {
        let parts: Vec<&str> = branch.name.split('/').filter(|p| !p.is_empty()).collect();
        if parts.is_empty() {
            continue;
        }
        let mut cur = &mut root;
        let mut path = String::new();
        for &part in &parts[..parts.len() - 1] {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(part);
            let idx = if let Some(pos) = cur.subdirs.iter().position(|d| d.name == part) {
                pos
            } else {
                cur.subdirs.push(BranchTreeNode {
                    name: part.to_string(),
                    path: path.clone(),
                    subdirs: Vec::new(),
                    branches: Vec::new(),
                });
                cur.subdirs.len() - 1
            };
            cur = &mut cur.subdirs[idx];
        }
        cur.branches.push(branch.clone());
    }
    root
}

pub fn flatten_branch_tree(
    root: &BranchTreeNode,
    collapsed: &HashSet<String>,
    start_depth: usize,
) -> Vec<FlatBranchRow> {
    let mut out = Vec::new();
    fn walk(
        node: &BranchTreeNode,
        depth: usize,
        collapsed: &HashSet<String>,
        out: &mut Vec<FlatBranchRow>,
    ) {
        for dir in &node.subdirs {
            out.push(FlatBranchRow {
                is_dir: true,
                depth,
                name: dir.name.clone(),
                path: dir.path.clone(),
                branch: None,
            });
            if !collapsed.contains(&dir.path) {
                walk(dir, depth + 1, collapsed, out);
            }
        }
        for b in &node.branches {
            let leaf_name = b.name.split('/').next_back().unwrap_or(&b.name).to_string();
            out.push(FlatBranchRow {
                is_dir: false,
                depth,
                name: leaf_name,
                path: b.name.clone(),
                branch: Some(b.clone()),
            });
        }
    }
    walk(root, start_depth, collapsed, &mut out);
    out
}

#[derive(Clone, Debug, Default)]
pub struct CommitFileTreeNode {
    pub name: String,
    pub path: String,
    pub subdirs: Vec<CommitFileTreeNode>,
    pub files: Vec<FileStat>,
    pub total_files: usize,
}

#[derive(Clone, Debug)]
pub struct FlatFileRow {
    pub is_dir: bool,
    pub depth: usize,
    pub name: String,
    pub path: String,
    pub file: Option<FileStat>,
    pub total_files: usize,
}

pub fn build_commit_file_tree(files: &[FileStat]) -> CommitFileTreeNode {
    let mut root = CommitFileTreeNode::default();
    for file in files {
        let parts: Vec<&str> = file.path.split('/').filter(|p| !p.is_empty()).collect();
        if parts.is_empty() {
            continue;
        }
        let mut cur = &mut root;
        let mut path = String::new();
        for &part in &parts[..parts.len() - 1] {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(part);
            let idx = if let Some(pos) = cur.subdirs.iter().position(|d| d.name == part) {
                pos
            } else {
                cur.subdirs.push(CommitFileTreeNode {
                    name: part.to_string(),
                    path: path.clone(),
                    subdirs: Vec::new(),
                    files: Vec::new(),
                    total_files: 0,
                });
                cur.subdirs.len() - 1
            };
            cur = &mut cur.subdirs[idx];
        }
        cur.files.push(file.clone());
    }

    fn count_and_sort(node: &mut CommitFileTreeNode) -> usize {
        node.subdirs.sort_by(|a, b| a.name.cmp(&b.name));
        node.files.sort_by(|a, b| a.path.cmp(&b.path));
        let mut count = node.files.len();
        for dir in &mut node.subdirs {
            count += count_and_sort(dir);
        }
        node.total_files = count;
        count
    }
    count_and_sort(&mut root);
    root
}

pub fn flatten_commit_file_tree(
    root: &CommitFileTreeNode,
    collapsed: &HashSet<String>,
    start_depth: usize,
) -> Vec<FlatFileRow> {
    let mut out = Vec::new();
    fn walk(
        node: &CommitFileTreeNode,
        depth: usize,
        collapsed: &HashSet<String>,
        out: &mut Vec<FlatFileRow>,
    ) {
        for dir in &node.subdirs {
            out.push(FlatFileRow {
                is_dir: true,
                depth,
                name: dir.name.clone(),
                path: dir.path.clone(),
                file: None,
                total_files: dir.total_files,
            });
            if !collapsed.contains(&dir.path) {
                walk(dir, depth + 1, collapsed, out);
            }
        }
        for f in &node.files {
            let leaf_name = f.path.split('/').next_back().unwrap_or(&f.path).to_string();
            out.push(FlatFileRow {
                is_dir: false,
                depth,
                name: leaf_name,
                path: f.path.clone(),
                file: Some(f.clone()),
                total_files: 1,
            });
        }
    }
    walk(root, start_depth, collapsed, &mut out);
    out
}

pub fn all_commit_file_dir_paths(node: &CommitFileTreeNode) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(node: &CommitFileTreeNode, out: &mut Vec<String>) {
        for dir in &node.subdirs {
            out.push(dir.path.clone());
            walk(dir, out);
        }
    }
    walk(node, &mut out);
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChangeKind;

    fn dummy_commit(id: &str, parents: &[&str]) -> CommitInfo {
        CommitInfo {
            id: id.to_string(),
            short: id.chars().take(7).collect(),
            message: format!("commit {id}"),
            body: String::new(),
            author: "Tester".to_string(),
            email: "test@example.com".to_string(),
            time: 1700000000,
            parents: parents.iter().map(|p| p.to_string()).collect(),
            in_head: Some(true),
            refs: Vec::new(),
            lane: 0,
            lane_count: 0,
            edges: Vec::new(),
        }
    }

    #[test]
    fn test_single_branch_straight() {
        let commits = vec![
            dummy_commit("c3", &["c2"]),
            dummy_commit("c2", &["c1"]),
            dummy_commit("c1", &[]),
        ];
        let (layout, max_lane) = compute_graph_layout(&commits);
        assert_eq!(max_lane, 0);
        assert_eq!(layout["c3"].node, 0);
        assert_eq!(layout["c3"].outs, vec![(0, 0)]);
        assert_eq!(layout["c2"].node, 0);
        assert_eq!(layout["c1"].node, 0);
        assert!(layout["c1"].outs.is_empty());
    }

    #[test]
    fn test_fork_and_merge_diamond() {
        // Diamond: base <- a1 <- a2 <- M; base <- b1 <- M
        // Topological order: M, a2, b1, a1, base
        let commits = vec![
            dummy_commit("M", &["a2", "b1"]),
            dummy_commit("a2", &["a1"]),
            dummy_commit("b1", &["base"]),
            dummy_commit("a1", &["base"]),
            dummy_commit("base", &[]),
        ];
        let (layout, max_lane) = compute_graph_layout(&commits);
        assert_eq!(max_lane, 1);
        assert_eq!(layout["M"].node, 0);
        assert_eq!(layout["M"].outs, vec![(0, 0), (0, 1)]);
        assert_eq!(layout["a2"].node, 0);
        assert_eq!(layout["a2"].straight, vec![1]);
        assert_eq!(layout["b1"].node, 1);
        assert_eq!(layout["b1"].straight, vec![0]);
        assert_eq!(layout["a1"].node, 0);
        assert_eq!(layout["a1"].straight, vec![1]);
        assert_eq!(layout["base"].node, 0);
        assert_eq!(layout["base"].ins, vec![1]);
    }

    #[test]
    fn commit_file_tree_empty_collapsed_set_expands_every_level() {
        let files = vec![
            FileStat {
                path: "a/b/c/deep.rs".into(),
                kind: ChangeKind::Modified,
                additions: 1,
                deletions: 0,
                binary: false,
            },
            FileStat {
                path: "a/top.rs".into(),
                kind: ChangeKind::Added,
                additions: 2,
                deletions: 0,
                binary: false,
            },
        ];
        let tree = build_commit_file_tree(&files);
        let dirs = all_commit_file_dir_paths(&tree);
        assert_eq!(dirs, vec!["a", "a/b", "a/b/c"]);

        let expanded = flatten_commit_file_tree(&tree, &HashSet::new(), 0);
        assert!(expanded.iter().any(|row| row.path == "a/b/c/deep.rs"));
        assert!(expanded.iter().any(|row| row.path == "a/top.rs"));

        let collapsed: HashSet<String> = dirs.into_iter().collect();
        let folded = flatten_commit_file_tree(&tree, &collapsed, 0);
        assert!(!folded.iter().any(|row| row.path == "a/b/c/deep.rs"));
        assert!(!folded.iter().any(|row| row.path == "a/top.rs"));
    }

    #[test]
    fn test_long_span_folding_arrows() {
        // Main branch has 30 commits between fork and merge
        let mut commits = Vec::new();
        commits.push(dummy_commit("M", &["sub_top", "main_top"]));
        commits.push(dummy_commit("sub_top", &["sub_base"]));
        // 30 commits on main
        for i in (1..=30).rev() {
            let id = format!("main_{i}");
            let p = if i == 1 {
                "base".to_string()
            } else {
                format!("main_{}", i - 1)
            };
            commits.push(dummy_commit(&id, &[&p]));
        }
        commits.push(dummy_commit("sub_base", &["base"]));
        commits.push(dummy_commit("base", &[]));

        let dag = compute_dag(&commits);
        // Should detect long span on lane 0 or 1 and produce breaks
        let has_break = dag
            .breaks
            .values()
            .any(|b| !b.below.is_empty() || !b.above.is_empty());
        assert!(
            has_break,
            "Long span >= 25 commits should trigger folding break arrows"
        );
    }
}
