use egui::{
    Align2, Color32, FontId, Key, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2,
};
use lazecode_core::{AppModel, Command, TabKind, ToastKind};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::{DialogState, MenuKind, MenuState, UiRuntime};
use crate::theme::Theme;
use crate::widgets::{checkbox, live_dot, primary_button, secondary_button, text_clipped};

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
            MenuKind::TreeContext { project, is_dir } => {
                let payload = menu.payload.clone();
                draw_tree_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                    is_dir,
                )
            }
            MenuKind::EditorTabContext { project, tab_id } => {
                let payload = menu.payload.clone();
                draw_editor_tab_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    tab_id,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::TerminalTabContext {
                project,
                terminal_id,
            } => {
                let payload = menu.payload.clone();
                draw_terminal_tab_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    terminal_id,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::ChangelistHeaderContext { project } => {
                let payload = menu.payload.clone();
                draw_changelist_header_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::ChangeFileContext { project } => {
                let payload = menu.payload.clone();
                draw_change_file_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::CommitHistory { project } => {
                draw_commit_history_menu(ui, model, rt, theme, screen, menu, project);
            }
            MenuKind::CompareUserFilter { side } => {
                draw_compare_user_menu(ui, model, rt, theme, screen, menu, side);
            }
            MenuKind::CompareDateFilter { side } => {
                draw_compare_date_menu(ui, model, rt, theme, screen, menu, side);
            }
            MenuKind::CompareRefFilter { side } => {
                draw_compare_ref_menu(ui, model, rt, theme, screen, menu, side);
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
    let is_point = anchor.width() <= 4.0 && anchor.height() <= 4.0;
    let x = if is_point {
        anchor
            .left()
            .min(screen.right() - width - 8.0)
            .max(screen.left() + 8.0)
    } else {
        (anchor.left() - 4.0)
            .min(screen.right() - width - 8.0)
            .max(screen.left() + 8.0)
    };
    let below = if is_point {
        anchor.top()
    } else {
        anchor.bottom() + 4.0
    };
    let y = if below + height < screen.bottom() - 8.0 {
        below
    } else {
        (anchor.top() - height - if is_point { 0.0 } else { 4.0 }).max(screen.top() + 8.0)
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

fn should_dismiss(ui: &Ui, rect: Rect, opened_frame: u64, current_frame: u64) -> bool {
    if ui.input(|input| input.key_pressed(Key::Escape)) {
        return true;
    }
    if opened_frame == current_frame {
        return false;
    }
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
        FontId::proportional(12.5),
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
        FontId::monospace(11.0),
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
            FontId::proportional(11.0),
            tone,
        );
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(row.left() + 40.0, row.top() + 4.0),
                Pos2::new(row.right() - 58.0, row.top() + 23.0),
            ),
            &project.name,
            FontId::proportional(13.0),
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
            FontId::proportional(11.0),
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
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
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
        FontId::proportional(12.5),
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
        FontId::monospace(11.0),
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
                        FontId::proportional(13.0),
                        p.text,
                    );
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(row.left() + 12.0, row.top() + 17.0),
                            Pos2::new(row.right() - 30.0, row.bottom()),
                        ),
                        &branch,
                        FontId::monospace(11.5),
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
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
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
        FontId::proportional(12.5),
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
        FontId::proportional(12.5),
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
                        FontId::proportional(12.5),
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
                            FontId::proportional(12.5),
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
                    FontId::proportional(12.5),
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
                        FontId::proportional(12.5),
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
                    FontId::proportional(12.5),
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
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
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
    let selected_commits = model.projects[index].selected_commits.clone();
    let is_multi = selected_commits.len() > 1;
    let is_head = model.projects[index]
        .log
        .first()
        .map(|c| c.id == commit.id)
        .unwrap_or(false);
    let items: Vec<(&str, u8)> = if is_multi {
        let mut list = vec![
            (
                crate::tr!(model, "Copy revision numbers", "复制选中的提交哈希"),
                0,
            ),
            (crate::tr!(model, "Squash commits…", "压缩选中的提交…"), 20),
            (
                crate::tr!(model, "Cherry-pick selected", "Cherry-pick 选中项"),
                21,
            ),
            (crate::tr!(model, "Revert selected", "Revert 选中项"), 22),
            (crate::tr!(model, "Drop selected", "Drop 选中项"), 23),
        ];
        if selected_commits.len() == 2 {
            list.insert(
                1,
                (
                    crate::tr!(model, "Compare versions", "比较所选两个版本"),
                    24,
                ),
            );
        }
        list
    } else {
        let mut list = vec![
            (crate::tr!(model, "Copy revision number", "复制提交哈希"), 0),
            (crate::tr!(model, "Compare with local", "与本地比较"), 1),
            (
                crate::tr!(model, "Interactive rebase from here…", "从此提交开始变基…"),
                10,
            ),
            (
                crate::tr!(model, "Checkout revision", "检出该版本 (游离 HEAD)"),
                11,
            ),
            (
                crate::tr!(model, "New branch from here…", "从这里新建分支…"),
                12,
            ),
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
            (
                crate::tr!(model, "Copy commit patch", "复制提交补丁 (Patch)"),
                15,
            ),
        ];
        if is_head {
            list.push((
                crate::tr!(model, "Undo HEAD commit", "撤销 HEAD 提交 (Undo)"),
                14,
            ));
        }
        list
    };
    let height = items.len() as f32 * 30.0 + 16.0;
    let rect = popover_rect(screen, menu.anchor, 250.0, height);
    frame(ui, rect, theme);
    // 稳定版提交右键菜单（CommitContextMenu）没有标题行。
    let mut action: Option<u8> = None;
    for (i, (label, code)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, rect.top() + 8.0 + i as f32 * 30.0),
            Vec2::new(rect.width() - 16.0, 28.0),
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
            FontId::proportional(12.5),
            p.text2,
        );
        if response.clicked() {
            action = Some(*code);
        }
    }
    let mut close = false;
    if let Some(code) = action {
        match code {
            0 => {
                if is_multi {
                    ui.ctx().copy_text(selected_commits.join("\n"));
                } else {
                    copy_to_clipboard(ui, &commit.id);
                }
            }
            1 => {
                // 「与本地比较」：提交 ↔ 工作区。走 DiffSource::CompareWithLocal
                // （path 空 = 取首个差异文件）；旧实现把 `{hash}::{file}`
                // 传给 OpenDiff，被当成工作区路径打开，文件永远不存在。
                let file = model.projects[index]
                    .commit_details
                    .as_ref()
                    .and_then(|details| details.files.first())
                    .map(|file| file.path.clone())
                    .unwrap_or_default();
                model.queue(Command::OpenCompareWithLocalDiff {
                    commit: commit.id.clone(),
                    path: file,
                });
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
            10 => {
                rt.dialog = DialogState::InteractiveRebase {
                    onto: commit.id.clone(),
                    todos: Vec::new(),
                };
            }
            11 => {
                model.queue(Command::CheckoutRevision(commit.id.clone()));
            }
            12 => {
                rt.dialog = DialogState::NewBranch {
                    value: String::new(),
                    error: None,
                };
            }
            14 => {
                model.queue(Command::UndoCommitHead);
            }
            20 => {
                let commits = selected_commits.clone();
                let default_msg = model.projects[index]
                    .log
                    .iter()
                    .filter(|c| commits.contains(&c.id))
                    .map(|c| c.message.trim())
                    .filter(|m| !m.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n\n");
                rt.dialog = DialogState::Squash {
                    commits,
                    value: default_msg,
                    error: None,
                };
            }
            21 => {
                for c in selected_commits {
                    model.queue(Command::CherryPick { hash: c });
                }
            }
            22 => {
                for c in selected_commits {
                    model.queue(Command::RevertCommit { hash: c });
                }
            }
            23 => {
                for c in selected_commits {
                    model.queue(Command::DropCommit {
                        hash: c,
                        force: false,
                    });
                }
            }
            24 if selected_commits.len() == 2 => {
                model.queue(Command::OpenCompareCommitsDiff {
                    from: selected_commits[0].clone(),
                    to: selected_commits[1].clone(),
                    path: String::new(),
                });
            }
            15 => {
                if let Some(project) = model.projects.get(index) {
                    if let Ok(patch) = lazecode_git::commit_patch(&project.root, &commit.id) {
                        ui.ctx().copy_text(patch);
                        model.toast(
                            crate::tr!(model, "Patch copied to clipboard", "已复制补丁到剪贴板")
                                .to_string(),
                            ToastKind::Info,
                        );
                    }
                }
            }
            _ => {}
        }
        close = true;
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
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
    let selected_refs: Vec<String> = model.projects[index].ref_selection.clone();
    let is_multi = selected_refs.len() > 1;
    let items: Vec<(&str, u8)> = if is_multi {
        let mut list = Vec::new();
        if selected_refs.len() == 2 {
            list.push((
                crate::tr!(model, "Compare branches…", "比较所选两个分支…"),
                70,
            ));
        }
        list.push((
            crate::tr!(model, "Delete selected branches", "删除选中的分支"),
            71,
        ));
        list
    } else if remote {
        vec![
            (crate::tr!(model, "Checkout branch", "检出分支"), 0),
            (
                crate::tr!(model, "New branch from here", "从这里新建分支"),
                1,
            ),
            (crate::tr!(model, "Compare branches…", "比较分支…"), 7),
            (crate::tr!(model, "Merge into current", "合并到当前分支"), 8),
            (
                crate::tr!(model, "Rebase current onto", "变基当前分支到此处"),
                9,
            ),
            (crate::tr!(model, "Set log filter", "设为日志过滤"), 2),
            (crate::tr!(model, "Delete branch", "删除分支"), 3),
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
            (crate::tr!(model, "Compare branches…", "比较分支…"), 7),
            (crate::tr!(model, "Merge into current", "合并到当前分支"), 8),
            (
                crate::tr!(model, "Rebase current onto", "变基当前分支到此处"),
                9,
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
    let height = items.len() as f32 * 28.0 + 16.0;
    let rect = popover_rect(screen, menu.anchor, 246.0, height);
    frame(ui, rect, theme);
    // 稳定版分支右键菜单没有标题行（BranchContextMenu 无 header）。
    let mut action: Option<u8> = None;
    let mut toggle_fav = false;
    let row_start = rect.top() + 8.0;
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
            FontId::proportional(12.5),
            p.text2,
        );
        if response.clicked() {
            action = Some(*code);
        }
    }
    // favorite row last.
    if !is_multi {
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
            FontId::proportional(12.5),
            p.text2,
        );
        if fav_response.clicked() {
            toggle_fav = true;
        }
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
            7 => {
                let current = model.projects[index].branch();
                crate::editor::open_compare_page(model, current, name.clone());
            }
            8 => {
                model.queue(Command::Merge {
                    ref_name: name.clone(),
                });
            }
            9 => {
                model.queue(Command::Rebase { onto: name.clone() });
            }
            70 => {
                if selected_refs.len() == 2 {
                    crate::editor::open_compare_page(
                        model,
                        selected_refs[0].clone(),
                        selected_refs[1].clone(),
                    );
                }
            }
            71 => {
                for b in selected_refs {
                    model.queue(Command::DeleteBranch(b));
                }
            }
            _ => {}
        }
        close = true;
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_tree_context_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    rel_path: &str,
    is_dir: bool,
) {
    let p = theme.palette;
    let project = &model.projects[project_idx];
    let abs_path = if rel_path.is_empty() {
        project.root.clone()
    } else {
        project.root.join(rel_path)
    };
    let dir_for_new = if is_dir {
        abs_path.clone()
    } else {
        abs_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| project.root.clone())
    };

    let zh = model.settings.language.is_zh();
    let mut items: Vec<(&str, u8)> = vec![
        (if zh { "新建文件…" } else { "New File…" }, 0),
        (
            if zh {
                "新建文件夹…"
            } else {
                "New Directory…"
            },
            1,
        ),
        (
            if zh {
                "复制绝对路径"
            } else {
                "Copy Absolute Path"
            },
            2,
        ),
        (
            if zh {
                "复制相对路径"
            } else {
                "Copy Relative Path"
            },
            3,
        ),
        (if zh { "重命名…" } else { "Rename…" }, 4),
        (if zh { "删除…" } else { "Delete…" }, 5),
        (
            if zh {
                "在访达中显示"
            } else {
                "Reveal in Finder"
            },
            6,
        ),
        (
            if zh {
                "在终端打开"
            } else {
                "Open in Terminal"
            },
            7,
        ),
    ];
    if !is_dir {
        items.push((
            if zh {
                "查看与 HEAD 差异"
            } else {
                "Show Diff vs HEAD"
            },
            8,
        ));
        items.push((
            if zh {
                "与本地比较"
            } else {
                "Compare with Local"
            },
            9,
        ));
    }
    items.push((
        if zh {
            "文件历史 (File History)"
        } else {
            "File History"
        },
        10,
    ));
    items.push((
        if zh {
            "放弃更改 (Rollback)"
        } else {
            "Rollback Changes"
        },
        11,
    ));

    let height = items.len() as f32 * 28.0 + 16.0;
    let rect = popover_rect(screen, menu.anchor, 220.0, height);
    frame(ui, rect, theme);

    // 稳定版项目树右键菜单（ProjectTreeContextMenu）没有标题行。
    let mut action: Option<u8> = None;
    let mut close = false;
    let row_start = rect.top() + 8.0;
    for (i, (label, code)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, row_start + i as f32 * 28.0),
            Vec2::new(rect.width() - 16.0, 26.0),
        );
        let response = ui.interact(row, ui.scope_id().with(("tctx", i)), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
        }
        ui.painter().text(
            Pos2::new(row.left() + 10.0, row.center().y),
            Align2::LEFT_CENTER,
            *label,
            FontId::proportional(12.5),
            if *code == 5 { p.rose } else { p.text },
        );
        if response.clicked() {
            action = Some(*code);
        }
    }

    if let Some(code) = action {
        match code {
            0 => {
                rt.dialog = DialogState::NewFile {
                    dir: dir_for_new,
                    value: String::new(),
                    error: None,
                };
            }
            1 => {
                rt.dialog = DialogState::NewDirectory {
                    dir: dir_for_new,
                    value: String::new(),
                    error: None,
                };
            }
            2 => {
                ui.ctx().copy_text(abs_path.to_string_lossy().to_string());
                model.toast(
                    if zh {
                        "已复制绝对路径".to_string()
                    } else {
                        "Absolute path copied".to_string()
                    },
                    ToastKind::Info,
                );
            }
            3 => {
                ui.ctx().copy_text(rel_path.to_string());
                model.toast(
                    if zh {
                        "已复制相对路径".to_string()
                    } else {
                        "Relative path copied".to_string()
                    },
                    ToastKind::Info,
                );
            }
            4 => {
                let name = abs_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                rt.dialog = DialogState::RenamePath {
                    old_path: abs_path,
                    value: name,
                    error: None,
                };
            }
            5 => {
                rt.dialog = DialogState::DeletePath {
                    path: abs_path,
                    is_dir,
                };
            }
            6 => {
                #[cfg(target_os = "macos")]
                {
                    let _ = std::process::Command::new("open")
                        .arg("-R")
                        .arg(&abs_path)
                        .spawn();
                }
                #[cfg(not(target_os = "macos"))]
                {
                    if let Some(parent) = abs_path.parent() {
                        let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
                    }
                }
            }
            7 => {
                let term_cwd = if is_dir {
                    abs_path
                } else {
                    abs_path.parent().unwrap_or(&abs_path).to_path_buf()
                };
                model.queue(Command::NewTerminalCommand {
                    title: term_cwd
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("shell")
                        .to_string(),
                    command: format!("cd {:?}", term_cwd),
                });
            }
            8 => {
                model.queue(Command::OpenDiff(rel_path.to_string()));
            }
            9 => {
                model.queue(Command::OpenCompareWithLocalDiff {
                    commit: "HEAD".to_string(),
                    path: rel_path.to_string(),
                });
            }
            10 => {
                rt.dialog = DialogState::FileHistory {
                    path: rel_path.to_string(),
                    entries: Vec::new(),
                    selected: 0,
                    loaded: false,
                };
            }
            11 => {
                rt.dialog = DialogState::ConfirmDiscard {
                    paths: vec![rel_path.to_string()],
                };
            }
            _ => {}
        }
        close = true;
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_editor_tab_context_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    tab_id: u64,
    path: &str,
) {
    let p = theme.palette;
    // 条目与禁用规则对齐稳定版 EditorTabBar 菜单：关闭左侧在首标签禁用，
    // 关闭右侧在末标签禁用；「关闭未修改」与路径三件套是新版补充项。
    let (is_first, is_last) = model
        .projects
        .get(project_idx)
        .map(|project| {
            let count = project.tabs.len();
            let pos = project.tabs.iter().position(|tab| tab.id == tab_id);
            (pos == Some(0), pos == Some(count.saturating_sub(1)))
        })
        .unwrap_or((false, false));
    let items: Vec<(String, u8, bool)> = vec![
        (crate::tr!(model, "Close", "关闭").to_string(), 0, true),
        (
            crate::tr!(model, "Close others", "关闭其他").to_string(),
            1,
            true,
        ),
        (
            crate::tr!(model, "Close to the left", "关闭左侧标签").to_string(),
            8,
            !is_first,
        ),
        (
            crate::tr!(model, "Close to the right", "关闭右侧标签").to_string(),
            2,
            !is_last,
        ),
        (String::new(), u8::MAX, true),
        (
            crate::tr!(model, "Close all", "关闭全部").to_string(),
            4,
            true,
        ),
        (
            crate::tr!(model, "Close unmodified", "关闭未修改标签").to_string(),
            3,
            true,
        ),
        (String::new(), u8::MAX, true),
        (
            crate::tr!(model, "Copy path", "复制绝对路径").to_string(),
            5,
            true,
        ),
        (
            crate::tr!(model, "Copy relative path", "复制相对路径").to_string(),
            6,
            true,
        ),
        (
            crate::tr!(model, "Reveal in Finder", "在访达中显示").to_string(),
            7,
            true,
        ),
    ];

    let height = items.len() as f32 * 28.0 + 16.0;
    let rect = popover_rect(screen, menu.anchor, 200.0, height);
    frame(ui, rect, theme);

    // 稳定版右键菜单没有标题行（对齐 EditorTabBar ctx menu），直接列菜单项。
    let mut action: Option<u8> = None;
    let mut close = false;
    let row_start = rect.top() + 8.0;
    for (i, (label, code, enabled)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, row_start + i as f32 * 28.0),
            Vec2::new(rect.width() - 16.0, 26.0),
        );
        if *code == u8::MAX {
            // 分隔线（稳定版菜单在关闭组与全部组之间有分隔线）。
            ui.painter().line_segment(
                [
                    Pos2::new(row.left() + 8.0, row.center().y),
                    Pos2::new(row.right() - 8.0, row.center().y),
                ],
                egui::Stroke::new(1.0, p.border),
            );
            continue;
        }
        let response = ui.interact(row, ui.scope_id().with(("etab_ctx", i)), Sense::click());
        if response.hovered() && *enabled {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
        }
        ui.painter().text(
            Pos2::new(row.left() + 10.0, row.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(12.5),
            if *enabled { p.text } else { p.text3 },
        );
        if response.clicked() && *enabled {
            action = Some(*code);
        }
    }

    if let Some(code) = action {
        if let Some(project) = model.projects.get_mut(project_idx) {
            match code {
                0 => project.close_tab(tab_id),
                1 => project.close_other_tabs(tab_id),
                2 => project.close_tabs_to_right(tab_id),
                3 => project.close_unmodified_tabs(),
                4 => project.close_all_tabs(),
                8 => project.close_tabs_to_left(tab_id),
                5 => {
                    ui.ctx().copy_text(path.to_string());
                    model.toast(
                        crate::tr!(model, "Path copied", "已复制路径").to_string(),
                        ToastKind::Info,
                    );
                }
                6 => {
                    let rel = std::path::Path::new(path)
                        .strip_prefix(&project.root)
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| path.to_string());
                    ui.ctx().copy_text(rel);
                    model.toast(
                        crate::tr!(model, "Relative path copied", "已复制相对路径").to_string(),
                        ToastKind::Info,
                    );
                }
                7 => {
                    #[cfg(target_os = "macos")]
                    {
                        let _ = std::process::Command::new("open")
                            .arg("-R")
                            .arg(path)
                            .spawn();
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        if let Some(parent) = std::path::Path::new(path).parent() {
                            let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
                        }
                    }
                }
                _ => {}
            }
        }
        close = true;
    }

    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_terminal_tab_context_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    terminal_id: u64,
    title: &str,
) {
    let p = theme.palette;
    let items = [
        (
            crate::tr!(model, "Copy working directory", "复制工作目录"),
            0,
        ),
        (crate::tr!(model, "Rename tab…", "重命名标签…"), 1),
        (crate::tr!(model, "Close", "关闭"), 2),
        (crate::tr!(model, "Close others", "关闭其他"), 3),
        (crate::tr!(model, "Close all", "关闭全部"), 4),
    ];

    let height = items.len() as f32 * 28.0 + 16.0;
    let rect = popover_rect(screen, menu.anchor, 200.0, height);
    frame(ui, rect, theme);

    // 稳定版右键菜单没有标题行，直接列菜单项。
    let mut action: Option<u8> = None;
    let mut close = false;
    let row_start = rect.top() + 8.0;
    for (i, (label, code)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, row_start + i as f32 * 28.0),
            Vec2::new(rect.width() - 16.0, 26.0),
        );
        let response = ui.interact(row, ui.scope_id().with(("ttab_ctx", i)), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
        }
        ui.painter().text(
            Pos2::new(row.left() + 10.0, row.center().y),
            Align2::LEFT_CENTER,
            *label,
            FontId::proportional(12.5),
            p.text,
        );
        if response.clicked() {
            action = Some(*code);
        }
    }

    if let Some(code) = action {
        if let Some(project) = model.projects.get_mut(project_idx) {
            match code {
                0 => {
                    let cwd = project.active_worktree().path.clone();
                    ui.ctx().copy_text(cwd.display().to_string());
                    model.toast(
                        crate::tr!(model, "Working directory copied", "已复制工作目录").to_string(),
                        ToastKind::Info,
                    );
                }
                1 => {
                    rt.dialog = DialogState::RenameTerminal {
                        terminal_id,
                        value: title.to_string(),
                        error: None,
                    };
                }
                2 => project.close_terminal(terminal_id),
                3 => project.close_other_terminals(terminal_id),
                4 => project.close_all_terminals(),
                _ => {}
            }
        }
        close = true;
    }

    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_commit_history_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
) {
    let p = theme.palette;
    let history = model.settings.commit_history.clone();
    let mut items: Vec<(String, u8)> = history
        .iter()
        .enumerate()
        .map(|(index, message)| (message.clone(), index as u8))
        .collect();
    if !items.is_empty() {
        items.push((
            crate::tr!(
                model,
                "Clear history (also removes it from this machine)",
                "清除历史（同时从本机删除）"
            )
            .to_string(),
            255,
        ));
    } else {
        items.push((
            crate::tr!(
                model,
                "Nothing has been committed on this machine yet",
                "本机还没有提交过"
            )
            .to_string(),
            254,
        ));
    }

    let height = items.len() as f32 * 26.0 + 24.0;
    let rect = popover_rect(screen, menu.anchor, 320.0, height);
    frame(ui, rect, theme);

    let mut action: Option<u8> = None;
    let mut close = false;
    for (i, (label, code)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, rect.top() + 12.0 + i as f32 * 26.0),
            Vec2::new(rect.width() - 16.0, 24.0),
        );
        let response = ui.interact(row, ui.scope_id().with(("hist", i)), Sense::click());
        if response.hovered() && *code < 254 {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
        }
        text_clipped(
            ui.painter(),
            row,
            label,
            FontId::proportional(11.5),
            if *code == 255 { p.rose } else { p.text2 },
            Align2::LEFT_CENTER,
        );
        if *code != 254 && response.clicked() {
            action = Some(*code);
        }
    }

    if let Some(code) = action {
        if code == 255 {
            model.settings.commit_history.clear();
            model.save_settings();
        } else if let Some(entry) = history.get(code as usize) {
            if let Some(project) = model.projects.get_mut(project_idx) {
                project.commit_message = entry.clone();
            }
        }
        close = true;
    }

    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_changelist_header_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    payload: &str,
) {
    // payload: 组名 \u{1} 组内全部文件路径（稳定版组头菜单 = New/Delete
    // Changelist + 整组文件菜单，条目与文件行右键完全一致）。
    let mut segments = payload.split('\u{1}');
    let Some(name) = segments.next() else {
        return;
    };
    let paths: Vec<String> = segments
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect();
    let Some(project) = model.projects.get(project_idx) else {
        return;
    };
    let is_default = name.eq_ignore_ascii_case("default") || name.eq_ignore_ascii_case("changes");
    let mut items: Vec<ChangeMenuItem> = Vec::new();
    items.push(change_menu_item(
        "New Changelist…".to_string(),
        100,
        false,
        true,
    ));
    if !is_default {
        items.push(change_menu_item(
            "Delete Changelist…".to_string(),
            101,
            true,
            true,
        ));
    }
    let mut menu_ctx_holder: Option<ChangeMenuCtx> = None;
    if !paths.is_empty() {
        change_menu_sep(&mut items);
        let (file_items, menu_ctx) = change_menu_items(model, project, &paths);
        items.extend(file_items);
        menu_ctx_holder = Some(menu_ctx);
    } else {
        change_menu_sep(&mut items);
        items.push(change_menu_item(
            "Refresh VCS Changes".to_string(),
            7,
            false,
            true,
        ));
    }

    let height = items.len() as f32 * 28.0 + 16.0;
    let rect = popover_rect(screen, menu.anchor, 280.0, height);
    frame(ui, rect, theme);

    // 稳定版右键菜单没有标题行，直接列菜单项。
    let mut close = false;
    if let Some(code) =
        render_change_menu_items(ui, theme, rect, rect.top() + 8.0, "clist_h_ctx", &items)
    {
        match code {
            100 => {
                rt.dialog = DialogState::NewChangelist {
                    value: String::new(),
                    error: None,
                };
                close = true;
            }
            101 => {
                // 删除前确认（稳定版语义）：移入默认列表 or 回滚丢弃。
                if let Some(project) = model.projects.get(project_idx) {
                    let lists = lazecode_git::Changelists::open(&project.root);
                    let count = lists.files_of(name).len();
                    rt.dialog = DialogState::DeleteChangelist {
                        name: name.to_string(),
                        count,
                        rollback: false,
                    };
                }
                close = true;
            }
            7 => {
                model.queue(Command::ReloadGit);
                close = true;
            }
            other => {
                if let Some(ctx) = &menu_ctx_holder {
                    close = apply_change_menu_action(ui, model, rt, ctx, other);
                }
            }
        }
    }

    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

/// 变更文件菜单条目：code 空间 0..10 通用动作，10+ Move to 列表，
/// 20/21 未跟踪（Add to VCS / Delete…），22..24 新增功能项（与本地
/// 前一版本比较 / 创建补丁 / 到此为止的历史），30..37 稳定版禁用占位
/// （只展示不可点），SEP 画分隔线，100/101 变更列表头菜单专用。
struct ChangeMenuItem {
    label: String,
    code: u8,
    danger: bool,
    enabled: bool,
}

const CHANGE_MENU_SEP: u8 = u8::MAX;

fn change_menu_sep(items: &mut Vec<ChangeMenuItem>) {
    items.push(ChangeMenuItem {
        label: String::new(),
        code: CHANGE_MENU_SEP,
        danger: false,
        enabled: false,
    });
}

fn change_menu_item(label: String, code: u8, danger: bool, enabled: bool) -> ChangeMenuItem {
    ChangeMenuItem {
        label,
        code,
        danger,
        enabled,
    }
}

fn change_menu_disabled(label: &str, code: u8) -> ChangeMenuItem {
    ChangeMenuItem {
        label: label.to_string(),
        code,
        danger: false,
        enabled: false,
    }
}

/// 菜单动作分发所需的上下文（从项目快照一次性取好）。
struct ChangeMenuCtx {
    project_root: std::path::PathBuf,
    conflicts: Vec<lazecode_git::ConflictEntry>,
    paths: Vec<String>,
    single: bool,
    first: String,
    display_name: String,
    all_lists: Vec<String>,
}

/// 构建变更文件菜单条目（文件行右键与变更列表组头右键共用，稳定版
/// CommitDrawer 菜单一比一：词条连 zh-CN 都保持原文，不走近路 tr!）。
fn change_menu_items(
    model: &AppModel,
    project: &lazecode_core::ProjectState,
    paths: &[String],
) -> (Vec<ChangeMenuItem>, ChangeMenuCtx) {
    let single = paths.len() == 1;
    let first = paths.first().cloned().unwrap_or_default();
    let lists = lazecode_git::Changelists::open(&project.root);
    let all_lists: Vec<String> = lists.lists().to_vec();
    let ctx = ChangeMenuCtx {
        project_root: project.root.clone(),
        conflicts: project.conflicts.clone(),
        paths: paths.to_vec(),
        single,
        first: first.clone(),
        display_name: first.rsplit('/').next().unwrap_or(&first).to_string(),
        all_lists: all_lists.clone(),
    };
    let conflicted = project
        .conflicts
        .iter()
        .any(|entry| paths.iter().any(|path| path == &entry.path));
    // 稳定版 allUntracked 规则：所选文件全部是未跟踪才走未跟踪菜单。
    let all_untracked = paths.iter().all(|path| {
        project
            .git
            .as_ref()
            .map(|git| {
                git.changes
                    .iter()
                    .any(|change| change.path == *path && change.untracked)
            })
            .unwrap_or(false)
    });

    let mut items: Vec<ChangeMenuItem> = Vec::new();
    if all_untracked {
        items.push(change_menu_item(
            if single {
                "Add to VCS".to_string()
            } else {
                format!("Add {} Files to VCS", paths.len())
            },
            20,
            false,
            true,
        ));
        if single {
            items.push(change_menu_item(
                "Jump to Source".to_string(),
                1,
                false,
                true,
            ));
        }
        change_menu_sep(&mut items);
        items.push(change_menu_item(
            if single {
                "Delete…".to_string()
            } else {
                format!("Delete {} Files…", paths.len())
            },
            21,
            true,
            true,
        ));
        return (items, ctx);
    }

    items.push(change_menu_item(
        if single {
            "Commit File...".to_string()
        } else {
            "提交所选文件…".to_string()
        },
        3,
        false,
        true,
    ));
    if single {
        items.push(change_menu_item("Show Diff".to_string(), 0, false, true));
        items.push(change_menu_disabled("Show Diff in a New Tab", 30));
        items.push(change_menu_disabled("Compare with Local", 31));
        items.push(change_menu_item(
            if model.settings.language.is_zh() {
                "与本地的前一版本比较".to_string()
            } else {
                "Compare Before with Local".to_string()
            },
            22,
            false,
            true,
        ));
        items.push(change_menu_item(
            "Jump to Source".to_string(),
            1,
            false,
            true,
        ));
        items.push(change_menu_disabled("Open Repository Version", 32));
        if conflicted {
            items.push(change_menu_item(
                crate::tr!(model, "Resolve…", "解决…").to_string(),
                2,
                false,
                true,
            ));
        }
    }
    change_menu_sep(&mut items);
    items.push(change_menu_item(
        if model.settings.language.is_zh() {
            "还原选中更改".to_string()
        } else {
            "Revert Selected Changes".to_string()
        },
        4,
        true,
        true,
    ));
    items.push(change_menu_disabled(
        if model.settings.language.is_zh() {
            "拣选选中更改"
        } else {
            "Cherry-Pick Selected Changes"
        },
        33,
    ));
    items.push(change_menu_disabled(
        if model.settings.language.is_zh() {
            "提取选中更改到独立提交…"
        } else {
            "Extract Selected Changes to Separate Commit…"
        },
        34,
    ));
    items.push(change_menu_disabled(
        if model.settings.language.is_zh() {
            "丢弃选中更改"
        } else {
            "Drop Selected Changes"
        },
        35,
    ));
    if single {
        items.push(change_menu_item(
            "Create Patch from Local Changes...".to_string(),
            23,
            false,
            true,
        ));
    }
    items.push(change_menu_disabled(
        if model.settings.language.is_zh() {
            "从版本获取"
        } else {
            "Get from Revision"
        },
        36,
    ));
    if single {
        items.push(change_menu_item(
            if model.settings.language.is_zh() {
                "到此为止的历史".to_string()
            } else {
                "History Up to Here".to_string()
            },
            24,
            false,
            true,
        ));
    }
    items.push(change_menu_disabled(
        if model.settings.language.is_zh() {
            "显示与父提交的差异"
        } else {
            "Show Changes to Parents"
        },
        37,
    ));
    change_menu_sep(&mut items);
    for (idx, list_name) in all_lists.iter().enumerate() {
        // 稳定版规则：所选文件已全部在该列表中 → 禁用（多选不再跳过）。
        let all_in_list = paths.iter().all(|path| lists.list_of(path) == *list_name);
        items.push(change_menu_item(
            format!("Move to {list_name}"),
            10 + idx as u8,
            false,
            !all_in_list,
        ));
    }
    if single {
        items.push(change_menu_item(
            "Copy as Patch to Clipboard".to_string(),
            5,
            false,
            true,
        ));
    }
    items.push(change_menu_item(
        "Shelve Changes...".to_string(),
        6,
        false,
        true,
    ));
    items.push(change_menu_item(
        "Refresh VCS Changes".to_string(),
        7,
        false,
        true,
    ));
    items.push(change_menu_item(
        "New Changelist…".to_string(),
        8,
        false,
        true,
    ));
    (items, ctx)
}

/// 变更菜单动作分发（文件行与组头菜单共用；返回是否应关闭菜单）。
fn apply_change_menu_action(
    ui: &Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    ctx: &ChangeMenuCtx,
    code: u8,
) -> bool {
    let project_idx = model.active_project.unwrap_or(0);
    match code {
        0 => model.queue(Command::OpenDiff(ctx.first.clone())),
        1 => {
            // Jump to Source：在编辑器打开（仓库相对 → 绝对路径）。
            let abs = ctx.project_root.join(&ctx.first);
            if let Some(project) = model.projects.get_mut(project_idx) {
                if let Err(err) = project.open_file(abs, &model.settings) {
                    model.toast(err, ToastKind::Error);
                }
            }
        }
        2 => {
            if ctx.conflicts.iter().any(|entry| entry.path == ctx.first) {
                if let Ok(content) = lazecode_git::merge_file_content(&ctx.project_root, &ctx.first)
                {
                    rt.dialog = DialogState::ConflictResolve {
                        path: ctx.first.clone(),
                        ours: content.ours,
                        theirs: content.theirs,
                        worktree: content.worktree,
                    };
                }
            }
        }
        3 => {
            rt.dialog = DialogState::CommitFiles {
                paths: ctx.paths.clone(),
                value: String::new(),
                error: None,
            };
        }
        4 => {
            // 统一确认弹窗（对齐稳定版 rollbackFileConfirm）。
            rt.dialog = DialogState::ConfirmDiscard {
                paths: ctx.paths.clone(),
            };
        }
        5 => {
            // Copy as Patch：HEAD→工作区补丁（含未跟踪文件）。
            match lazecode_git::diff_patch_paths(&ctx.project_root, &ctx.paths) {
                Ok(patch) => {
                    ui.ctx().copy_text(patch);
                    model.toast(
                        crate::tr!(model, "Patch copied to clipboard", "已将补丁复制到剪贴板")
                            .to_string(),
                        ToastKind::Info,
                    );
                }
                Err(err) => model.toast(err.to_string(), ToastKind::Error),
            }
        }
        6 => {
            // Shelve Changes...：架藏所选文件，默认名对齐稳定版。
            let default_name = if ctx.single {
                ctx.display_name.clone()
            } else {
                "Default".to_string()
            };
            rt.dialog = DialogState::NewShelf {
                value: default_name,
                error: None,
                paths: ctx.paths.clone(),
            };
        }
        7 => model.queue(Command::ReloadGit),
        8 => {
            rt.dialog = DialogState::NewChangelist {
                value: String::new(),
                error: None,
            };
        }
        20 => {
            // Add to VCS：git add 走后台任务，回填后刷新快照。
            model.queue(Command::AddToVcs(ctx.paths.clone()));
        }
        21 => {
            // 未跟踪文件 Delete…：全部移入废纸篓（稳定版 fs_delete 语义）。
            if ctx.single {
                rt.dialog = DialogState::DeletePath {
                    path: ctx.project_root.join(&ctx.first),
                    is_dir: false,
                };
            } else {
                rt.dialog = DialogState::ConfirmDeleteUntracked {
                    paths: ctx.paths.iter().map(|p| ctx.project_root.join(p)).collect(),
                };
            }
        }
        22 => {
            // Compare Before with Local：HEAD ↔ 工作区。
            model.queue(Command::OpenCompareWithLocalDiff {
                commit: "HEAD".to_string(),
                path: ctx.first.clone(),
            });
        }
        23 => {
            // Create Patch from Local Changes：先弹保存对话框再后台写文件。
            model.queue(Command::SavePatch(ctx.paths.clone()));
        }
        24 => {
            rt.dialog = DialogState::FileHistory {
                path: ctx.first.clone(),
                entries: Vec::new(),
                selected: 0,
                loaded: false,
            };
        }
        idx if (10..30).contains(&idx) => {
            let target_idx = (idx - 10) as usize;
            if let Some(target_list) = ctx.all_lists.get(target_idx) {
                model.queue(Command::ChangelistAssign {
                    paths: ctx.paths.clone(),
                    list: target_list.clone(),
                });
            }
        }
        _ => {}
    }
    true
}

fn render_change_menu_items(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    row_start: f32,
    id_salt: &str,
    items: &[ChangeMenuItem],
) -> Option<u8> {
    let p = theme.palette;
    let mut action: Option<u8> = None;
    for (i, item) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, row_start + i as f32 * 28.0),
            Vec2::new(rect.width() - 16.0, 26.0),
        );
        if item.code == CHANGE_MENU_SEP {
            ui.painter().line_segment(
                [
                    Pos2::new(row.left() + 8.0, row.center().y),
                    Pos2::new(row.right() - 8.0, row.center().y),
                ],
                egui::Stroke::new(1.0, p.border),
            );
            continue;
        }
        let response = ui.interact(row, ui.scope_id().with((id_salt, i)), Sense::click());
        if response.hovered() && item.enabled {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
        }
        ui.painter().text(
            Pos2::new(row.left() + 10.0, row.center().y),
            Align2::LEFT_CENTER,
            &item.label,
            FontId::proportional(12.5),
            if !item.enabled {
                p.text3
            } else if item.danger {
                p.rose
            } else {
                p.text
            },
        );
        if response.clicked() && item.enabled {
            action = Some(item.code);
        }
    }
    action
}

#[allow(clippy::too_many_arguments)]
fn draw_change_file_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    payload: &str,
) {
    let Some(project) = model.projects.get(project_idx) else {
        return;
    };
    // payload: 多选时以 \u{1} 连接的仓库相对路径。
    let paths: Vec<String> = payload
        .split('\u{1}')
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect();
    if paths.is_empty() {
        return;
    }
    let (items, ctx) = change_menu_items(model, project, &paths);

    let height = items.len() as f32 * 28.0 + 16.0;
    let rect = popover_rect(screen, menu.anchor, 280.0, height);
    frame(ui, rect, theme);

    // 稳定版右键菜单没有标题行，直接列菜单项。
    let mut close = false;
    if let Some(code) =
        render_change_menu_items(ui, theme, rect, rect.top() + 8.0, "cfile_ctx", &items)
    {
        close = apply_change_menu_action(ui, model, rt, &ctx, code);
    }

    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
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
        FontId::proportional(12.5),
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
            .font(FontId::proportional(12.0)),
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
    let other_worktree_branches: std::collections::HashMap<String, String> = {
        let active_wt_idx = model.projects[index].active_worktree;
        let mut map = std::collections::HashMap::new();
        for (i, wt) in model.projects[index].worktrees.iter().enumerate() {
            if i != active_wt_idx {
                let b = wt.branch();
                if !b.is_empty() && b != "no branch" && !b.contains("(detached)") {
                    map.insert(b, wt.name.clone());
                }
            }
        }
        if let Ok(wts) = lazecode_git::list_worktrees(&model.projects[index].root) {
            let active_wt_path = &model.projects[index].active_worktree().path;
            for wt in wts {
                if &wt.path != active_wt_path {
                    if let Some(b) = wt.branch {
                        map.insert(b, wt.name);
                    }
                }
            }
        }
        map
    };
    let rows_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 6.0, input.bottom() + 4.0),
        Pos2::new(rect.right() - 6.0, rect.bottom() - 40.0),
    );
    let mut checkout: Option<String> = None;
    let mut switch_worktree: Option<String> = None;
    let mut delete: Option<String> = None;
    super::app_ui::region(ui, rows_rect, "branch-menu", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("branch-menu-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for branch in &branches {
                    let (row, response) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click());
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
                    let other_wt = if !branch.remote {
                        other_worktree_branches.get(&branch.name)
                    } else {
                        None
                    };
                    let right_cut = if other_wt.is_some() {
                        90.0
                    } else if branch.remote {
                        56.0
                    } else {
                        32.0
                    };
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(row.left() + 20.0, row.top()),
                            Pos2::new(row.right() - right_cut, row.bottom()),
                        ),
                        &branch.name,
                        FontId::proportional(12.5),
                        if branch.remote {
                            p.text2
                        } else if other_wt.is_some() {
                            p.text3
                        } else {
                            p.text
                        },
                        Align2::LEFT_CENTER,
                    );
                    if branch.remote {
                        ui.painter().text(
                            Pos2::new(row.right() - 8.0, row.center().y),
                            Align2::RIGHT_CENTER,
                            crate::tr!(model, "remote", "远程"),
                            FontId::monospace(10.5),
                            p.text3,
                        );
                    } else if let Some(wt_name) = other_wt {
                        ui.painter().text(
                            Pos2::new(row.right() - 8.0, row.center().y),
                            Align2::RIGHT_CENTER,
                            format!("⎇ {wt_name}"),
                            FontId::proportional(11.0),
                            p.accent,
                        );
                        response
                            .clone()
                            .on_hover_text(if model.settings.language.is_zh() {
                                format!("已被工作树 '{wt_name}' 检出，点击直接切换至该工作树")
                            } else {
                                format!(
                                    "Checked out in worktree '{wt_name}'. Click to switch to it."
                                )
                            });
                    }
                    if !branch.remote && !is_current && other_wt.is_none() {
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
                        if let Some(wt_name) = other_wt {
                            switch_worktree = Some(wt_name.clone());
                        } else {
                            checkout = Some(branch.name.clone());
                        }
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
    if let Some(wt_name) = switch_worktree {
        model.queue(Command::SwitchWorktree(wt_name.clone()));
        model.toast(
            if model.settings.language.is_zh() {
                format!("已切换至工作树: {wt_name}")
            } else {
                format!("Switched to worktree '{wt_name}'")
            },
            ToastKind::Info,
        );
        close = true;
    }
    if let Some(name) = delete {
        rt.dialog = DialogState::DeleteBranch { name, error: None };
        close = true;
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
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
    if should_dismiss(ui, rect, rt.agent_menu_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.agent_menu = Some(anchor);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_conflict_tool(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    path: &mut String,
    ours: &mut String,
    theirs: &mut String,
    worktree: &mut String,
    close: &mut bool,
) {
    let p = theme.palette;
    let zh = model.settings.language.is_zh();
    let font = FontId::monospace(10.5);

    // Top action bar
    let header_rect = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 44.0));
    ui.painter()
        .rect_filled(header_rect, egui::CornerRadius::same(6), p.tabstrip);
    ui.painter().hline(
        header_rect.x_range(),
        header_rect.bottom() - 0.5,
        Stroke::new(1.0, p.border),
    );

    ui.painter().text(
        Pos2::new(header_rect.left() + 16.0, header_rect.center().y),
        Align2::LEFT_CENTER,
        format!(
            "⚡ {} — {}",
            if zh {
                "三方冲突解决"
            } else {
                "3-Way Merge Tool"
            },
            path
        ),
        FontId::proportional(12.0),
        p.text,
    );

    let mut btn_x = header_rect.right() - 16.0;

    // Close / Cancel
    btn_x -= 64.0;
    let cancel_rect = Rect::from_min_size(
        Pos2::new(btn_x, header_rect.center().y - 11.0),
        Vec2::new(60.0, 22.0),
    );
    if secondary_button(
        ui,
        cancel_rect,
        if zh { "取消" } else { "Cancel" },
        None,
        theme,
    )
    .clicked()
    {
        *close = true;
    }
    btn_x -= 8.0;

    // Mark as resolved button
    let count_conflicts = worktree.matches("<<<<<<<").count();
    btn_x -= 144.0;
    let save_rect = Rect::from_min_size(
        Pos2::new(btn_x, header_rect.center().y - 11.0),
        Vec2::new(140.0, 22.0),
    );
    let save_label = if count_conflicts == 0 {
        if zh {
            "✓ 保存并标记解决".to_string()
        } else {
            "✓ Save & Mark Resolved".to_string()
        }
    } else if zh {
        format!("标记解决 (剩余 {count_conflicts})")
    } else {
        format!("Resolve ({count_conflicts} left)")
    };
    if primary_button(ui, save_rect, &save_label, None, true, theme).clicked() {
        if let Some(active) = model.active_project {
            let root = model.projects[active].root.clone();
            let _ = lazecode_git::mark_conflict_resolved(&root, path, Some(worktree));
            model.queue(Command::RefreshLog);
            model.toast(
                if zh {
                    format!("已解决 {path}")
                } else {
                    format!("Resolved {path}")
                },
                ToastKind::Info,
            );
        }
        *close = true;
    }
    btn_x -= 8.0;

    btn_x -= 114.0;
    let theirs_rect = Rect::from_min_size(
        Pos2::new(btn_x, header_rect.center().y - 11.0),
        Vec2::new(110.0, 22.0),
    );
    if secondary_button(
        ui,
        theirs_rect,
        if zh {
            "全部采用对方 ▶"
        } else {
            "Accept Theirs ▶"
        },
        None,
        theme,
    )
    .clicked()
    {
        *worktree = theirs.clone();
    }
    btn_x -= 8.0;

    btn_x -= 114.0;
    let ours_rect = Rect::from_min_size(
        Pos2::new(btn_x, header_rect.center().y - 11.0),
        Vec2::new(110.0, 22.0),
    );
    if secondary_button(
        ui,
        ours_rect,
        if zh {
            "◀ 全部采用我方"
        } else {
            "◀ Accept Ours"
        },
        None,
        theme,
    )
    .clicked()
    {
        *worktree = ours.clone();
    }

    // Body: 3 panels
    let body_rect = Rect::from_min_max(Pos2::new(rect.left(), header_rect.bottom()), rect.max);
    let col_w = ((body_rect.width() - 4.0) / 3.0).floor();

    let left_rect = Rect::from_min_size(body_rect.min, Vec2::new(col_w, body_rect.height()));
    let mid_rect = Rect::from_min_size(
        Pos2::new(body_rect.min.x + col_w + 2.0, body_rect.min.y),
        Vec2::new(col_w, body_rect.height()),
    );
    let right_rect = Rect::from_min_size(
        Pos2::new(body_rect.min.x + (col_w * 2.0) + 4.0, body_rect.min.y),
        Vec2::new(col_w, body_rect.height()),
    );

    // Vertical dividers
    ui.painter().vline(
        left_rect.right() + 1.0,
        body_rect.y_range(),
        Stroke::new(1.0, p.border),
    );
    ui.painter().vline(
        mid_rect.right() + 1.0,
        body_rect.y_range(),
        Stroke::new(1.0, p.border),
    );

    // Left: Ours
    let l_head = Rect::from_min_size(left_rect.min, Vec2::new(left_rect.width(), 26.0));
    ui.painter().rect_filled(l_head, 0.0, p.panel2);
    ui.painter().text(
        Pos2::new(l_head.left() + 12.0, l_head.center().y),
        Align2::LEFT_CENTER,
        if zh {
            "我方 (Ours / 当前)"
        } else {
            "Ours (Current)"
        },
        FontId::proportional(10.5),
        p.accent,
    );
    let l_body = Rect::from_min_max(Pos2::new(left_rect.left(), l_head.bottom()), left_rect.max);
    super::app_ui::region(ui, l_body, "ours-scroll", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("ours-scroll-area")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(&*ours)
                        .font(font.clone())
                        .color(p.text2),
                );
            });
    });

    // Center: Result
    let m_head = Rect::from_min_size(mid_rect.min, Vec2::new(mid_rect.width(), 26.0));
    ui.painter().rect_filled(m_head, 0.0, p.panel2);
    if count_conflicts == 0 {
        ui.painter().text(
            Pos2::new(m_head.left() + 12.0, m_head.center().y),
            Align2::LEFT_CENTER,
            if zh {
                "✓ 合并结果 (冲突已全解)"
            } else {
                "✓ Result (No conflicts remaining)"
            },
            FontId::proportional(10.5),
            p.green,
        );
    } else {
        ui.painter().text(
            Pos2::new(m_head.left() + 12.0, m_head.center().y),
            Align2::LEFT_CENTER,
            if zh {
                format!("合并结果 (剩余 {count_conflicts} 处冲突)")
            } else {
                format!("Result ({count_conflicts} remaining)")
            },
            FontId::proportional(10.5),
            p.amber,
        );
    }
    let m_body = Rect::from_min_max(Pos2::new(mid_rect.left(), m_head.bottom()), mid_rect.max);
    super::app_ui::region(ui, m_body, "result-scroll", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("result-scroll-area")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add(
                    TextEdit::multiline(worktree)
                        .font(font.clone())
                        .frame(egui::Frame::NONE)
                        .desired_width(mid_rect.width() - 16.0),
                );
            });
    });

    // Right: Theirs
    let r_head = Rect::from_min_size(right_rect.min, Vec2::new(right_rect.width(), 26.0));
    ui.painter().rect_filled(r_head, 0.0, p.panel2);
    ui.painter().text(
        Pos2::new(r_head.left() + 12.0, r_head.center().y),
        Align2::LEFT_CENTER,
        if zh {
            "对方 (Theirs / 传入)"
        } else {
            "Theirs (Incoming)"
        },
        FontId::proportional(10.5),
        p.rose,
    );
    let r_body = Rect::from_min_max(
        Pos2::new(right_rect.left(), r_head.bottom()),
        right_rect.max,
    );
    super::app_ui::region(ui, r_body, "theirs-scroll", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("theirs-scroll-area")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(&*theirs)
                        .font(font.clone())
                        .color(p.text2),
                );
            });
    });
}
#[allow(clippy::too_many_arguments)]
/// User 过滤菜单：本侧出现过的作者（首见序）。
fn draw_compare_user_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    side: u8,
) {
    let p = theme.palette;
    let commits = match model.ui.compare_branches.sides.as_ref() {
        Some(sides) if side == 0 => sides.left.commits.clone(),
        Some(sides) => sides.right.commits.clone(),
        None => Vec::new(),
    };
    let mut authors = crate::compare::unique_authors(&commits);
    authors.insert(0, crate::tr!(model, "All authors", "全部作者").to_string());
    let height = (authors.len() as f32 * 24.0 + 20.0).min(320.0);
    let rect = popover_rect(screen, menu.anchor, 200.0, height);
    frame(ui, rect, theme);
    let mut picked: Option<Option<String>> = None;
    for (i, author) in authors.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, rect.top() + 10.0 + i as f32 * 24.0),
            Vec2::new(rect.width() - 16.0, 22.0),
        );
        let response = ui.interact(row, ui.scope_id().with(("cbuser", i)), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
        }
        text_clipped(
            ui.painter(),
            row,
            author,
            FontId::proportional(11.0),
            p.text,
            Align2::LEFT_CENTER,
        );
        if response.clicked() {
            picked = Some(if i == 0 { None } else { Some(author.clone()) });
        }
    }
    if let Some(pick) = picked {
        let view = if side == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.author = pick;
        return;
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        return;
    }
    rt.menu = Some(menu);
}

/// Date 过滤菜单：All / Today / Last 7 days / Last 30 days。
fn draw_compare_date_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    side: u8,
) {
    let p = theme.palette;
    let items: Vec<(String, Option<lazecode_core::CompareDateFilter>)> = vec![
        (crate::tr!(model, "All", "全部").to_string(), None),
        (
            crate::tr!(model, "Today", "今天").to_string(),
            Some(lazecode_core::CompareDateFilter::Today),
        ),
        (
            crate::tr!(model, "Last 7 days", "最近 7 天").to_string(),
            Some(lazecode_core::CompareDateFilter::Last7Days),
        ),
        (
            crate::tr!(model, "Last 30 days", "最近 30 天").to_string(),
            Some(lazecode_core::CompareDateFilter::Last30Days),
        ),
    ];
    let height = items.len() as f32 * 24.0 + 20.0;
    let rect = popover_rect(screen, menu.anchor, 160.0, height);
    frame(ui, rect, theme);
    let mut picked: Option<Option<lazecode_core::CompareDateFilter>> = None;
    for (i, (label, value)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, rect.top() + 10.0 + i as f32 * 24.0),
            Vec2::new(rect.width() - 16.0, 22.0),
        );
        let response = ui.interact(row, ui.scope_id().with(("cbdate", i)), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
        }
        ui.painter().text(
            Pos2::new(row.left() + 10.0, row.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(11.0),
            p.text,
        );
        if response.clicked() {
            picked = Some(*value);
        }
    }
    if let Some(pick) = picked {
        let view = if side == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.date = pick;
        return;
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        return;
    }
    rt.menu = Some(menu);
}

/// Branch(ref) 过滤菜单：本侧提交所属的引用名（首见序）。
fn draw_compare_ref_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    side: u8,
) {
    let p = theme.palette;
    let commits = match model.ui.compare_branches.sides.as_ref() {
        Some(sides) if side == 0 => sides.left.commits.clone(),
        Some(sides) => sides.right.commits.clone(),
        None => Vec::new(),
    };
    let mut refs = crate::compare::unique_refs(&commits);
    refs.insert(0, crate::tr!(model, "All Branches", "全部分支").to_string());
    let height = (refs.len() as f32 * 24.0 + 20.0).min(320.0);
    let rect = popover_rect(screen, menu.anchor, 220.0, height);
    frame(ui, rect, theme);
    let mut picked: Option<Option<String>> = None;
    for (i, name) in refs.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, rect.top() + 10.0 + i as f32 * 24.0),
            Vec2::new(rect.width() - 16.0, 22.0),
        );
        let response = ui.interact(row, ui.scope_id().with(("cbref", i)), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
        }
        text_clipped(
            ui.painter(),
            row,
            name,
            FontId::monospace(10.0),
            p.text,
            Align2::LEFT_CENTER,
        );
        if response.clicked() {
            picked = Some(if i == 0 { None } else { Some(name.clone()) });
        }
    }
    if let Some(pick) = picked {
        let view = if side == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.selected_ref_filter = pick;
        return;
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        return;
    }
    rt.menu = Some(menu);
}

#[allow(clippy::too_many_arguments)]
fn draw_stash_manager_dialog(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    screen: Rect,
    stashes: &mut Vec<lazecode_git::StashEntry>,
    selected: &mut usize,
    error: &mut Option<String>,
    loaded: &mut bool,
    close: &mut bool,
) {
    let p = theme.palette;
    let zh = model.settings.language.is_zh();
    ui.painter()
        .rect_filled(screen, 0.0, Color32::from_black_alpha(150));
    let width = 640.0;
    let height = 440.0;
    let rect = Rect::from_center_size(screen.center(), Vec2::new(width, height));
    frame(ui, rect, theme);

    // 只在首帧加载一次：空列表≠未加载，否则对话框打开期间每帧都会
    // spawn 一次 git 子进程。
    if !*loaded && error.is_none() {
        *loaded = true;
        if let Some(active) = model.active_project {
            let root = model.projects[active].root.clone();
            match lazecode_git::stash_list(&root) {
                Ok(list) => *stashes = list,
                Err(err) => *error = Some(err.to_string()),
            }
        }
    }

    let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 44.0));
    ui.painter()
        .rect_filled(header, egui::CornerRadius::same(6), p.tabstrip);
    ui.painter().hline(
        header.x_range(),
        header.bottom() - 0.5,
        Stroke::new(1.0, p.border),
    );
    ui.painter().text(
        Pos2::new(header.left() + 16.0, header.center().y),
        Align2::LEFT_CENTER,
        if zh {
            "贮藏管理 (Stashes)"
        } else {
            "Stash Manager"
        },
        FontId::proportional(13.0),
        p.text,
    );

    let mut btn_x = header.right() - 16.0;
    btn_x -= 60.0;
    if secondary_button(
        ui,
        Rect::from_min_size(
            Pos2::new(btn_x, header.center().y - 11.0),
            Vec2::new(56.0, 22.0),
        ),
        if zh { "关闭" } else { "Close" },
        None,
        theme,
    )
    .clicked()
    {
        *close = true;
    }

    let list_area = Rect::from_min_max(
        Pos2::new(rect.left() + 12.0, header.bottom() + 10.0),
        Pos2::new(rect.right() - 12.0, rect.bottom() - 48.0),
    );
    let mut selected_entry = None;
    super::app_ui::region(ui, list_area, "stash-mgr-list", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("stash-scroll")
            .show(ui, |ui| {
                if stashes.is_empty() {
                    ui.label(
                        egui::RichText::new(if let Some(err) = error {
                            err.clone()
                        } else if zh {
                            "当前没有贮藏记录".to_string()
                        } else {
                            "No stashes found".to_string()
                        })
                        .color(if error.is_some() {
                            p.rose
                        } else {
                            p.text3
                        }),
                    );
                } else {
                    for (i, entry) in stashes.iter().enumerate() {
                        let is_sel = *selected == i;
                        let (row, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 26.0),
                            Sense::click(),
                        );
                        if is_sel {
                            ui.painter().rect_filled(
                                row,
                                egui::CornerRadius::same(3),
                                p.accent_soft,
                            );
                        }
                        ui.painter().text(
                            Pos2::new(row.left() + 8.0, row.center().y),
                            Align2::LEFT_CENTER,
                            format!("{}: {}", entry.name, entry.subject),
                            FontId::monospace(10.0),
                            if is_sel { p.accent } else { p.text },
                        );
                        if ui
                            .interact(row, ui.scope_id().with(("stash-row", i)), Sense::click())
                            .clicked()
                        {
                            *selected = i;
                        }
                    }
                }
            });
    });

    if let Some(entry) = stashes.get(*selected) {
        selected_entry = Some(entry.name.clone());
    }

    let bottom_y = rect.bottom() - 36.0;
    if let Some(ref stash_name) = selected_entry {
        let apply_rect = Rect::from_min_size(
            Pos2::new(rect.left() + 16.0, bottom_y),
            Vec2::new(80.0, 24.0),
        );
        if secondary_button(
            ui,
            apply_rect,
            if zh { "应用" } else { "Apply" },
            None,
            theme,
        )
        .clicked()
        {
            model.queue(Command::StashApply(stash_name.clone()));
            *close = true;
        }

        let drop_rect = Rect::from_min_size(
            Pos2::new(apply_rect.right() + 8.0, bottom_y),
            Vec2::new(80.0, 24.0),
        );
        if secondary_button(ui, drop_rect, if zh { "删除" } else { "Drop" }, None, theme).clicked()
        {
            model.queue(Command::StashDrop(stash_name.clone()));
            *close = true;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_shelf_manager_dialog(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    shelves: &mut Vec<lazecode_git::ShelfEntry>,
    selected: &mut usize,
    error: &mut Option<String>,
    loaded: &mut bool,
    close: &mut bool,
) {
    let p = theme.palette;
    let zh = model.settings.language.is_zh();
    ui.painter()
        .rect_filled(screen, 0.0, Color32::from_black_alpha(150));
    let width = 640.0;
    let height = 440.0;
    let rect = Rect::from_center_size(screen.center(), Vec2::new(width, height));
    frame(ui, rect, theme);

    // 只在首帧加载一次（同 StashManager，避免每帧 spawn git）。
    if !*loaded && error.is_none() {
        *loaded = true;
        if let Some(active) = model.active_project {
            let root = model.projects[active].root.clone();
            match lazecode_git::shelf_list(&root) {
                Ok(list) => *shelves = list,
                Err(err) => *error = Some(err.to_string()),
            }
        }
    }

    let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 44.0));
    ui.painter()
        .rect_filled(header, egui::CornerRadius::same(6), p.tabstrip);
    ui.painter().hline(
        header.x_range(),
        header.bottom() - 0.5,
        Stroke::new(1.0, p.border),
    );
    ui.painter().text(
        Pos2::new(header.left() + 16.0, header.center().y),
        Align2::LEFT_CENTER,
        if zh {
            "架藏管理 (Shelves)"
        } else {
            "Shelves Manager"
        },
        FontId::proportional(13.0),
        p.text,
    );

    let mut btn_x = header.right() - 16.0;
    btn_x -= 60.0;
    if secondary_button(
        ui,
        Rect::from_min_size(
            Pos2::new(btn_x, header.center().y - 11.0),
            Vec2::new(56.0, 22.0),
        ),
        if zh { "关闭" } else { "Close" },
        None,
        theme,
    )
    .clicked()
    {
        *close = true;
    }

    let list_area = Rect::from_min_max(
        Pos2::new(rect.left() + 12.0, header.bottom() + 10.0),
        Pos2::new(rect.right() - 12.0, rect.bottom() - 48.0),
    );
    let mut selected_entry = None;
    super::app_ui::region(ui, list_area, "shelf-mgr-list", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("shelf-scroll")
            .show(ui, |ui| {
                if shelves.is_empty() {
                    ui.label(
                        egui::RichText::new(if zh {
                            "当前没有架藏记录"
                        } else {
                            "No shelves found"
                        })
                        .color(p.text3),
                    );
                } else {
                    for (i, entry) in shelves.iter().enumerate() {
                        let is_sel = *selected == i;
                        let (row, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 26.0),
                            Sense::click(),
                        );
                        if is_sel {
                            ui.painter().rect_filled(
                                row,
                                egui::CornerRadius::same(3),
                                p.accent_soft,
                            );
                        }
                        ui.painter().text(
                            Pos2::new(row.left() + 8.0, row.center().y),
                            Align2::LEFT_CENTER,
                            &entry.name,
                            FontId::monospace(10.0),
                            if is_sel { p.accent } else { p.text },
                        );
                        if ui
                            .interact(row, ui.scope_id().with(("shelf-row", i)), Sense::click())
                            .clicked()
                        {
                            *selected = i;
                        }
                    }
                }
            });
    });

    if let Some(entry) = shelves.get(*selected) {
        selected_entry = Some(entry.name.clone());
    }

    let bottom_y = rect.bottom() - 36.0;
    if let Some(ref shelf_name) = selected_entry {
        let r1 = Rect::from_min_size(
            Pos2::new(rect.left() + 16.0, bottom_y),
            Vec2::new(120.0, 24.0),
        );
        if secondary_button(
            ui,
            r1,
            if zh {
                "恢复并保留"
            } else {
                "Restore (Keep)"
            },
            None,
            theme,
        )
        .clicked()
        {
            model.queue(Command::ShelfRestore {
                name: shelf_name.clone(),
                paths: None,
                target_list: None,
                delete_after: false,
            });
            *close = true;
        }

        let r2 = Rect::from_min_size(
            Pos2::new(r1.right() + 8.0, bottom_y),
            Vec2::new(120.0, 24.0),
        );
        if secondary_button(
            ui,
            r2,
            if zh {
                "恢复并删除"
            } else {
                "Restore (Delete)"
            },
            None,
            theme,
        )
        .clicked()
        {
            model.queue(Command::ShelfRestore {
                name: shelf_name.clone(),
                paths: None,
                target_list: None,
                delete_after: true,
            });
            *close = true;
        }

        // Unshelve…（对齐稳定版）：逐文件勾选 + 目标 changelist。
        let r_un =
            Rect::from_min_size(Pos2::new(r2.right() + 8.0, bottom_y), Vec2::new(90.0, 24.0));
        if secondary_button(ui, r_un, "Unshelve…", None, theme).clicked() {
            if let Some(active) = model.active_project {
                let root = model.projects[active].root.clone();
                match lazecode_git::shelf_files(&root, shelf_name) {
                    Ok(files) => {
                        rt.dialog = DialogState::Unshelve {
                            name: shelf_name.clone(),
                            files: files.into_iter().map(|path| (path, true)).collect(),
                            // 稳定版默认目标 = 用架藏名新建列表（new 单选 +
                            // 输入框预填架藏名）。
                            target: None,
                            new_name: shelf_name.clone(),
                            error: None,
                        };
                        *close = true;
                    }
                    Err(err) => *error = Some(err.to_string()),
                }
            }
        }

        let r3 = Rect::from_min_size(
            Pos2::new(r_un.right() + 8.0, bottom_y),
            Vec2::new(80.0, 24.0),
        );
        if secondary_button(ui, r3, if zh { "删除" } else { "Delete" }, None, theme).clicked() {
            // 删除前确认（稳定版语义）；确认后回到重新懒加载的管理器。
            rt.dialog = DialogState::ConfirmDeleteShelf {
                name: shelf_name.clone(),
            };
            *close = true;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_file_history_dialog(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    screen: Rect,
    path: &str,
    entries: &mut Vec<lazecode_git::HistoryEntry>,
    selected: &mut usize,
    loaded: &mut bool,
    close: &mut bool,
) {
    let p = theme.palette;
    let zh = model.settings.language.is_zh();
    ui.painter()
        .rect_filled(screen, 0.0, Color32::from_black_alpha(150));
    let width = 760.0;
    let height = 500.0;
    let rect = Rect::from_center_size(screen.center(), Vec2::new(width, height));
    frame(ui, rect, theme);

    // 只在首帧加载一次：空历史≠未加载，否则每帧跑一次 `git log`。
    if !*loaded {
        *loaded = true;
        if let Some(active) = model.active_project {
            let root = model.projects[active].root.clone();
            if let Ok(list) = lazecode_git::file_history(&root, path, 200) {
                *entries = list;
            }
        }
    }

    let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 44.0));
    ui.painter()
        .rect_filled(header, egui::CornerRadius::same(6), p.tabstrip);
    ui.painter().hline(
        header.x_range(),
        header.bottom() - 0.5,
        Stroke::new(1.0, p.border),
    );
    ui.painter().text(
        Pos2::new(header.left() + 16.0, header.center().y),
        Align2::LEFT_CENTER,
        format!(
            "{} — {}",
            if zh { "文件历史" } else { "File History" },
            path
        ),
        FontId::proportional(12.0),
        p.text,
    );

    let mut btn_x = header.right() - 16.0;
    btn_x -= 60.0;
    if secondary_button(
        ui,
        Rect::from_min_size(
            Pos2::new(btn_x, header.center().y - 11.0),
            Vec2::new(56.0, 22.0),
        ),
        if zh { "关闭" } else { "Close" },
        None,
        theme,
    )
    .clicked()
    {
        *close = true;
    }

    let list_area = Rect::from_min_max(
        Pos2::new(rect.left() + 12.0, header.bottom() + 10.0),
        Pos2::new(rect.right() - 12.0, rect.bottom() - 48.0),
    );
    super::app_ui::region(ui, list_area, "file-hist-list", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("hist-scroll")
            .show(ui, |ui| {
                if entries.is_empty() {
                    ui.label(
                        egui::RichText::new(if zh {
                            "无该文件的历史记录"
                        } else {
                            "No history for this file"
                        })
                        .color(p.text3),
                    );
                } else {
                    for (i, entry) in entries.iter().enumerate() {
                        let is_sel = *selected == i;
                        let (row, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 26.0),
                            Sense::click(),
                        );
                        if is_sel {
                            ui.painter().rect_filled(
                                row,
                                egui::CornerRadius::same(3),
                                p.accent_soft,
                            );
                        }
                        let hash_str = &entry.short;
                        ui.painter().text(
                            Pos2::new(row.left() + 8.0, row.center().y),
                            Align2::LEFT_CENTER,
                            hash_str,
                            FontId::monospace(9.5),
                            p.accent,
                        );
                        ui.painter().text(
                            Pos2::new(row.left() + 80.0, row.center().y),
                            Align2::LEFT_CENTER,
                            &entry.subject,
                            FontId::proportional(10.0),
                            if is_sel { p.accent } else { p.text },
                        );
                        ui.painter().text(
                            Pos2::new(row.right() - 12.0, row.center().y),
                            Align2::RIGHT_CENTER,
                            &entry.author,
                            FontId::proportional(9.0),
                            p.text3,
                        );
                        if ui
                            .interact(row, ui.scope_id().with(("hist-row", i)), Sense::click())
                            .clicked()
                        {
                            *selected = i;
                        }
                    }
                }
            });
    });

    let bottom_y = rect.bottom() - 36.0;
    if let Some(entry) = entries.get(*selected) {
        let b1 = Rect::from_min_size(
            Pos2::new(rect.left() + 16.0, bottom_y),
            Vec2::new(120.0, 24.0),
        );
        if secondary_button(
            ui,
            b1,
            if zh {
                "查看该版本差异"
            } else {
                "View Diff"
            },
            None,
            theme,
        )
        .clicked()
        {
            model.queue(Command::OpenCommitDiff {
                commit: entry.hash.clone(),
                path: path.to_string(),
            });
            *close = true;
        }
        let b2 = Rect::from_min_size(
            Pos2::new(b1.right() + 8.0, bottom_y),
            Vec2::new(130.0, 24.0),
        );
        if secondary_button(
            ui,
            b2,
            if zh {
                "与本地工作区比较"
            } else {
                "Compare with Local"
            },
            None,
            theme,
        )
        .clicked()
        {
            model.queue(Command::OpenCompareWithLocalDiff {
                commit: entry.hash.clone(),
                path: path.to_string(),
            });
            *close = true;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_git_tools_dialog(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    screen: Rect,
    tab: &mut crate::runtime::GitToolsTab,
    submodules: &mut Vec<lazecode_git::SubmoduleEntry>,
    lfs_status: &mut Option<lazecode_git::LfsStatus>,
    loaded: &mut (bool, bool),
    close: &mut bool,
) {
    let p = theme.palette;
    let zh = model.settings.language.is_zh();
    ui.painter()
        .rect_filled(screen, 0.0, Color32::from_black_alpha(150));
    let width = 680.0;
    let height = 460.0;
    let rect = Rect::from_center_size(screen.center(), Vec2::new(width, height));
    frame(ui, rect, theme);

    // 各页签只加载一次：空列表≠未加载，否则对话框打开期间每帧 spawn git。
    if !loaded.0 && *tab == crate::runtime::GitToolsTab::Submodules {
        loaded.0 = true;
        if let Some(active) = model.active_project {
            let root = model.projects[active].root.clone();
            if let Ok(subs) = lazecode_git::submodule_list(&root) {
                *submodules = subs;
            }
        }
    }
    if !loaded.1 && *tab == crate::runtime::GitToolsTab::Lfs {
        loaded.1 = true;
        if let Some(active) = model.active_project {
            let root = model.projects[active].root.clone();
            *lfs_status = Some(lazecode_git::lfs_status(&root));
        }
    }

    let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 44.0));
    ui.painter()
        .rect_filled(header, egui::CornerRadius::same(6), p.tabstrip);
    ui.painter().hline(
        header.x_range(),
        header.bottom() - 0.5,
        Stroke::new(1.0, p.border),
    );
    ui.painter().text(
        Pos2::new(header.left() + 16.0, header.center().y),
        Align2::LEFT_CENTER,
        if zh {
            "Git 工具集 (Git Tools)"
        } else {
            "Git Tools"
        },
        FontId::proportional(13.0),
        p.text,
    );

    let mut tx = header.left() + 220.0;
    for (t, label) in [
        (
            crate::runtime::GitToolsTab::Submodules,
            if zh {
                "子模块 (Submodules)"
            } else {
                "Submodules"
            },
        ),
        (
            crate::runtime::GitToolsTab::Lfs,
            if zh { "Git LFS" } else { "LFS" },
        ),
    ] {
        let is_cur = *tab == t;
        let t_rect = Rect::from_min_size(
            Pos2::new(tx, header.center().y - 11.0),
            Vec2::new(120.0, 22.0),
        );
        tx += 128.0;
        if secondary_button(ui, t_rect, label, None, theme).clicked() {
            *tab = t;
        }
        if is_cur {
            ui.painter()
                .rect_filled(t_rect, egui::CornerRadius::same(3), p.accent_soft);
        }
    }

    let close_btn = Rect::from_min_size(
        Pos2::new(header.right() - 76.0, header.center().y - 11.0),
        Vec2::new(60.0, 22.0),
    );
    if secondary_button(
        ui,
        close_btn,
        if zh { "关闭" } else { "Close" },
        None,
        theme,
    )
    .clicked()
    {
        *close = true;
    }

    let body = Rect::from_min_max(
        Pos2::new(rect.left() + 12.0, header.bottom() + 10.0),
        Pos2::new(rect.right() - 12.0, rect.bottom() - 12.0),
    );
    match tab {
        crate::runtime::GitToolsTab::Submodules => {
            super::app_ui::region(ui, body, "submodules-list", |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("subs-scroll")
                    .show(ui, |ui| {
                        if submodules.is_empty() {
                            ui.label(
                                egui::RichText::new(if zh {
                                    "未检测到 Git 子模块"
                                } else {
                                    "No submodules detected"
                                })
                                .color(p.text3),
                            );
                        } else {
                            for sub in submodules.iter() {
                                let (row, _) = ui.allocate_exact_size(
                                    Vec2::new(ui.available_width(), 26.0),
                                    Sense::hover(),
                                );
                                ui.painter().text(
                                    Pos2::new(row.left() + 8.0, row.center().y),
                                    Align2::LEFT_CENTER,
                                    format!("📦 {}", sub.path),
                                    FontId::monospace(10.0),
                                    p.text,
                                );
                                ui.painter().text(
                                    Pos2::new(row.right() - 8.0, row.center().y),
                                    Align2::RIGHT_CENTER,
                                    &sub.url,
                                    FontId::monospace(9.0),
                                    p.text3,
                                );
                            }
                        }
                    });
            });
        }
        crate::runtime::GitToolsTab::Lfs => {
            super::app_ui::region(ui, body, "lfs-view", |ui| {
                if let Some(status) = lfs_status {
                    ui.label(
                        egui::RichText::new(format!(
                            "{}: {}",
                            if zh {
                                "LFS 可用状态"
                            } else {
                                "LFS Available"
                            },
                            status.installed
                        ))
                        .color(if status.installed {
                            p.green
                        } else {
                            p.amber
                        }),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(if zh {
                            "已追踪的文件规则模式:"
                        } else {
                            "Tracked patterns:"
                        })
                        .color(p.text2),
                    );
                    for pattern in &status.patterns {
                        ui.label(
                            egui::RichText::new(format!("  • {pattern}"))
                                .font(FontId::monospace(10.0))
                                .color(p.text),
                        );
                    }
                    if status.patterns.is_empty() {
                        ui.label(
                            egui::RichText::new(if zh {
                                "  (暂无 .gitattributes LFS 规则)"
                            } else {
                                "  (No patterns in .gitattributes)"
                            })
                            .color(p.text3),
                        );
                    }
                }
            });
        }
        crate::runtime::GitToolsTab::Remotes => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_clone_repo_dialog(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    screen: Rect,
    url: &mut String,
    dest: &mut String,
    error: &mut Option<String>,
    close: &mut bool,
) {
    let p = theme.palette;
    let zh = model.settings.language.is_zh();
    ui.painter()
        .rect_filled(screen, 0.0, Color32::from_black_alpha(150));
    let width = 520.0;
    let height = 260.0;
    let rect = Rect::from_center_size(screen.center(), Vec2::new(width, height));
    frame(ui, rect, theme);

    ui.painter().text(
        Pos2::new(rect.left() + 22.0, rect.top() + 28.0),
        Align2::LEFT_CENTER,
        if zh {
            "克隆仓库 (Clone Repository)"
        } else {
            "Clone Repository"
        },
        FontId::proportional(15.0),
        p.text,
    );
    ui.painter().text(
        Pos2::new(rect.left() + 22.0, rect.top() + 48.0),
        Align2::LEFT_CENTER,
        if zh {
            "克隆远程 Git 仓库至本地目录。"
        } else {
            "Clone a remote Git repository to local disk."
        },
        FontId::proportional(10.5),
        p.text3,
    );

    let url_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 22.0, rect.top() + 78.0),
        Vec2::new(rect.width() - 44.0, 30.0),
    );
    ui.painter()
        .rect_filled(url_rect, egui::CornerRadius::same(5), p.panel2);
    ui.put(
        url_rect.shrink2(Vec2::new(8.0, 4.0)),
        TextEdit::singleline(url)
            .frame(egui::Frame::NONE)
            .hint_text("https://github.com/user/repo.git"),
    );

    let dest_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 22.0, rect.top() + 120.0),
        Vec2::new(rect.width() - 44.0, 30.0),
    );
    ui.painter()
        .rect_filled(dest_rect, egui::CornerRadius::same(5), p.panel2);
    ui.put(
        dest_rect.shrink2(Vec2::new(8.0, 4.0)),
        TextEdit::singleline(dest)
            .frame(egui::Frame::NONE)
            .hint_text(if zh {
                "本地目标目录路径"
            } else {
                "/path/to/destination"
            }),
    );

    if let Some(err) = error {
        ui.painter().text(
            Pos2::new(rect.left() + 22.0, rect.top() + 165.0),
            Align2::LEFT_CENTER,
            err,
            FontId::proportional(10.0),
            p.rose,
        );
    }

    let cancel = Rect::from_min_size(
        Pos2::new(rect.right() - 190.0, rect.bottom() - 46.0),
        Vec2::new(80.0, 26.0),
    );
    if secondary_button(ui, cancel, if zh { "取消" } else { "Cancel" }, None, theme).clicked() {
        *close = true;
    }

    let clone_btn = Rect::from_min_size(
        Pos2::new(rect.right() - 100.0, rect.bottom() - 46.0),
        Vec2::new(88.0, 26.0),
    );
    if primary_button(
        ui,
        clone_btn,
        if zh { "克隆" } else { "Clone" },
        None,
        true,
        theme,
    )
    .clicked()
    {
        let u = url.trim().to_string();
        let d = dest.trim().to_string();
        if u.is_empty() || d.is_empty() {
            *error = Some(if zh {
                "URL 与目标目录均不能为空".into()
            } else {
                "URL and target path cannot be empty".into()
            });
        } else {
            let dest_path = std::path::PathBuf::from(&d);
            match lazecode_git::clone(&u, &dest_path) {
                Ok(_) => {
                    model.toast(
                        if zh {
                            "克隆完成，正在打开项目…".to_string()
                        } else {
                            "Repository cloned, opening project…".to_string()
                        },
                        ToastKind::Info,
                    );
                    model.queue(Command::OpenProject(dest_path));
                    *close = true;
                }
                Err(e) => *error = Some(e.to_string()),
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_squash_dialog(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    screen: Rect,
    commits: &[String],
    message: &mut String,
    error: &mut Option<String>,
    close: &mut bool,
) {
    let p = theme.palette;
    let zh = model.settings.language.is_zh();
    ui.painter()
        .rect_filled(screen, 0.0, Color32::from_black_alpha(150));
    let width = 560.0;
    let height = 380.0;
    let rect = Rect::from_center_size(screen.center(), Vec2::new(width, height));
    frame(ui, rect, theme);

    ui.painter().text(
        Pos2::new(rect.left() + 22.0, rect.top() + 26.0),
        Align2::LEFT_CENTER,
        if zh {
            "压缩提交 (Squash Commits)"
        } else {
            "Squash Commits"
        },
        FontId::proportional(15.0),
        p.text,
    );
    ui.painter().text(
        Pos2::new(rect.left() + 22.0, rect.top() + 48.0),
        Align2::LEFT_CENTER,
        if zh {
            format!("将选中的 {} 个提交合并为一个提交。", commits.len())
        } else {
            format!(
                "Combine {} selected commits into a single commit.",
                commits.len()
            )
        },
        FontId::proportional(11.0),
        p.text3,
    );

    let hashes_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 22.0, rect.top() + 70.0),
        Vec2::new(rect.width() - 44.0, 36.0),
    );
    ui.painter()
        .rect_filled(hashes_rect, egui::CornerRadius::same(5), p.panel2);
    let short_hashes = commits
        .iter()
        .map(|h| &h[..7.min(h.len())])
        .collect::<Vec<_>>()
        .join(", ");
    text_clipped(
        ui.painter(),
        hashes_rect.shrink2(Vec2::new(8.0, 4.0)),
        &short_hashes,
        FontId::monospace(11.0),
        p.accent,
        Align2::LEFT_CENTER,
    );

    ui.painter().text(
        Pos2::new(rect.left() + 22.0, rect.top() + 120.0),
        Align2::LEFT_CENTER,
        if zh {
            "新提交信息："
        } else {
            "Commit Message:"
        },
        FontId::proportional(11.5),
        p.text2,
    );

    let msg_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 22.0, rect.top() + 138.0),
        Vec2::new(rect.width() - 44.0, 160.0),
    );
    ui.painter()
        .rect_filled(msg_rect, egui::CornerRadius::same(5), p.panel2);
    ui.put(
        msg_rect.shrink2(Vec2::new(8.0, 6.0)),
        TextEdit::multiline(message)
            .frame(egui::Frame::NONE)
            .font(FontId::proportional(12.5)),
    );

    if let Some(err) = error {
        ui.painter().text(
            Pos2::new(rect.left() + 22.0, rect.bottom() - 32.0),
            Align2::LEFT_CENTER,
            err,
            FontId::proportional(10.0),
            p.rose,
        );
    }

    let cancel = Rect::from_min_size(
        Pos2::new(rect.right() - 190.0, rect.bottom() - 46.0),
        Vec2::new(80.0, 26.0),
    );
    if secondary_button(ui, cancel, if zh { "取消" } else { "Cancel" }, None, theme).clicked() {
        *close = true;
    }

    let squash_btn = Rect::from_min_size(
        Pos2::new(rect.right() - 100.0, rect.bottom() - 46.0),
        Vec2::new(88.0, 26.0),
    );
    let trimmed = message.trim();
    let can_squash = !trimmed.is_empty() && commits.len() >= 2;
    if primary_button(
        ui,
        squash_btn,
        if zh { "压缩" } else { "Squash" },
        None,
        can_squash,
        theme,
    )
    .clicked()
        && can_squash
    {
        model.queue(Command::SquashCommits {
            hashes: commits.to_vec(),
            message: trimmed.to_string(),
        });
        *close = true;
    }
}

fn draw_interactive_rebase_dialog(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    screen: Rect,
    onto: &str,
    todos: &mut [crate::runtime::RebaseTodoRow],
    close: &mut bool,
) {
    let p = theme.palette;
    let zh = model.settings.language.is_zh();
    ui.painter()
        .rect_filled(screen, 0.0, Color32::from_black_alpha(150));
    let width = 720.0;
    let height = 480.0;
    let rect = Rect::from_center_size(screen.center(), Vec2::new(width, height));
    frame(ui, rect, theme);

    let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 44.0));
    ui.painter()
        .rect_filled(header, egui::CornerRadius::same(6), p.tabstrip);
    ui.painter().hline(
        header.x_range(),
        header.bottom() - 0.5,
        Stroke::new(1.0, p.border),
    );
    ui.painter().text(
        Pos2::new(header.left() + 16.0, header.center().y),
        Align2::LEFT_CENTER,
        format!(
            "{} onto {}",
            if zh {
                "交互式变基 (Interactive Rebase)"
            } else {
                "Interactive Rebase"
            },
            onto
        ),
        FontId::proportional(12.0),
        p.text,
    );

    let mut btn_x = header.right() - 16.0;
    btn_x -= 60.0;
    if secondary_button(
        ui,
        Rect::from_min_size(
            Pos2::new(btn_x, header.center().y - 11.0),
            Vec2::new(56.0, 22.0),
        ),
        if zh { "取消" } else { "Cancel" },
        None,
        theme,
    )
    .clicked()
    {
        *close = true;
    }

    let list_area = Rect::from_min_max(
        Pos2::new(rect.left() + 12.0, header.bottom() + 10.0),
        Pos2::new(rect.right() - 12.0, rect.bottom() - 52.0),
    );
    super::app_ui::region(ui, list_area, "rebase-todo-list", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("rebase-scroll")
            .show(ui, |ui| {
                for item in todos.iter_mut() {
                    let (row, _) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
                    let act_rect = Rect::from_min_size(
                        Pos2::new(row.left() + 6.0, row.center().y - 10.0),
                        Vec2::new(60.0, 20.0),
                    );
                    if secondary_button(ui, act_rect, &item.action, None, theme).clicked() {
                        item.action = match item.action.as_str() {
                            "pick" => "reword".into(),
                            "reword" => "edit".into(),
                            "edit" => "squash".into(),
                            "squash" => "drop".into(),
                            _ => "pick".into(),
                        };
                    }
                    ui.painter().text(
                        Pos2::new(row.left() + 76.0, row.center().y),
                        Align2::LEFT_CENTER,
                        &item.commit[..item.commit.len().min(8)],
                        FontId::monospace(9.5),
                        p.accent,
                    );
                    ui.painter().text(
                        Pos2::new(row.left() + 150.0, row.center().y),
                        Align2::LEFT_CENTER,
                        &item.message,
                        FontId::proportional(10.0),
                        p.text,
                    );
                }
            });
    });

    let start_btn = Rect::from_min_size(
        Pos2::new(rect.right() - 140.0, rect.bottom() - 38.0),
        Vec2::new(124.0, 26.0),
    );
    if primary_button(
        ui,
        start_btn,
        if zh { "开始变基" } else { "Start Rebase" },
        None,
        true,
        theme,
    )
    .clicked()
    {
        // 真正执行交互式变基：todo 经后台 job 跑 `git rebase -i`；
        // 冲突时由面板的 Continue/Abort 横幅接管。
        let steps: Vec<lazecode_git::RebaseStep> = todos
            .iter()
            .map(|row| lazecode_git::RebaseStep {
                action: row.action.clone(),
                commit: row.commit.clone(),
            })
            .collect();
        model.queue(Command::InteractiveRebase {
            onto: onto.to_string(),
            steps,
        });
        *close = true;
    }
}

/// 把一批路径移入系统废纸篓并收尾（对齐稳定版 deletePathsToTrash）：
/// 逐个 trash、关闭已删除路径下的编辑器标签、按删除数量/失败名单提示，
/// 最后刷新活动项目快照让变更列表收窄。目录路径会连带关闭其下所有标签。
fn delete_paths_to_trash(model: &mut AppModel, paths: &[std::path::PathBuf]) {
    let mut failed: Vec<String> = Vec::new();
    for path in paths {
        if let Err(err) = lazecode_fs::move_to_trash(path) {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
            log::warn!("trash delete failed for {name}: {err}");
            failed.push(name);
        }
    }
    // 文件已不存在：关闭其编辑器标签，避免留下幽灵缓冲。
    for project in model.projects.iter_mut() {
        let mut closed: Vec<u64> = Vec::new();
        for path in paths {
            for tab in &project.tabs {
                if let TabKind::File(file) = &tab.kind {
                    let under = path.is_dir() && file.doc.path.starts_with(path);
                    if file.doc.path == *path || under {
                        closed.push(tab.id);
                    }
                }
            }
        }
        for tab_id in closed {
            project.close_tab(tab_id);
        }
    }
    if failed.is_empty() {
        let zh = model.settings.language.is_zh();
        let message = if paths.len() == 1 {
            let name = paths[0]
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            if zh {
                format!("已删除 {name}（已移入废纸篓）")
            } else {
                format!("Deleted {name} (moved to the trash)")
            }
        } else if zh {
            format!("已删除 {} 个文件（已移入废纸篓）", paths.len())
        } else {
            format!("Deleted {} files (moved to the trash)", paths.len())
        };
        model.toast(message, ToastKind::Info);
    } else {
        let joined = failed.join(", ");
        model.toast(
            if model.settings.language.is_zh() {
                format!("删除失败：{joined}")
            } else {
                format!("Delete failed: {joined}")
            },
            ToastKind::Error,
        );
    }
    model.queue(Command::ReloadGit);
}

fn draw_dialog(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, screen: Rect) {
    let p = theme.palette;
    let mut dialog = std::mem::replace(&mut rt.dialog, DialogState::None);
    // Esc 关闭对话框（稳定版行为）；冲突解决编辑器除外——里面是用户
    // 手工合并的内容，误按 Esc 直接丢弃太危险。
    let esc_closes = !matches!(dialog, DialogState::ConflictResolve { .. });
    if esc_closes && ui.input(|input| input.key_pressed(Key::Escape)) {
        return;
    }
    if let DialogState::ConflictResolve {
        mut path,
        mut ours,
        mut theirs,
        mut worktree,
    } = dialog
    {
        ui.painter()
            .rect_filled(screen, 0.0, Color32::from_black_alpha(150));
        let width = (screen.width() - 40.0).min(1140.0);
        let height = (screen.height() - 40.0).min(760.0);
        let rect = Rect::from_center_size(screen.center(), Vec2::new(width, height));
        frame(ui, rect, theme);
        let mut close = false;
        draw_conflict_tool(
            ui,
            model,
            theme,
            rect,
            &mut path,
            &mut ours,
            &mut theirs,
            &mut worktree,
            &mut close,
        );
        if !close {
            rt.dialog = DialogState::ConflictResolve {
                path,
                ours,
                theirs,
                worktree,
            };
        }
        return;
    }

    if let DialogState::Squash {
        commits,
        mut value,
        mut error,
    } = dialog
    {
        let mut close = false;
        draw_squash_dialog(
            ui, model, theme, screen, &commits, &mut value, &mut error, &mut close,
        );
        if !close {
            rt.dialog = DialogState::Squash {
                commits,
                value,
                error,
            };
        }
        return;
    }

    if let DialogState::StashManager {
        mut stashes,
        mut selected,
        mut error,
        mut loaded,
    } = dialog
    {
        let mut close = false;
        draw_stash_manager_dialog(
            ui,
            model,
            theme,
            screen,
            &mut stashes,
            &mut selected,
            &mut error,
            &mut loaded,
            &mut close,
        );
        if !close {
            rt.dialog = DialogState::StashManager {
                stashes,
                selected,
                error,
                loaded,
            };
        }
        return;
    }

    if let DialogState::ShelfManager {
        mut shelves,
        mut selected,
        mut error,
        mut loaded,
    } = dialog
    {
        let mut close = false;
        draw_shelf_manager_dialog(
            ui,
            model,
            rt,
            theme,
            screen,
            &mut shelves,
            &mut selected,
            &mut error,
            &mut loaded,
            &mut close,
        );
        if !close {
            rt.dialog = DialogState::ShelfManager {
                shelves,
                selected,
                error,
                loaded,
            };
        }
        return;
    }

    if let DialogState::FileHistory {
        path,
        mut entries,
        mut selected,
        mut loaded,
    } = dialog
    {
        let mut close = false;
        draw_file_history_dialog(
            ui,
            model,
            theme,
            screen,
            &path,
            &mut entries,
            &mut selected,
            &mut loaded,
            &mut close,
        );
        if !close {
            rt.dialog = DialogState::FileHistory {
                path,
                entries,
                selected,
                loaded,
            };
        }
        return;
    }

    if let DialogState::GitTools {
        mut tab,
        mut submodules,
        mut lfs_status,
        mut loaded,
    } = dialog
    {
        let mut close = false;
        draw_git_tools_dialog(
            ui,
            model,
            theme,
            screen,
            &mut tab,
            &mut submodules,
            &mut lfs_status,
            &mut loaded,
            &mut close,
        );
        if !close {
            rt.dialog = DialogState::GitTools {
                tab,
                submodules,
                lfs_status,
                loaded,
            };
        }
        return;
    }

    if let DialogState::CloneRepo {
        mut url,
        mut dest,
        mut error,
    } = dialog
    {
        let mut close = false;
        draw_clone_repo_dialog(
            ui, model, theme, screen, &mut url, &mut dest, &mut error, &mut close,
        );
        if !close {
            rt.dialog = DialogState::CloneRepo { url, dest, error };
        }
        return;
    }

    if let DialogState::InteractiveRebase { onto, mut todos } = dialog {
        let mut close = false;
        draw_interactive_rebase_dialog(ui, model, theme, screen, &onto, &mut todos, &mut close);
        if !close {
            rt.dialog = DialogState::InteractiveRebase { onto, todos };
        }
        return;
    }
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
        DialogState::NewFile { .. } => (
            crate::tr!(model, "New file", "新建文件"),
            crate::tr!(model, "Create a file in the selected directory.", "在选定目录下创建文件。"),
            440.0,
            220.0,
        ),
        DialogState::NewDirectory { .. } => (
            crate::tr!(model, "New directory", "新建文件夹"),
            crate::tr!(model, "Create a directory in the selected location.", "在选定位置创建文件夹。"),
            440.0,
            220.0,
        ),
        DialogState::RenamePath { .. } => (
            crate::tr!(model, "Rename", "重命名"),
            crate::tr!(model, "Enter a new name for the file or directory.", "输入文件或目录的新名称。"),
            440.0,
            220.0,
        ),
        DialogState::DeletePath { is_dir, .. } => (
            crate::tr!(model, "Confirm delete", "确认删除"),
            if *is_dir {
                crate::tr!(
                    model,
                    "Move this directory and all its contents to the trash?",
                    "将此目录及其所有内容移入系统废纸篓？"
                )
            } else {
                crate::tr!(
                    model,
                    "Move this file to the trash?",
                    "将此文件移入系统废纸篓？"
                )
            },
            440.0,
            210.0,
        ),
        DialogState::ConfirmDeleteUntracked { .. } => (
            crate::tr!(model, "Delete files", "删除文件"),
            // 问句保持静态文案；具体文件名与数量由下方路径预览呈现
            // （与 ConfirmDiscard 同一模式）。文案对齐稳定版
            // trashUnversionedConfirm 语义。
            crate::tr!(
                model,
                "Move the selected untracked files to the trash?",
                "将所选未跟踪文件移入系统废纸篓？"
            ),
            460.0,
            250.0,
        ),
        DialogState::ConfirmDeleteShelf { .. } => (
            crate::tr!(model, "Delete shelf", "删除架藏"),
            crate::tr!(
                model,
                "Delete this shelf? The shelved patch will be removed permanently.",
                "删除此架藏？架藏补丁将被永久移除。"
            ),
            440.0,
            210.0,
        ),
        DialogState::NewShelf { .. } => (
            crate::tr!(model, "Shelve changes", "架藏更改"),
            crate::tr!(
                model,
                "Save uncommitted changes into a shelf patch file.",
                "将未提交的更改保存到补丁文件中。"
            ),
            440.0,
            230.0,
        ),
        DialogState::GoToLine { .. } => (
            crate::tr!(model, "Go to line", "跳转到行"),
            crate::tr!(
                model,
                "Enter a line number to jump to in the active document.",
                "输入要跳转到的行号。"
            ),
            380.0,
            210.0,
        ),
        DialogState::RenameTerminal { .. } => (
            crate::tr!(model, "Rename console tab", "重命名控制台标签"),
            crate::tr!(
                model,
                "Enter a new title for this console session.",
                "输入此控制台会话的新标题。"
            ),
            420.0,
            210.0,
        ),
        DialogState::NewChangelist { .. } => (
            crate::tr!(model, "New changelist", "新建变更列表"),
            crate::tr!(
                model,
                "Create a separate changelist to organize changes.",
                "创建一个独立的变更列表以归类更改。"
            ),
            420.0,
            210.0,
        ),
        DialogState::StashCreate { .. } => (
            crate::tr!(model, "Stash Changes", "贮藏更改"),
            crate::tr!(
                model,
                "Stash the working tree changes with an optional description.",
                "将工作区更改贮藏入栈，可附说明。"
            ),
            460.0,
            280.0,
        ),
        DialogState::ConfirmDiscard { .. } => (
            crate::tr!(model, "Rollback changes", "回滚更改"),
            crate::tr!(
                model,
                "Discard the uncommitted changes of the selected files? This cannot be undone.",
                "放弃所选文件的未提交更改？此操作不可撤销。"
            ),
            460.0,
            250.0,
        ),
        DialogState::DeleteChangelist { .. } => (
            crate::tr!(model, "Delete Changelist", "删除变更列表"),
            crate::tr!(
                model,
                "Choose what happens to the files of the deleted changelist.",
                "选择被删除变更列表中文件的去向。"
            ),
            480.0,
            250.0,
        ),
        DialogState::Unshelve { .. } => (
            crate::tr!(model, "Unshelve", "恢复架藏"),
            crate::tr!(
                model,
                "Restore the selected files to the working tree and put them into a changelist.",
                "恢复所选文件到工作区，并放入变更列表。"
            ),
            500.0,
            400.0,
        ),
        DialogState::CommitFiles { .. } => (
            crate::tr!(model, "Commit selected files", "提交所选文件"),
            crate::tr!(
                model,
                "Commit only the chosen files with their own message.",
                "仅提交所选文件，并使用独立的提交信息。"
            ),
            480.0,
            260.0,
        ),
        _ => return,
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
        DialogState::NewFile { value, error, .. }
        | DialogState::NewDirectory { value, error, .. }
        | DialogState::RenamePath { value, error, .. }
        | DialogState::NewShelf { value, error, .. }
        | DialogState::GoToLine { value, error }
        | DialogState::RenameTerminal { value, error, .. }
        | DialogState::NewChangelist { value, error } => {
            let input = Rect::from_min_size(
                Pos2::new(rect.left() + 22.0, rect.top() + 104.0),
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
        DialogState::DeletePath { path, .. } => {
            text_clipped(
                ui.painter(),
                Rect::from_min_max(
                    Pos2::new(rect.left() + 22.0, rect.top() + 96.0),
                    Pos2::new(rect.right() - 22.0, rect.top() + 118.0),
                ),
                &path.to_string_lossy(),
                FontId::monospace(10.5),
                p.rose,
                Align2::LEFT_CENTER,
            );
        }
        DialogState::ConfirmDeleteUntracked { paths } => {
            // 与 ConfirmDiscard 同款路径预览：前若干条 + 计数收尾。
            let show = paths.len().min(4);
            for (index, path) in paths.iter().take(show).enumerate() {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.to_string_lossy().into_owned());
                text_clipped(
                    ui.painter(),
                    Rect::from_min_max(
                        Pos2::new(rect.left() + 22.0, rect.top() + 96.0 + index as f32 * 18.0),
                        Pos2::new(
                            rect.right() - 22.0,
                            rect.top() + 112.0 + index as f32 * 18.0,
                        ),
                    ),
                    &name,
                    FontId::monospace(10.5),
                    p.text2,
                    Align2::LEFT_CENTER,
                );
            }
            if paths.len() > show {
                ui.painter().text(
                    Pos2::new(rect.left() + 22.0, rect.top() + 96.0 + show as f32 * 18.0),
                    Align2::LEFT_CENTER,
                    format!("… +{}", paths.len() - show),
                    FontId::monospace(10.5),
                    p.text3,
                );
            }
        }
        DialogState::StashCreate {
            message,
            include_untracked,
            error,
        } => {
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
                TextEdit::singleline(message)
                    .frame(egui::Frame::NONE)
                    .hint_text(crate::tr!(
                        model,
                        "Stash message (optional)",
                        "贮藏说明（可选）"
                    ))
                    .font(FontId::monospace(11.0)),
            );
            response.request_focus();
            let check_rect = Rect::from_center_size(
                Pos2::new(rect.left() + 30.0, input.bottom() + 22.0),
                Vec2::splat(16.0),
            );
            if checkbox(ui, check_rect, *include_untracked, theme).clicked() {
                *include_untracked = !*include_untracked;
            }
            ui.painter().text(
                Pos2::new(rect.left() + 42.0, input.bottom() + 22.0),
                Align2::LEFT_CENTER,
                crate::tr!(
                    model,
                    "Include untracked files (git stash -u)",
                    "包含未跟踪文件（git stash -u）"
                ),
                FontId::proportional(11.0),
                p.text2,
            );
            if let Some(err) = error {
                ui.painter().text(
                    Pos2::new(input.left(), input.bottom() + 44.0),
                    Align2::LEFT_CENTER,
                    err,
                    FontId::proportional(9.5),
                    p.rose,
                );
            }
        }
        DialogState::ConfirmDiscard { paths } => {
            // 预览前若干路径，超出部分以计数收尾。
            let show = paths.len().min(4);
            for (index, path) in paths.iter().take(show).enumerate() {
                text_clipped(
                    ui.painter(),
                    Rect::from_min_max(
                        Pos2::new(rect.left() + 22.0, rect.top() + 96.0 + index as f32 * 18.0),
                        Pos2::new(
                            rect.right() - 22.0,
                            rect.top() + 112.0 + index as f32 * 18.0,
                        ),
                    ),
                    path,
                    FontId::monospace(10.5),
                    p.text2,
                    Align2::LEFT_CENTER,
                );
            }
            if paths.len() > show {
                ui.painter().text(
                    Pos2::new(rect.left() + 22.0, rect.top() + 96.0 + show as f32 * 18.0),
                    Align2::LEFT_CENTER,
                    format!("… +{}", paths.len() - show),
                    FontId::monospace(10.5),
                    p.text3,
                );
            }
        }
        DialogState::DeleteChangelist {
            name,
            count,
            rollback,
        } => {
            ui.painter().text(
                Pos2::new(rect.left() + 22.0, rect.top() + 100.0),
                Align2::LEFT_CENTER,
                if model.settings.language.is_zh() {
                    format!("「{name}」中有 {count} 个文件，删除后这些文件：")
                } else {
                    format!(
                        "\"{name}\" holds {count} file(s). After deleting it, these files will:"
                    )
                },
                FontId::proportional(11.5),
                p.text2,
            );
            for (index, (label, target)) in [
                (
                    crate::tr!(
                        model,
                        "Move into the default changelist (recommended)",
                        "移入默认变更列表（推荐）"
                    ),
                    false,
                ),
                (
                    crate::tr!(
                        model,
                        "Rollback (discard the uncommitted changes)",
                        "Rollback（放弃这些文件的未提交更改）"
                    ),
                    true,
                ),
            ]
            .into_iter()
            .enumerate()
            {
                let row = Rect::from_min_size(
                    Pos2::new(rect.left() + 22.0, rect.top() + 126.0 + index as f32 * 24.0),
                    Vec2::new(rect.width() - 44.0, 22.0),
                );
                let check = Rect::from_center_size(
                    Pos2::new(row.left() + 8.0, row.center().y),
                    Vec2::splat(16.0),
                );
                if checkbox(ui, check, *rollback == target, theme).clicked() {
                    *rollback = target;
                }
                ui.painter().text(
                    Pos2::new(row.left() + 22.0, row.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    FontId::proportional(11.0),
                    if target { p.rose } else { p.text2 },
                );
            }
        }
        DialogState::Unshelve {
            files,
            target,
            new_name,
            error,
            ..
        } => {
            // 文件勾选清单（默认全选；Unshelve 支持部分恢复）。
            let list_rect = Rect::from_min_max(
                Pos2::new(rect.left() + 22.0, rect.top() + 94.0),
                Pos2::new(rect.right() - 22.0, rect.top() + 214.0),
            );
            ui.painter()
                .rect_filled(list_rect, egui::CornerRadius::same(5), p.panel);
            let row_h = 22.0;
            let visible = ((list_rect.height() / row_h) as usize).max(1);
            // 简单滚动：egui ScrollArea 逐帧渲染即可。
            egui::ScrollArea::vertical()
                .id_salt("unshelve-files")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(list_rect.width());
                    for (index, (path, selected)) in files.iter_mut().enumerate() {
                        let row = Rect::from_min_size(
                            Pos2::new(
                                list_rect.left() + 4.0,
                                list_rect.top() + 2.0 + index as f32 * row_h,
                            ),
                            Vec2::new(list_rect.width() - 8.0, row_h),
                        );
                        let check = Rect::from_center_size(
                            Pos2::new(row.left() + 12.0, row.center().y),
                            Vec2::splat(14.0),
                        );
                        if checkbox(ui, check, *selected, theme).clicked() {
                            *selected = !*selected;
                        }
                        text_clipped(
                            ui.painter(),
                            Rect::from_min_max(
                                Pos2::new(row.left() + 26.0, row.top()),
                                Pos2::new(row.right() - 4.0, row.bottom()),
                            ),
                            path,
                            FontId::monospace(10.5),
                            p.text2,
                            Align2::LEFT_CENTER,
                        );
                    }
                    let _ = visible;
                });
            // 目标 changelist：现有列表单选 + 新建输入（项目可能已关闭，
            // 不 unwrap；无项目时只剩新建输入）。
            let existing_lists: Vec<String> = model
                .active_project()
                .map(|project| {
                    lazecode_git::Changelists::open(&project.root)
                        .lists()
                        .to_vec()
                })
                .unwrap_or_default();
            let mut y = rect.top() + 224.0;
            for name in existing_lists.iter() {
                let row = Rect::from_min_size(
                    Pos2::new(rect.left() + 22.0, y),
                    Vec2::new((rect.width() - 44.0) * 0.5, 20.0),
                );
                let check = Rect::from_center_size(
                    Pos2::new(row.left() + 8.0, row.center().y),
                    Vec2::splat(14.0),
                );
                let is_target = *target == Some(name.clone());
                if checkbox(ui, check, is_target, theme).clicked() {
                    *target = Some(name.clone());
                }
                ui.painter().text(
                    Pos2::new(row.left() + 20.0, row.center().y),
                    Align2::LEFT_CENTER,
                    name,
                    FontId::proportional(10.5),
                    p.text2,
                );
                y += 22.0;
            }
            let new_row = Rect::from_min_size(
                Pos2::new(rect.left() + 22.0, y),
                Vec2::new((rect.width() - 44.0) * 0.5, 22.0),
            );
            let check = Rect::from_center_size(
                Pos2::new(new_row.left() + 8.0, new_row.center().y),
                Vec2::splat(14.0),
            );
            let new_is_target = target.is_none();
            if checkbox(ui, check, new_is_target, theme).clicked() && !new_is_target {
                *target = None;
            }
            ui.painter().text(
                Pos2::new(new_row.left() + 20.0, new_row.center().y),
                Align2::LEFT_CENTER,
                crate::tr!(model, "New changelist:", "新建变更列表："),
                FontId::proportional(10.5),
                p.text2,
            );
            let name_input = Rect::from_min_size(
                Pos2::new(rect.left() + 150.0, y - 2.0),
                Vec2::new(rect.right() - rect.left() - 172.0, 24.0),
            );
            ui.painter()
                .rect_filled(name_input, egui::CornerRadius::same(4), p.panel2);
            ui.painter().rect_stroke(
                name_input,
                egui::CornerRadius::same(4),
                Stroke::new(1.0, p.border),
                StrokeKind::Inside,
            );
            let response = ui.put(
                name_input.shrink2(Vec2::new(6.0, 3.0)),
                TextEdit::singleline(new_name)
                    .frame(egui::Frame::NONE)
                    .hint_text(crate::tr!(model, "Name", "名称"))
                    .font(FontId::monospace(10.5)),
            );
            if response.changed() && !new_name.trim().is_empty() {
                *target = None;
            }
            if let Some(err) = error {
                ui.painter().text(
                    Pos2::new(rect.left() + 22.0, rect.bottom() - 58.0),
                    Align2::LEFT_CENTER,
                    err,
                    FontId::proportional(9.5),
                    p.rose,
                );
            }
        }
        DialogState::CommitFiles { value, error, .. } => {
            let input = Rect::from_min_size(
                Pos2::new(rect.left() + 22.0, rect.top() + 94.0),
                Vec2::new(rect.width() - 44.0, 80.0),
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
                TextEdit::multiline(value)
                    .frame(egui::Frame::NONE)
                    .hint_text(crate::tr!(model, "Commit message", "提交信息"))
                    .font(FontId::proportional(12.0)),
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
        }
        DialogState::Quit => {}
        _ => {}
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
        DialogState::NewFile { .. } | DialogState::NewDirectory { .. } => {
            crate::tr!(model, "Create", "创建")
        }
        DialogState::RenamePath { .. } => crate::tr!(model, "Rename", "重命名"),
        DialogState::DeletePath { .. }
        | DialogState::ConfirmDeleteUntracked { .. }
        | DialogState::ConfirmDeleteShelf { .. } => {
            crate::tr!(model, "Delete", "删除")
        }
        DialogState::NewShelf { .. } => crate::tr!(model, "Save", "保存"),
        DialogState::GoToLine { .. } => crate::tr!(model, "Go", "跳转"),
        DialogState::RenameTerminal { .. } => crate::tr!(model, "Rename", "重命名"),
        DialogState::NewChangelist { .. } => crate::tr!(model, "Create", "创建"),
        DialogState::StashCreate { .. } => crate::tr!(model, "Stash", "贮藏"),
        DialogState::ConfirmDiscard { .. } => crate::tr!(model, "Rollback", "回滚"),
        DialogState::DeleteChangelist { .. } => crate::tr!(model, "Delete", "删除"),
        DialogState::Unshelve { .. } => crate::tr!(model, "Unshelve", "恢复"),
        DialogState::CommitFiles { .. } => crate::tr!(model, "Commit", "提交"),
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
            DialogState::NewFile { dir, value, .. } => {
                let name = value.trim();
                if name.is_empty() {
                    rt.dialog = DialogState::NewFile {
                        dir: dir.clone(),
                        value: value.clone(),
                        error: Some(
                            crate::tr!(model, "Name cannot be empty", "名称不能为空").to_string(),
                        ),
                    };
                } else {
                    let target = dir.join(name);
                    if target.exists() {
                        rt.dialog = DialogState::NewFile {
                            dir: dir.clone(),
                            value: value.clone(),
                            error: Some(
                                crate::tr!(model, "File already exists", "文件已存在").to_string(),
                            ),
                        };
                    } else if let Err(e) = std::fs::write(&target, "") {
                        rt.dialog = DialogState::NewFile {
                            dir: dir.clone(),
                            value: value.clone(),
                            error: Some(e.to_string()),
                        };
                    } else {
                        if let Some(active) = model.active_project {
                            let _ = model.projects[active].open_file(target, &model.settings);
                        }
                        close = true;
                    }
                }
            }
            DialogState::NewDirectory { dir, value, .. } => {
                let name = value.trim();
                if name.is_empty() {
                    rt.dialog = DialogState::NewDirectory {
                        dir: dir.clone(),
                        value: value.clone(),
                        error: Some(
                            crate::tr!(model, "Name cannot be empty", "名称不能为空").to_string(),
                        ),
                    };
                } else {
                    let target = dir.join(name);
                    if target.exists() {
                        rt.dialog = DialogState::NewDirectory {
                            dir: dir.clone(),
                            value: value.clone(),
                            error: Some(
                                crate::tr!(model, "Directory already exists", "目录已存在")
                                    .to_string(),
                            ),
                        };
                    } else if let Err(e) = std::fs::create_dir_all(&target) {
                        rt.dialog = DialogState::NewDirectory {
                            dir: dir.clone(),
                            value: value.clone(),
                            error: Some(e.to_string()),
                        };
                    } else {
                        close = true;
                    }
                }
            }
            DialogState::RenamePath {
                old_path, value, ..
            } => {
                let name = value.trim();
                if name.is_empty() {
                    rt.dialog = DialogState::RenamePath {
                        old_path: old_path.clone(),
                        value: value.clone(),
                        error: Some(
                            crate::tr!(model, "Name cannot be empty", "名称不能为空").to_string(),
                        ),
                    };
                } else if let Some(parent) = old_path.parent() {
                    let target = parent.join(name);
                    if let Err(e) = std::fs::rename(old_path, &target) {
                        rt.dialog = DialogState::RenamePath {
                            old_path: old_path.clone(),
                            value: value.clone(),
                            error: Some(e.to_string()),
                        };
                    } else {
                        close = true;
                    }
                } else {
                    close = true;
                }
            }
            DialogState::DeletePath { path, .. } => {
                // 删除走废纸篓语义（稳定版 fs_delete），不再永久 unlink；
                // 关标签/提示/刷新统一在 delete_paths_to_trash 里收尾
                // （目录路径由助手连带关闭其下所有标签）。
                delete_paths_to_trash(model, std::slice::from_ref(path));
                close = true;
            }
            DialogState::ConfirmDeleteUntracked { paths } => {
                delete_paths_to_trash(model, paths);
                close = true;
            }
            DialogState::ConfirmDeleteShelf { name } => {
                // 稳定版删除架藏前有确认；确认后删除并回到（重新懒加载的）
                // 架藏管理器。
                if let Some(project) = model.active_project() {
                    let root = project.root.clone();
                    let _ = lazecode_git::shelf_delete(&root, name);
                    let zh = model.settings.language.is_zh();
                    model.toast(
                        if zh {
                            format!("已删除架藏 {name}")
                        } else {
                            format!("Deleted shelf {name}")
                        },
                        ToastKind::Info,
                    );
                }
                rt.dialog = DialogState::ShelfManager {
                    shelves: Vec::new(),
                    selected: 0,
                    error: None,
                    loaded: false,
                };
                close = true;
            }
            DialogState::NewShelf { value, paths, .. } => {
                let name = value.trim().to_string();
                if name.is_empty() {
                    rt.dialog = DialogState::NewShelf {
                        value: name,
                        error: Some(
                            crate::tr!(model, "Enter a shelf name", "请输入架藏名称").to_string(),
                        ),
                        paths: paths.clone(),
                    };
                    close = true;
                } else {
                    model.queue(Command::ShelfSave {
                        name,
                        paths: paths.clone(),
                    });
                    close = true;
                }
            }
            DialogState::GoToLine { value, .. } => {
                let text = value.trim();
                if let Ok(line) = text.parse::<usize>() {
                    model.queue(Command::GoToLine(line));
                    close = true;
                } else {
                    rt.dialog = DialogState::GoToLine {
                        value: value.clone(),
                        error: Some(
                            crate::tr!(model, "Enter a valid line number", "请输入有效的行号")
                                .to_string(),
                        ),
                    };
                    close = true;
                }
            }
            DialogState::RenameTerminal {
                terminal_id, value, ..
            } => {
                let name = value.trim().to_string();
                if !name.is_empty() {
                    if let Some(project) = model.active_project_mut() {
                        if let Some(term) = project.term_by_id_mut(*terminal_id) {
                            term.title = name;
                        }
                    }
                }
                close = true;
            }
            DialogState::NewChangelist { value, .. } => {
                let name = value.trim().to_string();
                if name.is_empty() {
                    rt.dialog = DialogState::NewChangelist {
                        value: name,
                        error: Some(
                            crate::tr!(model, "Enter a changelist name", "请输入变更列表名称")
                                .to_string(),
                        ),
                    };
                    close = true;
                } else {
                    // 写入走后台任务，与 stage/commit 等路径一致（不再在
                    // UI 线程同步 IO）。
                    model.queue(Command::ChangelistCreate(name));
                    close = true;
                }
            }
            DialogState::StashCreate {
                message,
                include_untracked,
                ..
            } => {
                let message = message.trim().to_string();
                model.queue(Command::StashPush {
                    message: (!message.is_empty()).then_some(message),
                    include_untracked: *include_untracked,
                });
                close = true;
            }
            DialogState::ConfirmDiscard { paths } => {
                let paths = paths.clone();
                if !paths.is_empty() {
                    model.queue(Command::DiscardFiles(paths));
                }
                close = true;
            }
            DialogState::DeleteChangelist { name, rollback, .. } => {
                model.queue(Command::ChangelistDelete {
                    name: name.clone(),
                    rollback: *rollback,
                });
                close = true;
            }
            DialogState::Unshelve {
                name,
                files,
                target,
                new_name,
                ..
            } => {
                let selected: Vec<String> = files
                    .iter()
                    .filter(|(_, selected)| *selected)
                    .map(|(path, _)| path.clone())
                    .collect();
                let mut target = target.clone();
                if target.is_none() {
                    let new_name = new_name.trim().to_string();
                    if new_name.is_empty() {
                        rt.dialog = DialogState::Unshelve {
                            name: name.clone(),
                            files: files.clone(),
                            target: None,
                            new_name,
                            error: Some(
                                crate::tr!(
                                    model,
                                    "Pick a changelist or enter a new name",
                                    "请选择目标变更列表或输入新名称"
                                )
                                .to_string(),
                            ),
                        };
                        close = true;
                    } else {
                        target = Some(new_name);
                    }
                }
                if let Some(target) = target {
                    if selected.is_empty() {
                        rt.dialog = DialogState::Unshelve {
                            name: name.clone(),
                            files: files.clone(),
                            target: Some(target),
                            new_name: String::new(),
                            error: Some(
                                crate::tr!(
                                    model,
                                    "Select at least one file to unshelve",
                                    "请至少勾选一个要恢复的文件"
                                )
                                .to_string(),
                            ),
                        };
                        close = true;
                    } else {
                        model.queue(Command::ShelfRestore {
                            name: name.clone(),
                            paths: Some(selected),
                            target_list: Some(target),
                            delete_after: false,
                        });
                        close = true;
                    }
                }
            }
            DialogState::CommitFiles { paths, value, .. } => {
                let message = value.trim().to_string();
                if message.is_empty() {
                    rt.dialog = DialogState::CommitFiles {
                        paths: paths.clone(),
                        value: value.clone(),
                        error: Some(
                            crate::tr!(model, "Add a commit message first", "请先填写提交信息")
                                .to_string(),
                        ),
                    };
                    close = true;
                } else {
                    model.queue(Command::CommitPaths {
                        paths: paths.clone(),
                        message,
                    });
                    close = true;
                }
            }
            _ => {}
        }
    }
    if !close {
        rt.dialog = dialog;
    } else {
        rt.branch_create_error = None;
    }
    let _ = (model, ToastKind::Info);
}
