use egui::{
    Align2, Color32, FontId, Key, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2,
};
use lazecode_core::{AppModel, Command, ToastKind};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::{DialogState, MenuKind, MenuState, UiRuntime};
use crate::theme::Theme;
use crate::widgets::{live_dot, primary_button, secondary_button, text_clipped};

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme) {
    let screen = ui.max_rect();
    if let Some(menu) = rt.menu.take() {
        match menu.kind {
            MenuKind::Project => draw_project_menu(ui, model, rt, theme, screen, menu),
            MenuKind::Branch => draw_branch_menu(ui, model, rt, theme, screen, menu),
            MenuKind::Worktree => draw_worktree_menu(ui, model, rt, theme, screen, menu),
        }
    }
    if let Some(anchor) = rt.agent_menu.take() {
        draw_agent_menu(ui, model, rt, theme, screen, anchor);
    }
    if !matches!(rt.dialog, DialogState::None) {
        draw_dialog(ui, model, rt, theme, screen);
    }
}

fn popover_rect(screen: Rect, anchor: Rect, width: f32, height: f32) -> Rect {
    let x = (anchor.left() - 4.0)
        .min(screen.right() - width - 8.0)
        .max(screen.left() + 8.0);
    let below = anchor.bottom() + 6.0;
    let y = if below + height < screen.bottom() - 8.0 {
        below
    } else {
        (anchor.top() - height - 6.0).max(screen.top() + 8.0)
    };
    Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, height))
}

fn frame(ui: &Ui, rect: Rect, theme: &Theme) {
    let p = theme.palette;
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(9), p.elev);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(9),
        Stroke::new(1.0, p.border_strong),
        StrokeKind::Inside,
    );
}

fn outside_click(ui: &Ui, rect: Rect) -> bool {
    ui.input(|input| {
        input.pointer.any_click()
            && input
                .pointer
                .interact_pos()
                .is_some_and(|pos| !rect.contains(pos))
    })
}

fn draw_project_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
) {
    let p = theme.palette;
    let count = model.projects.len();
    let height = 46.0 + (count as f32 * 46.0).min(300.0) + 74.0;
    let rect = popover_rect(screen, menu.anchor, 326.0, height);
    frame(ui, rect, theme);
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, rect.top() + 20.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Switch project", "切换项目"),
        FontId::proportional(11.0),
        p.text3,
    );
    ui.painter().text(
        Pos2::new(rect.right() - 14.0, rect.top() + 20.0),
        Align2::RIGHT_CENTER,
        if model.settings.language.is_zh() {
            format!("{} 个已打开", count)
        } else {
            format!("{count} open")
        },
        FontId::monospace(9.0),
        p.text3,
    );
    let mut y = rect.top() + 34.0;
    let mut switch_to: Option<usize> = None;
    let mut close = false;
    let max_rows = if height > 300.0 { 6 } else { count };
    for index in 0..model.projects.len().min(max_rows) {
        let project = &model.projects[index];
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 6.0, y),
            Vec2::new(rect.width() - 12.0, 44.0),
        );
        let response = ui.interact(row, ui.scope_id().with(("pm", index)), Sense::click());
        if model.active_project == Some(index) {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(5), p.active);
        } else if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(5), p.hover);
        }
        let tone = Color32::from_rgb(project.tone.0, project.tone.1, project.tone.2);
        let avatar = Rect::from_center_size(
            Pos2::new(row.left() + 20.0, row.center().y),
            Vec2::splat(26.0),
        );
        ui.painter().rect_filled(
            avatar,
            egui::CornerRadius::same(7),
            tone.gamma_multiply(0.16),
        );
        ui.painter().text(
            avatar.center(),
            Align2::CENTER_CENTER,
            &project.initials,
            FontId::proportional(9.5),
            tone,
        );
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(row.left() + 40.0, row.top() + 4.0),
                Pos2::new(row.right() - 58.0, row.top() + 23.0),
            ),
            &project.name,
            FontId::proportional(12.0),
            p.text,
            Align2::LEFT_CENTER,
        );
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(row.left() + 40.0, row.top() + 21.0),
                Pos2::new(row.right() - 58.0, row.bottom() - 4.0),
            ),
            &project.root.display().to_string(),
            FontId::proportional(9.0),
            p.text3,
            Align2::LEFT_CENTER,
        );
        let running = project
            .worktrees
            .iter()
            .flat_map(|worktree| worktree.terminals.iter())
            .filter(|terminal| terminal.session.is_running())
            .count();
        if running > 0 {
            live_dot(
                ui.painter(),
                Pos2::new(row.right() - 46.0, row.center().y),
                true,
                p.running,
            );
            ui.painter().text(
                Pos2::new(row.right() - 38.0, row.center().y),
                Align2::LEFT_CENTER,
                running.to_string(),
                FontId::monospace(9.0),
                p.running,
            );
        } else {
            ui.painter().text(
                Pos2::new(row.right() - 18.0, row.center().y),
                Align2::RIGHT_CENTER,
                crate::tr!(model, "idle", "空闲"),
                FontId::monospace(8.5),
                p.text3,
            );
        }
        if model.active_project == Some(index) {
            paint_icon(
                ui.painter(),
                Rect::from_center_size(
                    Pos2::new(row.right() - 16.0, row.center().y),
                    Vec2::splat(11.0),
                ),
                Icon::Check,
                p.accent,
            );
        }
        if response.clicked() {
            switch_to = Some(index);
            close = true;
        }
        y += 46.0;
    }
    // Footer actions.
    let footer = Rect::from_min_max(
        Pos2::new(rect.left() + 6.0, rect.bottom() - 36.0),
        Pos2::new(rect.right() - 6.0, rect.bottom() - 4.0),
    );
    let open_rect = Rect::from_min_size(footer.min, Vec2::new(footer.width() * 0.5 - 3.0, 30.0));
    if secondary_button(
        ui,
        open_rect,
        crate::tr!(model, "Open project…", "打开项目…"),
        Some(Icon::Plus),
        theme,
    )
    .clicked()
    {
        model.queue(Command::OpenProjectDialog);
        close = true;
    }
    let close_rect = Rect::from_min_size(
        Pos2::new(open_rect.right() + 6.0, footer.top()),
        Vec2::new(footer.width() * 0.5 - 3.0, 30.0),
    );
    if secondary_button(
        ui,
        close_rect,
        crate::tr!(model, "Close project", "关闭项目"),
        Some(Icon::X),
        theme,
    )
    .clicked()
    {
        if let Some(active) = model.active_project {
            let dirty = model.projects[active].tabs.iter().any(|tab| tab.is_dirty());
            if dirty {
                model.toast(
                    if model.settings.language.is_zh() {
                        "该项目有未保存的文件，请先保存或关闭标签页"
                    } else {
                        "This project has unsaved files; save or close its tabs first"
                    },
                    lazecode_core::ToastKind::Warning,
                );
            } else {
                model.close_project(active);
                model.save_workspace();
                rt.clear_terminal_cache();
            }
        }
        close = true;
    }
    if let Some(index) = switch_to {
        model.switch_project(index);
        rt.clear_terminal_cache();
    }
    if outside_click(ui, rect) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

fn draw_worktree_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
) {
    let p = theme.palette;
    let Some(index) = model.active_project else {
        return;
    };
    let count = model.projects[index].worktrees.len();
    let height = 86.0 + (count as f32 * 36.0).min(320.0) + 74.0;
    let rect = popover_rect(screen, menu.anchor, 324.0, height);
    frame(ui, rect, theme);
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, rect.top() + 18.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Worktrees", "工作树"),
        FontId::proportional(11.0),
        p.text3,
    );
    ui.painter().text(
        Pos2::new(rect.right() - 14.0, rect.top() + 18.0),
        Align2::RIGHT_CENTER,
        if model.settings.language.is_zh() {
            format!("{} 个已挂载", count)
        } else {
            format!("{count} attached")
        },
        FontId::monospace(9.0),
        p.text3,
    );
    let mut close = false;
    let mut switch_to: Option<String> = None;
    let mut remove: Option<String> = None;
    let rows_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 6.0, rect.top() + 32.0),
        Pos2::new(rect.right() - 6.0, rect.bottom() - 40.0),
    );
    super::app_ui::region(ui, rows_rect, "worktree-menu", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("worktree-menu-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for worktree in model.projects[index].worktrees.iter() {
                    let (row, response) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::click());
                    let is_active = model.projects[index].active_worktree_index()
                        == Some(row_index(model, index, &worktree.name));
                    if is_active {
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::same(4), p.active);
                    } else if response.hovered() {
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::same(4), p.hover);
                    }
                    // Branch chip.
                    let branch = worktree.branch();
                    ui.painter().text(
                        Pos2::new(row.left() + 12.0, row.top() + 9.0),
                        Align2::LEFT_CENTER,
                        &worktree.name,
                        FontId::proportional(11.0),
                        p.text,
                    );
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(row.left() + 12.0, row.top() + 17.0),
                            Pos2::new(row.right() - 30.0, row.bottom()),
                        ),
                        &branch,
                        FontId::monospace(9.0),
                        p.text3,
                        Align2::LEFT_CENTER,
                    );
                    if is_active {
                        paint_icon(
                            ui.painter(),
                            Rect::from_center_size(
                                Pos2::new(row.right() - 36.0, row.center().y),
                                Vec2::splat(11.0),
                            ),
                            Icon::Check,
                            p.accent,
                        );
                    }
                    // Remove control (linked worktrees only).
                    if !worktree.is_main {
                        let remove_rect = Rect::from_center_size(
                            Pos2::new(row.right() - 14.0, row.center().y),
                            Vec2::splat(20.0),
                        );
                        let remove_response = ui.interact(
                            remove_rect,
                            ui.scope_id()
                                .with(("worktree-remove", worktree.name.clone())),
                            Sense::click(),
                        );
                        if remove_response.hovered() {
                            ui.painter().rect_filled(
                                remove_rect,
                                egui::CornerRadius::same(4),
                                p.hover,
                            );
                            paint_icon(ui.painter(), remove_rect.shrink(4.0), Icon::Trash, p.rose);
                        } else {
                            paint_icon(ui.painter(), remove_rect.shrink(4.0), Icon::Trash, p.text3);
                        }
                        if remove_response.clicked() {
                            remove = Some(worktree.name.clone());
                        }
                    }
                    if response.clicked() {
                        switch_to = Some(worktree.name.clone());
                    }
                }
            });
    });
    let new_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 10.0, rect.bottom() - 34.0),
        Vec2::new(rect.width() - 20.0, 28.0),
    );
    if secondary_button(
        ui,
        new_rect,
        crate::tr!(model, "New worktree…", "新建工作树…"),
        Some(Icon::Plus),
        theme,
    )
    .clicked()
    {
        rt.dialog = DialogState::NewWorktree {
            value: String::new(),
            error: None,
        };
        close = true;
    }
    if let Some(name) = remove {
        if let Some(worktree) = model.projects[index]
            .worktrees
            .iter()
            .find(|worktree| worktree.name == name)
        {
            rt.dialog = DialogState::RemoveWorktree {
                name: name.clone(),
                worktree_path: worktree.path.clone(),
            };
        }
        close = true;
    }
    if let Some(name) = switch_to {
        model.queue(Command::SwitchWorktree(name));
        rt.clear_terminal_cache();
        close = true;
    }
    if outside_click(ui, rect) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

fn row_index(model: &AppModel, project: usize, name: &str) -> usize {
    model.projects[project]
        .worktrees
        .iter()
        .position(|worktree| worktree.name == name)
        .unwrap_or(0)
}

fn draw_branch_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
) {
    let p = theme.palette;
    let Some(index) = model.active_project else {
        return;
    };
    let height = 380.0;
    let rect = popover_rect(screen, menu.anchor, 324.0, height);
    frame(ui, rect, theme);
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, rect.top() + 18.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Branches", "分支"),
        FontId::proportional(11.0),
        p.text3,
    );
    let mut close = false;
    let input = Rect::from_min_size(
        Pos2::new(rect.left() + 10.0, rect.top() + 30.0),
        Vec2::new(rect.width() - 20.0, 28.0),
    );
    ui.painter()
        .rect_filled(input, egui::CornerRadius::same(5), p.panel2);
    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(input.left() + 13.0, input.center().y),
            Vec2::splat(11.0),
        ),
        Icon::Search,
        p.text3,
    );
    let mut query = model.projects[index].branch_query.clone();
    let query_response = ui.put(
        Rect::from_min_max(
            Pos2::new(input.left() + 24.0, input.top() + 3.0),
            Pos2::new(input.right() - 6.0, input.bottom() - 3.0),
        ),
        TextEdit::singleline(&mut query)
            .frame(egui::Frame::NONE)
            .hint_text(crate::tr!(model, "Filter branches", "过滤分支"))
            .font(FontId::proportional(10.5)),
    );
    if query_response.changed() {
        model.projects[index].branch_query = query.clone();
    }
    let query = query.to_lowercase();
    let branches: Vec<lazecode_git::BranchInfo> = model.projects[index]
        .branches
        .iter()
        .filter(|branch| query.is_empty() || branch.name.to_lowercase().contains(&query))
        .cloned()
        .collect();
    let current = model.projects[index].branch();
    let rows_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 6.0, input.bottom() + 4.0),
        Pos2::new(rect.right() - 6.0, rect.bottom() - 40.0),
    );
    let mut checkout: Option<String> = None;
    let mut delete: Option<String> = None;
    super::app_ui::region(ui, rows_rect, "branch-menu", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("branch-menu-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for branch in &branches {
                    let (row, response) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 25.0), Sense::click());
                    if response.hovered() {
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::same(4), p.hover);
                    }
                    let is_current = branch.name == current;
                    ui.painter().circle_stroke(
                        Pos2::new(row.left() + 10.0, row.center().y),
                        2.6,
                        Stroke::new(1.2, if is_current { p.accent } else { p.text3 }),
                    );
                    if is_current {
                        ui.painter().circle_filled(
                            Pos2::new(row.left() + 10.0, row.center().y),
                            1.5,
                            p.accent,
                        );
                    }
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(row.left() + 20.0, row.top()),
                            Pos2::new(row.right() - 56.0, row.bottom()),
                        ),
                        &branch.name,
                        FontId::proportional(10.5),
                        if branch.remote { p.text2 } else { p.text },
                        Align2::LEFT_CENTER,
                    );
                    if branch.remote {
                        ui.painter().text(
                            Pos2::new(row.right() - 8.0, row.center().y),
                            Align2::RIGHT_CENTER,
                            crate::tr!(model, "remote", "远程"),
                            FontId::monospace(8.0),
                            p.text3,
                        );
                    }
                    if !branch.remote && !is_current {
                        let delete_rect = Rect::from_center_size(
                            Pos2::new(row.right() - 14.0, row.center().y),
                            Vec2::splat(18.0),
                        );
                        let delete_response = ui.interact(
                            delete_rect,
                            ui.scope_id().with(("branch-delete", branch.name.clone())),
                            Sense::click(),
                        );
                        if delete_response.hovered() {
                            paint_icon(ui.painter(), delete_rect.shrink(4.0), Icon::Trash, p.rose);
                        }
                        if delete_response.clicked() {
                            delete = Some(branch.name.clone());
                        }
                    }
                    if response.clicked() && !branch.remote && !is_current {
                        checkout = Some(branch.name.clone());
                    }
                }
                if branches.is_empty() {
                    let (row, _) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
                    ui.painter().text(
                        Pos2::new(row.left() + 10.0, row.center().y),
                        Align2::LEFT_CENTER,
                        crate::tr!(model, "No branches match", "没有匹配的分支"),
                        FontId::proportional(10.0),
                        p.text3,
                    );
                }
                ui.add_space(6.0);
            });
    });
    let new_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 10.0, rect.bottom() - 34.0),
        Vec2::new(rect.width() - 20.0, 28.0),
    );
    if secondary_button(
        ui,
        new_rect,
        crate::tr!(model, "New branch…", "新建分支…"),
        Some(Icon::Plus),
        theme,
    )
    .clicked()
    {
        rt.dialog = DialogState::NewBranch {
            value: String::new(),
            error: None,
        };
        close = true;
    }
    if let Some(name) = checkout {
        model.queue(Command::CheckoutBranch(name));
        close = true;
    }
    if let Some(name) = delete {
        rt.dialog = DialogState::DeleteBranch { name, error: None };
        close = true;
    }
    if outside_click(ui, rect) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

fn draw_agent_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    anchor: Rect,
) {
    let p = theme.palette;
    let options = [
        ("Claude Code", "Anthropic's terminal agent", "claude"),
        ("Codex CLI", "OpenAI coding agent", "codex"),
        ("OpenCode", "Open-source terminal agent", "opencode"),
        ("Aider", "Pair programming in the terminal", "aider"),
        ("Shell", "Start a plain login shell", ""),
    ];
    let height = 52.0 + options.len() as f32 * 46.0 + 24.0;
    let rect = popover_rect(screen, anchor, 320.0, height);
    frame(ui, rect, theme);
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, rect.top() + 20.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Start agent", "启动 agent"),
        FontId::proportional(11.0),
        p.text3,
    );
    let mut y = rect.top() + 36.0;
    let mut launch: Option<(String, String)> = None;
    let mut close = false;
    for (label, description, command) in options {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 6.0, y),
            Vec2::new(rect.width() - 12.0, 44.0),
        );
        let response = ui.interact(row, ui.scope_id().with(("agent", label)), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(5), p.hover);
        }
        let icon_rect = Rect::from_center_size(
            Pos2::new(row.left() + 20.0, row.center().y),
            Vec2::splat(24.0),
        );
        ui.painter()
            .rect_filled(icon_rect, egui::CornerRadius::same(6), p.accent_soft);
        paint_icon(ui.painter(), icon_rect.shrink(5.0), Icon::Spark, p.accent);
        ui.painter().text(
            Pos2::new(row.left() + 38.0, row.center().y - 8.0),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(11.5),
            p.text,
        );
        ui.painter().text(
            Pos2::new(row.left() + 38.0, row.center().y + 8.0),
            Align2::LEFT_CENTER,
            description,
            FontId::proportional(9.5),
            p.text3,
        );
        if !command.is_empty() {
            ui.painter().text(
                Pos2::new(row.right() - 12.0, row.center().y),
                Align2::RIGHT_CENTER,
                command,
                FontId::monospace(9.0),
                p.text3,
            );
        }
        if response.clicked() {
            launch = Some((label.to_string(), command.to_string()));
            close = true;
        }
        y += 46.0;
    }
    if let Some((title, command)) = launch {
        if command.is_empty() {
            model.queue(Command::NewTerminal);
        } else {
            model.queue(Command::NewTerminalCommand { title, command });
        }
    }
    if outside_click(ui, rect) {
        close = true;
    }
    if !close {
        rt.agent_menu = Some(anchor);
    }
}

fn draw_dialog(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, screen: Rect) {
    let p = theme.palette;
    let mut dialog = std::mem::replace(&mut rt.dialog, DialogState::None);
    let (title, subtitle, width, height) = match &dialog {
        DialogState::NewBranch { .. } => (
            crate::tr!(model, "New branch", "新建分支"),
            crate::tr!(model, "Create a branch at HEAD. The branch is not checked out automatically.", "在 HEAD 处创建分支；不会自动检出。"),
            460.0,
            230.0,
        ),
        DialogState::NewWorktree { .. } => (
            crate::tr!(model, "New worktree", "新建工作树"),
            crate::tr!(
                model,
                "Creates a sibling directory next to the project, checked out at the current HEAD. Consoles and Git state stay independent per worktree.",
                "在项目同级目录创建一个新的检出（基于当前 HEAD），控制台与 Git 状态相互独立。"
            ),
            480.0,
            230.0,
        ),
        DialogState::RemoveWorktree { .. } => (
            crate::tr!(model, "Remove worktree", "删除工作树"),
            crate::tr!(
                model,
                "The directories of the worktree and its consoles are removed. Uncommitted changes require confirmation.",
                "将删除工作树目录及其控制台；未提交的更改需要确认。"
            ),
            470.0,
            210.0,
        ),
        DialogState::DeleteBranch { .. } => (
            crate::tr!(model, "Delete branch", "删除分支"),
            crate::tr!(model, "The branch and its reflog entry are removed. Commits stay reachable until garbage collection.", "将删除该分支及其 reflog 记录；提交在垃圾回收前仍可访问。"),
            440.0,
            210.0,
        ),
        DialogState::CloseDirtyTab { .. } => (
            crate::tr!(model, "Discard changes?", "放弃更改？"),
            crate::tr!(model, "This tab has unsaved changes. Closing it will discard them.", "此标签页有未保存的更改，关闭将丢弃它们。"),
            440.0,
            210.0,
        ),
        DialogState::Quit => (
            crate::tr!(model, "Quit LazeCode?", "退出 LazeCode？"),
            crate::tr!(model, "Tabs with unsaved changes will lose their edits unless you cancel and save them first.", "未保存的标签页将丢失更改，请取消并先保存。"),
            440.0,
            210.0,
        ),
        DialogState::None => return,
    };
    ui.painter()
        .rect_filled(screen, 0.0, Color32::from_black_alpha(140));
    let rect = Rect::from_center_size(screen.center(), Vec2::new(width, height));
    frame(ui, rect, theme);
    ui.painter().text(
        Pos2::new(rect.left() + 22.0, rect.top() + 28.0),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(15.0),
        p.text,
    );
    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(rect.left() + 22.0, rect.top() + 46.0),
            Pos2::new(rect.right() - 22.0, rect.top() + 86.0),
        ),
        subtitle,
        FontId::proportional(10.5),
        p.text3,
        Align2::LEFT_CENTER,
    );
    let mut close = false;
    let mut confirm = false;
    match &mut dialog {
        DialogState::NewBranch { value, error } => {
            let input = Rect::from_min_size(
                Pos2::new(rect.left() + 22.0, rect.top() + 94.0),
                Vec2::new(rect.width() - 44.0, 30.0),
            );
            ui.painter()
                .rect_filled(input, egui::CornerRadius::same(5), p.panel2);
            ui.painter().rect_stroke(
                input,
                egui::CornerRadius::same(5),
                Stroke::new(1.0, p.border),
                StrokeKind::Inside,
            );
            let response = ui.put(
                input.shrink2(Vec2::new(8.0, 4.0)),
                TextEdit::singleline(value)
                    .frame(egui::Frame::NONE)
                    .hint_text("feat/new-branch")
                    .font(FontId::monospace(11.0)),
            );
            if rt.branch_create_error.is_none() {
                response.request_focus();
            }
            if let Some(err) = error {
                ui.painter().text(
                    Pos2::new(input.left(), input.bottom() + 12.0),
                    Align2::LEFT_CENTER,
                    err,
                    FontId::proportional(9.5),
                    p.rose,
                );
            }
            if ui.input(|input| input.key_pressed(Key::Enter)) {
                confirm = true;
            }
        }
        DialogState::NewWorktree { value, error } => {
            let input = Rect::from_min_size(
                Pos2::new(rect.left() + 22.0, rect.top() + 94.0),
                Vec2::new(rect.width() - 44.0, 30.0),
            );
            ui.painter()
                .rect_filled(input, egui::CornerRadius::same(5), p.panel2);
            ui.painter().rect_stroke(
                input,
                egui::CornerRadius::same(5),
                Stroke::new(1.0, p.border),
                StrokeKind::Inside,
            );
            let response = ui.put(
                input.shrink2(Vec2::new(8.0, 4.0)),
                TextEdit::singleline(value)
                    .frame(egui::Frame::NONE)
                    .hint_text("my-feature")
                    .font(FontId::monospace(11.0)),
            );
            response.request_focus();
            if let Some(err) = error {
                ui.painter().text(
                    Pos2::new(input.left(), input.bottom() + 12.0),
                    Align2::LEFT_CENTER,
                    err,
                    FontId::proportional(9.5),
                    p.rose,
                );
            }
            if ui.input(|input| input.key_pressed(Key::Enter)) {
                confirm = true;
            }
        }
        DialogState::DeleteBranch { name, .. } => {
            text_clipped(
                ui.painter(),
                Rect::from_min_max(
                    Pos2::new(rect.left() + 22.0, rect.top() + 96.0),
                    Pos2::new(rect.right() - 22.0, rect.top() + 118.0),
                ),
                name,
                FontId::monospace(11.0),
                p.amber,
                Align2::LEFT_CENTER,
            );
        }
        DialogState::RemoveWorktree { name, .. } => {
            text_clipped(
                ui.painter(),
                Rect::from_min_max(
                    Pos2::new(rect.left() + 22.0, rect.top() + 96.0),
                    Pos2::new(rect.right() - 22.0, rect.top() + 118.0),
                ),
                name,
                FontId::monospace(11.0),
                p.amber,
                Align2::LEFT_CENTER,
            );
        }
        DialogState::CloseDirtyTab { name, .. } => {
            text_clipped(
                ui.painter(),
                Rect::from_min_max(
                    Pos2::new(rect.left() + 22.0, rect.top() + 96.0),
                    Pos2::new(rect.right() - 22.0, rect.top() + 118.0),
                ),
                name,
                FontId::monospace(11.0),
                p.amber,
                Align2::LEFT_CENTER,
            );
        }
        DialogState::Quit => {}
        DialogState::None => {}
    }
    let cancel = Rect::from_min_size(
        Pos2::new(rect.right() - 208.0, rect.bottom() - 48.0),
        Vec2::new(88.0, 30.0),
    );
    if secondary_button(ui, cancel, crate::tr!(model, "Cancel", "取消"), None, theme).clicked() {
        close = true;
    }
    let confirm_rect = Rect::from_min_size(
        Pos2::new(rect.right() - 112.0, rect.bottom() - 48.0),
        Vec2::new(90.0, 30.0),
    );
    let confirm_label = match &dialog {
        DialogState::NewBranch { .. } => crate::tr!(model, "Create", "创建"),
        DialogState::NewWorktree { .. } => crate::tr!(model, "Create", "创建"),
        DialogState::RemoveWorktree { .. } => crate::tr!(model, "Remove", "删除"),
        DialogState::DeleteBranch { .. } => crate::tr!(model, "Delete", "删除"),
        DialogState::Quit => crate::tr!(model, "Quit", "退出"),
        DialogState::CloseDirtyTab { .. } => crate::tr!(model, "Discard", "放弃修改"),
        _ => "Discard",
    };
    if primary_button(ui, confirm_rect, confirm_label, None, true, theme).clicked() {
        confirm = true;
    }
    if confirm {
        match &dialog {
            DialogState::NewBranch { value, .. } => {
                let value = value.trim().to_string();
                if value.is_empty() {
                    rt.dialog = DialogState::NewBranch {
                        value,
                        error: Some(
                            crate::tr!(model, "Enter a branch name", "请输入分支名").to_string(),
                        ),
                    };
                    close = true;
                } else {
                    model.queue(Command::CreateBranch(value));
                    close = true;
                }
            }
            DialogState::NewWorktree { value, .. } => {
                let value = value.trim().to_string();
                if value.is_empty() {
                    rt.dialog = DialogState::NewWorktree {
                        value,
                        error: Some(
                            crate::tr!(model, "Enter a worktree name", "请输入工作树名")
                                .to_string(),
                        ),
                    };
                    close = true;
                } else {
                    model.queue(Command::CreateWorktree { name: value });
                    close = true;
                }
            }
            DialogState::RemoveWorktree { name, .. } => {
                model.queue(Command::RemoveWorktree {
                    name: name.clone(),
                    force: false,
                });
                close = true;
            }
            DialogState::DeleteBranch { name, .. } => {
                model.queue(Command::DeleteBranch(name.clone()));
                close = true;
            }
            DialogState::CloseDirtyTab { tab_id, .. } => {
                if let Some(project) = model.active_project_mut() {
                    project.close_tab(*tab_id);
                }
                if rt.editor_focus == Some(*tab_id) {
                    rt.editor_focus = None;
                }
                close = true;
            }
            DialogState::Quit => {
                model.should_quit = true;
                close = true;
            }
            DialogState::None => {}
        }
    }
    if !close {
        rt.dialog = dialog;
    } else {
        rt.branch_create_error = None;
    }
    let _ = (model, ToastKind::Info);
}
