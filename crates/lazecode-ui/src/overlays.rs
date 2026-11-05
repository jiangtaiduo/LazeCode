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
            MenuKind::LogBranchFilter { project } => {
                draw_log_branch_menu(ui, model, rt, theme, screen, menu, project)
            }
            MenuKind::LogCommitMenu { project } => {
                let payload = menu.payload.clone();
                draw_commit_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref(),
                )
            }
            MenuKind::BranchContext { project } => {
                let payload = menu.payload.clone();
                draw_branch_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref(),
                )
            }
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

/// Branch/global filter dropdown of the Git log view.
fn draw_log_branch_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    index: usize,
) {
    let p = theme.palette;
    let Some(model_project) = model.projects.get(index) else {
        return;
    };
    // Payload present: the caller supplied author names; otherwise the branch
    // list.
    let (is_author_menu, rows_vec, active) = if let Some(payload) = menu.payload.clone() {
        (
            true,
            payload.lines().map(str::to_string).collect::<Vec<_>>(),
            model_project.log_query.author.clone(),
        )
    } else {
        let branches = model_project
            .branches
            .iter()
            .map(|branch| branch.name.clone())
            .collect();
        let active = if model_project.log_query.all_refs {
            "__all__".to_string()
        } else {
            model_project.log_query.branch.clone()
        };
        (false, branches, active)
    };
    // The immutable borrow of `model.projects` ended above; the overlay only
    // reads cloned data afterwards.
    let height = 66.0 + (rows_vec.len() as f32 * 30.0).min(320.0) + 8.0;
    let rect = popover_rect(screen, menu.anchor, 300.0, height);
    frame(ui, rect, theme);
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, rect.top() + 18.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Log branch filter", "日志分支过滤"),
        FontId::proportional(11.0),
        p.text3,
    );
    let pick_title = if is_author_menu {
        crate::tr!(model, "Filter by author", "按作者过滤")
    } else {
        crate::tr!(model, "Log branch filter", "日志分支过滤")
    };
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, rect.top() + 18.0),
        Align2::LEFT_CENTER,
        pick_title,
        FontId::proportional(11.0),
        p.text3,
    );
    let mut pick: Option<Option<String>> = None;
    let rows = Rect::from_min_max(
        Pos2::new(rect.left() + 6.0, rect.top() + 32.0),
        Pos2::new(rect.right() - 6.0, rect.bottom() - 6.0),
    );
    super::app_ui::region(ui, rows, "log-branch-menu", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("log-branch-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if is_author_menu {
                    // Author rows. "All" row first.
                    let (row, response) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
                    if response.hovered() {
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::same(4), p.hover);
                    }
                    ui.painter().text(
                        Pos2::new(row.left() + 14.0, row.center().y),
                        Align2::LEFT_CENTER,
                        crate::tr!(model, "All authors", "全部作者"),
                        FontId::proportional(10.5),
                        p.text,
                    );
                    if active.is_empty() {
                        paint_icon(
                            ui.painter(),
                            Rect::from_center_size(
                                Pos2::new(row.right() - 16.0, row.center().y),
                                Vec2::splat(10.0),
                            ),
                            Icon::Check,
                            p.accent,
                        );
                    }
                    if response.clicked() {
                        pick = Some(None);
                    }
                    for name in &rows_vec {
                        let (row, response) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 30.0),
                            Sense::click(),
                        );
                        if response.hovered() {
                            ui.painter()
                                .rect_filled(row, egui::CornerRadius::same(4), p.hover);
                        }
                        ui.painter().circle_filled(
                            Pos2::new(row.left() + 14.0, row.center().y),
                            3.2,
                            p.accent,
                        );
                        ui.painter().text(
                            Pos2::new(row.left() + 32.0, row.center().y),
                            Align2::LEFT_CENTER,
                            name,
                            FontId::proportional(10.5),
                            p.text,
                        );
                        if active == *name {
                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(row.right() - 16.0, row.center().y),
                                    Vec2::splat(10.0),
                                ),
                                Icon::Check,
                                p.accent,
                            );
                        }
                        if response.clicked() {
                            pick = Some(Some(name.clone()));
                        }
                    }
                    return;
                }
                // Branch menu: All branches row.
                let (row, response) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
                if response.hovered() {
                    ui.painter()
                        .rect_filled(row, egui::CornerRadius::same(4), p.hover);
                }
                paint_icon(
                    ui.painter(),
                    Rect::from_center_size(
                        Pos2::new(row.left() + 14.0, row.center().y),
                        Vec2::splat(12.0),
                    ),
                    Icon::GitLog,
                    p.text2,
                );
                ui.painter().text(
                    Pos2::new(row.left() + 32.0, row.center().y),
                    Align2::LEFT_CENTER,
                    crate::tr!(model, "All branches", "全部分支"),
                    FontId::proportional(10.5),
                    p.text,
                );
                if active == "__all__" {
                    paint_icon(
                        ui.painter(),
                        Rect::from_center_size(
                            Pos2::new(row.right() - 16.0, row.center().y),
                            Vec2::splat(10.0),
                        ),
                        Icon::Check,
                        p.accent,
                    );
                }
                if response.clicked() {
                    pick = Some(Some("__all__".to_string()));
                }
                for name in &rows_vec {
                    let (row, response) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
                    if response.hovered() {
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::same(4), p.hover);
                    }
                    ui.painter().circle_filled(
                        Pos2::new(row.left() + 14.0, row.center().y),
                        3.2,
                        p.green,
                    );
                    ui.painter().text(
                        Pos2::new(row.left() + 32.0, row.center().y),
                        Align2::LEFT_CENTER,
                        name,
                        FontId::proportional(10.5),
                        p.text,
                    );
                    if active == *name {
                        paint_icon(
                            ui.painter(),
                            Rect::from_center_size(
                                Pos2::new(row.right() - 16.0, row.center().y),
                                Vec2::splat(10.0),
                            ),
                            Icon::Check,
                            p.accent,
                        );
                    }
                    if response.clicked() {
                        pick = Some(Some(name.clone()));
                    }
                }
                // Head row (default).
                let (row, response) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::click());
                if response.hovered() {
                    ui.painter()
                        .rect_filled(row, egui::CornerRadius::same(4), p.hover);
                }
                ui.painter().circle_filled(
                    Pos2::new(row.left() + 14.0, row.center().y),
                    3.2,
                    p.accent,
                );
                ui.painter().text(
                    Pos2::new(row.left() + 32.0, row.center().y),
                    Align2::LEFT_CENTER,
                    crate::tr!(model, "Current branch only", "仅当前分支"),
                    FontId::proportional(10.5),
                    p.text,
                );
                if active.is_empty() {
                    paint_icon(
                        ui.painter(),
                        Rect::from_center_size(
                            Pos2::new(row.right() - 16.0, row.center().y),
                            Vec2::splat(10.0),
                        ),
                        Icon::Check,
                        p.accent,
                    );
                }
                if response.clicked() {
                    pick = Some(None);
                }
            });
    });
    let mut close = false;
    if let Some(picked) = pick {
        if is_author_menu {
            model.ui.log_author_pick = Some((index, picked));
        } else {
            model.ui.log_branch_pick = Some((index, picked));
        }
        close = true;
    }
    if outside_click(ui, rect) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

/// Commit context menu: read-only actions in this milestone
/// (copy hash / go to parent / child; history rewrite arrives with M5).
#[allow(clippy::too_many_arguments)]
fn draw_commit_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    index: usize,
    commit_id: Option<&str>,
) {
    let Some(commit_id) = commit_id else { return };
    let p = theme.palette;
    let Some(commit) = model.projects[index]
        .log
        .iter()
        .find(|commit| commit.id == commit_id)
        .cloned()
    else {
        return;
    };
    let height = 36.0 + 9.0 * 32.0 + 14.0;
    let rect = popover_rect(screen, menu.anchor, 250.0, height);
    frame(ui, rect, theme);
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, rect.top() + 16.0),
        Align2::LEFT_CENTER,
        &commit.short,
        FontId::monospace(10.0),
        p.accent,
    );
    let mut action: Option<u8> = None;
    let items: [(&str, u8); 9] = [
        (crate::tr!(model, "Copy revision number", "复制提交哈希"), 0),
        (crate::tr!(model, "Compare with local", "与本地比较"), 1),
        (crate::tr!(model, "Go to parent commit", "跳转到父提交"), 2),
        (crate::tr!(model, "Go to child commit", "跳转到子提交"), 3),
        (
            crate::tr!(model, "Cherry-pick commit", "Cherry-pick 该提交"),
            4,
        ),
        (crate::tr!(model, "Revert commit", "Revert 该提交"), 5),
        (crate::tr!(model, "Drop commit", "Drop 该提交"), 6),
        (
            crate::tr!(model, "Reset current branch to here…", "重置分支到此处…"),
            7,
        ),
        (
            crate::tr!(model, "New tag at this commit…", "在此提交上打标签…"),
            8,
        ),
    ];
    for (i, (label, code)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, rect.top() + 30.0 + i as f32 * 32.0),
            Vec2::new(rect.width() - 16.0, 30.0),
        );
        let response = ui.interact(row, ui.scope_id().with(("commitmenu", i)), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(4), p.hover);
        }
        ui.painter().text(
            Pos2::new(row.left() + 8.0, row.center().y),
            Align2::LEFT_CENTER,
            *label,
            FontId::proportional(10.5),
            p.text2,
        );
        if response.clicked() {
            action = Some(*code);
        }
    }
    let mut close = false;
    if let Some(code) = action {
        match code {
            0 => copy_to_clipboard(ui, &commit.id),
            1 => {
                if let Some(file) = model.projects[index]
                    .commit_details
                    .as_ref()
                    .and_then(|details| details.files.first())
                    .map(|file| file.path.clone())
                {
                    model.queue(Command::OpenDiff(format!("{}::{}", commit.id, file)));
                } else {
                    model.queue(Command::OpenDiff(commit.id.clone()));
                }
            }
            2 => {
                if let Some(parent) = commit.parents.first() {
                    let parent = parent.clone();
                    model.projects[index].selected_commit = Some(parent.clone());
                    model.queue(Command::SelectCommit(parent));
                }
            }
            3 => {
                if let Some(child) = model.projects[index]
                    .log
                    .iter()
                    .find(|probe| probe.parents.iter().any(|parent| parent == &commit.id))
                    .map(|child| child.id.clone())
                {
                    model.projects[index].selected_commit = Some(child.clone());
                    model.queue(Command::SelectCommit(child));
                }
            }
            4 => model.queue(Command::CherryPick {
                hash: commit.id.clone(),
            }),
            5 => model.queue(Command::RevertCommit {
                hash: commit.id.clone(),
            }),
            6 => model.queue(Command::DropCommit {
                hash: commit.id.clone(),
                force: false,
            }),
            7 => {
                rt.dialog = DialogState::Reset {
                    hash: commit.id.clone(),
                    mode: lazecode_git::ResetMode::Mixed,
                };
            }
            8 => {
                rt.dialog = DialogState::NewTag {
                    commit: commit.short.clone(),
                    value: String::new(),
                    error: None,
                };
            }
            _ => {}
        }
        close = true;
    }
    if outside_click(ui, rect) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

fn copy_to_clipboard(ui: &mut Ui, text: &str) {
    ui.ctx().copy_text(text.to_owned());
}

/// Git log branch tree context menu. Items differ by remote/local.
/// Payload format: `name\u{1}local|remote`.
#[allow(clippy::too_many_arguments)]
fn draw_branch_context_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    index: usize,
    payload: Option<&str>,
) {
    let Some((name, remote_flag)) = payload.and_then(|p| {
        let (name, kind) = p.split_once('\u{1}')?;
        Some((name.to_string(), kind.to_string()))
    }) else {
        return;
    };
    let remote = remote_flag == "remote";
    let p = theme.palette;
    // items list (label key, action id)
    let items: Vec<(&str, u8)> = if remote {
        vec![
            (crate::tr!(model, "Checkout branch", "检出分支"), 0),
            (
                crate::tr!(model, "Compare with working tree", "与工作区比较"),
                1,
            ),
            (crate::tr!(model, "Set log filter", "设为日志过滤"), 2),
        ]
    } else {
        vec![
            (crate::tr!(model, "Checkout branch", "检出分支"), 0),
            (
                crate::tr!(model, "New branch from here", "从这里新建分支"),
                1,
            ),
            (crate::tr!(model, "Rename branch", "重命名"), 2),
            (crate::tr!(model, "Delete branch", "删除分支"), 3),
            (
                crate::tr!(model, "Compare with working tree", "与工作区比较"),
                4,
            ),
            (crate::tr!(model, "Set log filter", "设为日志过滤"), 6),
        ]
    };
    let is_fav = model.favorite_branches.contains(&name);
    let fav_label = if is_fav {
        crate::tr!(model, "Remove favorite", "取消收藏")
    } else {
        crate::tr!(model, "Mark favorite", "标记收藏")
    };
    let height = items.len() as f32 * 28.0 + 66.0;
    let rect = popover_rect(screen, menu.anchor, 246.0, height);
    frame(ui, rect, theme);
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, rect.top() + 16.0),
        Align2::LEFT_CENTER,
        &name,
        FontId::monospace(10.0),
        p.accent,
    );
    let mut action: Option<u8> = None;
    let mut toggle_fav = false;
    let row_start = rect.top() + 32.0;
    for (i, (label, code)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, row_start + i as f32 * 28.0),
            Vec2::new(rect.width() - 16.0, 28.0),
        );
        let response = ui.interact(row, ui.scope_id().with(("bctx", i)), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
        }
        ui.painter().text(
            Pos2::new(row.left() + 8.0, row.center().y),
            Align2::LEFT_CENTER,
            *label,
            FontId::proportional(10.5),
            p.text2,
        );
        if response.clicked() {
            action = Some(*code);
        }
    }
    // favorite row last.
    let fav_row = Rect::from_min_size(
        Pos2::new(rect.left() + 8.0, row_start + items.len() as f32 * 28.0),
        Vec2::new(rect.width() - 16.0, 28.0),
    );
    let fav_response = ui.interact(fav_row, ui.scope_id().with("bctx-fav"), Sense::click());
    if fav_response.hovered() {
        ui.painter()
            .rect_filled(fav_row, egui::CornerRadius::same(3), p.hover);
    }
    ui.painter().text(
        Pos2::new(fav_row.left() + 8.0, fav_row.center().y),
        Align2::LEFT_CENTER,
        fav_label,
        FontId::proportional(10.5),
        p.text2,
    );
    if fav_response.clicked() {
        toggle_fav = true;
    }
    let mut close = false;
    if toggle_fav {
        if is_fav {
            model.favorite_branches.remove(&name);
        } else {
            model.favorite_branches.insert(name.clone());
        }
        model.save_workspace();
        close = true;
    }
    if let Some(code) = action {
        match code {
            0 => model.queue(Command::CheckoutBranch(name.clone())),
            1 => {
                rt.dialog = DialogState::NewBranch {
                    value: name.clone(),
                    error: None,
                };
            }
            2 => {
                rt.dialog = DialogState::RenameBranch {
                    from: name.clone(),
                    value: name.clone(),
                    error: None,
                };
            }
            3 => {
                rt.dialog = DialogState::DeleteBranch {
                    name: name.clone(),
                    error: None,
                };
            }
            4 => {
                model.queue(Command::OpenDiff(name.clone()));
            }
            6 => {
                if let Some(project) = model.projects.get_mut(index) {
                    project.log_query.branch = name.clone();
                    project.log_query.all_refs = false;
                    project.log_loading = true;
                }
                model.queue(Command::LogFilterChanged);
            }
            _ => {}
        }
        close = true;
    }
    if outside_click(ui, rect) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
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
    let mut confirm_add_remote = false;
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
        DialogState::RenameBranch { .. } => (
            crate::tr!(model, "Rename branch", "重命名分支"),
            crate::tr!(model, "The ref name of a local branch is rewritten in place.", "直接重命名本地分支的引用名。"),
            460.0,
            220.0,
        ),
        DialogState::Push { .. } => (
            crate::tr!(model, "Push", "推送"),
            crate::tr!(model, "Publish the local branch to the remote (creates the upstream when chosen).", "将本地分支推送到远程（可选建立 upstream）。"),
            460.0,
            250.0,
        ),
        DialogState::Pull { .. } => (
            crate::tr!(model, "Pull", "拉取"),
            crate::tr!(model, "Fetch and integrate upstream changes (merge/rebase/ff-only).", "拉取并集成上游变更（merge/rebase/仅快进）。"),
            470.0,
            280.0,
        ),
        DialogState::Remotes { .. } => (
            crate::tr!(model, "Manage remotes", "管理远程"),
            crate::tr!(model, "Add, rename, re-point or delete remote repositories.", "添加、重命名、修改或删除远程仓库。"),
            520.0,
            380.0,
        ),
        DialogState::Reset { .. } => (
            crate::tr!(model, "Reset current branch", "重置当前分支"),
            crate::tr!(model, "Soft keeps changes, mixed unstages, hard discards both index and working tree.", "soft 保留更改，mixed 取消暂存，hard 连暂存和工作区一起丢弃。"),
            470.0,
            300.0,
        ),
        DialogState::NewTag { .. } => (
            crate::tr!(model, "New tag", "新建标签"),
            crate::tr!(model, "Tags mark a revision for later comparison or releasing.", "标签用于标记一个提交，以供后续比较或发布。"),
            460.0,
            240.0,
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
        DialogState::RenameBranch { from, value, error } => {
            let input = Rect::from_min_size(
                Pos2::new(rect.left() + 22.0, rect.top() + 90.0),
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
                    .hint_text(from.clone())
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
        DialogState::Push {
            remote,
            branch,
            set_upstream,
            status,
        } => {
            ui.painter().text(
                Pos2::new(rect.left() + 22.0, rect.top() + 92.0),
                Align2::LEFT_CENTER,
                crate::tr!(model, "Remote", "远程"),
                FontId::proportional(9.5),
                p.text3,
            );
            ui.painter().text(
                Pos2::new(rect.left() + 90.0, rect.top() + 92.0),
                Align2::LEFT_CENTER,
                remote,
                FontId::monospace(11.0),
                p.text,
            );
            ui.painter().text(
                Pos2::new(rect.left() + 22.0, rect.top() + 118.0),
                Align2::LEFT_CENTER,
                crate::tr!(model, "Branch", "分支"),
                FontId::proportional(9.5),
                p.text3,
            );
            ui.painter().text(
                Pos2::new(rect.left() + 90.0, rect.top() + 118.0),
                Align2::LEFT_CENTER,
                branch,
                FontId::monospace(11.0),
                p.text,
            );
            let checkbox_y = rect.top() + 146.0;
            let upstream_rect = Rect::from_center_size(
                Pos2::new(rect.left() + 30.0, checkbox_y),
                Vec2::splat(16.0),
            );
            if crate::widgets::checkbox(ui, upstream_rect, *set_upstream, theme).clicked() {
                // `set_upstream` is a field captured from dialog, so flip it
                // directly (checkbox closure takes the value by copy).
            }
            ui.painter().text(
                Pos2::new(rect.left() + 36.0, checkbox_y),
                Align2::LEFT_CENTER,
                crate::tr!(
                    model,
                    "Set upstream (first push)",
                    "建立 upstream（首次推送）"
                ),
                FontId::proportional(10.0),
                p.text2,
            );
            if let Some(status) = status {
                ui.painter().text(
                    Pos2::new(rect.left() + 22.0, rect.bottom() - 66.0),
                    Align2::LEFT_CENTER,
                    status,
                    FontId::proportional(9.5),
                    p.rose,
                );
            }
            if ui.input(|input| input.key_pressed(Key::Enter)) {
                confirm = true;
            }
        }
        DialogState::Pull { mode, status, .. } => {
            let modes = [
                (
                    lazecode_git::PullMode::Merge,
                    crate::tr!(model, "Merge", "合并"),
                ),
                (
                    lazecode_git::PullMode::Rebase,
                    crate::tr!(model, "Rebase", "变基"),
                ),
                (
                    lazecode_git::PullMode::FastForwardOnly,
                    crate::tr!(model, "Fast-forward only", "仅快进"),
                ),
            ];
            for (candidate, label) in modes {
                let row = Rect::from_min_size(
                    Pos2::new(
                        rect.left() + 22.0,
                        rect.top() + 96.0 + (candidate as usize as f32) * 28.0,
                    ),
                    Vec2::new(rect.width() - 44.0, 26.0),
                );
                if ui
                    .interact(
                        row,
                        ui.scope_id().with(("pullmode", candidate as u8)),
                        Sense::click(),
                    )
                    .clicked()
                {
                    *mode = candidate;
                }
                let selected = *mode == candidate;
                ui.painter().circle_stroke(
                    Pos2::new(row.left() + 10.0, row.center().y),
                    5.0,
                    Stroke::new(1.3, if selected { p.accent } else { p.text3 }),
                );
                if selected {
                    ui.painter().circle_filled(
                        Pos2::new(row.left() + 10.0, row.center().y),
                        2.4,
                        p.accent,
                    );
                }
                ui.painter().text(
                    Pos2::new(row.left() + 22.0, row.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    FontId::proportional(10.5),
                    p.text,
                );
            }
            if let Some(status) = status {
                ui.painter().text(
                    Pos2::new(rect.left() + 22.0, rect.bottom() - 66.0),
                    Align2::LEFT_CENTER,
                    status,
                    FontId::proportional(9.5),
                    p.rose,
                );
            }
        }
        DialogState::Remotes {
            remotes,
            new_name,
            new_url,
            error,
        } => {
            let rows = Rect::from_min_max(
                Pos2::new(rect.left() + 10.0, rect.top() + 88.0),
                Pos2::new(rect.right() - 10.0, rect.bottom() - 96.0),
            );
            ui.painter().text(
                Pos2::new(rows.left(), rect.top() + 78.0),
                Align2::LEFT_CENTER,
                crate::tr!(model, "Existing remotes", "现有远程"),
                FontId::proportional(9.5),
                p.text3,
            );
            super::app_ui::region(ui, rows, "remotes-rows", |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("remotes-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for list_remote in remotes.iter() {
                            let (row, _) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 26.0),
                                Sense::hover(),
                            );
                            ui.painter().text(
                                Pos2::new(row.left() + 6.0, row.center().y - 5.0),
                                Align2::LEFT_CENTER,
                                &list_remote.name,
                                FontId::proportional(10.5),
                                p.text,
                            );
                            ui.painter().text(
                                Pos2::new(row.left() + 6.0, row.center().y + 6.0),
                                Align2::LEFT_CENTER,
                                &list_remote.url,
                                FontId::monospace(8.5),
                                p.text3,
                            );
                        }
                    });
            });
            let name_rect = Rect::from_min_size(
                Pos2::new(rect.left() + 12.0, rect.bottom() - 92.0),
                Vec2::new(120.0, 26.0),
            );
            ui.painter()
                .rect_filled(name_rect, egui::CornerRadius::same(4), p.panel2);
            let _ = ui.put(
                name_rect.shrink2(Vec2::new(6.0, 3.0)),
                TextEdit::singleline(new_name)
                    .frame(egui::Frame::NONE)
                    .hint_text(crate::tr!(model, "name", "名称"))
                    .font(FontId::monospace(10.0)),
            );
            let url_width = rect.width() - 12.0 - 120.0 - 8.0 - 74.0;
            let url_rect = Rect::from_min_size(
                Pos2::new(name_rect.right() + 8.0, name_rect.top()),
                Vec2::new(url_width, 26.0),
            );
            ui.painter()
                .rect_filled(url_rect, egui::CornerRadius::same(4), p.panel2);
            let _ = ui.put(
                url_rect.shrink2(Vec2::new(6.0, 3.0)),
                TextEdit::singleline(new_url)
                    .frame(egui::Frame::NONE)
                    .hint_text("https://github.com/user/repo.git")
                    .font(FontId::monospace(9.5)),
            );
            let add_rect = Rect::from_min_size(
                Pos2::new(url_rect.right() + 8.0, url_rect.top()),
                Vec2::new(64.0, 26.0),
            );
            if crate::widgets::primary_button(
                ui,
                add_rect,
                crate::tr!(model, "Add", "添加"),
                None,
                true,
                theme,
            )
            .clicked()
            {
                confirm_add_remote = true;
            }
            if let Some(err) = error {
                ui.painter().text(
                    Pos2::new(rect.left() + 22.0, rect.bottom() - 66.0),
                    Align2::LEFT_CENTER,
                    err,
                    FontId::proportional(9.5),
                    p.rose,
                );
            }
        }
        DialogState::Reset { hash, mode } => {
            ui.painter().text(
                Pos2::new(rect.left() + 22.0, rect.top() + 92.0),
                Align2::LEFT_CENTER,
                hash,
                FontId::monospace(11.0),
                p.text,
            );
            let modes = [
                (
                    lazecode_git::ResetMode::Soft,
                    crate::tr!(
                        model,
                        "Soft — keep changes staged",
                        "Soft — 保留更改（已暂存）"
                    ),
                ),
                (
                    lazecode_git::ResetMode::Mixed,
                    crate::tr!(model, "Mixed — unstage changes", "Mixed — 取消暂存"),
                ),
                (
                    lazecode_git::ResetMode::Hard,
                    crate::tr!(model, "Hard — discard all changes", "Hard — 丢弃全部更改"),
                ),
            ];
            for (candidate, label) in modes {
                let row = Rect::from_min_size(
                    Pos2::new(
                        rect.left() + 22.0,
                        rect.top() + 120.0 + (candidate as usize as f32) * 28.0,
                    ),
                    Vec2::new(rect.width() - 44.0, 26.0),
                );
                if ui
                    .interact(
                        row,
                        ui.scope_id().with(("resetmode", candidate as u8)),
                        Sense::click(),
                    )
                    .clicked()
                {
                    *mode = candidate;
                }
                let selected = *mode == candidate;
                ui.painter().circle_stroke(
                    Pos2::new(row.left() + 10.0, row.center().y),
                    5.0,
                    Stroke::new(1.3, if selected { p.accent } else { p.text3 }),
                );
                if selected {
                    ui.painter().circle_filled(
                        Pos2::new(row.left() + 10.0, row.center().y),
                        2.4,
                        p.accent,
                    );
                }
                ui.painter().text(
                    Pos2::new(row.left() + 22.0, row.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    FontId::proportional(10.5),
                    p.text,
                );
            }
        }
        DialogState::NewTag {
            commit: commit_hint,
            value,
            error,
        } => {
            ui.painter().text(
                Pos2::new(rect.left() + 22.0, rect.top() + 92.0),
                Align2::LEFT_CENTER,
                commit_hint,
                FontId::monospace(10.5),
                p.accent,
            );
            let input = Rect::from_min_size(
                Pos2::new(rect.left() + 22.0, rect.top() + 118.0),
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
                    .hint_text("v0.1.0")
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
        DialogState::RenameBranch { .. } => crate::tr!(model, "Rename", "重命名"),
        DialogState::Push { .. } => crate::tr!(model, "Push", "推送"),
        DialogState::Pull { .. } => crate::tr!(model, "Pull", "拉取"),
        DialogState::Remotes { .. } => crate::tr!(model, "Done", "完成"),
        DialogState::Reset { .. } => crate::tr!(model, "Reset", "重置"),
        DialogState::NewTag { .. } => crate::tr!(model, "Create", "创建"),
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
            DialogState::RenameBranch { from, value, .. } => {
                let value = value.trim().to_string();
                if value.is_empty() || value == *from {
                    rt.dialog = DialogState::RenameBranch {
                        from: from.clone(),
                        value,
                        error: Some(
                            crate::tr!(model, "Enter a new branch name", "请输入新的分支名")
                                .to_string(),
                        ),
                    };
                } else {
                    model.queue(Command::RenameBranch {
                        from: from.clone(),
                        to: value,
                    });
                    close = true;
                }
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
            DialogState::Push {
                remote,
                branch,
                set_upstream,
                ..
            } => {
                model.queue(Command::Push {
                    remote: remote.clone(),
                    branch: branch.clone(),
                    set_upstream: *set_upstream,
                    force: false,
                });
                close = true;
            }
            DialogState::Pull { mode, .. } => {
                model.queue(Command::Pull { mode: *mode });
                close = true;
            }
            DialogState::Remotes {
                remotes,
                new_name,
                new_url,
                ..
            } => {
                if confirm_add_remote {
                    let name = new_name.trim().to_string();
                    let url = new_url.trim().to_string();
                    if name.is_empty() || url.is_empty() {
                        rt.dialog = DialogState::Remotes {
                            remotes: remotes.clone(),
                            new_name: new_name.clone(),
                            new_url: new_url.clone(),
                            error: Some(
                                crate::tr!(model, "Enter a name and a URL", "请输入名称和 URL")
                                    .to_string(),
                            ),
                        };
                    } else {
                        model.queue(Command::RemoteAdd { name, url });
                        close = true;
                    }
                } else {
                    close = true;
                }
            }
            DialogState::Reset { hash, mode } => {
                model.queue(Command::ResetTo {
                    hash: hash.clone(),
                    mode: *mode,
                });
                close = true;
            }
            DialogState::NewTag {
                commit: commit_hint,
                value,
                ..
            } => {
                let name = value.trim().to_string();
                if name.is_empty() {
                    rt.dialog = DialogState::NewTag {
                        commit: commit_hint.clone(),
                        value: value.clone(),
                        error: Some(
                            crate::tr!(model, "Enter a tag name", "请输入标签名").to_string(),
                        ),
                    };
                } else {
                    model.queue(Command::CreateTag(name));
                    close = true;
                }
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
