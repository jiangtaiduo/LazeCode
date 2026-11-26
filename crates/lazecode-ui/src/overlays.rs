use std::time::Duration;

use egui::{Align2, Color32, FontId, Key, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use lazecode_core::AppModel;

use crate::runtime::{DialogState, UiRuntime};
use crate::theme::Theme;

mod conflict_tool;
mod dialogs;
mod menus;

use dialogs::draw_dialog;
use menus::draw_menu;

fn remotes_width(remotes: &[lazecode_git::RemoteInfo], available: f32) -> f32 {
    let longest = remotes
        .iter()
        .map(|remote| remote.name.chars().count())
        .max()
        .unwrap_or(8);
    (longest as f32 * 7.5 + 34.0).clamp(90.0, available)
}

/// Shared constructor for the Push dialog so every entry point sees the
/// project's remotes and pending commits.
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
        Stroke::new(1.0, p.border_strong),
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
/// Width that fits every label at the menu row font, including the left text
/// inset and the right check/submenu gutter. Callers keep their own minimum.
pub(crate) fn menu_width(
    ui: &Ui,
    labels: impl IntoIterator<Item = impl AsRef<str>>,
    min: f32,
) -> f32 {
    let font = FontId::proportional(12.0);
    labels
        .into_iter()
        .fold(min, |width, label| {
            let measured = ui
                .painter()
                .layout_no_wrap(label.as_ref().to_owned(), font.clone(), Color32::WHITE)
                .size()
                .x;
            width.max(measured + 34.0)
        })
        .min(420.0)
}

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
    crate::widgets::text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(row.left() + 10.0, row.top()),
            Pos2::new(row.right() - 8.0, row.bottom()),
        ),
        label,
        FontId::proportional(12.0),
        color,
        Align2::LEFT_CENTER,
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
