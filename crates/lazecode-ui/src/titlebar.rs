use egui::{
    Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, ViewportCommand,
};
use lazecode_core::{AppModel, Command, PanelTab, SidebarView};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::{MenuKind, UiRuntime};
use crate::theme::Theme;
use crate::widgets::{icon_button, text_clipped};

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    let painter = ui.painter().clone();
    let top = rect.top();

    // Empty titlebar surface drags the window; controls win hit testing and
    // `titlebar_control_hovered` guards drag/zoom. Traffic lights own 78px.
    let drag_rect = Rect::from_min_max(Pos2::new(rect.left() + 78.0, rect.top()), rect.max);
    let drag = ui.interact(
        drag_rect,
        ui.scope_id().with("title-drag"),
        Sense::click_and_drag(),
    );
    let mut titlebar_control_hovered = false;
    let mut control_rects: Vec<Rect> = Vec::new();

    // macOS traffic lights overlay the first ~70px of the full-width
    // titlebar (stable `padding-left: 78px`).
    let mut x = rect.left() + 78.0;

    // App logo → command palette. (The stable logo opens its project menu;
    // that lands with the titlebar replication batch.)
    let logo_rect = Rect::from_center_size(Pos2::new(x + 15.0, top + 20.0), Vec2::splat(30.0));
    let logo_response = ui.interact(logo_rect, ui.scope_id().with("title-logo"), Sense::click());
    // Use the packaged application icon rather than a synthetic `>_` tile.
    let icon_texture = rt.app_icon_texture(ui.ctx());
    painter.image(
        icon_texture,
        logo_rect,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
    titlebar_control_hovered |= logo_response.hovered();
    control_rects.push(logo_response.rect);
    if logo_response.hovered() {
        painter.rect_stroke(
            logo_rect.expand(1.0),
            egui::CornerRadius::same(8),
            Stroke::new(1.0, p.accent.gamma_multiply(0.7)),
            StrokeKind::Outside,
        );
    }
    if logo_response.clicked() {
        rt.open_palette();
        model.ui.command_palette_open = true;
    }
    logo_response.on_hover_text(crate::tr!(model, "Command palette · ⌘⇧A", "命令面板 · ⌘⇧A"));
    x = logo_rect.right() + 10.0;

    // Tool buttons used to live in the LeftStrip. They now form the titlebar
    // action group; the project list itself is the vertical rail.
    let has_project = model.active_project().is_some();
    let single_active = model
        .active_project()
        .is_some_and(|project| project.is_single_file);
    let single_empty = single_active
        && model
            .active_project()
            .is_some_and(|project| project.tabs.is_empty());
    let changes = model
        .active_project()
        .map(|project| project.changes().len())
        .unwrap_or(0);
    let project_active =
        !single_active && model.ui.sidebar_visible && model.ui.sidebar_view == SidebarView::Files;
    let commit_active =
        !single_active && model.ui.sidebar_visible && model.ui.sidebar_view == SidebarView::Git;
    let compare_active =
        !single_active && model.ui.sidebar_visible && model.ui.sidebar_view == SidebarView::Compare;
    let console_active = model.ui.sidebar_visible && model.ui.sidebar_view == SidebarView::Consoles;
    let log_active =
        !single_active && model.ui.panel_visible && model.ui.panel_tab == PanelTab::Git;
    let terminal_active = model.ui.terminal_visible;
    let has_compare = !single_active
        && model
            .active_project()
            .is_some_and(|project| project.compare_session.is_some());
    let single_needs_terminal = single_active
        && model.active_project().is_some_and(|project| {
            project
                .worktrees
                .iter()
                .all(|worktree| worktree.terminals.is_empty())
        });
    let terminal_needs_host = single_needs_terminal || !has_project;

    let mut tool_x = x + 14.0;
    let tool_y = top + 20.0;
    // Project/Commit/Git Log stay visible in single-file workspaces; the
    // buttons are disabled instead of disappearing so the titlebar does not
    // shift when opening a file from the empty home screen.
    let response = toolbar_button(
        ui,
        theme,
        Pos2::new(tool_x, tool_y),
        "title-project",
        Icon::Files,
        project_active,
        has_project && !single_active,
        None,
        crate::tr!(model, "Project", "项目").to_string(),
    );
    titlebar_control_hovered |= response.hovered();
    control_rects.push(response.rect);
    if response.clicked() {
        if project_active {
            model.ui.sidebar_visible = false;
        } else {
            model.ui.sidebar_view = SidebarView::Files;
            model.ui.sidebar_visible = true;
        }
    }
    tool_x += 34.0;

    let response = toolbar_button(
        ui,
        theme,
        Pos2::new(tool_x, tool_y),
        "title-commit",
        Icon::Changes,
        commit_active,
        has_project && !single_active,
        (changes > 0).then_some(p.rose),
        crate::tr!(model, "Commit", "提交").to_string(),
    );
    titlebar_control_hovered |= response.hovered();
    control_rects.push(response.rect);
    if response.clicked() {
        if commit_active {
            model.ui.sidebar_visible = false;
        } else {
            model.ui.sidebar_view = SidebarView::Git;
            model.ui.sidebar_visible = true;
        }
    }
    tool_x += 34.0;

    if has_compare {
        let response = toolbar_button(
            ui,
            theme,
            Pos2::new(tool_x, tool_y),
            "title-compare",
            Icon::Compare,
            compare_active,
            true,
            None,
            crate::tr!(model, "Compare", "比较").to_string(),
        );
        titlebar_control_hovered |= response.hovered();
        control_rects.push(response.rect);
        if response.clicked() {
            if compare_active {
                model.ui.sidebar_view = SidebarView::Git;
            } else {
                model.ui.sidebar_view = SidebarView::Compare;
                model.ui.sidebar_visible = true;
                if let Some(project) = model.active_project_mut() {
                    if let Some(session) = project.compare_session.as_mut() {
                        session.minimized = false;
                    }
                }
            }
        }
        tool_x += 34.0;
    }

    let response = toolbar_button(
        ui,
        theme,
        Pos2::new(tool_x, tool_y),
        "title-gitlog",
        Icon::GitLog,
        log_active,
        has_project && !single_active,
        None,
        crate::tr!(model, "Git Log · ⌘9", "Git 日志 · ⌘9").to_string(),
    );
    titlebar_control_hovered |= response.hovered();
    control_rects.push(response.rect);
    if response.clicked() {
        if log_active {
            model.ui.panel_visible = false;
        } else {
            model.ui.panel_visible = true;
            model.ui.panel_tab = PanelTab::Git;
            if model.ui.terminal_placement == lazecode_core::TerminalPlacement::Bottom {
                model.ui.terminal_visible = false;
            }
            model.queue(Command::RefreshLog);
        }
    }
    tool_x += 34.0;

    let response = toolbar_button(
        ui,
        theme,
        Pos2::new(tool_x, tool_y),
        "title-terminal",
        Icon::Terminal,
        terminal_active,
        true,
        None,
        crate::tr!(model, "Terminal", "终端").to_string(),
    );
    titlebar_control_hovered |= response.hovered();
    control_rects.push(response.rect);
    if response.clicked() {
        if model.ui.terminal_placement == lazecode_core::TerminalPlacement::Bottom {
            if terminal_active && terminal_needs_host {
                // The dock is visible but empty: the first click should
                // create the home-rooted console, not hide the dock.
                model.queue(Command::NewTerminal);
            } else if terminal_active {
                model.ui.terminal_visible = false;
            } else {
                model.ui.terminal_visible = true;
                model.ui.panel_visible = false;
                if terminal_needs_host {
                    model.queue(Command::NewTerminal);
                }
            }
        } else {
            model.ui.terminal_visible = !terminal_active;
            if model.ui.terminal_visible && terminal_needs_host {
                model.queue(Command::NewTerminal);
            }
        }
    }
    tool_x += 34.0;

    let response = toolbar_button(
        ui,
        theme,
        Pos2::new(tool_x, tool_y),
        "title-consoles",
        Icon::Spark,
        console_active,
        true,
        None,
        crate::tr!(model, "Consoles · sidebar", "控制台 · 侧栏").to_string(),
    );
    titlebar_control_hovered |= response.hovered();
    control_rects.push(response.rect);
    if response.clicked() {
        if console_active {
            model.ui.sidebar_visible = false;
        } else {
            if !has_project || single_needs_terminal {
                // No project, or the ad-hoc files workspace with no shell
                // yet: create a console rooted at the user's home directory.
                model.queue(Command::NewTerminal);
            }
            model.ui.sidebar_view = SidebarView::Consoles;
            model.ui.sidebar_visible = true;
        }
    }
    // Separate the action group from the project selector; the red-line
    // groups in the shell mock should never touch.
    x = tool_x + 24.0;

    let project = model.active_project();
    let project_name = if single_active {
        crate::tr!(model, "Open", "打开").to_string()
    } else {
        project
            .map(|project| project.name.clone())
            .unwrap_or_else(|| crate::tr!(model, "Open", "打开").to_string())
    };
    let project_path = if single_active {
        if single_empty {
            String::new()
        } else {
            crate::tr!(model, "Single files", "单文件").to_string()
        }
    } else {
        project
            .map(|project| lazecode_fs::display_path(&project.root))
            .unwrap_or_default()
    };
    let initials = project
        .map(|project| project.initials.clone())
        .unwrap_or_default();
    let tone = project
        .map(|project| Color32::from_rgb(project.tone.0, project.tone.1, project.tone.2))
        .unwrap_or(p.accent);
    // Keep the branch/right controls viable on narrow windows; the full
    // project card only pays for itself when there is horizontal room.
    let project_width = if rect.width() < 900.0 { 180.0 } else { 238.0 };
    let project_rect = Rect::from_min_size(Pos2::new(x, top + 6.0), Vec2::new(project_width, 28.0));
    let response = ui.interact(
        project_rect,
        ui.scope_id().with("project-id"),
        Sense::click(),
    );
    titlebar_control_hovered |= response.hovered();
    control_rects.push(response.rect);
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
    if single_active && !single_empty {
        paint_icon(ui.painter(), badge.shrink(5.0), Icon::File, tone);
    } else if has_project && !single_empty {
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
    if has_project && !single_empty {
        // Two lines share the 28px card: name 7..21, path 20..33. The group
        // centre stays on the card's centre line.
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(x + 32.0, top + 7.0),
                Pos2::new(project_rect.right() - 40.0, top + 21.0),
            ),
            &project_name,
            FontId::proportional(13.5),
            p.text,
            Align2::LEFT_CENTER,
        );
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(x + 32.0, top + 20.0),
                Pos2::new(project_rect.right() - 40.0, top + 33.0),
            ),
            &project_path,
            FontId::proportional(11.0),
            p.text3,
            Align2::LEFT_CENTER,
        );
    } else {
        // The empty state is one line: centre it in the whole card instead of
        // leaving it on the first line of the two-line project layout.
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(x + 32.0, project_rect.top()),
                Pos2::new(project_rect.right() - 40.0, project_rect.bottom()),
            ),
            &project_name,
            FontId::proportional(13.5),
            p.text,
            Align2::LEFT_CENTER,
        );
    }
    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(project_rect.right() - 14.0, project_rect.center().y),
            Vec2::splat(12.0),
        ),
        Icon::ChevronDown,
        p.text3,
    );
    titlebar_control_hovered |= response.hovered();
    control_rects.push(response.rect);
    if response.clicked() {
        // The selector is the project switcher even when no project is open:
        // the menu lists open projects, recents, and the open-project action.
        rt.toggle_menu(MenuKind::Project, project_rect, None);
    }
    if has_project && !single_empty {
        response.on_hover_text(format!("{project_name} · {project_path}"));
    }
    x = project_rect.right() + 12.0;

    painter.vline(
        x - 6.0,
        (top + 10.0)..=(rect.bottom() - 10.0),
        Stroke::new(1.0, p.border_soft),
    );

    // Branch/worktree chips are omitted for the ad-hoc files workspace; it
    // has no repository context of its own.
    if project.is_some_and(|project| !project.is_single_file) {
        let is_git = project.is_some_and(|project| project.active_git().is_some());
        let is_single = project.is_some_and(|project| project.is_single_file);
        // Worktree chip first (Hierarchy: Project -> Worktree -> Branch).
        // A plain folder has no worktree and no worktree menu.
        let worktree_name = if is_git {
            project
                .map(|project| project.active_worktree().name.clone())
                .unwrap_or_else(|| "main".to_string())
        } else {
            crate::tr!(model, "no worktree", "无工作树").to_string()
        };
        let worktree_enabled = is_git && !is_single;
        let wt_width = (worktree_name.chars().count() as f32 * 8.0 + 38.0).clamp(70.0, 180.0);
        let worktree_rect = Rect::from_min_size(Pos2::new(x, top + 6.0), Vec2::new(wt_width, 28.0));
        let worktree_response = ui.interact(
            worktree_rect,
            ui.scope_id().with("worktree-chip"),
            Sense::click(),
        );
        titlebar_control_hovered |= worktree_response.hovered();
        control_rects.push(worktree_response.rect);
        if worktree_enabled && worktree_response.hovered() {
            painter.rect_filled(worktree_rect, egui::CornerRadius::same(6), p.hover);
        }
        paint_icon(
            &painter,
            Rect::from_center_size(
                Pos2::new(worktree_rect.left() + 14.0, worktree_rect.center().y),
                Vec2::splat(15.0),
            ),
            Icon::Worktree,
            if is_git { p.accent } else { p.text3 },
        );
        text_clipped(
            &painter,
            Rect::from_min_max(
                Pos2::new(worktree_rect.left() + 28.0, worktree_rect.top()),
                Pos2::new(worktree_rect.right() - 8.0, worktree_rect.bottom()),
            ),
            &worktree_name,
            FontId::proportional(14.0),
            p.text2,
            Align2::LEFT_CENTER,
        );
        if worktree_response.clicked() && worktree_enabled {
            rt.toggle_menu(MenuKind::Worktree, worktree_rect, None);
        }
        if worktree_enabled {
            worktree_response.on_hover_text(crate::tr!(
                model,
                "Switch worktree · create linked checkouts with independent consoles",
                "切换工作树 · 可创建独立控制台的关联检出"
            ));
        }

        let (conflicts, sequencer) = project
            .map(|p| (p.conflicts.clone(), p.sequencer.clone()))
            .unwrap_or_default();
        let op_conflicts = !conflicts.is_empty();
        let op_active = sequencer.is_some() || op_conflicts;

        // Branch chip inside the selected worktree.
        let branch = if !is_git {
            crate::tr!(model, "no branch", "无分支").to_string()
        } else if is_single {
            crate::tr!(model, "Single-file", "单文件模式").to_string()
        } else {
            project
                .map(|project| project.branch())
                .unwrap_or_else(|| crate::tr!(model, "no branch", "无分支").to_string())
        };
        let ahead = project
            .and_then(|project| project.active_git())
            .map(|git| git.ahead)
            .unwrap_or(0);
        let behind = project
            .and_then(|project| project.active_git())
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
        let branch_width = (display_name.chars().count() as f32 * 7.4
            + 58.0
            + usize::from(!op_active && ahead > 0) as f32 * 24.0
            + usize::from(!op_active && behind > 0) as f32 * 24.0)
            .clamp(96.0, 340.0);
        let branch_rect = Rect::from_min_size(
            Pos2::new(worktree_rect.right() + 12.0, top + 6.0),
            Vec2::new(branch_width, 28.0),
        );
        let response = ui.interact(
            branch_rect,
            ui.scope_id().with("branch-chip"),
            Sense::click(),
        );
        titlebar_control_hovered |= response.hovered();
        control_rects.push(response.rect);
        if op_active {
            painter.rect_filled(
                branch_rect,
                egui::CornerRadius::same(6),
                p.amber.gamma_multiply(0.18),
            );
        } else if is_git && response.hovered() {
            painter.rect_filled(branch_rect, egui::CornerRadius::same(6), p.hover);
        }
        paint_icon(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(branch_rect.left() + 15.0, branch_rect.center().y),
                Vec2::splat(16.0),
            ),
            if op_active { Icon::Alert } else { Icon::Git },
            if op_active {
                p.amber
            } else if is_git {
                p.accent
            } else {
                p.text3
            },
        );
        let mut bx = branch_rect.left() + 28.0;
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(bx, branch_rect.top()),
                Pos2::new(
                    branch_rect.right()
                        - if op_active {
                            20.0
                        } else if is_git {
                            36.0
                        } else {
                            12.0
                        },
                    branch_rect.bottom(),
                ),
            ),
            display_name,
            FontId::proportional(14.0),
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
                    FontId::monospace(12.0),
                    p.green,
                );
                bx += 20.0;
            }
            if behind > 0 {
                painter.text(
                    Pos2::new(bx, branch_rect.center().y),
                    Align2::LEFT_CENTER,
                    format!("↓{behind}"),
                    FontId::monospace(12.0),
                    p.orange,
                );
            }
        }
        if is_git {
            paint_icon(
                ui.painter(),
                Rect::from_center_size(
                    Pos2::new(branch_rect.right() - 10.0, branch_rect.center().y),
                    Vec2::splat(12.0),
                ),
                Icon::ChevronDown,
                if op_active { p.amber } else { p.text3 },
            );
        }
        if response.clicked() && is_git && !is_single {
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
            titlebar_control_hovered |= continue_resp.hovered();
            control_rects.push(continue_resp.rect);
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
            titlebar_control_hovered |= abort_resp.hovered();
            control_rects.push(abort_resp.rect);
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

    // Right cluster: search then settings. Settings deliberately sits at the
    // far right edge, after the magnifier.
    let rx = rect.right() - 8.0;
    let settings_resp = toolbar_button_sized(
        ui,
        theme,
        Pos2::new(rx - 14.0, rect.center().y),
        "title-settings",
        28.0,
        15.0,
        Icon::Settings,
        rt.settings_open,
        true,
        None,
        crate::tr!(model, "Settings · ⌘,", "设置 · ⌘,").to_string(),
    );
    titlebar_control_hovered |= settings_resp.hovered();
    control_rects.push(settings_resp.rect);
    if settings_resp.clicked() {
        rt.settings_open = !rt.settings_open;
        rt.about_open = false;
    }
    let search_rect =
        Rect::from_center_size(Pos2::new(rx - 44.0, rect.center().y), Vec2::splat(28.0));
    let search_resp = icon_button(ui, search_rect, Icon::Search, false, theme);
    titlebar_control_hovered |= search_resp.hovered();
    control_rects.push(search_resp.rect);
    if search_resp.clicked() {
        rt.open_palette();
        model.ui.command_palette_open = true;
    }

    // Update badge sits left of search; the native settings button owns the
    // far right.
    if let Some(badge) = rt.update.badge(&model.settings) {
        let galley =
            ui.painter()
                .layout_no_wrap(badge.text.clone(), FontId::proportional(11.0), p.green);
        let width = (galley.size().x + 34.0).clamp(88.0, 170.0);
        let badge_rect = Rect::from_center_size(
            Pos2::new(search_rect.left() - 8.0 - width / 2.0, rect.center().y),
            Vec2::new(width, 24.0),
        );
        // Do not let the pill run into the project/branch cluster on narrow
        // windows; the update remains reachable from Settings.
        if badge_rect.left() > rect.left() + 360.0 {
            let response = ui.interact(
                badge_rect,
                ui.scope_id().with("title-update"),
                Sense::click(),
            );
            titlebar_control_hovered |= response.hovered();
            control_rects.push(response.rect);
            let fill = if response.hovered() {
                p.green.gamma_multiply(0.22)
            } else {
                p.green.gamma_multiply(0.12)
            };
            let border = p
                .green
                .gamma_multiply(if response.hovered() { 0.85 } else { 0.55 });
            ui.painter()
                .rect_filled(badge_rect, egui::CornerRadius::same(12), fill);
            ui.painter().rect_stroke(
                badge_rect,
                egui::CornerRadius::same(12),
                Stroke::new(1.0, border),
                StrokeKind::Inside,
            );
            paint_icon(
                ui.painter(),
                Rect::from_center_size(
                    Pos2::new(badge_rect.left() + 13.0, badge_rect.center().y),
                    Vec2::splat(12.0),
                ),
                Icon::UpdateArrow,
                p.green,
            );
            text_clipped(
                ui.painter(),
                Rect::from_min_max(
                    Pos2::new(badge_rect.left() + 24.0, badge_rect.top()),
                    Pos2::new(badge_rect.right() - 7.0, badge_rect.bottom()),
                ),
                &badge.text,
                FontId::proportional(11.0),
                p.green,
                Align2::LEFT_CENTER,
            );
            let response = response.on_hover_text(badge.tooltip);
            if response.clicked() {
                rt.update_open = true;
                rt.settings_open = false;
                rt.about_open = false;
                model.ui.command_palette_open = false;
            }
        }
    }
    if !titlebar_control_hovered && drag.dragged() {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }

    rt.titlebar_controls.clear();
    rt.titlebar_controls.extend(control_rects);
}

#[allow(clippy::too_many_arguments)]
fn toolbar_button(
    ui: &mut Ui,
    theme: &Theme,
    center: Pos2,
    id: &'static str,
    icon: Icon,
    active: bool,
    enabled: bool,
    dot: Option<Color32>,
    tooltip: String,
) -> egui::Response {
    toolbar_button_sized(
        ui, theme, center, id, 32.0, 18.0, icon, active, enabled, dot, tooltip,
    )
}

#[allow(clippy::too_many_arguments)]
fn toolbar_button_sized(
    ui: &mut Ui,
    theme: &Theme,
    center: Pos2,
    id: &'static str,
    size: f32,
    icon_size: f32,
    icon: Icon,
    active: bool,
    enabled: bool,
    dot: Option<Color32>,
    tooltip: String,
) -> egui::Response {
    let p = theme.palette;
    let rect = Rect::from_center_size(center, Vec2::splat(size));
    let response = ui.interact(
        rect,
        ui.scope_id().with(id),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if enabled && (active || response.hovered()) {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(7),
            if active { p.active } else { p.hover },
        );
    }
    paint_icon(
        ui.painter(),
        Rect::from_center_size(center, Vec2::splat(icon_size)),
        icon,
        if !enabled {
            p.text3.gamma_multiply(0.55)
        } else if active {
            p.text
        } else {
            p.text2
        },
    );
    if enabled {
        if let Some(color) = dot {
            let c = Pos2::new(rect.right() - 4.0, rect.top() + 4.0);
            ui.painter().circle_filled(c, 3.0, p.titlebar);
            ui.painter().circle_filled(c, 2.0, color);
        }
    }
    response.on_hover_text(tooltip)
}
