//! Git log panel: commit list, DAG lanes, click handling and their tests.

use super::branches::*;
use super::commit::*;
use super::prelude::*;

fn paint_quad_bezier(painter: &egui::Painter, p0: Pos2, p1: Pos2, p2: Pos2, stroke: Stroke) {
    const SEGMENTS: usize = 12;
    let mut points = Vec::with_capacity(SEGMENTS + 1);
    for i in 0..=SEGMENTS {
        let t = i as f32 / SEGMENTS as f32;
        let u = 1.0 - t;
        let x = u * u * p0.x + 2.0 * u * t * p1.x + t * t * p2.x;
        let y = u * u * p0.y + 2.0 * u * t * p1.y + t * t * p2.y;
        points.push(Pos2::new(x, y));
    }
    painter.add(egui::Shape::line(points, stroke));
}

fn fold_arrow_hit(start_x: f32, row_y: f32, row_h: f32, lane: usize, is_down: bool) -> Rect {
    let x = start_x + lane_x(lane);
    let tip_y = if is_down {
        row_y + row_h - GRAPH_BREAK_GAP
    } else {
        row_y + GRAPH_BREAK_GAP
    };
    let base_y = if is_down { tip_y - 6.0 } else { tip_y + 6.0 };
    let y0 = if is_down { base_y - 4.0 } else { row_y };
    let y1 = if is_down { row_y + row_h } else { tip_y + 4.0 };
    Rect::from_min_max(Pos2::new(x - 10.0, y0), Pos2::new(x + 10.0, y1))
}

pub(crate) fn draw_dag_cell(
    painter: &egui::Painter,
    row_rect: Rect,
    layout: Option<&GraphRowLayout>,
    brk: Option<&GraphBreak>,
    commit: &CommitInfo,
) {
    let node = layout.map(|l| l.node).unwrap_or(commit.lane);
    let has_in_node = layout.map(|l| l.has_in_node).unwrap_or(true);
    let ins = layout.map(|l| l.ins.as_slice()).unwrap_or(&[]);
    let straight = layout.map(|l| l.straight.as_slice()).unwrap_or(&[]);
    let outs = layout.map(|l| l.outs.as_slice()).unwrap_or(&[]);

    let gap_above: HashSet<usize> = brk
        .map(|b| b.above.iter().map(|a| a.lane).collect())
        .unwrap_or_default();
    let gap_below: HashSet<usize> = brk
        .map(|b| b.below.iter().map(|a| a.lane).collect())
        .unwrap_or_default();
    let omitted = brk.map(|b| &b.omitted);

    let start_x = row_rect.left();
    let half_y = row_rect.center().y;
    let y_top = row_rect.top();
    let y_bottom = row_rect.bottom();
    let stroke_w = 2.0;

    let to_x = |lane: usize| -> f32 { start_x + lane_x(lane) };
    let xm = to_x(node);

    // 1. Straight vertical lines (continuous, extending by 0.5px to bridge subpixel gaps)
    for &lane in straight {
        if omitted.is_some_and(|o| o.contains(&lane)) {
            continue;
        }
        let y1 = if gap_above.contains(&lane) {
            y_top + GRAPH_BREAK_GAP
        } else {
            y_top
        };
        let y2 = if gap_below.contains(&lane) {
            y_bottom - GRAPH_BREAK_GAP
        } else {
            y_bottom + 0.5
        };
        let c = lane_color(lane);
        painter.line_segment(
            [Pos2::new(to_x(lane), y1), Pos2::new(to_x(lane), y2)],
            Stroke::new(stroke_w, Color32::from_rgb(c.0, c.1, c.2)),
        );
    }

    // 2. In-node vertical line
    if has_in_node {
        let y1 = if gap_above.contains(&node) {
            y_top + GRAPH_BREAK_GAP
        } else {
            y_top
        };
        let c = lane_color(node);
        painter.line_segment(
            [Pos2::new(xm, y1), Pos2::new(xm, half_y)],
            Stroke::new(stroke_w, Color32::from_rgb(c.0, c.1, c.2)),
        );
    }

    // 3. Incoming merge curves (ins)
    for &src in ins {
        if src != node {
            let c = lane_color(src);
            let src_x = to_x(src);
            paint_quad_bezier(
                painter,
                Pos2::new(src_x, y_top),
                Pos2::new(src_x, half_y),
                Pos2::new(xm, half_y),
                Stroke::new(stroke_w, Color32::from_rgb(c.0, c.1, c.2)),
            );
        }
    }

    // 4. Outgoing fork curves (outs)
    for &(from, to) in outs {
        let _ = from;
        let c = lane_color(to);
        if to == node {
            let y2 = if gap_below.contains(&node) {
                y_bottom - GRAPH_BREAK_GAP
            } else {
                y_bottom + 0.5
            };
            painter.line_segment(
                [Pos2::new(xm, half_y), Pos2::new(xm, y2)],
                Stroke::new(stroke_w, Color32::from_rgb(c.0, c.1, c.2)),
            );
        } else {
            let target_x = to_x(to);
            let y2 = if gap_below.contains(&to) {
                y_bottom - GRAPH_BREAK_GAP
            } else {
                y_bottom + 0.5
            };
            paint_quad_bezier(
                painter,
                Pos2::new(xm, half_y),
                Pos2::new(target_x, half_y),
                Pos2::new(target_x, y2),
                Stroke::new(stroke_w, Color32::from_rgb(c.0, c.1, c.2)),
            );
        }
    }

    // 5. Commit node circle: solid filled color with 1.5px white border
    let node_c = lane_color(node);
    painter.circle_filled(
        Pos2::new(xm, half_y),
        NODE_R,
        Color32::from_rgb(node_c.0, node_c.1, node_c.2),
    );
    painter.circle_stroke(
        Pos2::new(xm, half_y),
        NODE_R,
        Stroke::new(1.5, Color32::WHITE),
    );

    // 6. Fold break arrows (Image #2)
    if let Some(brk) = brk {
        for arrow in &brk.below {
            let x = to_x(arrow.lane);
            let tip = Pos2::new(x, y_bottom - GRAPH_BREAK_GAP);
            let base_y = tip.y - 6.0;
            let c = lane_color(arrow.lane);
            let color = Color32::from_rgb(c.0, c.1, c.2);
            let poly = vec![Pos2::new(x - 4.5, base_y), Pos2::new(x + 4.5, base_y), tip];
            painter.add(egui::Shape::convex_polygon(poly, color, Stroke::NONE));
        }

        for arrow in &brk.above {
            let x = to_x(arrow.lane);
            let tip = Pos2::new(x, y_top + GRAPH_BREAK_GAP);
            let base_y = tip.y + 6.0;
            let c = lane_color(arrow.lane);
            let color = Color32::from_rgb(c.0, c.1, c.2);
            let poly = vec![Pos2::new(x - 4.5, base_y), Pos2::new(x + 4.5, base_y), tip];
            painter.add(egui::Shape::convex_polygon(poly, color, Stroke::NONE));
        }
    }
}

/// Apply a plain or Cmd/Ctrl click to the Git-log selection.
///
/// Returns the commit whose details now need loading. A Cmd-click that
/// removes a non-active row must not reload it, and removing the active row
/// moves the active marker to the nearest remaining selection instead of
/// leaving the removed row visually selected.
fn apply_log_click(
    visible: &[CommitInfo],
    selected: &mut Vec<String>,
    active: &mut Option<String>,
    id: &str,
    command_click: bool,
) -> Option<String> {
    let id = id.to_string();
    if !command_click {
        *selected = vec![id.clone()];
        *active = Some(id.clone());
        return Some(id);
    }

    if let Some(position) = selected.iter().position(|selected| selected == &id) {
        // Keep one current commit, matching Compare's multi-select semantics.
        if selected.len() <= 1 {
            return None;
        }
        selected.remove(position);
        let active_missing = active
            .as_ref()
            .is_none_or(|active| !selected.contains(active));
        if active_missing {
            let current = visible
                .iter()
                .position(|commit| commit.id == id)
                .unwrap_or(usize::MAX);
            let next = selected
                .iter()
                .filter_map(|selected| {
                    visible
                        .iter()
                        .position(|commit| &commit.id == selected)
                        .map(|row| (row, selected))
                })
                .min_by_key(|(row, _)| {
                    if current == usize::MAX {
                        0
                    } else {
                        row.abs_diff(current)
                    }
                })
                .map(|(_, selected)| selected.clone())
                .or_else(|| selected.last().cloned());
            *active = next.clone();
            return next;
        }
        return None;
    }

    selected.push(id.clone());
    *active = Some(id.clone());
    Some(id)
}

pub(super) fn draw_git_log(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    index: usize,
) {
    // Deferred overlay-menu pick lands here (one frame later), before any
    // borrow of `model.projects[index]` is taken below.
    if model
        .ui
        .log_branch_pick
        .as_ref()
        .is_some_and(|(project_index, _)| *project_index == index)
    {
        let (_, picked) = model.ui.log_branch_pick.take().expect("checked above");
        if let Some(project) = model.projects.get_mut(index) {
            match picked {
                Some(branch) if branch == "__all__" => {
                    project.log_query.all_refs = true;
                    project.log_query.branch.clear();
                }
                Some(branch) => {
                    project.log_query.all_refs = false;
                    project.log_query.branch = branch;
                }
                None => {
                    project.log_query.all_refs = false;
                    project.log_query.branch.clear();
                }
            }
            project.log_loading = true;
        }
        model.queue(Command::LogFilterChanged);
    }
    let p = theme.palette;
    // Take owned clones of everything the painter needs — the body mutates
    // via model.projects[index] elsewhere. The commit list itself is not
    // cloned: the derived view below is cached and only the filtered subset
    // is cloned when the inputs change.
    let (branches, current_branch, selected, details, commit_range, _log_branch, query, error) = {
        let project = &model.projects[index];
        (
            project.branches.clone(),
            project.branch(),
            project.selected_commit.clone(),
            project.commit_details.clone(),
            project.commit_range.clone(),
            project.log_branch.clone(),
            project.log_query.clone(),
            project.log_error.clone(),
        )
    };
    // A multi-selection can be loaded before any single-commit details exist;
    // keep the detail column visible for it as well.
    let range_active = commit_range.as_ref().filter(|range| {
        lazecode_core::selected_commit_range(
            &model.projects[index].log,
            &model.projects[index].selected_commits,
        )
        .is_some_and(|current| current.key == range.key)
    });

    // Filter constants and the derived view (filtered list + lane layout +
    // author chip list) are recomputed only when the log, the filter or the
    // graph toggle changes: doing it per frame recompiled a regex per commit
    // and cloned lane strings for every row.
    let date_since = match query.date {
        lazecode_core::DatePreset::All => (query.since > 0).then_some(query.since),
        lazecode_core::DatePreset::Today => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            Some(now - (now % 86400))
        }
        lazecode_core::DatePreset::Week => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            Some(now.saturating_sub(7 * 86400))
        }
        lazecode_core::DatePreset::Month => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            Some(now.saturating_sub(30 * 86400))
        }
    };
    let author_filter = query.author.trim().to_string();
    let show_graph = model.ui.git_log_show_graph;
    let cache_key = format!(
        "{index}|{}|{date_since:?}|{author_filter}|{}|{}|{}|{show_graph}",
        model.projects[index].log_generation, query.text, query.case_sensitive, query.use_regex,
    );
    let cache = rt.git_log_cache.take();
    let (filtered, dag, authors) = match cache {
        Some(cached) if cached.key == cache_key => (cached.filtered, cached.dag, cached.authors),
        _ => {
            // The query itself never changes while this closure runs, so the
            // needle is lowered once and the regex is compiled once.
            let needle = if query.case_sensitive {
                query.text.clone()
            } else {
                query.text.to_lowercase()
            };
            let regex = (query.use_regex && !needle.is_empty())
                .then(|| lazecode_fs::compile_regex(&needle, false).ok())
                .flatten();
            let matches_query = |haystack: &str| {
                if needle.is_empty() {
                    return true;
                }
                if query.use_regex {
                    return regex
                        .as_ref()
                        .is_some_and(|regex| !haystack.is_empty() && regex.is_match(haystack));
                }
                if query.case_sensitive {
                    haystack.contains(&needle)
                } else {
                    haystack.to_lowercase().contains(&needle)
                }
            };
            let filtered: Vec<CommitInfo> = {
                let log = &model.projects[index].log;
                log.iter()
                    .filter(|commit| {
                        if let Some(since) = date_since {
                            if commit.time < since {
                                return false;
                            }
                        }
                        if !author_filter.is_empty()
                            && !commit.author.eq_ignore_ascii_case(&author_filter)
                        {
                            return false;
                        }
                        matches_query(&commit.message)
                            || matches_query(&commit.author)
                            || needle.is_empty()
                            || commit.id.starts_with(&needle)
                            || commit.refs.iter().any(|label| matches_query(&label.label))
                    })
                    .cloned()
                    .collect()
            };
            let dag = if show_graph {
                lazecode_git::compute_dag(&filtered)
            } else {
                lazecode_git::dag::DagLayout::default()
            };
            let authors: Vec<String> = {
                let mut set = std::collections::BTreeSet::new();
                for commit in &filtered {
                    set.insert(commit.author.clone());
                }
                set.into_iter().take(24).collect()
            };
            (filtered, dag, authors)
        }
    };

    // Sequencer/conflict banner.
    let banner_rect = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 24.0));
    let (conflict_count, sequencer) = {
        let project = &model.projects[index];
        (project.conflicts.len(), project.sequencer.clone())
    };
    let banner_visible = conflict_count > 0 || sequencer.is_some();
    if banner_visible {
        ui.painter()
            .rect_filled(banner_rect, 0.0, p.amber.gamma_multiply(0.18));
        let label = match (&sequencer, conflict_count) {
            _ if conflict_count > 0 => {
                if model.settings.language.is_zh() {
                    format!(
                        "{} 个冲突文件 — 继续/中止操作可从右上角进行",
                        conflict_count
                    )
                } else {
                    format!(
                        "{} conflicted files — continue/abort from the right",
                        conflict_count
                    )
                }
            }
            (Some(lazecode_git::SequencerState::Merge { .. }), 0) => {
                crate::tr!(model, "Merge in progress", "合并进行中").to_string()
            }
            (Some(lazecode_git::SequencerState::Rebase), 0) => {
                crate::tr!(model, "Rebase in progress", "变基进行中").to_string()
            }
            (Some(lazecode_git::SequencerState::CherryPick { .. }), 0) => {
                crate::tr!(model, "Cherry-pick in progress", "Cherry-pick 进行中").to_string()
            }
            (Some(lazecode_git::SequencerState::Revert { .. }), 0) => {
                crate::tr!(model, "Revert in progress", "Revert 进行中").to_string()
            }
            _ => String::new(),
        };
        ui.painter().text(
            Pos2::new(banner_rect.left() + 12.0, banner_rect.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(10.0),
            p.text,
        );
        if conflict_count > 0 {
            let res_btn = Rect::from_center_size(
                Pos2::new(banner_rect.right() - 256.0, banner_rect.center().y),
                Vec2::new(108.0, 18.0),
            );
            let r_resp = ui.interact(
                res_btn,
                ui.scope_id().with("resolve-conflicts-btn"),
                Sense::click(),
            );
            if r_resp.hovered() {
                ui.painter()
                    .rect_filled(res_btn, egui::CornerRadius::same(4), p.accent_soft);
            }
            ui.painter().text(
                res_btn.center(),
                Align2::CENTER_CENTER,
                crate::tr!(model, "⚡ Resolve…", "⚡ 解决冲突…"),
                FontId::proportional(9.5),
                p.accent,
            );
            if r_resp.clicked() {
                if let Some(first) = model.projects[index].conflicts.first() {
                    let root = model.projects[index].active_root().to_path_buf();
                    if let Ok(c) = lazecode_git::merge_file_content(&root, &first.path) {
                        rt.dialog = crate::runtime::DialogState::ConflictResolve {
                            path: first.path.clone(),
                            ours: c.ours,
                            theirs: c.theirs,
                            worktree: c.worktree,
                        };
                    }
                }
            }
        }
        // Continue / Abort buttons.
        for (x, label, kind) in [
            (
                banner_rect.right() - 150.0,
                crate::tr!(model, "Continue", "继续"),
                0u8,
            ),
            (
                banner_rect.right() - 48.0,
                crate::tr!(model, "Abort", "中止"),
                1u8,
            ),
        ] {
            let button =
                Rect::from_center_size(Pos2::new(x, banner_rect.center().y), Vec2::new(88.0, 18.0));
            let response =
                ui.interact(button, ui.scope_id().with(("seqbtn", kind)), Sense::click());
            if response.hovered() {
                ui.painter()
                    .rect_filled(button, egui::CornerRadius::same(4), p.hover);
            }
            ui.painter().text(
                button.center(),
                Align2::CENTER_CENTER,
                label,
                FontId::proportional(9.5),
                if kind == 1 { p.rose } else { p.accent },
            );
            if response.clicked() {
                if kind == 0 {
                    // Continue semantic depends on the sequencer.
                    match sequencer {
                        Some(lazecode_git::SequencerState::Merge { .. }) => {
                            model.queue(Command::MergeContinue);
                        }
                        Some(lazecode_git::SequencerState::Rebase) => {
                            model.queue(Command::RebaseContinue);
                        }
                        Some(lazecode_git::SequencerState::CherryPick { .. }) => {
                            model.queue(Command::ContinueCherryPick);
                        }
                        Some(lazecode_git::SequencerState::Revert { .. }) => {
                            model.queue(Command::RevertContinue);
                        }
                        _ => {}
                    }
                } else {
                    match sequencer {
                        Some(lazecode_git::SequencerState::Merge { .. }) => {
                            model.queue(Command::MergeAbort);
                        }
                        Some(lazecode_git::SequencerState::Rebase) => {
                            model.queue(Command::RebaseAbort);
                        }
                        Some(lazecode_git::SequencerState::CherryPick { .. }) => {
                            model.queue(Command::AbortCherryPick);
                        }
                        Some(lazecode_git::SequencerState::Revert { .. }) => {
                            model.queue(Command::RevertAbort);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    let body_offset = if banner_visible { 24.0 } else { 0.0 };
    let body = Rect::from_min_max(Pos2::new(rect.left(), rect.top() + body_offset), rect.max);

    // 1. Column 1: Vertical tool strip on the far left (30.0px wide, exactly matching stable branch-vertical-toolbar)
    let toolbar_w = 30.0;
    let toolbar_rect = Rect::from_min_size(body.min, Vec2::new(toolbar_w, body.height()));

    // 2. Column 2: Branch tree column
    let show_branches = model.ui.git_branches_visible && body.width() > 500.0;
    let branch_w = if show_branches {
        model.ui.git_branches_width.clamp(140.0, 360.0)
    } else {
        0.0
    };
    let branch_rect = Rect::from_min_size(
        Pos2::new(toolbar_rect.right(), body.top()),
        Vec2::new(branch_w, body.height()),
    );

    // 3. Column 4: Commit details & changed files column
    let show_detail = model.ui.git_details_visible
        && (details.is_some() || range_active.is_some())
        && body.width() > 700.0;
    let detail_w = if show_detail {
        model.ui.git_details_width.clamp(220.0, 500.0)
    } else {
        0.0
    };
    let detail_rect = Rect::from_min_max(Pos2::new(body.right() - detail_w, body.top()), body.max);

    // 4. Column 3: Center commits column (contains filter bar at top, and commit log table below)
    let commits_left = if show_branches {
        branch_rect.right()
    } else {
        toolbar_rect.right()
    };
    let commits_right = if show_detail {
        detail_rect.left()
    } else {
        body.right()
    };
    let commits_col = Rect::from_min_max(
        Pos2::new(commits_left, body.top()),
        Pos2::new(commits_right, body.bottom()),
    );

    // Filter bar lives strictly at the top of Column 3 (Commits column), exactly matching stable git-filter-bar!
    let filter = Rect::from_min_size(commits_col.min, Vec2::new(commits_col.width(), 30.0));
    let list_rect = Rect::from_min_max(
        Pos2::new(commits_col.left(), filter.bottom()),
        commits_col.max,
    );

    ui.painter().rect_filled(filter, 0.0, p.panel);
    ui.painter().hline(
        filter.x_range(),
        filter.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    let search_w = (filter.width() * 0.30).clamp(150.0, 210.0);
    let search_rect = Rect::from_min_size(
        Pos2::new(filter.left() + 8.0, filter.top() + 4.0),
        Vec2::new(search_w, 22.0),
    );
    ui.painter()
        .rect_filled(search_rect, egui::CornerRadius::same(5), p.panel2);
    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(search_rect.left() + 11.0, search_rect.center().y),
            Vec2::splat(11.0),
        ),
        Icon::Search,
        p.text3,
    );
    let field_rect = Rect::from_min_max(
        Pos2::new(search_rect.left() + 22.0, search_rect.top() + 2.0),
        Pos2::new(search_rect.right() - 42.0, search_rect.bottom() - 2.0),
    );
    let mut query_value = query.text.clone();
    let response = ui.put(
        field_rect,
        TextEdit::singleline(&mut query_value)
            .frame(egui::Frame::NONE)
            .align(egui::Align2::LEFT_CENTER)
            .hint_text(crate::tr!(
                model,
                "Filter commits, authors, refs",
                "过滤提交、作者、引用"
            ))
            .font(FontId::proportional(12.0)),
    );
    if response.changed() {
        model.projects[index].log_query.text = query_value;
    }

    let draw_toggle = |ui: &mut Ui, rect: Rect, text: &str, active: bool, tooltip: &str| -> bool {
        if active {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(3), p.active);
        }
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::monospace(10.5),
            if active { p.accent } else { p.text3 },
        );
        let resp = ui.interact(
            rect,
            ui.scope_id().with(("logtoggle", tooltip)),
            Sense::click(),
        );
        if resp.hovered() {
            resp.show_tooltip_text(tooltip);
        }
        resp.clicked()
    };

    let regex_rect = Rect::from_center_size(
        Pos2::new(search_rect.right() - 30.0, search_rect.center().y),
        Vec2::splat(18.0),
    );
    if draw_toggle(
        ui,
        regex_rect,
        ".*",
        query.use_regex,
        if model.settings.language.is_zh() {
            "正则匹配"
        } else {
            "Regular expression"
        },
    ) {
        model.projects[index].log_query.use_regex = !query.use_regex;
    }
    let case_rect = Rect::from_center_size(
        Pos2::new(search_rect.right() - 11.0, search_rect.center().y),
        Vec2::splat(18.0),
    );
    if draw_toggle(
        ui,
        case_rect,
        "Cc",
        query.case_sensitive,
        if model.settings.language.is_zh() {
            "区分大小写"
        } else {
            "Match case"
        },
    ) {
        model.projects[index].log_query.case_sensitive = !query.case_sensitive;
    }
    let mut button_x = search_rect.right() + 6.0;
    // Compute right action buttons first to establish right boundary for filter chips
    let mut frx = filter.right() - 6.0;
    let refresh_rect =
        Rect::from_min_size(Pos2::new(frx - 20.0, filter.top() + 5.0), Vec2::splat(20.0));
    let refresh_resp = icon_button(ui, refresh_rect, Icon::Refresh, false, theme);
    if refresh_resp.hovered() {
        refresh_resp.show_tooltip_text(crate::tr!(model, "Refresh Log", "刷新日志"));
    }
    if refresh_resp.clicked() {
        model.queue(Command::RefreshLog);
    }
    frx -= 22.0;

    let graph_rect =
        Rect::from_min_size(Pos2::new(frx - 20.0, filter.top() + 5.0), Vec2::splat(20.0));
    let graph_resp = icon_button(
        ui,
        graph_rect,
        Icon::Graph,
        model.ui.git_log_show_graph,
        theme,
    );
    if graph_resp.hovered() {
        graph_resp.show_tooltip_text(crate::tr!(model, "Toggle Commit Graph", "显示/隐藏提交树"));
    }
    if graph_resp.clicked() {
        model.ui.git_log_show_graph = !model.ui.git_log_show_graph;
    }
    frx -= 22.0;

    let sort_rect =
        Rect::from_min_size(Pos2::new(frx - 20.0, filter.top() + 5.0), Vec2::splat(20.0));
    let sort_active = query.topo_order || query.first_parent || query.no_merges;
    let sort_resp = icon_button(ui, sort_rect, Icon::Sort, sort_active, theme);
    if sort_resp.hovered() {
        sort_resp.show_tooltip_text(crate::tr!(model, "Sort and Log Options", "排序与日志选项"));
    }
    if sort_resp.clicked() {
        rt.toggle_menu(
            crate::runtime::MenuKind::LogSortOptions { project: index },
            sort_rect,
            None,
        );
    }
    frx -= 26.0;

    let count_text = if model.projects[index].log_paging {
        if model.settings.language.is_zh() {
            format!("{} 个提交...", model.projects[index].log.len())
        } else {
            format!("{} commits...", model.projects[index].log.len())
        }
    } else if model.settings.language.is_zh() {
        format!("{} 个提交", model.projects[index].log.len())
    } else {
        format!("{} commits", model.projects[index].log.len())
    };
    let gal = ui
        .painter()
        .layout_no_wrap(count_text, FontId::monospace(10.0), p.text3);
    let count_w = gal.size().x;
    ui.painter().galley(
        Pos2::new(frx - count_w, filter.center().y - gal.size().y * 0.5),
        gal,
        p.text3,
    );
    let max_chip_right = frx - count_w - 10.0;

    // Branch filter menu
    let branch_label = if query.all_refs {
        if model.settings.language.is_zh() {
            "全部分支".to_string()
        } else {
            "All branches".to_string()
        }
    } else if query.branch.is_empty() {
        format!("⎇ {current_branch}")
    } else {
        format!("⎇ {}", query.branch)
    };
    let branch_w = (branch_label.chars().count() as f32 * 7.0 + 20.0).clamp(76.0, 140.0);
    if button_x + branch_w <= max_chip_right {
        let branch_menu_rect = Rect::from_min_size(
            Pos2::new(button_x, filter.top() + 4.5),
            Vec2::new(branch_w, 21.0),
        );
        ui.painter()
            .rect_filled(branch_menu_rect, egui::CornerRadius::same(4), p.panel2);
        ui.painter().text(
            branch_menu_rect.center(),
            Align2::CENTER_CENTER,
            &branch_label,
            FontId::proportional(12.0),
            p.text2,
        );
        let branch_menu_open = ui
            .interact(
                branch_menu_rect,
                ui.scope_id().with("log-branch-menu"),
                Sense::click(),
            )
            .clicked();
        if branch_menu_open {
            rt.toggle_menu(
                crate::runtime::MenuKind::LogBranchFilter { project: index },
                branch_menu_rect,
                None,
            );
        }
        button_x += branch_w + 6.0;
    }

    // Author chip
    let author_label = if query.author.is_empty() {
        if model.settings.language.is_zh() {
            "👤 全部作者".to_string()
        } else {
            "👤 All authors".to_string()
        }
    } else {
        format!("👤 {}", query.author)
    };
    let author_w = (author_label.chars().count() as f32 * 7.0 + 20.0).clamp(86.0, 160.0);
    if button_x + author_w <= max_chip_right {
        let author_rect = Rect::from_min_size(
            Pos2::new(button_x, filter.top() + 4.5),
            Vec2::new(author_w, 21.0),
        );
        ui.painter()
            .rect_filled(author_rect, egui::CornerRadius::same(4), p.panel2);
        ui.painter().text(
            author_rect.center(),
            Align2::CENTER_CENTER,
            &author_label,
            FontId::proportional(12.0),
            if query.author.is_empty() {
                p.text2
            } else {
                p.accent
            },
        );
        let author_open = ui
            .interact(
                author_rect,
                ui.scope_id().with("log-author-chip"),
                Sense::click(),
            )
            .clicked();
        if author_open {
            rt.toggle_menu(
                crate::runtime::MenuKind::LogBranchFilter { project: index },
                author_rect,
                Some(authors.join("\n")),
            );
        }
        button_x += author_w + 6.0;
    }

    if model.ui.log_author_pick.is_some() {
        let (pick_project, picked) = model.ui.log_author_pick.take().expect("checked above");
        if pick_project == index {
            if let Some(project) = model.projects.get_mut(pick_project) {
                project.log_query.author = picked.unwrap_or_default();
                project.log_loading = true;
            }
            model.queue(Command::LogFilterChanged);
        }
    }

    // Date filter chip
    let date_label = match query.date {
        lazecode_core::DatePreset::All => crate::tr!(model, "Date ▾", "日期 ▾").to_string(),
        lazecode_core::DatePreset::Today => crate::tr!(model, "Today", "今天").to_string(),
        lazecode_core::DatePreset::Week => {
            crate::tr!(model, "Last 7 days", "最近 7 天").to_string()
        }
        lazecode_core::DatePreset::Month => {
            crate::tr!(model, "Last 30 days", "最近 30 天").to_string()
        }
    };
    let date_w = (date_label.chars().count() as f32 * 7.5 + 20.0).clamp(62.0, 110.0);
    if button_x + date_w <= max_chip_right {
        let date_rect = Rect::from_min_size(
            Pos2::new(button_x, filter.top() + 4.5),
            Vec2::new(date_w, 21.0),
        );
        ui.painter()
            .rect_filled(date_rect, egui::CornerRadius::same(4), p.panel2);
        ui.painter().text(
            date_rect.center(),
            Align2::CENTER_CENTER,
            &date_label,
            FontId::proportional(12.0),
            if query.date == lazecode_core::DatePreset::All {
                p.text2
            } else {
                p.accent
            },
        );
        if ui
            .interact(
                date_rect,
                ui.scope_id().with("log-date-chip"),
                Sense::click(),
            )
            .clicked()
        {
            rt.toggle_menu(
                crate::runtime::MenuKind::LogDateFilter { project: index },
                date_rect,
                None,
            );
        }
        button_x += date_w + 6.0;
    }

    // Paths filter chip
    let paths_label = if query.path.is_empty() {
        crate::tr!(model, "Paths ▾", "路径 ▾").to_string()
    } else {
        format!("📁 {}", query.path)
    };
    let paths_w = (paths_label.chars().count() as f32 * 7.0 + 18.0).clamp(62.0, 120.0);
    if button_x + paths_w <= max_chip_right {
        let paths_rect = Rect::from_min_size(
            Pos2::new(button_x, filter.top() + 4.5),
            Vec2::new(paths_w, 21.0),
        );
        ui.painter()
            .rect_filled(paths_rect, egui::CornerRadius::same(4), p.panel2);
        ui.painter().text(
            paths_rect.center(),
            Align2::CENTER_CENTER,
            &paths_label,
            FontId::proportional(12.0),
            if query.path.is_empty() {
                p.text2
            } else {
                p.accent
            },
        );
        if ui
            .interact(
                paths_rect,
                ui.scope_id().with("log-path-chip"),
                Sense::click(),
            )
            .clicked()
        {
            rt.toggle_menu(
                crate::runtime::MenuKind::LogPathFilter { project: index },
                paths_rect,
                Some(query.path.clone()),
            );
        }
    }
    // Paint vertical toolbar (30px wide, PyCharm style)
    ui.painter().rect_filled(toolbar_rect, 0.0, p.panel);
    ui.painter().vline(
        toolbar_rect.right() - 0.5,
        toolbar_rect.y_range(),
        Stroke::new(1.0, p.border_soft),
    );
    let mut tb_y = toolbar_rect.top() + 6.0;
    // The Log tab already identifies the panel; the old vertical "Git"
    // caption wasted the first row of the toolbar.
    ui.painter().hline(
        (toolbar_rect.left() + 7.0)..=(toolbar_rect.right() - 7.0),
        tb_y + 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    tb_y += 6.0;

    // 1. Collapse / expand branches button (« / »)
    let collapse_rect = Rect::from_min_size(
        Pos2::new(toolbar_rect.left() + 3.0, tb_y),
        Vec2::splat(24.0),
    );
    let collapse_icon = if model.ui.git_branches_visible {
        Icon::DoubleChevronLeft
    } else {
        Icon::DoubleChevronRight
    };
    let collapse_resp = icon_button(ui, collapse_rect, collapse_icon, false, theme);
    if collapse_resp.hovered() {
        collapse_resp.show_tooltip_text(if model.ui.git_branches_visible {
            crate::tr!(model, "Hide branches", "收起分支列表")
        } else {
            crate::tr!(model, "Show branches", "展开分支列表")
        });
    }
    if collapse_resp.clicked() {
        model.ui.git_branches_visible = !model.ui.git_branches_visible;
    }
    tb_y += 25.0;

    // Separator 1
    ui.painter().hline(
        (toolbar_rect.left() + 7.0)..=(toolbar_rect.right() - 7.0),
        tb_y + 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    tb_y += 5.0;

    // 2. New branch button (+)
    let new_branch_rect = Rect::from_min_size(
        Pos2::new(toolbar_rect.left() + 3.0, tb_y),
        Vec2::splat(24.0),
    );
    let new_branch_resp = icon_button(ui, new_branch_rect, Icon::Plus, false, theme);
    if new_branch_resp.hovered() {
        new_branch_resp.show_tooltip_text(crate::tr!(model, "New Branch", "新建分支"));
    }
    if new_branch_resp.clicked() {
        rt.dialog = crate::runtime::DialogState::NewBranch {
            value: String::new(),
            start_point: None,
            checkout: true,
            error: None,
        };
    }
    tb_y += 25.0;

    // 3. Update selected branch (↓)
    let update_rect = Rect::from_min_size(
        Pos2::new(toolbar_rect.left() + 3.0, tb_y),
        Vec2::splat(24.0),
    );
    let update_resp = icon_button(ui, update_rect, Icon::UpdateArrow, false, theme);
    if update_resp.hovered() {
        update_resp.show_tooltip_text(crate::tr!(
            model,
            "Update Selected Branch",
            "更新选中的分支"
        ));
    }
    if update_resp.clicked() {
        model.queue(Command::FetchAll);
    }
    tb_y += 25.0;

    // 4. Delete branch button (trash)
    let del_rect = Rect::from_min_size(
        Pos2::new(toolbar_rect.left() + 3.0, tb_y),
        Vec2::splat(24.0),
    );
    let del_resp = icon_button(ui, del_rect, Icon::Trash, false, theme);
    if del_resp.hovered() {
        del_resp.show_tooltip_text(crate::tr!(
            model,
            "Delete Selected Branch",
            "删除选中的分支"
        ));
    }
    if del_resp.clicked() {
        if let Some(target) = model.projects[index].ref_selection.first() {
            if target != &current_branch {
                rt.dialog = crate::runtime::DialogState::DeleteBranch {
                    name: target.clone(),
                    force: false,
                    remote: false,
                    error: None,
                };
            }
        }
    }
    tb_y += 25.0;

    // 5. Checkout selected branch (⑂)
    let checkout_rect = Rect::from_min_size(
        Pos2::new(toolbar_rect.left() + 3.0, tb_y),
        Vec2::splat(24.0),
    );
    let checkout_target = model.projects[index].ref_selection.first().cloned();
    let checkout_enabled = checkout_target
        .as_ref()
        .is_some_and(|name| name != &current_branch);
    let checkout_resp = icon_button(ui, checkout_rect, Icon::Branch, checkout_enabled, theme);
    if checkout_resp.hovered() {
        checkout_resp.show_tooltip_text(crate::tr!(model, "Checkout Branch", "检出分支"));
    }
    if checkout_resp.clicked() {
        if let Some(name) = checkout_target {
            model.queue(Command::CheckoutBranch(name));
        }
    }
    tb_y += 25.0;

    // Separator 2
    ui.painter().hline(
        (toolbar_rect.left() + 7.0)..=(toolbar_rect.right() - 7.0),
        tb_y + 2.5,
        Stroke::new(1.0, p.border_soft),
    );
    tb_y += 6.0;

    // 6. Search branches 🔍
    let bsearch_rect = Rect::from_min_size(
        Pos2::new(toolbar_rect.left() + 3.0, tb_y),
        Vec2::splat(24.0),
    );
    let bsearch_resp = icon_button(
        ui,
        bsearch_rect,
        Icon::Search,
        model.ui.branch_search_open,
        theme,
    );
    if bsearch_resp.hovered() {
        bsearch_resp.show_tooltip_text(crate::tr!(model, "Search branches", "搜索分支"));
    }
    if bsearch_resp.clicked() {
        model.ui.branch_search_open = !model.ui.branch_search_open;
        if !model.ui.branch_search_open {
            model.projects[index].branch_query.clear();
        }
    }
    tb_y += 25.0;

    // 7. Fetch all remotes
    let fetch_rect = Rect::from_min_size(
        Pos2::new(toolbar_rect.left() + 3.0, tb_y),
        Vec2::splat(24.0),
    );
    let fetch_resp = icon_button(ui, fetch_rect, Icon::Refresh, false, theme);
    if fetch_resp.hovered() {
        fetch_resp.show_tooltip_text(crate::tr!(model, "Fetch All Remotes", "从所有远程拉取"));
    }
    if fetch_resp.clicked() {
        model.queue(Command::FetchAll);
    }
    tb_y += 25.0;

    // 8. Star / Favorite button
    let star_rect = Rect::from_min_size(
        Pos2::new(toolbar_rect.left() + 3.0, tb_y),
        Vec2::splat(24.0),
    );
    let star_target = model.projects[index].ref_selection.first().cloned();
    let favorite_root = model.projects[index].root.clone();
    let is_starred = star_target
        .as_ref()
        .is_some_and(|name| model.is_favorite_branch(&favorite_root, name));
    let star_resp = icon_button(ui, star_rect, Icon::Star, is_starred, theme);
    if star_resp.hovered() {
        star_resp.show_tooltip_text(crate::tr!(
            model,
            "Favorite / Unfavorite",
            "收藏 / 取消收藏"
        ));
    }
    if star_resp.clicked() {
        if let Some(target) = star_target {
            model.toggle_favorite_branch(&favorite_root, &target);
            model.save_workspace();
        }
    }
    tb_y += 25.0;

    // 9. Compare Selected Branches (⇄ / Compare)
    let compare_rect = Rect::from_min_size(
        Pos2::new(toolbar_rect.left() + 3.0, tb_y),
        Vec2::splat(24.0),
    );
    let compare_enabled = model
        .projects
        .get(index)
        .map(|project| project.ref_selection.len() == 2)
        .unwrap_or(false);
    let compare_resp = icon_button(ui, compare_rect, Icon::Compare, compare_enabled, theme);
    if compare_resp.hovered() {
        compare_resp.show_tooltip_text(crate::tr!(
            model,
            "Compare Selected Branches",
            "比较选中的分支"
        ));
    }
    if compare_resp.clicked() {
        if let Some(project) = model.projects.get(index) {
            let refs: Vec<String> = project.ref_selection.clone();
            if refs.len() == 2 {
                crate::editor::open_compare_page(model, refs[0].clone(), refs[1].clone());
            } else {
                model.toast(
                    crate::tr!(
                        model,
                        "Select exactly two branches to compare",
                        "请先选中两个分支再比较"
                    )
                    .to_string(),
                    lazecode_core::ToastKind::Warning,
                );
            }
        }
    }
    tb_y += 25.0;

    // Separator 3
    ui.painter().hline(
        (toolbar_rect.left() + 7.0)..=(toolbar_rect.right() - 7.0),
        tb_y + 2.5,
        Stroke::new(1.0, p.border_soft),
    );

    if show_branches {
        let splitter_rect = Rect::from_min_size(
            Pos2::new(branch_rect.right() - 3.0, body.top()),
            Vec2::new(6.0, body.height()),
        );
        let resp = ui.interact(
            splitter_rect,
            ui.scope_id().with("branch-col-splitter"),
            Sense::drag(),
        );
        let is_active_split = resp.hovered() || resp.dragged();
        if is_active_split {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn);
        }
        if resp.dragged() {
            model.ui.git_branches_width =
                (model.ui.git_branches_width + resp.drag_delta().x).clamp(140.0, 360.0);
        }
        let tags = model.projects[index].tags.clone();
        let tag_targets = model.projects[index].tag_targets.clone();
        draw_branch_list(
            ui,
            model,
            rt,
            theme,
            branch_rect,
            index,
            &branches,
            &current_branch,
            &tags,
            &tag_targets,
        );
        ui.painter().vline(
            branch_rect.right() - 0.5,
            body.y_range(),
            if is_active_split {
                Stroke::new(2.0, p.accent)
            } else {
                Stroke::new(1.0, p.border_soft)
            },
        );
    }

    if show_detail {
        let splitter_rect = Rect::from_min_size(
            Pos2::new(detail_rect.left() - 3.0, body.top()),
            Vec2::new(6.0, body.height()),
        );
        let resp = ui.interact(
            splitter_rect,
            ui.scope_id().with("detail-col-splitter"),
            Sense::drag(),
        );
        let is_active_split = resp.hovered() || resp.dragged();
        if is_active_split {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn);
        }
        if resp.dragged() {
            model.ui.git_details_width =
                (model.ui.git_details_width - resp.drag_delta().x).clamp(220.0, 500.0);
        }
        // The range tree reuses the single-commit renderer; its metadata card
        // shows the newest commit in the selection.
        let mut range_info = None;
        if let Some(range) = range_active {
            if let Some(info) = model.projects[index]
                .log
                .iter()
                .find(|commit| commit.id == range.to)
                .cloned()
            {
                range_info = Some(lazecode_git::CommitDetails {
                    info,
                    files: range.files.clone(),
                });
            }
        }
        if let Some(details) = range_info.as_ref().or(details.as_ref()) {
            draw_commit_detail(
                ui,
                model,
                theme,
                detail_rect,
                details,
                rt,
                model.settings.language,
                index,
                range_active,
            );
        }

        ui.painter().vline(
            detail_rect.left() + 0.5,
            body.y_range(),
            if is_active_split {
                Stroke::new(2.0, p.accent)
            } else {
                Stroke::new(1.0, p.border_soft)
            },
        );
    }

    if let Some(error) = error {
        rt.git_log_cache = Some(GitLogCache {
            key: cache_key,
            filtered,
            dag,
            authors,
        });
        ui.painter().text(
            Pos2::new(list_rect.left() + 20.0, list_rect.top() + 30.0),
            Align2::LEFT_CENTER,
            error,
            FontId::proportional(11.0),
            p.rose,
        );
        return;
    }

    let dag_width = if show_graph {
        (10.0 + (dag.max_lane + 1) as f32 * LANE_W).max(40.0)
    } else {
        4.0
    };
    let row_h = GRAPH_H; // 26.0!

    let painter = ui.painter().with_clip_rect(list_rect);
    painter.rect_filled(list_rect, 0.0, p.panel);

    // Keyboard: ↑/↓/Home/End move the active commit while the pointer is over
    // the log table. The existing jump target centres the row, and the normal
    // SelectCommit path refreshes the detail pane.
    let keys_free = !rt.blocks_input(model) && !rt.typing_surface_owns_keys(model, ui.ctx());
    if keys_free {
        if let Some(mv) = crate::list_nav::hovered_move(ui, list_rect) {
            let current = {
                let project = &model.projects[index];
                project
                    .selected_commit
                    .as_ref()
                    .and_then(|id| filtered.iter().position(|c| &c.id == id))
                    .or_else(|| {
                        project
                            .selected_commits
                            .first()
                            .and_then(|id| filtered.iter().position(|c| &c.id == id))
                    })
            };
            if let Some(next) = crate::list_nav::stepped(current, filtered.len(), mv) {
                let id = filtered[next].id.clone();
                {
                    let project = &mut model.projects[index];
                    project.selected_commits = vec![id.clone()];
                    project.selected_commit = Some(id.clone());
                }
                model.queue(Command::SelectCommit(id.clone()));
                model.ui.git_jump_to_commit = Some(id);
            }
        }
    }

    let mut select_commit = None;
    let jump_target = model.ui.git_jump_to_commit.take();
    let mut jump_to = None;
    let mut request_more = false;
    let has_more = !model.projects[index].log_reached_end;
    let total_rows = filtered.len();

    crate::app_ui::region(ui, list_rect, "log-list", |ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        let scroll_offset = jump_target.as_ref().and_then(|target| {
            filtered
                .iter()
                .position(|c| &c.id == target)
                .map(|pos| (pos as f32 * row_h - list_rect.height() * 0.5 + row_h * 0.5).max(0.0))
        });
        let mut scroll_area = egui::ScrollArea::vertical()
            .id_salt("git-log")
            .auto_shrink([false, false]);
        if let Some(offset) = scroll_offset {
            scroll_area = scroll_area.vertical_scroll_offset(offset);
        }
        scroll_area.show_rows(ui, row_h, total_rows, |ui, range| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            // Pure auto-scroll pagination matching main: trigger next page when scrolling within 25 rows of tail
            if has_more && !filtered.is_empty() && range.end + 25 >= filtered.len() {
                request_more = true;
            }
            for row_index in range {
                let commit = &filtered[row_index];
                let (row, response) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::click());
                let out_of_head = commit.in_head == Some(false);
                let is_selected = selected.as_deref() == Some(commit.id.as_str())
                    || model.projects[index].selected_commits.contains(&commit.id);
                if is_selected {
                    // .log-row.active → --bg-selected-row #26415f。
                    ui.painter()
                        .rect_filled(row, egui::CornerRadius::ZERO, p.selected_row);
                } else if response.hovered() {
                    ui.painter()
                        .rect_filled(row, egui::CornerRadius::ZERO, p.hover);
                }
                // inset 底部分隔线（box-shadow: inset 0 -1px 0 overlay-faint）。
                ui.painter().hline(
                    row.x_range(),
                    row.bottom() - 0.5,
                    Stroke::new(1.0, p.hairline),
                );
                let dim = out_of_head && !is_selected;

                // 1. DAG cell (100% continuous vertical lines, zero inter-row gaps)
                if show_graph {
                    let cell_rect = Rect::from_min_size(row.min, Vec2::new(dag_width, row_h));
                    draw_dag_cell(
                        ui.painter(),
                        cell_rect,
                        dag.rows.get(&commit.id),
                        dag.breaks.get(&commit.id),
                        commit,
                    );
                }

                // 2. Commit subject + Merge icon + Ref badges
                let mut msg_x = if show_graph {
                    row.left() + dag_width + 6.0
                } else {
                    row.left() + 10.0
                };
                let is_merge = commit.parents.len() > 1;
                if is_merge {
                    paint_icon(
                        ui.painter(),
                        Rect::from_center_size(
                            Pos2::new(msg_x + 6.0, row.center().y),
                            Vec2::splat(12.0),
                        ),
                        Icon::Merge,
                        if dim {
                            p.text3.gamma_multiply(0.65)
                        } else {
                            p.text3
                        },
                    );
                    msg_x += 16.0;
                }

                // Fixed right columns: date and author (aligning with stable .log-date and .log-author)
                let date_w = 104.0;
                let author_w = 76.0;
                let right_reserved = date_w + author_w + 16.0;
                let content_right = (row.right() - right_reserved).max(msg_x + 100.0);

                // Compact PyCharm-style reference label: one group name plus a
                // fanned icon stack, capped at a third of the text area. The
                // complete reference list lives in the hover tooltip.
                let refs_max_w = ((content_right - msg_x) / 3.0).clamp(24.0, 240.0);
                let refs_cluster = crate::log_refs::prepare(ui.painter(), &commit.refs, refs_max_w);
                let refs_w = refs_cluster
                    .as_ref()
                    .map(|cluster| cluster.width + 6.0)
                    .unwrap_or(0.0);

                // Available space for subject
                let avail_for_subject = (content_right - msg_x - refs_w).max(80.0);
                let subject_font = FontId::proportional(13.0);
                let subject_color = if dim {
                    p.text3.gamma_multiply(0.65)
                } else {
                    p.text
                };

                // Measure subject text width
                let subject_galley = ui.painter().layout_no_wrap(
                    commit.message.clone(),
                    subject_font.clone(),
                    subject_color,
                );
                let actual_subject_w = subject_galley.size().x.min(avail_for_subject);

                text_clipped(
                    ui.painter(),
                    Rect::from_min_max(
                        Pos2::new(msg_x, row.top()),
                        Pos2::new(msg_x + actual_subject_w, row.bottom()),
                    ),
                    &commit.message,
                    subject_font,
                    subject_color,
                    Align2::LEFT_CENTER,
                );

                // log-ref: drawn immediately following the subject text (stable flex layout)
                if let Some(cluster) = refs_cluster {
                    let badge_x = msg_x + actual_subject_w + 6.0;
                    let id = ui.make_persistent_id(("git-log-refs", commit.id.as_str()));
                    crate::log_refs::paint(
                        ui,
                        model.settings.language,
                        &p,
                        row,
                        badge_x,
                        &cluster,
                        id,
                    );
                }

                // 3. Author column (right-aligned before date)
                let author_rect = Rect::from_min_max(
                    Pos2::new(row.right() - date_w - author_w - 6.0, row.top()),
                    Pos2::new(row.right() - date_w - 6.0, row.bottom()),
                );
                text_clipped(
                    ui.painter(),
                    author_rect,
                    &commit.author,
                    FontId::proportional(12.0),
                    p.text2,
                    Align2::RIGHT_CENTER,
                );

                // 4. Date column
                let date_str = format_commit_datetime(commit.time);
                ui.painter().text(
                    Pos2::new(row.right() - 10.0, row.center().y),
                    Align2::RIGHT_CENTER,
                    date_str,
                    FontId::proportional(12.0),
                    p.text3,
                );

                let mut arrow_hit: Option<&lazecode_git::dag::FoldArrowMark> = None;
                if show_graph {
                    if let Some(brk) = dag.breaks.get(&commit.id) {
                        if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
                            let cell_rect =
                                Rect::from_min_size(row.min, Vec2::new(dag_width, row_h));
                            for arrow in brk.below.iter().chain(brk.above.iter()) {
                                let is_down =
                                    matches!(arrow.dir, lazecode_git::dag::ArrowDir::Down);
                                let hit = fold_arrow_hit(
                                    cell_rect.left(),
                                    row.top(),
                                    row_h,
                                    arrow.lane,
                                    is_down,
                                );
                                if hit.contains(pointer) {
                                    arrow_hit = Some(arrow);
                                    break;
                                }
                            }
                        }
                    }
                }

                if let Some(arrow) = arrow_hit {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    response.clone().show_tooltip_text(&arrow.tooltip);
                    if response.clicked() {
                        if arrow.pending {
                            model.projects[index].log_paging = true;
                            model.queue(Command::LogLoadMore);
                            model.ui.git_jump_to_commit = Some(arrow.peer_id.clone());
                        } else {
                            select_commit = Some(arrow.peer_id.clone());
                            jump_to = Some(arrow.peer_id.clone());
                        }
                    }
                } else if response.clicked() {
                    let before_selection = model.projects[index].selected_commits.clone();

                    let cmd_ctrl = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
                    let shift = ui.input(|i| i.modifiers.shift);
                    if cmd_ctrl {
                        let project = &mut model.projects[index];
                        let active = apply_log_click(
                            &filtered,
                            &mut project.selected_commits,
                            &mut project.selected_commit,
                            &commit.id,
                            true,
                        );
                        if active.is_some() {
                            select_commit = active;
                        }
                    } else if shift && !model.projects[index].selected_commits.is_empty() {
                        let anchor = model.projects[index].selected_commits[0].clone();
                        let pos_anchor = filtered.iter().position(|c| c.id == anchor).unwrap_or(0);
                        let pos_curr = filtered.iter().position(|c| c.id == commit.id).unwrap_or(0);
                        let start = pos_anchor.min(pos_curr);
                        let end = pos_anchor.max(pos_curr);
                        model.projects[index].selected_commits =
                            filtered[start..=end].iter().map(|c| c.id.clone()).collect();
                        select_commit = Some(commit.id.clone());
                    } else {
                        let project = &mut model.projects[index];
                        select_commit = apply_log_click(
                            &filtered,
                            &mut project.selected_commits,
                            &mut project.selected_commit,
                            &commit.id,
                            false,
                        );
                    }
                    // Cmd-clicking a non-active row can change the range
                    // without changing the active commit; reload either way.
                    if select_commit.is_none()
                        && model.projects[index].selected_commits != before_selection
                    {
                        select_commit = model.projects[index].selected_commit.clone();
                    }
                }
                if response.double_clicked() {
                    model.queue(Command::OpenCommitDiff {
                        commit: commit.id.clone(),
                        path: String::new(),
                    });
                }
                if response.secondary_clicked() {
                    let anchor = ui
                        .input(|i| i.pointer.hover_pos())
                        .map(|p| Rect::from_min_size(p, Vec2::splat(1.0)))
                        .unwrap_or(row);
                    if !model.projects[index].selected_commits.contains(&commit.id) {
                        model.projects[index].selected_commits = vec![commit.id.clone()];
                        select_commit = Some(commit.id.clone());
                    }
                    rt.open_menu(
                        crate::runtime::MenuKind::LogCommitMenu { project: index },
                        anchor,
                        Some(commit.id.clone()),
                    );
                }
            }
        });
    });
    if let Some(target) = jump_to {
        model.ui.git_jump_to_commit = Some(target);
        ui.ctx().request_repaint();
    }
    if let Some(id) = select_commit {
        model.projects[index].selected_commit = Some(id.clone());
        model.queue(Command::SelectCommit(id));
    }
    // Auto-paging: trigger next page automatically on scroll
    if request_more
        && !model.projects[index].log_paging
        && !model.projects[index].log_loading
        && !model.projects[index].log_reached_end
    {
        model.projects[index].log_paging = true;
        model.queue(Command::LogLoadMore);
        log::debug!("git log tail paged at {}", model.projects[index].log.len());
    }
    if model.projects[index].log_paging {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(33));
    }
    // Reuse the derived view across frames; the key covers the log
    // generation, the whole filter and the graph toggle.
    rt.git_log_cache = Some(GitLogCache {
        key: cache_key,
        filtered,
        dag,
        authors,
    });
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
mod log_click_tests {
    use super::*;

    fn commit(id: &str) -> CommitInfo {
        CommitInfo {
            id: id.to_string(),
            short: id.to_string(),
            message: id.to_string(),
            body: String::new(),
            author: "test".to_string(),
            email: "test@example.com".to_string(),
            time: 0,
            parents: Vec::new(),
            in_head: None,
            refs: Vec::new(),
            lane: 0,
            lane_count: 0,
            edges: Vec::new(),
        }
    }

    #[test]
    fn removing_the_active_commit_moves_the_marker_to_the_nearest_selection() {
        let visible = [commit("a"), commit("b"), commit("c"), commit("d")];
        let mut selected = vec!["a".to_string(), "c".to_string(), "d".to_string()];
        let mut active = Some("c".to_string());

        let reload = apply_log_click(&visible, &mut selected, &mut active, "c", true);

        assert_eq!(selected, ["a", "d"]);
        assert_eq!(active.as_deref(), Some("d"));
        assert_eq!(reload.as_deref(), Some("d"));
    }

    #[test]
    fn removing_an_inactive_commit_keeps_the_active_details() {
        let visible = [commit("c"), commit("b"), commit("a")];
        let mut selected = vec!["c".to_string(), "b".to_string()];
        let mut active = Some("c".to_string());

        let reload = apply_log_click(&visible, &mut selected, &mut active, "b", true);

        assert_eq!(selected, ["c"]);
        assert_eq!(active.as_deref(), Some("c"));
        assert_eq!(reload, None);
    }

    #[test]
    fn removing_the_only_selection_is_a_noop() {
        let visible = [commit("a")];
        let mut selected = vec!["a".to_string()];
        let mut active = Some("a".to_string());

        let reload = apply_log_click(&visible, &mut selected, &mut active, "a", true);

        assert_eq!(selected, ["a"]);
        assert_eq!(active.as_deref(), Some("a"));
        assert_eq!(reload, None);
    }

    #[test]
    fn command_click_adds_and_activates_a_commit() {
        let visible = [commit("c"), commit("b"), commit("a")];
        let mut selected = vec!["c".to_string()];
        let mut active = Some("c".to_string());

        let reload = apply_log_click(&visible, &mut selected, &mut active, "a", true);

        assert_eq!(selected, ["c", "a"]);
        assert_eq!(active.as_deref(), Some("a"));
        assert_eq!(reload.as_deref(), Some("a"));
    }

    #[test]
    fn plain_click_replaces_the_whole_selection() {
        let visible = [commit("c"), commit("b"), commit("a")];
        let mut selected = vec!["c".to_string(), "a".to_string()];
        let mut active = Some("a".to_string());

        let reload = apply_log_click(&visible, &mut selected, &mut active, "b", false);

        assert_eq!(selected, ["b"]);
        assert_eq!(active.as_deref(), Some("b"));
        assert_eq!(reload.as_deref(), Some("b"));
    }
}
