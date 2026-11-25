//! Project and worktree context menus.

use super::prelude::*;

pub(super) fn draw_project_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
) {
    let p = theme.palette;
    let zh = model.settings.language.is_zh();
    let open_roots: Vec<PathBuf> = model
        .projects
        .iter()
        .filter(|project| !project.is_single_file)
        .map(|project| project.root.clone())
        .collect();
    // An open project is not offered again as a recent entry.
    let recents: Vec<PathBuf> = model
        .recent
        .iter()
        .filter(|path| !open_roots.contains(path))
        .take(12)
        .cloned()
        .collect();
    let open_indices: Vec<usize> = model
        .projects
        .iter()
        .enumerate()
        .filter(|(_, project)| !(project.is_single_file && project.tabs.is_empty()))
        .map(|(index, _)| index)
        .collect();
    let open_count = open_indices.len();
    let recent_count = recents.len();
    let header_h = 36.0;
    let footer_h = 42.0;
    // Include item spacing or the popover is a few px short and shows a
    // scrollbar.
    const ROW_GAP: f32 = 4.0;
    let widget_count = 2
        + open_count
        + recent_count
        + usize::from(open_count == 0)
        + usize::from(recent_count == 0);
    let content_h = 24.0
        + open_count as f32 * 42.0
        + if open_count == 0 { 28.0 } else { 0.0 }
        + 24.0
        + recent_count as f32 * 40.0
        + if recent_count == 0 { 28.0 } else { 0.0 }
        + ROW_GAP * widget_count.saturating_sub(1) as f32;
    // Stay under 80% of the screen; only the excess scrolls.
    let max_body_h = (screen.height() * 0.8 - header_h - footer_h).max(120.0);
    let body_h = content_h.min(max_body_h);
    let rect = popover_rect(rt, screen, menu.anchor, 326.0, header_h + body_h + footer_h);
    menu_frame(ui, rect, theme);
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, rect.top() + 20.0),
        Align2::LEFT_CENTER,
        if zh { "项目" } else { "Projects" },
        FontId::proportional(12.5),
        p.text3,
    );
    ui.painter().text(
        Pos2::new(rect.right() - 14.0, rect.top() + 20.0),
        Align2::RIGHT_CENTER,
        if zh {
            format!("{open_count} 个已打开")
        } else {
            format!("{open_count} open")
        },
        FontId::monospace(11.0),
        p.text3,
    );

    let body_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 4.0, rect.top() + header_h),
        Pos2::new(rect.right() - 4.0, rect.bottom() - footer_h),
    );
    let mut close = false;
    let mut switch_to: Option<usize> = None;
    let mut open_recent: Option<PathBuf> = None;
    let mut remove_recent: Option<PathBuf> = None;
    crate::app_ui::region(ui, body_rect, "project-menu-body", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("project-menu-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = ROW_GAP;
                let width = ui.available_width();
                let (section, _) = ui.allocate_exact_size(Vec2::new(width, 24.0), Sense::hover());
                ui.painter().text(
                    Pos2::new(section.left() + 10.0, section.center().y),
                    Align2::LEFT_CENTER,
                    if zh { "已打开" } else { "Open" },
                    FontId::proportional(11.0),
                    p.text3,
                );
                if open_count == 0 {
                    let (empty, _) = ui.allocate_exact_size(Vec2::new(width, 28.0), Sense::hover());
                    ui.painter().text(
                        Pos2::new(empty.left() + 14.0, empty.center().y),
                        Align2::LEFT_CENTER,
                        if zh {
                            "暂无已打开项目"
                        } else {
                            "No projects open"
                        },
                        FontId::proportional(12.0),
                        p.text3,
                    );
                }
                for &index in &open_indices {
                    let project = &model.projects[index];
                    let is_single = project.is_single_file;
                    let (row, response) =
                        ui.allocate_exact_size(Vec2::new(width, 42.0), Sense::click());
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
                    if is_single {
                        paint_icon(ui.painter(), avatar.shrink(6.0), Icon::File, tone);
                    } else {
                        ui.painter().text(
                            avatar.center(),
                            Align2::CENTER_CENTER,
                            &project.initials,
                            FontId::proportional(11.0),
                            tone,
                        );
                    }
                    let display_name = if is_single {
                        crate::tr!(model, "Single files", "单文件").to_string()
                    } else {
                        project.name.clone()
                    };
                    let subtitle = if is_single {
                        if zh {
                            format!("{} 个文件", project.tabs.len())
                        } else {
                            format!("{} files", project.tabs.len())
                        }
                    } else {
                        project.root.display().to_string()
                    };
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(row.left() + 40.0, row.top() + 4.0),
                            Pos2::new(row.right() - 58.0, row.top() + 22.0),
                        ),
                        &display_name,
                        FontId::proportional(13.0),
                        p.text,
                        Align2::LEFT_CENTER,
                    );
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(row.left() + 40.0, row.top() + 21.0),
                            Pos2::new(row.right() - 58.0, row.bottom() - 3.0),
                        ),
                        &subtitle,
                        FontId::proportional(11.0),
                        p.text3,
                        Align2::LEFT_CENTER,
                    );
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
                }

                let (section, _) = ui.allocate_exact_size(Vec2::new(width, 24.0), Sense::hover());
                ui.painter().text(
                    Pos2::new(section.left() + 10.0, section.center().y),
                    Align2::LEFT_CENTER,
                    if zh {
                        "最近打开"
                    } else {
                        "Recent projects"
                    },
                    FontId::proportional(11.0),
                    p.text3,
                );
                if recents.is_empty() {
                    let (empty, _) = ui.allocate_exact_size(Vec2::new(width, 28.0), Sense::hover());
                    ui.painter().text(
                        Pos2::new(empty.left() + 14.0, empty.center().y),
                        Align2::LEFT_CENTER,
                        if zh {
                            "暂无最近项目"
                        } else {
                            "No recent projects"
                        },
                        FontId::proportional(12.0),
                        p.text3,
                    );
                }
                for path in &recents {
                    let (row, response) =
                        ui.allocate_exact_size(Vec2::new(width, 40.0), Sense::click());
                    if response.hovered() {
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::same(5), p.hover);
                    }
                    let icon_box = Rect::from_center_size(
                        Pos2::new(row.left() + 20.0, row.center().y),
                        Vec2::splat(24.0),
                    );
                    ui.painter()
                        .rect_filled(icon_box, egui::CornerRadius::same(6), p.panel2);
                    paint_icon(ui.painter(), icon_box.shrink(5.0), Icon::Folder, p.text3);
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(row.left() + 40.0, row.top() + 3.0),
                            Pos2::new(row.right() - 38.0, row.top() + 21.0),
                        ),
                        &name,
                        FontId::proportional(13.0),
                        p.text2,
                        Align2::LEFT_CENTER,
                    );
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(row.left() + 40.0, row.top() + 20.0),
                            Pos2::new(row.right() - 38.0, row.bottom() - 3.0),
                        ),
                        &path.display().to_string(),
                        FontId::proportional(11.0),
                        p.text3,
                        Align2::LEFT_CENTER,
                    );
                    let remove_rect = Rect::from_center_size(
                        Pos2::new(row.right() - 18.0, row.center().y),
                        Vec2::splat(20.0),
                    );
                    let remove_resp = ui.interact(
                        remove_rect,
                        ui.scope_id().with(("recent-remove", path.clone())),
                        Sense::click(),
                    );
                    paint_icon(
                        ui.painter(),
                        remove_rect.shrink(4.0),
                        Icon::X,
                        if remove_resp.hovered() {
                            p.rose
                        } else {
                            p.text3
                        },
                    );
                    if remove_resp.clicked() {
                        remove_recent = Some(path.clone());
                    } else if response.clicked() {
                        open_recent = Some(path.clone());
                        close = true;
                    }
                }
            });
    });

    let footer = Rect::from_min_max(
        Pos2::new(rect.left() + 6.0, rect.bottom() - 36.0),
        Pos2::new(rect.right() - 6.0, rect.bottom() - 4.0),
    );
    let has_active = model
        .active_project
        .is_some_and(|index| open_indices.contains(&index));
    let open_width = if has_active {
        footer.width() * 0.5 - 3.0
    } else {
        footer.width()
    };
    let open_rect = Rect::from_min_size(footer.min, Vec2::new(open_width, 30.0));
    if secondary_button(
        ui,
        open_rect,
        if zh { "打开…" } else { "Open…" },
        Some(Icon::Plus),
        theme,
    )
    .clicked()
    {
        model.queue(Command::OpenProjectDialog);
        close = true;
    }
    if let Some(active) = model.active_project {
        let close_rect = Rect::from_min_size(
            Pos2::new(open_rect.right() + 6.0, footer.top()),
            Vec2::new(footer.width() * 0.5 - 3.0, 30.0),
        );
        if secondary_button(
            ui,
            close_rect,
            if zh { "关闭项目" } else { "Close project" },
            Some(Icon::X),
            theme,
        )
        .clicked()
        {
            let dirty = model.projects[active].tabs.iter().any(|tab| tab.is_dirty());
            if dirty {
                model.toast(
                    if zh {
                        "该项目有未保存的文件，请先保存或关闭标签页"
                    } else {
                        "This project has unsaved files; save or close its tabs first"
                    },
                    lazecode_core::ToastKind::Warning,
                );
            } else {
                rt.request_terminal_close(
                    model,
                    crate::runtime::TerminalCloseAction::Project(active),
                );
            }
            close = true;
        }
    }
    if let Some(path) = remove_recent {
        model.forget_recent(&path);
    }
    if let Some(index) = switch_to {
        model.switch_project(index);
        rt.clear_terminal_cache();
    }
    if let Some(path) = open_recent {
        model.queue(Command::OpenProject(path));
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if close {
        rt.menu = None;
    } else {
        rt.menu = Some(menu);
    }
}

pub(super) fn draw_worktree_menu(
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
    let rect = popover_rect(rt, screen, menu.anchor, 324.0, height);
    menu_frame(ui, rect, theme);
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
    crate::app_ui::region(ui, rows_rect, "worktree-menu", |ui| {
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
        let (branch, create_new, default_loc) = if let Some(project) = model.projects.get(index) {
            let (branch, create_new) = next_worktree_branch(project);
            (
                branch,
                create_new,
                format!("{}/.worktree", project.root.display()),
            )
        } else {
            (String::new(), true, String::new())
        };
        rt.dialog = DialogState::NewWorktree {
            name: String::new(),
            branch,
            create_new,
            location: default_loc,
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
            let changes = lazecode_git::snapshot(&worktree.path)
                .map(|snapshot| snapshot.changes.len())
                .unwrap_or(0);
            rt.dialog = DialogState::RemoveWorktree {
                name: name.clone(),
                worktree_path: worktree.path.clone(),
                dirty: changes > 0,
                changes,
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

/// Default branch for the New worktree dialog. Prefer an unused local branch;
/// when every local branch is occupied, start in "create a new branch" mode.
fn next_worktree_branch(project: &lazecode_core::ProjectState) -> (String, bool) {
    let used = used_worktree_branches(project);
    let current = project.branch();
    if !current.is_empty() && current != "no branch" && !used.contains_key(&current) {
        return (current, false);
    }
    if let Some(branch) = project
        .branches
        .iter()
        .find(|branch| !branch.remote && !used.contains_key(&branch.name))
    {
        return (branch.name.clone(), false);
    }
    (String::new(), true)
}
