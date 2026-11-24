use std::path::PathBuf;
use std::time::Duration;

use egui::{
    Align2, Color32, FontId, Key, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2,
};
use lazecode_core::{AppModel, Command, TabKind, ToastKind};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::{DialogState, MenuKind, MenuState, UiRuntime};
use crate::theme::Theme;
use crate::widgets::{checkbox, primary_button, secondary_button, text_clipped};

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
    if let Some(menu) = rt.menu.take() {
        match menu.kind {
            MenuKind::Project => draw_project_menu(ui, model, rt, theme, screen, menu),
            MenuKind::Branch => draw_branch_menu(ui, model, rt, theme, screen, menu),
            MenuKind::Worktree => draw_worktree_menu(ui, model, rt, theme, screen, menu),
            MenuKind::LogBranchFilter { project } => {
                draw_log_branch_menu(ui, model, rt, theme, screen, menu, project)
            }
            MenuKind::LogDateFilter { project } => {
                draw_log_date_menu(ui, model, rt, theme, screen, menu, project)
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
            MenuKind::TagContext { project } => {
                let payload = menu.payload.clone();
                draw_tag_context_menu(
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
                tab_id,
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
                    tab_id,
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
            MenuKind::CommitExtras => {
                draw_commit_extras_menu(ui, model, rt, theme, screen, menu);
            }
            MenuKind::StashEntryMenu { project } => {
                let payload = menu.payload.clone();
                draw_stash_entry_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref(),
                );
            }
            MenuKind::StashFileMenu { project } => {
                let payload = menu.payload.clone();
                draw_stash_file_menu(
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
            MenuKind::CommitViewOptions => {
                draw_commit_view_options_menu(ui, model, rt, theme, screen, menu);
            }
            MenuKind::CompareViewOptions { project } => {
                draw_compare_view_options_menu(ui, model, rt, theme, screen, menu, project);
            }
            MenuKind::ShelfContext { project } => {
                let payload = menu.payload.clone();
                draw_shelf_context_menu(
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
            MenuKind::CompareUserFilter { side } => {
                draw_compare_user_menu(ui, model, rt, theme, screen, menu, side);
            }
            MenuKind::CompareDateFilter { side } => {
                draw_compare_date_menu(ui, model, rt, theme, screen, menu, side);
            }
            MenuKind::CompareRefFilter { side } => {
                draw_compare_ref_menu(ui, model, rt, theme, screen, menu, side);
            }
            MenuKind::LogSortOptions { project } => {
                draw_log_sort_options_menu(ui, model, rt, theme, screen, menu, project);
            }
            MenuKind::LogPathFilter { project } => {
                let payload = menu.payload.clone();
                draw_log_path_filter_menu(
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
            MenuKind::CommitDetailsFileContext { project } => {
                let payload = menu.payload.clone();
                draw_commit_details_file_menu(
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
            MenuKind::EditorContext { project } => {
                let payload = menu.payload.clone();
                draw_editor_context_menu(
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
        }
    }
    if let Some(anchor) = rt.agent_menu.take() {
        draw_agent_menu(ui, model, rt, theme, screen, anchor);
    }
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

fn draw_project_menu(
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
    super::app_ui::region(ui, body_rect, "project-menu-body", |ui| {
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
    let height = 12.0 + (rows_vec.len() as f32 * 24.0 + 26.0).min(320.0);
    let rect = popover_rect(rt, screen, menu.anchor, 260.0, height);
    menu_frame(ui, rect, theme);
    let mut pick: Option<Option<String>> = None;
    let rows = Rect::from_min_max(
        Pos2::new(rect.left() + 6.0, rect.top() + 6.0),
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
fn draw_log_date_menu(
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
fn draw_tag_context_menu(
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
    let _p = theme.palette;
    let selected_refs: Vec<String> = model.projects[index].ref_selection.clone();
    let is_multi = selected_refs.len() > 1;
    let items: Vec<(&str, u8)> = if is_multi {
        let mut list = Vec::new();
        if selected_refs.len() == 2 {
            list.push((
                crate::tr!(model, "Compare branches…", "比较所选两个分支…"),
                70,
            ));
            list.push((crate::tr!(model, "Show files diff…", "显示文件差异…"), 73));
        }
        list.push((crate::tr!(model, "Update selected", "更新选中项"), 72));
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
            (crate::tr!(model, "Set log filter", "设为日志过滤"), 6),
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
    let favorite_root = model.projects[index].root.clone();
    let is_fav = model.is_favorite_branch(&favorite_root, &name);
    let fav_label = if is_fav {
        crate::tr!(model, "Remove favorite", "取消收藏")
    } else {
        crate::tr!(model, "Mark favorite", "标记收藏")
    };
    // The favorite row is drawn after `items`, so include it in the frame.
    let extra_row = if is_multi { 0.0 } else { MENU_ROW_H };
    let height = items.len() as f32 * MENU_ROW_H + extra_row + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 260.0, height);
    menu_frame(ui, rect, theme);
    // 稳定版分支右键菜单没有标题行（BranchContextMenu 无 header）。
    let mut action: Option<u8> = None;
    let mut toggle_fav = false;
    let row_start = rect.top() + 4.0;
    for (i, (label, code)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, row_start + i as f32 * MENU_ROW_H),
            Vec2::new(rect.width() - 8.0, MENU_ROW_H),
        );
        let danger = matches!(*code, 3 | 71);
        if menu_row(
            ui,
            theme,
            row,
            ui.scope_id().with(("bctx", i)),
            label,
            danger,
            true,
        ) {
            action = Some(*code);
        }
    }
    // favorite row last.
    if !is_multi {
        let fav_row = Rect::from_min_size(
            Pos2::new(
                rect.left() + 4.0,
                row_start + items.len() as f32 * MENU_ROW_H,
            ),
            Vec2::new(rect.width() - 8.0, MENU_ROW_H),
        );
        if menu_row(
            ui,
            theme,
            fav_row,
            ui.scope_id().with("bctx-fav"),
            fav_label,
            false,
            true,
        ) {
            toggle_fav = true;
        }
    }
    let mut close = false;
    if toggle_fav {
        model.toggle_favorite_branch(&favorite_root, &name);
        model.save_workspace();
        close = true;
    }
    if let Some(code) = action {
        match code {
            0 => model.queue(Command::CheckoutBranch(name.clone())),
            1 => {
                rt.dialog = DialogState::NewBranch {
                    value: name.clone(),
                    start_point: Some(name.clone()),
                    checkout: true,
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
                    force: false,
                    remote,
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
            72 => {
                model.queue(Command::FetchAll);
            }
            73 => {
                if selected_refs.len() == 2 {
                    model.queue(Command::CompareOpen {
                        from: selected_refs[1].clone(),
                        to: selected_refs[0].clone(),
                    });
                    model.ui.sidebar_view = lazecode_core::SidebarView::Compare;
                }
            }
            71 => {
                for b in selected_refs {
                    if model.projects[index]
                        .branches
                        .iter()
                        .any(|branch| branch.name == b && branch.remote)
                    {
                        model.queue(Command::DeleteRemoteBranch(b));
                    } else if model.projects[index].tags.iter().any(|tag| tag == &b) {
                        model.queue(Command::DeleteTag(b));
                    } else {
                        model.queue(Command::DeleteBranch {
                            name: b,
                            force: false,
                        });
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
        project.active_root().to_path_buf()
    } else {
        project.active_root().join(rel_path)
    };
    let dir_for_new = if is_dir {
        abs_path.clone()
    } else {
        abs_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| project.active_root().to_path_buf())
    };

    let zh = model.settings.language.is_zh();
    let is_batch = model.ui.tree_selection.len() > 1
        && model
            .ui
            .tree_selection
            .contains(&std::path::PathBuf::from(rel_path));

    enum Item {
        Row(&'static str, &'static str, u8, bool, bool),
        Sub(&'static str, &'static str, &'static str),
        Sep,
    }

    let items: Vec<Item> = if is_batch {
        vec![Item::Row("Delete…", "删除…", 50, true, false)]
    } else {
        let mut list = vec![
            Item::Sub("New", "新建", "new"),
            Item::Row("Cut", "剪切", 101, false, false),
            Item::Row("Copy", "复制", 102, false, false),
            Item::Sub("Copy Path/Reference...", "复制路径/引用…", "copy-path"),
            Item::Row("Paste", "粘贴", 103, false, true),
            Item::Sep,
            Item::Row("Rename...", "重命名…", 4, false, false),
            Item::Sep,
            Item::Row("Find in Files...", "在文件中查找…", 104, false, false),
            Item::Sep,
            Item::Sub("Git", "Git", "git"),
            Item::Sep,
            Item::Row("Delete", "删除", 5, true, false),
            Item::Sep,
        ];
        if is_dir {
            list.push(Item::Row("Expand All", "全部展开", 105, false, false));
            list.push(Item::Row("Collapse All", "全部折叠", 106, false, false));
        }
        list.push(Item::Row(
            "Reload from Disk",
            "从磁盘重新加载",
            107,
            false,
            false,
        ));
        list.push(Item::Sub("Open In", "打开方式", "open-in"));
        if !is_dir {
            list.push(Item::Sep);
            list.push(Item::Row("Compare With...", "与本地比较…", 9, false, false));
        }
        list
    };

    let row_count = items.iter().filter(|i| !matches!(i, Item::Sep)).count();
    let sep_count = items.iter().filter(|i| matches!(i, Item::Sep)).count();
    let height = 12.0 + row_count as f32 * 24.0 + sep_count as f32 * 8.0;
    let width = if is_batch { 160.0 } else { 200.0 };
    let rect = popover_rect(rt, screen, menu.anchor, width, height);
    menu_frame(ui, rect, theme);

    let mut action: Option<u8> = None;
    let mut close = false;
    let mut y = rect.top() + 6.0;
    let mut hovered_sub: Option<(&'static str, Rect)> = None;

    for item in &items {
        match item {
            Item::Sep => {
                ui.painter().hline(
                    (rect.left() + 8.0)..=(rect.right() - 8.0),
                    y + 3.5,
                    Stroke::new(1.0, p.border_soft),
                );
                y += 8.0;
            }
            Item::Row(en, cn, code, danger, disabled) => {
                let row = Rect::from_min_size(
                    Pos2::new(rect.left() + 4.0, y),
                    Vec2::new(rect.width() - 8.0, 22.0),
                );
                let resp = ui.interact(row, ui.scope_id().with(("tctx", *code)), Sense::click());
                if !*disabled && resp.hovered() {
                    ui.painter()
                        .rect_filled(row, egui::CornerRadius::same(4), p.hover);
                }
                let text_col = if *disabled {
                    p.text3
                } else if *danger {
                    p.rose
                } else {
                    p.text
                };
                let label = if *code == 50 {
                    if zh {
                        format!("删除 ({} 项)", model.ui.tree_selection.len())
                    } else {
                        format!("Delete ({} items)", model.ui.tree_selection.len())
                    }
                } else {
                    (if zh { *cn } else { *en }).to_string()
                };
                ui.painter().text(
                    Pos2::new(row.left() + 10.0, row.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    FontId::proportional(12.5),
                    text_col,
                );
                if !*disabled && resp.clicked() {
                    action = Some(*code);
                }
                y += 24.0;
            }
            Item::Sub(en, cn, sub_id) => {
                let row = Rect::from_min_size(
                    Pos2::new(rect.left() + 4.0, y),
                    Vec2::new(rect.width() - 8.0, 22.0),
                );
                let resp = ui.interact(
                    row,
                    ui.scope_id().with(("tctx-sub", *sub_id)),
                    Sense::hover(),
                );
                let is_active_sub = rt.tree_submenu.as_ref().is_some_and(|(s, _)| s == *sub_id);
                if resp.hovered() || is_active_sub {
                    ui.painter()
                        .rect_filled(row, egui::CornerRadius::same(4), p.hover);
                }
                ui.painter().text(
                    Pos2::new(row.left() + 10.0, row.center().y),
                    Align2::LEFT_CENTER,
                    if zh { *cn } else { *en },
                    FontId::proportional(12.5),
                    p.text,
                );
                paint_icon(
                    ui.painter(),
                    Rect::from_center_size(
                        Pos2::new(row.right() - 12.0, row.center().y),
                        Vec2::splat(10.0),
                    ),
                    Icon::ChevronRight,
                    p.text3,
                );
                if resp.hovered() {
                    hovered_sub = Some((*sub_id, row));
                }
                y += 24.0;
            }
        }
    }

    if let Some((sub_id, sub_row)) = hovered_sub {
        rt.tree_submenu = Some((sub_id.to_string(), sub_row));
    }

    let mut sub_rect_opt: Option<Rect> = None;
    if let Some((sub_id, anchor_row)) = rt.tree_submenu.clone() {
        let sub_items: Vec<(&str, &str, u8)> = match sub_id.as_str() {
            "new" => vec![("File…", "文件…", 0), ("Directory…", "目录…", 1)],
            "copy-path" => vec![
                ("Absolute Path", "绝对路径", 2),
                ("Relative Path", "相对路径", 3),
                ("Path from Repository Root", "从仓库根目录的相对路径", 201),
            ],
            "git" => {
                if is_dir {
                    vec![("Add", "添加到暂存区", 210), ("Rollback…", "回滚…", 11)]
                } else {
                    vec![
                        ("Add", "添加到暂存区", 210),
                        ("Show Diff", "显示差异", 8),
                        ("Show History", "显示历史", 10),
                        ("Annotate with Git Blame", "显示代码追溯 (Blame)", 211),
                        ("Rollback…", "回滚…", 11),
                    ]
                }
            }
            "open-in" => vec![
                ("Finder", "访达 (Finder)", 6),
                ("Terminal", "终端 (Terminal)", 7),
            ],
            _ => Vec::new(),
        };

        if !sub_items.is_empty() {
            let sub_w = 200.0;
            let sub_h = 12.0 + sub_items.len() as f32 * 24.0;
            let sub_x = if anchor_row.right() + sub_w < screen.right() - 8.0 {
                anchor_row.right() + 2.0
            } else {
                anchor_row.left() - sub_w - 2.0
            };
            let sub_y = anchor_row
                .top()
                .clamp(screen.top() + 8.0, screen.bottom() - sub_h - 8.0);
            let sub_rect = Rect::from_min_size(Pos2::new(sub_x, sub_y), Vec2::new(sub_w, sub_h));
            sub_rect_opt = Some(sub_rect);
            rt.menu_hit_rects.push(sub_rect);
            menu_frame(ui, sub_rect, theme);

            let mut sy = sub_rect.top() + 6.0;
            for (sub_en, sub_cn, sub_code) in sub_items {
                let s_row = Rect::from_min_size(
                    Pos2::new(sub_rect.left() + 4.0, sy),
                    Vec2::new(sub_rect.width() - 8.0, 22.0),
                );
                let s_resp = ui.interact(
                    s_row,
                    ui.scope_id().with(("tsub", sub_code)),
                    Sense::click(),
                );
                if s_resp.hovered() {
                    ui.painter()
                        .rect_filled(s_row, egui::CornerRadius::same(4), p.hover);
                }
                ui.painter().text(
                    Pos2::new(s_row.left() + 10.0, s_row.center().y),
                    Align2::LEFT_CENTER,
                    if zh { sub_cn } else { sub_en },
                    FontId::proportional(12.5),
                    p.text,
                );
                if s_resp.clicked() {
                    action = Some(sub_code);
                }
                sy += 24.0;
            }
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
                        "已复制绝对路径"
                    } else {
                        "Absolute path copied"
                    },
                    ToastKind::Info,
                );
            }
            3 | 201 => {
                ui.ctx().copy_text(rel_path.to_string());
                model.toast(
                    if zh {
                        "已复制相对路径"
                    } else {
                        "Relative path copied"
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
                let prefix = format!("{rel_path}/");
                let changes: Vec<(String, bool)> = model.projects[project_idx]
                    .active_git()
                    .map(|git| {
                        git.changes
                            .iter()
                            .map(|change| (change.path.clone(), change.untracked))
                            .collect()
                    })
                    .unwrap_or_default();
                if is_dir {
                    // `discard_file_changes` removes a directory recursively
                    // when it has no HEAD entry. Expand the directory to its
                    // changed tracked files instead, and never touch untracked
                    // content from a Rollback command.
                    let paths: Vec<String> = changes
                        .iter()
                        .filter(|(path, untracked)| {
                            !untracked
                                && (rel_path.is_empty()
                                    || path == rel_path
                                    || path.starts_with(&prefix))
                        })
                        .map(|(path, _)| path.clone())
                        .collect();
                    if paths.is_empty() {
                        model.toast(
                            crate::tr!(
                                model,
                                "No tracked changes under this folder",
                                "该目录下没有已跟踪的更改"
                            )
                            .to_string(),
                            ToastKind::Warning,
                        );
                    } else {
                        rt.dialog = DialogState::ConfirmDiscard { paths };
                    }
                } else if changes
                    .iter()
                    .any(|(path, untracked)| *untracked && path == rel_path)
                {
                    // Untracked files have a trash-based Delete flow; Rollback
                    // must not permanently unlink them.
                    rt.dialog = DialogState::ConfirmDeleteUntracked {
                        paths: vec![abs_path],
                    };
                } else {
                    rt.dialog = DialogState::ConfirmDiscard {
                        paths: vec![rel_path.to_string()],
                    };
                }
            }
            50 => {
                let paths: Vec<std::path::PathBuf> = model
                    .ui
                    .tree_selection
                    .iter()
                    .map(|p| project.active_root().join(p))
                    .collect();
                rt.dialog = DialogState::ConfirmDeleteUntracked { paths };
            }
            101 | 102 => {
                ui.ctx().copy_text(abs_path.to_string_lossy().to_string());
            }
            104 => {
                rt.search.open = true;
                rt.search.focus_requested = true;
                rt.search.file_mask = rel_path.to_string();
            }
            105 => {
                if let Some(project) = model.projects.get_mut(project_idx) {
                    fn expand_rec(
                        nodes: &[lazecode_fs::tree::TreeNode],
                        expanded: &mut std::collections::HashSet<std::path::PathBuf>,
                    ) {
                        for node in nodes {
                            if node.kind == lazecode_fs::tree::NodeKind::Folder {
                                expanded.insert(node.path.clone());
                                expand_rec(&node.children, expanded);
                            }
                        }
                    }
                    let base = std::path::Path::new(rel_path);
                    project.expanded.insert(base.to_path_buf());
                    let nodes = project.active_tree().nodes.clone();
                    expand_rec(&nodes, &mut project.expanded);
                }
            }
            106 => {
                if let Some(project) = model.projects.get_mut(project_idx) {
                    let base = std::path::Path::new(rel_path);
                    project
                        .expanded
                        .retain(|p| !p.starts_with(base) || p == base);
                    project.expanded.remove(base);
                }
            }
            107 => {
                if let Some(project) = model.projects.get_mut(project_idx) {
                    let root = project.active_root().to_path_buf();
                    project.active_worktree_mut().tree = lazecode_fs::tree::load_tree(&root);
                    model.queue(Command::ReloadGit);
                }
            }
            210 => {
                model.queue(Command::Stage(vec![rel_path.to_string()]));
            }
            211 => {
                if let Ok(id) = model.open_file_in(project_idx, abs_path.clone()) {
                    rt.set_editor_focus(id);
                    let root = model.projects[project_idx].active_root().to_path_buf();
                    let blame = lazecode_git::blame(&root, rel_path).ok();
                    if let Some(tab) = model.projects[project_idx].tab_mut_by_id(id) {
                        if let lazecode_core::TabKind::File(file_tab) = &mut tab.kind {
                            file_tab.annotate = true;
                            if file_tab.blame.is_none() {
                                file_tab.blame = blame;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        close = true;
        rt.tree_submenu = None;
    }

    let dismiss_main = should_dismiss(ui, rect, menu.opened_frame, rt.frame_count);
    let in_sub = sub_rect_opt.is_some_and(|sr| {
        ui.input(|i| i.pointer.hover_pos())
            .is_some_and(|p| sr.contains(p))
    });
    if dismiss_main && !in_sub {
        close = true;
        rt.tree_submenu = None;
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
            let count = project.active_worktree_tabs().count();
            let pos = project
                .active_worktree_tabs()
                .position(|tab| tab.id == tab_id);
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

    let height = items.len() as f32 * MENU_ROW_H + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 200.0, height);
    menu_frame(ui, rect, theme);

    // 稳定版右键菜单没有标题行（对齐 EditorTabBar ctx menu），直接列菜单项。
    let mut action: Option<u8> = None;
    let mut close = false;
    let row_start = rect.top() + 4.0;
    for (i, (label, code, enabled)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, row_start + i as f32 * MENU_ROW_H),
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
        let relative_root = model
            .projects
            .get(project_idx)
            .map(|project| project.active_root().to_path_buf());
        match code {
            0 => model.close_tab(project_idx, tab_id),
            1 => model.close_other_tabs(project_idx, tab_id),
            2 => model.close_tabs_to_right(project_idx, tab_id),
            3 => model.close_unmodified_tabs(project_idx),
            4 => model.close_all_tabs(project_idx),
            8 => model.close_tabs_to_left(project_idx, tab_id),
            5 => {
                ui.ctx().copy_text(path.to_string());
                model.toast(
                    crate::tr!(model, "Path copied", "已复制路径").to_string(),
                    ToastKind::Info,
                );
            }
            6 => {
                let rel = relative_root
                    .as_deref()
                    .and_then(|root| std::path::Path::new(path).strip_prefix(root).ok())
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string());
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
    tab_id: u64,
    terminal_id: u64,
    title: &str,
) {
    let p = theme.palette;
    // 与稳定版 TerminalTabContextMenu 同序：Copy / 分屏 / 关闭 / 标签批量关闭 / 重命名。
    let tab_info = model.projects.get(project_idx).and_then(|project| {
        project.worktrees.iter().find_map(|worktree| {
            worktree
                .tab_by_id(tab_id)
                .map(|tab| (tab.panes.len(), worktree.path.clone()))
        })
    });
    let pane_count = tab_info.as_ref().map_or(0, |(count, _)| *count);
    let tab_path = tab_info.map(|(_, path)| path).unwrap_or_default();
    let can_split = pane_count < lazecode_core::MAX_PANES_PER_TAB;
    let can_close_pane = pane_count > 1;

    #[derive(Clone, Copy)]
    enum Item {
        Row(&'static str, &'static str, u8, bool),
        Sep,
    }
    let items = [
        Item::Row(
            "Copy Working Directory",
            "复制工作目录",
            0,
            tab_path.as_os_str().is_empty(),
        ),
        Item::Sep,
        Item::Row("Split Right", "向右分屏", 1, !can_split),
        Item::Row("Split Down", "向下分屏", 2, !can_split),
        Item::Row("Close Pane", "关闭分屏", 3, !can_close_pane),
        Item::Sep,
        Item::Row("Close", "关闭", 4, false),
        Item::Sep,
        Item::Row("Close Others", "关闭其他", 5, false),
        Item::Row("Close All", "关闭全部", 6, false),
        Item::Sep,
        Item::Row("Rename…", "重命名…", 7, false),
    ];

    let rows = items
        .iter()
        .filter(|item| !matches!(item, Item::Sep))
        .count();
    let seps = items.len() - rows;
    let height = rows as f32 * MENU_ROW_H + seps as f32 * 8.0 + 12.0;
    let rect = popover_rect(rt, screen, menu.anchor, 200.0, height);
    menu_frame(ui, rect, theme);

    let zh = model.settings.language.is_zh();
    let mut action: Option<u8> = None;
    let mut close = false;
    let mut y = rect.top() + 6.0;
    let mut row_index = 0usize;
    for item in &items {
        match item {
            Item::Sep => {
                ui.painter().hline(
                    (rect.left() + 8.0)..=(rect.right() - 8.0),
                    y + 3.5,
                    Stroke::new(1.0, p.border_soft),
                );
                y += 8.0;
            }
            Item::Row(en, cn, code, disabled) => {
                let row = Rect::from_min_size(
                    Pos2::new(rect.left() + 4.0, y),
                    Vec2::new(rect.width() - 8.0, 22.0),
                );
                let response = ui.interact(
                    row,
                    ui.scope_id().with(("ttab_ctx", row_index)),
                    Sense::click(),
                );
                if !*disabled && response.hovered() {
                    ui.painter()
                        .rect_filled(row, egui::CornerRadius::same(4), p.hover);
                }
                ui.painter().text(
                    Pos2::new(row.left() + 10.0, row.center().y),
                    Align2::LEFT_CENTER,
                    if zh { *cn } else { *en },
                    FontId::proportional(12.5),
                    if *disabled { p.text3 } else { p.text },
                );
                if !*disabled && response.clicked() {
                    action = Some(*code);
                }
                y += MENU_ROW_H;
                row_index += 1;
            }
        }
    }

    if let Some(code) = action {
        // Collect the panes a close operation tears down so their cached
        // render state can be dropped afterwards.
        let settings = model.settings.clone();
        let notifier = model.notifier.clone();
        let mut notice: Option<String> = None;
        let mut focus: Option<u64> = None;
        let mut terminal_action: Option<crate::runtime::TerminalCloseAction> = None;
        if let Some(project) = model.projects.get_mut(project_idx) {
            match code {
                0 => {
                    if tab_path.as_os_str().is_empty() {
                        // Disabled row; nothing to copy.
                    } else {
                        ui.ctx().copy_text(tab_path.display().to_string());
                    }
                }
                1 | 2 => {
                    let direction = if code == 1 {
                        lazecode_core::PaneSplit::Row
                    } else {
                        lazecode_core::PaneSplit::Column
                    };
                    match project.split_terminal(terminal_id, direction, &settings, notifier) {
                        Ok(id) => focus = Some(id),
                        Err(err) => notice = Some(err),
                    }
                }
                3 => {
                    terminal_action = Some(crate::runtime::TerminalCloseAction::Pane(terminal_id));
                }
                4 => {
                    terminal_action = Some(crate::runtime::TerminalCloseAction::Tab(tab_id));
                }
                5 => {
                    terminal_action =
                        Some(crate::runtime::TerminalCloseAction::Others(terminal_id));
                }
                6 => {
                    terminal_action = Some(crate::runtime::TerminalCloseAction::All);
                }
                7 => {
                    rt.dialog = DialogState::RenameTerminal {
                        terminal_id,
                        value: title.to_string(),
                        error: None,
                    };
                }
                _ => {}
            }
        }
        if let Some(action) = terminal_action {
            rt.request_terminal_close(model, action);
        }
        if let Some(id) = focus {
            rt.set_terminal_focus(id);
        }
        if code == 0 && !tab_path.as_os_str().is_empty() {
            model.toast(
                crate::tr!(model, "Working directory copied", "已复制工作目录").to_string(),
                ToastKind::Info,
            );
        }
        if let Some(message) = notice {
            model.toast(message, ToastKind::Error);
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
    let unversioned = crate::tr!(model, "Unversioned Files", "未版本控制的文件");
    let ignored = crate::tr!(model, "Ignored Files", "被忽略的文件");
    let is_pseudo = name == unversioned || name == ignored;
    let zh = model.settings.language.is_zh();
    let mut items: Vec<ChangeMenuItem> = Vec::new();
    items.push(change_menu_item(
        if zh {
            "新建变更列表…".to_string()
        } else {
            "New Changelist…".to_string()
        },
        100,
        false,
        true,
    ));
    if !is_default && !is_pseudo {
        items.push(change_menu_item(
            if zh {
                "删除变更列表…".to_string()
            } else {
                "Delete Changelist…".to_string()
            },
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
            if zh {
                "刷新 VCS 更改".to_string()
            } else {
                "Refresh VCS Changes".to_string()
            },
            7,
            false,
            true,
        ));
    }

    let height = items.len() as f32 * MENU_ROW_H + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 260.0, height);
    menu_frame(ui, rect, theme);

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
                    let lists = lazecode_git::Changelists::open(project.active_root());
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

#[allow(clippy::too_many_arguments)]
fn draw_log_sort_options_menu(
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
fn draw_log_path_filter_menu(
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
fn draw_commit_details_file_menu(
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

#[allow(clippy::too_many_arguments)]
fn draw_editor_context_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    path_str: &str,
) {
    let zh = model.settings.language.is_zh();
    let mut items = vec![
        (if zh { "剪切" } else { "Cut" }, 0),
        (if zh { "复制" } else { "Copy" }, 1),
        (if zh { "粘贴" } else { "Paste" }, 2),
        (if zh { "全选" } else { "Select All" }, 3),
        (
            if zh {
                "跳转到行…"
            } else {
                "Go to Line…"
            },
            4,
        ),
        (
            if zh {
                "切换行注释"
            } else {
                "Toggle Line Comment"
            },
            5,
        ),
        (if zh { "文件历史" } else { "File History" }, 6),
        (
            if zh {
                "显示 Git 追溯标注"
            } else {
                "Annotate with Git Blame"
            },
            7,
        ),
    ];
    if lazecode_core::is_json_path(std::path::Path::new(path_str)) {
        items.push((if zh { "格式化 JSON" } else { "Format JSON" }, 8));
        items.push((if zh { "压缩 JSON" } else { "Minify JSON" }, 9));
    }
    let height = items.len() as f32 * MENU_ROW_H + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 200.0, height);
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
            ui.scope_id().with(("ed_ctx", i)),
            label,
            false,
            true,
        ) {
            action = Some(*code);
        }
    }
    let mut close = false;
    if let Some(code) = action {
        let active_tab_id = model.projects[project_idx].active_tab;
        match code {
            0 => {
                if let Some(id) = active_tab_id {
                    if let Some(tab) = model.projects[project_idx].tab_mut_by_id(id) {
                        if let lazecode_core::TabKind::File(file) = &mut tab.kind {
                            if let Some(text) = file.doc.selected_text() {
                                ui.ctx().copy_text(text);
                                file.doc.delete_selection();
                            }
                        }
                    }
                }
            }
            1 => {
                if let Some(id) = active_tab_id {
                    if let Some(tab) = model.projects[project_idx].tab_by_id(id) {
                        if let lazecode_core::TabKind::File(file) = &tab.kind {
                            if let Some(text) = file.doc.selected_text() {
                                ui.ctx().copy_text(text);
                            } else {
                                let line = file.doc.cursor_line_text();
                                ui.ctx().copy_text(line);
                            }
                        }
                    }
                }
            }
            2 => {
                if let Some(id) = active_tab_id {
                    #[cfg(target_os = "macos")]
                    let text = std::process::Command::new("pbpaste")
                        .output()
                        .ok()
                        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                        .unwrap_or_default();
                    #[cfg(not(target_os = "macos"))]
                    let text = String::new();
                    if !text.is_empty() {
                        if let Some(tab) = model.projects[project_idx].tab_mut_by_id(id) {
                            if let lazecode_core::TabKind::File(file) = &mut tab.kind {
                                file.doc.insert_text(&text);
                            }
                        }
                    }
                }
            }
            3 => {
                if let Some(id) = active_tab_id {
                    if let Some(tab) = model.projects[project_idx].tab_mut_by_id(id) {
                        if let lazecode_core::TabKind::File(file) = &mut tab.kind {
                            file.doc.select_all();
                        }
                    }
                }
            }
            4 => {
                model.ui.command_palette_open = true;
                model.ui.palette_query = ":".to_string();
            }
            5 => {
                if let Some(id) = active_tab_id {
                    if let Some(tab) = model.projects[project_idx].tab_mut_by_id(id) {
                        if let lazecode_core::TabKind::File(file) = &mut tab.kind {
                            file.doc.toggle_comment();
                        }
                    }
                }
            }
            6 => {
                let root = model.projects[project_idx].active_root().to_path_buf();
                let rel = std::path::Path::new(path_str)
                    .strip_prefix(&root)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path_str.to_string());
                rt.dialog = DialogState::FileHistory {
                    path: rel,
                    entries: Vec::new(),
                    selected: 0,
                    loaded: false,
                };
            }
            7 => {
                if let Some(id) = active_tab_id {
                    if let Some(tab) = model.projects[project_idx].tab_mut_by_id(id) {
                        if let lazecode_core::TabKind::File(file) = &mut tab.kind {
                            file.annotate = !file.annotate;
                        }
                    }
                }
            }
            8 | 9 => {
                if let Some(id) = active_tab_id {
                    crate::editor::format_json_tab(model, project_idx, id, code == 9);
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

/// Change-file menu codes: 0..10 generic, 50..200 move-to lists, 20/21 untracked,
/// 22..24 new actions, 30..37 extended actions, SEP divider, 100/101
/// changelist headers.
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

struct ChangeMenuCtx {
    project_root: std::path::PathBuf,
    conflicts: Vec<lazecode_git::ConflictEntry>,
    paths: Vec<String>,
    untracked: std::collections::HashSet<String>,
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
    let lists = lazecode_git::Changelists::open(project.active_root());
    let all_lists: Vec<String> = lists.lists().to_vec();
    let untracked: std::collections::HashSet<String> = project
        .active_git()
        .map(|git| {
            git.changes
                .iter()
                .filter(|change| change.untracked)
                .map(|change| change.path.clone())
                .collect()
        })
        .unwrap_or_default();
    let ctx = ChangeMenuCtx {
        project_root: project.active_root().to_path_buf(),
        conflicts: project.conflicts.clone(),
        paths: paths.to_vec(),
        untracked: untracked.clone(),
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
    let all_untracked = paths.iter().all(|path| untracked.contains(path));

    let mut items: Vec<ChangeMenuItem> = Vec::new();
    let zh = model.settings.language.is_zh();
    if all_untracked {
        items.push(change_menu_item(
            if zh {
                if single {
                    "添加到 VCS".to_string()
                } else {
                    format!("添加 {} 个文件到 VCS", paths.len())
                }
            } else {
                if single {
                    "Add to VCS".to_string()
                } else {
                    format!("Add {} Files to VCS", paths.len())
                }
            },
            20,
            false,
            true,
        ));
        if single {
            items.push(change_menu_item(
                if zh {
                    "跳转到源代码".to_string()
                } else {
                    "Jump to Source".to_string()
                },
                1,
                false,
                true,
            ));
        }
        change_menu_sep(&mut items);
        items.push(change_menu_item(
            if zh {
                if single {
                    "删除…".to_string()
                } else {
                    format!("删除 {} 个文件…", paths.len())
                }
            } else {
                if single {
                    "Delete…".to_string()
                } else {
                    format!("Delete {} Files…", paths.len())
                }
            },
            21,
            true,
            true,
        ));
        return (items, ctx);
    }

    items.push(change_menu_item(
        if zh {
            if single {
                "提交文件…".to_string()
            } else {
                "提交所选文件…".to_string()
            }
        } else {
            if single {
                "Commit File...".to_string()
            } else {
                "Commit Selected Files…".to_string()
            }
        },
        3,
        false,
        true,
    ));
    if single {
        items.push(change_menu_item(
            if zh {
                "查看差异".to_string()
            } else {
                "Show Diff".to_string()
            },
            0,
            false,
            true,
        ));
        items.push(change_menu_item(
            if zh {
                "在新标签页查看差异".to_string()
            } else {
                "Show Diff in a New Tab".to_string()
            },
            30,
            false,
            true,
        ));
        items.push(change_menu_item(
            if zh {
                "与本地比较".to_string()
            } else {
                "Compare with Local".to_string()
            },
            31,
            false,
            true,
        ));
        items.push(change_menu_item(
            if zh {
                "与本地的前一版本比较".to_string()
            } else {
                "Compare Before with Local".to_string()
            },
            22,
            false,
            true,
        ));
        items.push(change_menu_item(
            if zh {
                "跳转到源代码".to_string()
            } else {
                "Jump to Source".to_string()
            },
            1,
            false,
            true,
        ));
        items.push(change_menu_item(
            if zh {
                "打开仓库版本".to_string()
            } else {
                "Open Repository Version".to_string()
            },
            32,
            false,
            true,
        ));
        if conflicted {
            items.push(change_menu_item(
                if zh {
                    "解决…".to_string()
                } else {
                    "Resolve…".to_string()
                },
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
    items.push(change_menu_item(
        if model.settings.language.is_zh() {
            "拣选选中更改".to_string()
        } else {
            "Cherry-Pick Selected Changes".to_string()
        },
        33,
        false,
        true,
    ));
    items.push(change_menu_item(
        if model.settings.language.is_zh() {
            "提取选中更改到独立提交…".to_string()
        } else {
            "Extract Selected Changes to Separate Commit…".to_string()
        },
        34,
        false,
        true,
    ));
    items.push(change_menu_item(
        if model.settings.language.is_zh() {
            "丢弃选中更改".to_string()
        } else {
            "Drop Selected Changes".to_string()
        },
        35,
        true,
        true,
    ));
    if single {
        items.push(change_menu_item(
            if zh {
                "从本地更改创建补丁…".to_string()
            } else {
                "Create Patch from Local Changes...".to_string()
            },
            23,
            false,
            true,
        ));
        items.push(change_menu_item(
            if model.settings.language.is_zh() {
                "从版本获取…".to_string()
            } else {
                "Get from Revision…".to_string()
            },
            36,
            false,
            true,
        ));
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
        items.push(change_menu_item(
            if model.settings.language.is_zh() {
                "显示与父提交的差异".to_string()
            } else {
                "Show Changes to Parents".to_string()
            },
            37,
            false,
            true,
        ));
    }
    change_menu_sep(&mut items);
    for (idx, list_name) in all_lists.iter().enumerate() {
        // 稳定版规则：所选文件已全部在该列表中 → 禁用（多选不再跳过）。
        let all_in_list = paths.iter().all(|path| lists.list_of(path) == *list_name);
        items.push(change_menu_item(
            if zh {
                format!("移动到 {list_name}")
            } else {
                format!("Move to {list_name}")
            },
            50 + idx as u8,
            false,
            !all_in_list,
        ));
    }
    if single {
        items.push(change_menu_item(
            if zh {
                "复制补丁到剪贴板".to_string()
            } else {
                "Copy as Patch to Clipboard".to_string()
            },
            5,
            false,
            true,
        ));
    }
    items.push(change_menu_item(
        if zh {
            "贮藏更改…".to_string()
        } else {
            "Shelve Changes...".to_string()
        },
        6,
        false,
        true,
    ));
    items.push(change_menu_item(
        if zh {
            "刷新 VCS 更改".to_string()
        } else {
            "Refresh VCS Changes".to_string()
        },
        7,
        false,
        true,
    ));
    items.push(change_menu_item(
        if zh {
            "新建变更列表…".to_string()
        } else {
            "New Changelist…".to_string()
        },
        8,
        false,
        true,
    ));
    (items, ctx)
}

fn discardable_paths(ctx: &ChangeMenuCtx) -> Vec<String> {
    // The untracked branch has its own Delete-to-trash confirmation. A mixed
    // selection must not silently delete an untracked file through Rollback.
    ctx.paths
        .iter()
        .filter(|path| !ctx.untracked.contains(*path))
        .cloned()
        .collect()
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
            if let Err(err) = model.open_file_in(project_idx, abs) {
                model.toast(err, ToastKind::Error);
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
            // 统一确认弹窗（对齐稳定版 rollbackFileConfirm）。Mixed
            // selections must not delete untracked files via Rollback.
            let paths = discardable_paths(ctx);
            if paths.is_empty() {
                model.toast(
                    crate::tr!(
                        model,
                        "Use Delete for untracked files",
                        "未跟踪文件请使用「删除」"
                    )
                    .to_string(),
                    ToastKind::Warning,
                );
            } else {
                rt.dialog = DialogState::ConfirmDiscard { paths };
            }
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
        32 => model.queue(Command::OpenRepositoryVersion {
            path: ctx.first.clone(),
        }),
        30 => model.queue(Command::OpenDiffNewTab(ctx.first.clone())),
        31 => model.queue(Command::OpenIndexDiff(ctx.first.clone())),
        33 => {
            rt.dialog = DialogState::CherryPickChanges {
                paths: ctx.paths.clone(),
                target: String::new(),
                error: None,
            };
        }
        34 => {
            rt.dialog = DialogState::ExtractChanges {
                paths: ctx.paths.clone(),
                value: String::new(),
                error: None,
            };
        }
        35 => {
            let paths = discardable_paths(ctx);
            if paths.is_empty() {
                model.toast(
                    crate::tr!(
                        model,
                        "Use Delete for untracked files",
                        "未跟踪文件请使用「删除」"
                    )
                    .to_string(),
                    ToastKind::Warning,
                );
            } else {
                rt.dialog = DialogState::ConfirmDiscard { paths };
            }
        }
        36 => {
            rt.dialog = DialogState::GetFileFromRevision {
                path: ctx.first.clone(),
                revision: "HEAD".to_string(),
                error: None,
            };
        }
        37 => {
            let project = &model.projects[project_idx];
            let head_id = project
                .git
                .as_ref()
                .and_then(|git| git.head.as_ref())
                .map(|head| head.id.as_str());
            let head_row = head_id.and_then(|id| project.log.iter().find(|commit| commit.id == id));
            let parent = match head_row {
                Some(row) => row.parents.first().cloned(),
                // Log not loaded/filtered: let git resolve HEAD^ itself.
                None => Some("HEAD^".to_string()),
            };
            if let Some(parent) = parent {
                model.queue(Command::OpenCompareWithLocalDiff {
                    commit: parent,
                    path: ctx.first.clone(),
                });
            } else {
                model.toast(
                    crate::tr!(
                        model,
                        "The current branch has no parent commit",
                        "当前分支没有父提交"
                    )
                    .to_string(),
                    ToastKind::Warning,
                );
            }
        }
        idx if (50..=200).contains(&idx) => {
            let target_idx = (idx - 50) as usize;
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
            Pos2::new(rect.left() + 4.0, row_start + i as f32 * MENU_ROW_H),
            Vec2::new(rect.width() - 8.0, MENU_ROW_H),
        );
        if item.code == CHANGE_MENU_SEP {
            ui.painter().line_segment(
                [
                    Pos2::new(row.left() + 8.0, row.center().y),
                    Pos2::new(row.right() - 8.0, row.center().y),
                ],
                egui::Stroke::new(1.0, p.menu_border),
            );
            continue;
        }
        if menu_row(
            ui,
            theme,
            row,
            ui.scope_id().with((id_salt, i)),
            &item.label,
            item.danger,
            item.enabled,
        ) {
            action = Some(item.code);
        }
    }
    action
}

#[allow(clippy::too_many_arguments)]
/// 提交抽屉 ⋮ 扩展入口：稳定版抽屉没有这些按钮，保留功能入口
/// （Stash Changes… / Stash Manager…）。
fn draw_commit_extras_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
) {
    let items = [
        (crate::tr!(model, "Stash Changes…", "贮藏更改…"), 0u8, false),
        (crate::tr!(model, "Stash Manager…", "贮藏管理器…"), 1, false),
        (crate::tr!(model, "Shelf Manager…", "架藏管理器…"), 2, false),
        (
            crate::tr!(model, "Import into Shelf…", "导入架藏…"),
            3,
            false,
        ),
    ];
    let height = items.len() as f32 * MENU_ROW_H + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 220.0, height);
    menu_frame(ui, rect, theme);
    let mut picked: Option<u8> = None;
    for (i, (label, code, danger)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, rect.top() + 4.0 + i as f32 * MENU_ROW_H),
            Vec2::new(rect.width() - 8.0, MENU_ROW_H),
        );
        if menu_row(
            ui,
            theme,
            row,
            ui.scope_id().with(("cextras", i)),
            label,
            *danger,
            true,
        ) {
            picked = Some(*code);
        }
    }
    let mut close = false;
    match picked {
        Some(0) => {
            let has_untracked = model
                .active_project()
                .is_some_and(|project| project.changes().iter().any(|change| change.untracked));
            rt.dialog = crate::runtime::DialogState::StashCreate {
                message: String::new(),
                include_untracked: has_untracked,
                error: None,
            };
            close = true;
        }
        Some(1) => {
            rt.dialog = crate::runtime::DialogState::StashManager {
                stashes: Vec::new(),
                selected: 0,
                error: None,
                loaded: false,
            };
            close = true;
        }
        Some(2) => {
            rt.dialog = crate::runtime::DialogState::ShelfManager {
                shelves: Vec::new(),
                selected: 0,
                error: None,
                loaded: false,
            };
            close = true;
        }
        Some(3) => {
            #[cfg(target_os = "macos")]
            {
                if let Ok(output) = std::process::Command::new("osascript")
                    .arg("-e")
                    .arg("POSIX path of (choose file with prompt \"Select patch file to import into Shelf\")")
                    .output()
                {
                    if output.status.success() {
                        let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        if !path_str.is_empty() {
                            let root = model
                                .active_project()
                                .map(|p| p.active_root().to_path_buf())
                                .unwrap_or_default();
                            match lazecode_git::shelf_import_patch(&root, std::path::Path::new(&path_str), None) {
                                Ok(name) => {
                                    model.toast(
                                        if model.settings.language.is_zh() {
                                            format!("已导入架藏: {name}")
                                        } else {
                                            format!("Imported shelf: {name}")
                                        },
                                        lazecode_core::ToastKind::Success,
                                    );
                                }
                                Err(err) => model.toast(err.to_string(), lazecode_core::ToastKind::Error),
                            }
                        }
                    }
                }
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
/// 贮藏条目右键菜单：弹出 / 应用 / Unstash… / 丢弃 / 清空 / 显示差异。
#[allow(clippy::too_many_arguments)]
fn draw_stash_entry_menu(
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
fn draw_stash_file_menu(
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

fn draw_commit_view_options_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
) {
    let p = theme.palette;
    let menu_w = 210.0;
    let menu_h = 100.0;
    let rect = popover_rect(rt, screen, menu.anchor, menu_w, menu_h);
    menu_frame(ui, rect, theme);

    ui.painter().text(
        Pos2::new(rect.left() + 12.0, rect.top() + 12.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Group By", "分组依据"),
        FontId::proportional(10.5),
        p.text3,
    );

    let item_tree = Rect::from_min_size(
        Pos2::new(rect.left() + 4.0, rect.top() + 22.0),
        Vec2::new(rect.width() - 8.0, 24.0),
    );
    let mut close = false;
    if menu_row(
        ui,
        theme,
        item_tree,
        ui.scope_id().with("commit-view-tree"),
        &*crate::tr!(model, "Directory", "目录"),
        false,
        true,
    ) {
        rt.commit_tree_view = !rt.commit_tree_view;
        close = true;
    }
    if rt.commit_tree_view {
        ui.painter().text(
            Pos2::new(item_tree.right() - 34.0, item_tree.center().y),
            Align2::RIGHT_CENTER,
            "✓",
            FontId::proportional(11.0),
            p.menu_item_text,
        );
    }
    ui.painter().text(
        Pos2::new(item_tree.right() - 10.0, item_tree.center().y),
        Align2::RIGHT_CENTER,
        "^P",
        FontId::proportional(10.5),
        p.text3,
    );

    ui.painter().hline(
        (rect.left() + 6.0)..=(rect.right() - 6.0),
        rect.top() + 52.0,
        Stroke::new(1.0, p.border_soft),
    );

    ui.painter().text(
        Pos2::new(rect.left() + 12.0, rect.top() + 62.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Show", "显示"),
        FontId::proportional(10.5),
        p.text3,
    );

    let item_ignored = Rect::from_min_size(
        Pos2::new(rect.left() + 4.0, rect.top() + 70.0),
        Vec2::new(rect.width() - 8.0, 24.0),
    );
    if menu_row(
        ui,
        theme,
        item_ignored,
        ui.scope_id().with("commit-view-ignored"),
        &*crate::tr!(model, "Show Ignored Files", "显示被忽略的文件"),
        false,
        true,
    ) {
        rt.commit_show_ignored = !rt.commit_show_ignored;
        close = true;
    }
    if rt.commit_show_ignored {
        ui.painter().text(
            Pos2::new(item_ignored.right() - 12.0, item_ignored.center().y),
            Align2::RIGHT_CENTER,
            "✓",
            FontId::proportional(11.0),
            p.menu_item_text,
        );
    }

    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}
fn draw_compare_view_options_menu(
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
    let menu_w = 180.0;
    let menu_h = 56.0;
    let rect = popover_rect(rt, screen, menu.anchor, menu_w, menu_h);
    menu_frame(ui, rect, theme);

    ui.painter().text(
        Pos2::new(rect.left() + 10.0, rect.top() + 11.0),
        Align2::LEFT_CENTER,
        if zh { "分组方式" } else { "Group by" },
        FontId::proportional(10.5),
        p.text3,
    );

    let item = Rect::from_min_size(
        Pos2::new(rect.left() + 4.0, rect.top() + 20.0),
        Vec2::new(menu_w - 8.0, 24.0),
    );
    let mut close = false;
    let is_grouped = model
        .projects
        .get(project_idx)
        .and_then(|p| p.compare_session.as_ref())
        .map(|s| s.group_by_dir)
        .unwrap_or(false);
    if menu_row(
        ui,
        theme,
        item,
        ui.scope_id().with("cmp-view-dir"),
        if zh { "目录" } else { "Directory" },
        false,
        true,
    ) {
        if let Some(session) = model
            .projects
            .get_mut(project_idx)
            .and_then(|p| p.compare_session.as_mut())
        {
            session.group_by_dir = !session.group_by_dir;
        }
        close = true;
    }
    if is_grouped {
        ui.painter().text(
            Pos2::new(item.left() + 10.0, item.center().y),
            Align2::LEFT_CENTER,
            "✓",
            FontId::proportional(11.0),
            p.menu_item_text,
        );
    }
    ui.painter().text(
        Pos2::new(item.right() - 8.0, item.center().y),
        Align2::RIGHT_CENTER,
        "^P",
        FontId::proportional(10.5),
        p.text3,
    );

    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

/// 架藏右键菜单（稳定版 Shelf 页签）：恢复 / 补丁 / 重命名 / 删除。
#[allow(clippy::too_many_arguments)]
fn draw_shelf_context_menu(
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

    let height = items.len() as f32 * MENU_ROW_H + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 260.0, height);
    menu_frame(ui, rect, theme);

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
    let current_branch = model.projects[index].branch();
    let mut query = model.projects[index].branch_query.clone();
    let query_lower = query.trim().to_lowercase();
    let has_filter = !query_lower.is_empty();

    let all_branches = model.projects[index].branches.clone();
    let all_tags = model.projects[index].tags.clone();

    let local_branches: Vec<lazecode_git::BranchInfo> = all_branches
        .iter()
        .filter(|b| {
            !b.remote && (query_lower.is_empty() || b.name.to_lowercase().contains(&query_lower))
        })
        .cloned()
        .collect();

    let remote_branches: Vec<lazecode_git::BranchInfo> = all_branches
        .iter()
        .filter(|b| {
            b.remote && (query_lower.is_empty() || b.name.to_lowercase().contains(&query_lower))
        })
        .cloned()
        .collect();

    let filtered_tags: Vec<String> = all_tags
        .iter()
        .filter(|t| query_lower.is_empty() || t.to_lowercase().contains(&query_lower))
        .cloned()
        .collect();

    let recent_branches: Vec<lazecode_git::BranchInfo> = {
        let mut list = Vec::new();
        if let Some(cur) = local_branches.iter().find(|b| b.name == current_branch) {
            list.push(cur.clone());
        }
        for b in &local_branches {
            if b.name != current_branch && list.len() < 4 {
                list.push(b.clone());
            }
        }
        list
    };

    // 下拉菜单最大高度为app窗口高度的80%，内容未超限时用内容撑起高度
    let top_h = if has_filter { 42.0 } else { 148.0 };
    let mut sections_h = 0.0f32;
    if !has_filter && !recent_branches.is_empty() {
        sections_h += 22.0;
        if !rt.branch_sections_collapsed.contains("recent") {
            sections_h += recent_branches.len() as f32 * 26.0 + 4.0;
        }
    }
    if !local_branches.is_empty() {
        sections_h += 22.0;
        if has_filter || !rt.branch_sections_collapsed.contains("local") {
            sections_h += local_branches.len() as f32 * 26.0 + 4.0;
        }
    }
    if !remote_branches.is_empty() {
        sections_h += 22.0;
        if has_filter || !rt.branch_sections_collapsed.contains("remote") {
            sections_h += remote_branches.len() as f32 * 26.0 + 4.0;
        }
    }
    if !filtered_tags.is_empty() {
        sections_h += 22.0;
        if has_filter || !rt.branch_sections_collapsed.contains("tags") {
            sections_h += filtered_tags.len() as f32 * 26.0 + 4.0;
        }
    }
    let max_height = (screen.height() * 0.8).max(180.0);
    let needed_height = top_h + sections_h + 16.0;
    let height = needed_height.clamp(140.0, max_height);
    let rect = popover_rect(rt, screen, menu.anchor, 330.0, height);
    menu_frame(ui, rect, theme);

    let mut close = false;

    // 1. Search Bar Header
    let input_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 10.0, rect.top() + 8.0),
        Vec2::new(rect.width() - 20.0, 28.0),
    );
    ui.painter()
        .rect_filled(input_rect, egui::CornerRadius::same(5), p.panel2);
    ui.painter().rect_stroke(
        input_rect,
        egui::CornerRadius::same(5),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );

    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(input_rect.left() + 14.0, input_rect.center().y),
            Vec2::splat(12.0),
        ),
        Icon::Search,
        p.text3,
    );

    let gear_rect = Rect::from_center_size(
        Pos2::new(input_rect.right() - 14.0, input_rect.center().y),
        Vec2::splat(18.0),
    );
    let gear_resp = ui.interact(
        gear_rect,
        ui.scope_id().with("branch-menu-gear"),
        Sense::click(),
    );
    if gear_resp.hovered() {
        ui.painter()
            .rect_filled(gear_rect, egui::CornerRadius::same(3), p.hover);
    }
    paint_icon(
        ui.painter(),
        gear_rect.shrink(3.0),
        Icon::Settings,
        if gear_resp.hovered() { p.text } else { p.text3 },
    );
    if gear_resp.clicked() {
        rt.settings_open = true;
        close = true;
    }

    let text_rect = Rect::from_min_max(
        Pos2::new(input_rect.left() + 26.0, input_rect.top() + 2.0),
        Pos2::new(input_rect.right() - 26.0, input_rect.bottom() - 2.0),
    );
    let query_resp = ui.put(
        text_rect,
        TextEdit::singleline(&mut query)
            .frame(egui::Frame::NONE)
            .align(egui::Align2::LEFT_CENTER)
            .hint_text(crate::tr!(
                model,
                "Search for branches and actions",
                "过滤分支与操作"
            ))
            .font(FontId::proportional(12.0)),
    );
    if query_resp.changed() {
        model.projects[index].branch_query = query.clone();
        rt.branch_submenu = None;
    }
    let mut cur_y = input_rect.bottom() + 6.0;

    // 2. Top Action Rows (when filter is empty)
    if !has_filter {
        struct TopAction {
            label: &'static str,
            label_zh: &'static str,
            icon: Icon,
            icon_accent: bool,
            shortcut: &'static str,
            id: u8,
        }
        let actions = [
            TopAction {
                label: "Update Project...",
                label_zh: "更新项目...",
                icon: Icon::ArrowDown,
                icon_accent: true,
                shortcut: "⌘T",
                id: 1,
            },
            TopAction {
                label: "Commit...",
                label_zh: "提交...",
                icon: Icon::Commit,
                icon_accent: false,
                shortcut: "⌘K",
                id: 2,
            },
            TopAction {
                label: "Push...",
                label_zh: "推送...",
                icon: Icon::ArrowUp,
                icon_accent: false,
                shortcut: "⇧⌘K",
                id: 3,
            },
            TopAction {
                label: "New Branch...",
                label_zh: "新建分支...",
                icon: Icon::Plus,
                icon_accent: false,
                shortcut: "⌥N",
                id: 4,
            },
        ];

        for act in &actions {
            let row = Rect::from_min_size(
                Pos2::new(rect.left() + 6.0, cur_y),
                Vec2::new(rect.width() - 12.0, 24.0),
            );
            let resp = ui.interact(row, ui.scope_id().with(("top-act", act.id)), Sense::click());
            if resp.hovered() {
                ui.painter()
                    .rect_filled(row, egui::CornerRadius::same(3), p.hover);
            }
            paint_icon(
                ui.painter(),
                Rect::from_center_size(
                    Pos2::new(row.left() + 14.0, row.center().y),
                    Vec2::splat(13.0),
                ),
                act.icon,
                if act.icon_accent { p.accent } else { p.text2 },
            );
            ui.painter().text(
                Pos2::new(row.left() + 28.0, row.center().y),
                Align2::LEFT_CENTER,
                crate::tr!(model, act.label, act.label_zh),
                FontId::proportional(12.5),
                p.text,
            );
            if !act.shortcut.is_empty() {
                ui.painter().text(
                    Pos2::new(row.right() - 8.0, row.center().y),
                    Align2::RIGHT_CENTER,
                    act.shortcut,
                    FontId::proportional(11.0),
                    p.text3,
                );
            }
            if resp.clicked() {
                match act.id {
                    1 => {
                        rt.dialog = DialogState::Pull {
                            mode: lazecode_git::PullMode::Merge,
                            status: None,
                        };
                        close = true;
                    }
                    2 => {
                        model.ui.sidebar_view = lazecode_core::SidebarView::Git;
                        rt.commit_tab = crate::runtime::CommitTab::Commit;
                        close = true;
                    }
                    3 => {
                        if let Some(project) = model.active_project() {
                            rt.dialog =
                                push_dialog_state(project, "origin".into(), current_branch.clone());
                        }
                        close = true;
                    }
                    4 => {
                        rt.dialog = DialogState::NewBranch {
                            value: String::new(),
                            start_point: None,
                            checkout: true,
                            error: None,
                        };
                        close = true;
                    }
                    _ => {}
                }
            }
            cur_y += 24.0;
        }

        ui.painter().line_segment(
            [
                Pos2::new(rect.left() + 6.0, cur_y + 3.0),
                Pos2::new(rect.right() - 6.0, cur_y + 3.0),
            ],
            Stroke::new(1.0, p.border),
        );
        cur_y += 8.0;
    }

    // 3. Scrollable Sections (Recent, Local, Remote, Tags)
    let scroll_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 6.0, cur_y),
        Pos2::new(rect.right() - 6.0, rect.bottom() - 6.0),
    );

    let mut hovered_branch_row: Option<(lazecode_git::BranchInfo, Rect)> = None;
    let mut delete_tag: Option<String> = None;
    let mut checkout_tag: Option<String> = None;

    super::app_ui::region(ui, scroll_rect, "branch-menu-sections", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("branch-menu-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut draw_section =
                    |title: &str, branches: &[lazecode_git::BranchInfo], sec_key: &'static str| {
                        if branches.is_empty() {
                            return;
                        }
                        let is_collapsed =
                            !has_filter && rt.branch_sections_collapsed.contains(sec_key);
                        let (hdr_rect, hdr_resp) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 20.0),
                            Sense::click(),
                        );
                        if hdr_resp.hovered() {
                            ui.painter().rect_filled(
                                hdr_rect,
                                egui::CornerRadius::same(3),
                                p.hover,
                            );
                        }
                        paint_icon(
                            ui.painter(),
                            Rect::from_center_size(
                                Pos2::new(hdr_rect.left() + 6.0, hdr_rect.center().y),
                                Vec2::splat(9.0),
                            ),
                            if is_collapsed {
                                Icon::ChevronRight
                            } else {
                                Icon::ChevronDown
                            },
                            p.text3,
                        );
                        ui.painter().text(
                            Pos2::new(hdr_rect.left() + 16.0, hdr_rect.center().y),
                            Align2::LEFT_CENTER,
                            title,
                            FontId::proportional(11.0),
                            p.text3,
                        );
                        if hdr_resp.clicked() {
                            if rt.branch_sections_collapsed.contains(sec_key) {
                                rt.branch_sections_collapsed.remove(sec_key);
                            } else {
                                rt.branch_sections_collapsed.insert(sec_key.to_string());
                            }
                        }

                        if is_collapsed {
                            ui.add_space(2.0);
                            return;
                        }

                        for b in branches {
                            let (row_rect, resp) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 26.0),
                                Sense::click(),
                            );
                            let is_current = b.name == current_branch;
                            let row_screen = Rect::from_min_size(row_rect.min, row_rect.size());
                            let active_submenu = rt
                                .branch_submenu
                                .as_ref()
                                .map(|s| s.anchor == row_screen)
                                .unwrap_or(false);

                            if is_current {
                                ui.painter().rect_filled(
                                    row_rect,
                                    egui::CornerRadius::same(3),
                                    Color32::from_rgb(33, 66, 131),
                                );
                            } else if resp.hovered() || active_submenu {
                                ui.painter().rect_filled(
                                    row_rect,
                                    egui::CornerRadius::same(3),
                                    p.hover,
                                );
                            }

                            if is_current {
                                paint_icon(
                                    ui.painter(),
                                    Rect::from_center_size(
                                        Pos2::new(row_rect.left() + 10.0, row_rect.center().y),
                                        Vec2::splat(12.0),
                                    ),
                                    Icon::Tag,
                                    Color32::from_rgb(229, 192, 123),
                                );
                            } else {
                                paint_icon(
                                    ui.painter(),
                                    Rect::from_center_size(
                                        Pos2::new(row_rect.left() + 10.0, row_rect.center().y),
                                        Vec2::splat(12.0),
                                    ),
                                    Icon::Branch,
                                    if b.remote { p.text3 } else { p.text2 },
                                );
                            }

                            let name_x = row_rect.left() + 22.0;
                            let mut end_x = row_rect.right() - 18.0;

                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(row_rect.right() - 8.0, row_rect.center().y),
                                    Vec2::splat(10.0),
                                ),
                                Icon::ChevronRight,
                                p.text3,
                            );

                            if let Some(ref tr) = b.tracking {
                                let tr_font = FontId::proportional(11.0);
                                let tr_w = ui
                                    .painter()
                                    .layout_no_wrap(tr.clone(), tr_font.clone(), p.text3)
                                    .size()
                                    .x;
                                end_x -= tr_w + 6.0;
                                ui.painter().text(
                                    Pos2::new(end_x, row_rect.center().y),
                                    Align2::LEFT_CENTER,
                                    tr,
                                    tr_font,
                                    p.text3,
                                );
                            }

                            if b.ahead > 0 {
                                let badge_text = format!("↗{}", b.ahead);
                                let badge_font = FontId::proportional(11.0);
                                let badge_w = ui
                                    .painter()
                                    .layout_no_wrap(
                                        badge_text.clone(),
                                        badge_font.clone(),
                                        Color32::from_rgb(46, 160, 67),
                                    )
                                    .size()
                                    .x;
                                end_x -= badge_w + 6.0;
                                ui.painter().text(
                                    Pos2::new(end_x, row_rect.center().y),
                                    Align2::LEFT_CENTER,
                                    badge_text,
                                    badge_font,
                                    Color32::from_rgb(46, 160, 67),
                                );
                            }

                            text_clipped(
                                ui.painter(),
                                Rect::from_min_max(
                                    Pos2::new(name_x, row_rect.top()),
                                    Pos2::new(end_x.max(name_x + 10.0), row_rect.bottom()),
                                ),
                                &b.name,
                                FontId::proportional(12.5),
                                if is_current {
                                    Color32::WHITE
                                } else if b.remote {
                                    p.text2
                                } else {
                                    p.text
                                },
                                Align2::LEFT_CENTER,
                            );

                            let row_screen = Rect::from_min_size(row_rect.min, row_rect.size());
                            if resp.clicked() {
                                let _ = sec_key;
                                hovered_branch_row = Some((b.clone(), row_screen));
                            }
                        }
                        ui.add_space(4.0);
                    };

                if !has_filter && !recent_branches.is_empty() {
                    draw_section(
                        crate::tr!(model, "Recent", "最近"),
                        &recent_branches,
                        "recent",
                    );
                }

                draw_section(crate::tr!(model, "Local", "本地"), &local_branches, "local");

                draw_section(
                    crate::tr!(model, "Remote", "远程"),
                    &remote_branches,
                    "remote",
                );

                if !filtered_tags.is_empty() {
                    let is_collapsed = !has_filter && rt.branch_sections_collapsed.contains("tags");
                    let (hdr_rect, hdr_resp) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::click());
                    if hdr_resp.hovered() {
                        ui.painter()
                            .rect_filled(hdr_rect, egui::CornerRadius::same(3), p.hover);
                    }
                    paint_icon(
                        ui.painter(),
                        Rect::from_center_size(
                            Pos2::new(hdr_rect.left() + 6.0, hdr_rect.center().y),
                            Vec2::splat(9.0),
                        ),
                        if is_collapsed {
                            Icon::ChevronRight
                        } else {
                            Icon::ChevronDown
                        },
                        p.text3,
                    );
                    ui.painter().text(
                        Pos2::new(hdr_rect.left() + 16.0, hdr_rect.center().y),
                        Align2::LEFT_CENTER,
                        crate::tr!(model, "Tags", "标签"),
                        FontId::proportional(11.0),
                        p.text3,
                    );
                    if hdr_resp.clicked() {
                        if rt.branch_sections_collapsed.contains("tags") {
                            rt.branch_sections_collapsed.remove("tags");
                        } else {
                            rt.branch_sections_collapsed.insert("tags".to_string());
                        }
                    }

                    if !is_collapsed {
                        for tag in &filtered_tags {
                            let (row_rect, resp) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 26.0),
                                Sense::click(),
                            );
                            if resp.hovered() {
                                ui.painter().rect_filled(
                                    row_rect,
                                    egui::CornerRadius::same(3),
                                    p.hover,
                                );
                            }

                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(row_rect.left() + 10.0, row_rect.center().y),
                                    Vec2::splat(12.0),
                                ),
                                Icon::Tag,
                                Color32::from_rgb(229, 192, 123),
                            );

                            text_clipped(
                                ui.painter(),
                                Rect::from_min_max(
                                    Pos2::new(row_rect.left() + 22.0, row_rect.top()),
                                    Pos2::new(row_rect.right() - 26.0, row_rect.bottom()),
                                ),
                                tag,
                                FontId::proportional(12.5),
                                p.text,
                                Align2::LEFT_CENTER,
                            );

                            let trash_rect = Rect::from_center_size(
                                Pos2::new(row_rect.right() - 10.0, row_rect.center().y),
                                Vec2::splat(16.0),
                            );
                            let trash_resp = ui.interact(
                                trash_rect,
                                ui.scope_id().with(("del-tag", tag)),
                                Sense::click(),
                            );
                            let trash_color = if trash_resp.hovered() {
                                ui.painter().rect_filled(
                                    trash_rect,
                                    egui::CornerRadius::same(3),
                                    p.hover,
                                );
                                p.rose
                            } else {
                                p.text3
                            };
                            paint_icon(
                                ui.painter(),
                                trash_rect.shrink(3.0),
                                Icon::Trash,
                                trash_color,
                            );
                            if trash_resp.clicked() {
                                delete_tag = Some(tag.clone());
                            } else if resp.secondary_clicked() {
                                rt.open_menu(
                                    MenuKind::TagContext { project: index },
                                    row_rect,
                                    Some(tag.clone()),
                                );
                                close = true;
                            } else if resp.clicked() {
                                checkout_tag = Some(tag.clone());
                            }
                        }
                    }
                    ui.add_space(4.0);
                }
            });
    });

    if let Some((b, row_rect)) = hovered_branch_row {
        rt.branch_submenu = Some(crate::runtime::BranchSubmenuState {
            branch_name: b.name.clone(),
            is_current: b.name == current_branch,
            is_remote: b.remote,
            anchor: row_rect,
        });
    }

    if let Some(tag) = delete_tag {
        model.queue(Command::DeleteTag(tag));
        close = true;
    }
    if let Some(tag) = checkout_tag {
        model.queue(Command::CheckoutRevision(tag));
        close = true;
    }

    // 4. Cascade Submenu Rendering
    let mut sub_rect_opt: Option<Rect> = None;
    if let Some(ref sub) = rt.branch_submenu.clone() {
        let bname = sub.branch_name.clone();
        let is_curr = sub.is_current;
        let is_rem = sub.is_remote;

        enum SubItem {
            Row(&'static str, &'static str, u8, bool, &'static str),
            Sep,
        }

        let items: Vec<SubItem> = if is_curr {
            vec![
                SubItem::Row("New Branch from", "基于此新建分支", 1, false, ""),
                SubItem::Row(
                    "Show Diff with Working Tree",
                    "与工作区比较差异",
                    2,
                    false,
                    "",
                ),
                SubItem::Sep,
                SubItem::Row("Update", "更新", 3, false, ""),
                SubItem::Row("Push...", "推送...", 4, false, ""),
                SubItem::Sep,
                SubItem::Row("Rename...", "重命名...", 5, false, "F2"),
            ]
        } else if is_rem {
            vec![
                SubItem::Row("Checkout", "检出", 10, false, ""),
                SubItem::Row("New Branch from", "基于此新建分支", 1, false, ""),
                SubItem::Row("Checkout and Rebase onto", "检出并变基到", 11, false, ""),
                SubItem::Sep,
                SubItem::Row("Compare with Current", "与当前分支比较", 12, false, ""),
                SubItem::Row(
                    "Show Diff with Working Tree",
                    "与工作区比较差异",
                    2,
                    false,
                    "",
                ),
                SubItem::Sep,
                SubItem::Row("Rebase onto", "变基到当前分支", 13, false, ""),
                SubItem::Row("Merge into current", "合并到当前分支", 14, false, ""),
                SubItem::Sep,
                SubItem::Row(
                    "Pull into current Using Rebase",
                    "使用变基拉取到当前分支",
                    15,
                    false,
                    "",
                ),
                SubItem::Row(
                    "Pull into current Using Merge",
                    "使用合并拉取到当前分支",
                    16,
                    false,
                    "",
                ),
                SubItem::Sep,
                SubItem::Row("Delete", "删除", 20, true, ""),
            ]
        } else {
            vec![
                SubItem::Row("Checkout", "检出", 10, false, ""),
                SubItem::Row("New Branch from", "基于此新建分支", 1, false, ""),
                SubItem::Row("Checkout and Rebase onto", "检出并变基到", 11, false, ""),
                SubItem::Sep,
                SubItem::Row("Compare with Current", "与当前分支比较", 12, false, ""),
                SubItem::Row(
                    "Show Diff with Working Tree",
                    "与工作区比较差异",
                    2,
                    false,
                    "",
                ),
                SubItem::Sep,
                SubItem::Row("Rebase onto", "变基到当前分支", 13, false, ""),
                SubItem::Row("Merge into current", "合并到当前分支", 14, false, ""),
                SubItem::Sep,
                SubItem::Row("Update", "更新", 3, false, ""),
                SubItem::Row("Push...", "推送...", 4, false, ""),
                SubItem::Sep,
                SubItem::Row("Rename...", "重命名...", 5, false, "F2"),
                SubItem::Row("Delete", "删除", 20, true, ""),
            ]
        };

        let sub_w = 340.0f32.min((screen.width() - 16.0).max(220.0));
        let sub_h = items
            .iter()
            .map(|it| match it {
                SubItem::Row(..) => 24.0,
                SubItem::Sep => 9.0,
            })
            .sum::<f32>()
            + 12.0;

        let sub_x = if rect.right() + 4.0 + sub_w <= screen.right() - 4.0 {
            rect.right() + 4.0
        } else {
            (rect.left() - sub_w - 4.0).max(screen.left() + 4.0)
        };
        let sub_y = sub.anchor.top().clamp(
            screen.top() + 8.0,
            (screen.bottom() - sub_h - 8.0).max(screen.top() + 8.0),
        );
        let sub_rect = Rect::from_min_size(Pos2::new(sub_x, sub_y), Vec2::new(sub_w, sub_h));
        sub_rect_opt = Some(sub_rect);
        // Register for the shell's wheel interception (next frame), matching
        // the tree context submenu.
        rt.menu_hit_rects.push(sub_rect);

        menu_frame(ui, sub_rect, theme);

        let mut action: Option<u8> = None;
        let mut sy = sub_rect.top() + 6.0;

        for (i, item) in items.iter().enumerate() {
            match item {
                SubItem::Sep => {
                    ui.painter().line_segment(
                        [
                            Pos2::new(sub_rect.left() + 6.0, sy + 4.0),
                            Pos2::new(sub_rect.right() - 6.0, sy + 4.0),
                        ],
                        Stroke::new(1.0, p.border),
                    );
                    sy += 9.0;
                }
                SubItem::Row(label, label_zh, code, is_danger, shortcut) => {
                    let row = Rect::from_min_size(
                        Pos2::new(sub_rect.left() + 6.0, sy),
                        Vec2::new(sub_rect.width() - 12.0, 24.0),
                    );
                    let resp =
                        ui.interact(row, ui.scope_id().with(("bsub", i, *code)), Sense::click());
                    if resp.hovered() {
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::same(3), p.hover);
                    }

                    let display_text = match *code {
                        1 => format!(
                            "{} '{bname}'...",
                            crate::tr!(model, "New Branch from", "基于此新建分支")
                        ),
                        10 => format!("{} '{bname}'", crate::tr!(model, "Checkout", "检出")),
                        11 => format!(
                            "{} '{current_branch}'...",
                            crate::tr!(model, "Checkout and Rebase onto", "检出并变基到")
                        ),
                        13 => format!(
                            "{} '{current_branch}' onto '{bname}'...",
                            crate::tr!(model, "Rebase", "变基")
                        ),
                        14 => format!(
                            "{} '{bname}' into '{current_branch}'",
                            crate::tr!(model, "Merge", "合并")
                        ),
                        15 => format!(
                            "{} '{}' {}",
                            crate::tr!(model, "Pull into", "拉取到"),
                            current_branch,
                            crate::tr!(model, "Using Rebase", "使用变基")
                        ),
                        16 => format!(
                            "{} '{}' {}",
                            crate::tr!(model, "Pull into", "拉取到"),
                            current_branch,
                            crate::tr!(model, "Using Merge", "使用合并")
                        ),
                        _ => crate::tr!(model, *label, *label_zh).to_string(),
                    };

                    let text_color = if *is_danger { p.rose } else { p.text };
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(row.left() + 10.0, row.top()),
                            Pos2::new(row.right() - 8.0, row.bottom()),
                        ),
                        &display_text,
                        FontId::proportional(12.0),
                        text_color,
                        Align2::LEFT_CENTER,
                    );

                    if !shortcut.is_empty() {
                        ui.painter().text(
                            Pos2::new(row.right() - 8.0, row.center().y),
                            Align2::RIGHT_CENTER,
                            *shortcut,
                            FontId::proportional(11.0),
                            p.text3,
                        );
                    }

                    if resp.clicked() {
                        action = Some(*code);
                    }
                    sy += 24.0;
                }
            }
        }

        if let Some(code) = action {
            match code {
                1 => {
                    rt.dialog = DialogState::NewBranch {
                        value: String::new(),
                        start_point: Some(bname.clone()),
                        checkout: true,
                        error: None,
                    };
                    close = true;
                }
                2 => {
                    model.queue(Command::OpenCompareWithLocal {
                        commit: bname.clone(),
                    });
                    close = true;
                }
                3 => {
                    if is_curr {
                        rt.dialog = DialogState::Pull {
                            mode: lazecode_git::PullMode::Merge,
                            status: None,
                        };
                    } else {
                        model.queue(Command::FetchAll);
                    }
                    close = true;
                }
                4 => {
                    if let Some(project) = model.active_project() {
                        rt.dialog = push_dialog_state(project, "origin".into(), bname.clone());
                    }
                    close = true;
                }
                5 => {
                    rt.dialog = DialogState::RenameBranch {
                        from: bname.clone(),
                        value: bname.clone(),
                        error: None,
                    };
                    close = true;
                }
                10 => {
                    model.queue(Command::CheckoutBranch(bname.clone()));
                    close = true;
                }
                11 => {
                    model.queue(Command::CheckoutBranch(bname.clone()));
                    model.queue(Command::Rebase {
                        onto: current_branch.clone(),
                    });
                    close = true;
                }
                12 => {
                    crate::editor::open_compare_page(model, current_branch.clone(), bname.clone());
                    close = true;
                }
                13 => {
                    model.queue(Command::Rebase {
                        onto: bname.clone(),
                    });
                    close = true;
                }
                14 => {
                    model.queue(Command::Merge {
                        ref_name: bname.clone(),
                    });
                    close = true;
                }
                15 => {
                    rt.dialog = DialogState::Pull {
                        mode: lazecode_git::PullMode::Rebase,
                        status: None,
                    };
                    close = true;
                }
                16 => {
                    rt.dialog = DialogState::Pull {
                        mode: lazecode_git::PullMode::Merge,
                        status: None,
                    };
                    close = true;
                }
                20 => {
                    rt.dialog = DialogState::DeleteBranch {
                        name: bname.clone(),
                        force: false,
                        remote: is_rem,
                        error: None,
                    };
                    close = true;
                }
                _ => {}
            }
            rt.branch_submenu = None;
        }
    }

    let dismiss_main = should_dismiss(ui, rect, menu.opened_frame, rt.frame_count);
    let in_sub = sub_rect_opt.is_some_and(|sr| {
        ui.input(|i| i.pointer.hover_pos())
            .is_some_and(|p| sr.contains(p))
    });
    if dismiss_main && !in_sub {
        close = true;
        rt.branch_submenu = None;
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
    let options = model.settings.agents.options.clone();
    let visible_rows = options.len().min(9);
    let height = 52.0 + visible_rows as f32 * 46.0 + 24.0;
    let rect = popover_rect(rt, screen, anchor, 320.0, height);
    menu_frame(ui, rect, theme);
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, rect.top() + 20.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Start agent", "启动 agent"),
        FontId::proportional(11.0),
        p.text3,
    );
    let rows = Rect::from_min_max(
        Pos2::new(rect.left() + 6.0, rect.top() + 34.0),
        Pos2::new(rect.right() - 6.0, rect.bottom() - 10.0),
    );
    let mut launch: Option<(String, String)> = None;
    let mut close = false;
    super::app_ui::region(ui, rows, "agent-menu", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("agent-menu-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for option in &options {
                    let (row, response) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 44.0), Sense::click());
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
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(row.left() + 38.0, row.top() + 5.0),
                            Pos2::new(row.right() - 12.0, row.center().y + 3.0),
                        ),
                        &option.name,
                        FontId::proportional(11.5),
                        p.text,
                        Align2::LEFT_CENTER,
                    );
                    let description = if option.description.is_empty() {
                        if option.command.is_empty() {
                            crate::tr!(model, "Start a plain login shell", "启动登录 Shell")
                                .to_string()
                        } else {
                            format!("Run {}", option.command)
                        }
                    } else {
                        option.description.clone()
                    };
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(row.left() + 38.0, row.center().y + 3.0),
                            Pos2::new(row.right() - 12.0, row.bottom() - 3.0),
                        ),
                        &description,
                        FontId::proportional(9.5),
                        p.text3,
                        Align2::LEFT_CENTER,
                    );
                    if !option.command.is_empty() {
                        text_clipped(
                            ui.painter(),
                            Rect::from_min_max(
                                Pos2::new(row.right() - 120.0, row.top()),
                                Pos2::new(row.right() - 12.0, row.center().y - 2.0),
                            ),
                            &option.command,
                            FontId::monospace(8.5),
                            p.text3,
                            Align2::RIGHT_CENTER,
                        );
                    }
                    if response.clicked() {
                        launch = Some((option.name.clone(), option.command.clone()));
                        close = true;
                    }
                }
            });
    });
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
        Stroke::new(1.0, p.border_soft),
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
    if primary_button(
        ui,
        save_rect,
        &save_label,
        None,
        count_conflicts == 0,
        theme,
    )
    .clicked()
        && count_conflicts == 0
    {
        if let Some(active) = model.active_project {
            let root = model.projects[active].active_root().to_path_buf();
            match lazecode_git::mark_conflict_resolved(&root, path, Some(worktree)) {
                Ok(()) => {
                    model.queue(Command::ReloadGit);
                    model.queue(Command::RefreshLog);
                    model.toast(
                        if zh {
                            format!("已解决 {path}")
                        } else {
                            format!("Resolved {path}")
                        },
                        ToastKind::Info,
                    );
                    *close = true;
                }
                Err(error) => {
                    model.toast(
                        if zh {
                            format!("无法标记解决: {error}")
                        } else {
                            format!("Could not mark resolved: {error}")
                        },
                        ToastKind::Error,
                    );
                }
            }
        }
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
        Stroke::new(1.0, p.border_soft),
    );
    ui.painter().vline(
        mid_rect.right() + 1.0,
        body_rect.y_range(),
        Stroke::new(1.0, p.border_soft),
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
    let rect = popover_rect(rt, screen, menu.anchor, 200.0, height);
    menu_frame(ui, rect, theme);
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
    let rect = popover_rect(rt, screen, menu.anchor, 160.0, height);
    menu_frame(ui, rect, theme);
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
    let rect = popover_rect(rt, screen, menu.anchor, 220.0, height);
    menu_frame(ui, rect, theme);
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
        rt.dialog = dialog;
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
