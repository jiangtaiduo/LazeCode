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
        let worktree = model.projects[index].active_worktree();
        let tabs: Vec<(u64, String, bool, bool)> = worktree
            .terminals
            .iter()
            .map(|terminal| {
                (
                    terminal.id,
                    terminal.title.clone(),
                    terminal.session.is_running(),
                    worktree.active_terminal == Some(terminal.id),
                )
            })
            .collect();
        (
            worktree.terminals.len(),
            tabs,
            worktree.active_terminal,
            worktree
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
    let active_state = active.and_then(|id| model.projects[index].term_by_id(id));
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
            model.projects[index].active_worktree_mut().active_terminal = Some(*id);
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
    let project = &mut model.projects[index];
    let active = project.active_worktree().active_terminal;
    let Some(id) = active else {
        draw_empty(ui, theme, body, "No consoles open");
        return;
    };
    let Some(terminal) = project.term_by_id(id) else {
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
        if let Some(terminal) = project.term_by_id_mut(id) {
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
    // via model.projects[index] elsewhere.
    let (log, branches, current_branch, selected, details, _log_branch, query, error) = {
        let project = &model.projects[index];
        (
            project.log.clone(),
            project.branches.clone(),
            project.branch(),
            project.selected_commit.clone(),
            project.commit_details.clone(),
            project.log_branch.clone(),
            project.log_query.clone(),
            project.log_error.clone(),
        )
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
                    let root = model.projects[index].root.clone();
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
                let root = model.projects[index].root.clone();
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
                        _ => {}
                    }
                }
                let _ = root;
            }
        }
    }

    // Filter bar.
    let filter_offset = if banner_visible { 24.0 } else { 0.0 };
    let filter = Rect::from_min_size(
        Pos2::new(rect.left(), rect.top() + filter_offset),
        Vec2::new(rect.width(), 30.0),
    );
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
    let mut query_value = query.text.clone();
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
        model.projects[index].log_query.text = query_value;
    }
    // Text filter option toggles: plugin anchors right of the search box.
    let mut button_x = search_rect.right() + 8.0;
    let draw_toggle = |ui: &mut Ui, rect: Rect, text: &str, active: bool, tooltip: &str| -> bool {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(4), p.panel2);
        if active {
            ui.painter().rect_stroke(
                rect,
                egui::CornerRadius::same(4),
                Stroke::new(1.0, p.accent),
                egui::StrokeKind::Inside,
            );
        }
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::monospace(9.0),
            if active { p.accent } else { p.text3 },
        );
        ui.interact(
            rect,
            ui.scope_id().with(("logtoggle", tooltip)),
            Sense::click(),
        )
        .clicked()
    };
    let regex_rect = Rect::from_min_size(
        Pos2::new(button_x, filter.top() + 4.5),
        Vec2::new(30.0, 21.0),
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
    button_x += 34.0;
    let case_rect = Rect::from_min_size(
        Pos2::new(button_x, filter.top() + 4.5),
        Vec2::new(30.0, 21.0),
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
    button_x += 38.0;
    // Branch filter menu.
    let branch_menu_rect = Rect::from_min_size(
        Pos2::new(button_x, filter.top() + 4.5),
        Vec2::new(96.0, 21.0),
    );
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
    ui.painter()
        .rect_filled(branch_menu_rect, egui::CornerRadius::same(4), p.panel2);
    ui.painter().text(
        branch_menu_rect.center(),
        Align2::CENTER_CENTER,
        &branch_label,
        FontId::proportional(9.5),
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
        rt.menu = Some(crate::runtime::MenuState {
            kind: crate::runtime::MenuKind::LogBranchFilter { project: index },
            anchor: branch_menu_rect,
            payload: None,
        });
    }
    // Author chip: opens the same menu kind; the overlay presents its payload
    // rows as author names (payload format: newline-separated names).
    let authors: Vec<String> = {
        let mut set = std::collections::BTreeSet::new();
        for commit in log.iter() {
            set.insert(commit.author.clone());
        }
        set.into_iter().take(24).collect()
    };
    button_x += branch_menu_rect.width() + 6.0;
    let author_label = if query.author.is_empty() {
        if model.settings.language.is_zh() {
            "👤 全部作者".to_string()
        } else {
            "👤 All authors".to_string()
        }
    } else {
        format!("👤 {}", query.author)
    };
    let author_rect = Rect::from_min_size(
        Pos2::new(button_x, filter.top() + 4.5),
        Vec2::new(
            (author_label.chars().count() as f32 * 7.0 + 26.0).clamp(96.0, 190.0),
            21.0,
        ),
    );
    ui.painter()
        .rect_filled(author_rect, egui::CornerRadius::same(4), p.panel2);
    ui.painter().text(
        author_rect.center(),
        Align2::CENTER_CENTER,
        &author_label,
        FontId::proportional(9.5),
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
        rt.menu = Some(crate::runtime::MenuState {
            kind: crate::runtime::MenuKind::LogBranchFilter { project: index },
            anchor: author_rect,
            payload: Some(authors.join("\n")),
        });
    }
    // Author pick consumed one frame after the menu closes.
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
    }
    let filtered: Vec<CommitInfo> = log
        .into_iter()
        .filter(|commit| {
            let query = if model.projects[index].log_query.case_sensitive {
                model.projects[index].log_query.text.clone()
            } else {
                model.projects[index].log_query.text.to_lowercase()
            };
            let matches_query = |haystack: String| {
                if query.is_empty() {
                    return true;
                }
                let haystack = if model.projects[index].log_query.case_sensitive {
                    haystack
                } else {
                    haystack.to_lowercase()
                };
                if model.projects[index].log_query.use_regex {
                    if let Ok(re) = lazecode_fs::compile_regex(&query, false) {
                        return !haystack.is_empty() && re.is_match(&haystack);
                    }
                    return false;
                }
                haystack.contains(&query)
            };
            matches_query(commit.message.clone())
                || matches_query(commit.author.clone())
                || (query.is_empty() || commit.id.starts_with(&query))
                || commit
                    .refs
                    .iter()
                    .any(|label| matches_query(label.label.clone()))
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
                    let out_of_head = commit.in_head == Some(false);
                    let is_selected = selected.as_deref() == Some(commit.id.as_str());
                    if is_selected {
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::ZERO, p.active);
                    } else if response.hovered() {
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::ZERO, p.hover);
                    }
                    // Projection: font dimming for out-of-HEAD commits.
                    let dim = out_of_head && !is_selected;
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
                        if dim {
                            p.text3.gamma_multiply(0.65)
                        } else if is_selected {
                            p.text
                        } else {
                            p.text2
                        },
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
                    if response.secondary_clicked() {
                        rt.menu = Some(crate::runtime::MenuState {
                            kind: crate::runtime::MenuKind::LogCommitMenu { project: index },
                            anchor: row,
                            payload: Some(commit.id.clone()),
                        });
                    }
                }
            });
    });
    if let Some(id) = select_commit {
        model.projects[index].selected_commit = Some(id.clone());
        model.queue(Command::SelectCommit(id));
    }

    // Auto-paging: request the next page when the scroll tail is reached and
    // every page still filled to capacity (a short page = history end).
    let tailed = ui.input(|input| {
        input
            .pointer
            .latest_pos()
            .is_some_and(|pos| list_rect.contains(pos) && pos.y > list_rect.bottom() - 240.0)
    });
    let full_page = model.projects[index].log.len() >= 300;
    if tailed
        && full_page
        && !model.projects[index].log_paging
        && !model.projects[index].log_loading
    {
        model.projects[index].log_paging = true;
        model.queue(Command::LogLoadMore);
        log::debug!("git log tail paged at {}", model.projects[index].log.len());
    }

    if show_detail {
        if let Some(details) = details {
            draw_commit_detail(
                ui,
                model,
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
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    index: usize,
    branches: &[lazecode_git::BranchInfo],
    current: &str,
    tags: &[String],
    tag_targets: &[(String, String)],
) {
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.panel);
    ui.painter().vline(
        rect.right() - 0.5,
        rect.y_range(),
        Stroke::new(1.0, p.border_soft),
    );

    // Search field of the branch pane.
    let search_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 6.0, rect.top() + 6.0),
        Vec2::new(rect.width() - 12.0, 24.0),
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
    let mut query_value = model.projects[index].branch_query.clone();
    let response = ui.put(
        Rect::from_min_max(
            Pos2::new(search_rect.left() + 24.0, search_rect.top() + 2.0),
            Pos2::new(search_rect.right() - 6.0, search_rect.bottom() - 2.0),
        ),
        TextEdit::singleline(&mut query_value)
            .frame(egui::Frame::NONE)
            .hint_text(crate::tr!(model, "Filter branches", "过滤分支"))
            .font(FontId::proportional(10.0)),
    );
    if response.changed() {
        model.projects[index].branch_query = query_value;
    }
    let needle = model.projects[index].branch_query.to_lowercase();
    let matches = |name: &str| needle.is_empty() || name.to_lowercase().contains(&needle);

    // Build the ref tree from the branches that pass the filter.
    let local: Vec<lazecode_git::BranchInfo> = branches
        .iter()
        .filter(|branch| !branch.remote && matches(&branch.name))
        .cloned()
        .collect();
    let remote: Vec<lazecode_git::BranchInfo> = branches
        .iter()
        .filter(|branch| branch.remote && matches(&branch.name))
        .cloned()
        .collect();
    let tags: Vec<String> = tags.iter().filter(|tag| matches(tag)).cloned().collect();

    let rows_rect = Rect::from_min_max(
        Pos2::new(rect.left(), search_rect.bottom() + 4.0),
        Pos2::new(rect.right(), rect.bottom()),
    );
    let painter = ui.painter().with_clip_rect(rows_rect);
    let row_h = 22.0;
    let mut y = rows_rect.top() + 6.0;

    // Sections: Local / Remote / Tags; the roots are pre-built above.
    let expanded_dirs: std::collections::HashSet<String> =
        model.projects[index].ref_dirs_expanded.clone();
    let mut toggle_dir: Option<String> = None;
    let mut pick_ref: Option<(String, bool)> = None; // (name, remote)
    let mut open_menu: Option<(String, bool)> = None; // branch name + remote flag

    let draw_dir_row = |painter: &egui::Painter,
                        ui: &mut Ui,
                        label: &str,
                        path: &str,
                        expanded: bool,
                        y: &mut f32|
     -> bool {
        let row = Rect::from_min_size(
            Pos2::new(rows_rect.left() + 4.0, *y),
            Vec2::new(rows_rect.width() - 8.0, 20.0),
        );
        let response = ui.interact(
            row,
            ui.scope_id().with(("refdir", path.to_string())),
            Sense::click(),
        );
        if response.hovered() {
            painter.rect_filled(row, egui::CornerRadius::same(3), p.hover);
        }
        // chevron.
        let cx = row.left() + 12.0;
        let cy = row.center().y;
        let color = p.text3;
        let (dy, dx) = if expanded { (0.0, 3.0) } else { (-3.0, 0.0) };
        painter.line_segment(
            [
                Pos2::new(cx - 3.5 + dx, cy - 3.5 + dy),
                Pos2::new(cx + 3.5 + dx, cy + 3.5 + dy),
            ],
            Stroke::new(1.4, color),
        );
        painter.line_segment(
            [
                Pos2::new(cx + 3.5 + dx, cy + 3.5 + dy),
                Pos2::new(cx - 3.5 + dx * 2.0, cy - 3.5 + dy * 2.0),
            ],
            Stroke::new(1.4, color),
        );
        painter.text(
            Pos2::new(row.left() + 26.0, row.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(10.5),
            p.text2,
        );
        *y += row_h;
        response.clicked()
    };

    // Local tree.
    painter.text(
        Pos2::new(rows_rect.left() + 12.0, y + 8.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Local", "本地"),
        FontId::proportional(9.5),
        p.text3,
    );
    y += 24.0;
    let local_tree = lazecode_core::gitref::build_ref_tree(&local);
    let flat = lazecode_core::gitref::flatten_ref_tree(&local_tree, &expanded_dirs);
    for row_ref in flat.iter() {
        if y > rows_rect.bottom() - 8.0 {
            break;
        }
        if row_ref.dir {
            if draw_dir_row(
                &painter,
                ui,
                &row_ref.label,
                &row_ref.path,
                expanded_dirs.contains(&row_ref.path),
                &mut y,
            ) {
                toggle_dir = Some(row_ref.path.clone());
            }
        } else if row_ref.is_tag {
            // Tags live in their own section below.
        } else if let Some(branch) = row_ref.branch.as_ref() {
            let row = Rect::from_min_size(
                Pos2::new(rows_rect.left() + 4.0, y),
                Vec2::new(rows_rect.width() - 8.0, 20.0),
            );
            let indent = 18.0 + 14.0 * row_ref.depth as f32;
            let is_current = branch.name == current;
            let is_fav = model.favorite_branches.contains(&branch.name);
            let response = ui.interact(
                row,
                ui.scope_id().with(("refrow", branch.name.clone())),
                Sense::click(),
            );
            if response.hovered() {
                painter.rect_filled(row, egui::CornerRadius::same(3), p.hover);
            }
            // Branch knob: filled dot for HEAD.
            if is_current {
                painter.circle_filled(
                    Pos2::new(row.left() + indent, row.center().y),
                    3.0,
                    p.accent,
                );
            } else {
                painter.circle_stroke(
                    Pos2::new(row.left() + indent, row.center().y),
                    3.0,
                    Stroke::new(1.3, p.text3),
                );
            }
            let name_label = row_ref.label.clone();
            painter.text(
                Pos2::new(row.left() + indent + 8.0, row.center().y),
                Align2::LEFT_CENTER,
                &name_label,
                FontId::proportional(10.5),
                if is_current { p.text } else { p.text2 },
            );
            // ahead/behind badge.
            if branch.ahead > 0 || branch.behind > 0 {
                let badge = format!("↑{} ↓{}", branch.ahead, branch.behind);
                painter.text(
                    Pos2::new(row.right() - 8.0, row.center().y),
                    Align2::RIGHT_CENTER,
                    badge,
                    FontId::monospace(8.5),
                    p.text3,
                );
            }
            // Favorite star.
            if is_fav {
                painter.text(
                    Pos2::new(
                        row.right()
                            - (if branch.ahead > 0 || branch.behind > 0 {
                                56.0
                            } else {
                                10.0
                            }),
                        row.center().y,
                    ),
                    Align2::RIGHT_CENTER,
                    "★",
                    FontId::proportional(10.0),
                    p.amber,
                );
            }
            if response.secondary_clicked() {
                open_menu = Some((branch.name.clone(), branch.remote));
            }
            if response.clicked() {
                pick_ref = Some((branch.name.clone(), branch.remote));
            }
            y += row_h;
        }
    }

    // Remote section.
    if !remote.is_empty() {
        painter.text(
            Pos2::new(rows_rect.left() + 12.0, y + 8.0),
            Align2::LEFT_CENTER,
            crate::tr!(model, "Remote", "远程"),
            FontId::proportional(9.5),
            p.text3,
        );
        y += 24.0;
        let remote_tree = lazecode_core::gitref::build_ref_tree(&remote);
        let flat = lazecode_core::gitref::flatten_ref_tree(&remote_tree, &expanded_dirs);
        for row_ref in flat.iter() {
            if y > rows_rect.bottom() - 8.0 {
                break;
            }
            if row_ref.dir {
                if draw_dir_row(
                    &painter,
                    ui,
                    &row_ref.label,
                    &row_ref.path,
                    expanded_dirs.contains(&row_ref.path),
                    &mut y,
                ) {
                    toggle_dir = Some(row_ref.path.clone());
                }
            } else if let Some(branch) = row_ref.branch.as_ref() {
                let row = Rect::from_min_size(
                    Pos2::new(rows_rect.left() + 4.0, y),
                    Vec2::new(rows_rect.width() - 8.0, 20.0),
                );
                let indent = 18.0 + 14.0 * row_ref.depth as f32;
                let response = ui.interact(
                    row,
                    ui.scope_id().with(("refrow", branch.name.clone())),
                    Sense::click(),
                );
                if response.hovered() {
                    painter.rect_filled(row, egui::CornerRadius::same(3), p.hover);
                }
                painter.circle_stroke(
                    Pos2::new(row.left() + indent, row.center().y),
                    3.0,
                    Stroke::new(1.3, p.blue),
                );
                painter.text(
                    Pos2::new(row.left() + indent + 8.0, row.center().y),
                    Align2::LEFT_CENTER,
                    &row_ref.label,
                    FontId::proportional(10.5),
                    p.text2,
                );
                if response.secondary_clicked() {
                    open_menu = Some((branch.name.clone(), true));
                }
                if response.clicked() {
                    pick_ref = Some((branch.name.clone(), true));
                }
                y += row_h;
            }
        }
    }

    // Tags section.
    if !tags.is_empty() {
        painter.text(
            Pos2::new(rows_rect.left() + 12.0, y + 8.0),
            Align2::LEFT_CENTER,
            crate::tr!(model, "Tags", "标签"),
            FontId::proportional(9.5),
            p.text3,
        );
        y += 24.0;
        for tag in tags.iter() {
            if y > rows_rect.bottom() - 8.0 {
                break;
            }
            let tag_target = tag_targets
                .iter()
                .find(|(name, _)| name == tag)
                .map(|(_, target)| target.clone())
                .unwrap_or_default();
            let row = Rect::from_min_size(
                Pos2::new(rows_rect.left() + 4.0, y),
                Vec2::new(rows_rect.width() - 8.0, 20.0),
            );
            let response = ui.interact(
                row,
                ui.scope_id().with(("tagrow", tag.clone())),
                Sense::click(),
            );
            if response.hovered() {
                painter.rect_filled(row, egui::CornerRadius::same(3), p.hover);
            }
            painter.text(
                Pos2::new(row.left() + 18.0, row.center().y),
                Align2::LEFT_CENTER,
                "◇",
                FontId::proportional(10.0),
                p.amber,
            );
            painter.text(
                Pos2::new(row.left() + 32.0, row.center().y),
                Align2::LEFT_CENTER,
                tag,
                FontId::proportional(10.5),
                p.text2,
            );
            if !tag_target.is_empty() {
                painter.text(
                    Pos2::new(row.right() - 8.0, row.center().y),
                    Align2::RIGHT_CENTER,
                    &tag_target,
                    FontId::monospace(8.5),
                    p.text3,
                );
            }
            y += row_h;
        }
    }

    // Apply deferred actions.
    if let Some(dir) = toggle_dir {
        if expanded_dirs.contains(&dir) {
            model.projects[index].ref_dirs_expanded.remove(&dir);
        } else {
            model.projects[index].ref_dirs_expanded.insert(dir);
        }
    }
    if let Some((name, remote)) = pick_ref {
        // Click sets the log branch filter (keep multi-select for M5 batch ops).
        let query = name.clone();
        if let Some(project) = model.projects.get_mut(index) {
            project.log_query.branch = query;
            project.log_query.all_refs = false;
        }
        model.queue(Command::LogFilterChanged);
        let _ = remote;
    }
    if let Some((name, remote)) = open_menu {
        rt.menu = Some(crate::runtime::MenuState {
            kind: crate::runtime::MenuKind::BranchContext { project: index },
            anchor: rect,
            payload: Some(format!(
                "{name}\u{1}{}",
                if remote { "remote" } else { "local" }
            )),
        });
    }
}

fn draw_commit_detail(
    ui: &mut Ui,
    model: &mut AppModel,
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
    for (row_idx, file) in details.files.iter().take(shown).enumerate() {
        let row_rect = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, y - 8.0),
            Vec2::new(rect.width() - 8.0, 17.0),
        );
        let resp = ui.interact(
            row_rect,
            ui.scope_id().with(("commit-file", row_idx, &file.path)),
            Sense::click(),
        );
        if resp.hovered() {
            ui.painter()
                .rect_filled(row_rect, egui::CornerRadius::same(3), p.hover);
        }
        if resp.clicked() {
            model.queue(Command::OpenCommitDiff {
                commit: details.info.id.clone(),
                path: file.path.clone(),
            });
        }
        if resp.secondary_clicked() {
            model.queue(Command::OpenCompareWithLocalDiff {
                commit: details.info.id.clone(),
                path: file.path.clone(),
            });
        }
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
            if resp.hovered() { p.text } else { p.text2 },
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
