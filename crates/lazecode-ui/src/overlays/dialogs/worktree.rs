//! New-worktree dialog body: name, branch picker (existing or new) and the
//! on-disk location with a directory picker.

use egui::{Align2, FontId, Key, Pos2, Rect, Stroke, StrokeKind, TextEdit, Ui, Vec2};
use lazecode_core::AppModel;

use crate::runtime::DialogState;
use crate::theme::Theme;
use crate::widgets::{checkbox, secondary_button};

use super::super::used_worktree_branches;

/// Draw the body of `DialogState::NewWorktree`. Returns `(close, confirm)`.
pub(super) fn draw_new_worktree(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    dialog: &mut DialogState,
) -> (bool, bool) {
    let DialogState::NewWorktree {
        name,
        branch,
        create_new,
        location,
        error,
    } = dialog
    else {
        return (false, false);
    };
    let p = theme.palette;
    let close = false;
    let mut confirm = false;

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
                                egui::Button::new(egui::RichText::new(label_text).color(p.text3)),
                            );
                        } else {
                            let is_sel = *branch == b.name;
                            if ui.selectable_label(is_sel, &b.name).clicked() {
                                *branch = b.name.clone();
                                if name.is_empty() {
                                    *name = b.name.clone();
                                    let base_wt = format!("{}/.worktree", proj.root.display());
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

    let browse_btn = Rect::from_min_size(Pos2::new(left + width - 66.0, y), Vec2::new(66.0, 28.0));
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

    (close, confirm)
}
