use std::collections::BTreeMap;
use std::path::Path;

use gix::hash::ObjectId;

use crate::model::{BranchInfo, CommitInfo, GraphEdge, LogOptions, RefKind, RefLabel};
use crate::{discover, short_id, Result};

/// Collect refs grouped by the commit they point at.
fn ref_map(repo: &gix::Repository) -> Result<BTreeMap<ObjectId, Vec<RefLabel>>> {
    let mut map: BTreeMap<ObjectId, Vec<RefLabel>> = BTreeMap::new();
    for reference in repo.references()?.all()? {
        let Ok(mut reference) = reference else {
            continue;
        };
        let name = reference.name().to_string();
        let (label, kind) = if let Some(rest) = name.strip_prefix("refs/heads/") {
            (rest.to_string(), RefKind::LocalBranch)
        } else if let Some(rest) = name.strip_prefix("refs/remotes/") {
            if rest.ends_with("/HEAD") {
                continue;
            }
            (rest.to_string(), RefKind::RemoteBranch)
        } else if let Some(rest) = name.strip_prefix("refs/tags/") {
            (rest.to_string(), RefKind::Tag)
        } else {
            continue;
        };
        let id = if kind == RefKind::Tag {
            reference.peel_to_id().ok().map(|id| id.detach())
        } else {
            reference.target().try_id().map(|id| id.to_owned())
        };
        if let Some(id) = id {
            map.entry(id).or_default().push(RefLabel { label, kind });
        }
    }
    Ok(map)
}

pub fn log(root: &Path, options: LogOptions) -> Result<Vec<CommitInfo>> {
    let repo = discover(root)?;
    let head_id = match repo.head_id() {
        Ok(id) => id.detach(),
        Err(_) => return Ok(Vec::new()),
    };
    log_from_tip(&repo, head_id, options)
}

/// Log starting at an arbitrary revision (branch, tag, commit).
pub fn log_ref(root: &Path, spec: &str, options: LogOptions) -> Result<Vec<CommitInfo>> {
    let repo = discover(root)?;
    let tip = crate::resolve_commit(&repo, spec)?;
    log_from_tip(&repo, tip, options)
}

fn log_from_tip(
    repo: &gix::Repository,
    tip: ObjectId,
    options: LogOptions,
) -> Result<Vec<CommitInfo>> {
    let refs = ref_map(repo)?;
    let head_branch = repo
        .head()
        .ok()
        .and_then(|head| head.referent_name().map(|name| name.shorten().to_string()));
    let limit = if options.limit == 0 {
        300
    } else {
        options.limit.min(5_000)
    };
    let mut walk = repo.rev_walk([tip]);
    if options.first_parent {
        walk = walk.first_parent_only();
    }
    let walk = walk.all()?;

    let mut commits: Vec<CommitInfo> = Vec::with_capacity(limit.min(512));
    for info in walk.take(limit) {
        let Ok(info) = info else { continue };
        let Ok(commit) = info.object() else { continue };
        let raw = commit
            .message_raw()
            .map(|msg| msg.to_string())
            .unwrap_or_default();
        let (message, body) = split_message(&raw);
        let author = commit.author().ok();
        let name = author
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
        let id = info.id().detach();
        let mut labels = refs.get(&id).cloned().unwrap_or_default();
        if head_branch.as_deref().is_some_and(|branch| {
            labels
                .iter()
                .any(|label| label.kind == RefKind::LocalBranch && label.label == branch)
        }) {
            labels.insert(
                0,
                RefLabel {
                    label: "HEAD".to_string(),
                    kind: RefKind::Head,
                },
            );
        }
        commits.push(CommitInfo {
            short: short_id(&id),
            id: id.to_string(),
            message,
            body,
            author: name,
            email,
            time,
            parents,
            refs: labels,
            lane: 0,
            lane_count: 1,
            edges: Vec::new(),
        });
    }

    assign_lanes(&mut commits);
    Ok(commits)
}

fn split_message(raw: &str) -> (String, String) {
    let mut lines = raw.lines();
    let subject = lines.next().unwrap_or("").trim().to_string();
    let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    (subject, body)
}

/// Assign a stable lane per commit and build parent edges for the graph view.
fn assign_lanes(commits: &mut [CommitInfo]) {
    let mut lanes: Vec<Option<String>> = Vec::new();
    for commit in commits.iter_mut() {
        let lane = lanes
            .iter()
            .position(|lane| lane.as_deref() == Some(commit.id.as_str()))
            .unwrap_or_else(|| {
                let position = lanes
                    .iter()
                    .position(|lane| lane.is_none())
                    .unwrap_or(lanes.len());
                if position == lanes.len() {
                    lanes.push(None);
                }
                position
            });
        commit.lane = lane;
        let parents = commit.parents.clone();
        if parents.is_empty() {
            lanes[lane] = None;
        } else {
            lanes[lane] = Some(parents[0].clone());
            for parent in &parents[1..] {
                if lanes
                    .iter()
                    .any(|lane| lane.as_deref() == Some(parent.as_str()))
                {
                    continue;
                }
                if let Some(free) = lanes.iter().position(|lane| lane.is_none()) {
                    lanes[free] = Some(parent.clone());
                } else {
                    lanes.push(Some(parent.clone()));
                }
            }
        }
        // Collapse lanes that now track the same parent commit.
        for i in 0..lanes.len() {
            if lanes[i].is_none() {
                continue;
            }
            for j in (i + 1)..lanes.len() {
                if lanes[i] == lanes[j] {
                    lanes[j] = None;
                }
            }
        }
        commit.lane_count = lanes
            .iter()
            .filter(|lane| lane.is_some())
            .count()
            .max(commit.lane + 1);
    }

    let lane_by_id: BTreeMap<String, usize> = commits
        .iter()
        .map(|commit| (commit.id.clone(), commit.lane))
        .collect();
    let edge_sets: Vec<(usize, Vec<GraphEdge>)> = commits
        .iter()
        .map(|commit| {
            let from_lane = commit.lane;
            let edges = commit
                .parents
                .iter()
                .map(|parent| {
                    let to_lane = lane_by_id.get(parent).copied().unwrap_or(from_lane);
                    GraphEdge {
                        from_lane,
                        to_lane,
                        color_lane: to_lane,
                        target_commit: Some(parent.clone()),
                    }
                })
                .collect();
            (from_lane, edges)
        })
        .collect();
    for (commit, (_, edges)) in commits.iter_mut().zip(edge_sets) {
        commit.edges = edges;
    }
}

pub fn branches(root: &Path) -> Result<Vec<BranchInfo>> {
    let repo = discover(root)?;
    let head_branch = repo
        .head()
        .ok()
        .and_then(|head| head.referent_name().map(|name| name.shorten().to_string()));
    let mut branches = Vec::new();
    for reference in repo.references()?.all()? {
        let Ok(reference) = reference else { continue };
        let name = reference.name().to_string();
        let (short, remote) = if let Some(rest) = name.strip_prefix("refs/heads/") {
            (rest.to_string(), false)
        } else if let Some(rest) = name.strip_prefix("refs/remotes/") {
            if rest.ends_with("/HEAD") {
                continue;
            }
            (rest.to_string(), true)
        } else {
            continue;
        };
        let target = reference
            .target()
            .try_id()
            .map(|id| short_id(&id.to_owned()))
            .unwrap_or_else(|| "?".to_string());
        branches.push(BranchInfo {
            current: !remote && head_branch.as_deref() == Some(short.as_str()),
            full_name: name,
            name: short,
            remote,
            target,
        });
    }
    branches.sort_by(|a, b| a.remote.cmp(&b.remote).then_with(|| a.name.cmp(&b.name)));
    Ok(branches)
}

pub fn tags(root: &Path) -> Result<Vec<String>> {
    let repo = discover(root)?;
    let mut tags = Vec::new();
    for reference in repo.references()?.all()? {
        let Ok(reference) = reference else { continue };
        let name = reference.name().to_string();
        if let Some(rest) = name.strip_prefix("refs/tags/") {
            tags.push(rest.to_string());
        }
    }
    tags.sort();
    Ok(tags)
}
