use std::collections::HashMap;

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, TextEdit, Ui, Vec2};
use lazecode_core::{AppModel, Command, PanelTab, TabKind};
use lazecode_git::{CommitInfo, RefKind};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::UiRuntime;
use crate::theme::Theme;
use crate::widgets::{icon_button, live_dot, text_clipped};

const HEAD_H: f32 = 33.0;

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.panel);
    let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), HEAD_H));
    ui.painter().rect_filled(head, 0.0, p.panel);
    ui.painter().hline(
        head.x_range(),
        head.bottom() - 0.5,
        Stroke::new(1.0, p.border),
    );
    let body = Rect::from_min_max(Pos2::new(rect.left(), head.bottom()), rect.max);
    match model.ui.panel_tab {
        PanelTab::Console => draw_console(ui, model, rt, theme, head, body),
        PanelTab::Git => draw_git_panel(ui, model, rt, theme, head, body),
    }
}

fn draw_console(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    head: Rect,
    body: Rect,
) {
    let p = theme.palette;
    let Some(index) = model.active_project else {
        draw_empty(ui, theme, body, "No project open");
        return;
    };
    let (count, tabs, active, running) = {
        let project = &model.projects[index];
        let tabs: Vec<(u64, String, bool, bool)> = project
            .terminals
            .iter()
            .map(|terminal| {
                (
                    terminal.id,
                    terminal.title.clone(),
                    terminal.session.is_running(),
                    project.active_terminal == Some(terminal.id),
                )
            })
            .collect();
        (
            project.terminals.len(),
            tabs,
            project.active_terminal,
            project
                .terminals
                .iter()
                .filter(|terminal| terminal.session.is_running())
                .count(),
        )
    };
    // Dock identity.
    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(head.left() + 16.0, head.center().y),
            Vec2::splat(14.0),
        ),
        Icon::Terminal,
        p.text2,
    );
    ui.painter().text(
        Pos2::new(head.left() + 28.0, head.center().y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Consoles", "控制台"),
        FontId::proportional(11.0),
        p.text2,
    );
    if count > 0 {
        let badge = Rect::from_center_size(
            Pos2::new(head.left() + 86.0, head.center().y),
            Vec2::new(18.0, 14.0),
        );
        ui.painter()
            .rect_filled(badge, egui::CornerRadius::same(7), p.panel2);
        ui.painter().text(
            badge.center(),
            Align2::CENTER_CENTER,
            count.to_string(),
            FontId::monospace(8.5),
            p.text3,
        );
    }
    if running > 0 {
        live_dot(
            ui.painter(),
            Pos2::new(head.left() + 106.0, head.center().y),
            true,
            p.running,
        );
    }

    // Right actions.
    let mut rx = head.right() - 6.0;
    let close_rect = Rect::from_min_size(Pos2::new(rx - 24.0, head.top() + 4.0), Vec2::splat(24.0));
    if icon_button(ui, close_rect, Icon::X, false, theme).clicked() {
        model.ui.panel_visible = false;
    }
    rx -= 26.0;
    let active_state = active.and_then(|id| model.projects[index].terminal_by_id(id));
    if let Some(terminal) = active_state {
        let action_rect =
            Rect::from_min_size(Pos2::new(rx - 24.0, head.top() + 4.0), Vec2::splat(24.0));
        let running = terminal.session.is_running();
        if icon_button(
            ui,
            action_rect,
            if running { Icon::Stop } else { Icon::Play },
            false,
            theme,
        )
        .clicked()
        {
            if running {
                terminal.session.kill();
            } else if let Some(id) = active {
                model.queue(Command::RestartTerminal(id));
            }
        }
        rx -= 26.0;
    }
    let max_rect = Rect::from_min_size(Pos2::new(rx - 24.0, head.top() + 4.0), Vec2::splat(24.0));
    let max_icon = if model.ui.panel_maximised {
        Icon::Minimize
    } else {
        Icon::Maximize
    };
    if icon_button(ui, max_rect, max_icon, model.ui.panel_maximised, theme).clicked() {
        model.ui.panel_maximised = !model.ui.panel_maximised;
    }
    rx -= 26.0;
    let spark_rect = Rect::from_min_size(Pos2::new(rx - 24.0, head.top() + 4.0), Vec2::splat(24.0));
    if icon_button(ui, spark_rect, Icon::Spark, false, theme).clicked() {
        rt.agent_menu = Some(spark_rect);
    }
    rx -= 26.0;
    let search_rect =
        Rect::from_min_size(Pos2::new(rx - 24.0, head.top() + 4.0), Vec2::splat(24.0));
    let searching = active
        .and_then(|id| rt.terminal_search.get(&id))
        .is_some_and(|state| state.open);
    if icon_button(ui, search_rect, Icon::Search, searching, theme).clicked() {
        if let Some(id) = active {
            let state = rt.terminal_search.entry(id).or_default();
            state.open = !state.open;
            state.focus_requested = state.open;
        }
    }
    rx -= 4.0;

    // Console tabs.
    let mut x = head.left() + 118.0;
    let available = (rx - x).max(0.0);
    let mut close_terminal = None;
    for (id, title, is_running, is_active) in &tabs {
        let width = (title.chars().count() as f32 * 6.2 + 54.0).clamp(96.0, 178.0);
        if width > available || x + width > rx {
            break;
        }
        let tab = Rect::from_min_size(Pos2::new(x, head.top() + 4.0), Vec2::new(width, 25.0));
        let body_rect = Rect::from_min_max(tab.min, Pos2::new(tab.right() - 20.0, tab.bottom()));
        let close_rect_inner =
            Rect::from_min_max(Pos2::new(tab.right() - 20.0, tab.top()), tab.max);
        let response = ui.interact(body_rect, ui.scope_id().with(("ctab", id)), Sense::click());
        let close_response = ui.interact(
            close_rect_inner,
            ui.scope_id().with(("ctab-close", id)),
            Sense::click(),
        );
        if *is_active {
            ui.painter()
                .rect_filled(tab, egui::CornerRadius::same(5), p.active);
        } else if response.hovered() || close_response.hovered() {
            ui.painter()
                .rect_filled(tab, egui::CornerRadius::same(5), p.hover);
        }
        live_dot(
            ui.painter(),
            Pos2::new(tab.left() + 12.0, tab.center().y),
            *is_running,
            if *is_running { p.running } else { p.text3 },
        );
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(tab.left() + 20.0, tab.top()),
                Pos2::new(tab.right() - 22.0, tab.bottom()),
            ),
            title,
            FontId::proportional(10.5),
            if *is_active { p.text } else { p.text2 },
            Align2::LEFT_CENTER,
        );
        if close_response.hovered() || *is_active {
            paint_icon(
                ui.painter(),
                Rect::from_center_size(close_rect_inner.center(), Vec2::splat(10.0)),
                Icon::X,
                if close_response.hovered() {
                    p.text
                } else {
                    p.text3
                },
            );
        }
        if close_response.clicked() {
            close_terminal = Some(*id);
        } else if response.clicked() {
            model.projects[index].active_terminal = Some(*id);
            rt.set_terminal_focus(*id);
            model.ui.panel_tab = PanelTab::Console;
        }
        x = tab.right() + 2.0;
    }
    // Plus button.
    if x + 22.0 < rx {
        let plus = Rect::from_center_size(Pos2::new(x + 11.0, head.center().y), Vec2::splat(22.0));
        let response = ui.interact(plus, ui.scope_id().with("new-console"), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(plus, egui::CornerRadius::same(5), p.hover);
        }
        paint_icon(ui.painter(), plus.shrink(5.0), Icon::Plus, p.text3);
        if response.clicked() {
            model.queue(Command::NewTerminal);
        }
    }
    if let Some(id) = close_terminal {
        model.projects[index].close_terminal(id);
        rt.terminal_snapshots.remove(&id);
        rt.terminal_selections.remove(&id);
        rt.terminal_search.remove(&id);
        if rt.terminal_focus == Some(id) {
            rt.terminal_focus = None;
        }
    }

    // Body.
    let active = model.projects[index].active_terminal;
    let Some(id) = active else {
        draw_empty(ui, theme, body, "No consoles open");
        return;
    };
    let Some(terminal) = model.projects[index].terminal_by_id(id) else {
        draw_empty(ui, theme, body, "No consoles open");
        return;
    };
    let session = &terminal.session;
    // Overlays own the keyboard; don't feed their keystrokes to the shell.
    let overlays_open = rt.settings_open
        || rt.about_open
        || rt.search.open
        || model.ui.command_palette_open
        || rt.menu.is_some()
        || rt.agent_menu.is_some()
        || !matches!(rt.dialog, crate::runtime::DialogState::None);
    let focus = rt.terminal_focus == Some(id) && !overlays_open;
    let font_size = model.settings.terminal.font_size;
    let rt_ref = &mut *rt;
    let snapshot = rt_ref.terminal_snapshots.entry(id).or_default();
    let selection = rt_ref.terminal_selections.entry(id).or_insert(None);
    let search = rt_ref.terminal_search.entry(id).or_default();
    let preedit = rt_ref.terminal_preedit.entry(id).or_default();
    let output = crate::terminal::draw(
        ui, session, snapshot, selection, search, focus, preedit, theme, body, id, font_size,
    );
    if output.focus_requested {
        rt.set_terminal_focus(id);
    }
    // Keep tab titles in sync with OSC titles.
    if let Some(title) = session.title() {
        if let Some(terminal) = model.projects[index].terminal_by_id_mut(id) {
            if !title.is_empty() && terminal.title != title {
                terminal.title = title;
            }
        }
    }
    // Resize is driven by the terminal view; nothing else to do here.
}

fn draw_empty(ui: &mut Ui, theme: &Theme, rect: Rect, text: &str) {
    let p = theme.palette;
    let center = rect.center();
    paint_icon(
        ui.painter(),
        Rect::from_center_size(Pos2::new(center.x, center.y - 18.0), Vec2::splat(22.0)),
        Icon::Terminal,
        p.text3.gamma_multiply(0.7),
    );
    ui.painter().text(
        Pos2::new(center.x, center.y + 10.0),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(11.5),
        p.text3,
    );
}

fn draw_git_panel(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    head: Rect,
    body: Rect,
) {
    let p = theme.palette;
    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(head.left() + 16.0, head.center().y),
            Vec2::splat(14.0),
        ),
        Icon::GitLog,
        p.text2,
    );
    ui.painter().text(
        Pos2::new(head.left() + 28.0, head.center().y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Git Log", "Git 日志"),
        FontId::proportional(11.0),
        p.text2,
    );
    let scope = model
        .active_project()
        .and_then(|project| project.log_branch.clone())
        .unwrap_or_else(|| crate::tr!(model, "All branches", "所有分支").to_string());
    let scope_rect = Rect::from_min_size(
        Pos2::new(head.left() + 80.0, head.top() + 5.0),
        Vec2::new((scope.chars().count() as f32 * 6.2 + 38.0).min(240.0), 23.0),
    );
    ui.painter()
        .rect_filled(scope_rect, egui::CornerRadius::same(5), p.panel2);
    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(scope_rect.left() + 11.0, scope_rect.center().y),
            Vec2::splat(11.0),
        ),
        Icon::Worktree,
        p.text3,
    );
    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(scope_rect.left() + 20.0, scope_rect.top()),
            Pos2::new(scope_rect.right() - 6.0, scope_rect.bottom()),
        ),
        &scope,
        FontId::proportional(10.0),
        p.text3,
        Align2::LEFT_CENTER,
    );

    let mut rx = head.right() - 6.0;
    let close_rect = Rect::from_min_size(Pos2::new(rx - 24.0, head.top() + 4.0), Vec2::splat(24.0));
    if icon_button(ui, close_rect, Icon::X, false, theme).clicked() {
        model.ui.panel_visible = false;
    }
    rx -= 26.0;
    let max_icon = if model.ui.panel_maximised {
        Icon::Minimize
    } else {
        Icon::Maximize
    };
    let max_rect = Rect::from_min_size(Pos2::new(rx - 24.0, head.top() + 4.0), Vec2::splat(24.0));
    if icon_button(ui, max_rect, max_icon, model.ui.panel_maximised, theme).clicked() {
        model.ui.panel_maximised = !model.ui.panel_maximised;
    }
    rx -= 26.0;
    let refresh_rect =
        Rect::from_min_size(Pos2::new(rx - 24.0, head.top() + 4.0), Vec2::splat(24.0));
    if icon_button(ui, refresh_rect, Icon::Refresh, false, theme).clicked() {
        model.queue(Command::RefreshLog);
    }

    let Some(index) = model.active_project else {
        draw_empty(ui, theme, body, "No project open");
        return;
    };
    draw_git_log(ui, model, rt, theme, body, index);
}

fn draw_git_log(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    index: usize,
) {
    let p = theme.palette;
    let project = &model.projects[index];
    let log = project.log.clone();
    let branches = project.branches.clone();
    let current_branch = project.branch();
    let selected = project.selected_commit.clone();
    let details = project.commit_details.clone();
    let log_branch = project.log_branch.clone();
    let query = project.log_query.clone();
    let error = project.log_error.clone();

    // Filter bar.
    let filter = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 30.0));
    ui.painter().rect_filled(filter, 0.0, p.panel);
    ui.painter().hline(
        filter.x_range(),
        filter.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    let search_rect = Rect::from_min_size(
        Pos2::new(filter.left() + 12.0, filter.top() + 4.0),
        Vec2::new((filter.width() * 0.34).clamp(140.0, 320.0), 22.0),
    );
    ui.painter()
        .rect_filled(search_rect, egui::CornerRadius::same(5), p.panel2);
    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(search_rect.left() + 12.0, search_rect.center().y),
            Vec2::splat(11.0),
        ),
        Icon::Search,
        p.text3,
    );
    let mut query_value = query.clone();
    let response = ui.put(
        Rect::from_min_max(
            Pos2::new(search_rect.left() + 22.0, search_rect.top() + 2.0),
            Pos2::new(search_rect.right() - 6.0, search_rect.bottom() - 2.0),
        ),
        TextEdit::singleline(&mut query_value)
            .frame(egui::Frame::NONE)
            .hint_text(crate::tr!(
                model,
                "Filter commits, authors, refs",
                "过滤提交、作者、引用"
            ))
            .font(FontId::proportional(10.5)),
    );
    if response.changed() {
        model.projects[index].log_query = query_value;
    }
    ui.painter().text(
        Pos2::new(filter.right() - 14.0, filter.center().y),
        Align2::RIGHT_CENTER,
        if model.settings.language.is_zh() {
            format!("{} 个提交", log.len())
        } else {
            format!("{} commits", log.len())
        },
        FontId::monospace(9.0),
        p.text3,
    );

    let area = Rect::from_min_max(Pos2::new(rect.left(), filter.bottom()), rect.max);
    let show_branches = area.width() > 720.0;
    let show_detail = area.width() > 950.0 && details.is_some();
    let branch_w = if show_branches { 190.0 } else { 0.0 };
    let detail_w = if show_detail { 300.0 } else { 0.0 };
    let branch_rect = Rect::from_min_size(area.min, Vec2::new(branch_w, area.height()));
    let detail_rect = Rect::from_min_max(Pos2::new(area.right() - detail_w, area.top()), area.max);
    let list_rect = Rect::from_min_max(
        Pos2::new(area.left() + branch_w, area.top()),
        Pos2::new(area.right() - detail_w, area.bottom()),
    );

    if show_branches {
        draw_branch_list(
            ui,
            model,
            theme,
            branch_rect,
            index,
            &branches,
            &current_branch,
            log_branch.as_deref(),
        );
    }
    let filtered: Vec<CommitInfo> = log
        .into_iter()
        .filter(|commit| {
            let query = query.to_lowercase();
            query.is_empty()
                || commit.message.to_lowercase().contains(&query)
                || commit.author.to_lowercase().contains(&query)
                || commit.id.starts_with(&query)
                || commit
                    .refs
                    .iter()
                    .any(|label| label.label.to_lowercase().contains(&query))
        })
        .collect();

    if let Some(error) = error {
        ui.painter().text(
            Pos2::new(list_rect.left() + 20.0, list_rect.top() + 30.0),
            Align2::LEFT_CENTER,
            error,
            FontId::proportional(11.0),
            p.rose,
        );
        return;
    }

    // Row index map for graph edges.
    let row_by_id: HashMap<&str, usize> = filtered
        .iter()
        .enumerate()
        .map(|(row, commit)| (commit.id.as_str(), row))
        .collect();
    let row_h = 30.0;
    let painter = ui.painter().with_clip_rect(list_rect);
    painter.rect_filled(list_rect, 0.0, p.panel);
    let mut select_commit = None;
    super::app_ui::region(ui, list_rect, "log-list", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("git-log")
            .auto_shrink([false, false])
            .show_rows(ui, row_h, filtered.len(), |ui, range| {
                for row_index in range {
                    let commit = &filtered[row_index];
                    let (row, response) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), row_h),
                        Sense::click(),
                    );
                    let is_selected = selected.as_deref() == Some(commit.id.as_str());
                    if is_selected {
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::ZERO, p.active);
                    } else if response.hovered() {
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::ZERO, p.hover);
                    }
                    // Graph lanes.
                    let graph_x = row.left() + 18.0;
                    let lane_gap = 12.0;
                    for edge in &commit.edges {
                        let color = p.lane[edge.color_lane % p.lane.len()];
                        let x0 = graph_x + edge.from_lane as f32 * lane_gap;
                        let x1 = graph_x + edge.to_lane as f32 * lane_gap;
                        let target_row = edge
                            .target_commit
                            .as_deref()
                            .and_then(|id| row_by_id.get(id).copied());
                        let y1 = if let Some(target_row) = target_row {
                            if target_row <= row_index {
                                row.center().y
                            } else {
                                row.center().y
                                    + ((target_row - row_index) as f32 * row_h).min(row_h * 1.05)
                            }
                        } else {
                            row.bottom()
                        };
                        ui.painter().line_segment(
                            [Pos2::new(x0, row.center().y), Pos2::new(x1, y1)],
                            Stroke::new(1.4, color.gamma_multiply(0.85)),
                        );
                    }
                    let x = graph_x + commit.lane as f32 * lane_gap;
                    let color = p.lane[commit.lane % p.lane.len()];
                    ui.painter()
                        .circle_filled(Pos2::new(x, row.center().y), 3.4, p.panel);
                    ui.painter().circle_stroke(
                        Pos2::new(x, row.center().y),
                        3.5,
                        Stroke::new(1.8, color),
                    );
                    // Hash.
                    ui.painter().text(
                        Pos2::new(row.left() + 62.0, row.center().y),
                        Align2::LEFT_CENTER,
                        &commit.short,
                        FontId::monospace(9.5),
                        p.accent,
                    );
                    // Message.
                    let text_x = row.left() + 126.0;
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(text_x, row.top()),
                            Pos2::new(row.right() - 250.0, row.bottom()),
                        ),
                        &commit.message,
                        FontId::proportional(11.0),
                        if is_selected { p.text } else { p.text2 },
                        Align2::LEFT_CENTER,
                    );
                    // Refs near the end of the message column.
                    let refs_right = row.right() - 250.0;
                    for (ref_index, label) in commit.refs.iter().rev().take(2).enumerate() {
                        let width = label.label.chars().count() as f32 * 5.8 + 10.0;
                        let ref_rect = Rect::from_min_size(
                            Pos2::new(
                                refs_right - width - ref_index as f32 * 4.0 - 6.0,
                                row.center().y - 7.5,
                            ),
                            Vec2::new(width, 15.0),
                        );
                        let color = match label.kind {
                            RefKind::Head => p.accent,
                            RefKind::LocalBranch => p.green,
                            RefKind::RemoteBranch => p.blue,
                            RefKind::Tag => p.amber,
                        };
                        ui.painter().rect_filled(
                            ref_rect,
                            egui::CornerRadius::same(3),
                            color.gamma_multiply(0.15),
                        );
                        ui.painter().text(
                            ref_rect.center(),
                            Align2::CENTER_CENTER,
                            &label.label,
                            FontId::monospace(8.0),
                            color,
                        );
                    }
                    ui.painter().text(
                        Pos2::new(row.right() - 160.0, row.center().y),
                        Align2::LEFT_CENTER,
                        &commit.author,
                        FontId::proportional(10.0),
                        p.text3,
                    );
                    ui.painter().text(
                        Pos2::new(row.right() - 16.0, row.center().y),
                        Align2::RIGHT_CENTER,
                        time_ago(commit.time, model.settings.language.is_zh()),
                        FontId::monospace(9.0),
                        p.text3,
                    );
                    if response.clicked() {
                        select_commit = Some(commit.id.clone());
                    }
                }
            });
    });
    if let Some(id) = select_commit {
        model.projects[index].selected_commit = Some(id.clone());
        model.queue(Command::SelectCommit(id));
    }

    if show_detail {
        if let Some(details) = details {
            draw_commit_detail(
                ui,
                theme,
                detail_rect,
                &details,
                rt,
                model.settings.language,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_branch_list(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    index: usize,
    branches: &[lazecode_git::BranchInfo],
    current: &str,
    filter: Option<&str>,
) {
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.panel);
    ui.painter().vline(
        rect.right() - 0.5,
        rect.y_range(),
        Stroke::new(1.0, p.border_soft),
    );
    ui.painter().text(
        Pos2::new(rect.left() + 12.0, rect.top() + 16.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Branches", "分支"),
        FontId::proportional(9.5),
        p.text3,
    );
    let mut y = rect.top() + 30.0;
    let all_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 6.0, y),
        Vec2::new(rect.width() - 12.0, 24.0),
    );
    let response = ui.interact(
        all_rect,
        ui.scope_id().with("log-all-branches"),
        Sense::click(),
    );
    if filter.is_none() {
        ui.painter()
            .rect_filled(all_rect, egui::CornerRadius::same(4), p.active);
    } else if response.hovered() {
        ui.painter()
            .rect_filled(all_rect, egui::CornerRadius::same(4), p.hover);
    }
    ui.painter().text(
        Pos2::new(all_rect.left() + 10.0, all_rect.center().y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "All branches", "所有分支"),
        FontId::proportional(10.5),
        p.text2,
    );
    if response.clicked() {
        model.projects[index].log_branch = None;
        model.queue(Command::RefreshLog);
    }
    y += 26.0;
    let mut pending_branch: Option<Option<String>> = None;
    for branch in branches.iter().filter(|branch| !branch.remote) {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 6.0, y),
            Vec2::new(rect.width() - 12.0, 22.0),
        );
        let active = filter == Some(branch.name.as_str());
        let response = ui.interact(
            row,
            ui.scope_id().with(("log-branch", branch.name.clone())),
            Sense::click(),
        );
        if active {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(4), p.active);
        } else if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(4), p.hover);
        }
        let is_current = branch.name == current;
        ui.painter().circle_stroke(
            Pos2::new(row.left() + 9.0, row.center().y),
            2.4,
            Stroke::new(1.2, if is_current { p.accent } else { p.text3 }),
        );
        if is_current {
            ui.painter()
                .circle_filled(Pos2::new(row.left() + 9.0, row.center().y), 1.4, p.accent);
        }
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(row.left() + 18.0, row.top()),
                Pos2::new(row.right() - 6.0, row.bottom()),
            ),
            &branch.name,
            FontId::proportional(10.5),
            if active { p.text } else { p.text2 },
            Align2::LEFT_CENTER,
        );
        if response.clicked() {
            pending_branch = Some(if active {
                None
            } else {
                Some(branch.name.clone())
            });
        }
        y += 23.0;
    }
    if !branches.iter().any(|branch| branch.remote) {
        // no remote rows
    }
    for branch in branches.iter().filter(|branch| branch.remote) {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 6.0, y),
            Vec2::new(rect.width() - 12.0, 22.0),
        );
        let active = filter == Some(branch.name.as_str());
        let response = ui.interact(
            row,
            ui.scope_id().with(("log-rbranch", branch.name.clone())),
            Sense::click(),
        );
        if active {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(4), p.active);
        } else if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(4), p.hover);
        }
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(row.left() + 18.0, row.top()),
                Pos2::new(row.right() - 6.0, row.bottom()),
            ),
            &branch.name,
            FontId::proportional(10.0),
            p.text3,
            Align2::LEFT_CENTER,
        );
        if response.clicked() {
            pending_branch = Some(if active {
                None
            } else {
                Some(branch.name.clone())
            });
        }
        y += 23.0;
        if y > rect.bottom() - 10.0 {
            break;
        }
    }
    if let Some(branch) = pending_branch {
        model.projects[index].log_branch = branch;
        model.queue(Command::RefreshLog);
    }
}

fn draw_commit_detail(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    details: &lazecode_git::CommitDetails,
    _rt: &mut UiRuntime,
    lang: lazecode_core::Language,
) {
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.panel);
    ui.painter().vline(
        rect.left() + 0.5,
        rect.y_range(),
        Stroke::new(1.0, p.border_soft),
    );
    let mut y = rect.top() + 14.0;
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, y),
        Align2::LEFT_CENTER,
        &details.info.short,
        FontId::monospace(10.0),
        p.accent,
    );
    y += 22.0;
    text_clipped(
        ui.painter(),
        Rect::from_min_size(
            Pos2::new(rect.left() + 14.0, y),
            Vec2::new(rect.width() - 28.0, 18.0),
        ),
        &details.info.message,
        FontId::proportional(12.0),
        p.text,
        Align2::LEFT_CENTER,
    );
    y += 22.0;
    if !details.info.body.is_empty() {
        for line in details.info.body.lines().take(3) {
            text_clipped(
                ui.painter(),
                Rect::from_min_size(
                    Pos2::new(rect.left() + 14.0, y),
                    Vec2::new(rect.width() - 28.0, 16.0),
                ),
                line,
                FontId::proportional(10.0),
                p.text3,
                Align2::LEFT_CENTER,
            );
            y += 15.0;
        }
    }
    y += 4.0;
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, y),
        Align2::LEFT_CENTER,
        format!(
            "{} · {}",
            details.info.author,
            time_ago(details.info.time, lang.is_zh())
        ),
        FontId::proportional(9.5),
        p.text3,
    );
    y += 20.0;
    let total_add: u32 = details.files.iter().map(|file| file.additions).sum();
    let total_del: u32 = details.files.iter().map(|file| file.deletions).sum();
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, y),
        Align2::LEFT_CENTER,
        if lang.is_zh() {
            format!("{} 个文件变更", details.files.len())
        } else {
            format!("{} files changed", details.files.len())
        },
        FontId::proportional(9.5),
        p.text3,
    );
    ui.painter().text(
        Pos2::new(rect.right() - 14.0, y),
        Align2::RIGHT_CENTER,
        format!("+{total_add} −{total_del}"),
        FontId::monospace(9.0),
        p.text3,
    );
    y += 14.0;
    let max_rows = (((rect.bottom() - y - 18.0) / 17.0).floor()).max(0.0) as usize;
    let shown = details.files.len().min(64).min(max_rows);
    for file in details.files.iter().take(shown) {
        let letter = file.kind.letter();
        ui.painter().text(
            Pos2::new(rect.left() + 16.0, y),
            Align2::LEFT_CENTER,
            letter.to_string(),
            FontId::monospace(9.0),
            match letter {
                'A' => p.green,
                'D' => p.rose,
                _ => p.amber,
            },
        );
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(rect.left() + 28.0, y - 8.0),
                Pos2::new(rect.right() - 56.0, y + 8.0),
            ),
            &file.path,
            FontId::proportional(9.5),
            p.text2,
            Align2::LEFT_CENTER,
        );
        ui.painter().text(
            Pos2::new(rect.right() - 14.0, y),
            Align2::RIGHT_CENTER,
            format!("+{} −{}", file.additions, file.deletions),
            FontId::monospace(8.5),
            p.text3,
        );
        y += 17.0;
    }
    let hidden = details.files.len().saturating_sub(shown);
    if hidden > 0 {
        ui.painter().text(
            Pos2::new(rect.left() + 14.0, y.min(rect.bottom() - 10.0)),
            Align2::LEFT_CENTER,
            if lang.is_zh() {
                format!("… 另有 {hidden} 个文件（最大化面板查看）")
            } else {
                format!("… {hidden} more files (maximize the panel)")
            },
            FontId::proportional(9.0),
            p.text3,
        );
    }
    let _ = (Color32::WHITE, TabKind::Diff);
}

fn time_ago(seconds: i64, zh: bool) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let delta = (now - seconds).max(0);
    if zh {
        if delta < 60 {
            "刚刚".to_string()
        } else if delta < 3600 {
            format!("{} 分钟前", delta / 60)
        } else if delta < 86_400 {
            format!("{} 小时前", delta / 3600)
        } else if delta < 86_400 * 30 {
            format!("{} 天前", delta / 86_400)
        } else if delta < 86_400 * 365 {
            format!("{} 个月前", delta / (86_400 * 30))
        } else {
            format!("{} 年前", delta / (86_400 * 365))
        }
    } else if delta < 60 {
        "just now".to_string()
    } else if delta < 3600 {
        format!("{}m ago", delta / 60)
    } else if delta < 86_400 {
        format!("{}h ago", delta / 3600)
    } else if delta < 86_400 * 30 {
        format!("{}d ago", delta / 86_400)
    } else if delta < 86_400 * 365 {
        format!("{}mo ago", delta / (86_400 * 30))
    } else {
        format!("{}y ago", delta / (86_400 * 365))
    }
}
