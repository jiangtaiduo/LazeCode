use lazecode_git::log::{assign_lanes_seeded, lanes_after};
use lazecode_git::model::{CommitInfo, GraphEdge, RefKind, RefLabel};

fn commit(id: &str, parents: &[&str]) -> CommitInfo {
    CommitInfo {
        id: id.to_string(),
        short: id.chars().take(8).collect(),
        message: id.to_string(),
        body: String::new(),
        author: "T".into(),
        email: "t@e".into(),
        time: 0,
        parents: parents.iter().map(|p| p.to_string()).collect(),
        in_head: None,
        refs: vec![RefLabel {
            label: id.to_string(),
            kind: RefKind::LocalBranch,
        }],
        lane: 0,
        lane_count: 1,
        edges: Vec::<GraphEdge>::new(),
    }
}

#[test]
fn lanes_prefix_stable_pagination() {
    // main: a -> e ; branch b merges back with c -> d (two tips)
    let rows = vec![
        commit("a", &["e", "d"]),
        commit("d", &["e"]),
        commit("c", &["d"]),
        commit("b", &["c"]),
        commit("e", &[]),
    ];

    // Full list assignment.
    let mut full = rows.clone();
    assign_lanes_seeded(&mut full, &[]);
    let expected_lanes: Vec<usize> = full.iter().map(|c| c.lane).collect();

    // First page: commit a only. Second page continues from the seed.
    let mut page1 = vec![commit("a", &["e", "d"])];
    assign_lanes_seeded(&mut page1, &[]);
    assert_eq!(page1[0].lane, full[0].lane, "first row lane identical");

    let seed = lanes_after(&page1);
    let mut page2 = vec![commit("d", &["e"]), commit("c", &["d"])];
    assign_lanes_seeded(&mut page2, &seed);
    assert_eq!(
        page2[0].lane, full[1].lane,
        "page 2 first row must reuse the seeded lane"
    );
    let _ = expected_lanes;
}

#[test]
fn lanes_merge_and_fork_shape() {
    let mut rows = vec![
        commit("m", &["e", "c"]), // merge commit
        commit("e", &[]),
        commit("c", &["b"]),
        commit("b", &[]),
    ];
    assign_lanes_seeded(&mut rows, &[]);
    let merge = &rows[0];
    assert_eq!(merge.edges.len(), 2, "two parents, two edges");
    assert!(
        merge.edges.iter().all(|edge| edge.from_lane == merge.lane),
        "both edges leave from the merge node's lane"
    );
    // The first parent keeps the node lane; the second gets another lane.
    assert_ne!(merge.edges[0].to_lane, merge.edges[1].to_lane);
}

#[test]
fn empty_parents_release_lane() {
    let mut rows = vec![commit("root", &[])];
    assign_lanes_seeded(&mut rows, &[]);
    assert_eq!(rows[0].lane_count, 1);
    assert!(lanes_after(&rows).into_iter().all(|lane| lane.is_none()));
}
