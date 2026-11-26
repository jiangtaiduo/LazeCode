//! Compare details column: file tree, metadata and the diff opener.

use super::prelude::*;
use super::{cb_icon_btn, DETAILS_HEAD_H, FILE_ROW_H, ROOT_ROW_H};

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_details(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    ref_name: &str,
    other_ref: &str,
    side: &lazecode_git::CompareSide,
    merge_base: &Option<String>,
    side_index: u8,
    selected: &[String],
    select_all: bool,
    collapsed: &std::collections::HashSet<String>,
    selected_file: &Option<String>,
) {
    let p = theme.palette;
    let zh = model.settings.language.is_zh();
    let side_index_us = usize::from(side_index);
    // .cb-details：bg-sidebar + 左缘分隔。
    ui.painter().rect_filled(rect, 0.0, p.sidebar);

    let detail = if side_index == 0 {
        &model.ui.compare_branches.left_view.detail
    } else {
        &model.ui.compare_branches.right_view.detail
    };
    let detail = detail.as_ref();
    // 详情文件清单：⌘A = 整侧；单选/范围 = detail.files。Borrow the cached
    // lists: the compare pane redraws every frame and must not copy them.
    let details_files: &[lazecode_git::CompareFile] = if select_all {
        &side.files
    } else {
        match detail {
            Some(detail) if !detail.loading && detail.error.is_none() => &detail.files,
            _ => &[],
        }
    };
    let files_loading =
        !select_all && detail.is_some_and(|detail| detail.loading) && !selected.is_empty();
    // Mutations are applied after the borrowed lists are done being drawn.
    let mut diff_click: Option<String> = None;
    let mut collapse_all_click: Option<Vec<String>> = None;
    let mut expand_all_click = false;

    // 头部 26px。
    let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), DETAILS_HEAD_H));
    ui.painter().hline(
        head.x_range(),
        head.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    let files_label = if zh {
        format!("{} 个文件变更", details_files.len())
    } else if details_files.len() == 1 {
        "1 file changed".to_string()
    } else {
        format!("{} files changed", details_files.len())
    };
    let title = if files_loading {
        crate::tr!(model, "Loading…", "加载中…").to_string()
    } else if select_all {
        let all = if zh {
            format!("已选择全部 {} 个提交 · ", side.commits.len())
        } else {
            format!("All {} selected · ", side.commits.len())
        };
        format!("{all}{files_label}")
    } else if selected.len() > 1 {
        let all = if zh {
            format!("已选择 {} 个提交 · ", selected.len())
        } else {
            format!("{} selected · ", selected.len())
        };
        format!("{all}{files_label}")
    } else if selected.len() == 1 && detail.is_some_and(|detail| detail.error.is_some()) {
        crate::tr!(model, "Load failed", "加载失败").to_string()
    } else {
        files_label
    };
    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(head.left() + 10.0, head.top()),
            Pos2::new(head.right() - 70.0, head.bottom()),
        ),
        &title,
        FontId::proportional(11.0),
        p.text2,
        Align2::LEFT_CENTER,
    );
    // 展开全部目录路径（禁用判定：无目录时 Expand/Collapse 置灰）。
    let all_rows =
        crate::compare::flatten_file_tree(details_files, &std::collections::HashSet::new());
    let has_dirs = all_rows.iter().any(|row| row.is_dir);
    let mut rx = head.right() - 4.0;
    let diff_rect = Rect::from_min_size(
        Pos2::new(rx - 20.0, head.center().y - 10.0),
        Vec2::splat(20.0),
    );
    rx -= 21.0;
    let collapse_rect = Rect::from_min_size(
        Pos2::new(rx - 20.0, head.center().y - 10.0),
        Vec2::splat(20.0),
    );
    rx -= 21.0;
    let expand_rect = Rect::from_min_size(
        Pos2::new(rx - 20.0, head.center().y - 10.0),
        Vec2::splat(20.0),
    );
    let diff_enabled = selected_file.is_some() || !selected.is_empty() || select_all;
    if cb_icon_btn(
        ui,
        diff_rect,
        ("cb-diff", side_index),
        Icon::Diff,
        false,
        diff_enabled,
    ) {
        diff_click = Some(selected_file.clone().unwrap_or_default());
    }
    if cb_icon_btn(
        ui,
        collapse_rect,
        ("cb-collapse", side_index),
        Icon::CollapseAll,
        false,
        has_dirs,
    ) {
        collapse_all_click = Some(
            all_rows
                .iter()
                .filter(|row| row.is_dir)
                .map(|row| row.key.clone())
                .collect(),
        );
        rt.cb_root_collapsed[side_index_us] = true;
    }
    if cb_icon_btn(
        ui,
        expand_rect,
        ("cb-expand", side_index),
        Icon::ExpandAll,
        false,
        has_dirs,
    ) {
        expand_all_click = true;
        rt.cb_root_collapsed[side_index_us] = false;
    }

    // ---- 文件树（root 行 24px + 目录/文件行 21px）----
    let tree_rect = Rect::from_min_max(
        Pos2::new(rect.left(), head.bottom()),
        Pos2::new(rect.right(), rect.bottom()),
    );
    // 元信息高度（内容自适应，最大 44%）。
    let meta = meta_rect_content(
        model,
        side_index,
        ref_name,
        &side.commits,
        select_all,
        selected,
    );
    let meta_h = meta.1.min(tree_rect.height() * 0.44);
    let list_rect = Rect::from_min_max(
        tree_rect.min,
        Pos2::new(tree_rect.right(), tree_rect.bottom() - meta_h),
    );

    let root_collapsed = rt.cb_root_collapsed[side_index_us];
    let mut rows: Vec<crate::compare::FileRow> = Vec::new();
    if !details_files.is_empty() && !root_collapsed {
        rows = crate::compare::flatten_file_tree(details_files, collapsed);
    }
    let mut toggle_click_action: Option<String> = None;
    let mut select_click_action: Option<String> = None;
    let mut open_click_action: Option<String> = None;
    crate::app_ui::region(ui, list_rect, "cb-tree", |ui| {
        if details_files.is_empty() && !files_loading {
            ui.painter().text(
                Pos2::new(rect.left() + 10.0, list_rect.top() + 24.0),
                Align2::LEFT_CENTER,
                crate::tr!(model, "No changed files", "没有变更文件"),
                FontId::proportional(11.0),
                p.text3,
            );
        }
        let mut open_click: Option<String> = None;
        let mut select_click: Option<String> = None;
        let mut toggle_click: Option<String> = None;
        egui::ScrollArea::vertical()
            .id_salt(("cb-tree", side_index))
            .auto_shrink([false, false])
            .show_rows(
                ui,
                FILE_ROW_H,
                rows.len() + usize::from(!details_files.is_empty()),
                |ui, range| {
                    for row_index in range.clone() {
                        if row_index == 0 {
                            // 根行（cb-root-row）：chevron + folder + 仓库名 + 计数。
                            let (row, response) = ui.allocate_exact_size(
                                Vec2::new(list_rect.width(), ROOT_ROW_H),
                                Sense::click(),
                            );
                            if response.hovered() {
                                ui.painter()
                                    .rect_filled(row, egui::CornerRadius::ZERO, p.hover);
                            }
                            draw_chevron(
                                ui.painter(),
                                Pos2::new(row.left() + 11.0, row.center().y),
                                root_collapsed,
                                p.text3,
                            );
                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(row.left() + 24.0, row.center().y),
                                    Vec2::splat(13.0),
                                ),
                                Icon::Folder,
                                Color32::from_rgb(0xdc, 0xd7, 0xaa),
                            );
                            let repo_name = model
                                .active_project()
                                .map(|pr| pr.name.clone())
                                .unwrap_or_else(|| "Repository".to_string());
                            ui.painter().text(
                                Pos2::new(row.left() + 36.0, row.center().y),
                                Align2::LEFT_CENTER,
                                repo_name,
                                FontId::proportional(13.0),
                                p.text_bright,
                            );
                            let count = if zh {
                                format!("{} 个文件", details_files.len())
                            } else if details_files.len() == 1 {
                                "1 file".to_string()
                            } else {
                                format!("{} files", details_files.len())
                            };
                            ui.painter().text(
                                Pos2::new(row.right() - 8.0, row.center().y),
                                Align2::RIGHT_CENTER,
                                count,
                                FontId::proportional(11.5),
                                p.text3,
                            );
                            if response.clicked() {
                                rt.cb_root_collapsed[side_index_us] =
                                    !rt.cb_root_collapsed[side_index_us];
                            }
                            if range.end <= 1 {
                                break;
                            }
                            continue;
                        }
                        let Some(file_row) = rows.get(row_index - 1) else {
                            continue;
                        };
                        let (row, response) = ui.allocate_exact_size(
                            Vec2::new(list_rect.width(), FILE_ROW_H),
                            Sense::click(),
                        );
                        let indent = 8.0 + file_row.depth as f32 * 12.0;
                        let is_selected = !file_row.is_dir
                            && selected_file.as_deref() == Some(file_row.key.as_str());
                        if is_selected {
                            ui.painter()
                                .rect_filled(row, egui::CornerRadius::ZERO, p.jb_accent);
                        } else if response.hovered() {
                            ui.painter()
                                .rect_filled(row, egui::CornerRadius::ZERO, p.hover);
                        }
                        if file_row.is_dir {
                            let open = !collapsed.contains(&file_row.key);
                            draw_chevron(
                                ui.painter(),
                                Pos2::new(row.left() + indent + 5.0, row.center().y),
                                open,
                                p.text3,
                            );
                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(row.left() + indent + 18.0, row.center().y),
                                    Vec2::splat(13.0),
                                ),
                                Icon::Folder,
                                Color32::from_rgb(0xdc, 0xd7, 0xaa),
                            );
                            ui.painter().text(
                                Pos2::new(row.left() + indent + 30.0, row.center().y),
                                Align2::LEFT_CENTER,
                                &file_row.label,
                                FontId::proportional(13.0),
                                p.text,
                            );
                            let count = if zh {
                                format!("{} 个文件", file_row.count)
                            } else if file_row.count == 1 {
                                "1 file".to_string()
                            } else {
                                format!("{} files", file_row.count)
                            };
                            ui.painter().text(
                                Pos2::new(row.right() - 8.0, row.center().y),
                                Align2::RIGHT_CENTER,
                                count,
                                FontId::proportional(11.5),
                                p.text3,
                            );
                            if response.clicked() {
                                toggle_click = Some(file_row.key.clone());
                            }
                        } else {
                            let Some(file) = &file_row.file else { continue };
                            let status = file.status.to_ascii_uppercase();
                            let name_color = if is_selected {
                                Color32::WHITE
                            } else if status == 'A' || status == '?' || status == 'R' {
                                p.green
                            } else if status == 'D' {
                                p.rose
                            } else {
                                p.blue
                            };
                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(row.left() + indent + 7.0, row.center().y),
                                    Vec2::splat(13.0),
                                ),
                                Icon::File,
                                if is_selected { Color32::WHITE } else { p.text3 },
                            );
                            ui.painter().text(
                                Pos2::new(row.left() + indent + 19.0, row.center().y),
                                Align2::LEFT_CENTER,
                                &file_row.label,
                                FontId::proportional(13.0),
                                name_color,
                            );
                            // numstat（mono 10px，+绿 / -红，仅 >0）。
                            let mut nx = row.right() - 8.0;
                            if file.deletions > 0 {
                                let label = format!("-{}", file.deletions);
                                nx -= label.chars().count() as f32 * 6.0;
                                ui.painter().text(
                                    Pos2::new(nx, row.center().y),
                                    Align2::LEFT_CENTER,
                                    label,
                                    FontId::monospace(11.5),
                                    if is_selected {
                                        Color32::from_rgba_unmultiplied(255, 255, 255, 219)
                                    } else {
                                        p.rose
                                    },
                                );
                                nx -= 5.0;
                            }
                            if file.additions > 0 || file.deletions == 0 {
                                let label = format!("+{}", file.additions);
                                nx -= label.chars().count() as f32 * 6.0;
                                ui.painter().text(
                                    Pos2::new(nx, row.center().y),
                                    Align2::LEFT_CENTER,
                                    label,
                                    FontId::monospace(11.5),
                                    if is_selected {
                                        Color32::from_rgba_unmultiplied(255, 255, 255, 219)
                                    } else {
                                        p.green
                                    },
                                );
                            }
                            if response.clicked() {
                                select_click = Some(file_row.key.clone());
                            }
                            if response.double_clicked() {
                                open_click = Some(file_row.key.clone());
                            }
                        }
                    }
                },
            );
        toggle_click_action = toggle_click;
        select_click_action = select_click;
        open_click_action = open_click;
    });

    // Apply model mutations only after the borrowed file lists are no longer
    // used by the drawing closure.
    if let Some(keys) = collapse_all_click {
        let view = if side_index == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.collapsed_dirs = keys.into_iter().collect();
    }
    if expand_all_click {
        let view = if side_index == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.collapsed_dirs.clear();
    }
    if let Some(path) = diff_click {
        open_file_diff(
            model, side_index, side, merge_base, select_all, selected, &path, other_ref,
        );
    }
    if let Some(key) = toggle_click_action {
        let view = if side_index == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        if view.collapsed_dirs.contains(&key) {
            view.collapsed_dirs.remove(&key);
        } else {
            view.collapsed_dirs.insert(key);
        }
    }
    if let Some(path) = select_click_action {
        let view = if side_index == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.selected_file = Some(path);
    }
    if let Some(path) = open_click_action {
        open_file_diff(
            model, side_index, side, merge_base, select_all, selected, &path, other_ref,
        );
    }

    // ---- 元信息（cb-details-meta：message/hash/email/date/inBranches）----
    if meta_h > 0.0 {
        let meta_rect = Rect::from_min_max(Pos2::new(rect.left(), list_rect.bottom()), rect.max);
        ui.painter().hline(
            meta_rect.x_range(),
            meta_rect.top() + 0.5,
            Stroke::new(1.0, p.border_soft),
        );
        let (message, hash, email, date_line, branches_line) = meta.0;
        let mut y = meta_rect.top() + 7.0;
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(meta_rect.left() + 10.0, y),
                Pos2::new(meta_rect.right() - 10.0, y + 17.0),
            ),
            &message,
            FontId::proportional(13.0),
            p.text_bright,
            Align2::LEFT_TOP,
        );
        y += 20.0;
        if !hash.is_empty() {
            ui.painter().text(
                Pos2::new(meta_rect.left() + 10.0, y + 7.0),
                Align2::LEFT_CENTER,
                &hash,
                FontId::monospace(12.0),
                p.accent,
            );
            let hash_w = hash.chars().count() as f32 * 7.2 + 10.0;
            ui.painter().text(
                Pos2::new(meta_rect.left() + 10.0 + hash_w, y + 7.0),
                Align2::LEFT_CENTER,
                &email,
                FontId::proportional(12.0),
                p.text2,
            );
            y += 16.0;
        }
        if !date_line.is_empty() {
            ui.painter().text(
                Pos2::new(meta_rect.left() + 10.0, y + 7.0),
                Align2::LEFT_CENTER,
                &date_line,
                FontId::proportional(12.0),
                p.text3,
            );
            y += 16.0;
        }
        if !branches_line.is_empty() {
            ui.painter().hline(
                meta_rect.x_range(),
                y + 2.0,
                Stroke::new(1.0, p.border_soft),
            );
            ui.painter().text(
                Pos2::new(meta_rect.left() + 10.0, y + 12.0),
                Align2::LEFT_CENTER,
                &branches_line,
                FontId::proportional(12.0),
                p.text3,
            );
        }
    }
}

type MetaContent = (String, String, String, String, String);

/// 元信息内容 + 估算高度（cb-details-meta）。
#[allow(clippy::type_complexity)]
fn meta_rect_content(
    model: &AppModel,
    _side_index: u8,
    ref_name: &str,
    commits: &[lazecode_git::CommitInfo],
    select_all: bool,
    selected: &[String],
) -> (MetaContent, f32) {
    let zh = model.settings.language.is_zh();
    if select_all {
        let message = if zh {
            format!("已选择全部 {} 个提交", commits.len())
        } else {
            format!("All {} commits selected", commits.len())
        };
        return (
            (
                message,
                String::new(),
                String::new(),
                ref_name.to_string(),
                String::new(),
            ),
            46.0,
        );
    }
    // 详情展示第一个被选中的提交（显示序 = 最新在前）。
    let Some(details_commit) = selected
        .first()
        .and_then(|id| commits.iter().find(|commit| &commit.id == id))
    else {
        return (
            (
                crate::tr!(model, "No commit selected", "未选择提交").to_string(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            ),
            32.0,
        );
    };
    let message = details_commit.message.clone();
    let hash = details_commit.short.clone();
    let email = details_commit.email.clone();
    let date_line = if zh {
        format!(
            "{email} 于 {}",
            crate::panel::format_commit_datetime(details_commit.time)
        )
    } else {
        format!(
            "{email} on {}",
            crate::panel::format_commit_datetime(details_commit.time)
        )
    };
    let branches: Vec<String> = details_commit
        .refs
        .iter()
        .filter(|r| {
            matches!(
                r.kind,
                lazecode_git::RefKind::LocalBranch
                    | lazecode_git::RefKind::Head
                    | lazecode_git::RefKind::RemoteBranch
            )
        })
        .map(|r| r.label.clone())
        .collect();
    let branches_line = if branches.is_empty() {
        String::new()
    } else if zh {
        format!("存在于 {} 个分支：{}", branches.len(), branches.join(", "))
    } else if branches.len() == 1 {
        format!("In 1 branch: {}", branches.join(", "))
    } else {
        format!("In {} branches: {}", branches.len(), branches.join(", "))
    };
    let height = 27.0
        + 16.0
        + usize::from(!date_line.is_empty()) as f32 * 16.0
        + usize::from(!branches_line.is_empty()) as f32 * 20.0;
    ((message, hash, email, date_line, branches_line), height)
}

/// 10px 折叠箭头。
fn draw_chevron(painter: &egui::Painter, center: Pos2, open: bool, color: Color32) {
    use egui::Stroke;
    let r = 3.2;
    if open {
        painter.line_segment(
            [
                Pos2::new(center.x - r, center.y - r * 0.6),
                Pos2::new(center.x, center.y + r * 0.6),
            ],
            Stroke::new(1.2, color),
        );
        painter.line_segment(
            [
                Pos2::new(center.x, center.y + r * 0.6),
                Pos2::new(center.x + r, center.y - r * 0.6),
            ],
            Stroke::new(1.2, color),
        );
    } else {
        painter.line_segment(
            [
                Pos2::new(center.x - r * 0.6, center.y - r),
                Pos2::new(center.x + r * 0.6, center.y),
            ],
            Stroke::new(1.2, color),
        );
        painter.line_segment(
            [
                Pos2::new(center.x + r * 0.6, center.y),
                Pos2::new(center.x - r * 0.6, center.y + r),
            ],
            Stroke::new(1.2, color),
        );
    }
}

/// File-diff source, stable rules: ⌘A whole side = merge-base..side; multi
/// selection = parent of oldest..newest (empty tree for a root); else the
/// commit itself.
#[allow(clippy::too_many_arguments)]
pub(super) fn open_file_diff(
    model: &mut AppModel,
    side_index: u8,
    side: &lazecode_git::CompareSide,
    merge_base: &Option<String>,
    select_all: bool,
    selected: &[String],
    path: &str,
    other_ref: &str,
) {
    const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
    if select_all || selected.is_empty() {
        let from = merge_base.clone().unwrap_or_else(|| other_ref.to_string());
        let to = if side_index == 0 {
            model.ui.compare_branches.left_ref.clone()
        } else {
            model.ui.compare_branches.right_ref.clone()
        };
        model.queue(Command::OpenCompareCommitsDiff {
            from,
            to,
            path: path.to_string(),
        });
        return;
    }
    if selected.len() == 1 {
        model.queue(Command::OpenCommitDiff {
            commit: selected[0].clone(),
            path: path.to_string(),
        });
        return;
    }
    let positions: Vec<usize> = selected
        .iter()
        .filter_map(|id| side.commits.iter().position(|commit| &commit.id == id))
        .collect();
    let (newest, oldest) = match (positions.iter().min(), positions.iter().max()) {
        (Some(newest), Some(oldest)) => (*newest, *oldest),
        _ => return,
    };
    let from = side.commits[oldest]
        .parents
        .first()
        .cloned()
        .unwrap_or_else(|| EMPTY_TREE.to_string());
    let to = side.commits[newest].id.clone();
    model.queue(Command::OpenCompareCommitsDiff {
        from,
        to,
        path: path.to_string(),
    });
}
