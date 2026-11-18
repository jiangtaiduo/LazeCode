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

pub fn log(root: &Path, mut options: LogOptions) -> Result<Vec<CommitInfo>> {
    let repo = discover(root)?;
    // Two-element dotted ranges drive the walk's hidden side.
    if let Some(branch) = options.branch.as_deref() {
        if branch.split_once("..").is_some() {
            options.range_walk = true;
        }
    }
    let tips = log_tips(&repo, &options)?;
    log_from_tips(&repo, &tips, options)
}

/// Log starting at an arbitrary revision (branch, tag, commit).
pub fn log_ref(root: &Path, spec: &str, options: LogOptions) -> Result<Vec<CommitInfo>> {
    let repo = discover(root)?;
    let tip = crate::resolve_commit(&repo, spec)?;
    log_from_tips(&repo, &[tip], options)
}

/// Resolve the walk tips (and hidden boundary ids) implied by `options`:
/// `- Some(branch)` walks that branch; `- all_refs` walks heads+remotes
/// (deliberately not refs/stash, like `--branches --remotes`); otherwise the
/// current HEAD.
fn log_tips(repo: &gix::Repository, options: &LogOptions) -> Result<Vec<ObjectId>> {
    if let Some(name) = options.branch.as_deref().filter(|b| !b.trim().is_empty()) {
        // `left..right` walks `right` and hides `left`'s ancestry (the
        // log_view's "commits only in X" slice).
        if let Some((excluded, tip_name)) = name.trim().split_once("..") {
            let tip = crate::resolve_commit(repo, tip_name.trim().trim_start_matches('.'))?;
            if excluded.trim().is_empty() {
                return Ok(vec![tip]);
            }
            let _excluded = crate::resolve_commit(repo, excluded.trim())?;
            return Ok(vec![tip, _excluded]);
        }
        let id = crate::resolve_commit(repo, name.trim())?;
        return Ok(vec![id]);
    }
    if options.all_refs {
        let head = repo.head_id().ok().map(|id| id.detach());
        let mut tips: Vec<ObjectId> = Vec::new();
        if let Some(head) = head {
            tips.push(head);
        }
        for reference in repo.references()?.all()? {
            let Ok(reference) = reference else { continue };
            let name = reference.name().to_string();
            let usable = name.starts_with("refs/heads/") || name.starts_with("refs/remotes/");
            let symbolic = name.starts_with("refs/remotes/") && name.ends_with("/HEAD");
            let stash = name == "refs/stash" || name.starts_with("refs/stash/");
            if !usable || symbolic || stash {
                continue;
            }
            if let Some(id) = reference.target().try_id() {
                let id = id.to_owned();
                if !tips.contains(&id) {
                    tips.push(id);
                }
            }
        }
        return Ok(tips);
    }
    match repo.head_id() {
        Ok(id) => Ok(vec![id.detach()]),
        Err(_) => Ok(Vec::new()),
    }
}

fn log_from_tips(
    repo: &gix::Repository,
    tips: &[ObjectId],
    options: LogOptions,
) -> Result<Vec<CommitInfo>> {
    if tips.is_empty() {
        return Ok(Vec::new());
    }
    let refs = ref_map(repo)?;
    let head_branch = repo
        .head()
        .ok()
        .and_then(|head| head.referent_name().map(|name| name.shorten().to_string()));
    let limit = if options.limit == 0 {
        300
    } else {
        options.limit.min(100_000)
    };
    let walk = repo.rev_walk(tips.iter().copied());
    // `git log`'s topo order keeps parents after children so lane graphs
    // terminate; date order must also guarantee that property in gix's
    // `ByCommitTime` walk.
    let sorting = match options.topo_order {
        true => gix::revision::walk::Sorting::BreadthFirst,
        false => gix::revision::walk::Sorting::ByCommitTime(
            gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
        ),
    };
    let mut walk = walk.sorting(sorting);
    if options.first_parent {
        walk = walk.first_parent_only();
    }
    // When the caller passed a `left..right` range, tips[1] is the excluded
    // side: never emit it or its ancestors (git: `right --not left`).
    if tips.len() == 2 && options.range_walk {
        let excluded = tips[1];
        walk = walk.with_hidden([excluded]);
    }
    let walk = walk.all()?;
    let path_needle = options
        .path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty());
    let head_ancestors = options.head_ancestors.clone().unwrap_or_default();

    let mut commits: Vec<CommitInfo> = Vec::with_capacity(limit.min(512));
    for info in walk {
        if commits.len() >= limit {
            break;
        }
        let Ok(info) = info else { continue };
        let id = info.id().detach();
        let Ok(commit) = repo.find_object(info.id()) else {
            continue;
        };
        let commit: gix::Commit<'_> = commit.try_into_commit()?;
        let parents: Vec<String> = info.parent_ids().map(|id| id.to_string()).collect();
        // Filters shared with the walk predicate (walkers don't author-filter).
        let sig = commit.author().ok();
        let name = sig
            .as_ref()
            .map(|sig| sig.name.to_string())
            .unwrap_or_default();
        let email = sig
            .as_ref()
            .map(|sig| sig.email.to_string())
            .unwrap_or_default();
        let time = sig
            .as_ref()
            .and_then(|sig| sig.time.split_whitespace().next())
            .and_then(|t| t.parse::<i64>().ok())
            .unwrap_or(0);
        if let Some(author) = options.author.as_deref().filter(|a| !a.trim().is_empty()) {
            if !name.eq_ignore_ascii_case(author.trim()) {
                continue;
            }
        }
        if let Some(since) = options.since {
            if time < since {
                // With time sorting the queue is monotone, but branch merges can
                // still inject older commits downstream; keep accepting rather
                // than aborting (matches `git log --since` semantics loosely).
                continue;
            }
        }
        if options.no_merges && parents.len() > 1 {
            continue;
        }
        if let Some(path) = path_needle {
            let commit_tree = commit.tree()?;
            let parent_tree = match commit.parent_ids().next() {
                Some(parent_id) => Some(repo.find_object(parent_id)?.try_into_commit()?.tree()?),
                None => None,
            };
            if !commit_touches_path(repo, commit_tree, parent_tree, path)? {
                continue;
            }
        }
        let raw = commit
            .message_raw()
            .map(|msg| msg.to_string())
            .unwrap_or_default();
        let (message, body) = split_message(&raw);
        let mut labels = refs.get(&id).cloned().unwrap_or_default();
        if let Some(branch) = head_branch.as_deref() {
            // The log label shows the checked-out branch first (PyCharm's
            // current-branch group sits right after HEAD): move it ahead of
            // the other local branches and mark the commit with HEAD.
            if let Some(position) = labels
                .iter()
                .position(|label| label.kind == RefKind::LocalBranch && label.label == branch)
            {
                let current = labels.remove(position);
                labels.insert(0, current);
                labels.insert(
                    0,
                    RefLabel {
                        label: "HEAD".to_string(),
                        kind: RefKind::Head,
                    },
                );
            }
        }
        let in_head = if head_ancestors.is_empty() || head_branch.is_none() {
            None
        } else {
            Some(head_ancestors.contains(&id.to_string()))
        };
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
            in_head,
        });
    }

    assign_lanes_seeded(&mut commits, &[]);
    Ok(commits)
}

/// Does the commit change `path` relative to its first parent? Compares the
/// entry (oid + mode) at `path` in the commit tree against the parent tree.
fn commit_touches_path(
    _repo: &gix::Repository,
    commit_tree: gix::Tree<'_>,
    parent_tree: Option<gix::Tree<'_>>,
    path: &str,
) -> Result<bool> {
    let commits_entry = commit_tree.lookup_entry_by_path(path).ok().flatten();
    let parents_entry = match parent_tree {
        Some(tree) => tree.lookup_entry_by_path(path).ok().flatten(),
        None => None,
    };
    // Submodule (commit-mode) entries are compared like other entries:
    // existence or oid differences count as a change.
    let spec = |entry: Option<gix::object::tree::Entry<'_>>| -> Option<(String, u32)> {
        entry.map(|entry| (entry.object_id().to_string(), entry.mode().value() as u32))
    };
    let (a, b) = (spec(commits_entry), spec(parents_entry));
    Ok(a.is_some() != b.is_some() || (a.is_some() && a != b))
}

fn split_message(raw: &str) -> (String, String) {
    let mut lines = raw.lines();
    let subject = lines.next().unwrap_or("").trim().to_string();
    let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    (subject, body)
}

/// Assign lanes across a commit list, resuming from a previous page's final
/// lane state so pagination keeps lanes stable.
pub fn assign_lanes_seeded(commits: &mut [CommitInfo], seed: &[Option<String>]) {
    let mut lanes: Vec<Option<String>> = seed.to_vec();
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

    // Final lane state doubles as the continuity marker for edges.
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

/// Full hash set of every commit reachable from HEAD (including HEAD).
/// Used to dim log rows not in the current branch's history. Returns `None`
/// when there is no HEAD (unborn) so the view skips the dimming pass.
pub fn head_ancestors(root: &Path) -> Option<std::collections::HashSet<String>> {
    let repo = discover(root).ok()?;
    let head = repo.head_id().ok()?;
    let walk = repo.rev_walk([head]).all().ok()?;
    let mut set = std::collections::HashSet::new();
    for info in walk {
        let Ok(info) = info else { continue };
        set.insert(info.id().to_string());
    }
    Some(set)
}

/// Final lane state after `commits`, usable as the next page's seed.
pub fn lanes_after(commits: &[CommitInfo]) -> Vec<Option<String>> {
    let mut lanes: Vec<Option<String>> = Vec::new();
    for commit in commits {
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
    }
    lanes
}

pub fn branches(root: &Path) -> Result<Vec<BranchInfo>> {
    let repo = discover(root)?;
    let head_branch = repo
        .head()
        .ok()
        .and_then(|head| head.referent_name().map(|name| name.shorten().to_string()));
    // one pass over tags to spot branch-decorated commits quickly.
    let tag_owners = tag_map(&repo)?;
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
        let mut tracking = None;
        let (mut ahead, mut behind) = (0usize, 0usize);
        if !remote {
            // Upstream = branch.<short>.merge resolved against the branch's
            // remote; the common shape is `refs/remotes/<remote>/<short>`.
            if let Some(id) = reference.target().try_id() {
                let id = id.to_owned();
                // find remote-tracking ref for the same branch name.
                for candidate in [
                    format!("refs/remotes/origin/{short}"),
                    format!("refs/remotes/origin/main/{short}"),
                ] {
                    if let Ok(up_ref) = repo.try_find_reference(&candidate) {
                        let Some(up_ref) = up_ref else { continue };
                        if let Some(up_id) = up_ref.try_id() {
                            let up_id = up_id.detach();
                            let (base, left, right) = {
                                match repo.merge_base(id, up_id) {
                                    Ok(Some(base)) => {
                                        let base = base.detach();
                                        (
                                            base,
                                            count_ahead(&repo, id, base).unwrap_or(0),
                                            count_ahead(&repo, up_id, base).unwrap_or(0),
                                        )
                                    }
                                    _ => (id, 0, 0),
                                }
                            };
                            ahead = left;
                            behind = right;
                            let _ = base;
                            tracking = Some(
                                candidate
                                    .strip_prefix("refs/remotes/")
                                    .unwrap_or(&candidate)
                                    .to_string(),
                            );
                            break;
                        }
                    }
                }
            }
        }
        let _ = tag_owners;
        branches.push(BranchInfo {
            current: !remote && head_branch.as_deref() == Some(short.as_str()),
            full_name: name,
            name: short,
            remote,
            target,
            tracking,
            ahead,
            behind,
        });
    }
    branches.sort_by(|a, b| {
        a.remote
            .cmp(&b.remote)
            .then_with(|| b.current.cmp(&a.current))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(branches)
}

/// Number of commits in `from` that are not in `base`'s history.
fn count_ahead(repo: &gix::Repository, from: gix::hash::ObjectId, base: ObjectId) -> Result<usize> {
    let walk = repo.rev_walk([from]).with_hidden([base]).all()?;
    Ok(walk.filter_map(|info| info.ok()).count())
}

/// tags pointing into each commit id, for branch-row badges.
fn tag_map(repo: &gix::Repository) -> Result<std::collections::BTreeMap<ObjectId, Vec<String>>> {
    let mut map = std::collections::BTreeMap::new();
    for reference in repo.references()?.all()? {
        let Ok(mut reference) = reference else {
            continue;
        };
        let name = reference.name().to_string();
        let Some(tag_name) = name.strip_prefix("refs/tags/") else {
            continue;
        };
        if let Ok(id) = reference.peel_to_id() {
            map.entry(id.detach())
                .or_insert_with(Vec::new)
                .push(tag_name.to_string());
        }
    }
    Ok(map)
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
