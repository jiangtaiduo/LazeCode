use std::time::Duration;

use egui::{
    Align2, Color32, FontId, Key, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2,
};
use lazecode_core::{AppModel, Command, TabKind, ToastKind};

use crate::runtime::{DialogState, UiRuntime};
use crate::theme::Theme;
use crate::widgets::{checkbox, primary_button, secondary_button, text_clipped};

mod conflict_tool;
mod menus;

use conflict_tool::draw_conflict_tool;
use menus::draw_menu;

/// Shared constructor for the Push dialog so every entry point sees the
/// project's remotes and pending commits.
fn remotes_width(remotes: &[lazecode_git::RemoteInfo], available: f32) -> f32 {
    let longest = remotes
        .iter()
        .map(|remote| remote.name.chars().count())
        .max()
        .unwrap_or(8);
    (longest as f32 * 7.5 + 34.0).clamp(90.0, available)
}

pub fn push_dialog_state(
    project: &lazecode_core::ProjectState,
    remote: String,
    branch: String,
) -> DialogState {
    let tracking = project
        .branches
        .iter()
        .find(|candidate| candidate.name == branch)
        .and_then(|candidate| candidate.tracking.clone());
    let pending =
        lazecode_git::pending_commits(project.active_root(), tracking.as_deref(), &branch, 200)
            .unwrap_or_default();
    DialogState::Push {
        remote,
        branch,
        set_upstream: tracking.is_none(),
        force: false,
        push_tags: false,
        remotes: project.remotes.clone(),
        pending,
        status: None,
    }
}

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme) {
    let screen = ui.max_rect();

    // Keep the updater worker polled even while its window is closed so an
    // automatic check can surface as the title-bar badge/toast.
    let update_changed = rt.update.tick(model);
    if rt.update.job.is_some() {
        ui.ctx().request_repaint_after(Duration::from_millis(150));
    } else if let Some(delay) = rt.update.next_wakeup() {
        ui.ctx()
            .request_repaint_after(delay.max(Duration::from_millis(16)));
    } else if update_changed {
        ui.ctx().request_repaint();
    }
    draw_menu(ui, model, rt, theme, screen);

    if !matches!(rt.dialog, DialogState::None) {
        draw_dialog(ui, model, rt, theme, screen);
    }
    if rt.update_open {
        crate::update_ui::draw(ui, model, rt, theme, screen);
    }
}

fn popover_rect(rt: &mut UiRuntime, screen: Rect, anchor: Rect, width: f32, height: f32) -> Rect {
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
    let rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, height));
    // Register the surface for the shell's wheel interception (next frame).
    rt.menu_hit_rects.push(rect);
    rect
}

/// Stable `.py-dialog-window` material: opaque dialog surface, radius 12,
/// hairline border, heavy drop shadow.
pub(crate) fn frame(ui: &Ui, rect: Rect, theme: &Theme) {
    let p = theme.palette;
    dialog_shadow(ui.painter(), rect, theme, 12.0, 10.0, 32.0, 72);
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(12), p.dialog);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(12),
        Stroke::new(1.0, p.menu_border),
        StrokeKind::Inside,
    );
}

/// Stable `.branch-ctx-menu` material: `--menu-bg` glass at 0.96, radius 8,
/// menu shadow. (Menus and dialogs share `frame()` call sites; menus use
/// this lighter material instead.)
pub(crate) fn menu_frame(ui: &Ui, rect: Rect, theme: &Theme) {
    let p = theme.palette;
    dialog_shadow(ui.painter(), rect, theme, 8.0, 6.0, 20.0, 56);
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(8), p.menu_bg);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(8),
        Stroke::new(1.0, p.menu_border),
        StrokeKind::Inside,
    );
}

/// Soft drop shadow built from a few concentric rounded rects. epaint's
/// `blur_width` feathers only the outer edge, so a single large blurred rect
/// leaves a solid dark ring; stacking low-alpha layers gives a real falloff
/// instead. Light mode uses roughly half the alpha.
pub(crate) fn dialog_shadow(
    painter: &egui::Painter,
    rect: Rect,
    theme: &Theme,
    radius: f32,
    dy: f32,
    spread: f32,
    alpha: u8,
) {
    let base = if theme.dark {
        alpha
    } else {
        (alpha as f32 * 0.65).round() as u8
    };
    if base == 0 {
        return;
    }
    // (expand, downward offset, share of base alpha); innermost first.
    let layers = [
        (0.00, 0.00, 0.32),
        (0.15, 0.18, 0.24),
        (0.32, 0.38, 0.20),
        (0.52, 0.60, 0.15),
        (0.75, 0.85, 0.09),
    ];
    for (expand_frac, offset_frac, alpha_frac) in layers {
        let expand = spread * expand_frac;
        let offset = dy * offset_frac;
        let layer_alpha = (base as f32 * alpha_frac).round() as u8;
        if layer_alpha == 0 {
            continue;
        }
        let shape = egui::epaint::RectShape {
            rect: rect.translate(egui::vec2(0.0, offset)).expand(expand),
            corner_radius: egui::CornerRadius::same((radius + expand).round() as u8),
            fill: Color32::from_black_alpha(layer_alpha),
            stroke: Stroke::NONE,
            stroke_kind: egui::StrokeKind::Inside,
            round_to_pixels: None,
            blur_width: 2.0,
            brush: None,
            angle: 0.0,
        };
        painter.add(egui::Shape::Rect(shape));
    }
}

/// Stable context-menu row height (12px text + 2×5px padding).
pub(crate) const MENU_ROW_H: f32 = 24.0;

/// One stable CtxMenuShell row: 12px text, radius 6, macOS hover = solid
/// accent fill + white text; danger rows use `--git-del`. Returns clicked.
pub(crate) fn menu_row(
    ui: &mut Ui,
    theme: &Theme,
    row: Rect,
    id_salt: egui::Id,
    label: &str,
    danger: bool,
    enabled: bool,
) -> bool {
    let p = theme.palette;
    let response = ui.interact(
        row,
        id_salt,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let hovered = response.hovered() && enabled;
    if hovered {
        ui.painter()
            .rect_filled(row, egui::CornerRadius::same(6), p.menu_item_hover);
    }
    let color = if !enabled {
        p.text3
    } else if hovered {
        p.menu_item_text
    } else if danger {
        p.rose
    } else {
        p.text
    };
    ui.painter().text(
        Pos2::new(row.left() + 10.0, row.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(12.0),
        color,
    );
    response.clicked() && enabled
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

/// Branches currently checked out in any worktree of `project`, mapped to the
/// worktree name. Includes registrations that are not attached to a pane yet.
fn used_worktree_branches(
    project: &lazecode_core::ProjectState,
) -> std::collections::HashMap<String, String> {
    let mut used = std::collections::HashMap::new();
    let main_branch = project.branch();
    if !main_branch.is_empty() && main_branch != "no branch" {
        used.insert(main_branch, "main".to_string());
    }
    for worktree in &project.worktrees {
        let branch = worktree.branch();
        if !branch.is_empty() && branch != "no branch" && !branch.contains("(detached)") {
            used.insert(branch, worktree.name.clone());
        }
    }
    if let Ok(worktrees) = lazecode_git::list_worktrees(&project.root) {
        for worktree in worktrees {
            if let Some(branch) = worktree.branch {
                used.insert(branch, worktree.name);
            }
        }
    }
    used
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

/// Rename only accepts a single path component. Allowing `../other` here
/// would turn a rename into a silent move outside the current directory.
fn is_plain_name(name: &str) -> bool {
    let mut components = std::path::Path::new(name).components();
    matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none()
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
    model.settle_single_files_workspace();
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

/// Open buffers with unsaved edits covered by `paths` (a directory covers its
/// children). The trash recovers the file on disk, but not the in-memory
/// edits, so the delete confirmation has to name them.
fn dirty_paths_under(model: &AppModel, paths: &[std::path::PathBuf]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for project in &model.projects {
        for tab in &project.tabs {
            let TabKind::File(file) = &tab.kind else {
                continue;
            };
            if !file.doc.dirty {
                continue;
            }
            let covered = paths.iter().any(|path| {
                file.doc.path == *path || (path.is_dir() && file.doc.path.starts_with(path))
            });
            if covered {
                let name = file.doc.title();
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
    }
    names
}

/// Amber banner above the confirmation buttons: deleting files must never
/// silently drop edits that only exist in memory.
fn paint_unsaved_delete_warning(
    ui: &Ui,
    model: &AppModel,
    theme: &Theme,
    rect: Rect,
    paths: &[std::path::PathBuf],
) {
    let dirty = dirty_paths_under(model, paths);
    if dirty.is_empty() {
        return;
    }
    let zh = model.settings.language.is_zh();
    let mut list = dirty
        .iter()
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .join(if zh { "、" } else { ", " });
    if dirty.len() > 3 {
        if zh {
            list.push_str(&format!(" 等 {} 个文件", dirty.len()));
        } else {
            list.push_str(&format!(" (+{})", dirty.len() - 3));
        }
    }
    let text = if zh {
        format!("有未保存的修改，删除后将无法恢复：{list}")
    } else {
        format!("Unsaved edits will be lost: {list}")
    };
    let band = Rect::from_min_max(
        Pos2::new(rect.left() + 22.0, rect.bottom() - 80.0),
        Pos2::new(rect.right() - 22.0, rect.bottom() - 56.0),
    );
    ui.painter().rect_filled(
        band,
        egui::CornerRadius::same(4),
        theme.palette.amber.gamma_multiply(0.14),
    );
    text_clipped(
        ui.painter(),
        band.shrink2(Vec2::new(8.0, 4.0)),
        &text,
        FontId::proportional(10.5),
        theme.palette.amber,
        Align2::LEFT_CENTER,
    );
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
    let unstash_text = if let DialogState::Unstash { message, .. } = &dialog {
        let name = if message.trim().is_empty() {
            crate::tr!(model, "stash", "贮藏").to_string()
        } else {
            message.clone()
        };
        Some((
            crate::tr!(
                model,
                "Unstash Changes in {name}…",
                "Unstash Changes in {name}…"
            )
            .replace("{name}", &name),
            crate::tr!(
                model,
                "Apply the stashed changes to the current branch or a new one.",
                "将贮藏的更改应用到当前分支或新分支。"
            )
            .to_string(),
        ))
    } else {
        None
    };
    let (title, subtitle, width, height) = match &dialog {
        DialogState::NewBranch { .. } => (
            crate::tr!(model, "New branch", "新建分支"),
            crate::tr!(model, "Create a branch at the selected start point. Checkout is optional.", "从选定起点创建分支，可选创建后检出。"),
            460.0,
            290.0,
        ),
        DialogState::NewWorktree { .. } => (
            crate::tr!(model, "New worktree", "新建工作树"),
            crate::tr!(
                model,
                "Creates a linked worktree. One branch can be checked out in only one worktree; create a new branch or pick an unused one.",
                "创建关联的工作树。同一分支只能在一个工作树中检出；可新建分支或选择未使用的分支。"
            ),
            520.0,
            370.0,
        ),
        DialogState::CheckoutRevision { .. } => (
            crate::tr!(model, "Checkout tag or revision", "检出标签或修订版本"),
            crate::tr!(
                model,
                "Enter a branch, tag, or commit hash to check out.",
                "输入要检出的分支、标签或提交哈希。"
            ),
            460.0,
            230.0,
        ),
        DialogState::RemoveWorktree { dirty: false, .. } => (
            crate::tr!(model, "Remove worktree", "删除工作树"),
            crate::tr!(
                model,
                "The directories of the worktree and its consoles are removed.",
                "将删除工作树目录及其控制台。"
            ),
            470.0,
            210.0,
        ),
        DialogState::RemoveWorktree { dirty: true, .. } => (
            crate::tr!(model, "Remove worktree with changes", "删除有更改的工作树"),
            crate::tr!(
                model,
                "This worktree has uncommitted changes. They will be permanently discarded.",
                "此工作树有未提交的更改，删除后将永久丢失。"
            ),
            470.0,
            230.0,
        ),
        DialogState::DeleteBranch { remote, force, .. } => {
            if *remote {
                (
                    crate::tr!(model, "Delete remote branch", "删除远程分支"),
                    crate::tr!(model, "The remote branch is deleted on the remote. The local branch is not touched.", "将在远程删除该分支，不会影响本地分支。"),
                    460.0,
                    220.0,
                )
            } else if *force {
                (
                    crate::tr!(model, "Force delete branch", "强制删除分支"),
                    crate::tr!(model, "This branch is not fully merged. Confirm explicitly to delete its reference.", "该分支尚未合并。确认后将强制删除其引用。"),
                    480.0,
                    230.0,
                )
            } else {
                (
                    crate::tr!(model, "Delete branch", "删除分支"),
                    crate::tr!(model, "The branch and its reflog entry are removed. Commits stay reachable until garbage collection.", "将删除该分支及其 reflog 记录；提交在垃圾回收前仍可访问。"),
                    440.0,
                    210.0,
                )
            }
        }
        DialogState::RenameBranch { .. } => (
            crate::tr!(model, "Rename branch", "重命名分支"),
            crate::tr!(model, "The ref name of a local branch is rewritten in place.", "直接重命名本地分支的引用名。"),
            460.0,
            220.0,
        ),
        DialogState::Push { .. } => (
            crate::tr!(model, "Push", "推送"),
            crate::tr!(model, "Publish the branch and review commits before pushing. Force uses force-with-lease semantics.", "推送分支并在推送前检查提交；Force 使用 force-with-lease 语义。"),
            570.0,
            500.0,
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
        DialogState::ConfirmTerminalClose { action, .. } => (
            if matches!(action, crate::runtime::TerminalCloseAction::Project(_)) {
                crate::tr!(model, "Close project", "关闭项目")
            } else {
                crate::tr!(model, "Close terminal", "关闭控制台")
            },
            crate::tr!(
                model,
                "Commands are still running in this console. Closing it will terminate them.",
                "控制台里还有命令在运行，关闭会终止这些进程。"
            ),
            520.0,
            230.0,
        ),
        DialogState::Quit {
            restart_update, ..
        } => (
            if *restart_update {
                crate::tr!(model, "Restart to finish updating?", "重启以完成更新？")
            } else {
                crate::tr!(model, "Quit LazeCode?", "退出 LazeCode？")
            },
            if *restart_update {
                crate::tr!(model, "The new version is installed. Restarting closes all windows; save any open files first.", "新版本已安装。重启会关闭所有窗口，请先保存打开的文件。")
            } else {
                crate::tr!(model, "Tabs with unsaved changes will lose their edits unless you cancel and save them first.", "未保存的标签页将丢失更改，请取消并先保存。")
            },
            460.0,
            240.0,
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
        DialogState::RenameSymbol { .. } => (
            crate::tr!(model, "Rename symbol", "重命名符号"),
            crate::tr!(
                model,
                "Rename this symbol and every reference through the language server.",
                "通过语言服务器重命名该符号及其全部引用。"
            ),
            440.0,
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
        DialogState::GetFileFromRevision { .. } => (
            crate::tr!(model, "Get from revision", "从版本获取"),
            crate::tr!(
                model,
                "Restore the path from a branch, tag, or commit hash.",
                "从分支、标签或提交哈希恢复该路径。"
            ),
            500.0,
            230.0,
        ),
        DialogState::ExtractChanges { .. } => (
            crate::tr!(
                model,
                "Extract selected changes to a separate commit",
                "提取选中更改到独立提交"
            ),
            crate::tr!(
                model,
                "Commit only these changes; everything else stays uncommitted.",
                "仅提交这些更改，其他更改保持未提交。"
            ),
            500.0,
            290.0,
        ),
        DialogState::CherryPickChanges { .. } => (
            crate::tr!(model, "Cherry-pick selected changes", "拣选选中更改"),
            crate::tr!(
                model,
                "Move the selected working-tree changes onto the target branch.",
                "将选中的工作区更改移动到目标分支。"
            ),
            500.0,
            260.0,
        ),
        DialogState::Unstash { .. } => ("", "", 500.0, 360.0),
        _ => return,
    };
    let (title, subtitle) = match unstash_text {
        Some((title, subtitle)) => (title, subtitle),
        None => (title.to_string(), subtitle.to_string()),
    };
    let title = title.as_str();
    let subtitle = subtitle.as_str();
    let subtitle = match &dialog {
        DialogState::ConfirmTerminalClose {
            action, commands, ..
        } => {
            let names = commands.join(if model.settings.language.is_zh() {
                "、"
            } else {
                ", "
            });
            let project = matches!(action, crate::runtime::TerminalCloseAction::Project(_));
            if model.settings.language.is_zh() {
                if project {
                    format!(
                        "项目控制台里还有命令在运行（{names}），关闭项目会终止这些进程。确定关闭？"
                    )
                } else {
                    format!("控制台里还有命令在运行（{names}），关闭会终止这些进程。确定关闭？")
                }
            } else if project {
                format!(
                    "Commands are still running in this project's consoles ({names}). Closing the project will terminate them. Close anyway?"
                )
            } else {
                format!(
                    "Commands are still running in the terminal ({names}). Closing it will terminate them. Close anyway?"
                )
            }
        }
        DialogState::Quit {
            commands,
            restart_update,
        } => {
            let mut lines = Vec::new();
            if *restart_update {
                lines.push(if model.settings.language.is_zh() {
                    "更新将在重启后生效。".to_string()
                } else {
                    "The installed update takes effect after the restart.".to_string()
                });
            }
            if model
                .projects
                .iter()
                .any(|project| project.tabs.iter().any(|tab| tab.is_dirty()))
            {
                lines.push(if model.settings.language.is_zh() {
                    "未保存的标签页将丢失更改。".to_string()
                } else {
                    "Tabs with unsaved changes will lose their edits.".to_string()
                });
            }
            if !commands.is_empty() {
                let names = commands.join(if model.settings.language.is_zh() {
                    "、"
                } else {
                    ", "
                });
                lines.push(if model.settings.language.is_zh() {
                    format!("控制台里仍有命令在运行（{names}），退出会终止它们。")
                } else {
                    format!(
                        "Console commands are still running ({names}); quitting will terminate them."
                    )
                });
            }
            lines.join("\n")
        }
        _ => subtitle.to_string(),
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
    paint_wrapped_text(
        ui,
        Rect::from_min_max(
            Pos2::new(rect.left() + 22.0, rect.top() + 46.0),
            Pos2::new(rect.right() - 22.0, rect.top() + 86.0),
        ),
        &subtitle,
        FontId::proportional(10.5),
        p.text3,
    );
    let mut close = false;
    let mut confirm = false;
    match &mut dialog {
        DialogState::NewBranch {
            value,
            start_point,
            checkout,
            error,
        } => {
            let start_label = start_point.as_deref().unwrap_or("HEAD");
            ui.painter().text(
                Pos2::new(rect.left() + 22.0, rect.top() + 92.0),
                Align2::LEFT_CENTER,
                format!(
                    "{} {}",
                    crate::tr!(model, "Start point:", "起点："),
                    start_label
                ),
                FontId::monospace(10.5),
                p.text3,
            );
            let input = Rect::from_min_size(
                Pos2::new(rect.left() + 22.0, rect.top() + 112.0),
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
                    .align(egui::Align2::LEFT_CENTER)
                    .hint_text("feat/new-branch")
                    .font(FontId::monospace(11.0)),
            );
            if rt.branch_create_error.is_none() {
                response.request_focus();
            }
            let check = Rect::from_center_size(
                Pos2::new(rect.left() + 30.0, rect.top() + 166.0),
                Vec2::splat(16.0),
            );
            if checkbox(ui, check, *checkout, theme).clicked() {
                *checkout = !*checkout;
            }
            ui.painter().text(
                Pos2::new(rect.left() + 44.0, rect.top() + 166.0),
                Align2::LEFT_CENTER,
                crate::tr!(model, "Checkout after create", "创建后检出"),
                FontId::proportional(10.5),
                p.text2,
            );
            if let Some(err) = error {
                ui.painter().text(
                    Pos2::new(rect.left() + 22.0, rect.top() + 196.0),
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
        DialogState::NewWorktree {
            name,
            branch,
            create_new,
            location,
            error,
        } => {
            let left = rect.left() + 22.0;
            let width = rect.width() - 44.0;
            let mut y = rect.top() + 82.0;

            let used_branches = model
                .active_project()
                .map(used_worktree_branches)
                .unwrap_or_default();

            // 1. 名称 (Name)
            ui.painter().text(
                Pos2::new(left, y),
                Align2::LEFT_TOP,
                crate::tr!(model, "Name:", "名称:"),
                FontId::proportional(11.5),
                p.text2,
            );
            y += 18.0;
            let input_name = Rect::from_min_size(Pos2::new(left, y), Vec2::new(width, 28.0));
            ui.painter()
                .rect_filled(input_name, egui::CornerRadius::same(4), p.panel2);
            ui.painter().rect_stroke(
                input_name,
                egui::CornerRadius::same(4),
                Stroke::new(1.0, p.border),
                StrokeKind::Inside,
            );
            let prev_name = name.clone();
            let resp_name = ui.put(
                input_name.shrink2(Vec2::new(8.0, 4.0)),
                TextEdit::singleline(name)
                    .frame(egui::Frame::NONE)
                    .align(egui::Align2::LEFT_CENTER)
                    .hint_text("my-feature")
                    .font(FontId::monospace(11.5)),
            );
            if resp_name.changed() {
                if let Some(proj) = model.active_project() {
                    let base_wt = format!("{}/.worktree", proj.root.display());
                    let old_def = if prev_name.is_empty() {
                        base_wt.clone()
                    } else {
                        format!("{}/{}", base_wt, prev_name)
                    };
                    if *location == old_def || location.is_empty() || location == &base_wt {
                        *location = if name.is_empty() {
                            base_wt
                        } else {
                            format!("{}/{}", base_wt, name)
                        };
                    }
                }
            }
            y += 36.0;

            // 2. 分支 (Branch): existing branch picker or a new branch name.
            ui.painter().text(
                Pos2::new(left, y),
                Align2::LEFT_TOP,
                if *create_new {
                    crate::tr!(model, "New branch name:", "新分支名称:")
                } else {
                    crate::tr!(
                        model,
                        "Branch (cannot reuse branches checked out in other worktrees):",
                        "选择分支 (不能与已有工作树冲突):"
                    )
                },
                FontId::proportional(11.5),
                p.text2,
            );
            y += 18.0;

            if *create_new {
                let input = Rect::from_min_size(Pos2::new(left, y), Vec2::new(width, 28.0));
                ui.painter()
                    .rect_filled(input, egui::CornerRadius::same(4), p.panel2);
                ui.painter().rect_stroke(
                    input,
                    egui::CornerRadius::same(4),
                    Stroke::new(1.0, p.border),
                    StrokeKind::Inside,
                );
                ui.put(
                    input.shrink2(Vec2::new(8.0, 4.0)),
                    TextEdit::singleline(branch)
                        .frame(egui::Frame::NONE)
                        .align(egui::Align2::LEFT_CENTER)
                        .hint_text("feat/my-branch")
                        .font(FontId::monospace(11.5)),
                );
            } else {
                let selected_branch_label = if branch.is_empty() {
                    crate::tr!(model, "Select branch...", "选择分支...").to_string()
                } else if let Some(wt_name) = used_branches.get(branch) {
                    format!(
                        "{branch} ({}: {wt_name})",
                        crate::tr!(model, "used in", "已检出于")
                    )
                } else {
                    branch.clone()
                };
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(Rect::from_min_size(
                            Pos2::new(left, y),
                            Vec2::new(width, 28.0),
                        ))
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                egui::ComboBox::from_id_salt("worktree-branch-picker")
                    .width(width - 8.0)
                    .selected_text(egui::RichText::new(selected_branch_label).color(p.text))
                    .show_ui(&mut child, |ui| {
                        if let Some(proj) = model.active_project() {
                            for b in proj.branches.iter().filter(|b| !b.remote) {
                                if let Some(wt_name) = used_branches.get(&b.name) {
                                    let label_text = format!(
                                        "{} ({} {})",
                                        b.name,
                                        crate::tr!(model, "checked out in", "已检出于"),
                                        wt_name
                                    );
                                    ui.add_enabled(
                                        false,
                                        egui::Button::new(
                                            egui::RichText::new(label_text).color(p.text3),
                                        ),
                                    );
                                } else {
                                    let is_sel = *branch == b.name;
                                    if ui.selectable_label(is_sel, &b.name).clicked() {
                                        *branch = b.name.clone();
                                        if name.is_empty() {
                                            *name = b.name.clone();
                                            let base_wt =
                                                format!("{}/.worktree", proj.root.display());
                                            *location = format!("{}/{}", base_wt, name);
                                        }
                                    }
                                }
                            }
                        }
                    });
            }
            y += 34.0;

            let check = Rect::from_center_size(Pos2::new(left + 8.0, y + 8.0), Vec2::splat(16.0));
            if checkbox(ui, check, *create_new, theme).clicked() {
                *create_new = !*create_new;
                if *create_new {
                    *branch = String::new();
                } else {
                    let used = used_branches.clone();
                    *branch = model
                        .active_project()
                        .and_then(|project| {
                            project
                                .branches
                                .iter()
                                .find(|branch| !branch.remote && !used.contains_key(&branch.name))
                                .map(|branch| branch.name.clone())
                        })
                        .unwrap_or_default();
                }
            }
            ui.painter().text(
                Pos2::new(left + 22.0, y + 8.0),
                Align2::LEFT_CENTER,
                crate::tr!(model, "Create a new branch", "新建分支"),
                FontId::proportional(10.5),
                p.text2,
            );
            y += 26.0;

            // 3. 保存位置 (Save Location)
            ui.painter().text(
                Pos2::new(left, y),
                Align2::LEFT_TOP,
                crate::tr!(
                    model,
                    "Location (default in .worktree/ and git-ignored):",
                    "保存位置 (默认在 .worktree 且 Git 忽略):"
                ),
                FontId::proportional(11.5),
                p.text2,
            );
            y += 18.0;
            let loc_input = Rect::from_min_size(Pos2::new(left, y), Vec2::new(width - 72.0, 28.0));
            ui.painter()
                .rect_filled(loc_input, egui::CornerRadius::same(4), p.panel2);
            ui.painter().rect_stroke(
                loc_input,
                egui::CornerRadius::same(4),
                Stroke::new(1.0, p.border),
                StrokeKind::Inside,
            );
            ui.put(
                loc_input.shrink2(Vec2::new(8.0, 4.0)),
                TextEdit::singleline(location)
                    .frame(egui::Frame::NONE)
                    .align(egui::Align2::LEFT_CENTER)
                    .font(FontId::monospace(11.0)),
            );

            let browse_btn =
                Rect::from_min_size(Pos2::new(left + width - 66.0, y), Vec2::new(66.0, 28.0));
            if secondary_button(
                ui,
                browse_btn,
                crate::tr!(model, "Browse...", "选择目录..."),
                None,
                theme,
            )
            .clicked()
            {
                let default_dir = model
                    .active_project()
                    .map(|p| p.root.join(".worktree"))
                    .unwrap_or_default();
                if let Some(folder) = rfd::FileDialog::new()
                    .set_directory(default_dir)
                    .pick_folder()
                {
                    *location = folder.display().to_string();
                }
            }
            y += 34.0;

            if let Some(err) = error {
                ui.painter().text(
                    Pos2::new(left, y),
                    Align2::LEFT_TOP,
                    err,
                    FontId::proportional(10.5),
                    p.rose,
                );
            }
            if ui.input(|input| input.key_pressed(Key::Enter)) {
                confirm = true;
            }
        }
        DialogState::CheckoutRevision { value, error } => {
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
                    .align(egui::Align2::LEFT_CENTER)
                    .hint_text("v1.0.0 or commit hash or branch")
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
        DialogState::DeleteBranch {
            name,
            force,
            remote,
            error,
        } => {
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
            if *remote {
                ui.painter().text(
                    Pos2::new(rect.left() + 22.0, rect.top() + 142.0),
                    Align2::LEFT_CENTER,
                    crate::tr!(
                        model,
                        "Remote deletion uses push --delete.",
                        "远程删除使用 push --delete。"
                    ),
                    FontId::proportional(9.5),
                    p.text3,
                );
            } else if *force {
                ui.painter().text(
                    Pos2::new(rect.left() + 22.0, rect.top() + 142.0),
                    Align2::LEFT_CENTER,
                    crate::tr!(
                        model,
                        "This bypasses the merged check.",
                        "此操作会跳过合并检查。"
                    ),
                    FontId::proportional(9.5),
                    p.rose,
                );
            }
            if let Some(err) = error {
                ui.painter().text(
                    Pos2::new(rect.left() + 22.0, rect.top() + 166.0),
                    Align2::LEFT_CENTER,
                    err,
                    FontId::proportional(9.5),
                    p.rose,
                );
            }
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
                    .align(egui::Align2::LEFT_CENTER)
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
        DialogState::RemoveWorktree {
            name,
            worktree_path,
            dirty,
            changes,
        } => {
            text_clipped(
                ui.painter(),
                Rect::from_min_max(
                    Pos2::new(rect.left() + 22.0, rect.top() + 96.0),
                    Pos2::new(rect.right() - 22.0, rect.top() + 112.0),
                ),
                name,
                FontId::monospace(11.0),
                p.amber,
                Align2::LEFT_CENTER,
            );
            text_clipped(
                ui.painter(),
                Rect::from_min_max(
                    Pos2::new(rect.left() + 22.0, rect.top() + 114.0),
                    Pos2::new(rect.right() - 22.0, rect.top() + 130.0),
                ),
                &worktree_path.display().to_string(),
                FontId::monospace(10.0),
                p.text3,
                Align2::LEFT_CENTER,
            );
            if *dirty {
                let detail = if *changes > 0 {
                    if model.settings.language.is_zh() {
                        format!("{changes} 个文件有未提交的更改，将一并丢弃")
                    } else {
                        format!(
                            "{changes} uncommitted change{} will be discarded",
                            if *changes == 1 { "" } else { "s" }
                        )
                    }
                } else if model.settings.language.is_zh() {
                    "检测到未提交的更改，将一并丢弃".to_string()
                } else {
                    "Uncommitted changes detected and will be discarded".to_string()
                };
                ui.painter().text(
                    Pos2::new(rect.left() + 22.0, rect.top() + 150.0),
                    Align2::LEFT_CENTER,
                    detail,
                    FontId::proportional(10.5),
                    p.amber,
                );
            }
        }
        DialogState::Push {
            remote,
            branch,
            set_upstream,
            force,
            push_tags,
            remotes,
            pending,
            status,
        } => {
            ui.painter().text(
                Pos2::new(rect.left() + 22.0, rect.top() + 92.0),
                Align2::LEFT_CENTER,
                crate::tr!(model, "Remote", "远程"),
                FontId::proportional(9.5),
                p.text3,
            );
            let remote_rect = Rect::from_min_size(
                Pos2::new(rect.left() + 90.0, rect.top() + 78.0),
                Vec2::new(210.0, 26.0),
            );
            if remotes.is_empty() {
                ui.painter().text(
                    Pos2::new(remote_rect.left() + 8.0, remote_rect.center().y),
                    Align2::LEFT_CENTER,
                    remote.as_str(),
                    FontId::monospace(11.0),
                    p.text,
                );
            } else {
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(remote_rect)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    |ui| {
                        egui::ComboBox::from_id_salt("push-remote")
                            .width(remotes_width(remotes, remote_rect.width()))
                            .selected_text(egui::RichText::new(remote.as_str()).color(p.text))
                            .show_ui(ui, |ui| {
                                for candidate in remotes.iter() {
                                    if ui
                                        .selectable_label(
                                            remote == &candidate.name,
                                            &candidate.name,
                                        )
                                        .clicked()
                                    {
                                        *remote = candidate.name.clone();
                                    }
                                }
                            });
                    },
                );
            }
            ui.painter().text(
                Pos2::new(rect.left() + 22.0, rect.top() + 124.0),
                Align2::LEFT_CENTER,
                crate::tr!(model, "Branch", "分支"),
                FontId::proportional(9.5),
                p.text3,
            );
            ui.painter().text(
                Pos2::new(rect.left() + 90.0, rect.top() + 124.0),
                Align2::LEFT_CENTER,
                branch,
                FontId::monospace(11.0),
                p.text,
            );
            let option_y = rect.top() + 158.0;
            let mut option = |x: f32, value: &mut bool, label: &str| {
                let check = Rect::from_center_size(
                    Pos2::new(rect.left() + x + 8.0, option_y),
                    Vec2::splat(16.0),
                );
                if checkbox(ui, check, *value, theme).clicked() {
                    *value = !*value;
                }
                ui.painter().text(
                    Pos2::new(check.right() + 6.0, option_y),
                    Align2::LEFT_CENTER,
                    label,
                    FontId::proportional(10.5),
                    p.text2,
                );
            };
            option(
                22.0,
                set_upstream,
                crate::tr!(model, "Set upstream", "建立 upstream"),
            );
            option(
                190.0,
                force,
                crate::tr!(model, "Force (with lease)", "强制（with lease）"),
            );
            option(382.0, push_tags, crate::tr!(model, "Push tags", "推送标签"));

            ui.painter().text(
                Pos2::new(rect.left() + 22.0, rect.top() + 196.0),
                Align2::LEFT_CENTER,
                format!(
                    "{} {}",
                    crate::tr!(model, "Commits to push:", "待推送提交："),
                    pending.len()
                ),
                FontId::proportional(10.5),
                p.text3,
            );
            let list_rect = Rect::from_min_max(
                Pos2::new(rect.left() + 18.0, rect.top() + 208.0),
                Pos2::new(rect.right() - 18.0, rect.bottom() - 72.0),
            );
            super::app_ui::region(ui, list_rect, "push-pending", |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("push-pending-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if pending.is_empty() {
                            ui.label(
                                egui::RichText::new(crate::tr!(
                                    model,
                                    "Everything is already pushed.",
                                    "没有待推送的提交。"
                                ))
                                .color(p.text3)
                                .size(10.5),
                            );
                        }
                        for commit in pending.iter() {
                            let (row, _) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 24.0),
                                Sense::hover(),
                            );
                            ui.painter().text(
                                Pos2::new(row.left() + 4.0, row.center().y),
                                Align2::LEFT_CENTER,
                                &commit.short,
                                FontId::monospace(9.5),
                                p.accent,
                            );
                            text_clipped(
                                ui.painter(),
                                Rect::from_min_max(
                                    Pos2::new(row.left() + 66.0, row.top()),
                                    Pos2::new(row.right() - 160.0, row.bottom()),
                                ),
                                &commit.message,
                                FontId::proportional(10.5),
                                p.text,
                                Align2::LEFT_CENTER,
                            );
                            ui.painter().text(
                                Pos2::new(row.right() - 4.0, row.center().y),
                                Align2::RIGHT_CENTER,
                                &commit.author,
                                FontId::proportional(9.0),
                                p.text3,
                            );
                        }
                    });
            });

            if let Some(status) = status {
                ui.painter().text(
                    Pos2::new(rect.left() + 22.0, rect.bottom() - 58.0),
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
            editing,
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
            let mut edit_remote: Option<(String, String)> = None;
            let mut remove_remote: Option<String> = None;
            super::app_ui::region(ui, rows, "remotes-rows", |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("remotes-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for list_remote in remotes.iter() {
                            let (row, _) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 34.0),
                                Sense::hover(),
                            );
                            ui.painter().text(
                                Pos2::new(row.left() + 6.0, row.center().y - 6.0),
                                Align2::LEFT_CENTER,
                                &list_remote.name,
                                FontId::proportional(10.5),
                                p.text,
                            );
                            text_clipped(
                                ui.painter(),
                                Rect::from_min_max(
                                    Pos2::new(row.left() + 6.0, row.center().y + 3.0),
                                    Pos2::new(row.right() - 92.0, row.bottom()),
                                ),
                                &list_remote.url,
                                FontId::monospace(8.5),
                                p.text3,
                                Align2::LEFT_CENTER,
                            );
                            let edit_rect = Rect::from_center_size(
                                Pos2::new(row.right() - 74.0, row.center().y),
                                Vec2::new(38.0, 20.0),
                            );
                            let delete_rect = Rect::from_center_size(
                                Pos2::new(row.right() - 34.0, row.center().y),
                                Vec2::new(38.0, 20.0),
                            );
                            let edit_resp = ui.interact(
                                edit_rect,
                                ui.scope_id().with(("edit-remote", &list_remote.name)),
                                Sense::click(),
                            );
                            let delete_resp = ui.interact(
                                delete_rect,
                                ui.scope_id().with(("delete-remote", &list_remote.name)),
                                Sense::click(),
                            );
                            for (rect, hovered, label, color) in [
                                (
                                    edit_rect,
                                    edit_resp.hovered(),
                                    crate::tr!(model, "Edit", "编辑"),
                                    p.accent,
                                ),
                                (
                                    delete_rect,
                                    delete_resp.hovered(),
                                    crate::tr!(model, "Delete", "删除"),
                                    p.rose,
                                ),
                            ] {
                                if hovered {
                                    ui.painter().rect_filled(
                                        rect,
                                        egui::CornerRadius::same(3),
                                        p.hover,
                                    );
                                }
                                ui.painter().text(
                                    rect.center(),
                                    Align2::CENTER_CENTER,
                                    label,
                                    FontId::proportional(9.0),
                                    color,
                                );
                            }
                            if edit_resp.clicked() {
                                edit_remote =
                                    Some((list_remote.name.clone(), list_remote.url.clone()));
                            } else if delete_resp.clicked() {
                                remove_remote = Some(list_remote.name.clone());
                            }
                        }
                    });
            });
            if let Some((name, url)) = edit_remote {
                *editing = Some(name.clone());
                *new_name = name;
                *new_url = url;
            }
            if let Some(name) = remove_remote {
                model.queue(Command::RemoteRemove(name));
                close = true;
            }
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
                    .align(egui::Align2::LEFT_CENTER)
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
                    .align(egui::Align2::LEFT_CENTER)
                    .hint_text("https://github.com/user/repo.git")
                    .font(FontId::monospace(9.5)),
            );
            let add_rect = Rect::from_min_size(
                Pos2::new(url_rect.right() + 8.0, url_rect.top()),
                Vec2::new(64.0, 26.0),
            );
            let form_label = if editing.is_some() {
                crate::tr!(model, "Save", "保存")
            } else {
                crate::tr!(model, "Add", "添加")
            };
            if crate::widgets::primary_button(ui, add_rect, form_label, None, true, theme).clicked()
            {
                confirm_add_remote = true;
                // The Remotes arm lives inside the footer's `if confirm`
                // match; without also setting `confirm`, the Add/Save click
                // only armed a flag that was then dropped at the end of the
                // frame (the button looked alive but did nothing).
                confirm = true;
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
                    .align(egui::Align2::LEFT_CENTER)
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
        | DialogState::RenameSymbol { value, error, .. }
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
                    .align(egui::Align2::LEFT_CENTER)
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
            paint_unsaved_delete_warning(ui, model, theme, rect, std::slice::from_ref(path));
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
            paint_unsaved_delete_warning(ui, model, theme, rect, paths);
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
                    .align(egui::Align2::LEFT_CENTER)
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
        DialogState::Unstash {
            branch,
            as_new_branch,
            pop,
            reinstate_index,
            error,
            ..
        } => {
            ui.painter().text(
                Pos2::new(rect.left() + 22.0, rect.top() + 92.0),
                Align2::LEFT_CENTER,
                crate::tr!(model, "Current branch:", "当前分支:"),
                FontId::proportional(11.5),
                p.text3,
            );
            ui.painter().text(
                Pos2::new(rect.left() + 130.0, rect.top() + 92.0),
                Align2::LEFT_CENTER,
                if branch.is_empty() {
                    "—"
                } else {
                    branch.as_str()
                },
                FontId::proportional(11.5),
                p.text,
            );
            ui.painter().text(
                Pos2::new(rect.left() + 22.0, rect.top() + 128.0),
                Align2::LEFT_CENTER,
                crate::tr!(model, "As new branch:", "作为新分支:"),
                FontId::proportional(11.5),
                p.text3,
            );
            let input = Rect::from_min_size(
                Pos2::new(rect.left() + 130.0, rect.top() + 113.0),
                Vec2::new(rect.width() - 152.0, 28.0),
            );
            ui.painter()
                .rect_filled(input, egui::CornerRadius::same(4), p.panel2);
            ui.painter().rect_stroke(
                input,
                egui::CornerRadius::same(4),
                Stroke::new(1.0, p.border),
                StrokeKind::Inside,
            );
            ui.put(
                input.shrink2(Vec2::new(8.0, 4.0)),
                TextEdit::singleline(as_new_branch)
                    .frame(egui::Frame::NONE)
                    .align(egui::Align2::LEFT_CENTER)
                    .hint_text(crate::tr!(
                        model,
                        "Leave empty to stay on the current branch",
                        "留空则停留在当前分支"
                    ))
                    .font(FontId::monospace(11.0)),
            );
            let pop_check = Rect::from_center_size(
                Pos2::new(rect.left() + 30.0, rect.top() + 170.0),
                Vec2::splat(16.0),
            );
            if checkbox(ui, pop_check, *pop, theme).clicked() {
                *pop = !*pop;
            }
            ui.painter().text(
                Pos2::new(rect.left() + 42.0, rect.top() + 170.0),
                Align2::LEFT_CENTER,
                crate::tr!(model, "Pop stash", "弹出贮藏"),
                FontId::proportional(11.0),
                p.text2,
            );
            let index_check = Rect::from_center_size(
                Pos2::new(rect.left() + 30.0, rect.top() + 198.0),
                Vec2::splat(16.0),
            );
            if checkbox(ui, index_check, *reinstate_index, theme).clicked() {
                *reinstate_index = !*reinstate_index;
            }
            ui.painter().text(
                Pos2::new(rect.left() + 42.0, rect.top() + 198.0),
                Align2::LEFT_CENTER,
                crate::tr!(model, "Reinstate index", "恢复索引"),
                FontId::proportional(11.0),
                p.text2,
            );
            if let Some(err) = error {
                ui.painter().text(
                    Pos2::new(rect.left() + 22.0, rect.top() + 228.0),
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
                        "回滚（放弃这些文件的未提交更改）"
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
            let mut list_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(list_rect)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            egui::ScrollArea::vertical()
                .id_salt("unshelve-files")
                .auto_shrink([false, false])
                .show(&mut list_ui, |ui| {
                    ui.set_width(list_rect.width());
                    for (path, selected) in files.iter_mut() {
                        let (row, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), row_h),
                            Sense::hover(),
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
                });
            // 目标 changelist：现有列表单选 + 新建输入（项目可能已关闭，
            // 不 unwrap；无项目时只剩新建输入）。
            let existing_lists: Vec<String> = model
                .active_project()
                .map(|project| {
                    lazecode_git::Changelists::open(project.active_root())
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
                    .align(egui::Align2::LEFT_CENTER)
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
        DialogState::CommitFiles { value, error, .. }
        | DialogState::ExtractChanges { value, error, .. } => {
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
        DialogState::GetFileFromRevision {
            path,
            revision,
            error,
        } => {
            ui.painter().text(
                Pos2::new(rect.left() + 22.0, rect.top() + 100.0),
                Align2::LEFT_CENTER,
                std::path::Path::new(path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(path),
                FontId::monospace(10.5),
                p.text3,
            );
            let input = Rect::from_min_size(
                Pos2::new(rect.left() + 22.0, rect.top() + 118.0),
                Vec2::new(rect.width() - 44.0, 28.0),
            );
            ui.painter()
                .rect_filled(input, egui::CornerRadius::same(5), p.panel2);
            let response = ui.put(
                input.shrink2(Vec2::new(8.0, 3.0)),
                TextEdit::singleline(revision)
                    .frame(egui::Frame::NONE)
                    .hint_text(crate::tr!(
                        model,
                        "Revision (HEAD, tag, hash)",
                        "版本（HEAD、标签、哈希）"
                    ))
                    .font(FontId::monospace(10.5)),
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
        DialogState::CherryPickChanges {
            paths,
            target,
            error,
        } => {
            ui.painter().text(
                Pos2::new(rect.left() + 22.0, rect.top() + 100.0),
                Align2::LEFT_CENTER,
                if model.settings.language.is_zh() {
                    format!("{} 个文件", paths.len())
                } else {
                    format!("{} file(s)", paths.len())
                },
                FontId::proportional(10.0),
                p.text3,
            );
            let input = Rect::from_min_size(
                Pos2::new(rect.left() + 22.0, rect.top() + 118.0),
                Vec2::new(rect.width() - 44.0, 28.0),
            );
            ui.painter()
                .rect_filled(input, egui::CornerRadius::same(5), p.panel2);
            let response = ui.put(
                input.shrink2(Vec2::new(8.0, 3.0)),
                TextEdit::singleline(target)
                    .frame(egui::Frame::NONE)
                    .hint_text(crate::tr!(model, "Target branch", "目标分支"))
                    .font(FontId::monospace(10.5)),
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
        DialogState::Quit { .. } | DialogState::ConfirmTerminalClose { .. } => {}
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
        DialogState::CheckoutRevision { .. } => crate::tr!(model, "Checkout", "检出"),
        DialogState::RemoveWorktree { dirty: true, .. } => {
            crate::tr!(model, "Force remove", "强制删除")
        }
        DialogState::RemoveWorktree { dirty: false, .. } => {
            crate::tr!(model, "Remove", "删除")
        }
        DialogState::DeleteBranch { force, .. } => {
            if *force {
                crate::tr!(model, "Force delete", "强制删除")
            } else {
                crate::tr!(model, "Delete", "删除")
            }
        }
        DialogState::RenameBranch { .. } => crate::tr!(model, "Rename", "重命名"),
        DialogState::Push { .. } => crate::tr!(model, "Push", "推送"),
        DialogState::Pull { .. } => crate::tr!(model, "Pull", "拉取"),
        DialogState::Remotes { .. } => crate::tr!(model, "Done", "完成"),
        DialogState::Reset { .. } => crate::tr!(model, "Reset", "重置"),
        DialogState::NewTag { .. } => crate::tr!(model, "Create", "创建"),
        DialogState::Quit { restart_update, .. } => {
            if *restart_update {
                crate::tr!(model, "Restart now", "立即重启")
            } else {
                crate::tr!(model, "Quit", "退出")
            }
        }
        DialogState::CloseDirtyTab { .. } => crate::tr!(model, "Discard", "放弃修改"),
        DialogState::ConfirmTerminalClose { .. } => {
            crate::tr!(model, "Close anyway", "确定关闭")
        }
        DialogState::NewFile { .. } | DialogState::NewDirectory { .. } => {
            crate::tr!(model, "Create", "创建")
        }
        DialogState::RenamePath { .. } | DialogState::RenameShelf { .. } => {
            crate::tr!(model, "Rename", "重命名")
        }
        DialogState::DeletePath { .. }
        | DialogState::ConfirmDeleteUntracked { .. }
        | DialogState::ConfirmDeleteShelf { .. } => {
            crate::tr!(model, "Delete", "删除")
        }
        DialogState::NewShelf { .. } => crate::tr!(model, "Save", "保存"),
        DialogState::GoToLine { .. } => crate::tr!(model, "Go", "跳转"),
        DialogState::RenameSymbol { .. } => crate::tr!(model, "Rename", "重命名"),
        DialogState::RenameTerminal { .. } => crate::tr!(model, "Rename", "重命名"),
        DialogState::NewChangelist { .. } => crate::tr!(model, "Create", "创建"),
        DialogState::StashCreate { .. } => crate::tr!(model, "Stash", "贮藏"),
        DialogState::Unstash { .. } => crate::tr!(model, "Apply Stash", "应用贮藏"),
        DialogState::ConfirmDiscard { .. } => crate::tr!(model, "Rollback", "回滚"),
        DialogState::DeleteChangelist { .. } => crate::tr!(model, "Delete", "删除"),
        DialogState::Unshelve { .. } => crate::tr!(model, "Unshelve", "恢复"),
        DialogState::CommitFiles { .. } => crate::tr!(model, "Commit", "提交"),
        DialogState::ExtractChanges { .. } => crate::tr!(model, "Extract", "提取"),
        DialogState::GetFileFromRevision { .. } => crate::tr!(model, "Restore", "恢复"),
        DialogState::CherryPickChanges { .. } => crate::tr!(model, "Cherry-pick", "拣选"),
        _ => "Discard",
    };
    if primary_button(ui, confirm_rect, confirm_label, None, true, theme).clicked() {
        confirm = true;
    }
    if confirm {
        match &dialog {
            DialogState::NewBranch {
                value,
                start_point,
                checkout,
                ..
            } => {
                let value = value.trim().to_string();
                if value.is_empty() {
                    rt.dialog = DialogState::NewBranch {
                        value,
                        start_point: start_point.clone(),
                        checkout: *checkout,
                        error: Some(
                            crate::tr!(model, "Enter a branch name", "请输入分支名").to_string(),
                        ),
                    };
                    close = true;
                } else {
                    model.queue(Command::CreateBranch {
                        name: value,
                        start_point: start_point.clone(),
                        checkout: *checkout,
                    });
                    close = true;
                }
            }
            DialogState::NewWorktree {
                name,
                branch,
                create_new,
                location,
                ..
            } => {
                let name = name.trim().to_string();
                let branch = branch.trim().to_string();
                let location_str = location.trim().to_string();

                let used_branches = model
                    .active_project()
                    .map(used_worktree_branches)
                    .unwrap_or_default();

                if name.is_empty() {
                    rt.dialog = DialogState::NewWorktree {
                        name,
                        branch,
                        create_new: *create_new,
                        location: location_str,
                        error: Some(
                            crate::tr!(model, "Enter a worktree name", "请输入工作树名称")
                                .to_string(),
                        ),
                    };
                } else if branch.is_empty() {
                    rt.dialog = DialogState::NewWorktree {
                        name,
                        branch,
                        create_new: *create_new,
                        location: location_str,
                        error: Some(if *create_new {
                            crate::tr!(model, "Enter a new branch name", "请输入新分支名称")
                                .to_string()
                        } else {
                            crate::tr!(model, "Select a branch", "请选择分支").to_string()
                        }),
                    };
                } else if let Some(wt_name) = used_branches.get(&branch) {
                    rt.dialog = DialogState::NewWorktree {
                        name,
                        branch: branch.clone(),
                        create_new: *create_new,
                        location: location_str,
                        error: Some(if model.settings.language.is_zh() {
                            format!("分支 '{branch}' 已在工作树 '{wt_name}' 检出，多个工作树不能使用相同分支")
                        } else {
                            format!("Branch '{branch}' is already checked out in '{wt_name}'")
                        }),
                    };
                } else {
                    let loc_path = if location_str.is_empty() {
                        let root = model
                            .active_project()
                            .map(|p| p.root.clone())
                            .unwrap_or_default();
                        root.join(".worktree").join(&name)
                    } else {
                        std::path::PathBuf::from(location_str)
                    };
                    model.queue(Command::CreateWorktree {
                        name,
                        branch,
                        location: loc_path,
                    });
                    close = true;
                }
            }
            DialogState::CheckoutRevision { value, .. } => {
                let value = value.trim().to_string();
                if value.is_empty() {
                    rt.dialog = DialogState::CheckoutRevision {
                        value,
                        error: Some(
                            crate::tr!(model, "Enter revision or branch", "请输入修订版本或分支")
                                .to_string(),
                        ),
                    };
                    close = true;
                } else {
                    model.queue(Command::CheckoutBranch(value));
                    close = true;
                }
            }
            DialogState::RemoveWorktree {
                name,
                dirty,
                worktree_path: _,
                changes: _,
            } => {
                model.queue(Command::RemoveWorktree {
                    name: name.clone(),
                    force: *dirty,
                });
                close = true;
            }
            DialogState::DeleteBranch {
                name,
                force,
                remote,
                ..
            } => {
                if *remote {
                    model.queue(Command::DeleteRemoteBranch(name.clone()));
                } else {
                    model.queue(Command::DeleteBranch {
                        name: name.clone(),
                        force: *force,
                    });
                }
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
                if let Some(index) = model.active_project {
                    model.close_tab(index, *tab_id);
                }
                if rt.editor_focus == Some(*tab_id) {
                    rt.editor_focus = None;
                }
                close = true;
            }
            DialogState::ConfirmTerminalClose { action, .. } => {
                let action = action.clone();
                rt.apply_terminal_close(model, action);
                close = true;
            }
            DialogState::Quit { restart_update, .. } => {
                if *restart_update {
                    match rt.update.relaunch_installed() {
                        Ok(()) => model.should_quit = true,
                        Err(err) => {
                            rt.update.error = Some(err);
                            rt.update_open = true;
                        }
                    }
                } else {
                    model.should_quit = true;
                }
                close = true;
            }
            DialogState::Push {
                remote,
                branch,
                set_upstream,
                force,
                push_tags,
                ..
            } => {
                model.queue(Command::Push {
                    remote: remote.clone(),
                    branch: branch.clone(),
                    set_upstream: *set_upstream,
                    force: *force,
                    push_tags: *push_tags,
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
                editing,
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
                            editing: editing.clone(),
                            error: Some(
                                crate::tr!(model, "Enter a name and a URL", "请输入名称和 URL")
                                    .to_string(),
                            ),
                        };
                    } else if let Some(from) = editing.clone() {
                        model.queue(Command::RemoteEdit { from, name, url });
                        close = true;
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
                    model.queue(Command::CreateTag {
                        name,
                        commit: commit_hint.clone(),
                    });
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
                } else if !is_plain_name(name) {
                    rt.dialog = DialogState::NewFile {
                        dir: dir.clone(),
                        value: value.clone(),
                        error: Some(
                            crate::tr!(
                                model,
                                "Enter a file name, not a path",
                                "请输入文件名，不要包含路径"
                            )
                            .to_string(),
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
                            let _ = model.open_file_in(active, target);
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
                } else if !is_plain_name(name) {
                    rt.dialog = DialogState::NewDirectory {
                        dir: dir.clone(),
                        value: value.clone(),
                        error: Some(
                            crate::tr!(
                                model,
                                "Enter a directory name, not a path",
                                "请输入目录名，不要包含路径"
                            )
                            .to_string(),
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
                } else if !is_plain_name(name) {
                    rt.dialog = DialogState::RenamePath {
                        old_path: old_path.clone(),
                        value: value.clone(),
                        error: Some(
                            crate::tr!(
                                model,
                                "Enter a file name, not a path",
                                "请输入文件名，不要包含路径"
                            )
                            .to_string(),
                        ),
                    };
                } else if old_path.file_name() == Some(std::ffi::OsStr::new(name)) {
                    rt.dialog = DialogState::RenamePath {
                        old_path: old_path.clone(),
                        value: value.clone(),
                        error: Some(crate::tr!(model, "Name unchanged", "名称未改变").to_string()),
                    };
                } else if let Some(parent) = old_path.parent() {
                    let target = parent.join(name);
                    match lazecode_fs::rename_no_overwrite(old_path, &target) {
                        Err(lazecode_fs::RenameError::TargetExists { is_dir: true }) => {
                            rt.dialog = DialogState::RenamePath {
                                old_path: old_path.clone(),
                                value: value.clone(),
                                error: Some(
                                    crate::tr!(model, "Directory already exists", "目录已存在")
                                        .to_string(),
                                ),
                            };
                        }
                        Err(lazecode_fs::RenameError::TargetExists { is_dir: false }) => {
                            rt.dialog = DialogState::RenamePath {
                                old_path: old_path.clone(),
                                value: value.clone(),
                                error: Some(
                                    crate::tr!(model, "File already exists", "文件已存在")
                                        .to_string(),
                                ),
                            };
                        }
                        Err(lazecode_fs::RenameError::Io(e)) => {
                            rt.dialog = DialogState::RenamePath {
                                old_path: old_path.clone(),
                                value: value.clone(),
                                error: Some(e.to_string()),
                            };
                        }
                        Ok(()) => {
                            model.retarget_path(old_path, &target);
                            // Recent files lists files; a renamed directory
                            // would open as an error.
                            if target.is_file() {
                                model.note_recent_file(target);
                            }
                            close = true;
                        }
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
            DialogState::ConfirmDeleteShelf { name, purge } => {
                // 稳定版删除架藏前有确认；删除进入「最近删除」（可恢复），
                // purge = 彻底删除。确认后重新加载架藏数据。
                if let Some(project) = model.active_project() {
                    let root = project.root.clone();
                    let result = if *purge {
                        lazecode_git::shelf_purge_deleted(&root, name)
                    } else {
                        lazecode_git::shelf_delete(&root, name)
                    };
                    let zh = model.settings.language.is_zh();
                    model.toast(
                        match &result {
                            Ok(_) if !purge => {
                                if zh {
                                    format!("已删除架藏 {name}（可在最近删除中恢复）")
                                } else {
                                    format!("Deleted shelf {name} (restorable)")
                                }
                            }
                            Ok(_) => "Purged".to_string(),
                            Err(err) => err.to_string(),
                        },
                        if result.is_ok() {
                            ToastKind::Info
                        } else {
                            ToastKind::Error
                        },
                    );
                    rt.shelf_loaded = None;
                }
                if *purge {
                    rt.dialog = DialogState::None;
                } else {
                    rt.dialog = DialogState::ShelfManager {
                        shelves: Vec::new(),
                        selected: 0,
                        error: None,
                        loaded: false,
                    };
                }
                close = true;
            }
            DialogState::RenameShelf { from, value, .. } => {
                let name = value.trim().to_string();
                if name.is_empty() {
                    rt.dialog = DialogState::RenameShelf {
                        from: from.clone(),
                        value: name,
                        error: Some(
                            crate::tr!(model, "Enter a shelf name", "请输入架藏名称").to_string(),
                        ),
                    };
                    close = true;
                } else {
                    model.queue(Command::ShelfRename {
                        from: from.clone(),
                        to: name,
                    });
                    rt.shelf_loaded = None;
                    close = true;
                }
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
            DialogState::RenameSymbol {
                tab_id,
                path,
                line,
                col,
                value,
                ..
            } => {
                let new_name = value.trim().to_string();
                if new_name.is_empty() {
                    rt.dialog = DialogState::RenameSymbol {
                        tab_id: *tab_id,
                        path: path.clone(),
                        line: *line,
                        col: *col,
                        value: new_name,
                        error: Some(
                            crate::tr!(model, "Enter a new name", "请输入新名称").to_string(),
                        ),
                    };
                } else {
                    let path_buf = std::path::PathBuf::from(path);
                    let position = model.projects.iter().find_map(|project| {
                        project.tabs.iter().find_map(|tab| match &tab.kind {
                            TabKind::File(file) if file.doc.path == path_buf => {
                                Some(file.doc.lsp_position(lazecode_text::Cursor {
                                    line: *line,
                                    col: *col,
                                }))
                            }
                            _ => None,
                        })
                    });
                    let message = match position {
                        Some((lsp_line, lsp_character)) => {
                            match model
                                .lsp
                                .rename(&path_buf, lsp_line, lsp_character, &new_name)
                            {
                                Some(id) => {
                                    rt.lsp.rename_pending = Some(id);
                                    close = true;
                                    None
                                }
                                None => Some(
                                    crate::tr!(
                                        model,
                                        "No language server is available",
                                        "没有可用的语言服务器"
                                    )
                                    .to_string(),
                                ),
                            }
                        }
                        None => Some(
                            crate::tr!(model, "The file is no longer open", "文件已不再打开")
                                .to_string(),
                        ),
                    };
                    if let Some(error) = message {
                        rt.dialog = DialogState::RenameSymbol {
                            tab_id: *tab_id,
                            path: path.clone(),
                            line: *line,
                            col: *col,
                            value: new_name,
                            error: Some(error),
                        };
                    }
                }
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
            DialogState::Unstash {
                stash,
                as_new_branch,
                pop,
                reinstate_index,
                ..
            } => {
                let branch = as_new_branch.trim().to_string();
                model.queue(Command::Unstash {
                    stash: stash.clone(),
                    branch: (!branch.is_empty()).then_some(branch),
                    pop: *pop,
                    reinstate_index: *reinstate_index,
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
            DialogState::CommitFiles { paths, value, .. }
            | DialogState::ExtractChanges { paths, value, .. } => {
                let message = value.trim().to_string();
                if message.is_empty() {
                    let dialog = match &dialog {
                        DialogState::ExtractChanges { paths, value, .. } => {
                            DialogState::ExtractChanges {
                                paths: paths.clone(),
                                value: value.clone(),
                                error: Some(
                                    crate::tr!(
                                        model,
                                        "Add a commit message first",
                                        "请先填写提交信息"
                                    )
                                    .to_string(),
                                ),
                            }
                        }
                        _ => DialogState::CommitFiles {
                            paths: paths.clone(),
                            value: value.clone(),
                            error: Some(
                                crate::tr!(model, "Add a commit message first", "请先填写提交信息")
                                    .to_string(),
                            ),
                        },
                    };
                    rt.dialog = dialog;
                    close = true;
                } else {
                    model.queue(Command::CommitPaths {
                        paths: paths.clone(),
                        message,
                    });
                    close = true;
                }
            }
            DialogState::GetFileFromRevision { path, revision, .. } => {
                let revision = revision.trim().to_string();
                if revision.is_empty() {
                    if let DialogState::GetFileFromRevision { error, .. } = &mut dialog {
                        *error =
                            Some(crate::tr!(model, "Enter a revision", "请输入版本").to_string());
                    }
                } else {
                    model.queue(Command::GetFileFromRevision {
                        path: path.clone(),
                        revision,
                    });
                    close = true;
                }
            }
            DialogState::CherryPickChanges { paths, target, .. } => {
                let target = target.trim().to_string();
                if target.is_empty() {
                    if let DialogState::CherryPickChanges { error, .. } = &mut dialog {
                        *error = Some(
                            crate::tr!(model, "Enter a target branch", "请输入目标分支")
                                .to_string(),
                        );
                    }
                } else {
                    model.queue(Command::CherryPickChanges {
                        paths: paths.clone(),
                        target,
                    });
                    close = true;
                }
            }
            _ => {}
        }
    }
    if !close {
        // Confirmation branches replace `rt.dialog` with an error-bearing
        // variant and leave the dialog open. Only restore the pre-draw state
        // when no branch produced a replacement; unconditionally assigning
        // `dialog` here would silently discard every validation message.
        if matches!(&rt.dialog, DialogState::None) {
            rt.dialog = dialog;
        }
    } else {
        rt.branch_create_error = None;
    }
    let _ = (model, ToastKind::Info);
}

fn paint_wrapped_text(ui: &Ui, rect: Rect, text: &str, font: FontId, color: Color32) {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = rect.width();
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
    ui.painter().galley(rect.left_top(), galley, color);
}
