//! Git log, commit, tag and branch-tree context menus.

use super::prelude::*;

pub(super) fn draw_log_branch_menu(
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
    let height = 12.0 + (rows_vec.len() as f32 * 24.0 + 26.0).min(320.0);
    let rect = popover_rect(rt, screen, menu.anchor, 260.0, height);
    menu_frame(ui, rect, theme);
    let mut pick: Option<Option<String>> = None;
    let rows = Rect::from_min_max(
        Pos2::new(rect.left() + 6.0, rect.top() + 6.0),
        Pos2::new(rect.right() - 6.0, rect.bottom() - 6.0),
    );
    crate::app_ui::region(ui, rows, "log-branch-menu", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("log-branch-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if is_author_menu {
                    // Author rows. "All" row first.
                    let (row, response) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click());
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
                            Vec2::new(ui.available_width(), 22.0),
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
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click());
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
                        .allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click());
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
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click());
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
pub(super) fn draw_log_date_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
) {
    let p = theme.palette;
    let current_date = model
        .projects
        .get(project_idx)
        .map(|p| p.log_query.date)
        .unwrap_or_default();
    let items = [
        (
            lazecode_core::DatePreset::All,
            crate::tr!(model, "All", "全部"),
        ),
        (
            lazecode_core::DatePreset::Today,
            crate::tr!(model, "Today", "今天"),
        ),
        (
            lazecode_core::DatePreset::Week,
            crate::tr!(model, "Last 7 days", "最近 7 天"),
        ),
        (
            lazecode_core::DatePreset::Month,
            crate::tr!(model, "Last 30 days", "最近 30 天"),
        ),
    ];
    let height = items.len() as f32 * 26.0 + 12.0;
    let rect = popover_rect(rt, screen, menu.anchor, 160.0, height);
    menu_frame(ui, rect, theme);

    let mut pick: Option<lazecode_core::DatePreset> = None;
    let mut y = rect.top() + 6.0;
    for (preset, label) in items {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 6.0, y),
            Vec2::new(rect.width() - 12.0, 24.0),
        );
        let response = ui.interact(
            row,
            ui.scope_id().with(("log-date-opt", preset as u8)),
            Sense::click(),
        );
        if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(4), p.hover);
        }
        ui.painter().text(
            Pos2::new(row.left() + 12.0, row.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(12.5),
            if current_date == preset {
                p.accent
            } else {
                p.text
            },
        );
        if current_date == preset {
            paint_icon(
                ui.painter(),
                Rect::from_center_size(
                    Pos2::new(row.right() - 14.0, row.center().y),
                    Vec2::splat(12.0),
                ),
                Icon::Check,
                p.accent,
            );
        }
        if response.clicked() {
            pick = Some(preset);
        }
        y += 26.0;
    }

    let mut close = false;
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if let Some(preset) = pick {
        if let Some(project) = model.projects.get_mut(project_idx) {
            project.log_query.date = preset;
        }
        model.queue(Command::LogFilterChanged);
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

/// Commit context menu: read-only actions in this milestone
/// (copy hash / go to parent / child; history rewrite arrives with M5).
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_commit_menu(
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
    let _p = theme.palette;
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
            (crate::tr!(model, "Checkout revision", "检出该版本"), 11),
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
            (crate::tr!(model, "Copy commit patch", "复制提交补丁"), 15),
        ];
        if is_head {
            list.push((crate::tr!(model, "Undo HEAD commit", "撤销 HEAD 提交"), 14));
        }
        list
    };
    let height = items.len() as f32 * MENU_ROW_H + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 260.0, height);
    menu_frame(ui, rect, theme);
    // 稳定版提交右键菜单（CommitContextMenu）没有标题行。
    let mut action: Option<u8> = None;
    for (i, (label, code)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, rect.top() + 4.0 + i as f32 * MENU_ROW_H),
            Vec2::new(rect.width() - 8.0, MENU_ROW_H),
        );
        let danger = matches!(*code, 6 | 23 | 14);
        if menu_row(
            ui,
            theme,
            row,
            ui.scope_id().with(("commitmenu", i)),
            label,
            danger,
            true,
        ) {
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
                // 「与本地比较」：打开 Compare 抽屉，列出该提交与本地
                // 工作区之间的全部差异文件（含未提交改动），而不是只开
                // 单个文件 diff。
                model.queue(Command::OpenCompareWithLocal {
                    commit: commit.id.clone(),
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
                // `git rebase -i <onto>` lists the commits *after* onto,
                // oldest first. The log is newest-first, so walk backwards
                // from the clicked row to HEAD and reverse.
                let mut todos = Vec::new();
                if let Some(project) = model.projects.get(index) {
                    if let Some(position) = project.log.iter().position(|row| row.id == commit.id) {
                        for row in project.log[..position].iter().rev() {
                            if row.parents.len() > 1 || row.in_head == Some(false) {
                                continue;
                            }
                            todos.push(crate::runtime::RebaseTodoRow {
                                action: "pick".to_string(),
                                commit: row.id.clone(),
                                message: row.message.lines().next().unwrap_or("").to_string(),
                            });
                        }
                    }
                }
                rt.dialog = DialogState::InteractiveRebase {
                    onto: commit.id.clone(),
                    todos,
                };
            }
            11 => {
                model.queue(Command::CheckoutRevision(commit.id.clone()));
            }
            12 => {
                rt.dialog = DialogState::NewBranch {
                    value: String::new(),
                    start_point: Some(commit.id.clone()),
                    checkout: true,
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
                // 稳定版 handleCompareVersions：from=后选的（旧），to=先选的（新），
                // 打开 CompareDrawer 会话而非直接开 diff。
                model.queue(Command::CompareOpen {
                    from: selected_commits[1].clone(),
                    to: selected_commits[0].clone(),
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

/// Context menu for a tag row: checkout, working-tree diff, merge, push,
/// delete.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_tag_context_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    index: usize,
    payload: Option<&str>,
) {
    let Some(tag) = payload else { return };
    let current = model.projects[index].branch();
    let remote = model.projects[index]
        .remotes
        .first()
        .map(|remote| remote.name.clone())
        .unwrap_or_else(|| "origin".to_string());
    let items = [
        (
            crate::tr!(model, "Checkout tag", "检出标签").to_string(),
            0u8,
        ),
        (
            crate::tr!(model, "Show Diff with Working Tree", "与工作区比较差异").to_string(),
            1,
        ),
        (
            format!(
                "{} '{tag}' {} '{current}'",
                crate::tr!(model, "Merge", "合并"),
                crate::tr!(model, "into", "到")
            ),
            2,
        ),
        (
            format!(
                "{} '{}' {} '{}'",
                crate::tr!(model, "Push tag", "推送标签"),
                tag,
                crate::tr!(model, "to", "到"),
                remote
            ),
            3,
        ),
        (crate::tr!(model, "Delete tag", "删除标签").to_string(), 4),
    ];
    let height = items.len() as f32 * MENU_ROW_H + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 300.0, height);
    menu_frame(ui, rect, theme);
    let mut action = None;
    for (i, (label, code)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, rect.top() + 4.0 + i as f32 * MENU_ROW_H),
            Vec2::new(rect.width() - 8.0, MENU_ROW_H),
        );
        if menu_row(
            ui,
            theme,
            row,
            ui.scope_id().with(("tagctx", i)),
            label,
            *code == 4,
            true,
        ) {
            action = Some(*code);
        }
    }
    let mut close = false;
    if let Some(code) = action {
        match code {
            0 => model.queue(Command::CheckoutRevision(tag.to_string())),
            1 => model.queue(Command::OpenCompareWithLocal {
                commit: tag.to_string(),
            }),
            2 => model.queue(Command::Merge {
                ref_name: tag.to_string(),
            }),
            3 => model.queue(Command::PushTag {
                remote: remote.clone(),
                tag: tag.to_string(),
            }),
            4 => model.queue(Command::DeleteTag(tag.to_string())),
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

/// Git log branch tree context menu. Items differ by remote/local.
/// Payload format: `name\u{1}local|remote`.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_commit_history_menu(
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
    let rect = popover_rect(rt, screen, menu.anchor, 320.0, height);
    menu_frame(ui, rect, theme);

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
pub(super) fn draw_log_sort_options_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
) {
    let p = theme.palette;
    let zh = model.settings.language.is_zh();
    let rect = popover_rect(rt, screen, menu.anchor, 220.0, 160.0);
    menu_frame(ui, rect, theme);

    let (topo, first_parent, no_merges) = {
        let q = &model.projects[project_idx].log_query;
        (q.topo_order, q.first_parent, q.no_merges)
    };

    let mut y = rect.top() + 6.0;
    // Header 1: Sort by
    ui.painter().text(
        Pos2::new(rect.left() + 12.0, y + 8.0),
        Align2::LEFT_CENTER,
        if zh { "排序方式" } else { "Sort by" },
        FontId::proportional(11.0),
        p.text3,
    );
    y += 20.0;

    // Item 1: Date Order
    let item1 = Rect::from_min_size(
        Pos2::new(rect.left() + 4.0, y),
        Vec2::new(rect.width() - 8.0, 24.0),
    );
    let resp1 = ui.interact(item1, ui.scope_id().with("sort-date"), Sense::click());
    if resp1.hovered() {
        ui.painter()
            .rect_filled(item1, egui::CornerRadius::same(4), p.hover);
    }
    if !topo {
        ui.painter().text(
            Pos2::new(item1.left() + 10.0, item1.center().y),
            Align2::LEFT_CENTER,
            "✓",
            FontId::proportional(11.0),
            p.accent,
        );
    }
    ui.painter().text(
        Pos2::new(item1.left() + 26.0, item1.center().y),
        Align2::LEFT_CENTER,
        if zh {
            "按提交日期"
        } else {
            "By Commit Date"
        },
        FontId::proportional(12.0),
        p.text,
    );
    if resp1.clicked() {
        model.projects[project_idx].log_query.topo_order = false;
        model.queue(Command::LogFilterChanged);
    }
    y += 25.0;

    // Item 2: Topo Order
    let item2 = Rect::from_min_size(
        Pos2::new(rect.left() + 4.0, y),
        Vec2::new(rect.width() - 8.0, 24.0),
    );
    let resp2 = ui.interact(item2, ui.scope_id().with("sort-topo"), Sense::click());
    if resp2.hovered() {
        ui.painter()
            .rect_filled(item2, egui::CornerRadius::same(4), p.hover);
    }
    if topo {
        ui.painter().text(
            Pos2::new(item2.left() + 10.0, item2.center().y),
            Align2::LEFT_CENTER,
            "✓",
            FontId::proportional(11.0),
            p.accent,
        );
    }
    ui.painter().text(
        Pos2::new(item2.left() + 26.0, item2.center().y),
        Align2::LEFT_CENTER,
        if zh {
            "按拓扑排序"
        } else {
            "Topologically"
        },
        FontId::proportional(12.0),
        p.text,
    );
    if resp2.clicked() {
        model.projects[project_idx].log_query.topo_order = true;
        model.queue(Command::LogFilterChanged);
    }
    y += 27.0;

    ui.painter().hline(
        (rect.left() + 8.0)..=(rect.right() - 8.0),
        y,
        Stroke::new(1.0, p.border_soft),
    );
    y += 4.0;

    // Header 2: Options
    ui.painter().text(
        Pos2::new(rect.left() + 12.0, y + 8.0),
        Align2::LEFT_CENTER,
        if zh { "选项" } else { "Options" },
        FontId::proportional(11.0),
        p.text3,
    );
    y += 20.0;

    // Item 3: First Parent
    let item3 = Rect::from_min_size(
        Pos2::new(rect.left() + 4.0, y),
        Vec2::new(rect.width() - 8.0, 24.0),
    );
    let resp3 = ui.interact(
        item3,
        ui.scope_id().with("opt-first-parent"),
        Sense::click(),
    );
    if resp3.hovered() {
        ui.painter()
            .rect_filled(item3, egui::CornerRadius::same(4), p.hover);
    }
    if first_parent {
        ui.painter().text(
            Pos2::new(item3.left() + 10.0, item3.center().y),
            Align2::LEFT_CENTER,
            "✓",
            FontId::proportional(11.0),
            p.accent,
        );
    }
    ui.painter().text(
        Pos2::new(item3.left() + 26.0, item3.center().y),
        Align2::LEFT_CENTER,
        if zh {
            "仅第一父提交"
        } else {
            "First Parent"
        },
        FontId::proportional(12.0),
        p.text,
    );
    if resp3.clicked() {
        model.projects[project_idx].log_query.first_parent = !first_parent;
        model.queue(Command::LogFilterChanged);
    }
    y += 25.0;

    // Item 4: No Merges
    let item4 = Rect::from_min_size(
        Pos2::new(rect.left() + 4.0, y),
        Vec2::new(rect.width() - 8.0, 24.0),
    );
    let resp4 = ui.interact(item4, ui.scope_id().with("opt-no-merges"), Sense::click());
    if resp4.hovered() {
        ui.painter()
            .rect_filled(item4, egui::CornerRadius::same(4), p.hover);
    }
    if no_merges {
        ui.painter().text(
            Pos2::new(item4.left() + 10.0, item4.center().y),
            Align2::LEFT_CENTER,
            "✓",
            FontId::proportional(11.0),
            p.accent,
        );
    }
    ui.painter().text(
        Pos2::new(item4.left() + 26.0, item4.center().y),
        Align2::LEFT_CENTER,
        if zh {
            "不显示合并提交 (--no-merges)"
        } else {
            "No Merges"
        },
        FontId::proportional(12.0),
        p.text,
    );
    if resp4.clicked() {
        model.projects[project_idx].log_query.no_merges = !no_merges;
        model.queue(Command::LogFilterChanged);
    }

    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count)
        || resp1.clicked()
        || resp2.clicked()
        || resp3.clicked()
        || resp4.clicked()
    {
        rt.menu = None;
    } else {
        rt.menu = Some(menu);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_log_path_filter_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    _initial_path: &str,
) {
    let p = theme.palette;
    let zh = model.settings.language.is_zh();
    let rect = popover_rect(rt, screen, menu.anchor, 260.0, 84.0);
    menu_frame(ui, rect, theme);

    let mut current_path = model.projects[project_idx].log_query.path.clone();
    let input_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 10.0, rect.top() + 10.0),
        Vec2::new(rect.width() - 20.0, 26.0),
    );
    ui.painter()
        .rect_filled(input_rect, egui::CornerRadius::same(4), p.panel2);
    ui.painter().rect_stroke(
        input_rect,
        egui::CornerRadius::same(4),
        Stroke::new(1.0, p.border_soft),
        StrokeKind::Inside,
    );

    let resp = ui.put(
        input_rect.shrink2(Vec2::new(6.0, 2.0)),
        TextEdit::singleline(&mut current_path)
            .frame(egui::Frame::NONE)
            .align(egui::Align2::LEFT_CENTER)
            .hint_text(if zh {
                "路径，如 src/main.rs"
            } else {
                "e.g. src/main.rs"
            })
            .font(FontId::monospace(11.0)),
    );
    if resp.changed() {
        model.projects[project_idx].log_query.path = current_path.clone();
    }
    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));

    let apply_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 10.0, input_rect.bottom() + 8.0),
        Vec2::new(60.0, 24.0),
    );
    let apply_clicked = primary_button(
        ui,
        apply_rect,
        if zh { "应用" } else { "Apply" },
        None,
        true,
        theme,
    )
    .clicked()
        || enter;
    if apply_clicked {
        model.queue(Command::LogFilterChanged);
    }

    let clear_rect = Rect::from_min_size(
        Pos2::new(apply_rect.right() + 8.0, input_rect.bottom() + 8.0),
        Vec2::new(60.0, 24.0),
    );
    if secondary_button(
        ui,
        clear_rect,
        if zh { "清除" } else { "Clear" },
        None,
        theme,
    )
    .clicked()
    {
        model.projects[project_idx].log_query.path.clear();
        model.queue(Command::LogFilterChanged);
    }

    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) || apply_clicked {
        rt.menu = None;
    } else {
        rt.menu = Some(menu);
    }
}
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_commit_details_file_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    payload: &str,
) {
    let Some((commit_id, file_path)) = payload.split_once('\u{1}') else {
        return;
    };
    let commit_id = commit_id.to_string();
    let file_path = file_path.to_string();
    let zh = model.settings.language.is_zh();
    let items = [
        (if zh { "查看差异" } else { "Show Diff" }, 0),
        (
            if zh {
                "与本地比较"
            } else {
                "Compare with Local"
            },
            1,
        ),
        (
            if zh {
                "获取此版本"
            } else {
                "Get from Revision"
            },
            2,
        ),
        (
            if zh {
                "复制文件补丁"
            } else {
                "Copy File Patch"
            },
            3,
        ),
        (
            if zh {
                "创建文件补丁…"
            } else {
                "Create File Patch…"
            },
            4,
        ),
    ];
    let height = items.len() as f32 * MENU_ROW_H + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 220.0, height);
    menu_frame(ui, rect, theme);
    let mut action: Option<u8> = None;
    let row_start = rect.top() + 4.0;
    for (i, (label, code)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, row_start + i as f32 * MENU_ROW_H),
            Vec2::new(rect.width() - 8.0, MENU_ROW_H),
        );
        if menu_row(
            ui,
            theme,
            row,
            ui.scope_id().with(("cd_f_ctx", i)),
            label,
            false,
            true,
        ) {
            action = Some(*code);
        }
    }
    let mut close = false;
    if let Some(code) = action {
        match code {
            0 => {
                model.queue(Command::OpenCommitDiff {
                    commit: commit_id,
                    path: file_path,
                });
            }
            1 => {
                model.queue(Command::OpenCompareWithLocalDiff {
                    commit: commit_id,
                    path: file_path,
                });
            }
            2 => model.queue(Command::GetFileFromRevision {
                path: file_path,
                revision: commit_id,
            }),
            3 => {
                let root = model.projects[project_idx].active_root().to_path_buf();
                match lazecode_git::commit_file_diff(&root, &commit_id, &file_path) {
                    Ok(diff) => {
                        let patch = lazecode_text::unified_patch(&diff.path, &diff.diff);
                        ui.ctx().copy_text(patch);
                        model.toast(
                            if zh {
                                "已将文件补丁复制到剪贴板"
                            } else {
                                "File patch copied to clipboard"
                            },
                            ToastKind::Info,
                        );
                    }
                    Err(err) => model.toast(err.to_string(), ToastKind::Error),
                }
            }
            4 => {
                let root = model.projects[project_idx].active_root().to_path_buf();
                match lazecode_git::commit_file_diff(&root, &commit_id, &file_path) {
                    Ok(diff) => {
                        let patch = lazecode_text::unified_patch(&diff.path, &diff.diff);
                        let short = &commit_id[..commit_id.len().min(8)];
                        let name = std::path::Path::new(&file_path)
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("file");
                        let default_name = format!("{short}-{name}.patch");
                        let target = dirs::desktop_dir()
                            .unwrap_or_else(|| root.clone())
                            .join(&default_name);
                        match std::fs::write(&target, patch) {
                            Ok(()) => model.toast(
                                if zh {
                                    format!("补丁已保存至 {}", target.display())
                                } else {
                                    format!("Patch saved to {}", target.display())
                                },
                                ToastKind::Success,
                            ),
                            Err(err) => model.toast(err.to_string(), ToastKind::Error),
                        }
                    }
                    Err(err) => model.toast(err.to_string(), ToastKind::Error),
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
