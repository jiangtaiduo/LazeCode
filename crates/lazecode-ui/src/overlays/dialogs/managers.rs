//! Manager windows: stash and shelf managers, file history, Git tools,
//! clone, squash and interactive rebase.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, TextEdit, Ui, Vec2};
use lazecode_core::{AppModel, Command, ToastKind};

use crate::runtime::{DialogState, UiRuntime};
use crate::theme::Theme;
use crate::widgets::{primary_button, secondary_button, text_clipped};

use super::frame;

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_stash_manager_dialog(
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
            let root = model.projects[active].active_root().to_path_buf();
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
        Stroke::new(1.0, p.border_soft),
    );
    ui.painter().text(
        Pos2::new(header.left() + 16.0, header.center().y),
        Align2::LEFT_CENTER,
        if zh { "贮藏管理" } else { "Stash Manager" },
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
    crate::app_ui::region(ui, list_area, "stash-mgr-list", |ui| {
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
pub(super) fn draw_shelf_manager_dialog(
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
            let root = model.projects[active].active_root().to_path_buf();
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
        Stroke::new(1.0, p.border_soft),
    );
    ui.painter().text(
        Pos2::new(header.left() + 16.0, header.center().y),
        Align2::LEFT_CENTER,
        if zh {
            "架藏管理"
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
    crate::app_ui::region(ui, list_area, "shelf-mgr-list", |ui| {
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
                let root = model.projects[active].active_root().to_path_buf();
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
                purge: false,
            };
            *close = true;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_file_history_dialog(
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
            let root = model.projects[active].active_root().to_path_buf();
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
        Stroke::new(1.0, p.border_soft),
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
    crate::app_ui::region(ui, list_area, "file-hist-list", |ui| {
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
pub(super) fn draw_git_tools_dialog(
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
            let root = model.projects[active].active_root().to_path_buf();
            if let Ok(subs) = lazecode_git::submodule_list(&root) {
                *submodules = subs;
            }
        }
    }
    if !loaded.1 && *tab == crate::runtime::GitToolsTab::Lfs {
        loaded.1 = true;
        if let Some(active) = model.active_project {
            let root = model.projects[active].active_root().to_path_buf();
            *lfs_status = Some(lazecode_git::lfs_status(&root));
        }
    }

    let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 44.0));
    ui.painter()
        .rect_filled(header, egui::CornerRadius::same(6), p.tabstrip);
    ui.painter().hline(
        header.x_range(),
        header.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    ui.painter().text(
        Pos2::new(header.left() + 16.0, header.center().y),
        Align2::LEFT_CENTER,
        if zh { "Git 工具集" } else { "Git Tools" },
        FontId::proportional(13.0),
        p.text,
    );

    let mut tx = header.left() + 220.0;
    for (t, label) in [
        (
            crate::runtime::GitToolsTab::Submodules,
            if zh { "子模块" } else { "Submodules" },
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
            crate::app_ui::region(ui, body, "submodules-list", |ui| {
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
            crate::app_ui::region(ui, body, "lfs-view", |ui| {
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
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_clone_repo_dialog(
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
            "克隆仓库"
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
            .align(egui::Align2::LEFT_CENTER)
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
            .align(egui::Align2::LEFT_CENTER)
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
pub(super) fn draw_squash_dialog(
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
        if zh { "压缩提交" } else { "Squash Commits" },
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

pub(super) fn draw_interactive_rebase_dialog(
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
        Stroke::new(1.0, p.border_soft),
    );
    ui.painter().text(
        Pos2::new(header.left() + 16.0, header.center().y),
        Align2::LEFT_CENTER,
        format!(
            "{} onto {}",
            if zh {
                "交互式变基"
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
    crate::app_ui::region(ui, list_area, "rebase-todo-list", |ui| {
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
        !todos.is_empty(),
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
