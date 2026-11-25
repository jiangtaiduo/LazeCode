//! Stash and shelf context menus.

use super::prelude::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_stash_entry_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    payload: Option<&str>,
) {
    let p = theme.palette;
    let stash = payload
        .map(str::to_string)
        .or_else(|| rt.stash_selected.clone());
    let message = stash
        .as_ref()
        .and_then(|stash| {
            rt.stash_entries
                .iter()
                .find(|entry| &entry.name == stash)
                .map(|entry| entry.message())
        })
        .unwrap_or_else(|| stash.clone().unwrap_or_default());
    // 显示差异需要一个文件：优先用该贮藏下当前选中的文件，否则第一条。
    let diff_target = stash.as_ref().and_then(|stash| {
        let paths = model.projects.get_mut(project_idx)?.stash_paths_for(stash);
        rt.stash_file_sel
            .as_ref()
            .filter(|path| paths.iter().any(|known| known == *path))
            .cloned()
            .or_else(|| paths.first().cloned())
    });
    let branch = model
        .projects
        .get(project_idx)
        .map(|project| project.branch())
        .unwrap_or_default();

    // (label, code, danger, enabled)
    let items: [(&str, u8, bool, bool); 7] = [
        (crate::tr!(model, "Pop", "弹出"), 0, false, stash.is_some()),
        (
            crate::tr!(model, "Apply", "应用"),
            1,
            false,
            stash.is_some(),
        ),
        (
            crate::tr!(model, "Unstash…", "Unstash…"),
            2,
            false,
            stash.is_some(),
        ),
        (crate::tr!(model, "Drop", "丢弃"), 3, true, stash.is_some()),
        (crate::tr!(model, "Clear", "清空"), 4, true, true),
        (
            crate::tr!(model, "Show Diff", "显示差异"),
            5,
            false,
            diff_target.is_some(),
        ),
        (
            crate::tr!(model, "Show Diff in a New Tab", "在新标签页显示差异"),
            6,
            false,
            diff_target.is_some(),
        ),
    ];
    let sep_after = 4usize;
    let height = items.len() as f32 * MENU_ROW_H + 12.0;
    let rect = popover_rect(rt, screen, menu.anchor, 230.0, height);
    menu_frame(ui, rect, theme);
    let mut picked: Option<u8> = None;
    let mut y = rect.top() + 4.0;
    for (i, (label, code, danger, enabled)) in items.iter().enumerate() {
        if i == sep_after {
            ui.painter().hline(
                (rect.left() + 8.0)..=(rect.right() - 8.0),
                y + 3.0,
                Stroke::new(1.0, p.border_soft),
            );
            y += 6.0;
        }
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, y),
            Vec2::new(rect.width() - 8.0, MENU_ROW_H),
        );
        if menu_row(
            ui,
            theme,
            row,
            ui.scope_id().with(("stash-entry", i)),
            label,
            *danger,
            *enabled,
        ) {
            picked = Some(*code);
        }
        y += MENU_ROW_H;
    }
    let mut close = false;
    match picked {
        Some(0) => {
            if let Some(stash) = stash {
                model.queue(Command::StashPop(stash));
            }
            close = true;
        }
        Some(1) => {
            if let Some(stash) = stash {
                model.queue(Command::StashApply(stash));
            }
            close = true;
        }
        Some(2) => {
            if let Some(stash) = stash {
                rt.dialog = crate::runtime::DialogState::Unstash {
                    stash,
                    message,
                    branch,
                    as_new_branch: String::new(),
                    pop: false,
                    reinstate_index: false,
                    error: None,
                };
            }
            close = true;
        }
        Some(3) => {
            if let Some(stash) = stash {
                model.queue(Command::StashDrop(stash));
            }
            close = true;
        }
        Some(4) => {
            model.queue(Command::StashClear);
            close = true;
        }
        Some(5) => {
            if let (Some(stash), Some(path)) = (stash, diff_target) {
                model.queue(Command::OpenStashDiff { stash, path });
            }
            close = true;
        }
        Some(6) => {
            if let (Some(stash), Some(path)) = (stash, diff_target) {
                model.queue(Command::OpenStashDiffNewTab { stash, path });
            }
            close = true;
        }
        _ => {}
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

/// 贮藏文件 / 目录右键菜单（对齐参考实现）：
/// Show Diff · Show Diff in a New Tab · Compare Before with Local ·
/// Edit Source · Open Repository Version · Cherry-Pick Selected Changes。
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_stash_file_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    payload: &str,
) {
    let mut segments = payload.split('\u{1}');
    let Some(stash) = segments.next().filter(|stash| !stash.is_empty()) else {
        return;
    };
    let kind = segments.next().unwrap_or("f");
    let path = segments.next().unwrap_or("").to_string();
    let stash = stash.to_string();

    // 目录 / 树根展开为该范围内的文件清单；文件就是它自己。
    let mut files: Vec<String> = Vec::new();
    if let Some(details) = model
        .projects
        .get_mut(project_idx)
        .and_then(|project| project.stash_details_for(&stash))
    {
        match kind {
            "t" => files.extend(details.tracked.iter().cloned()),
            "u" => files.extend(details.untracked.iter().cloned()),
            "d" => {
                let prefix = if path.is_empty() {
                    String::new()
                } else {
                    format!("{path}/")
                };
                files.extend(
                    details
                        .tracked
                        .iter()
                        .chain(details.untracked.iter())
                        .filter(|file| !prefix.is_empty() && file.starts_with(&prefix))
                        .cloned(),
                );
            }
            _ => {}
        }
    }
    if kind == "f" && !path.is_empty() {
        files = vec![path.clone()];
    }
    let target = files.first().cloned();
    let worktree_target = target.as_ref().and_then(|target| {
        model
            .active_project()
            .map(|project| project.active_root().join(target))
    });
    let target_exists = worktree_target.as_ref().is_some_and(|path| path.is_file());

    // (label, code, danger, enabled)
    let items: [(&str, u8, bool, bool); 6] = [
        (
            crate::tr!(model, "Show Diff", "显示差异"),
            0,
            false,
            target.is_some(),
        ),
        (
            crate::tr!(model, "Show Diff in a New Tab", "在新标签页显示差异"),
            1,
            false,
            target.is_some(),
        ),
        (
            crate::tr!(model, "Compare Before with Local", "与本地的前一版本比较"),
            2,
            false,
            target.is_some(),
        ),
        (
            crate::tr!(model, "Edit Source", "编辑源文件"),
            3,
            false,
            target_exists,
        ),
        (
            crate::tr!(model, "Open Repository Version", "打开仓库版本"),
            4,
            false,
            target.is_some(),
        ),
        (
            crate::tr!(model, "Cherry-Pick Selected Changes", "拣选选中更改"),
            5,
            false,
            !files.is_empty(),
        ),
    ];
    let height = items.len() as f32 * MENU_ROW_H + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 250.0, height);
    menu_frame(ui, rect, theme);
    let mut picked: Option<u8> = None;
    for (i, (label, code, danger, enabled)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, rect.top() + 4.0 + i as f32 * MENU_ROW_H),
            Vec2::new(rect.width() - 8.0, MENU_ROW_H),
        );
        if menu_row(
            ui,
            theme,
            row,
            ui.scope_id().with(("stash-file", i)),
            label,
            *danger,
            *enabled,
        ) {
            picked = Some(*code);
        }
    }
    let mut close = false;
    match picked {
        Some(0) => {
            if let Some(path) = target {
                model.queue(Command::OpenStashDiff { stash, path });
            }
            close = true;
        }
        Some(1) => {
            if let Some(path) = target {
                model.queue(Command::OpenStashDiffNewTab { stash, path });
            }
            close = true;
        }
        Some(2) => {
            if let Some(path) = target {
                model.queue(Command::OpenStashBeforeLocalDiff { stash, path });
            }
            close = true;
        }
        Some(3) => {
            if let Some(path) = worktree_target {
                model.queue(Command::OpenFileAt {
                    path,
                    line: 0,
                    character: 0,
                });
            }
            close = true;
        }
        Some(4) => {
            if let Some(path) = target {
                model.queue(Command::OpenRepositoryVersion { path });
            }
            close = true;
        }
        Some(5) => {
            if !files.is_empty() {
                model.queue(Command::CherryPickStashPaths {
                    stash,
                    paths: files,
                });
            }
            close = true;
        }
        _ => {}
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

/// 架藏右键菜单（稳定版 Shelf 页签）：恢复 / 补丁 / 重命名 / 删除。
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_shelf_context_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    payload: &str,
) {
    let mut segments = payload.split('\u{1}');
    let Some(name) = segments.next().filter(|name| !name.is_empty()) else {
        return;
    };
    let name = name.to_string();
    let deleted = segments.next() == Some("1");
    let paths: Vec<String> = segments
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect();
    let _ = project_idx;

    // (label, code, danger, enabled)
    let mut items: Vec<(String, u8, bool, bool)> = Vec::new();
    if deleted {
        items.push((
            crate::tr!(model, "Restore", "恢复").to_string(),
            0,
            false,
            true,
        ));
        items.push((
            crate::tr!(model, "Delete…", "删除…").to_string(),
            3,
            true,
            true,
        ));
    } else {
        items.push((
            crate::tr!(model, "Unshelve…", "恢复贮藏…").to_string(),
            1,
            false,
            true,
        ));
        items.push((
            crate::tr!(model, "Copy as Patch to Clipboard", "以补丁复制到剪贴板").to_string(),
            2,
            false,
            true,
        ));
        items.push((
            crate::tr!(model, "Rename…", "重命名…").to_string(),
            4,
            false,
            paths.is_empty(),
        ));
        items.push((
            crate::tr!(model, "Delete…", "删除…").to_string(),
            3,
            true,
            true,
        ));
    }
    let height = items.len() as f32 * MENU_ROW_H + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 260.0, height);
    menu_frame(ui, rect, theme);
    let mut picked: Option<u8> = None;
    for (i, (label, code, danger, enabled)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, rect.top() + 4.0 + i as f32 * MENU_ROW_H),
            Vec2::new(rect.width() - 8.0, MENU_ROW_H),
        );
        if menu_row(
            ui,
            theme,
            row,
            ui.scope_id().with(("shelf-ctx", i)),
            label,
            *danger,
            *enabled,
        ) {
            picked = Some(*code);
        }
    }
    let mut close = false;
    match picked {
        // Restore（最近删除 → 恢复）
        Some(0) => {
            model.queue(Command::ShelfRestoreDeleted(name.clone()));
            rt.shelf_loaded = None;
            close = true;
        }
        // Unshelve…
        Some(1) => {
            {
                let files: Vec<(String, bool)> =
                    paths.iter().map(|path| (path.clone(), true)).collect();
                rt.dialog = crate::runtime::DialogState::Unshelve {
                    name: name.clone(),
                    files,
                    target: None,
                    new_name: name.clone(),
                    error: None,
                };
            }
            close = true;
        }
        // Copy as Patch（补丁读取是本地小文件，同步可接受）
        Some(2) => {
            if let Some(project) = model.projects.get(model.active_project.unwrap_or(0)) {
                match lazecode_git::shelf_patch(project.active_root(), &name) {
                    Ok(patch) => {
                        ui.ctx().copy_text(patch);
                        let zh = model.settings.language.is_zh();
                        model.toast(
                            if zh {
                                "补丁已复制到剪贴板"
                            } else {
                                "Patch copied to clipboard"
                            },
                            ToastKind::Success,
                        );
                    }
                    Err(err) => model.toast(err.to_string(), ToastKind::Error),
                }
            }
            close = true;
        }
        // Delete…（确认）
        Some(3) => {
            rt.dialog = crate::runtime::DialogState::ConfirmDeleteShelf {
                name: name.clone(),
                purge: deleted,
            };
            close = true;
        }
        // Rename…
        Some(4) => {
            rt.dialog = crate::runtime::DialogState::RenameShelf {
                from: name.clone(),
                value: name.clone(),
                error: None,
            };
            close = true;
        }
        _ => {}
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}
