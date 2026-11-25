//! Shelf sidebar: saved shelves with restore/patch/rename actions.

use super::active;
use super::prelude::*;

fn shelf_entry_files(patch: &str) -> Vec<String> {
    patch
        .lines()
        .filter_map(|line| line.strip_prefix("+++ b/"))
        .map(str::to_string)
        .collect()
}

/// 架藏时间：unix 秒 → YYYY/M/D。
fn shelf_date(seconds: u64) -> String {
    unsafe {
        let t = seconds as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return String::new();
        }
        format!("{}/{}/{}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday)
    }
}

/// 稳定版 Shelf 页签：架藏列表（可展开文件）+ Recently Deleted + 右键。
pub(super) fn shelf_view(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
) {
    let p = theme.palette;
    let root = active(model).active_root().to_path_buf();
    if rt.shelf_loaded.as_deref() != Some(root.as_path()) {
        rt.shelf_entries = lazecode_git::shelf_list(&root).unwrap_or_default();
        rt.shelf_deleted = lazecode_git::shelf_list_deleted(&root).unwrap_or_default();
        rt.shelf_files_map.clear();
        for entry in &rt.shelf_entries {
            rt.shelf_files_map
                .insert(entry.name.clone(), shelf_entry_files(&entry.patch));
        }
        rt.shelf_loaded = Some(root.clone());
        rt.shelf_selected = None;
        rt.shelf_file_sel.clear();
    }

    #[derive(Clone)]
    enum ShRow {
        Header {
            name: String,
            deleted: bool,
            count: usize,
        },
        File {
            shelf: String,
            path: String,
            last: bool,
        },
        DeletedBanner {
            count: usize,
        },
    }
    let mut rows: Vec<ShRow> = Vec::new();
    let collapsed = model
        .active_project()
        .map(|project| project.collapsed_lists.clone())
        .unwrap_or_default();
    for entry in &rt.shelf_entries {
        let files = rt
            .shelf_files_map
            .get(&entry.name)
            .cloned()
            .unwrap_or_default();
        rows.push(ShRow::Header {
            name: entry.name.clone(),
            deleted: false,
            count: files.len(),
        });
        if !collapsed.contains(&format!("__shelf__::{}", entry.name)) {
            let total = files.len();
            for (i, path) in files.iter().enumerate() {
                rows.push(ShRow::File {
                    shelf: entry.name.clone(),
                    path: path.clone(),
                    last: i + 1 == total,
                });
            }
        }
    }
    if !rt.shelf_deleted.is_empty() {
        rows.push(ShRow::DeletedBanner {
            count: rt.shelf_deleted.len(),
        });
        for entry in &rt.shelf_deleted {
            let files = shelf_entry_files(&entry.patch);
            rows.push(ShRow::Header {
                name: entry.name.clone(),
                deleted: true,
                count: files.len(),
            });
        }
    }

    // Keyboard: ↑/↓/Home/End step the shelf entries (file rows stay
    // mouse-only) and the selected entry's patch is what the footer actions
    // operate on.
    let keys_free = !rt.blocks_input(model) && !rt.typing_surface_owns_keys(model, ui.ctx());
    let mut nav_scroll: Option<f32> = None;
    if keys_free {
        if let Some(mv) = crate::list_nav::hovered_move(ui, rect) {
            let entries: Vec<(usize, String)> = rows
                .iter()
                .enumerate()
                .filter_map(|(index, row)| match row {
                    ShRow::Header { name, .. } => Some((index, name.clone())),
                    _ => None,
                })
                .collect();
            let current = rt
                .shelf_selected
                .as_ref()
                .and_then(|sel| entries.iter().position(|(_, name)| name == sel));
            if let Some(next) = crate::list_nav::stepped(current, entries.len(), mv) {
                let (row_index, name) = entries[next].clone();
                rt.shelf_selected = Some(name);
                rt.shelf_file_sel.clear();
                nav_scroll = Some((row_index as f32 * 24.0 - rect.height() * 0.5 + 12.0).max(0.0));
            }
        }
    }

    crate::app_ui::region(ui, rect, "shelf-rows", |ui| {
        let mut area = egui::ScrollArea::vertical()
            .id_salt("shelf-scroll")
            .auto_shrink([false, false]);
        if let Some(offset) = nav_scroll {
            area = area.vertical_scroll_offset(offset);
        }
        area.show_rows(ui, 24.0, rows.len(), |ui, range| {
            for row_index in range {
                let Some(row) = rows.get(row_index) else {
                    continue;
                };
                match row {
                    ShRow::DeletedBanner { count } => {
                        let (rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 24.0),
                            Sense::hover(),
                        );
                        ui.painter().hline(
                            rect.x_range(),
                            rect.top() + 4.0,
                            Stroke::new(1.0, p.border_soft),
                        );
                        ui.painter().text(
                            Pos2::new(rect.left() + 12.0, rect.center().y + 2.0),
                            Align2::LEFT_CENTER,
                            crate::tr!(model, "Recently Deleted · {count}", "最近删除 · {count}")
                                .replace("{count}", &count.to_string()),
                            FontId::proportional(12.0),
                            p.text2,
                        );
                    }
                    ShRow::Header {
                        name,
                        deleted,
                        count,
                    } => {
                        let (rect, response) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 24.0),
                            Sense::click(),
                        );
                        let selected = rt.shelf_selected.as_deref() == Some(name.as_str())
                            && rt.shelf_file_sel.is_empty();
                        if selected || response.hovered() {
                            ui.painter().rect_filled(
                                rect.shrink2(Vec2::new(2.0, 0.0)),
                                egui::CornerRadius::same(4),
                                if selected { p.selected_row } else { p.hover },
                            );
                        }
                        let expanded =
                            !collapsed.contains(&format!("__shelf__::{name}")) && !deleted;
                        paint_icon(
                            ui.painter(),
                            Rect::from_center_size(
                                Pos2::new(rect.left() + 10.0, rect.center().y),
                                Vec2::splat(11.0),
                            ),
                            if expanded {
                                Icon::ChevronDown
                            } else {
                                Icon::ChevronRight
                            },
                            p.text3,
                        );
                        let mut name_rect = rect;
                        name_rect.min.x = rect.left() + 24.0;
                        name_rect.max.x = rect.right() - 140.0;
                        let mut name_color = if *deleted { p.text3 } else { p.text };
                        if response.hovered() && !selected {
                            name_color = p.text_bright;
                        }
                        text_clipped(
                            ui.painter(),
                            name_rect,
                            name,
                            FontId::proportional(12.5),
                            name_color,
                            Align2::LEFT_CENTER,
                        );
                        if *deleted {
                            // 删除行：删除线（稳定版 line-through 近似）。
                            ui.painter().hline(
                                (rect.left() + 24.0)
                                    ..=(rect.left() + 24.0 + 7.0 * name.chars().count() as f32),
                                rect.center().y,
                                Stroke::new(1.0, p.text3),
                            );
                        }
                        ui.painter().text(
                            Pos2::new(rect.right() - 10.0, rect.center().y),
                            Align2::RIGHT_CENTER,
                            crate::tr!(model, "{n} files, {date}", "{n} 个文件，{date}")
                                .replace("{n}", &count.to_string())
                                .replace(
                                    "{date}",
                                    &shelf_date(
                                        rt.shelf_deleted
                                            .iter()
                                            .chain(rt.shelf_entries.iter())
                                            .find(|entry| &entry.name == name)
                                            .map(|entry| entry.created)
                                            .unwrap_or(0),
                                    ),
                                ),
                            FontId::proportional(11.0),
                            p.text3,
                        );
                        if response.clicked() {
                            rt.shelf_selected = Some(name.clone());
                            rt.shelf_file_sel.clear();
                        }
                        if response.secondary_clicked() {
                            let payload = format!("{name}\u{1}{}\u{1}", u8::from(*deleted));
                            rt.open_menu(
                                crate::runtime::MenuKind::ShelfContext {
                                    project: model.active_project.unwrap_or(0),
                                },
                                rect,
                                Some(payload),
                            );
                        }
                    }
                    ShRow::File {
                        shelf, path, last, ..
                    } => {
                        let (rect, response) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 24.0),
                            Sense::click(),
                        );
                        let row_rect = Rect::from_min_max(
                            Pos2::new(rect.left() + 22.0, rect.top() + 1.0),
                            Pos2::new(rect.right() - 2.0, rect.bottom() - 1.0),
                        );
                        let selected = rt.shelf_file_sel.contains(path);
                        if selected || response.hovered() {
                            ui.painter().rect_filled(
                                row_rect,
                                egui::CornerRadius::same(4),
                                if selected { p.selected_row } else { p.hover },
                            );
                        }
                        let name = path.rsplit('/').next().unwrap_or(path);
                        paint_icon(
                            ui.painter(),
                            Rect::from_center_size(
                                Pos2::new(row_rect.left() + 16.0, row_rect.center().y),
                                Vec2::splat(13.0),
                            ),
                            Icon::File,
                            p.text3,
                        );
                        text_clipped(
                            ui.painter(),
                            Rect::from_min_max(
                                Pos2::new(row_rect.left() + 28.0, row_rect.top()),
                                Pos2::new(row_rect.right() - 120.0, row_rect.bottom()),
                            ),
                            name,
                            FontId::proportional(12.0),
                            p.text,
                            Align2::LEFT_CENTER,
                        );
                        text_clipped(
                            ui.painter(),
                            Rect::from_min_max(
                                Pos2::new(row_rect.right() - 220.0, row_rect.top()),
                                Pos2::new(row_rect.right() - 10.0, row_rect.bottom()),
                            ),
                            path.trim_end_matches(name).trim_end_matches('/'),
                            FontId::proportional(11.0),
                            p.text3,
                            Align2::RIGHT_CENTER,
                        );
                        if response.clicked() {
                            if !rt.shelf_selected.as_deref().is_some_and(|s| s == shelf) {
                                rt.shelf_selected = Some(shelf.clone());
                                rt.shelf_file_sel.clear();
                            }
                            if let Some(pos) =
                                rt.shelf_file_sel.iter().position(|entry| entry == path)
                            {
                                rt.shelf_file_sel.remove(pos);
                            } else {
                                rt.shelf_file_sel.push(path.clone());
                            }
                        }
                        if response.secondary_clicked() {
                            let paths = if rt.shelf_file_sel.contains(path) {
                                rt.shelf_file_sel.clone()
                            } else {
                                vec![path.clone()]
                            };
                            let payload = std::iter::once(shelf.clone())
                                .chain(std::iter::once("0".to_string()))
                                .chain(paths)
                                .collect::<Vec<_>>()
                                .join("\u{1}");
                            rt.open_menu(
                                crate::runtime::MenuKind::ShelfContext {
                                    project: model.active_project.unwrap_or(0),
                                },
                                row_rect,
                                Some(payload),
                            );
                        }
                        let _ = last;
                    }
                }
            }
        });
    });
}
