use egui::{
    Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, ViewportCommand,
};
use lazecode_core::{AppModel, Command};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::{MenuKind, UiRuntime};
use crate::theme::Theme;
use crate::widgets::{icon_button, text_clipped};

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    let painter = ui.painter().clone();
    let top = rect.top();

    // Window dragging on the empty titlebar surface.
    let drag_rect = Rect::from_min_max(rect.min, Pos2::new(rect.left() + 6.0, rect.bottom()));
    let drag = ui.interact(
        drag_rect,
        ui.scope_id().with("title-drag-left"),
        Sense::click_and_drag(),
    );
    if drag.dragged() {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if drag.double_clicked() {
        let maximized = ui
            .ctx()
            .input(|input| input.viewport().maximized.unwrap_or(false));
        ui.ctx()
            .send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    }

    // macOS traffic lights overlay the first ~70px of the full-width
    // titlebar (stable `padding-left: 78px`).
    let mut x = rect.left() + 78.0;
    let mut open_project_dialog = false;

    // App logo → command palette. (The stable logo opens its project menu;
    // that lands with the titlebar replication batch.)
    let logo_rect = Rect::from_center_size(Pos2::new(x + 14.0, top + 20.0), Vec2::splat(28.0));
    let logo_response = ui.interact(logo_rect, ui.scope_id().with("title-logo"), Sense::click());
    let logo_fill = if logo_response.hovered() {
        p.accent.gamma_multiply(1.06)
    } else {
        p.accent
    };
    painter.rect_filled(logo_rect, egui::CornerRadius::same(8), logo_fill);
    painter.text(
        logo_rect.center(),
        Align2::CENTER_CENTER,
        ">_",
        FontId::monospace(12.0),
        p.accent_on_fill,
    );
    if logo_response.clicked() {
        rt.open_palette();
        model.ui.command_palette_open = true;
    }
    logo_response.on_hover_text(crate::tr!(model, "Command palette · ⌘⇧A", "命令面板 · ⌘⇧A"));
    x = logo_rect.right() + 10.0;

    // 多项目模式下展示其它项目的切换头像；单项目时直接展示项目胶囊，避免重复绘制双重头像。
    if model.projects.len() > 1 {
        for index in 0..model.projects.len() {
            let (tone, active, running, initials, project_name, project_branch) = {
                let project = &model.projects[index];
                (
                    Color32::from_rgb(project.tone.0, project.tone.1, project.tone.2),
                    model.active_project == Some(index),
                    project
                        .worktrees
                        .iter()
                        .flat_map(|worktree| worktree.terminals.iter())
                        .flat_map(|tab| tab.panes.iter())
                        .filter(|pane| pane.session.is_running())
                        .count(),
                    project.initials.clone(),
                    project.name.clone(),
                    project.branch(),
                )
            };
            let tile = Rect::from_center_size(Pos2::new(x + 12.0, top + 20.0), Vec2::splat(24.0));
            let response = ui.interact(
                tile,
                ui.scope_id().with(("title-project", index)),
                Sense::click(),
            );
            if active {
                painter.rect_filled(tile, egui::CornerRadius::same(7), tone);
            } else {
                painter.rect_filled(
                    tile,
                    egui::CornerRadius::same(7),
                    tone.gamma_multiply(if response.hovered() { 0.24 } else { 0.15 }),
                );
                painter.rect_stroke(
                    tile,
                    egui::CornerRadius::same(7),
                    Stroke::new(1.0, tone.gamma_multiply(0.35)),
                    StrokeKind::Inside,
                );
            }
            painter.text(
                tile.center(),
                Align2::CENTER_CENTER,
                &initials,
                FontId::proportional(10.0),
                if active { p.accent_on_fill } else { tone },
            );
            if running > 0 {
                painter.circle_filled(
                    Pos2::new(tile.right() - 1.0, tile.top() + 1.0),
                    4.0,
                    p.running,
                );
            }
            if response.clicked() {
                model.switch_project(index);
                rt.clear_terminal_cache();
            }
            response.on_hover_text(format!("{project_name} · {project_branch}"));
            x += 28.0;
        }
        x += 6.0;
    }

    let project = model.active_project();
    let has_project = project.is_some();
    let project_name = project
        .map(|project| project.name.clone())
        .unwrap_or_else(|| crate::tr!(model, "Open a project", "打开项目").to_string());
    let project_path = project
        .map(|project| lazecode_fs::display_path(&project.root))
        .unwrap_or_default();
    let initials = project
        .map(|project| project.initials.clone())
        .unwrap_or_default();
    let tone = project
        .map(|project| Color32::from_rgb(project.tone.0, project.tone.1, project.tone.2))
        .unwrap_or(p.accent);

    let project_rect = Rect::from_min_size(Pos2::new(x, top + 6.0), Vec2::new(238.0, 28.0));
    let response = ui.interact(
        project_rect,
        ui.scope_id().with("project-id"),
        Sense::click(),
    );
    if response.hovered() {
        painter.rect_filled(project_rect, egui::CornerRadius::same(6), p.hover);
    }
    let badge = Rect::from_center_size(
        Pos2::new(x + 15.0, project_rect.center().y),
        Vec2::splat(22.0),
    );
    painter.rect_filled(
        badge,
        egui::CornerRadius::same(6),
        tone.gamma_multiply(0.16),
    );
    if has_project {
        painter.text(
            badge.center(),
            Align2::CENTER_CENTER,
            &initials,
            FontId::proportional(11.0),
            tone,
        );
    } else {
        paint_icon(ui.painter(), badge.shrink(5.0), Icon::FolderOpen, p.text3);
    }
    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(x + 32.0, top + 7.0),
            Pos2::new(x + 150.0, top + 22.0),
        ),
        &project_name,
        FontId::proportional(13.5),
        p.text,
        Align2::LEFT_CENTER,
    );
    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(x + 32.0, top + 21.0),
            Pos2::new(x + 198.0, top + 34.0),
        ),
        &project_path,
        FontId::proportional(11.0),
        p.text3,
        Align2::LEFT_CENTER,
    );
    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(project_rect.right() - 14.0, project_rect.center().y),
            Vec2::splat(12.0),
        ),
        Icon::ChevronDown,
        p.text3,
    );
    if response.clicked() {
        if has_project {
            rt.toggle_menu(MenuKind::Project, project_rect, None);
        } else {
            open_project_dialog = true;
        }
    }
    x = project_rect.right() + 6.0;

    let add_rect = Rect::from_center_size(Pos2::new(x + 11.0, top + 20.0), Vec2::splat(22.0));
    let add_response = ui.interact(add_rect, ui.scope_id().with("title-add"), Sense::click());
    if add_response.hovered() {
        painter.rect_filled(add_rect, egui::CornerRadius::same(6), p.hover);
    }
    paint_icon(ui.painter(), add_rect.shrink(5.0), Icon::Plus, p.text3);
    if add_response.clicked() {
        open_project_dialog = true;
    }
    add_response.on_hover_text(crate::tr!(model, "Open another project", "打开其他项目"));
    x = add_rect.right() + 7.0;

    painter.vline(
        x - 3.5,
        (top + 10.0)..=(rect.bottom() - 10.0),
        Stroke::new(1.0, p.border_soft),
    );

    // Branch chip only exists with an open project.
    if project.is_some() {
        // Worktree chip first (Hierarchy: Project -> Worktree -> Branch).
        let worktree_name = project
            .map(|project| project.active_worktree().name.clone())
            .unwrap_or_else(|| "main".to_string());
        let wt_width = (worktree_name.chars().count() as f32 * 7.0 + 36.0).clamp(66.0, 160.0);
        let worktree_rect =
            Rect::from_min_size(Pos2::new(x + 4.0, top + 7.0), Vec2::new(wt_width, 26.0));
        let worktree_response = ui.interact(
            worktree_rect,
            ui.scope_id().with("worktree-chip"),
            Sense::click(),
        );
        painter.rect_filled(
            worktree_rect,
            egui::CornerRadius::same(6),
            if worktree_response.hovered() {
                p.hover
            } else {
                p.panel2
            },
        );
        painter.rect_stroke(
            worktree_rect,
            egui::CornerRadius::same(6),
            Stroke::new(
                1.0,
                if worktree_response.hovered() {
                    p.border_strong
                } else {
                    p.border
                },
            ),
            StrokeKind::Inside,
        );
        paint_icon(
            &painter,
            Rect::from_center_size(
                Pos2::new(worktree_rect.left() + 12.0, worktree_rect.center().y),
                Vec2::splat(12.0),
            ),
            Icon::Worktree,
            p.accent,
        );
        text_clipped(
            &painter,
            Rect::from_min_max(
                Pos2::new(worktree_rect.left() + 22.0, worktree_rect.top()),
                Pos2::new(worktree_rect.right() - 6.0, worktree_rect.bottom()),
            ),
            &worktree_name,
            FontId::proportional(12.5),
            p.text,
            Align2::LEFT_CENTER,
        );
        let is_single = project.map(|p| p.is_single_file).unwrap_or(false);
        if worktree_response.clicked() && has_project && !is_single {
            rt.toggle_menu(MenuKind::Worktree, worktree_rect, None);
        }
        worktree_response.on_hover_text(crate::tr!(
            model,
            "Switch worktree · create linked checkouts with independent consoles",
            "切换工作树 · 可创建独立控制台的关联检出"
        ));

        let (conflicts, sequencer) = project
            .map(|p| (p.conflicts.clone(), p.sequencer.clone()))
            .unwrap_or_default();
        let op_conflicts = !conflicts.is_empty();
        let op_active = sequencer.is_some() || op_conflicts;

        // Branch chip inside the selected worktree.
        let branch = if is_single {
            crate::tr!(model, "Single-file", "单文件模式").to_string()
        } else {
            project
                .map(|project| project.branch())
                .unwrap_or_else(|| crate::tr!(model, "no repository", "无仓库").to_string())
        };
        let ahead = project
            .and_then(|project| project.git.as_ref())
            .map(|git| git.ahead)
            .unwrap_or(0);
        let behind = project
            .and_then(|project| project.git.as_ref())
            .map(|git| git.behind)
            .unwrap_or(0);

        let op_label = if op_active {
            match &sequencer {
                Some(lazecode_git::SequencerState::Merge { .. }) => {
                    if model.settings.language.is_zh() {
                        format!("正在合并到 {branch}")
                    } else {
                        format!("Merging in {branch}")
                    }
                }
                Some(lazecode_git::SequencerState::Rebase) => {
                    if model.settings.language.is_zh() {
                        format!("正在变基 {branch}")
                    } else {
                        format!("Rebasing {branch}")
                    }
                }
                Some(lazecode_git::SequencerState::CherryPick { .. }) => {
                    if model.settings.language.is_zh() {
                        format!("正在拣选到 {branch}")
                    } else {
                        format!("Cherry-picking in {branch}")
                    }
                }
                Some(lazecode_git::SequencerState::Revert { .. }) => {
                    if model.settings.language.is_zh() {
                        format!("正在回滚 {branch}")
                    } else {
                        format!("Reverting in {branch}")
                    }
                }
                _ if op_conflicts => {
                    if model.settings.language.is_zh() {
                        format!("存在 {} 处冲突", conflicts.len())
                    } else {
                        format!("{} conflicts", conflicts.len())
                    }
                }
                _ => branch.clone(),
            }
        } else {
            branch.clone()
        };

        let display_name = if op_active { &op_label } else { &branch };
        let branch_width = (display_name.chars().count() as f32 * 6.6
            + 58.0
            + usize::from(!op_active && ahead > 0) as f32 * 24.0
            + usize::from(!op_active && behind > 0) as f32 * 24.0)
            .clamp(90.0, 320.0);
        let branch_rect = Rect::from_min_size(
            Pos2::new(worktree_rect.right() + 4.0, top + 7.0),
            Vec2::new(branch_width, 26.0),
        );
        let response = ui.interact(
            branch_rect,
            ui.scope_id().with("branch-chip"),
            Sense::click(),
        );
        let bg_color = if op_active {
            p.amber.gamma_multiply(0.18)
        } else if response.hovered() {
            p.hover
        } else {
            p.panel2
        };
        let border_color = if op_active {
            p.amber
        } else if response.hovered() {
            p.border_strong
        } else {
            p.border
        };
        painter.rect_filled(branch_rect, egui::CornerRadius::same(6), bg_color);
        painter.rect_stroke(
            branch_rect,
            egui::CornerRadius::same(6),
            Stroke::new(1.0, border_color),
            StrokeKind::Inside,
        );
        paint_icon(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(branch_rect.left() + 13.0, branch_rect.center().y),
                Vec2::splat(14.0),
            ),
            if op_active { Icon::Alert } else { Icon::Git },
            if op_active { p.amber } else { p.accent },
        );
        let mut bx = branch_rect.left() + 22.0;
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(bx, branch_rect.top()),
                Pos2::new(
                    branch_rect.right() - if op_active { 20.0 } else { 36.0 },
                    branch_rect.bottom(),
                ),
            ),
            display_name,
            FontId::proportional(13.0),
            if op_active { p.amber } else { p.text2 },
            Align2::LEFT_CENTER,
        );
        if !op_active {
            bx = branch_rect.right() - 34.0;
            if ahead > 0 {
                painter.text(
                    Pos2::new(bx, branch_rect.center().y),
                    Align2::LEFT_CENTER,
                    format!("↑{ahead}"),
                    FontId::monospace(11.0),
                    p.green,
                );
                bx += 20.0;
            }
            if behind > 0 {
                painter.text(
                    Pos2::new(bx, branch_rect.center().y),
                    Align2::LEFT_CENTER,
                    format!("↓{behind}"),
                    FontId::monospace(11.0),
                    p.orange,
                );
            }
        }
        paint_icon(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(branch_rect.right() - 10.0, branch_rect.center().y),
                Vec2::splat(11.0),
            ),
            Icon::ChevronDown,
            if op_active { p.amber } else { p.text3 },
        );
        if response.clicked() && has_project && !is_single {
            rt.toggle_menu(MenuKind::Branch, branch_rect, None);
        }

        let mut after_branch_x = branch_rect.right();
        if op_active {
            // Action 1: Green Continue / Resolve button
            let continue_rect = Rect::from_min_size(
                Pos2::new(after_branch_x + 4.0, top + 7.0),
                Vec2::new(26.0, 26.0),
            );
            let continue_resp = ui.interact(
                continue_rect,
                ui.scope_id().with("sequencer-continue"),
                Sense::click(),
            );
            let green_base = Color32::from_rgb(73, 156, 84);
            let green_hover = Color32::from_rgb(87, 167, 99);
            painter.rect_filled(
                continue_rect,
                egui::CornerRadius::same(6),
                if continue_resp.hovered() {
                    green_hover
                } else {
                    green_base
                },
            );
            paint_icon(
                ui.painter(),
                continue_rect.shrink(5.0),
                Icon::DoubleChevronRight,
                Color32::WHITE,
            );
            let continue_tooltip = if op_conflicts {
                crate::tr!(
                    model,
                    "Resolve conflicts · 3-way merge",
                    "解决冲突 · 逐文件三方合并"
                )
            } else {
                crate::tr!(model, "Continue operation", "继续当前操作")
            };
            let continue_resp = continue_resp.on_hover_text(continue_tooltip);
            if continue_resp.clicked() {
                if op_conflicts {
                    if let Some(first) = conflicts.first() {
                        if let Some(proj) = model.active_project() {
                            let root = proj.root.clone();
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
                } else {
                    match &sequencer {
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
                }
            }

            // Action 2: Red Abort button
            let abort_rect = Rect::from_min_size(
                Pos2::new(continue_rect.right() + 4.0, top + 7.0),
                Vec2::new(26.0, 26.0),
            );
            let abort_resp = ui.interact(
                abort_rect,
                ui.scope_id().with("sequencer-abort"),
                Sense::click(),
            );
            let red_base = Color32::from_rgb(219, 88, 96);
            let red_hover = Color32::from_rgb(226, 106, 113);
            painter.rect_filled(
                abort_rect,
                egui::CornerRadius::same(6),
                if abort_resp.hovered() {
                    red_hover
                } else {
                    red_base
                },
            );
            paint_icon(
                ui.painter(),
                abort_rect.shrink(6.0),
                Icon::X,
                Color32::WHITE,
            );
            let abort_resp = abort_resp.on_hover_text(crate::tr!(
                model,
                "Abort operation and restore worktree",
                "中止当前操作并恢复工作树"
            ));
            if abort_resp.clicked() {
                match &sequencer {
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
            after_branch_x = abort_rect.right();
        }

        let _ = after_branch_x;
    }

    // Right cluster: 稳定版顶栏最右侧仅放置放大镜搜索按钮（28x28，无 running 指示与开关窗口图标）。
    let rx = rect.right() - 8.0;
    let search_rect =
        Rect::from_center_size(Pos2::new(rx - 14.0, rect.center().y), Vec2::splat(28.0));
    let search_resp = icon_button(ui, search_rect, Icon::Search, false, theme);
    if search_resp.clicked() {
        rt.open_palette();
        model.ui.command_palette_open = true;
    }
    let _ = MenuKind::Project;
    if open_project_dialog {
        model.queue(Command::OpenProjectDialog);
    }
}
