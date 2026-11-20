use std::collections::HashSet;

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2};
use lazecode_core::{AppModel, Command};
use lazecode_git::{
    lane_color, lane_x, CommitInfo, GraphBreak, GraphRowLayout, GRAPH_BREAK_GAP, GRAPH_H, LANE_W,
    NODE_R,
};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::UiRuntime;
use crate::theme::Theme;
use crate::widgets::{icon_button, text_clipped};

/// 稳定版 commitDate：今天 → "Today HH:MM"，昨天 → "Yesterday HH:MM"，
/// 其余 → "YYYY/M/D HH:MM"（月/日不补零，时分补零）。
pub(crate) fn format_commit_datetime(seconds: i64) -> String {
    unsafe {
        let t = seconds as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return format!("{seconds}");
        }
        let clock = format!("{:02}:{:02}", tm.tm_hour, tm.tm_min);
        let now = libc::time(std::ptr::null_mut());
        let mut local: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut local).is_null() {
            return format!(
                "{}/{}/{} {}",
                tm.tm_year + 1900,
                tm.tm_mon + 1,
                tm.tm_mday,
                clock
            );
        }
        let same_day = local.tm_year == tm.tm_year && local.tm_yday == tm.tm_yday;
        let yesterday = {
            let mut y = local;
            y.tm_mday -= 1;
            libc::mktime(&mut y);
            y.tm_year == tm.tm_year && y.tm_yday == tm.tm_yday
        };
        if same_day {
            format!("Today {clock}")
        } else if yesterday {
            format!("Yesterday {clock}")
        } else {
            format!(
                "{}/{}/{} {}",
                tm.tm_year + 1900,
                tm.tm_mon + 1,
                tm.tm_mday,
                clock
            )
        }
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
const HEAD_H: f32 = 28.0;

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.panel);
    let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), HEAD_H));
    ui.painter().rect_filled(head, 0.0, p.panel);
    ui.painter().hline(
        head.x_range(),
        head.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    let body = Rect::from_min_max(Pos2::new(rect.left(), head.bottom()), rect.max);
    draw_git_panel(ui, model, rt, theme, head, body);
}

pub fn draw_terminal_dock(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
) {
    let p = theme.palette;
    // 稳定版 `.terminal-header-bar`（30px，双击最大化）。
    let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 30.0));
    ui.painter().rect_filled(head, 0.0, p.titlebar);
    ui.painter().hline(
        head.x_range(),
        head.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    let bar = ui.interact(head, ui.scope_id().with("term-head-bar"), Sense::click());
    if bar.double_clicked() {
        model.ui.terminal_maximised = !model.ui.terminal_maximised;
        model.save_workspace();
    }
    let Some(index) = model.active_project else {
        draw_empty(ui, theme, rect, "No project open");
        return;
    };
    // 一页一个标签：标题取活动分屏，激活态 = 拥有活动分屏的标签。
    let tabs: Vec<(u64, String, u64, bool)> = {
        let worktree = model.projects[index].active_worktree();
        let active_tab = worktree.active_tab_id();
        worktree
            .terminals
            .iter()
            .map(|tab| {
                (
                    tab.id,
                    tab.title().to_string(),
                    tab.active_pane,
                    Some(tab.id) == active_tab,
                )
            })
            .collect()
    };

    // 页签：底对齐 26px；激活 = 终端底色 + 2px accent 顶边（稳定版 tab-item）。
    let mut close_tab = None;
    let mut x = head.left() + 6.0;
    for (id, title, active_pane, is_active) in &tabs {
        let width = (title.chars().count() as f32 * 6.2 + 46.0).clamp(84.0, 178.0);
        let tab = Rect::from_min_size(Pos2::new(x, head.bottom() - 26.0), Vec2::new(width, 26.0));
        let close_rect_inner = Rect::from_center_size(
            Pos2::new(tab.right() - 12.0, tab.center().y),
            Vec2::splat(14.0),
        );
        let response = ui.interact(
            Rect::from_min_max(tab.min, Pos2::new(close_rect_inner.left(), tab.max.y)),
            ui.scope_id().with(("ctab", id)),
            Sense::click(),
        );
        let close_response = ui.interact(
            close_rect_inner,
            ui.scope_id().with(("ctab-close", id)),
            Sense::click(),
        );
        if *is_active {
            ui.painter().rect_filled(
                tab,
                egui::CornerRadius {
                    nw: 4,
                    ne: 4,
                    se: 0,
                    sw: 0,
                },
                p.terminal_bg,
            );
            ui.painter().rect_filled(
                Rect::from_min_max(tab.min, Pos2::new(tab.right(), tab.top() + 2.0)),
                egui::CornerRadius::ZERO,
                p.accent,
            );
        } else if response.hovered() || close_response.hovered() {
            ui.painter().rect_filled(
                tab,
                egui::CornerRadius {
                    nw: 4,
                    ne: 4,
                    se: 0,
                    sw: 0,
                },
                p.hover,
            );
        }
        // `>_` 等宽 10px accent（稳定版 tab-term-icon）。
        ui.painter().text(
            Pos2::new(tab.left() + 10.0, tab.center().y),
            Align2::LEFT_CENTER,
            ">_",
            FontId::monospace(10.0),
            p.accent,
        );
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(tab.left() + 26.0, tab.top()),
                Pos2::new(tab.right() - 21.0, tab.bottom()),
            ),
            title,
            FontId::proportional(11.0),
            if *is_active { p.text_bright } else { p.text2 },
            Align2::LEFT_CENTER,
        );
        if close_response.hovered() {
            ui.painter()
                .circle_filled(close_rect_inner.center(), 7.0, p.hover);
        }
        if close_response.hovered() || *is_active {
            ui.painter().text(
                close_rect_inner.center(),
                Align2::CENTER_CENTER,
                "×",
                FontId::proportional(10.0),
                if close_response.hovered() {
                    p.text_bright
                } else {
                    p.text3
                },
            );
        }
        if close_response.clicked() || response.middle_clicked() {
            close_tab = Some(*id);
        } else if response.clicked() {
            model.projects[index].active_worktree_mut().active_terminal = Some(*active_pane);
            rt.set_terminal_focus(*active_pane);
            model.ui.terminal_visible = true;
        } else if response.secondary_clicked() {
            rt.open_menu(
                crate::runtime::MenuKind::TerminalTabContext {
                    project: index,
                    tab_id: *id,
                    terminal_id: *active_pane,
                },
                response.rect,
                Some(title.clone()),
            );
        }
        x += width + 2.0;
    }
    // 新建按钮（22×22，底对齐）。
    if x + 22.0 < head.right() - 120.0 {
        let plus =
            Rect::from_center_size(Pos2::new(x + 11.0, head.bottom() - 15.0), Vec2::splat(22.0));
        let response = ui.interact(plus, ui.scope_id().with("new-console"), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(plus, egui::CornerRadius::same(6), p.hover);
        }
        paint_icon(ui.painter(), plus.shrink(4.5), Icon::Plus, p.text2);
        if response.clicked() {
            model.queue(Command::NewTerminal);
        }
    }
    // 右侧动作：停靠切换（占位）/ 最大化 / 关闭。
    let mut rx = head.right() - 6.0;
    let close_rect =
        Rect::from_center_size(Pos2::new(rx - 10.0, head.center().y), Vec2::splat(20.0));
    let close_tooltip = crate::tr!(model, "Close", "关闭");
    if icon_button(ui, close_rect, Icon::X, false, theme)
        .on_hover_text(close_tooltip)
        .clicked()
    {
        model.ui.terminal_visible = false;
    }
    rx -= 24.0;
    let max_icon = if model.ui.terminal_maximised {
        Icon::Minimize
    } else {
        Icon::Maximize
    };
    let max_tooltip = if model.ui.terminal_maximised {
        crate::tr!(model, "Restore height", "还原高度")
    } else {
        crate::tr!(model, "Maximize height", "最大化")
    };
    let max_rect = Rect::from_center_size(Pos2::new(rx - 10.0, head.center().y), Vec2::splat(20.0));
    if icon_button(ui, max_rect, max_icon, model.ui.terminal_maximised, theme)
        .on_hover_text(max_tooltip)
        .clicked()
    {
        model.ui.terminal_maximised = !model.ui.terminal_maximised;
        model.save_workspace();
    }
    rx -= 24.0;
    // 停靠右侧/底部切换：右侧时显示 DockBottom，底部时显示 DockRight
    let is_right = model.ui.terminal_placement == lazecode_core::TerminalPlacement::Right;
    let dock_rect =
        Rect::from_center_size(Pos2::new(rx - 10.0, head.center().y), Vec2::splat(20.0));
    let dock_icon = if is_right {
        Icon::DockBottom
    } else {
        Icon::DockRight
    };
    let dock_tooltip = if is_right {
        crate::tr!(model, "Move to bottom", "停靠到底部")
    } else {
        crate::tr!(model, "Dock to right", "停靠到右侧")
    };
    let dock_resp = icon_button(ui, dock_rect, dock_icon, false, theme).on_hover_text(dock_tooltip);
    if dock_resp.clicked() {
        model.ui.terminal_placement = if is_right {
            lazecode_core::TerminalPlacement::Bottom
        } else {
            lazecode_core::TerminalPlacement::Right
        };
        model.save_workspace();
    }
    rx -= 24.0;
    let agent_rect =
        Rect::from_center_size(Pos2::new(rx - 10.0, head.center().y), Vec2::splat(20.0));
    if icon_button(ui, agent_rect, Icon::Spark, rt.agent_menu.is_some(), theme)
        .on_hover_text(crate::tr!(model, "Coding Agent", "编码智能体"))
        .clicked()
    {
        rt.toggle_agent_menu(agent_rect);
    }
    if let Some(tab_id) = close_tab {
        rt.request_terminal_close(model, crate::runtime::TerminalCloseAction::Tab(tab_id));
    }

    // 终端画布：稳定版 `.terminal-canvas-host`（12px 14px 内边距，--term-bg）。
    let body = Rect::from_min_max(Pos2::new(rect.left(), head.bottom()), rect.max);
    ui.painter().rect_filled(body, 0.0, p.terminal_bg);
    let content = Rect::from_min_max(
        Pos2::new(body.left() + 14.0, body.top() + 12.0),
        Pos2::new(body.right() - 14.0, body.bottom() - 12.0),
    );
    // Overlays own the keyboard; don't feed their keystrokes to the shell.
    let overlays_open = rt.blocks_input(model);
    let font_size = model.settings.terminal.font_size;
    let project = &mut model.projects[index];
    let Some(tab_id) = project.active_worktree().active_tab_id() else {
        draw_empty(ui, theme, body, "No consoles open");
        return;
    };
    let Some((pane_ids, split, active_pane)) =
        project.active_worktree().tab_by_id(tab_id).map(|tab| {
            (
                tab.panes.iter().map(|pane| pane.id).collect::<Vec<_>>(),
                tab.split,
                tab.active_pane,
            )
        })
    else {
        draw_empty(ui, theme, body, "No consoles open");
        return;
    };
    if pane_ids.is_empty() {
        draw_empty(ui, theme, body, "No consoles open");
        return;
    }
    if rt.terminal_focus.is_none() && rt.editor_focus.is_none() {
        rt.set_terminal_focus(active_pane);
    }
    // 1 个分屏铺满；row = 左右均分，column = 上下均分（稳定版 flex 布局）。
    let pane_rects = split_pane_rects(content, pane_ids.len(), split);
    for area in pane_rects.iter().skip(1) {
        match split {
            lazecode_core::PaneSplit::Column => {
                ui.painter().hline(
                    content.x_range(),
                    area.top() - 0.5,
                    Stroke::new(1.0, p.border_soft),
                );
            }
            _ => {
                ui.painter().vline(
                    area.left() - 0.5,
                    content.y_range(),
                    Stroke::new(1.0, p.border_soft),
                );
            }
        }
    }
    let multi = pane_ids.len() > 1;
    let mut activate: Option<u64> = None;
    let mut close_pane: Option<u64> = None;
    for (slot, pane_id) in pane_ids.iter().enumerate() {
        let area = pane_rects[slot];
        let is_active = *pane_id == active_pane;
        let focus = is_active && rt.terminal_focus == Some(*pane_id) && !overlays_open;
        let Some(terminal) = project.term_by_id(*pane_id) else {
            continue;
        };
        let session = &terminal.session;
        let rt_ref = &mut *rt;
        let snapshot = rt_ref.terminal_snapshots.entry(*pane_id).or_default();
        let selection = rt_ref.terminal_selections.entry(*pane_id).or_insert(None);
        let search = rt_ref.terminal_search.entry(*pane_id).or_default();
        let preedit = rt_ref.terminal_preedit.entry(*pane_id).or_default();
        let wheel_accum = rt_ref.terminal_wheel.entry(*pane_id).or_insert(0.0);
        let graphics = rt_ref.terminal_graphics.entry(*pane_id).or_default();
        let wake_counter = session.wake_counter();
        let last_wake = rt_ref.terminal_wake.entry(*pane_id).or_insert(u64::MAX);
        let refresh = *last_wake != wake_counter;
        let output = crate::terminal::draw(
            ui,
            session,
            snapshot,
            selection,
            search,
            focus,
            preedit,
            wheel_accum,
            graphics,
            refresh,
            theme,
            area,
            *pane_id,
            font_size,
        );
        // A synchronized update or resize settle holds the last complete
        // frame; leave the wake unconsumed so the retry takes a snapshot.
        if refresh && !output.refresh_pending {
            *last_wake = wake_counter;
        }
        if output.focus_requested {
            activate = Some(*pane_id);
        }
        if multi {
            // 活动分屏焦点提示：只在分屏时出现（稳定版 `.terminal-pane.active::after`）。
            if is_active {
                ui.painter().rect_stroke(
                    area,
                    egui::CornerRadius::ZERO,
                    Stroke::new(1.0, p.accent.gamma_multiply(0.55)),
                    StrokeKind::Inside,
                );
            }
            // 分屏关闭按钮：悬停分屏时出现，避免常驻遮挡终端内容。
            let close_rect = Rect::from_min_size(
                Pos2::new(area.right() - 22.0, area.top() + 4.0),
                Vec2::splat(16.0),
            );
            let close_response = ui.interact(
                close_rect,
                ui.scope_id().with(("pane-close", *pane_id)),
                Sense::click(),
            );
            if ui.rect_contains_pointer(area) || close_response.hovered() {
                ui.painter()
                    .rect_filled(close_rect, egui::CornerRadius::same(3), p.titlebar);
                ui.painter().rect_stroke(
                    close_rect,
                    egui::CornerRadius::same(3),
                    Stroke::new(
                        1.0,
                        if close_response.hovered() {
                            p.accent
                        } else {
                            p.border
                        },
                    ),
                    StrokeKind::Inside,
                );
                ui.painter().text(
                    close_rect.center(),
                    Align2::CENTER_CENTER,
                    "×",
                    FontId::proportional(10.0),
                    if close_response.hovered() {
                        p.text_bright
                    } else {
                        p.text3
                    },
                );
            }
            if close_response.clicked() {
                close_pane = Some(*pane_id);
            }
        }
    }
    if let Some(pane_id) = close_pane {
        rt.request_terminal_close(model, crate::runtime::TerminalCloseAction::Pane(pane_id));
    } else if let Some(pane_id) = activate {
        let worktree = project.active_worktree_mut();
        if let Some(tab) = worktree
            .terminals
            .iter_mut()
            .find(|tab| tab.pane(pane_id).is_some())
        {
            tab.active_pane = pane_id;
        }
        worktree.active_terminal = Some(pane_id);
        rt.set_terminal_focus(pane_id);
    }
    // OSC titles are not mapped onto tab names: a shell alias must not rename
    // the user-visible console tab.
}

/// Equal-size pane areas with a 1px gutter between neighbours.
fn split_pane_rects(content: Rect, count: usize, split: lazecode_core::PaneSplit) -> Vec<Rect> {
    if count <= 1 {
        return vec![content];
    }
    const GAP: f32 = 1.0;
    let n = count as f32;
    let mut rects = Vec::with_capacity(count);
    match split {
        lazecode_core::PaneSplit::Column => {
            let height = ((content.height() - GAP * (n - 1.0)) / n).max(1.0);
            for index in 0..count {
                let top = content.top() + index as f32 * (height + GAP);
                rects.push(Rect::from_min_max(
                    Pos2::new(content.left(), top),
                    Pos2::new(content.right(), top + height),
                ));
            }
        }
        lazecode_core::PaneSplit::Row | lazecode_core::PaneSplit::None => {
            let width = ((content.width() - GAP * (n - 1.0)) / n).max(1.0);
            for index in 0..count {
                let left = content.left() + index as f32 * (width + GAP);
                rects.push(Rect::from_min_max(
                    Pos2::new(left, content.top()),
                    Pos2::new(left + width, content.bottom()),
                ));
            }
        }
    }
    rects
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
    let is_log = model.ui.git_subtab == lazecode_core::GitSubtab::Log;

    // 1. Log subtab
    let log_rect = Rect::from_min_size(Pos2::new(tab_x, head.top() + 5.0), Vec2::new(44.0, 22.0));
    let log_resp = ui.interact(
        log_rect,
        ui.scope_id().with("git-subtab-log"),
        Sense::click(),
    );
    if is_log {
        ui.painter()
            .rect_filled(log_rect, egui::CornerRadius::same(6), p.hover);
        ui.painter().rect_stroke(
            log_rect,
            egui::CornerRadius::same(6),
            Stroke::new(1.0, p.border_soft),
            StrokeKind::Inside,
        );
    } else if log_resp.hovered() {
        ui.painter().rect_filled(
            log_rect,
            egui::CornerRadius::same(6),
            p.hover.gamma_multiply(0.5),
        );
    }
    ui.painter().text(
        log_rect.center(),
        Align2::CENTER_CENTER,
        crate::tr!(model, "Log", "日志"),
        FontId::proportional(12.0),
        if is_log { p.text } else { p.text3 },
    );
    if log_resp.clicked() {
        model.ui.git_subtab = lazecode_core::GitSubtab::Log;
    }
    tab_x += 48.0;

    // 2. Console subtab (Git commands log)
    let console_rect =
        Rect::from_min_size(Pos2::new(tab_x, head.top() + 5.0), Vec2::new(66.0, 22.0));
    let console_resp = ui.interact(
        console_rect,
        ui.scope_id().with("git-subtab-console"),
        Sense::click(),
    );
    let is_console = model.ui.git_subtab == lazecode_core::GitSubtab::Console;
    if is_console {
        ui.painter()
            .rect_filled(console_rect, egui::CornerRadius::same(6), p.hover);
        ui.painter().rect_stroke(
            console_rect,
            egui::CornerRadius::same(6),
            Stroke::new(1.0, p.border_soft),
            StrokeKind::Inside,
        );
    } else if console_resp.hovered() {
        ui.painter().rect_filled(
            console_rect,
            egui::CornerRadius::same(6),
            p.hover.gamma_multiply(0.5),
        );
    }
    ui.painter().text(
        console_rect.center(),
        Align2::CENTER_CENTER,
        "Console",
        FontId::proportional(12.0),
        if is_console { p.text } else { p.text3 },
    );
    if console_resp.clicked() {
        model.ui.git_subtab = lazecode_core::GitSubtab::Console;
    }
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
    if is_console {
        draw_git_console(ui, model, theme, body);
    } else {
        draw_git_log(ui, model, rt, theme, body, index);
    }
}

fn draw_git_console(ui: &mut Ui, model: &AppModel, theme: &Theme, body: Rect) {
    let p = theme.palette;
    ui.painter().rect_filled(body, 0.0, p.terminal_bg);
    super::app_ui::region(ui, body.shrink(10.0), "git-console-region", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("git-console-scroll")
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if model.git_cmd_log.is_empty() {
                    let (row, _) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::hover());
                    ui.painter().text(
                        row.left_top(),
                        Align2::LEFT_TOP,
                        crate::tr!(
                            model,
                            "No Git commands recorded in this session yet.",
                            "暂无 Git 调用记录"
                        ),
                        FontId::monospace(11.5),
                        p.text3,
                    );
                } else {
                    for line in &model.git_cmd_log {
                        let (row, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 18.0),
                            Sense::hover(),
                        );
                        ui.painter().text(
                            row.left_top(),
                            Align2::LEFT_TOP,
                            line,
                            FontId::monospace(11.0),
                            p.text,
                        );
                    }
                }
            });
    });
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
    let show_detail = model.ui.git_details_visible && details.is_some() && body.width() > 700.0;
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
            format!("{} 个提交...", log.len())
        } else {
            format!("{} commits...", log.len())
        }
    } else if model.settings.language.is_zh() {
        format!("{} 个提交", log.len())
    } else {
        format!("{} commits", log.len())
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
    let authors: Vec<String> = {
        let mut set = std::collections::BTreeSet::new();
        for commit in log.iter() {
            set.insert(commit.author.clone());
        }
        set.into_iter().take(24).collect()
    };
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
    let is_starred = star_target
        .as_ref()
        .is_some_and(|name| model.projects[index].favorites.contains(name));
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
            if model.projects[index].favorites.contains(&target) {
                model.projects[index].favorites.remove(&target);
            } else {
                model.projects[index].favorites.insert(target);
            }
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
    tb_y += 6.0;

    // 10. Jump Folding Arrow toggle (≡→)
    let jump_rect = Rect::from_min_size(
        Pos2::new(toolbar_rect.left() + 3.0, tb_y),
        Vec2::splat(24.0),
    );
    let jump_resp = icon_button(ui, jump_rect, Icon::ShelveIn, true, theme);
    if jump_resp.hovered() {
        jump_resp.show_tooltip_text(crate::tr!(model, "Jump to Linked Commit", "跳转到关联提交"));
    }
    tb_y += 25.0;

    // 11. View Options / Gear (👁)
    let view_opt_rect = Rect::from_min_size(
        Pos2::new(toolbar_rect.left() + 3.0, tb_y),
        Vec2::splat(24.0),
    );
    let view_opt_resp = icon_button(ui, view_opt_rect, Icon::ViewOptions, false, theme);
    if view_opt_resp.hovered() {
        view_opt_resp.show_tooltip_text(crate::tr!(model, "View Options", "视图选项"));
    }
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
            body.y_range(),
            if is_active_split {
                Stroke::new(2.0, p.accent)
            } else {
                Stroke::new(1.0, p.border_soft)
            },
        );
    }

    let date_since = match model.projects[index].log_query.date {
        lazecode_core::DatePreset::All => (model.projects[index].log_query.since > 0)
            .then_some(model.projects[index].log_query.since),
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
    let author_filter = model.projects[index].log_query.author.trim().to_string();

    let filtered: Vec<CommitInfo> = log
        .into_iter()
        .filter(|commit| {
            if let Some(since) = date_since {
                if commit.time < since {
                    return false;
                }
            }
            if !author_filter.is_empty() && !commit.author.eq_ignore_ascii_case(&author_filter) {
                return false;
            }
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

    let show_graph = model.ui.git_log_show_graph;
    let dag = if show_graph {
        lazecode_git::compute_dag(&filtered)
    } else {
        lazecode_git::dag::DagLayout::default()
    };
    let dag_width = if show_graph {
        (10.0 + (dag.max_lane + 1) as f32 * LANE_W).max(40.0)
    } else {
        4.0
    };
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
                        let project = &mut model.projects[index];
                        select_commit = apply_log_click(
                            &filtered,
                            &mut project.selected_commits,
                            &mut project.selected_commit,
                            &commit.id,
                            false,
                        );
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
    _tag_targets: &[(String, String)],
) {
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.panel);
    ui.painter().vline(
        rect.right() - 0.5,
        rect.y_range(),
        Stroke::new(1.0, p.border_soft),
    );

    // 分支搜索框：稳定版 branchSearchOpen，🔍 工具条按钮切换显示。
    let search_open = model.ui.branch_search_open;
    let search_h = if search_open { 26.0 } else { 4.0 };
    let search_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 6.0, rect.top() + 5.0),
        Vec2::new(rect.width() - 12.0, 22.0),
    );
    if search_open {
        ui.painter()
            .rect_filled(search_rect, egui::CornerRadius::same(4), p.scrim_subtle);
        ui.painter().rect_stroke(
            search_rect,
            egui::CornerRadius::same(4),
            Stroke::new(1.0, p.border_soft),
            StrokeKind::Inside,
        );
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
                .align(egui::Align2::LEFT_CENTER)
                .hint_text(crate::tr!(model, "Filter branches", "过滤分支"))
                .font(FontId::proportional(11.0)),
        );
        if response.changed() {
            model.projects[index].branch_query = query_value;
        }
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
        Pos2::new(rect.left(), search_rect.top() + search_h),
        Pos2::new(rect.right(), rect.bottom()),
    );
    let row_h = 22.0;

    // Sections: Local / Remote / Tags; the roots are pre-built above.
    let expanded_dirs: std::collections::HashSet<String> =
        model.projects[index].ref_dirs_expanded.clone();
    let mut toggle_dir: Option<String> = None;
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

                // HEAD (Current Branch)：纯标签行（稳定版 b-head-label，
                // 22px、padding-left 24、weight 500）；当前分支行在 Local 组里。
                ui.painter().text(
                    Pos2::new(container_rect.left() + 24.0, y + 8.0),
                    Align2::LEFT_CENTER,
                    "HEAD (Current Branch)",
                    FontId::proportional(12.0),
                    p.text2,
                );
                y += 22.0;
                // Local tree.
                ui.painter().text(
                    Pos2::new(container_rect.left() + 12.0, y + 8.0),
                    Align2::LEFT_CENTER,
                    crate::tr!(model, "Local", "本地"),
                    FontId::proportional(12.5),
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
                        let indent = 10.0 + 12.0 * row_ref.depth as f32;
                        let is_current = branch.name == current;
                        let is_fav = model.favorite_branches.contains(&branch.name);
                        let response = ui.interact(
                            row,
                            ui.scope_id().with(("refrow", branch.name.clone())),
                            Sense::click(),
                        );
                        let is_selected =
                            model.projects[index].ref_selection.contains(&branch.name);
                        // b-item：margin 1px 4px、radius 4；选中 #26415f。
                        let row = row.shrink2(Vec2::new(2.0, 1.0));
                        if is_selected {
                            ui.painter().rect_filled(
                                row,
                                egui::CornerRadius::same(4),
                                p.selected_row,
                            );
                        } else if response.hovered() {
                            ui.painter()
                                .rect_filled(row, egui::CornerRadius::same(4), p.hover);
                        }
                        // 图标规则（renderRefRow）：收藏→黄星；tag/当前→黄 tag；
                        // 远程→云；本地→绿分支。
                        let green = Color32::from_rgb(0x3f, 0xb9, 0x50);
                        let amber = Color32::from_rgb(0xe3, 0xb3, 0x41);
                        let (icon, tint, isize) = if is_fav && !is_current {
                            (Icon::Star, amber, 12.0)
                        } else if is_current {
                            (Icon::Tag, amber, 12.0)
                        } else {
                            (Icon::Branch, green, 13.0)
                        };
                        paint_icon(
                            ui.painter(),
                            Rect::from_center_size(
                                Pos2::new(row.left() + indent + 6.0, row.center().y),
                                Vec2::splat(isize),
                            ),
                            icon,
                            tint,
                        );
                        let name_label = row_ref.label.clone();
                        ui.painter().text(
                            Pos2::new(row.left() + indent + 18.0, row.center().y),
                            Align2::LEFT_CENTER,
                            &name_label,
                            FontId::proportional(12.5),
                            if is_current { p.text_bright } else { p.text },
                        );
                        // b-up-arrow：↑n ↓n 绿色 10px，margin-left auto。
                        let mut badge = String::new();
                        if branch.ahead > 0 {
                            badge.push_str(&format!("↑{}", branch.ahead));
                        }
                        if branch.behind > 0 {
                            badge.push_str(&format!(" ↓{}", branch.behind));
                        }
                        if !badge.is_empty() {
                            ui.painter().text(
                                Pos2::new(row.right() - 6.0, row.center().y),
                                Align2::RIGHT_CENTER,
                                badge,
                                FontId::proportional(11.0),
                                green,
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
                            // 分支树成为键盘焦点：编辑器/控制台让位，Cmd+C
                            // 复制选中分支全名。
                            rt.editor_focus = None;
                            rt.terminal_focus = None;
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
                        FontId::proportional(12.5),
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
                            let indent = 10.0 + 12.0 * row_ref.depth as f32;
                            let response = ui.interact(
                                row,
                                ui.scope_id().with(("refrow", branch.name.clone())),
                                Sense::click(),
                            );
                            let is_selected =
                                model.projects[index].ref_selection.contains(&branch.name);
                            let row = row.shrink2(Vec2::new(2.0, 1.0));
                            if is_selected {
                                ui.painter().rect_filled(
                                    row,
                                    egui::CornerRadius::same(4),
                                    p.selected_row,
                                );
                            } else if response.hovered() {
                                ui.painter()
                                    .rect_filled(row, egui::CornerRadius::same(4), p.hover);
                            }
                            // 远程分支：云图标 muted（renderRefRow）。
                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(row.left() + indent + 6.0, row.center().y),
                                    Vec2::splat(13.0),
                                ),
                                Icon::Cloud,
                                p.text3,
                            );
                            ui.painter().text(
                                Pos2::new(row.left() + indent + 18.0, row.center().y),
                                Align2::LEFT_CENTER,
                                &row_ref.label,
                                FontId::proportional(12.5),
                                p.text,
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
                                rt.editor_focus = None;
                                rt.terminal_focus = None;
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
                            }
                            y += row_h;
                        }
                    }
                }

                // Tags section（b-group-row + tag 行：黄 tag 图标 + 11px 名字）。
                if !tags.is_empty() {
                    ui.painter().text(
                        Pos2::new(container_rect.left() + 12.0, y + 8.0),
                        Align2::LEFT_CENTER,
                        crate::tr!(model, "Tags", "标签"),
                        FontId::proportional(12.5),
                        p.text2,
                    );
                    y += 24.0;
                    for tag in tags.iter() {
                        let row = Rect::from_min_size(
                            Pos2::new(container_rect.left() + 4.0, y),
                            Vec2::new(container_rect.width() - 8.0, 20.0),
                        );
                        let response = ui.interact(
                            row,
                            ui.scope_id().with(("tagrow", tag.clone())),
                            Sense::click(),
                        );
                        let is_selected = model.projects[index].ref_selection.contains(tag);
                        let row = row.shrink2(Vec2::new(2.0, 1.0));
                        if is_selected {
                            ui.painter().rect_filled(
                                row,
                                egui::CornerRadius::same(4),
                                p.selected_row,
                            );
                        } else if response.hovered() {
                            ui.painter()
                                .rect_filled(row, egui::CornerRadius::same(4), p.hover);
                        }
                        paint_icon(
                            ui.painter(),
                            Rect::from_center_size(
                                Pos2::new(row.left() + 16.0, row.center().y),
                                Vec2::splat(12.0),
                            ),
                            Icon::Tag,
                            p.amber,
                        );
                        ui.painter().text(
                            Pos2::new(row.left() + 28.0, row.center().y),
                            Align2::LEFT_CENTER,
                            tag,
                            FontId::proportional(12.5),
                            p.text,
                        );
                        y += row_h;
                    }
                }
            });
    });
    // Cmd+C / Ctrl+C：复制选中分支的全名（保留 a/b 路径；多选按行拼接）。
    // 文本面（编辑器/终端）持有焦点时让位给它们自己的复制。
    let copy_event = ui.input(|input| {
        input
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::Copy))
    });
    if copy_event && rt.editor_focus.is_none() && rt.terminal_focus.is_none() {
        let selection = model.projects[index].ref_selection.clone();
        if !selection.is_empty() {
            ui.ctx().copy_text(selection.join("\n"));
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
    rt: &mut UiRuntime,
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

    // Top Header (30.0px) - exactly matching stable details-files-header
    let header_rect = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 30.0));
    ui.painter().rect_filled(header_rect, 0.0, p.panel);
    ui.painter().hline(
        header_rect.x_range(),
        header_rect.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );

    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(header_rect.left() + 14.0, header_rect.center().y),
            Vec2::splat(13.0),
        ),
        Icon::File,
        p.text3,
    );
    let count_text = if details.files.len() == 1 {
        if lang.is_zh() {
            "1 个文件变更".to_string()
        } else {
            "1 file changed".to_string()
        }
    } else {
        if lang.is_zh() {
            format!("{} 个文件变更", details.files.len())
        } else {
            format!("{} files changed", details.files.len())
        }
    };
    ui.painter().text(
        Pos2::new(header_rect.left() + 26.0, header_rect.center().y),
        Align2::LEFT_CENTER,
        count_text,
        FontId::proportional(12.5),
        p.text,
    );

    // Top action buttons from right to left (stable order: CollapseAll, ExpandAll, ViewOptions, Diff, Undo):
    let mut rx = header_rect.right() - 6.0;

    // 5. Collapse all
    let col_rect = Rect::from_min_size(
        Pos2::new(rx - 20.0, header_rect.top() + 5.0),
        Vec2::splat(20.0),
    );
    let col_resp = icon_button(ui, col_rect, Icon::CollapseAll, false, theme);
    if col_resp.hovered() {
        col_resp.show_tooltip_text(crate::tr!(model, "Collapse All", "全部折叠"));
    }
    if col_resp.clicked() {
        let tree = lazecode_git::build_commit_file_tree(&details.files);
        for path in lazecode_git::all_commit_file_dir_paths(&tree) {
            model.projects[index]
                .commit_file_dirs_collapsed
                .insert(path);
        }
        model.projects[index].commit_files_root_collapsed = true;
    }
    rx -= 22.0;

    // 4. Expand all
    let exp_rect = Rect::from_min_size(
        Pos2::new(rx - 20.0, header_rect.top() + 5.0),
        Vec2::splat(20.0),
    );
    let exp_resp = icon_button(ui, exp_rect, Icon::ExpandAll, false, theme);
    if exp_resp.hovered() {
        exp_resp.show_tooltip_text(crate::tr!(model, "Expand All", "全部展开"));
    }
    if exp_resp.clicked() {
        model.projects[index].commit_files_root_collapsed = false;
        // The set stores collapsed directories; an empty set expands all.
        model.projects[index].commit_file_dirs_collapsed.clear();
    }
    rx -= 22.0;

    // 3. View options
    let view_rect = Rect::from_min_size(
        Pos2::new(rx - 20.0, header_rect.top() + 5.0),
        Vec2::splat(20.0),
    );
    let view_resp = icon_button(ui, view_rect, Icon::ViewOptions, false, theme);
    if view_resp.hovered() {
        view_resp.show_tooltip_text(crate::tr!(model, "View Options", "视图选项"));
    }
    rx -= 22.0;

    // 2. Full commit diff
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
    rx -= 22.0;

    // 1. Revert commit (Undo)
    let revert_rect = Rect::from_min_size(
        Pos2::new(rx - 20.0, header_rect.top() + 5.0),
        Vec2::splat(20.0),
    );
    let revert_resp = icon_button(ui, revert_rect, Icon::Undo, false, theme);
    if revert_resp.hovered() {
        revert_resp.show_tooltip_text(crate::tr!(model, "Revert Commit", "回滚提交"));
    }
    if revert_resp.clicked() {
        model.queue(Command::RevertCommit {
            hash: details.info.id.clone(),
        });
    }

    // Bottom Summary Card and Draggable Resizer (height clamped between 60.0 and panel height - 90.0)
    let card_h = model
        .ui
        .git_details_card_height
        .clamp(60.0, (rect.height() - 90.0).max(60.0));
    let split_rect = Rect::from_min_size(
        Pos2::new(rect.left(), rect.bottom() - card_h - 4.0),
        Vec2::new(rect.width(), 6.0),
    );
    let split_resp = ui.interact(
        split_rect,
        ui.scope_id().with("details-split-resizer"),
        Sense::drag(),
    );
    if split_resp.hovered() || split_resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if split_resp.dragged() {
        model.ui.git_details_card_height = (model.ui.git_details_card_height
            - split_resp.drag_delta().y)
            .clamp(60.0, (rect.height() - 90.0).max(60.0));
    }

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
        FontId::proportional(13.0),
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
            FontId::proportional(12.0),
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
        FontId::monospace(11.5),
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
        FontId::proportional(12.0),
        p.text3,
    );
    ui.painter().text(
        Pos2::new(card_rect.left() + 12.0, card_rect.bottom() - 10.0),
        Align2::LEFT_CENTER,
        format_commit_datetime(details.info.time),
        FontId::monospace(12.0),
        p.text3,
    );

    // Changed Files Tree Area (middle area)
    let files_rect = Rect::from_min_max(
        Pos2::new(rect.left(), header_rect.bottom()),
        Pos2::new(rect.right(), split_rect.top()),
    );
    let tree = lazecode_git::build_commit_file_tree(&details.files);
    let flat_files = lazecode_git::flatten_commit_file_tree(
        &tree,
        &model.projects[index].commit_file_dirs_collapsed,
        0,
    );

    super::app_ui::region(ui, files_rect, "commit-files-tree", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("commit-files")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Root Project Row (exactly matching stable details-tree-root)
                let root_collapsed = model.projects[index].commit_files_root_collapsed;
                let (root_row, root_resp) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click());
                if root_resp.hovered() {
                    ui.painter()
                        .rect_filled(root_row, egui::CornerRadius::same(3), p.hover);
                }
                paint_icon(
                    ui.painter(),
                    Rect::from_center_size(
                        Pos2::new(root_row.left() + 10.0, root_row.center().y),
                        Vec2::splat(10.0),
                    ),
                    if root_collapsed {
                        Icon::ChevronRight
                    } else {
                        Icon::ChevronDown
                    },
                    p.text3,
                );
                paint_icon(
                    ui.painter(),
                    Rect::from_center_size(
                        Pos2::new(root_row.left() + 24.0, root_row.center().y),
                        Vec2::splat(13.0),
                    ),
                    if root_collapsed {
                        Icon::Folder
                    } else {
                        Icon::FolderOpen
                    },
                    Color32::from_rgb(0xdc, 0xd7, 0xaa),
                );
                let root_name = model.projects[index].name.clone();
                text_clipped(
                    ui.painter(),
                    Rect::from_min_max(
                        Pos2::new(root_row.left() + 36.0, root_row.top()),
                        Pos2::new(root_row.right() - 50.0, root_row.bottom()),
                    ),
                    &root_name,
                    FontId::proportional(12.5),
                    p.text,
                    Align2::LEFT_CENTER,
                );
                ui.painter().text(
                    Pos2::new(root_row.right() - 8.0, root_row.center().y),
                    Align2::RIGHT_CENTER,
                    format!(
                        "{} {}",
                        details.files.len(),
                        if details.files.len() == 1 {
                            "file"
                        } else {
                            "files"
                        }
                    ),
                    FontId::proportional(11.0),
                    p.text3,
                );
                if root_resp.clicked() {
                    model.projects[index].commit_files_root_collapsed = !root_collapsed;
                }

                if !root_collapsed {
                    for item in &flat_files {
                        let (row, response) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 20.0),
                            Sense::click(),
                        );
                        if response.hovered() {
                            ui.painter()
                                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
                        }
                        let indent = 8.0 + (item.depth + 1) as f32 * 12.0;
                        if item.is_dir {
                            let is_expanded = !model.projects[index]
                                .commit_file_dirs_collapsed
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
                                Color32::from_rgb(0xdc, 0xd7, 0xaa),
                            );
                            ui.painter().text(
                                Pos2::new(row.left() + indent + 24.0, row.center().y),
                                Align2::LEFT_CENTER,
                                &item.name,
                                FontId::proportional(12.5),
                                p.text,
                            );
                            ui.painter().text(
                                Pos2::new(row.right() - 8.0, row.center().y),
                                Align2::RIGHT_CENTER,
                                format!(
                                    "{} {}",
                                    item.total_files,
                                    if item.total_files == 1 {
                                        "file"
                                    } else {
                                        "files"
                                    }
                                ),
                                FontId::proportional(10.5),
                                p.text3,
                            );
                            if response.clicked() {
                                if is_expanded {
                                    model.projects[index]
                                        .commit_file_dirs_collapsed
                                        .insert(item.path.clone());
                                } else {
                                    model.projects[index]
                                        .commit_file_dirs_collapsed
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
                                crate::sidebar::file_tone(&item.name, theme),
                            );
                            let name_color = match file.kind {
                                lazecode_git::ChangeKind::Added => {
                                    Color32::from_rgb(0x73, 0xc9, 0x91)
                                }
                                lazecode_git::ChangeKind::Deleted => {
                                    Color32::from_rgb(0xe0, 0x6c, 0x75)
                                }
                                _ => Color32::from_rgb(0x58, 0xa6, 0xff),
                            };
                            text_clipped(
                                ui.painter(),
                                Rect::from_min_max(
                                    Pos2::new(row.left() + indent + 22.0, row.top()),
                                    Pos2::new(row.right() - 72.0, row.bottom()),
                                ),
                                &item.name,
                                FontId::proportional(12.5),
                                if response.hovered() {
                                    p.text
                                } else {
                                    name_color
                                },
                                Align2::LEFT_CENTER,
                            );
                            let mut sx = row.right() - 8.0;
                            if file.deletions > 0 {
                                let del_str = format!("-{}", file.deletions);
                                let gal = ui.painter().layout_no_wrap(
                                    del_str,
                                    FontId::monospace(10.5),
                                    Color32::from_rgb(0xe0, 0x6c, 0x75),
                                );
                                let del_w = gal.size().x;
                                ui.painter().galley(
                                    Pos2::new(sx - del_w, row.center().y - gal.size().y * 0.5),
                                    gal,
                                    Color32::from_rgb(0xe0, 0x6c, 0x75),
                                );
                                sx -= del_w + 4.0;
                            }
                            if file.additions > 0 {
                                let add_str = format!("+{}", file.additions);
                                let gal = ui.painter().layout_no_wrap(
                                    add_str,
                                    FontId::monospace(10.5),
                                    Color32::from_rgb(0x73, 0xc9, 0x91),
                                );
                                ui.painter().galley(
                                    Pos2::new(
                                        sx - gal.size().x,
                                        row.center().y - gal.size().y * 0.5,
                                    ),
                                    gal,
                                    Color32::from_rgb(0x73, 0xc9, 0x91),
                                );
                            }
                            if response.double_clicked() {
                                model.queue(Command::OpenCommitDiff {
                                    commit: details.info.id.clone(),
                                    path: file.path.clone(),
                                });
                            }
                            if response.secondary_clicked() {
                                rt.open_menu(
                                    crate::runtime::MenuKind::CommitDetailsFileContext {
                                        project: index,
                                    },
                                    row,
                                    Some(format!("{}\u{1}{}", details.info.id, file.path)),
                                );
                            }
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
