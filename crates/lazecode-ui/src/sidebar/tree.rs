//! Explorer tree: file tree, git decorations and search matches.

use super::prelude::*;
use super::{file_tone, side_head, SideAction, HEAD_H, ROW_H};

pub(super) fn explorer(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
) {
    let p = theme.palette;
    let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), HEAD_H));
    let actions = [
        SideAction {
            icon: Icon::Locate,
            id: "locate",
            active: false,
            tooltip: crate::tr!(model, "Select Opened File", "在树中定位已打开的文件").to_string(),
        },
        SideAction {
            icon: Icon::AutoLocate,
            id: "auto-locate",
            active: model.ui.auto_locate,
            tooltip: crate::tr!(model, "Always Select Opened File", "自动定位树中打开的文件")
                .to_string(),
        },
        SideAction {
            icon: Icon::CollapseAll,
            id: "collapse-all",
            active: false,
            tooltip: crate::tr!(model, "Collapse All", "全部折叠").to_string(),
        },
        SideAction {
            icon: Icon::Folder,
            id: "open-folder",
            active: false,
            tooltip: crate::tr!(model, "Open Project...", "打开项目...").to_string(),
        },
    ];
    if let Some(action) = side_head(
        ui,
        theme,
        head,
        crate::tr!(model, "Project", "项目"),
        &actions,
    ) {
        match action {
            0 => {
                model.locate_active_file(false);
            }
            1 => {
                model.ui.auto_locate = !model.ui.auto_locate;
                if model.ui.auto_locate {
                    model.locate_active_file(true);
                }
                model.save_workspace();
            }
            2 => {
                if let Some(project) = model.active_project_mut() {
                    project.expanded.clear();
                }
            }
            3 => {
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    let _ = model.open_project(folder);
                }
            }
            _ => {}
        }
    }

    let list = Rect::from_min_max(Pos2::new(rect.left(), head.bottom()), rect.max);
    ui.painter().rect_filled(list, 0.0, p.sidebar);
    draw_tree(ui, model, rt, theme, list);
}

pub(super) fn draw_tree(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
) {
    let p = theme.palette;
    let active_index = model
        .active_project
        .expect("sidebar drawn without an active project");
    let filter = model.ui.tree_filter.to_lowercase();
    let (rows, changes): (Vec<VisibleRow>, HashMap<String, char>) = {
        let project = &model.projects[active_index];
        let tree = project.active_tree();
        let changes = project
            .changes()
            .iter()
            .map(|change| (change.path.clone(), change.letter()))
            .collect();
        let mut rows = Vec::new();
        if project.is_single_file {
            if filter.is_empty() {
                rows = visible_rows(tree, &project.expanded);
            } else {
                collect_matches(&tree.nodes, &filter, 0, &mut rows);
            }
        } else {
            // 对齐稳定版：第一行为项目根目录节点 (ProjectDrawer tree-root-item)
            rows.push(VisibleRow {
                path: std::path::PathBuf::new(),
                name: project.name.clone(),
                kind: NodeKind::Folder,
                depth: 0,
                expanded: project.root_expanded,
            });
            if project.root_expanded {
                if filter.is_empty() {
                    for mut r in visible_rows(tree, &project.expanded) {
                        r.depth += 1;
                        rows.push(r);
                    }
                } else {
                    let mut matches = Vec::new();
                    collect_matches(&tree.nodes, &filter, 1, &mut matches);
                    rows.extend(matches);
                }
            }
        }
        (rows, changes)
    };

    let show_filter = !model.ui.tree_filter.is_empty();
    let list_rect = if show_filter {
        let input_rect = Rect::from_min_size(
            Pos2::new(rect.left() + 10.0, rect.top() + 6.0),
            Vec2::new(rect.width() - 20.0, 24.0),
        );
        ui.painter()
            .rect_filled(input_rect, egui::CornerRadius::same(5), p.panel);
        ui.painter().rect_stroke(
            input_rect,
            egui::CornerRadius::same(5),
            Stroke::new(1.0, p.border_soft),
            StrokeKind::Inside,
        );
        paint_icon(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(input_rect.left() + 13.0, input_rect.center().y),
                Vec2::splat(12.0),
            ),
            Icon::Search,
            p.text3,
        );
        let field = Rect::from_min_max(
            Pos2::new(input_rect.left() + 24.0, input_rect.top() + 2.0),
            Pos2::new(input_rect.right() - 26.0, input_rect.bottom() - 2.0),
        );
        let mut filter_value = model.ui.tree_filter.clone();
        let response = ui.put(
            field,
            TextEdit::singleline(&mut filter_value)
                .frame(egui::Frame::NONE)
                .align(egui::Align2::LEFT_CENTER)
                .hint_text(crate::tr!(model, "Filter files", "过滤文件"))
                .font(FontId::proportional(12.5)),
        );
        if response.changed() {
            model.ui.tree_filter = filter_value;
        }
        Rect::from_min_max(Pos2::new(rect.left(), input_rect.bottom() + 4.0), rect.max)
    } else {
        Rect::from_min_max(Pos2::new(rect.left(), rect.top() + 2.0), rect.max)
    };
    let _active_tab = model.projects[active_index].active_tab;
    let active_rel = model.projects[active_index]
        .active_tab
        .and_then(|id| model.projects[active_index].tab_by_id(id))
        .and_then(|tab| {
            tab.path().map(|path| {
                std::path::Path::new(path)
                    .strip_prefix(model.projects[active_index].active_root())
                    .map(|rel| rel.to_path_buf())
                    .unwrap_or_else(|_| std::path::PathBuf::from(path))
            })
        });
    let target_reveal = model.ui.tree_reveal.clone();
    // `show_rows` only builds visible rows, so scroll before virtualization
    // and clear the request after the frame.
    let reveal_offset = target_reveal
        .as_ref()
        .and_then(|(rel, _)| rows.iter().position(|row| &row.path == rel))
        .map(|index| (index as f32 * ROW_H - list_rect.height() * 0.5 + ROW_H * 0.5).max(0.0));
    crate::app_ui::region(ui, list_rect, "tree-scroll", |ui| {
        // `show_rows` adds item spacing between virtual rows; the rows already
        // have a fixed height, so drop the gap.
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut area = egui::ScrollArea::vertical()
            .id_salt("tree")
            .auto_shrink([false, false]);
        if let Some(offset) = reveal_offset {
            area = area.vertical_scroll_offset(offset);
        }
        area.show_rows(ui, ROW_H, rows.len(), |ui, range| {
            for index in range {
                let row = &rows[index];
                let (draw_rect, response) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
                let selected = model.ui.tree_selection.contains(&row.path)
                    || (model.ui.tree_selection.is_empty()
                        && (model
                            .ui
                            .tree_selected
                            .as_ref()
                            .is_some_and(|sel| sel == &row.path)
                            || (model.ui.tree_selected.is_none()
                                && active_rel.as_ref().is_some_and(|rel| rel == &row.path))));
                if selected {
                    ui.painter()
                        .rect_filled(draw_rect, egui::CornerRadius::same(4), p.active);
                } else if response.hovered() {
                    ui.painter()
                        .rect_filled(draw_rect, egui::CornerRadius::same(4), p.hover);
                }
                // The active editor file keeps its own marker, independent of
                // the multi-selection: locating a tab must not wipe the work
                // set, and a stale selection must not hide where the active
                // file lives.
                if active_rel.as_ref().is_some_and(|rel| rel == &row.path) {
                    ui.painter().rect_filled(
                        Rect::from_min_size(
                            Pos2::new(draw_rect.left() + 1.0, draw_rect.top() + 3.0),
                            Vec2::new(2.0, ROW_H - 6.0),
                        ),
                        egui::CornerRadius::same(1),
                        p.accent,
                    );
                }
                if let Some((target_rel, quiet)) = &target_reveal {
                    if &row.path == target_rel {
                        if *quiet {
                            response.scroll_to_me(None);
                        } else {
                            response.scroll_to_me(Some(egui::Align::Center));
                        }
                    }
                }
                if let Some((flash_path, at)) = &model.ui.locate_flash {
                    if flash_path == &row.path {
                        let elapsed = at.elapsed().as_secs_f32();
                        if elapsed < 1.5 {
                            let pulse = ((1.5 - elapsed) / 1.5).max(0.0);
                            ui.painter().rect_filled(
                                draw_rect,
                                egui::CornerRadius::same(4),
                                p.accent.gamma_multiply(pulse * 0.45),
                            );
                        }
                    }
                }
                let is_git_ignored = model.projects[active_index].is_git_ignored(&row.path);
                let pad = 7.0 + row.depth as f32 * 13.0;
                let twisty = Rect::from_center_size(
                    Pos2::new(draw_rect.left() + pad + 6.0, draw_rect.center().y),
                    Vec2::splat(12.0),
                );
                if row.kind == NodeKind::Folder {
                    paint_icon(
                        ui.painter(),
                        twisty,
                        if row.expanded {
                            Icon::ChevronDown
                        } else {
                            Icon::ChevronRight
                        },
                        if is_git_ignored {
                            p.git_ignored
                        } else {
                            p.text3
                        },
                    );
                }
                let icon_rect = Rect::from_center_size(
                    Pos2::new(draw_rect.left() + pad + 24.0, draw_rect.center().y),
                    Vec2::splat(14.0),
                );
                match row.kind {
                    NodeKind::Folder => paint_icon(
                        ui.painter(),
                        icon_rect,
                        if row.expanded {
                            Icon::FolderOpen
                        } else {
                            Icon::Folder
                        },
                        if is_git_ignored {
                            p.git_ignored
                        } else {
                            p.text3
                        },
                    ),
                    NodeKind::File => {
                        if is_git_ignored {
                            paint_icon(ui.painter(), icon_rect, Icon::File, p.git_ignored);
                        } else {
                            let lower = row.name.to_lowercase();
                            if lower == ".gitignore"
                                || lower == ".gitattributes"
                                || lower.ends_with(".ignore")
                            {
                                crate::icons_svg::paint_svg(
                                    ui.painter(),
                                    icon_rect,
                                    "file_ignore",
                                    Color32::WHITE,
                                );
                            } else if lower.ends_with(".toml") {
                                crate::icons_svg::paint_svg(
                                    ui.painter(),
                                    icon_rect,
                                    "file_toml",
                                    Color32::WHITE,
                                );
                            } else if lower.ends_with(".md") {
                                crate::icons_svg::paint_svg(
                                    ui.painter(),
                                    icon_rect,
                                    "file_md",
                                    Color32::WHITE,
                                );
                            } else {
                                paint_icon(
                                    ui.painter(),
                                    icon_rect,
                                    Icon::File,
                                    file_tone(&row.name, theme),
                                );
                            }
                        }
                    }
                }
                let letter = changes.get(row.path.to_string_lossy().as_ref()).copied();
                let text_rect = Rect::from_min_max(
                    Pos2::new(draw_rect.left() + pad + 34.0, draw_rect.top()),
                    Pos2::new(
                        draw_rect.right() - if letter.is_some() { 30.0 } else { 8.0 },
                        draw_rect.bottom(),
                    ),
                );
                let text_color = if let Some(letter) = letter {
                    match letter {
                        'M' => Color32::from_rgb(56, 178, 240),
                        'A' => p.green,
                        'D' => p.rose,
                        'U' => p.orange,
                        'C' => p.rose,
                        _ => p.blue,
                    }
                } else if is_git_ignored {
                    p.git_ignored
                } else if row.kind == NodeKind::Folder {
                    p.text
                } else {
                    p.text2
                };
                if row.path.as_os_str().is_empty() {
                    // Project Root Row (对齐稳定版 ProjectDrawer tree-root-item)
                    let text_color = if selected { p.text_bright } else { p.text };
                    let name_galley = ui.painter().layout_no_wrap(
                        row.name.clone(),
                        FontId::proportional(13.0),
                        text_color,
                    );
                    let name_w = name_galley.rect.width();
                    ui.painter().galley(
                        Pos2::new(
                            draw_rect.left() + pad + 34.0,
                            draw_rect.center().y - name_galley.rect.height() / 2.0,
                        ),
                        name_galley,
                        text_color,
                    );
                    let display_path =
                        lazecode_fs::display_path(model.projects[active_index].active_root());
                    let path_rect = Rect::from_min_max(
                        Pos2::new(draw_rect.left() + pad + 38.0 + name_w, draw_rect.top()),
                        Pos2::new(draw_rect.right() - 8.0, draw_rect.bottom()),
                    );
                    if path_rect.width() > 10.0 {
                        text_clipped(
                            ui.painter(),
                            path_rect,
                            &display_path,
                            FontId::proportional(11.0),
                            p.text3,
                            Align2::LEFT_CENTER,
                        );
                    }
                } else {
                    text_clipped(
                        ui.painter(),
                        text_rect,
                        &row.name,
                        FontId::proportional(13.0),
                        if selected { p.text_bright } else { text_color },
                        Align2::LEFT_CENTER,
                    );
                }
                if let Some(letter) = letter {
                    ui.painter().text(
                        Pos2::new(draw_rect.right() - 14.0, draw_rect.center().y),
                        Align2::CENTER_CENTER,
                        letter.to_string(),
                        FontId::monospace(11.0),
                        match letter {
                            'U' => p.green,
                            'D' => p.rose,
                            'A' => p.green,
                            'M' => Color32::from_rgb(56, 178, 240),
                            _ => p.amber,
                        },
                    );
                } else if row.kind == NodeKind::Folder {
                    let has_child_change = if row.path.as_os_str().is_empty() {
                        !changes.is_empty()
                    } else {
                        let folder_prefix = format!("{}/", row.path.display());
                        changes.keys().any(|k| k.starts_with(&folder_prefix))
                    };
                    if has_child_change {
                        ui.painter().circle_filled(
                            Pos2::new(draw_rect.right() - 14.0, draw_rect.center().y),
                            3.2,
                            Color32::from_rgb(56, 178, 240),
                        );
                    }
                }
                if response.secondary_clicked() {
                    if !model.ui.tree_selection.contains(&row.path) {
                        model.ui.tree_selection.clear();
                        model.ui.tree_selection.insert(row.path.clone());
                        model.ui.tree_selected = Some(row.path.clone());
                    }
                    let anchor = ui
                        .input(|i| i.pointer.hover_pos())
                        .map(|p| Rect::from_min_size(p, Vec2::splat(1.0)))
                        .unwrap_or(response.rect);
                    rt.open_menu(
                        crate::runtime::MenuKind::TreeContext {
                            project: active_index,
                            is_dir: matches!(row.kind, NodeKind::Folder),
                        },
                        anchor,
                        Some(row.path.to_string_lossy().to_string()),
                    );
                }
                if response.double_clicked() {
                    match row.kind {
                        NodeKind::Folder => {
                            if row.path.as_os_str().is_empty() {
                                let project = &mut model.projects[active_index];
                                project.root_expanded = !project.root_expanded;
                            } else {
                                let project = &mut model.projects[active_index];
                                if project.expanded.contains(&row.path) {
                                    project.expanded.remove(&row.path);
                                } else {
                                    project.expanded.insert(row.path.clone());
                                }
                            }
                        }
                        NodeKind::File => {
                            let path = model.projects[active_index].active_root().join(&row.path);
                            match model.open_file_in(active_index, path) {
                                Ok(id) => {
                                    model.ui.reveal_editor_area();
                                    rt.set_editor_focus(id);
                                }
                                Err(err) => model.toast(err, ToastKind::Error),
                            }
                        }
                    }
                } else if response.clicked() {
                    let cmd_down = ui.input(|i| i.modifiers.command);
                    let shift_down = ui.input(|i| i.modifiers.shift);
                    if cmd_down {
                        if model.ui.tree_selection.contains(&row.path) {
                            model.ui.tree_selection.remove(&row.path);
                        } else {
                            model.ui.tree_selection.insert(row.path.clone());
                        }
                        model.ui.tree_selected = Some(row.path.clone());
                    } else if shift_down {
                        let anchor = model.ui.tree_selected.clone();
                        if let Some(anchor) = anchor {
                            let anchor_idx =
                                rows.iter().position(|r| r.path == anchor).unwrap_or(index);
                            let (start, end) = if anchor_idx < index {
                                (anchor_idx, index)
                            } else {
                                (index, anchor_idx)
                            };
                            model.ui.tree_selection.clear();
                            for r in &rows[start..=end] {
                                model.ui.tree_selection.insert(r.path.clone());
                            }
                        } else {
                            model.ui.tree_selection.insert(row.path.clone());
                            model.ui.tree_selected = Some(row.path.clone());
                        }
                    } else {
                        model.ui.tree_selection.clear();
                        model.ui.tree_selection.insert(row.path.clone());
                        model.ui.tree_selected = Some(row.path.clone());
                        if row.kind == NodeKind::Folder {
                            if row.path.as_os_str().is_empty() {
                                let project = &mut model.projects[active_index];
                                project.root_expanded = !project.root_expanded;
                            } else {
                                let project = &mut model.projects[active_index];
                                if project.expanded.contains(&row.path) {
                                    project.expanded.remove(&row.path);
                                } else {
                                    project.expanded.insert(row.path.clone());
                                }
                            }
                        }
                    }
                }
            }
        });
    });
    if target_reveal.is_some() {
        model.ui.tree_reveal = None;
    }
}

fn collect_matches(
    nodes: &[lazecode_fs::tree::TreeNode],
    filter: &str,
    depth: usize,
    out: &mut Vec<VisibleRow>,
) {
    for node in nodes {
        match node.kind {
            NodeKind::Folder => {
                collect_matches(&node.children, filter, depth + 1, out);
            }
            NodeKind::File => {
                if node.path.to_string_lossy().to_lowercase().contains(filter) {
                    out.push(VisibleRow {
                        path: node.path.clone(),
                        name: node.name.clone(),
                        kind: NodeKind::File,
                        depth,
                        expanded: false,
                    });
                }
            }
        }
    }
}
