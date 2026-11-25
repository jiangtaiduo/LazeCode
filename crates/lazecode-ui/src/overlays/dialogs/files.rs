//! File / path / commit-per-file dialog bodies and the delete-confirmation
//! warning helper shared by DeletePath and ConfirmDeleteUntracked.

use egui::{Align2, FontId, Key, Pos2, Rect, Stroke, StrokeKind, TextEdit, Ui, Vec2};
use lazecode_core::{AppModel, TabKind};

use crate::runtime::DialogState;
use crate::theme::Theme;
use crate::widgets::text_clipped;

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

/// Draw the shared single-line input body for NewFile / NewDirectory /
/// RenamePath / NewShelf / GoToLine / RenameSymbol / RenameTerminal /
/// NewChangelist. Returns `(close, confirm)`.
pub(super) fn draw_path_input_dialogs(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    dialog: &mut DialogState,
) -> (bool, bool) {
    let (value, error) = match dialog {
        DialogState::NewFile { value, error, .. }
        | DialogState::NewDirectory { value, error, .. }
        | DialogState::RenamePath { value, error, .. }
        | DialogState::NewShelf { value, error, .. }
        | DialogState::GoToLine { value, error }
        | DialogState::RenameSymbol { value, error, .. }
        | DialogState::RenameTerminal { value, error, .. }
        | DialogState::NewChangelist { value, error } => (value, error),
        _ => return (false, false),
    };
    let p = theme.palette;
    let close = false;
    let mut confirm = false;

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

    (close, confirm)
}

/// Draw the body of `DialogState::DeletePath`. Returns `(close, confirm)`.
pub(super) fn draw_delete_path(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    dialog: &mut DialogState,
) -> (bool, bool) {
    let DialogState::DeletePath { path, .. } = dialog else {
        return (false, false);
    };
    let p = theme.palette;
    let close = false;
    let confirm = false;

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

    (close, confirm)
}

/// Draw the body of `DialogState::ConfirmDeleteUntracked`.
pub(super) fn draw_delete_untracked(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    dialog: &mut DialogState,
) -> (bool, bool) {
    let DialogState::ConfirmDeleteUntracked { paths } = dialog else {
        return (false, false);
    };
    let p = theme.palette;
    let close = false;
    let confirm = false;

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

    (close, confirm)
}

/// Draw the shared commit-message body for CommitFiles / ExtractChanges.
pub(super) fn draw_commit_files(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    dialog: &mut DialogState,
) -> (bool, bool) {
    let (value, error) = match dialog {
        DialogState::CommitFiles { value, error, .. }
        | DialogState::ExtractChanges { value, error, .. } => (value, error),
        _ => return (false, false),
    };
    let p = theme.palette;
    let close = false;
    let confirm = false;

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

    (close, confirm)
}

/// Draw the body of `DialogState::GetFileFromRevision`.
pub(super) fn draw_get_file_from_revision(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    dialog: &mut DialogState,
) -> (bool, bool) {
    let DialogState::GetFileFromRevision {
        path,
        revision,
        error,
    } = dialog
    else {
        return (false, false);
    };
    let p = theme.palette;
    let close = false;
    let confirm = false;

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

    (close, confirm)
}

/// Draw the body of `DialogState::CherryPickChanges`.
pub(super) fn draw_cherry_pick_changes(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    dialog: &mut DialogState,
) -> (bool, bool) {
    let DialogState::CherryPickChanges {
        paths,
        target,
        error,
    } = dialog
    else {
        return (false, false);
    };
    let p = theme.palette;
    let close = false;
    let confirm = false;

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

    (close, confirm)
}
