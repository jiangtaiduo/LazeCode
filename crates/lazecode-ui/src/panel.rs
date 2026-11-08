use std::collections::HashSet;

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, TextEdit, Ui, Vec2};
use lazecode_core::{AppModel, Command, PanelTab};
use lazecode_git::{
    lane_color, lane_x, CommitInfo, GraphBreak, GraphRowLayout, RefKind, GRAPH_BREAK_GAP, GRAPH_H,
    LANE_W, NODE_R,
};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::UiRuntime;
use crate::theme::Theme;
use crate::widgets::{icon_button, live_dot, text_clipped};

fn format_commit_datetime(seconds: i64) -> String {
    unsafe {
        let t = seconds as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return format!("{seconds}");
        }
        format!(
            "{}/{}/{} {:02}:{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min
        )
    }
}

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

fn draw_dag_cell(
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
const HEAD_H: f32 = 34.0;

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
    // Subtabs: Git / Log / Console
    ui.painter().text(
        Pos2::new(head.left() + 14.0, head.center().y),
        Align2::LEFT_CENTER,
        "Git",
        FontId::proportional(13.0),
        p.text,
    );

    let mut tab_x = head.left() + 48.0;
    // Log subtab
    let log_rect = Rect::from_min_size(Pos2::new(tab_x, head.top() + 5.0), Vec2::new(42.0, 23.0));
    let log_resp = ui.interact(
        log_rect,
        ui.scope_id().with("console-subtab-log"),
        Sense::click(),
    );
    if log_resp.hovered() {
        ui.painter()
            .rect_filled(log_rect, egui::CornerRadius::same(5), p.hover);
    }
    ui.painter().text(
        log_rect.center(),
        Align2::CENTER_CENTER,
        crate::tr!(model, "Log", "日志"),
        FontId::proportional(12.0),
        p.text3,
    );
    if log_resp.clicked() {
        model.ui.panel_tab = PanelTab::Git;
    }
    tab_x += 46.0;

    // Console subtab (active)
    let con_rect = Rect::from_min_size(Pos2::new(tab_x, head.top() + 5.0), Vec2::new(58.0, 23.0));
    ui.painter()
        .rect_filled(con_rect, egui::CornerRadius::same(5), p.panel2);
    ui.painter().hline(
        con_rect.x_range(),
        con_rect.bottom() - 1.0,
        Stroke::new(2.0, p.accent),
    );
    ui.painter().text(
        con_rect.center(),
        Align2::CENTER_CENTER,
        crate::tr!(model, "Console", "控制台"),
        FontId::proportional(12.0),
        p.text,
    );
    tab_x += 62.0;
    if count > 0 {
        let badge = Rect::from_center_size(
            Pos2::new(tab_x + 10.0, head.center().y),
            Vec2::new(18.0, 14.0),
        );
        ui.painter()
            .rect_filled(badge, egui::CornerRadius::same(7), p.panel2);
        ui.painter().text(
            badge.center(),
            Align2::CENTER_CENTER,
            count.to_string(),
            FontId::monospace(10.5),
            p.text3,
        );
        tab_x += 22.0;
    }
    if running > 0 {
        live_dot(
            ui.painter(),
            Pos2::new(tab_x + 10.0, head.center().y),
            true,
            p.running,
        );
        tab_x += 16.0;
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
        rt.toggle_agent_menu(spark_rect);
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

    // Console tabs: positioned safely after Console subtab with a subtle vertical divider.
    ui.painter().vline(
        tab_x + 6.0,
        (head.top() + 7.0)..=(head.bottom() - 7.0),
        Stroke::new(1.0, p.border_soft),
    );
    let mut x = tab_x + 14.0;
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
                Pos2::new(tab.left() + 23.0, tab.top()),
                Pos2::new(tab.right() - 22.0, tab.bottom()),
            ),
            title,
            FontId::proportional(12.5),
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
        if close_response.clicked() || response.middle_clicked() {
            close_terminal = Some(*id);
        } else if response.clicked() {
            model.projects[index].active_worktree_mut().active_terminal = Some(*id);
            rt.set_terminal_focus(*id);
            model.ui.panel_tab = PanelTab::Console;
        } else if response.secondary_clicked() {
            rt.open_menu(
                crate::runtime::MenuKind::TerminalTabContext {
                    project: index,
                    terminal_id: *id,
                },
                tab,
                Some(title.clone()),
            );
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
    // Dock title & Subtabs: Log | Console
    ui.painter().text(
        Pos2::new(head.left() + 14.0, head.center().y),
        Align2::LEFT_CENTER,
        "Git",
        FontId::proportional(13.0),
        p.text,
    );

    let mut tab_x = head.left() + 48.0;
    // Log subtab (active)
    let log_rect = Rect::from_min_size(Pos2::new(tab_x, head.top() + 5.0), Vec2::new(42.0, 23.0));
    ui.painter()
        .rect_filled(log_rect, egui::CornerRadius::same(5), p.panel2);
    ui.painter().hline(
        log_rect.x_range(),
        log_rect.bottom() - 1.0,
        Stroke::new(2.0, p.accent),
    );
    ui.painter().text(
        log_rect.center(),
        Align2::CENTER_CENTER,
        crate::tr!(model, "Log", "日志"),
        FontId::proportional(12.0),
        p.text,
    );
    tab_x += 46.0;

    // Console subtab
    let con_rect = Rect::from_min_size(Pos2::new(tab_x, head.top() + 5.0), Vec2::new(58.0, 23.0));
    let con_resp = ui.interact(
        con_rect,
        ui.scope_id().with("git-subtab-console"),
        Sense::click(),
    );
    if con_resp.hovered() {
        ui.painter()
            .rect_filled(con_rect, egui::CornerRadius::same(5), p.hover);
    }
    ui.painter().text(
        con_rect.center(),
        Align2::CENTER_CENTER,
        crate::tr!(model, "Console", "控制台"),
        FontId::proportional(12.0),
        p.text3,
    );
    if con_resp.clicked() {
        model.ui.panel_tab = PanelTab::Console;
    }
    tab_x += 64.0;
    let scope = model
        .active_project()
        .and_then(|project| project.log_branch.clone())
        .unwrap_or_else(|| crate::tr!(model, "All branches", "所有分支").to_string());
    let scope_rect = Rect::from_min_size(
        Pos2::new(tab_x, head.top() + 5.0),
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
            .font(FontId::proportional(12.5)),
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
            FontId::monospace(11.0),
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
    button_x += author_rect.width() + 6.0;

    // Date filter chip
    let date_label = match query.date {
        lazecode_core::DatePreset::All => crate::tr!(model, "Date ▾", "日期 ▾").to_string(),
        lazecode_core::DatePreset::Today => crate::tr!(model, "Today", "今天").to_string(),
        lazecode_core::DatePreset::Week => crate::tr!(model, "This Week", "本周").to_string(),
        lazecode_core::DatePreset::Month => crate::tr!(model, "This Month", "本月").to_string(),
    };
    let date_rect = Rect::from_min_size(
        Pos2::new(button_x, filter.top() + 4.5),
        Vec2::new(
            (date_label.chars().count() as f32 * 7.0 + 20.0).clamp(65.0, 110.0),
            21.0,
        ),
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
        let next_date = match query.date {
            lazecode_core::DatePreset::All => lazecode_core::DatePreset::Today,
            lazecode_core::DatePreset::Today => lazecode_core::DatePreset::Week,
            lazecode_core::DatePreset::Week => lazecode_core::DatePreset::Month,
            lazecode_core::DatePreset::Month => lazecode_core::DatePreset::All,
        };
        model.projects[index].log_query.date = next_date;
        model.queue(Command::LogFilterChanged);
    }
    button_x += date_rect.width() + 6.0;

    // Paths filter chip
    let paths_label = if query.path.is_empty() {
        crate::tr!(model, "Paths ▾", "路径 ▾").to_string()
    } else {
        format!("📁 {}", query.path)
    };
    let paths_rect = Rect::from_min_size(
        Pos2::new(button_x, filter.top() + 4.5),
        Vec2::new(
            (paths_label.chars().count() as f32 * 7.0 + 20.0).clamp(65.0, 130.0),
            21.0,
        ),
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

    // Right toolbar buttons in filter bar (Image #1):
    // Refresh, Compact graph toggle, Sort toggle, and Commit count
    let mut frx = filter.right() - 8.0;
    let refresh_rect =
        Rect::from_min_size(Pos2::new(frx - 20.0, filter.top() + 5.0), Vec2::splat(20.0));
    let refresh_resp = icon_button(ui, refresh_rect, Icon::Refresh, false, theme);
    if refresh_resp.hovered() {
        refresh_resp.show_tooltip_text(crate::tr!(model, "Refresh Log", "刷新日志"));
    }
    if refresh_resp.clicked() {
        model.queue(Command::RefreshLog);
    }
    frx -= 24.0;

    let compact_rect =
        Rect::from_min_size(Pos2::new(frx - 20.0, filter.top() + 5.0), Vec2::splat(20.0));
    let compact_resp = icon_button(
        ui,
        compact_rect,
        Icon::GitLog,
        model.ui.git_log_compact,
        theme,
    );
    if compact_resp.hovered() {
        compact_resp.show_tooltip_text(crate::tr!(model, "Toggle Compact Graph", "切换紧凑图"));
    }
    if compact_resp.clicked() {
        model.ui.git_log_compact = !model.ui.git_log_compact;
    }
    frx -= 24.0;

    let sort_rect =
        Rect::from_min_size(Pos2::new(frx - 20.0, filter.top() + 5.0), Vec2::splat(20.0));
    let sort_resp = icon_button(ui, sort_rect, Icon::Commit, query.topo_order, theme);
    if sort_resp.hovered() {
        sort_resp.show_tooltip_text(if query.topo_order {
            crate::tr!(model, "Topological Order (Active)", "拓扑排序（已激活）")
        } else {
            crate::tr!(model, "Date Order (Active)", "日期排序（已激活）")
        });
    }
    if sort_resp.clicked() {
        model.projects[index].log_query.topo_order = !query.topo_order;
        model.queue(Command::LogFilterChanged);
    }
    frx -= 28.0;

    ui.painter().text(
        Pos2::new(frx, filter.center().y),
        Align2::RIGHT_CENTER,
        if model.projects[index].log_paging {
            if model.settings.language.is_zh() {
                format!("{} 个提交 (加载中...)", log.len())
            } else {
                format!("{} commits (loading...)", log.len())
            }
        } else if model.settings.language.is_zh() {
            format!("{} 个提交", log.len())
        } else {
            format!("{} commits", log.len())
        },
        FontId::monospace(9.5),
        p.text3,
    );

    let area = Rect::from_min_max(Pos2::new(rect.left(), filter.bottom()), rect.max);

    // 1. Column 1: Vertical tool strip on the far left (26px wide)
    let toolbar_w = 26.0;
    let toolbar_rect = Rect::from_min_size(area.min, Vec2::new(toolbar_w, area.height()));

    // 2. Column 2: Branch tree column
    let show_branches = model.ui.git_branches_visible && area.width() > 500.0;
    let branch_w = if show_branches {
        model.ui.git_branches_width.clamp(140.0, 360.0)
    } else {
        0.0
    };
    let branch_rect = Rect::from_min_size(
        Pos2::new(toolbar_rect.right(), area.top()),
        Vec2::new(branch_w, area.height()),
    );

    // 3. Column 4: Commit details & changed files column
    let show_detail = model.ui.git_details_visible && details.is_some() && area.width() > 700.0;
    let detail_w = if show_detail {
        model.ui.git_details_width.clamp(200.0, 500.0)
    } else {
        0.0
    };
    let detail_rect = Rect::from_min_max(Pos2::new(area.right() - detail_w, area.top()), area.max);

    // 4. Column 3: Center commit log table
    let list_rect = Rect::from_min_max(
        Pos2::new(branch_rect.right(), area.top()),
        Pos2::new(area.right() - detail_w, area.bottom()),
    );

    // Paint vertical toolbar (PyCharm style)
    ui.painter().rect_filled(toolbar_rect, 0.0, p.panel);
    ui.painter().vline(
        toolbar_rect.right() - 0.5,
        toolbar_rect.y_range(),
        Stroke::new(1.0, p.border_soft),
    );
    let mut tb_y = toolbar_rect.top() + 6.0;

    // 1. Collapse / expand branches button
    let collapse_rect = Rect::from_min_size(
        Pos2::new(toolbar_rect.left() + 3.0, tb_y),
        Vec2::splat(20.0),
    );
    let collapse_icon = if model.ui.git_branches_visible {
        Icon::DoubleChevronLeft
    } else {
        Icon::DoubleChevronRight
    };
    if icon_button(ui, collapse_rect, collapse_icon, false, theme).clicked() {
        model.ui.git_branches_visible = !model.ui.git_branches_visible;
    }
    tb_y += 24.0;

    // Separator
    ui.painter().hline(
        (toolbar_rect.left() + 4.0)..=(toolbar_rect.right() - 4.0),
        tb_y + 2.0,
        Stroke::new(1.0, p.border_soft),
    );
    tb_y += 6.0;

    // 2. New branch button (+)
    let new_branch_rect = Rect::from_min_size(
        Pos2::new(toolbar_rect.left() + 3.0, tb_y),
        Vec2::splat(20.0),
    );
    let new_branch_resp = icon_button(ui, new_branch_rect, Icon::Plus, false, theme);
    if new_branch_resp.hovered() {
        new_branch_resp.show_tooltip_text(crate::tr!(model, "New Branch", "新建分支"));
    }
    if new_branch_resp.clicked() {
        rt.dialog = crate::runtime::DialogState::NewBranch {
            value: String::new(),
            error: None,
        };
    }
    tb_y += 24.0;

    // 3. Update / fetch button (↓)
    let fetch_rect = Rect::from_min_size(
        Pos2::new(toolbar_rect.left() + 3.0, tb_y),
        Vec2::splat(20.0),
    );
    let fetch_resp = icon_button(ui, fetch_rect, Icon::ArrowDown, false, theme);
    if fetch_resp.hovered() {
        fetch_resp.show_tooltip_text(crate::tr!(model, "Update / Fetch", "更新 / 拉取"));
    }
    if fetch_resp.clicked() {
        model.queue(Command::FetchAll);
    }
    tb_y += 24.0;

    // 4. Delete branch button (trash)
    let del_rect = Rect::from_min_size(
        Pos2::new(toolbar_rect.left() + 3.0, tb_y),
        Vec2::splat(20.0),
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
                    error: None,
                };
            }
        }
    }
    tb_y += 24.0;

    // 5. Compare Selected（原「Compare with Working Tree」死按钮：有
    // tooltip 无 clicked。接上稳定版 GitBottomPanel 工具栏的「比较选中
    // 分支」语义：恰好选中 2 个分支时打开比较）。
    let diff_rect = Rect::from_min_size(
        Pos2::new(toolbar_rect.left() + 3.0, tb_y),
        Vec2::splat(20.0),
    );
    let compare_enabled = model
        .projects
        .get(index)
        .map(|project| project.ref_selection.len() == 2)
        .unwrap_or(false);
    let diff_resp = icon_button(ui, diff_rect, Icon::Diff, compare_enabled, theme);
    if diff_resp.hovered() {
        diff_resp.show_tooltip_text(crate::tr!(
            model,
            "Compare Selected Branches",
            "比较选中的分支"
        ));
    }
    if diff_resp.clicked() {
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
    tb_y += 24.0;

    // 6. Star / Favorite button
    let star_rect = Rect::from_min_size(
        Pos2::new(toolbar_rect.left() + 3.0, tb_y),
        Vec2::splat(20.0),
    );
    let star_resp = icon_button(ui, star_rect, Icon::Star, false, theme);
    if star_resp.hovered() {
        star_resp.show_tooltip_text(crate::tr!(
            model,
            "Favorite / Unfavorite",
            "收藏 / 取消收藏"
        ));
    }
    if star_resp.clicked() {
        if let Some(target) = model.projects[index].ref_selection.first().cloned() {
            if model.projects[index].favorites.contains(&target) {
                model.projects[index].favorites.remove(&target);
            } else {
                model.projects[index].favorites.insert(target);
            }
        }
    }

    if show_branches {
        let splitter_rect = Rect::from_min_size(
            Pos2::new(branch_rect.right() - 3.0, area.top()),
            Vec2::new(6.0, area.height()),
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
            area.y_range(),
            if is_active_split {
                Stroke::new(2.0, p.accent)
            } else {
                Stroke::new(1.0, p.border)
            },
        );
    }

    if show_detail {
        let splitter_rect = Rect::from_min_size(
            Pos2::new(detail_rect.left() - 3.0, area.top()),
            Vec2::new(6.0, area.height()),
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
                (model.ui.git_details_width - resp.drag_delta().x).clamp(200.0, 500.0);
        }
        if let Some(details) = details.as_ref() {
            draw_commit_detail(
                ui,
                model,
                theme,
                detail_rect,
                details,
                rt,
                model.settings.language,
                index,
            );
        }
        ui.painter().vline(
            detail_rect.left() + 0.5,
            area.y_range(),
            if is_active_split {
                Stroke::new(2.0, p.accent)
            } else {
                Stroke::new(1.0, p.border)
            },
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

    let dag = lazecode_git::compute_dag(&filtered);
    let dag_width = (10.0 + (dag.max_lane + 1) as f32 * LANE_W).max(40.0);
    let row_h = GRAPH_H; // 26.0!

    let painter = ui.painter().with_clip_rect(list_rect);
    painter.rect_filled(list_rect, 0.0, p.panel);
    let mut select_commit = None;
    let jump_target = model.ui.git_jump_to_commit.take();
    let mut jump_to = None;
    let mut request_more = false;
    let has_more = !model.projects[index].log_reached_end;
    let total_rows = filtered.len();

    super::app_ui::region(ui, list_rect, "log-list", |ui| {
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
                    ui.painter()
                        .rect_filled(row, egui::CornerRadius::ZERO, p.active);
                } else if response.hovered() {
                    ui.painter()
                        .rect_filled(row, egui::CornerRadius::ZERO, p.hover);
                }
                let dim = out_of_head && !is_selected;

                // 1. DAG cell (100% continuous vertical lines, zero inter-row gaps)
                let cell_rect = Rect::from_min_size(row.min, Vec2::new(dag_width, row_h));
                draw_dag_cell(
                    ui.painter(),
                    cell_rect,
                    dag.rows.get(&commit.id),
                    dag.breaks.get(&commit.id),
                    commit,
                );

                // 2. Commit subject + Merge icon + Ref badges
                let mut msg_x = row.left() + dag_width + 6.0;
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

                let author_w = 110.0;
                let date_w = 110.0;
                let refs_w = commit
                    .refs
                    .iter()
                    .take(3)
                    .map(|r| r.label.chars().count() as f32 * 6.0 + 16.0)
                    .sum::<f32>();
                let msg_right = row.right() - author_w - date_w - refs_w - 12.0;

                text_clipped(
                    ui.painter(),
                    Rect::from_min_max(
                        Pos2::new(msg_x, row.top()),
                        Pos2::new(msg_right.max(msg_x + 60.0), row.bottom()),
                    ),
                    &commit.message,
                    FontId::proportional(12.5),
                    if dim {
                        p.text3.gamma_multiply(0.65)
                    } else if is_selected {
                        p.text
                    } else {
                        p.text2
                    },
                    Align2::LEFT_CENTER,
                );

                let mut badge_x = msg_right.max(msg_x + 60.0) + 4.0;
                for label in commit.refs.iter().take(3) {
                    let width = label.label.chars().count() as f32 * 6.0 + 12.0;
                    let ref_rect = Rect::from_min_size(
                        Pos2::new(badge_x, row.center().y - 8.0),
                        Vec2::new(width, 16.0),
                    );
                    let color = match label.kind {
                        RefKind::Head => p.accent,
                        RefKind::LocalBranch => Color32::from_rgb(0x3f, 0xb9, 0x50),
                        RefKind::RemoteBranch => Color32::from_rgb(0xa3, 0x71, 0xf7),
                        RefKind::Tag => Color32::from_rgb(0xe3, 0xb3, 0x41),
                    };
                    ui.painter().rect_filled(
                        ref_rect,
                        egui::CornerRadius::same(3),
                        color.gamma_multiply(0.18),
                    );
                    ui.painter().rect_stroke(
                        ref_rect,
                        egui::CornerRadius::same(3),
                        Stroke::new(1.0, color),
                        egui::StrokeKind::Inside,
                    );
                    ui.painter().text(
                        ref_rect.center(),
                        Align2::CENTER_CENTER,
                        &label.label,
                        FontId::monospace(10.0),
                        color,
                    );
                    badge_x += width + 4.0;
                }

                // 3. Author column
                let author_rect = Rect::from_min_max(
                    Pos2::new(row.right() - author_w - date_w, row.top()),
                    Pos2::new(row.right() - date_w - 8.0, row.bottom()),
                );
                text_clipped(
                    ui.painter(),
                    author_rect,
                    &commit.author,
                    FontId::proportional(11.5),
                    p.text3,
                    Align2::LEFT_CENTER,
                );

                // 4. Exact date column: YYYY/M/D HH:mm
                let date_str = format_commit_datetime(commit.time);
                ui.painter().text(
                    Pos2::new(row.right() - 10.0, row.center().y),
                    Align2::RIGHT_CENTER,
                    date_str,
                    FontId::monospace(11.0),
                    p.text3,
                );

                let mut arrow_hit: Option<&lazecode_git::dag::FoldArrowMark> = None;
                if let Some(brk) = dag.breaks.get(&commit.id) {
                    if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
                        for arrow in brk.below.iter().chain(brk.above.iter()) {
                            let is_down = matches!(arrow.dir, lazecode_git::dag::ArrowDir::Down);
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
                    let cmd_ctrl = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
                    let shift = ui.input(|i| i.modifiers.shift);
                    if cmd_ctrl {
                        if model.projects[index].selected_commits.contains(&commit.id) {
                            model.projects[index]
                                .selected_commits
                                .retain(|c| c != &commit.id);
                        } else {
                            model.projects[index]
                                .selected_commits
                                .push(commit.id.clone());
                        }
                        select_commit = Some(commit.id.clone());
                    } else if shift && !model.projects[index].selected_commits.is_empty() {
                        let anchor = model.projects[index].selected_commits[0].clone();
                        let pos_anchor = model.projects[index]
                            .log
                            .iter()
                            .position(|c| c.id == anchor)
                            .unwrap_or(0);
                        let pos_curr = model.projects[index]
                            .log
                            .iter()
                            .position(|c| c.id == commit.id)
                            .unwrap_or(0);
                        let start = pos_anchor.min(pos_curr);
                        let end = pos_anchor.max(pos_curr);
                        model.projects[index].selected_commits = model.projects[index].log
                            [start..=end]
                            .iter()
                            .map(|c| c.id.clone())
                            .collect();
                        select_commit = Some(commit.id.clone());
                    } else {
                        model.projects[index].selected_commits = vec![commit.id.clone()];
                        select_commit = Some(commit.id.clone());
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
        ui.ctx().request_repaint();
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
            .font(FontId::proportional(12.0)),
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
    let row_h = 22.0;

    // Sections: Local / Remote / Tags; the roots are pre-built above.
    let expanded_dirs: std::collections::HashSet<String> =
        model.projects[index].ref_dirs_expanded.clone();
    let mut toggle_dir: Option<String> = None;
    let mut pick_ref: Option<(String, bool)> = None; // (name, remote)
    let mut open_menu: Option<(String, bool, Rect)> = None; // branch name + remote flag + anchor

    let curr_branch = branches.iter().find(|b| b.name == current);
    let local_tree = lazecode_core::gitref::build_ref_tree(&local);
    let local_flat = lazecode_core::gitref::flatten_ref_tree(&local_tree, &expanded_dirs);
    let remote_tree = lazecode_core::gitref::build_ref_tree(&remote);
    let remote_flat = lazecode_core::gitref::flatten_ref_tree(&remote_tree, &expanded_dirs);

    let mut total_h = 24.0; // HEAD title
    if curr_branch.is_some() {
        total_h += row_h + 6.0;
    }
    total_h += 24.0 + local_flat.len() as f32 * row_h;
    if !remote.is_empty() {
        total_h += 24.0 + remote_flat.len() as f32 * row_h;
    }
    if !tags.is_empty() {
        total_h += 24.0 + tags.len() as f32 * row_h;
    }
    total_h += 24.0;

    super::app_ui::region(ui, rows_rect, "branch-tree-region", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("branch-tree-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let (container_rect, _) = ui
                    .allocate_exact_size(Vec2::new(ui.available_width(), total_h), Sense::hover());
                let mut y = container_rect.top() + 6.0;

                let draw_dir_row =
                    |ui: &mut Ui, label: &str, path: &str, expanded: bool, y: &mut f32| -> bool {
                        let row = Rect::from_min_size(
                            Pos2::new(container_rect.left() + 4.0, *y),
                            Vec2::new(container_rect.width() - 8.0, 20.0),
                        );
                        let response = ui.interact(
                            row,
                            ui.scope_id().with(("refdir", path.to_string())),
                            Sense::click(),
                        );
                        if response.hovered() {
                            ui.painter()
                                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
                        }
                        // chevron.
                        let cx = row.left() + 12.0;
                        let cy = row.center().y;
                        let color = p.text3;
                        let (dy, dx) = if expanded { (0.0, 3.0) } else { (-3.0, 0.0) };
                        ui.painter().line_segment(
                            [
                                Pos2::new(cx - 3.5 + dx, cy - 3.5 + dy),
                                Pos2::new(cx + 3.5 + dx, cy + 3.5 + dy),
                            ],
                            Stroke::new(1.4, color),
                        );
                        ui.painter().line_segment(
                            [
                                Pos2::new(cx + 3.5 + dx, cy + 3.5 + dy),
                                Pos2::new(cx - 3.5 + dx * 2.0, cy - 3.5 + dy * 2.0),
                            ],
                            Stroke::new(1.4, color),
                        );
                        ui.painter().text(
                            Pos2::new(row.left() + 26.0, row.center().y),
                            Align2::LEFT_CENTER,
                            label,
                            FontId::proportional(12.5),
                            p.text2,
                        );
                        *y += row_h;
                        response.clicked()
                    };

                // HEAD (Current Branch) section (Image #1)
                ui.painter().text(
                    Pos2::new(container_rect.left() + 12.0, y + 8.0),
                    Align2::LEFT_CENTER,
                    "HEAD (Current Branch)",
                    FontId::proportional(11.0),
                    p.text3,
                );
                y += 22.0;
                if let Some(curr) = curr_branch {
                    let row = Rect::from_min_size(
                        Pos2::new(container_rect.left() + 4.0, y),
                        Vec2::new(container_rect.width() - 8.0, 20.0),
                    );
                    let resp =
                        ui.interact(row, ui.scope_id().with("head-branch-row"), Sense::click());
                    if resp.hovered() {
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::same(3), p.hover);
                    }
                    let green = Color32::from_rgb(0x3f, 0xb9, 0x50);
                    paint_icon(
                        ui.painter(),
                        Rect::from_center_size(
                            Pos2::new(row.left() + 14.0, row.center().y),
                            Vec2::splat(12.0),
                        ),
                        Icon::Branch,
                        green,
                    );
                    ui.painter().text(
                        Pos2::new(row.left() + 26.0, row.center().y),
                        Align2::LEFT_CENTER,
                        &curr.name,
                        FontId::proportional(12.5),
                        green,
                    );
                    if curr.ahead > 0 {
                        ui.painter().text(
                            Pos2::new(row.right() - 8.0, row.center().y),
                            Align2::RIGHT_CENTER,
                            format!("↑{}", curr.ahead),
                            FontId::monospace(11.0),
                            green,
                        );
                    }
                    if resp.clicked() {
                        pick_ref = Some((curr.name.clone(), false));
                    }
                    if resp.secondary_clicked() {
                        let anchor = ui
                            .input(|i| i.pointer.hover_pos())
                            .map(|p| Rect::from_min_size(p, Vec2::splat(1.0)))
                            .unwrap_or(row);
                        open_menu = Some((curr.name.clone(), false, anchor));
                    }
                    y += row_h + 6.0;
                }

                // Local tree.
                ui.painter().text(
                    Pos2::new(container_rect.left() + 12.0, y + 8.0),
                    Align2::LEFT_CENTER,
                    crate::tr!(model, "Local", "本地"),
                    FontId::proportional(11.5),
                    p.text3,
                );
                y += 24.0;
                for row_ref in local_flat.iter() {
                    if row_ref.dir {
                        if draw_dir_row(
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
                            Pos2::new(container_rect.left() + 4.0, y),
                            Vec2::new(container_rect.width() - 8.0, 20.0),
                        );
                        let indent = 18.0 + 14.0 * row_ref.depth as f32;
                        let is_current = branch.name == current;
                        let is_fav = model.favorite_branches.contains(&branch.name);
                        let response = ui.interact(
                            row,
                            ui.scope_id().with(("refrow", branch.name.clone())),
                            Sense::click(),
                        );
                        let is_selected =
                            model.projects[index].ref_selection.contains(&branch.name);
                        if is_selected {
                            ui.painter()
                                .rect_filled(row, egui::CornerRadius::same(3), p.active);
                        } else if response.hovered() {
                            ui.painter()
                                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
                        }
                        // Branch knob: filled dot for HEAD.
                        if is_current {
                            ui.painter().circle_filled(
                                Pos2::new(row.left() + indent, row.center().y),
                                3.0,
                                p.accent,
                            );
                        } else {
                            ui.painter().circle_stroke(
                                Pos2::new(row.left() + indent, row.center().y),
                                3.0,
                                Stroke::new(1.3, p.text3),
                            );
                        }
                        let name_label = row_ref.label.clone();
                        ui.painter().text(
                            Pos2::new(row.left() + indent + 8.0, row.center().y),
                            Align2::LEFT_CENTER,
                            &name_label,
                            FontId::proportional(12.5),
                            if is_current { p.text } else { p.text2 },
                        );
                        // ahead/behind badge.
                        if branch.ahead > 0 {
                            let badge = format!("↑{}", branch.ahead);
                            ui.painter().text(
                                Pos2::new(
                                    row.right() - (if is_fav { 24.0 } else { 8.0 }),
                                    row.center().y,
                                ),
                                Align2::RIGHT_CENTER,
                                badge,
                                FontId::monospace(11.0),
                                Color32::from_rgb(0x3f, 0xb9, 0x50),
                            );
                        }
                        // Favorite star.
                        if is_fav {
                            let star_offset = if branch.ahead > 0 || branch.behind > 0 {
                                56.0
                            } else {
                                10.0
                            };
                            ui.painter().text(
                                Pos2::new(row.right() - star_offset, row.center().y),
                                Align2::RIGHT_CENTER,
                                "★",
                                FontId::proportional(10.0),
                                p.amber,
                            );
                        }
                        if response.secondary_clicked() {
                            if !model.projects[index].ref_selection.contains(&branch.name) {
                                model.projects[index].ref_selection.clear();
                                model.projects[index]
                                    .ref_selection
                                    .push(branch.name.clone());
                            }
                            let anchor = ui
                                .input(|i| i.pointer.hover_pos())
                                .map(|p| Rect::from_min_size(p, Vec2::splat(1.0)))
                                .unwrap_or(row);
                            open_menu = Some((branch.name.clone(), branch.remote, anchor));
                        }
                        if response.clicked() {
                            let cmd = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
                            let shift = ui.input(|i| i.modifiers.shift);
                            if cmd {
                                let selection = &mut model.projects[index].ref_selection;
                                if let Some(pos) = selection.iter().position(|n| n == &branch.name)
                                {
                                    selection.remove(pos);
                                } else {
                                    selection.push(branch.name.clone());
                                }
                            } else if shift && !model.projects[index].ref_selection.is_empty() {
                                let all_names: Vec<String> = local_flat
                                    .iter()
                                    .filter_map(|r| r.branch.as_ref().map(|b| b.name.clone()))
                                    .collect();
                                let anchor = model.projects[index]
                                    .ref_selection
                                    .first()
                                    .cloned()
                                    .unwrap_or_else(|| branch.name.clone());
                                let pos1 = all_names.iter().position(|n| n == &anchor).unwrap_or(0);
                                let pos2 = all_names
                                    .iter()
                                    .position(|n| n == &branch.name)
                                    .unwrap_or(0);
                                let start = pos1.min(pos2);
                                let end = pos1.max(pos2);
                                for n in &all_names[start..=end] {
                                    let selection = &mut model.projects[index].ref_selection;
                                    if !selection.contains(n) {
                                        selection.push(n.clone());
                                    }
                                }
                            } else {
                                model.projects[index].ref_selection.clear();
                                model.projects[index]
                                    .ref_selection
                                    .push(branch.name.clone());
                            }
                            pick_ref = Some((branch.name.clone(), branch.remote));
                        }
                        if response.double_clicked() {
                            model.queue(Command::CheckoutBranch(branch.name.clone()));
                        }
                        y += row_h;
                    }
                }

                // Remote section.
                if !remote.is_empty() {
                    ui.painter().text(
                        Pos2::new(container_rect.left() + 12.0, y + 8.0),
                        Align2::LEFT_CENTER,
                        crate::tr!(model, "Remote", "远程"),
                        FontId::proportional(9.5),
                        p.text3,
                    );
                    y += 24.0;
                    for row_ref in remote_flat.iter() {
                        if row_ref.dir {
                            if draw_dir_row(
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
                                Pos2::new(container_rect.left() + 4.0, y),
                                Vec2::new(container_rect.width() - 8.0, 20.0),
                            );
                            let indent = 18.0 + 14.0 * row_ref.depth as f32;
                            let response = ui.interact(
                                row,
                                ui.scope_id().with(("refrow", branch.name.clone())),
                                Sense::click(),
                            );
                            let is_selected =
                                model.projects[index].ref_selection.contains(&branch.name);
                            if is_selected {
                                ui.painter().rect_filled(
                                    row,
                                    egui::CornerRadius::same(3),
                                    p.active,
                                );
                            } else if response.hovered() {
                                ui.painter()
                                    .rect_filled(row, egui::CornerRadius::same(3), p.hover);
                            }
                            ui.painter().circle_stroke(
                                Pos2::new(row.left() + indent, row.center().y),
                                3.0,
                                Stroke::new(1.3, p.blue),
                            );
                            ui.painter().text(
                                Pos2::new(row.left() + indent + 8.0, row.center().y),
                                Align2::LEFT_CENTER,
                                &row_ref.label,
                                FontId::proportional(10.5),
                                p.text2,
                            );
                            if response.secondary_clicked() {
                                if !model.projects[index].ref_selection.contains(&branch.name) {
                                    model.projects[index].ref_selection.clear();
                                    model.projects[index]
                                        .ref_selection
                                        .push(branch.name.clone());
                                }
                                let anchor = ui
                                    .input(|i| i.pointer.hover_pos())
                                    .map(|p| Rect::from_min_size(p, Vec2::splat(1.0)))
                                    .unwrap_or(row);
                                open_menu = Some((branch.name.clone(), true, anchor));
                            }
                            if response.clicked() {
                                let cmd = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
                                if cmd {
                                    let selection = &mut model.projects[index].ref_selection;
                                    if let Some(pos) =
                                        selection.iter().position(|n| n == &branch.name)
                                    {
                                        selection.remove(pos);
                                    } else {
                                        selection.push(branch.name.clone());
                                    }
                                } else {
                                    model.projects[index].ref_selection.clear();
                                    model.projects[index]
                                        .ref_selection
                                        .push(branch.name.clone());
                                }
                                pick_ref = Some((branch.name.clone(), true));
                            }
                            y += row_h;
                        }
                    }
                }

                // Tags section.
                if !tags.is_empty() {
                    ui.painter().text(
                        Pos2::new(container_rect.left() + 12.0, y + 8.0),
                        Align2::LEFT_CENTER,
                        crate::tr!(model, "Tags", "标签"),
                        FontId::proportional(9.5),
                        p.text3,
                    );
                    y += 24.0;
                    for tag in tags.iter() {
                        let tag_target = tag_targets
                            .iter()
                            .find(|(name, _)| name == tag)
                            .map(|(_, target)| target.clone())
                            .unwrap_or_default();
                        let row = Rect::from_min_size(
                            Pos2::new(container_rect.left() + 4.0, y),
                            Vec2::new(container_rect.width() - 8.0, 20.0),
                        );
                        let response = ui.interact(
                            row,
                            ui.scope_id().with(("tagrow", tag.clone())),
                            Sense::click(),
                        );
                        if response.hovered() {
                            ui.painter()
                                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
                        }
                        ui.painter().text(
                            Pos2::new(row.left() + 18.0, row.center().y),
                            Align2::LEFT_CENTER,
                            "◇",
                            FontId::proportional(10.0),
                            p.amber,
                        );
                        ui.painter().text(
                            Pos2::new(row.left() + 32.0, row.center().y),
                            Align2::LEFT_CENTER,
                            tag,
                            FontId::proportional(10.5),
                            p.text2,
                        );
                        if !tag_target.is_empty() {
                            ui.painter().text(
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
            });
    });
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
    if let Some((name, remote, anchor)) = open_menu {
        rt.open_menu(
            crate::runtime::MenuKind::BranchContext { project: index },
            anchor,
            Some(format!(
                "{name}\u{1}{}",
                if remote { "remote" } else { "local" }
            )),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_commit_detail(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    details: &lazecode_git::CommitDetails,
    _rt: &mut UiRuntime,
    lang: lazecode_core::Language,
    index: usize,
) {
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.panel);
    ui.painter().vline(
        rect.left() + 0.5,
        rect.y_range(),
        Stroke::new(1.0, p.border_soft),
    );

    // Top Header (30.0px)
    let header_rect = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 30.0));
    ui.painter().rect_filled(header_rect, 0.0, p.panel);
    ui.painter().hline(
        header_rect.x_range(),
        header_rect.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );

    let count_text = if lang.is_zh() {
        format!("{} 个文件变更", details.files.len())
    } else {
        format!("{} files changed", details.files.len())
    };
    ui.painter().text(
        Pos2::new(header_rect.left() + 12.0, header_rect.center().y),
        Align2::LEFT_CENTER,
        count_text,
        FontId::proportional(11.5),
        p.text2,
    );

    // Top action buttons: Copy, CollapseAll, ExpandAll, Diff
    let mut rx = header_rect.right() - 6.0;
    // Copy hash button
    let copy_rect = Rect::from_min_size(
        Pos2::new(rx - 20.0, header_rect.top() + 5.0),
        Vec2::splat(20.0),
    );
    let copy_resp = icon_button(ui, copy_rect, Icon::Copy, false, theme);
    if copy_resp.hovered() {
        copy_resp.show_tooltip_text(crate::tr!(model, "Copy Hash", "复制提交哈希"));
    }
    if copy_resp.clicked() {
        ui.ctx().copy_text(details.info.id.clone());
        model.toast(
            crate::tr!(model, "Copied hash to clipboard", "已复制提交哈希到剪贴板"),
            lazecode_core::ToastKind::Info,
        );
    }
    rx -= 22.0;

    // Collapse all
    let col_rect = Rect::from_min_size(
        Pos2::new(rx - 20.0, header_rect.top() + 5.0),
        Vec2::splat(20.0),
    );
    let col_resp = icon_button(ui, col_rect, Icon::CollapseAll, false, theme);
    if col_resp.hovered() {
        col_resp.show_tooltip_text(crate::tr!(model, "Collapse All", "全部折叠"));
    }
    if col_resp.clicked() {
        model.projects[index].commit_file_dirs_expanded.clear();
    }
    rx -= 22.0;

    // Expand all
    let exp_rect = Rect::from_min_size(
        Pos2::new(rx - 20.0, header_rect.top() + 5.0),
        Vec2::splat(20.0),
    );
    let exp_resp = icon_button(ui, exp_rect, Icon::ExpandAll, false, theme);
    if exp_resp.hovered() {
        exp_resp.show_tooltip_text(crate::tr!(model, "Expand All", "全部展开"));
    }
    if exp_resp.clicked() {
        let tree = lazecode_git::build_commit_file_tree(&details.files);
        for path in lazecode_git::all_commit_file_dir_paths(&tree) {
            model.projects[index].commit_file_dirs_expanded.insert(path);
        }
    }
    rx -= 22.0;

    // Full commit diff button
    let diff_rect = Rect::from_min_size(
        Pos2::new(rx - 20.0, header_rect.top() + 5.0),
        Vec2::splat(20.0),
    );
    let diff_resp = icon_button(ui, diff_rect, Icon::Diff, false, theme);
    if diff_resp.hovered() {
        diff_resp.show_tooltip_text(crate::tr!(model, "View Commit Diff", "查看完整提交差异"));
    }
    if diff_resp.clicked() {
        model.queue(Command::OpenCommitDiff {
            commit: details.info.id.clone(),
            path: String::new(),
        });
    }

    // Bottom Summary Card (height = 96.0)
    let card_h = 96.0;
    let card_rect = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - card_h), rect.max);
    ui.painter().rect_filled(card_rect, 0.0, p.panel2);
    ui.painter().hline(
        card_rect.x_range(),
        card_rect.top() + 0.5,
        Stroke::new(1.0, p.border_soft),
    );

    // Card message
    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(card_rect.left() + 12.0, card_rect.top() + 8.0),
            Pos2::new(card_rect.right() - 12.0, card_rect.top() + 26.0),
        ),
        &details.info.message,
        FontId::proportional(12.5),
        p.text,
        Align2::LEFT_CENTER,
    );
    if !details.info.body.is_empty() {
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(card_rect.left() + 12.0, card_rect.top() + 26.0),
                Pos2::new(card_rect.right() - 12.0, card_rect.top() + 44.0),
            ),
            details.info.body.lines().next().unwrap_or(""),
            FontId::proportional(11.0),
            p.text3,
            Align2::LEFT_CENTER,
        );
    }

    // Card blue short hash pill
    let hash_pill_rect = Rect::from_min_size(
        Pos2::new(card_rect.left() + 12.0, card_rect.bottom() - 32.0),
        Vec2::new(68.0, 18.0),
    );
    let hash_resp = ui.interact(
        hash_pill_rect,
        ui.scope_id().with("card-hash-pill"),
        Sense::click(),
    );
    let pill_color = Color32::from_rgb(0x58, 0xa6, 0xff); // Blue
    ui.painter().rect_filled(
        hash_pill_rect,
        egui::CornerRadius::same(4),
        pill_color.gamma_multiply(0.18),
    );
    ui.painter().text(
        hash_pill_rect.center(),
        Align2::CENTER_CENTER,
        &details.info.short,
        FontId::monospace(10.5),
        pill_color,
    );
    if hash_resp.hovered() {
        hash_resp.show_tooltip_text(crate::tr!(model, "Click to copy hash", "点击复制哈希"));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if hash_resp.clicked() {
        ui.ctx().copy_text(details.info.id.clone());
        model.toast(
            crate::tr!(model, "Copied hash to clipboard", "已复制提交哈希到剪贴板"),
            lazecode_core::ToastKind::Info,
        );
    }

    // Author & Date
    ui.painter().text(
        Pos2::new(hash_pill_rect.right() + 8.0, hash_pill_rect.center().y),
        Align2::LEFT_CENTER,
        format!("{} <{}>", details.info.author, details.info.email),
        FontId::proportional(10.5),
        p.text3,
    );
    ui.painter().text(
        Pos2::new(card_rect.left() + 12.0, card_rect.bottom() - 10.0),
        Align2::LEFT_CENTER,
        format_commit_datetime(details.info.time),
        FontId::monospace(10.5),
        p.text3,
    );

    // Changed Files Tree Area (middle area)
    let files_rect = Rect::from_min_max(
        Pos2::new(rect.left(), header_rect.bottom()),
        Pos2::new(rect.right(), card_rect.top()),
    );
    let tree = lazecode_git::build_commit_file_tree(&details.files);
    let flat_files = lazecode_git::flatten_commit_file_tree(
        &tree,
        &model.projects[index].commit_file_dirs_expanded,
        0,
    );

    super::app_ui::region(ui, files_rect, "commit-files-tree", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("commit-files")
            .auto_shrink([false, false])
            .show_rows(ui, 20.0, flat_files.len(), |ui, range| {
                for i in range {
                    let item = &flat_files[i];
                    let (row, response) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::click());
                    if response.hovered() {
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::same(3), p.hover);
                    }
                    let indent = 12.0 + item.depth as f32 * 14.0;
                    if item.is_dir {
                        let is_expanded = !model.projects[index]
                            .commit_file_dirs_expanded
                            .contains(&item.path);
                        paint_icon(
                            ui.painter(),
                            Rect::from_center_size(
                                Pos2::new(row.left() + indent, row.center().y),
                                Vec2::splat(10.0),
                            ),
                            if is_expanded {
                                Icon::ChevronDown
                            } else {
                                Icon::ChevronRight
                            },
                            p.text3,
                        );
                        paint_icon(
                            ui.painter(),
                            Rect::from_center_size(
                                Pos2::new(row.left() + indent + 12.0, row.center().y),
                                Vec2::splat(12.0),
                            ),
                            if is_expanded {
                                Icon::FolderOpen
                            } else {
                                Icon::Folder
                            },
                            p.accent,
                        );
                        ui.painter().text(
                            Pos2::new(row.left() + indent + 24.0, row.center().y),
                            Align2::LEFT_CENTER,
                            &item.name,
                            FontId::proportional(12.0),
                            p.text,
                        );
                        ui.painter().text(
                            Pos2::new(row.right() - 8.0, row.center().y),
                            Align2::RIGHT_CENTER,
                            format!("{} files", item.total_files),
                            FontId::monospace(9.5),
                            p.text3,
                        );
                        if response.clicked() {
                            if is_expanded {
                                model.projects[index]
                                    .commit_file_dirs_expanded
                                    .insert(item.path.clone());
                            } else {
                                model.projects[index]
                                    .commit_file_dirs_expanded
                                    .remove(&item.path);
                            }
                        }
                    } else if let Some(file) = &item.file {
                        paint_icon(
                            ui.painter(),
                            Rect::from_center_size(
                                Pos2::new(row.left() + indent + 10.0, row.center().y),
                                Vec2::splat(12.0),
                            ),
                            Icon::File,
                            p.text3,
                        );
                        text_clipped(
                            ui.painter(),
                            Rect::from_min_max(
                                Pos2::new(row.left() + indent + 22.0, row.top()),
                                Pos2::new(row.right() - 65.0, row.bottom()),
                            ),
                            &item.name,
                            FontId::proportional(12.0),
                            if response.hovered() { p.text } else { p.text2 },
                            Align2::LEFT_CENTER,
                        );
                        let stat_str = if file.additions > 0 && file.deletions > 0 {
                            format!("+{} -{}", file.additions, file.deletions)
                        } else if file.additions > 0 {
                            format!("+{}", file.additions)
                        } else {
                            format!("-{}", file.deletions)
                        };
                        let stat_color = if file.additions > 0 && file.deletions == 0 {
                            Color32::from_rgb(0x3f, 0xb9, 0x50)
                        } else if file.deletions > 0 && file.additions == 0 {
                            Color32::from_rgb(0xf8, 0x51, 0x49)
                        } else {
                            p.text3
                        };
                        ui.painter().text(
                            Pos2::new(row.right() - 8.0, row.center().y),
                            Align2::RIGHT_CENTER,
                            stat_str,
                            FontId::monospace(10.0),
                            stat_color,
                        );
                        if response.clicked() {
                            model.queue(Command::OpenCommitDiff {
                                commit: details.info.id.clone(),
                                path: file.path.clone(),
                            });
                        }
                        if response.secondary_clicked() {
                            model.queue(Command::OpenCompareWithLocalDiff {
                                commit: details.info.id.clone(),
                                path: file.path.clone(),
                            });
                        }
                    }
                }
            });
    });
}

#[allow(dead_code)]
pub(crate) fn time_ago(seconds: i64, zh: bool) -> String {
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
