//! Consoles sidebar: session list, agent picker and output summary.

use super::prelude::*;
use super::{side_head, SideAction, FOOT_H, HEAD_H};

pub(super) fn consoles(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
) {
    let p = theme.palette;
    let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), HEAD_H));
    if let Some(action) = side_head(
        ui,
        theme,
        head,
        crate::tr!(model, "Consoles", "控制台"),
        &[
            SideAction {
                icon: Icon::Spark,
                id: "agent",
                active: false,
                tooltip: crate::tr!(model, "Coding Agent", "编码智能体").to_string(),
            },
            SideAction {
                icon: Icon::Plus,
                id: "new",
                active: false,
                tooltip: crate::tr!(model, "New Terminal", "新建终端").to_string(),
            },
        ],
    ) {
        match action {
            0 => {
                rt.toggle_agent_menu(Rect::from_min_size(
                    Pos2::new(head.right() - 36.0, head.bottom()),
                    Vec2::splat(1.0),
                ));
            }
            1 => model.queue(Command::NewTerminal),
            _ => {}
        }
    }
    let list = Rect::from_min_max(Pos2::new(rect.left(), head.bottom()), rect.max);
    let total = rt.busy_count();

    #[derive(Clone)]
    struct ConsoleRow {
        project: usize,
        worktree: usize,
        terminal: u64,
        name: String,
        command: String,
        busy: bool,
        outputting: bool,
        elapsed: String,
    }
    struct WorktreeGroup {
        name: String,
        key: String,
        outputting: usize,
        rows: Vec<ConsoleRow>,
    }
    struct ProjectGroup {
        name: String,
        key: String,
        outputting: usize,
        worktrees: Vec<WorktreeGroup>,
    }
    let mut groups: Vec<ProjectGroup> = Vec::new();
    for (project_index, project) in model.projects.iter().enumerate() {
        let mut group = ProjectGroup {
            name: project.name.clone(),
            key: format!("p:{}", project.root.display()),
            outputting: 0,
            worktrees: Vec::new(),
        };
        for (worktree_index, worktree) in project.worktrees.iter().enumerate() {
            let mut worktree_group = WorktreeGroup {
                name: worktree.name.clone(),
                key: format!("w:{}", worktree.path.display()),
                outputting: 0,
                rows: Vec::new(),
            };
            for terminal in worktree.terminals.iter().flat_map(|tab| tab.panes.iter()) {
                // 与项目条 marquee 同源的状态：busy = 有前台进程，
                // outputting = console_outputting() 判定的“正在输出”。
                let state = rt.terminal_busy.get(&terminal.id);
                let busy = state.is_some_and(|state| state.busy);
                let outputting = busy && state.is_some_and(|state| state.outputting);
                // 运行中把标题换成探针看到的前台进程名（如 omp），空闲回落。
                let name = match state.filter(|state| state.busy && !state.commands.is_empty()) {
                    Some(state) => state.commands.join(" "),
                    None => terminal.title.clone(),
                };
                worktree_group.rows.push(ConsoleRow {
                    project: project_index,
                    worktree: worktree_index,
                    terminal: terminal.id,
                    name,
                    command: if let Some(command) = terminal.spawn_command.clone() {
                        command
                    } else if terminal.title != terminal.session.command_label {
                        terminal.session.command_label.clone()
                    } else {
                        crate::tr!(model, "default shell", "默认 shell").to_string()
                    },
                    busy,
                    outputting,
                    elapsed: format_elapsed(terminal.started.elapsed().as_secs()),
                });
            }
            worktree_group.outputting = worktree_group
                .rows
                .iter()
                .filter(|row| row.outputting)
                .count();
            group.outputting += worktree_group.outputting;
            group.worktrees.push(worktree_group);
        }
        groups.push(group);
    }

    crate::app_ui::region(ui, list, "consoles", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("console-list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    live_dot(
                        ui.painter(),
                        Pos2::new(ui.cursor().left() + 3.0, ui.cursor().center().y),
                        total > 0,
                        if total > 0 { p.running } else { p.text3 },
                    );
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(if model.settings.language.is_zh() {
                            format!("{} 个进程运行于 {} 个项目", total, model.projects.len())
                        } else {
                            let process = if total == 1 { "process" } else { "processes" };
                            let project = if model.projects.len() == 1 {
                                "project"
                            } else {
                                "projects"
                            };
                            format!(
                                "{total} {process} running across {} {project}",
                                model.projects.len()
                            )
                        })
                        .size(10.5)
                        .color(p.text3),
                    );
                });
                ui.add_space(6.0);
                // 项目 → 工作树 → 控制台：两级分组，组头点击折叠，默认全展开。
                for group in &groups {
                    let collapsed = rt.consoles_collapsed.contains(&group.key);
                    let (head_rect, head) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
                    if head.hovered() {
                        ui.painter().rect_filled(
                            head_rect.shrink2(Vec2::new(6.0, 1.0)),
                            egui::CornerRadius::same(4),
                            p.hover,
                        );
                    }
                    if head.clicked() {
                        if collapsed {
                            rt.consoles_collapsed.remove(&group.key);
                        } else {
                            rt.consoles_collapsed.insert(group.key.clone());
                        }
                    }
                    paint_icon(
                        ui.painter(),
                        Rect::from_center_size(
                            Pos2::new(head_rect.left() + 16.0, head_rect.center().y),
                            Vec2::splat(14.0),
                        ),
                        if collapsed {
                            Icon::ChevronRight
                        } else {
                            Icon::ChevronDown
                        },
                        p.text3,
                    );
                    ui.painter().text(
                        Pos2::new(head_rect.left() + 28.0, head_rect.center().y),
                        Align2::LEFT_CENTER,
                        &group.name,
                        FontId::proportional(11.5),
                        p.text,
                    );
                    group_summary(
                        ui,
                        model,
                        head_rect.right() - 10.0,
                        head_rect.center().y,
                        group.outputting,
                        p,
                    );
                    if collapsed {
                        continue;
                    }
                    for worktree in &group.worktrees {
                        let collapsed = rt.consoles_collapsed.contains(&worktree.key);
                        let (sub_rect, sub) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 22.0),
                            Sense::click(),
                        );
                        if sub.hovered() {
                            ui.painter().rect_filled(
                                sub_rect.shrink2(Vec2::new(6.0, 1.0)),
                                egui::CornerRadius::same(4),
                                p.hover,
                            );
                        }
                        if sub.clicked() {
                            if collapsed {
                                rt.consoles_collapsed.remove(&worktree.key);
                            } else {
                                rt.consoles_collapsed.insert(worktree.key.clone());
                            }
                        }
                        paint_icon(
                            ui.painter(),
                            Rect::from_center_size(
                                Pos2::new(sub_rect.left() + 30.0, sub_rect.center().y),
                                Vec2::splat(12.0),
                            ),
                            if collapsed {
                                Icon::ChevronRight
                            } else {
                                Icon::ChevronDown
                            },
                            p.text3,
                        );
                        ui.painter().text(
                            Pos2::new(sub_rect.left() + 44.0, sub_rect.center().y),
                            Align2::LEFT_CENTER,
                            &worktree.name,
                            FontId::proportional(10.5),
                            p.text2,
                        );
                        group_summary(
                            ui,
                            model,
                            sub_rect.right() - 10.0,
                            sub_rect.center().y,
                            worktree.outputting,
                            p,
                        );
                        if collapsed {
                            continue;
                        }
                        for row in &worktree.rows {
                            let (row_rect, response) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 44.0),
                                Sense::click(),
                            );
                            let active = model.active_project == Some(row.project)
                                && model.projects[row.project].worktrees[row.worktree]
                                    .active_terminal
                                    == Some(row.terminal);
                            if active {
                                ui.painter().rect_filled(
                                    row_rect.shrink2(Vec2::new(6.0, 2.0)),
                                    egui::CornerRadius::same(4),
                                    p.active,
                                );
                            } else if response.hovered() {
                                ui.painter().rect_filled(
                                    row_rect.shrink2(Vec2::new(6.0, 2.0)),
                                    egui::CornerRadius::same(4),
                                    p.hover,
                                );
                            }
                            // 状态：空闲灰点 / 运行中绿对勾 / 输出中红圈旋转。
                            let glyph = Pos2::new(row_rect.left() + 46.0, row_rect.center().y);
                            if row.outputting {
                                paint_output_spinner(ui, glyph, 5.0, p.rose);
                            } else if row.busy {
                                paint_icon(
                                    ui.painter(),
                                    Rect::from_center_size(glyph, Vec2::splat(14.0)),
                                    Icon::Check,
                                    p.green,
                                );
                            } else {
                                live_dot(ui.painter(), glyph, false, p.text3);
                            }
                            text_clipped(
                                ui.painter(),
                                Rect::from_min_max(
                                    Pos2::new(row_rect.left() + 56.0, row_rect.top() + 4.0),
                                    Pos2::new(row_rect.right() - 62.0, row_rect.top() + 24.0),
                                ),
                                &row.name,
                                FontId::proportional(11.5),
                                p.text,
                                Align2::LEFT_CENTER,
                            );
                            text_clipped(
                                ui.painter(),
                                Rect::from_min_max(
                                    Pos2::new(row_rect.left() + 56.0, row_rect.top() + 25.0),
                                    Pos2::new(row_rect.right() - 62.0, row_rect.bottom() - 3.0),
                                ),
                                &row.command,
                                FontId::monospace(9.0),
                                p.text3,
                                Align2::LEFT_CENTER,
                            );
                            ui.painter().text(
                                Pos2::new(row_rect.right() - 12.0, row_rect.center().y),
                                Align2::RIGHT_CENTER,
                                &row.elapsed,
                                FontId::monospace(9.0),
                                if row.busy { p.text2 } else { p.text3 },
                            );
                            if response.clicked() {
                                model.switch_project(row.project);
                                model.projects[row.project].active_worktree = row.worktree;
                                let worktree =
                                    &mut model.projects[row.project].worktrees[row.worktree];
                                if let Some(tab) = worktree
                                    .terminals
                                    .iter_mut()
                                    .find(|tab| tab.pane(row.terminal).is_some())
                                {
                                    tab.active_pane = row.terminal;
                                }
                                worktree.active_terminal = Some(row.terminal);
                                model.ui.panel_visible = true;
                                model.ui.terminal_visible = true;
                                model.ui.panel_visible = false;
                                rt.clear_terminal_cache();
                                rt.set_terminal_focus(row.terminal);
                            }
                        }
                    }
                }
                ui.add_space(10.0);
            });
    });

    let foot = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - FOOT_H), rect.max);
    ui.painter().rect_filled(foot, 0.0, p.sidebar);
    ui.painter().hline(
        foot.x_range(),
        foot.top() + 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    ui.painter().text(
        Pos2::new(foot.left() + 12.0, foot.center().y),
        Align2::LEFT_CENTER,
        crate::tr!(
            model,
            "Each console stays with its project.",
            "每个控制台都跟随其项目。"
        ),
        FontId::proportional(9.5),
        p.text3,
    );
}

/// 组头右侧的状态摘要：只统计控制台条目前面红色转圈（正在输出）的数量。
/// 没有红圈时显示灰色 `idle`。
fn group_summary(
    ui: &Ui,
    model: &AppModel,
    right: f32,
    center_y: f32,
    outputting: usize,
    p: crate::theme::Palette,
) {
    let (text, color) = if outputting > 0 {
        (
            if model.settings.language.is_zh() {
                format!("{outputting} 个运行中")
            } else {
                format!("{outputting} running")
            },
            p.rose,
        )
    } else {
        (crate::tr!(model, "idle", "空闲").to_string(), p.text3)
    };
    let font = FontId::proportional(9.5);
    let width = ui
        .painter()
        .layout_no_wrap(text.clone(), font.clone(), color)
        .size()
        .x;
    ui.painter().text(
        Pos2::new(right, center_y),
        Align2::RIGHT_CENTER,
        &text,
        font,
        color,
    );
    if outputting > 0 {
        paint_output_spinner(ui, Pos2::new(right - width - 11.0, center_y), 4.5, p.rose);
    }
}

/// 红色旋转圆弧，语义与项目条 marquee 的“正在输出”一致；按 30 fps 重绘。
fn paint_output_spinner(ui: &Ui, center: Pos2, radius: f32, color: Color32) {
    let time = ui.ctx().input(|input| input.time) as f32;
    let start = (time * 1.8).rem_euclid(std::f32::consts::TAU);
    let sweep = std::f32::consts::TAU * 0.72;
    let steps = 20;
    let points: Vec<Pos2> = (0..=steps)
        .map(|step| {
            let angle = start + sweep * step as f32 / steps as f32;
            center + Vec2::new(angle.cos() * radius, angle.sin() * radius)
        })
        .collect();
    ui.painter()
        .add(egui::Shape::line(points, Stroke::new(1.8, color)));
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(33));
}

fn format_elapsed(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {:02}m", seconds / 3600, (seconds % 3600) / 60)
    }
}
