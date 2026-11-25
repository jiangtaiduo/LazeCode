//! Bottom panel: terminal dock and the Git panel (log, branches, commit
//! details), split into modules.

mod branches;
mod commit;
mod git_log;
mod prelude;

pub(crate) use commit::format_commit_datetime;
pub(crate) use git_log::draw_dag_cell;

use git_log::*;
use prelude::*;
/// Lay out wrapped text into `rect`, capped at `max_rows`, and clip it.
/// Returns the painted galley height so the next block can flow below it.
pub(super) fn paint_wrapped_text(
    ui: &Ui,
    rect: Rect,
    text: &str,
    font: FontId,
    color: Color32,
    max_rows: usize,
) -> f32 {
    if text.is_empty() || rect.width() <= 1.0 || rect.height() <= 1.0 || max_rows == 0 {
        return 0.0;
    }
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = rect.width();
    job.wrap.max_rows = max_rows;
    job.break_on_newline = true;
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: font,
            color,
            ..Default::default()
        },
    );
    let galley = ui.ctx().fonts_mut(|fonts| fonts.layout_job(job));
    let height = galley.size().y.min(rect.height());
    ui.painter()
        .with_clip_rect(rect)
        .galley(rect.left_top(), galley, color);
    height
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
        let resize = rt_ref.terminal_resize.entry(*pane_id).or_default();
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
            resize,
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
