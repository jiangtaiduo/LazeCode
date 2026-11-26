//! Compare pane: toolbar, commit list, splitter and the details dock.

use super::prelude::*;
use super::{cb_filter_btn, cb_icon_btn, draw_details, filtered_commits};

const BANNER_H: f32 = 28.0;
const TOOLBAR_H: f32 = 30.0;
const ROW_H: f32 = 26.0;
pub(super) const DETAILS_HEAD_H: f32 = 26.0;
pub(super) const ROOT_ROW_H: f32 = 24.0;
pub(super) const FILE_ROW_H: f32 = 24.0;
const AUTHOR_W: f32 = 80.0;
const DATE_W: f32 = 108.0;
const RULER_GAP: f32 = 10.0;

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_pane(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    ref_name: &str,
    other_ref: &str,
    side: &lazecode_git::CompareSide,
    merge_base: &Option<String>,
    side_index: u8,
    now: i64,
) {
    let p = theme.palette;
    let zh = model.settings.language.is_zh();
    let side_index_us = usize::from(side_index);

    // ---- banner：28px 琥珀色提示条（rgba(227,179,65,0.1)）----
    let banner = Rect::from_min_size(rect.min, Vec2::new(rect.width(), BANNER_H));
    ui.painter()
        .rect_filled(banner, 0.0, p.amber.gamma_multiply(0.10));
    ui.painter().hline(
        banner.x_range(),
        banner.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(banner.left() + 16.0, banner.center().y),
            Vec2::splat(13.0),
        ),
        Icon::Alert,
        p.amber,
    );
    let banner_text = if zh {
        format!("存在于 {ref_name} 但不存在于 {other_ref} 的提交")
    } else {
        format!("Commits that exist in {ref_name} but don't exist in {other_ref}")
    };
    let text_right = if side_index == 0 {
        banner.right() - 100.0
    } else {
        banner.right() - 10.0
    };
    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(banner.left() + 28.0, banner.top()),
            Pos2::new(text_right, banner.bottom()),
        ),
        &banner_text,
        FontId::proportional(13.0),
        p.amber,
        Align2::LEFT_CENTER,
    );
    if side_index == 0 {
        // 稳定版 Swap 只出现在 left（上）pane：cb-swap-btn [⇄ Swap Branches]。
        let label = if zh {
            "⇄ 交换分支"
        } else {
            "⇄ Swap Branches"
        };
        let w = label.chars().count() as f32 * 6.0 + 18.0;
        let swap = Rect::from_min_size(
            Pos2::new(banner.right() - 8.0 - w, banner.center().y - 10.0),
            Vec2::new(w, 20.0),
        );
        let resp = ui.interact(swap, ui.scope_id().with("cb-swap"), Sense::click());
        if resp.hovered() {
            ui.painter()
                .rect_filled(swap, egui::CornerRadius::same(4), p.hover);
            ui.painter().rect_stroke(
                swap,
                egui::CornerRadius::same(4),
                Stroke::new(1.0, p.border_soft),
                StrokeKind::Inside,
            );
        }
        ui.painter().text(
            swap.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(12.5),
            if resp.hovered() {
                p.text_bright
            } else {
                p.text2
            },
        );
        if resp.clicked() {
            {
                let state = &mut model.ui.compare_branches;
                std::mem::swap(&mut state.left_ref, &mut state.right_ref);
                std::mem::swap(&mut state.left_path, &mut state.right_path);
                state.sides = None;
                state.error = None;
                // 换向后旧选择/详情全部作废（回填成功也会再清一次兜底）。
                for view in [&mut state.left_view, &mut state.right_view] {
                    view.selected.clear();
                    view.anchor = None;
                    view.select_all = false;
                    view.selected_file = None;
                    view.detail = None;
                }
                state.loading = true;
            }
            // 换向后同步标签页身份（标题/refs），下一帧整页重查。
            let (l, r, title) = {
                let state = &model.ui.compare_branches;
                let tab_title = if zh {
                    format!("比较：{} 和 {}", state.left_ref, state.right_ref)
                } else {
                    format!("Compare: {} and {}", state.left_ref, state.right_ref)
                };
                (state.left_ref.clone(), state.right_ref.clone(), tab_title)
            };
            model
                .active_project_mut()
                .unwrap()
                .open_compare_branches(l, r, title);
        }
    }

    // ---- 过滤工具行（30px，bg-app，搜索框内嵌 .*/Cc）----
    let toolbar = Rect::from_min_size(
        Pos2::new(rect.left(), banner.bottom()),
        Vec2::new(rect.width(), TOOLBAR_H),
    );
    ui.painter().rect_filled(toolbar, 0.0, p.bg);
    ui.painter().hline(
        toolbar.x_range(),
        toolbar.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );

    let view_ref = if side_index == 0 {
        &model.ui.compare_branches.left_view
    } else {
        &model.ui.compare_branches.right_view
    };
    let user_active = view_ref.author.is_some();
    let date_active = view_ref.date.is_some();
    let ref_active = view_ref.selected_ref_filter.is_some();
    let path_active = !view_ref.path.is_empty();
    let filter_count =
        usize::from(user_active) + usize::from(date_active) + usize::from(ref_active);
    let show_graph = rt.cb_show_graph[side_index_us];
    let has_side_data = !side.commits.is_empty() || !side.files.is_empty();
    let _ = has_side_data;
    let _ = view_ref;

    // 右组：refresh 20 + graph 20（margin-left auto 语义，从右往左布）。
    let mut rx = toolbar.right() - 6.0;
    let refresh_rect =
        Rect::from_min_size(Pos2::new(rx - 20.0, toolbar.top() + 5.0), Vec2::splat(20.0));
    rx -= 22.0;
    let graph_rect =
        Rect::from_min_size(Pos2::new(rx - 20.0, toolbar.top() + 5.0), Vec2::splat(20.0));
    rx -= 20.0;
    let clear_rect =
        Rect::from_min_size(Pos2::new(rx - 18.0, toolbar.top() + 6.0), Vec2::splat(18.0));
    let clear_w = if filter_count > 0 { 20.0 } else { 0.0 };
    // 从右往左的按钮组（先画右侧固定组）。
    if cb_icon_btn(
        ui,
        refresh_rect,
        ("cb-refresh", side_index),
        Icon::Refresh,
        false,
        true,
    ) {
        let state = &mut model.ui.compare_branches;
        state.loading = true;
        let (l, r, lp, rp) = (
            state.left_ref.clone(),
            state.right_ref.clone(),
            state.left_path.clone(),
            state.right_path.clone(),
        );
        model.queue(Command::CompareBranches {
            left: l,
            right: r,
            left_path: lp,
            right_path: rp,
        });
        return;
    }
    if cb_icon_btn(
        ui,
        graph_rect,
        ("cb-graph", side_index),
        Icon::GitLog,
        show_graph,
        true,
    ) {
        rt.cb_show_graph[side_index_us] = !show_graph;
    }
    let lx_left = toolbar.left() + 6.0;
    if filter_count > 0
        && cb_icon_btn(
            ui,
            clear_rect,
            ("cb-clear", side_index),
            Icon::X,
            false,
            true,
        )
    {
        // 稳定版 Clear 只清 author/date/ref/path，不动搜索词。
        let view = if side_index == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.author = None;
        view.date = None;
        view.selected_ref_filter = None;
        if !view.path.is_empty() {
            view.path.clear();
            state_reload(model);
            return;
        }
    }
    // Paths / User / Date / ⑂（从右往左排）。
    let paths_label = crate::tr!(model, "Paths", "路径");
    let user_label = crate::tr!(model, "User", "用户");
    let date_label = crate::tr!(model, "Date", "日期");
    let btn_w = |label: &str| label.chars().count() as f32 * 7.2 + 18.0;
    let paths_w = btn_w(paths_label);
    let user_w = btn_w(user_label);
    let date_w = btn_w(date_label);
    let ref_w = 26.0;
    let mut fx = rx - clear_w - 2.0;
    let paths_rect = Rect::from_min_size(
        Pos2::new(fx - paths_w, toolbar.top() + 3.0),
        Vec2::new(paths_w, 24.0),
    );
    fx -= paths_w + 3.0;
    let date_rect = Rect::from_min_size(
        Pos2::new(fx - date_w, toolbar.top() + 3.0),
        Vec2::new(date_w, 24.0),
    );
    fx -= date_w + 3.0;
    let user_rect = Rect::from_min_size(
        Pos2::new(fx - user_w, toolbar.top() + 3.0),
        Vec2::new(user_w, 24.0),
    );
    fx -= user_w + 3.0;
    let ref_rect = Rect::from_min_size(
        Pos2::new(fx - ref_w, toolbar.top() + 3.0),
        Vec2::new(ref_w, 24.0),
    );
    let search_right = ref_rect.left() - 3.0;

    if cb_filter_btn(
        ui,
        paths_rect,
        ("cb-paths", side_index),
        paths_label,
        path_active,
    ) {
        let view = if side_index == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.path_edit_open = !view.path_edit_open;
        view.path_edit = view.path.clone();
    }
    if cb_filter_btn(
        ui,
        user_rect,
        ("cb-user", side_index),
        user_label,
        user_active,
    ) {
        rt.open_menu(
            crate::runtime::MenuKind::CompareUserFilter { side: side_index },
            user_rect,
            None,
        );
    }
    if cb_filter_btn(
        ui,
        date_rect,
        ("cb-date", side_index),
        date_label,
        date_active,
    ) {
        rt.open_menu(
            crate::runtime::MenuKind::CompareDateFilter { side: side_index },
            date_rect,
            None,
        );
    }
    // Ref 过滤：仅图标按钮（GitBranchIcon 11）。
    if cb_icon_btn(
        ui,
        ref_rect,
        ("cb-ref", side_index),
        Icon::Branch,
        ref_active,
        true,
    ) {
        rt.open_menu(
            crate::runtime::MenuKind::CompareRefFilter { side: side_index },
            ref_rect,
            None,
        );
    }

    // 搜索框（cb-search-box：flex 1 1 120px，h22，内嵌 Q▾ + .*/Cc）。
    let search_rect = Rect::from_min_max(
        Pos2::new(lx_left, toolbar.top() + 4.0),
        Pos2::new(search_right, toolbar.bottom() - 4.0),
    );
    if search_rect.width() > 60.0 {
        ui.painter()
            .rect_filled(search_rect, egui::CornerRadius::same(4), p.hairline);
        paint_icon(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(search_rect.left() + 10.0, search_rect.center().y),
                Vec2::splat(12.0),
            ),
            Icon::Search,
            p.text3,
        );
        let toggle_w = 22.0;
        let mut query = {
            let view = if side_index == 0 {
                &model.ui.compare_branches.left_view
            } else {
                &model.ui.compare_branches.right_view
            };
            view.query.clone()
        };
        let input = Rect::from_min_max(
            Pos2::new(search_rect.left() + 20.0, search_rect.top() + 1.0),
            Pos2::new(
                search_rect.right() - toggle_w * 2.0 - 4.0,
                search_rect.bottom() - 1.0,
            ),
        );
        let resp = ui.put(
            input,
            TextEdit::singleline(&mut query)
                .frame(egui::Frame::NONE)
                .align(egui::Align2::LEFT_CENTER)
                .hint_text(crate::tr!(model, "Text or hash", "搜索提交、哈希或作者"))
                .font(FontId::proportional(12.5)),
        );
        if resp.changed() {
            let view = if side_index == 0 {
                &mut model.ui.compare_branches.left_view
            } else {
                &mut model.ui.compare_branches.right_view
            };
            view.query = query;
        }
        let (use_regex, match_case) = {
            let view = if side_index == 0 {
                &model.ui.compare_branches.left_view
            } else {
                &model.ui.compare_branches.right_view
            };
            (view.regex, view.match_case)
        };
        let regex_rect = Rect::from_min_size(
            Pos2::new(
                search_rect.right() - toggle_w * 2.0 - 2.0,
                search_rect.center().y - 9.0,
            ),
            Vec2::new(toggle_w, 18.0),
        );
        let case_rect = Rect::from_min_size(
            Pos2::new(
                search_rect.right() - toggle_w - 2.0,
                search_rect.center().y - 9.0,
            ),
            Vec2::new(toggle_w, 18.0),
        );
        let _ = has_side_data;
        if cb_toggle(ui, regex_rect, ("cb-regex", side_index), ".*", use_regex) {
            let view = if side_index == 0 {
                &mut model.ui.compare_branches.left_view
            } else {
                &mut model.ui.compare_branches.right_view
            };
            view.regex = !view.regex;
        }
        if cb_toggle(ui, case_rect, ("cb-case", side_index), "Cc", match_case) {
            let view = if side_index == 0 {
                &mut model.ui.compare_branches.left_view
            } else {
                &mut model.ui.compare_branches.right_view
            };
            view.match_case = !view.match_case;
        }
    }

    // ---- 过滤后的可见提交（搜索/作者/日期/引用过滤，保持显示顺序）----
    let visible: &[usize] = {
        let view = if side_index == 0 {
            &model.ui.compare_branches.left_view
        } else {
            &model.ui.compare_branches.right_view
        };
        filtered_commits(
            &side.commits,
            view,
            now,
            &mut rt.compare_filter_cache[side_index_us],
        )
    };

    let body = Rect::from_min_max(Pos2::new(rect.left(), toolbar.bottom()), rect.max);

    // ---- [提交日志 | 详情列]（详情宽两侧共用、可拖，稳定版 24-62%）----
    let details_frac = (model.settings.compare_details_width / 100.0).clamp(0.24, 0.62);
    let details_w = (body.width() * details_frac).max(170.0);
    let list = Rect::from_min_max(body.min, Pos2::new(body.right() - details_w, body.bottom()));
    let details = Rect::from_min_max(Pos2::new(list.right() + 1.0, body.min.y), body.max);
    let divider = Rect::from_min_size(
        Pos2::new(list.right() - 4.0, body.min.y),
        Vec2::new(8.0, body.height()),
    );

    // 提交日志列（无表头；空态居中偏上提示）。
    let filter_active = {
        let view = if side_index == 0 {
            &model.ui.compare_branches.left_view
        } else {
            &model.ui.compare_branches.right_view
        };
        !view.query.trim().is_empty()
            || view.author.is_some()
            || view.date.is_some()
            || view.selected_ref_filter.is_some()
    };
    let empty_reason: String = if filter_active {
        crate::tr!(model, "No matching commits", "没有匹配的提交").to_string()
    } else if zh {
        format!("{ref_name} 没有独有提交")
    } else {
        format!("No commits unique to {ref_name}")
    };
    let (selected, anchor) = {
        let view = if side_index == 0 {
            &model.ui.compare_branches.left_view
        } else {
            &model.ui.compare_branches.right_view
        };
        (view.selected.clone(), view.anchor.clone())
    };

    let mut click: Option<(String, bool, bool)> = None;
    let mut double_click: Option<String> = None;
    let footer_row = side.truncated;
    let row_total = visible.len() + usize::from(footer_row);
    let show_graph = rt.cb_show_graph[side_index_us];
    // 稳定版每 pane 独立 graphLayout（computeGraphLayout）。
    let dag = if show_graph {
        lazecode_git::compute_dag(&side.commits)
    } else {
        lazecode_git::DagLayout::default()
    };
    crate::app_ui::region(ui, list, "cb-list", |ui| {
        if visible.is_empty() {
            ui.painter().text(
                Pos2::new(list.left() + 16.0, list.top() + 30.0),
                Align2::LEFT_CENTER,
                empty_reason,
                FontId::proportional(11.0),
                p.text3,
            );
        }
        egui::ScrollArea::vertical()
            .id_salt(("cb-list", side_index))
            .auto_shrink([false, false])
            .show_rows(ui, ROW_H, row_total.max(1), |ui, range| {
                for row_index in range {
                    if row_index == visible.len() && footer_row {
                        let (row, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), ROW_H),
                            Sense::hover(),
                        );
                        // cb-load-more-wrap：居中按钮 + 截断提示。
                        let loading_more = model.ui.compare_branches.loading_more[side_index_us];
                        let more_label = if loading_more {
                            crate::tr!(model, "Loading…", "加载中…").to_string()
                        } else {
                            crate::tr!(model, "Load more", "加载更多").to_string()
                        };
                        let more_w = more_label.chars().count() as f32 * 6.2 + 24.0;
                        let note = if zh {
                            format!("仅显示前 {} 个提交", side.commits.len())
                        } else {
                            format!("Showing the first {} commits", side.commits.len())
                        };
                        let total_w = more_w + 8.0 + note.chars().count() as f32 * 5.6;
                        let mut bx = row.center().x - total_w / 2.0;
                        let more = Rect::from_min_size(
                            Pos2::new(bx, row.center().y - 11.0),
                            Vec2::new(more_w, 22.0),
                        );
                        bx += more_w + 8.0;
                        if !loading_more {
                            let resp = ui.interact(
                                more,
                                ui.scope_id().with(("cb-more", side_index)),
                                Sense::click(),
                            );
                            ui.painter().rect_stroke(
                                more,
                                egui::CornerRadius::same(4),
                                Stroke::new(1.0, p.border_soft),
                                StrokeKind::Inside,
                            );
                            if resp.hovered() {
                                ui.painter().rect_filled(
                                    more,
                                    egui::CornerRadius::same(4),
                                    p.hover,
                                );
                            }
                            ui.painter().text(
                                more.center(),
                                Align2::CENTER_CENTER,
                                more_label,
                                FontId::proportional(11.0),
                                if resp.hovered() {
                                    p.text_bright
                                } else {
                                    p.text2
                                },
                            );
                            if resp.clicked() {
                                let path = {
                                    let state = &mut model.ui.compare_branches;
                                    state.loading_more[side_index_us] = true;
                                    if side_index == 0 {
                                        state.left_path.clone()
                                    } else {
                                        state.right_path.clone()
                                    }
                                };
                                model.queue(Command::CompareBranchesMore {
                                    side: side_index,
                                    exclude: other_ref.to_string(),
                                    include: ref_name.to_string(),
                                    limit: side.commits.len() + 500,
                                    path,
                                });
                            }
                        } else {
                            ui.painter().text(
                                more.center(),
                                Align2::CENTER_CENTER,
                                more_label,
                                FontId::proportional(11.0),
                                p.text3,
                            );
                        }
                        ui.painter().text(
                            Pos2::new(bx, row.center().y),
                            Align2::LEFT_CENTER,
                            note,
                            FontId::proportional(10.0),
                            p.text3,
                        );
                        continue;
                    }
                    let Some(commit_index) = visible.get(row_index) else {
                        continue;
                    };
                    let commit = &side.commits[*commit_index];
                    let (row, response) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), ROW_H),
                        Sense::click(),
                    );
                    let is_selected = selected.contains(&commit.id);
                    if is_selected {
                        // .log-row.active → #26415f。
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::ZERO, p.selected_row);
                    } else if response.hovered() {
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::ZERO, p.hover);
                    }
                    ui.painter().hline(
                        row.x_range(),
                        row.bottom() - 0.5,
                        Stroke::new(1.0, p.hairline),
                    );

                    // DAG cell（26px，与行高等高）。
                    if show_graph {
                        let cell_rect = Rect::from_min_size(row.min, Vec2::new(36.0, ROW_H));
                        crate::panel::draw_dag_cell(
                            ui.painter(),
                            cell_rect,
                            dag.rows.get(&commit.id),
                            dag.breaks.get(&commit.id),
                            commit,
                        );
                    }

                    // 主题 + merge 图标 + 引用章。
                    let mut msg_x = row.left() + RULER_GAP + if show_graph { 30.0 } else { 0.0 };
                    if commit.parents.len() > 1 {
                        paint_icon(
                            ui.painter(),
                            Rect::from_center_size(
                                Pos2::new(msg_x + 6.0, row.center().y),
                                Vec2::splat(12.0),
                            ),
                            Icon::Merge,
                            p.text3,
                        );
                        msg_x += 16.0;
                    }
                    let content_right = (row.right() - AUTHOR_W - DATE_W - 12.0).max(msg_x + 60.0);
                    let refs_max_w = ((content_right - msg_x) / 3.0).clamp(24.0, 240.0);
                    let refs_cluster =
                        crate::log_refs::prepare(ui.painter(), &commit.refs, refs_max_w);
                    let refs_w = refs_cluster
                        .as_ref()
                        .map(|cluster| cluster.width + 6.0)
                        .unwrap_or(0.0);
                    let subject_right = (content_right - refs_w).max(msg_x + 60.0);
                    let subject_font = FontId::proportional(13.0);
                    let subject_galley = ui.painter().layout_no_wrap(
                        commit.message.clone(),
                        subject_font.clone(),
                        p.text,
                    );
                    let actual_subject_w = subject_galley.size().x.min(subject_right - msg_x);
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(msg_x, row.top()),
                            Pos2::new(msg_x + actual_subject_w, row.bottom()),
                        ),
                        &commit.message,
                        subject_font,
                        p.text,
                        Align2::LEFT_CENTER,
                    );
                    if let Some(cluster) = refs_cluster {
                        let badge_x = msg_x + actual_subject_w + 6.0;
                        let id = ui.make_persistent_id(("compare-log-refs", commit.id.as_str()));
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
                    // 作者（76px 右对齐）+ 日期（104px 右对齐）。
                    let author_rect = Rect::from_min_max(
                        Pos2::new(row.right() - AUTHOR_W - DATE_W, row.top()),
                        Pos2::new(row.right() - DATE_W, row.bottom()),
                    );
                    text_clipped(
                        ui.painter(),
                        author_rect,
                        &commit.author,
                        FontId::proportional(12.0),
                        p.text2,
                        Align2::RIGHT_CENTER,
                    );
                    let date_str = crate::panel::format_commit_datetime(commit.time);
                    ui.painter().text(
                        Pos2::new(row.right() - 10.0, row.center().y),
                        Align2::RIGHT_CENTER,
                        date_str,
                        FontId::proportional(12.0),
                        p.text3,
                    );

                    // 选择（单击/Shift 范围/⌘ 多选）+ 双击开整提交 diff。
                    if response.clicked() {
                        let cmd = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
                        let shift = ui.input(|i| i.modifiers.shift);
                        if shift && anchor.is_some() {
                            click = Some((commit.id.clone(), true, cmd));
                        } else {
                            click = Some((commit.id.clone(), false, cmd));
                        }
                    }
                    if response.double_clicked() {
                        double_click = Some(commit.id.clone());
                    }
                }
            });
    });

    // ---- 选择落地（对齐稳定版 handleCommitClick）----
    if let Some((hash, shift, cmd)) = click {
        let state = &mut model.ui.compare_branches;
        let view = if side_index == 0 {
            &mut state.left_view
        } else {
            &mut state.right_view
        };
        view.select_all = false;
        if shift && view.anchor.is_some() {
            let anchor_id = view.anchor.clone().unwrap_or_default();
            let pos_anchor = visible
                .iter()
                .position(|index| side.commits[*index].id == anchor_id);
            let pos_curr = visible
                .iter()
                .position(|index| side.commits[*index].id == hash);
            if let (Some(a), Some(c)) = (pos_anchor, pos_curr) {
                let (lo, hi) = if a < c { (a, c) } else { (c, a) };
                view.selected = visible[lo..=hi]
                    .iter()
                    .map(|index| side.commits[*index].id.clone())
                    .collect();
            } else {
                view.selected = vec![hash.clone()];
            }
            view.anchor = Some(hash);
        } else if cmd {
            if let Some(pos) = view.selected.iter().position(|id| id == &hash) {
                if view.selected.len() > 1 {
                    view.selected.remove(pos);
                }
            } else {
                view.selected.push(hash.clone());
            }
            view.anchor = Some(hash);
        } else {
            view.selected = vec![hash.clone()];
            view.anchor = Some(hash);
        }
        view.selected_file = None;
        view.detail = None;
    }
    if let Some(id) = double_click {
        model.queue(Command::OpenCommitDiff {
            commit: id,
            path: String::new(),
        });
    }

    // 详情文件加载触发（对齐稳定版：选择即加载）。
    {
        let state = &mut model.ui.compare_branches;
        let view = if side_index == 0 {
            &mut state.left_view
        } else {
            &mut state.right_view
        };
        if view.select_all {
            view.detail = None;
        } else {
            let key = if view.selected.len() == 1 {
                format!("c:{}", view.selected[0])
            } else if view.selected.len() >= 2 {
                let positions: Vec<usize> = view
                    .selected
                    .iter()
                    .filter_map(|id| side.commits.iter().position(|commit| &commit.id == id))
                    .collect();
                match (positions.iter().min(), positions.iter().max()) {
                    (Some(newest), Some(oldest)) => {
                        const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
                        let from = side.commits[*oldest]
                            .parents
                            .first()
                            .cloned()
                            .unwrap_or_else(|| EMPTY_TREE.to_string());
                        format!("r:{from}..{}", side.commits[*newest].id)
                    }
                    _ => String::new(),
                }
            } else {
                String::new()
            };
            if !key.is_empty() {
                // key 变化即重查（含加载中切换提交的场景）：旧请求的结果
                // 回填时按 key 匹配会被丢弃，不会污染新选择。
                let mismatch = match &view.detail {
                    Some(detail) => detail.key != key,
                    None => true,
                };
                if mismatch {
                    view.detail = Some(lazecode_core::CompareDetail {
                        key: key.clone(),
                        loading: true,
                        ..Default::default()
                    });
                    let (commit, from, to) = if let Some(commit) = key.strip_prefix("c:") {
                        (commit.to_string(), String::new(), String::new())
                    } else {
                        let range = key.trim_start_matches("r:").to_string();
                        let (from, to) = range.split_once("..").unwrap_or(("", ""));
                        (String::new(), from.to_string(), to.to_string())
                    };
                    model.queue(Command::CompareFiles {
                        key,
                        commit,
                        from,
                        to,
                    });
                }
            }
        }
    }

    // ---- 详情列 ----
    let (select_all, collapsed, selected_file) = {
        let state = &model.ui.compare_branches;
        let view = if side_index == 0 {
            &state.left_view
        } else {
            &state.right_view
        };
        (
            view.select_all,
            view.collapsed_dirs.clone(),
            view.selected_file.clone(),
        )
    };
    draw_details(
        ui,
        model,
        rt,
        theme,
        details,
        ref_name,
        other_ref,
        side,
        merge_base,
        side_index,
        &selected,
        select_all,
        &collapsed,
        &selected_file,
    );

    // 详情列宽度分隔条（8px strip，hover 时左缘 2px accent 线）。
    let drag = ui.interact(
        divider,
        ui.scope_id().with(("cb-details", side_index)),
        Sense::drag(),
    );
    let hovered = drag.hovered() || drag.dragged();
    if hovered {
        ui.painter().vline(
            divider.left() + 3.0,
            divider.y_range(),
            Stroke::new(2.0, p.accent),
        );
    }
    if drag.dragged() {
        model.settings.compare_details_width = (model.settings.compare_details_width
            - drag.drag_delta().x / body.width() * 100.0)
            .clamp(24.0, 62.0);
        rt.compare_details_drag = true;
    } else if rt.compare_details_drag {
        rt.compare_details_drag = false;
        model.save_settings();
    }

    // 路径过滤浮层（cb-path-form：置于 draw_pane 终点绘制，确保层级绝对置顶，绝不被提交列表与文件列表遮盖穿透）。
    let path_open = {
        let view = if side_index == 0 {
            &model.ui.compare_branches.left_view
        } else {
            &model.ui.compare_branches.right_view
        };
        view.path_edit_open
    };
    if path_open {
        let form_w = 260.0;
        let form = Rect::from_min_size(
            Pos2::new(
                paths_rect.left().min(toolbar.right() - form_w - 4.0),
                toolbar.bottom() + 2.0,
            ),
            Vec2::new(form_w, 82.0),
        );
        crate::overlays::menu_frame(ui, form, theme);
        rt.menu_hit_rects.push(form);
        let input = Rect::from_min_size(
            Pos2::new(form.left() + 8.0, form.top() + 8.0),
            Vec2::new(form_w - 16.0, 26.0),
        );
        ui.painter()
            .rect_filled(input, egui::CornerRadius::same(4), p.scrim_subtle);
        ui.painter().rect_stroke(
            input,
            egui::CornerRadius::same(4),
            Stroke::new(1.0, p.border_soft),
            StrokeKind::Inside,
        );
        let mut edit = {
            let view = if side_index == 0 {
                &model.ui.compare_branches.left_view
            } else {
                &model.ui.compare_branches.right_view
            };
            view.path_edit.clone()
        };
        let resp = ui.put(
            input.shrink2(Vec2::new(6.0, 2.0)),
            TextEdit::singleline(&mut edit)
                .frame(egui::Frame::NONE)
                .align(egui::Align2::LEFT_CENTER)
                .hint_text(crate::tr!(model, "path", "路径"))
                .font(FontId::proportional(12.5)),
        );
        if resp.changed() {
            let view = if side_index == 0 {
                &mut model.ui.compare_branches.left_view
            } else {
                &mut model.ui.compare_branches.right_view
            };
            view.path_edit = edit.clone();
        }
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let view = if side_index == 0 {
                &mut model.ui.compare_branches.left_view
            } else {
                &mut model.ui.compare_branches.right_view
            };
            view.path = view.path_edit.trim().to_string();
            view.path_edit_open = false;
            state_reload(model);
            return;
        }
        let apply = Rect::from_min_size(
            Pos2::new(form.right() - 128.0, form.bottom() - 32.0),
            Vec2::new(56.0, 24.0),
        );
        let clear = Rect::from_min_size(
            Pos2::new(form.right() - 64.0, form.bottom() - 32.0),
            Vec2::new(56.0, 24.0),
        );
        if secondary_button(ui, apply, crate::tr!(model, "Apply", "应用"), None, theme).clicked()
        {
            let view = if side_index == 0 {
                &mut model.ui.compare_branches.left_view
            } else {
                &mut model.ui.compare_branches.right_view
            };
            view.path = view.path_edit.trim().to_string();
            view.path_edit_open = false;
            state_reload(model);
            return;
        }
        if secondary_button(ui, clear, crate::tr!(model, "Clear", "清除"), None, theme).clicked()
        {
            let view = if side_index == 0 {
                &mut model.ui.compare_branches.left_view
            } else {
                &mut model.ui.compare_branches.right_view
            };
            view.path_edit.clear();
            view.path.clear();
            view.path_edit_open = false;
            state_reload(model);
            return;
        }
        let clicked_outside = ui.input(|i| {
            i.pointer.any_click()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|pos| !form.contains(pos) && !paths_rect.contains(pos))
        });
        if clicked_outside || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            let view = if side_index == 0 {
                &mut model.ui.compare_branches.left_view
            } else {
                &mut model.ui.compare_branches.right_view
            };
            view.path_edit_open = false;
        }
    }
}

/// 过滤条件变化后的整页重查（稳定版 onPathFilterChange 触发重载）。
fn state_reload(model: &mut AppModel) {
    let state = &mut model.ui.compare_branches;
    state.loading = true;
    let (l, r, lp, rp) = (
        state.left_ref.clone(),
        state.right_ref.clone(),
        state.left_path.clone(),
        state.right_path.clone(),
    );
    model.queue(Command::CompareBranches {
        left: l,
        right: r,
        left_path: lp,
        right_path: rp,
    });
}

/// h18 内嵌小开关（cb-toggle：.* / Cc，active accent-soft+蓝）。
pub(super) fn cb_toggle(
    ui: &mut Ui,
    rect: Rect,
    key: (&'static str, u8),
    label: &str,
    active: bool,
) -> bool {
    let p = crate::theme::Palette::dark();
    let response = ui.interact(rect, ui.scope_id().with(key), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(3), p.hover);
    } else if active {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(3), p.accent_soft);
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::monospace(10.0),
        if active {
            p.blue
        } else if response.hovered() {
            p.text_bright
        } else {
            p.text3
        },
    );
    response.clicked()
}
