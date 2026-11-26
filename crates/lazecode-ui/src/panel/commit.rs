//! Commit detail panel and its time helpers.

use super::paint_wrapped_text;
use super::prelude::*;

/// 稳定版 commitDate：今天 → "Today HH:MM"，昨天 → "Yesterday HH:MM"，
/// 其余 → "YYYY/M/D HH:MM"（月/日不补零，时分补零）。
pub(crate) fn format_commit_datetime(seconds: i64) -> String {
    unsafe {
        let t = seconds as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return format!("{seconds}");
        }
        let clock = format!("{:02}:{:02}", tm.tm_hour, tm.tm_min);
        let now = libc::time(std::ptr::null_mut());
        let mut local: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut local).is_null() {
            return format!(
                "{}/{}/{} {}",
                tm.tm_year + 1900,
                tm.tm_mon + 1,
                tm.tm_mday,
                clock
            );
        }
        let same_day = local.tm_year == tm.tm_year && local.tm_yday == tm.tm_yday;
        let yesterday = {
            let mut y = local;
            y.tm_mday -= 1;
            libc::mktime(&mut y);
            y.tm_year == tm.tm_year && y.tm_yday == tm.tm_yday
        };
        if same_day {
            format!("Today {clock}")
        } else if yesterday {
            format!("Yesterday {clock}")
        } else {
            format!(
                "{}/{}/{} {}",
                tm.tm_year + 1900,
                tm.tm_mon + 1,
                tm.tm_mday,
                clock
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_commit_detail(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    details: &lazecode_git::CommitDetails,
    rt: &mut UiRuntime,
    lang: lazecode_core::Language,
    index: usize,
    range: Option<&lazecode_core::CommitRangeDetails>,
) {
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.panel);
    ui.painter().vline(
        rect.left() + 0.5,
        rect.y_range(),
        Stroke::new(1.0, p.border_soft),
    );

    // Top Header (30.0px) - exactly matching stable details-files-header
    let header_rect = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 30.0));
    ui.painter().rect_filled(header_rect, 0.0, p.panel);
    ui.painter().hline(
        header_rect.x_range(),
        header_rect.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );

    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(header_rect.left() + 14.0, header_rect.center().y),
            Vec2::splat(13.0),
        ),
        Icon::File,
        p.text3,
    );
    let count_text = if let Some(range) = range {
        crate::tr!(model, "{n} commits selected", "已选择 {n} 个提交")
            .replace("{n}", &range.count.to_string())
    } else if details.files.len() == 1 {
        if lang.is_zh() {
            "1 个文件变更".to_string()
        } else {
            "1 file changed".to_string()
        }
    } else if lang.is_zh() {
        format!("{} 个文件变更", details.files.len())
    } else {
        format!("{} files changed", details.files.len())
    };

    ui.painter().text(
        Pos2::new(header_rect.left() + 26.0, header_rect.center().y),
        Align2::LEFT_CENTER,
        count_text,
        FontId::proportional(12.5),
        p.text,
    );
    // A new selection keeps the previous rows painted; mark the short reload
    // with an accent underline instead of clearing the whole detail column.
    if range.is_some_and(|range| range.loading) {
        ui.painter().hline(
            header_rect.x_range(),
            header_rect.bottom() - 0.5,
            Stroke::new(2.0, p.accent),
        );
    }

    // Top action buttons from right to left (stable order: CollapseAll, ExpandAll, ViewOptions, Diff, Undo):
    let mut rx = header_rect.right() - 6.0;

    // 5. Collapse all
    let col_rect = Rect::from_min_size(
        Pos2::new(rx - 20.0, header_rect.top() + 5.0),
        Vec2::splat(20.0),
    );
    let col_resp = icon_button(ui, col_rect, Icon::CollapseAll, false, theme);
    if col_resp.hovered() {
        col_resp.show_tooltip_text(crate::tr!(model, "Collapse All", "全部折叠"));
    }
    if col_resp.clicked() {
        let tree = lazecode_git::build_commit_file_tree(&details.files);
        for path in lazecode_git::all_commit_file_dir_paths(&tree) {
            model.projects[index]
                .commit_file_dirs_collapsed
                .insert(path);
        }
        model.projects[index].commit_files_root_collapsed = true;
    }
    rx -= 22.0;

    // 4. Expand all
    let exp_rect = Rect::from_min_size(
        Pos2::new(rx - 20.0, header_rect.top() + 5.0),
        Vec2::splat(20.0),
    );
    let exp_resp = icon_button(ui, exp_rect, Icon::ExpandAll, false, theme);
    if exp_resp.hovered() {
        exp_resp.show_tooltip_text(crate::tr!(model, "Expand All", "全部展开"));
    }
    if exp_resp.clicked() {
        model.projects[index].commit_files_root_collapsed = false;
        // The set stores collapsed directories; an empty set expands all.
        model.projects[index].commit_file_dirs_collapsed.clear();
    }
    rx -= 22.0;

    // 2. Full commit diff
    let diff_rect = Rect::from_min_size(
        Pos2::new(rx - 20.0, header_rect.top() + 5.0),
        Vec2::splat(20.0),
    );
    let diff_resp = icon_button(ui, diff_rect, Icon::Diff, false, theme);
    if diff_resp.hovered() {
        diff_resp.show_tooltip_text(crate::tr!(model, "View Commit Diff", "查看完整提交差异"));
    }
    if diff_resp.clicked() {
        if let Some(range) = range {
            model.queue(Command::OpenCompareCommitsDiff {
                from: range.from.clone(),
                to: range.to.clone(),
                path: String::new(),
            });
        } else {
            model.queue(Command::OpenCommitDiff {
                commit: details.info.id.clone(),
                path: String::new(),
            });
        }
    }

    rx -= 22.0;

    // Reverting a range would need a replay of several commits; never let
    // this button act on an arbitrary member of a multi-selection.
    if range.is_none() {
        let revert_rect = Rect::from_min_size(
            Pos2::new(rx - 20.0, header_rect.top() + 5.0),
            Vec2::splat(20.0),
        );
        let revert_resp = icon_button(ui, revert_rect, Icon::Undo, false, theme);
        if revert_resp.hovered() {
            revert_resp.show_tooltip_text(crate::tr!(model, "Revert Commit", "回滚提交"));
        }
        if revert_resp.clicked() {
            model.queue(Command::RevertCommit {
                hash: details.info.id.clone(),
            });
        }
    }

    // Bottom Summary Card and Draggable Resizer (height clamped between 60.0 and panel height - 90.0)
    let card_h = model
        .ui
        .git_details_card_height
        .clamp(88.0, (rect.height() - 90.0).max(88.0));
    let split_rect = Rect::from_min_size(
        Pos2::new(rect.left(), rect.bottom() - card_h - 4.0),
        Vec2::new(rect.width(), 6.0),
    );
    let split_resp = ui.interact(
        split_rect,
        ui.scope_id().with("details-split-resizer"),
        Sense::drag(),
    );
    if split_resp.hovered() || split_resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if split_resp.dragged() {
        model.ui.git_details_card_height = (model.ui.git_details_card_height
            - split_resp.drag_delta().y)
            .clamp(88.0, (rect.height() - 90.0).max(88.0));
    }

    let card_rect = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - card_h), rect.max);
    ui.painter().rect_filled(card_rect, 0.0, p.panel2);
    ui.painter().hline(
        card_rect.x_range(),
        card_rect.top() + 0.5,
        Stroke::new(1.0, p.border_soft),
    );

    // Card content: subject and body wrap to the available width; metadata
    // sits in a compact footer so a taller card shows more message, not a
    // growing empty gap.
    let pad = 12.0;
    let hash_h = 18.0;
    let hash_center_y = card_rect.bottom() - 10.0 - hash_h * 0.5;
    let hash_pill_rect = Rect::from_min_size(
        Pos2::new(card_rect.left() + pad, hash_center_y - hash_h * 0.5),
        Vec2::new(68.0, hash_h),
    );
    let author_h = 14.0;
    let author_bottom = hash_pill_rect.top() - 4.0;
    let author_rect = Rect::from_min_max(
        Pos2::new(card_rect.left() + pad, author_bottom - author_h),
        Pos2::new(card_rect.right() - pad, author_bottom),
    );
    let content_top = card_rect.top() + 8.0;
    let content_bottom = author_rect.top() - 6.0;
    let content_left = card_rect.left() + pad;
    let content_right = card_rect.right() - pad;

    let subject_font = FontId::proportional(13.0);
    let subject_line_h = ui
        .painter()
        .layout_no_wrap("Ag".to_owned(), subject_font.clone(), p.text)
        .size()
        .y
        .max(1.0);
    let available_h = (content_bottom - content_top).max(0.0);
    let subject_max_rows = if details.info.body.trim().is_empty() {
        3
    } else {
        2
    };
    let subject_rows = ((available_h / subject_line_h).floor() as usize).clamp(1, subject_max_rows);
    let subject_h = paint_wrapped_text(
        ui,
        Rect::from_min_max(
            Pos2::new(content_left, content_top),
            Pos2::new(content_right, content_bottom),
        ),
        &details.info.message,
        subject_font,
        p.text,
        subject_rows,
    );

    let body_font = FontId::proportional(12.0);
    let body_line_h = ui
        .painter()
        .layout_no_wrap("Ag".to_owned(), body_font.clone(), p.text3)
        .size()
        .y
        .max(1.0);
    let body_top = content_top + subject_h + 4.0;
    let body_rect = Rect::from_min_max(
        Pos2::new(content_left, body_top),
        Pos2::new(content_right, content_bottom),
    );
    let body_rows = (body_rect.height() / body_line_h).floor() as usize;
    let body = details.info.body.trim();
    if body_rows > 0 && !body.is_empty() {
        paint_wrapped_text(ui, body_rect, body, body_font, p.text3, body_rows);
    }

    // Card blue short hash pill
    let hash_resp = ui.interact(
        hash_pill_rect,
        ui.scope_id().with("card-hash-pill"),
        Sense::click(),
    );
    let pill_color = Color32::from_rgb(0x58, 0xa6, 0xff); // Blue
    ui.painter().rect_filled(
        hash_pill_rect,
        egui::CornerRadius::same(4),
        pill_color.gamma_multiply(0.18),
    );
    ui.painter().text(
        hash_pill_rect.center(),
        Align2::CENTER_CENTER,
        &details.info.short,
        FontId::monospace(11.5),
        pill_color,
    );
    if hash_resp.hovered() {
        hash_resp.show_tooltip_text(crate::tr!(model, "Click to copy hash", "点击复制哈希"));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if hash_resp.clicked() {
        ui.ctx().copy_text(details.info.id.clone());
        model.toast(
            crate::tr!(model, "Copied hash to clipboard", "已复制提交哈希到剪贴板"),
            lazecode_core::ToastKind::Info,
        );
    }

    // Author line gets the full card width; the hash and date share the
    // compact footer row below it.
    text_clipped(
        ui.painter(),
        author_rect,
        &format!("{} <{}>", details.info.author, details.info.email),
        FontId::proportional(11.5),
        p.text3,
        Align2::LEFT_CENTER,
    );
    ui.painter().text(
        Pos2::new(card_rect.right() - pad, hash_pill_rect.center().y),
        Align2::RIGHT_CENTER,
        format_commit_datetime(details.info.time),
        FontId::monospace(10.5),
        p.text3,
    );

    // Changed Files Tree Area (middle area)
    let files_rect = Rect::from_min_max(
        Pos2::new(rect.left(), header_rect.bottom()),
        Pos2::new(rect.right(), split_rect.top()),
    );
    let tree = lazecode_git::build_commit_file_tree(&details.files);
    let flat_files = lazecode_git::flatten_commit_file_tree(
        &tree,
        &model.projects[index].commit_file_dirs_collapsed,
        0,
    );

    crate::app_ui::region(ui, files_rect, "commit-files-tree", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("commit-files")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Root Project Row (exactly matching stable details-tree-root)
                let root_collapsed = model.projects[index].commit_files_root_collapsed;
                let (root_row, root_resp) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click());
                if root_resp.hovered() {
                    ui.painter()
                        .rect_filled(root_row, egui::CornerRadius::same(3), p.hover);
                }
                paint_icon(
                    ui.painter(),
                    Rect::from_center_size(
                        Pos2::new(root_row.left() + 10.0, root_row.center().y),
                        Vec2::splat(10.0),
                    ),
                    if root_collapsed {
                        Icon::ChevronRight
                    } else {
                        Icon::ChevronDown
                    },
                    p.text3,
                );
                paint_icon(
                    ui.painter(),
                    Rect::from_center_size(
                        Pos2::new(root_row.left() + 24.0, root_row.center().y),
                        Vec2::splat(13.0),
                    ),
                    Icon::Folder,
                    Color32::from_rgb(0xdc, 0xd7, 0xaa),
                );
                let root_name = model.projects[index].name.clone();
                text_clipped(
                    ui.painter(),
                    Rect::from_min_max(
                        Pos2::new(root_row.left() + 36.0, root_row.top()),
                        Pos2::new(root_row.right() - 50.0, root_row.bottom()),
                    ),
                    &root_name,
                    FontId::proportional(12.5),
                    p.text,
                    Align2::LEFT_CENTER,
                );
                ui.painter().text(
                    Pos2::new(root_row.right() - 8.0, root_row.center().y),
                    Align2::RIGHT_CENTER,
                    format!(
                        "{} {}",
                        details.files.len(),
                        if details.files.len() == 1 {
                            "file"
                        } else {
                            "files"
                        }
                    ),
                    FontId::proportional(11.0),
                    p.text3,
                );
                if root_resp.clicked() {
                    model.projects[index].commit_files_root_collapsed = !root_collapsed;
                }

                if !root_collapsed {
                    for item in &flat_files {
                        let (row, response) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 20.0),
                            Sense::click(),
                        );
                        if response.hovered() {
                            ui.painter()
                                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
                        }
                        let indent = 8.0 + (item.depth + 1) as f32 * 12.0;
                        if item.is_dir {
                            let is_expanded = !model.projects[index]
                                .commit_file_dirs_collapsed
                                .contains(&item.path);
                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(row.left() + indent, row.center().y),
                                    Vec2::splat(10.0),
                                ),
                                if is_expanded {
                                    Icon::ChevronDown
                                } else {
                                    Icon::ChevronRight
                                },
                                p.text3,
                            );
                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(row.left() + indent + 12.0, row.center().y),
                                    Vec2::splat(12.0),
                                ),
                                Icon::Folder,
                                Color32::from_rgb(0xdc, 0xd7, 0xaa),
                            );
                            ui.painter().text(
                                Pos2::new(row.left() + indent + 24.0, row.center().y),
                                Align2::LEFT_CENTER,
                                &item.name,
                                FontId::proportional(12.5),
                                p.text,
                            );
                            ui.painter().text(
                                Pos2::new(row.right() - 8.0, row.center().y),
                                Align2::RIGHT_CENTER,
                                format!(
                                    "{} {}",
                                    item.total_files,
                                    if item.total_files == 1 {
                                        "file"
                                    } else {
                                        "files"
                                    }
                                ),
                                FontId::proportional(10.5),
                                p.text3,
                            );
                            if response.clicked() {
                                if is_expanded {
                                    model.projects[index]
                                        .commit_file_dirs_collapsed
                                        .insert(item.path.clone());
                                } else {
                                    model.projects[index]
                                        .commit_file_dirs_collapsed
                                        .remove(&item.path);
                                }
                            }
                        } else if let Some(file) = &item.file {
                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(row.left() + indent + 10.0, row.center().y),
                                    Vec2::splat(12.0),
                                ),
                                Icon::File,
                                crate::sidebar::file_tone(&item.name, theme),
                            );
                            let name_color = match file.kind {
                                lazecode_git::ChangeKind::Added => {
                                    Color32::from_rgb(0x73, 0xc9, 0x91)
                                }
                                lazecode_git::ChangeKind::Deleted => {
                                    Color32::from_rgb(0xe0, 0x6c, 0x75)
                                }
                                _ => Color32::from_rgb(0x58, 0xa6, 0xff),
                            };
                            text_clipped(
                                ui.painter(),
                                Rect::from_min_max(
                                    Pos2::new(row.left() + indent + 22.0, row.top()),
                                    Pos2::new(row.right() - 72.0, row.bottom()),
                                ),
                                &item.name,
                                FontId::proportional(12.5),
                                if response.hovered() {
                                    p.text
                                } else {
                                    name_color
                                },
                                Align2::LEFT_CENTER,
                            );
                            let mut sx = row.right() - 8.0;
                            if file.deletions > 0 {
                                let del_str = format!("-{}", file.deletions);
                                let gal = ui.painter().layout_no_wrap(
                                    del_str,
                                    FontId::monospace(10.5),
                                    Color32::from_rgb(0xe0, 0x6c, 0x75),
                                );
                                let del_w = gal.size().x;
                                ui.painter().galley(
                                    Pos2::new(sx - del_w, row.center().y - gal.size().y * 0.5),
                                    gal,
                                    Color32::from_rgb(0xe0, 0x6c, 0x75),
                                );
                                sx -= del_w + 4.0;
                            }
                            if file.additions > 0 {
                                let add_str = format!("+{}", file.additions);
                                let gal = ui.painter().layout_no_wrap(
                                    add_str,
                                    FontId::monospace(10.5),
                                    Color32::from_rgb(0x73, 0xc9, 0x91),
                                );
                                ui.painter().galley(
                                    Pos2::new(
                                        sx - gal.size().x,
                                        row.center().y - gal.size().y * 0.5,
                                    ),
                                    gal,
                                    Color32::from_rgb(0x73, 0xc9, 0x91),
                                );
                            }
                            if response.double_clicked() {
                                if let Some(range) = range {
                                    model.queue(Command::OpenCompareCommitsDiff {
                                        from: range.from.clone(),
                                        to: range.to.clone(),
                                        path: file.path.clone(),
                                    });
                                } else {
                                    model.queue(Command::OpenCommitDiff {
                                        commit: details.info.id.clone(),
                                        path: file.path.clone(),
                                    });
                                }
                            }

                            if response.secondary_clicked() {
                                rt.open_menu(
                                    crate::runtime::MenuKind::CommitDetailsFileContext {
                                        project: index,
                                    },
                                    row,
                                    Some(if let Some(range) = range {
                                        format!(
                                            "r\u{1}{}\u{1}{}\u{1}{}",
                                            range.from, range.to, file.path
                                        )
                                    } else {
                                        format!("{}\u{1}{}", details.info.id, file.path)
                                    }),
                                );
                            }
                        }
                    }
                }
            });
    });
}

#[allow(dead_code)]
pub(crate) fn time_ago(seconds: i64, zh: bool) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let delta = (now - seconds).max(0);
    if zh {
        if delta < 60 {
            "刚刚".to_string()
        } else if delta < 3600 {
            format!("{} 分钟前", delta / 60)
        } else if delta < 86_400 {
            format!("{} 小时前", delta / 3600)
        } else if delta < 86_400 * 30 {
            format!("{} 天前", delta / 86_400)
        } else if delta < 86_400 * 365 {
            format!("{} 个月前", delta / (86_400 * 30))
        } else {
            format!("{} 年前", delta / (86_400 * 365))
        }
    } else if delta < 60 {
        "just now".to_string()
    } else if delta < 3600 {
        format!("{}m ago", delta / 60)
    } else if delta < 86_400 {
        format!("{}h ago", delta / 3600)
    } else if delta < 86_400 * 30 {
        format!("{}d ago", delta / 86_400)
    } else if delta < 86_400 * 365 {
        format!("{}mo ago", delta / (86_400 * 30))
    } else {
        format!("{}y ago", delta / (86_400 * 365))
    }
}
