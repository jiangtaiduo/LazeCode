use std::collections::{HashMap, HashSet};

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2};
use lazecode_core::{AppModel, Command, ProjectState, SidebarView, ToastKind};
use lazecode_fs::tree::{visible_rows, NodeKind, VisibleRow};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::{CommitTab, UiRuntime};
use crate::theme::Theme;
use crate::widgets::{
    checkbox, checkbox_tri, icon_button, live_dot, primary_button, secondary_button_enabled,
    text_clipped, TriState,
};

const HEAD_H: f32 = 36.0;
const FOOT_H: f32 = 25.0;
const ROW_H: f32 = 25.0;

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.sidebar);
    let Some(_) = model.active_project() else {
        let text = crate::tr!(model, "No project open", "未打开项目");
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(12.5),
            p.text3,
        );
        return;
    };
    match model.ui.sidebar_view {
        SidebarView::Files => explorer(ui, model, rt, theme, rect),
        SidebarView::Git => source_control(ui, model, rt, theme, rect),
        SidebarView::Consoles => consoles(ui, model, rt, theme, rect),
        SidebarView::Compare => compare_drawer(ui, model, rt, theme, rect),
    }
}

struct SideAction {
    icon: Icon,
    id: &'static str,
    active: bool,
    tooltip: String,
}

fn side_head(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    title: &str,
    actions: &[SideAction],
) -> Option<usize> {
    let p = theme.palette;
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(12.5),
        p.text3,
    );
    let mut right = rect.right() - 8.0;
    let mut clicked = None;
    for (index, action) in actions.iter().enumerate().rev() {
        let button =
            Rect::from_center_size(Pos2::new(right - 11.0, rect.center().y), Vec2::splat(24.0));
        right -= 25.0;
        let resp = icon_button(ui, button, action.icon, action.active, theme);
        if !action.tooltip.is_empty() && resp.hovered() {
            resp.show_tooltip_text(&action.tooltip);
        }
        if resp.clicked() {
            clicked = Some(index);
        }
        let _ = action.id;
    }
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    clicked
}

pub(crate) fn file_tone(path: &str, theme: &Theme) -> Color32 {
    let p = theme.palette;
    match path.rsplit('.').next().unwrap_or_default() {
        "rs" => p.orange,
        "ts" | "tsx" => p.blue,
        "js" | "jsx" | "mjs" => p.amber,
        "json" | "jsonc" => p.amber,
        "css" | "scss" | "less" => p.violet,
        "html" | "htm" => p.orange,
        "py" => p.blue,
        "go" => p.green,
        "sh" | "zsh" | "bash" => p.green,
        "md" | "markdown" => p.text2,
        "toml" | "yaml" | "yml" => p.pink,
        _ => p.text3,
    }
}

fn explorer(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
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
                model.reload_active_tree();
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

fn draw_tree(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    let active_index = model.active_project.unwrap();
    let filter = model.ui.tree_filter.to_lowercase();
    let (rows, changes): (Vec<VisibleRow>, HashMap<String, char>) = {
        let project = &model.projects[active_index];
        let changes = project
            .changes()
            .iter()
            .map(|change| (change.path.clone(), change.letter()))
            .collect();
        let mut rows = Vec::new();
        if project.is_single_file {
            if filter.is_empty() {
                rows = visible_rows(&project.tree, &project.expanded);
            } else {
                collect_matches(&project.tree.nodes, &filter, 0, &mut rows);
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
                    for mut r in visible_rows(&project.tree, &project.expanded) {
                        r.depth += 1;
                        rows.push(r);
                    }
                } else {
                    let mut matches = Vec::new();
                    collect_matches(&project.tree.nodes, &filter, 1, &mut matches);
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
                    .strip_prefix(&model.projects[active_index].root)
                    .map(|rel| rel.to_path_buf())
                    .unwrap_or_else(|_| std::path::PathBuf::from(path))
            })
        });
    let target_reveal = model.ui.tree_reveal.clone();
    // `show_rows` only builds visible rows, so a row outside the viewport can
    // never call `scroll_to_me`. Move the scroll offset before virtualization
    // and clear the request after the frame.
    let reveal_offset = target_reveal
        .as_ref()
        .and_then(|(rel, _)| rows.iter().position(|row| &row.path == rel))
        .map(|index| (index as f32 * ROW_H - list_rect.height() * 0.5 + ROW_H * 0.5).max(0.0));
    super::app_ui::region(ui, list_rect, "tree-scroll", |ui| {
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
                        lazecode_fs::display_path(&model.projects[active_index].root);
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
                                model.reload_active_tree();
                            }
                        }
                        NodeKind::File => {
                            let settings = model.settings.clone();
                            let path = model.projects[active_index].root.join(&row.path);
                            match model.projects[active_index].open_file(path, &settings) {
                                Ok(id) => rt.set_editor_focus(id),
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
                                model.reload_active_tree();
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

/// 源控列表文件行（稳定版 `.change-file-row` 22px 单行：勾选框 + 文件
/// 图标 + 状态色文件名 + 右对齐目录提示 + hover 回滚）。返回
/// (勾选翻转, 单击, 双击, 右键, hover-Rollback 点击)。
#[allow(clippy::too_many_arguments)]
fn change_row(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    change: &lazecode_git::Change,
    checked: bool,
    selected: bool,
    radius: egui::CornerRadius,
    depth: usize,
    show_dir_hint: bool,
    is_ignored: bool,
) -> (bool, bool, bool, bool, bool) {
    let p = theme.palette;
    let response = ui.interact(
        rect,
        ui.scope_id().with(("change", change.path.clone())),
        Sense::click(),
    );
    if selected {
        ui.painter().rect_filled(rect, radius, p.selected_row);
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(3), p.hover);
    }
    let indent = rect.left() + 6.0 + depth as f32 * 14.0;
    let mut checked = checked;
    let chk_offset = if is_ignored {
        0.0
    } else {
        let checkbox_rect =
            Rect::from_center_size(Pos2::new(indent + 7.0, rect.center().y), Vec2::splat(14.0));
        let toggle = checkbox(ui, checkbox_rect, checked, theme);
        if toggle.clicked() {
            checked = !checked;
        }
        15.0
    };
    let name = change.path.rsplit('/').next().unwrap_or(&change.path);
    let icon_rect = Rect::from_center_size(
        Pos2::new(indent + 7.0 + chk_offset, rect.center().y),
        Vec2::splat(14.0),
    );
    paint_icon(ui.painter(), icon_rect, Icon::File, p.text3);
    let text_left = indent + 18.0 + chk_offset;
    let letter = change.letter();
    let name_color = if is_ignored {
        p.text3
    } else if change.untracked {
        Color32::from_rgb(0xd0, 0x87, 0x70)
    } else {
        match letter {
            'A' | 'U' => p.green,
            'D' => p.rose,
            _ => p.blue,
        }
    };
    let name_right = if show_dir_hint {
        rect.right() - 100.0
    } else {
        rect.right() - 26.0
    };
    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(text_left, rect.top()),
            Pos2::new(name_right, rect.bottom()),
        ),
        name,
        FontId::proportional(13.0),
        name_color,
        Align2::LEFT_CENTER,
    );
    if show_dir_hint {
        let dir = {
            let path = change.path.trim_end_matches(name);
            path.trim_end_matches('/').to_string()
        };
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(rect.right() - 220.0, rect.top()),
                Pos2::new(rect.right() - 26.0, rect.bottom()),
            ),
            &dir,
            FontId::proportional(12.0),
            p.text3,
            Align2::RIGHT_CENTER,
        );
    }
    // 行尾 hover 回滚（稳定版：未跟踪/被忽略不显示）。
    let rollback_rect = Rect::from_center_size(
        Pos2::new(rect.right() - 18.0, rect.center().y),
        Vec2::splat(16.0),
    );
    let mut discard_clicked = false;
    if response.hovered()
        && !change.untracked
        && icon_button(ui, rollback_rect, Icon::Undo, false, theme).clicked()
    {
        discard_clicked = true;
    }
    (
        checked,
        response.clicked(),
        response.double_clicked(),
        response.secondary_clicked(),
        discard_clicked,
    )
}

/// 变更清单分组：`files` 含冲突文件（组勾选覆盖），冲突行在渲染时
/// 抽到 `conflicts` 子组顶部；`key` 是折叠状态的存储键。
struct ScGroup {
    name: String,
    key: String,
    files: Vec<usize>,
    conflicts: Vec<usize>,
    unversioned: bool,
    is_ignored: bool,
}

fn source_control(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
) {
    let p = theme.palette;
    let checked: HashSet<String> = model
        .active_project()
        .map(|project| project.checked_paths.clone())
        .unwrap_or_default();

    // ----- 稳定版 CommitDrawer 头部（34px）：Commit/Shelf 页签 + ⋮ + ——
    let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 34.0));
    let tab = rt.commit_tab;
    let tab_x = |index: usize| head.left() + 10.0 + index as f32 * 62.0;
    for (index, (label, kind)) in [
        (crate::tr!(model, "Commit", "提交"), CommitTab::Commit),
        (crate::tr!(model, "Shelf", "架藏"), CommitTab::Shelf),
    ]
    .into_iter()
    .enumerate()
    {
        let active = tab == kind;
        let tab_rect = Rect::from_min_size(
            Pos2::new(tab_x(index), head.top()),
            Vec2::new(56.0, head.height()),
        );
        let response = ui.interact(
            tab_rect,
            ui.scope_id().with((
                "commit-tab",
                if matches!(kind, CommitTab::Commit) {
                    0u8
                } else {
                    1u8
                },
            )),
            Sense::click(),
        );
        ui.painter().text(
            Pos2::new(tab_rect.center().x, tab_rect.center().y - 1.0),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(13.0),
            if active { p.text_bright } else { p.text3 },
        );
        if active {
            ui.painter().rect_filled(
                Rect::from_min_max(
                    Pos2::new(tab_rect.left() + 2.0, head.bottom() - 2.0),
                    Pos2::new(tab_rect.right() - 2.0, head.bottom() - 1.0),
                ),
                egui::CornerRadius::ZERO,
                p.accent,
            );
        }
        if response.clicked() && !active {
            rt.commit_tab = kind;
            rt.shelf_file_sel.clear();
            rt.shelf_selected = None;
        }
    }
    // 右侧：⋮（扩展入口：贮藏/架藏库）+ 最小化（稳定版隐藏面板）。
    let more_rect = Rect::from_center_size(
        Pos2::new(head.right() - 40.0, head.center().y),
        Vec2::splat(22.0),
    );
    if icon_button(ui, more_rect, Icon::More, false, theme).clicked() {
        rt.open_menu(
            crate::runtime::MenuKind::CommitExtras,
            more_rect,
            None::<String>,
        );
    }
    let min_rect = Rect::from_center_size(
        Pos2::new(head.right() - 16.0, head.center().y),
        Vec2::splat(22.0),
    );
    if icon_button(ui, min_rect, Icon::Minimize, false, theme).clicked() {
        model.ui.sidebar_visible = false;
    }
    ui.painter().hline(
        head.x_range(),
        head.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );

    // ----- 工具栏（28px，22×22 图标按钮，稳定版 commit-toolbar） -----
    let toolbar = Rect::from_min_size(
        Pos2::new(rect.left(), head.bottom()),
        Vec2::new(rect.width(), 28.0),
    );
    let tool_btn = |ui: &mut Ui, index: usize, icon: Icon, enabled: bool, theme: &Theme| {
        let rect = Rect::from_center_size(
            Pos2::new(
                toolbar.left() + 12.0 + index as f32 * 24.0,
                toolbar.center().y,
            ),
            Vec2::splat(22.0),
        );
        let active = icon == Icon::ViewOptions
            && rt
                .menu
                .as_ref()
                .is_some_and(|m| m.kind == crate::runtime::MenuKind::CommitViewOptions);
        crate::widgets::icon_button(ui, rect, icon, active, theme)
            .clicked()
            .then_some(())
            .filter(|_| enabled)
            .is_some()
    };
    let mut expand_all = false;
    let mut collapse_all = false;
    if tab == CommitTab::Commit {
        let refresh = tool_btn(ui, 0, Icon::Refresh, true, theme);
        let rollback = tool_btn(ui, 1, Icon::Undo, true, theme);
        let project_now = model.active_project().unwrap();
        let has_selection = !project_now.change_selection.is_empty();
        let shelve_enabled = tool_btn(ui, 2, Icon::ShelveIn, true, theme);
        let eye = tool_btn(ui, 3, Icon::ViewOptions, true, theme);
        expand_all |= tool_btn(ui, 4, Icon::ExpandAll, true, theme);
        collapse_all |= tool_btn(ui, 5, Icon::CollapseAll, true, theme);
        if refresh {
            model.queue(Command::ReloadGit);
            model.queue(Command::ReloadTree);
        }
        if rollback && has_selection {
            let paths: Vec<String> = model
                .active_project()
                .unwrap()
                .change_selection
                .iter()
                .cloned()
                .collect();
            rt.dialog = crate::runtime::DialogState::ConfirmDiscard { paths };
        }
        if shelve_enabled {
            // Shelve：只架藏勾选文件；默认名单文件取文件名（稳定版规则）。
            let checked_paths: Vec<String> = model
                .active_project()
                .unwrap()
                .changes()
                .iter()
                .filter(|change| checked.contains(&change.path))
                .map(|change| change.path.clone())
                .collect();
            if !checked_paths.is_empty() {
                let default_name = if checked_paths.len() == 1 {
                    checked_paths[0]
                        .rsplit('/')
                        .next()
                        .unwrap_or("Default")
                        .to_string()
                } else {
                    "Default".to_string()
                };
                rt.dialog = crate::runtime::DialogState::NewShelf {
                    value: default_name,
                    error: None,
                    paths: checked_paths,
                };
            }
        }
        if eye {
            let eye_rect = Rect::from_center_size(
                Pos2::new(toolbar.left() + 12.0 + 3.0 * 24.0, toolbar.center().y),
                Vec2::splat(22.0),
            );
            rt.toggle_menu(crate::runtime::MenuKind::CommitViewOptions, eye_rect, None);
        }
    } else {
        // Shelf 工具栏：静默恢复 / 视图选项。
        let unshelve_silent = tool_btn(ui, 0, Icon::Undo, true, theme);
        let eye = tool_btn(ui, 1, Icon::ViewOptions, true, theme);
        expand_all |= tool_btn(ui, 2, Icon::ExpandAll, true, theme);
        collapse_all |= tool_btn(ui, 3, Icon::CollapseAll, true, theme);
        if unshelve_silent {
            // ⌥⌘U：恢复到首个变更列表，不弹对话框（稳定版 Unshelve Silently）。
            if let Some(name) = rt.shelf_selected.clone() {
                let default_list = {
                    let project = model.active_project().unwrap();
                    lazecode_git::Changelists::open(&project.root)
                        .lists()
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "Default".to_string())
                };
                let paths = if rt.shelf_file_sel.is_empty() {
                    None
                } else {
                    Some(rt.shelf_file_sel.clone())
                };
                model.queue(Command::ShelfRestore {
                    name,
                    paths,
                    target_list: Some(default_list),
                    delete_after: false,
                });
            }
        }
        if eye {
            let eye_rect = Rect::from_center_size(
                Pos2::new(toolbar.left() + 12.0 + 1.0 * 24.0, toolbar.center().y),
                Vec2::splat(22.0),
            );
            rt.toggle_menu(crate::runtime::MenuKind::CommitViewOptions, eye_rect, None);
        }
    }
    ui.painter().hline(
        toolbar.x_range(),
        toolbar.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );

    let project = model.active_project().unwrap();
    let mut changes = project.changes().to_vec();
    let mut ignored_indices: Vec<usize> = Vec::new();
    if rt.commit_show_ignored {
        for path in project.ignored_files() {
            let idx = changes.len();
            changes.push(lazecode_git::Change {
                path: path.clone(),
                staged: None,
                unstaged: None,
                untracked: false,
                conflicted: false,
                additions: 0,
                deletions: 0,
                binary: false,
            });
            ignored_indices.push(idx);
        }
    }
    let last = project.git.as_ref().and_then(|git| git.head.clone());
    let can_git = project.git.is_some();
    let git_error = project.git_error.clone();
    let commit_message = project.commit_message.clone();
    let amend = project.amend;
    let checked: HashSet<String> = project.checked_paths.clone();
    let conflicts: HashSet<String> = project
        .conflicts
        .iter()
        .map(|entry| entry.path.clone())
        .collect();
    let selection: HashSet<String> = project.change_selection.clone();

    let dock_height = if changes.is_empty() { 0.0 } else { 168.0 };
    let dock = Rect::from_min_max(
        Pos2::new(rect.left(), rect.bottom() - dock_height),
        rect.max,
    );
    let list = Rect::from_min_max(
        Pos2::new(rect.left(), head.bottom()),
        Pos2::new(rect.right(), dock.top()),
    );

    if let Some(err) = &git_error {
        text_clipped(
            ui.painter(),
            Rect::from_min_size(
                Pos2::new(list.left() + 12.0, list.top() + 10.0),
                Vec2::new(list.width() - 24.0, 20.0),
            ),
            err,
            FontId::proportional(10.5),
            p.rose,
            Align2::LEFT_CENTER,
        );
    }

    if !can_git {
        // 非 Git 项目：对齐稳定版 notRepoNotice，并提供初始化入口
        // （原 dock 内的初始化按钮放在空态之外，实际不可达）。
        let center = Pos2::new(rect.center().x, rect.top() + 58.0);
        ui.painter().text(
            Pos2::new(center.x, center.y + 30.0),
            Align2::CENTER_CENTER,
            crate::tr!(
                model,
                "The current directory is not a Git repository, so changes cannot be committed",
                "当前目录不是 Git 仓库，无法提交"
            ),
            FontId::proportional(12.0),
            p.text3,
        );
        let init =
            Rect::from_center_size(Pos2::new(center.x, center.y + 62.0), Vec2::new(170.0, 26.0));
        if crate::widgets::secondary_button(
            ui,
            init,
            crate::tr!(model, "Initialize Git Repository", "初始化 Git 仓库"),
            Some(Icon::Git),
            theme,
        )
        .clicked()
        {
            model.queue(Command::GitInit);
        }
        return;
    }

    if tab == CommitTab::Shelf {
        let body = Rect::from_min_max(
            Pos2::new(rect.left(), toolbar.bottom()),
            Pos2::new(rect.right(), rect.bottom()),
        );
        crate::sidebar::shelf_view(ui, model, rt, theme, body);
        return;
    }

    if changes.is_empty() {
        if project.git_loading {
            // 稳定版加载中空态（Reading changes…），别把「工作区干净」
            // 在快照回来前先画出来。
            let zh = model.settings.language.is_zh();
            ui.painter().text(
                Pos2::new(list.center().x, list.top() + 60.0),
                Align2::CENTER_CENTER,
                if zh {
                    "正在读取更改…"
                } else {
                    "Reading changes…"
                },
                FontId::proportional(12.0),
                theme.palette.text3,
            );
            return;
        }
        draw_clean_state(ui, theme, list, last.as_ref(), model.settings.language);
        return;
    }

    // ----- 分组构建：Default（含冲突子组）→ 自定义列表 → Unversioned -----
    // 每帧从 JSON 重载归属（读取廉价；写入路径已统一走后台任务）。
    let lists = lazecode_git::Changelists::open(&model.active_project().unwrap().root);
    let default_name = lists
        .lists()
        .first()
        .cloned()
        .unwrap_or_else(|| "Default".to_string());
    let mut groups: Vec<ScGroup> = Vec::new();
    for name in lists.lists() {
        if !name.is_empty() && !groups.iter().any(|g| g.name == *name) {
            groups.push(ScGroup {
                name: name.clone(),
                key: name.clone(),
                files: Vec::new(),
                conflicts: Vec::new(),
                unversioned: false,
                is_ignored: false,
            });
        }
    }
    if groups.is_empty() {
        groups.push(ScGroup {
            name: default_name.clone(),
            key: default_name.clone(),
            files: Vec::new(),
            conflicts: Vec::new(),
            unversioned: false,
            is_ignored: false,
        });
    }
    let mut unversioned: Vec<usize> = Vec::new();
    for (index, change) in changes.iter().enumerate() {
        // Ignored paths live only in the Ignored Files group; never let the
        // synthetic entries fall through into a changelist/Changes group.
        if ignored_indices.contains(&index) {
            continue;
        }
        if change.untracked {
            // 未跟踪文件不参与 changelist 分组，固定进 Unversioned Files
            // （对齐稳定版语义）。
            unversioned.push(index);
            continue;
        }
        let assigned = lists.list_of(&change.path);
        if let Some(group) = groups
            .iter_mut()
            .find(|group| group.name == assigned && !group.unversioned)
        {
            group.files.push(index);
        } else if let Some(group) = groups.first_mut() {
            group.files.push(index);
        }
    }
    // 冲突文件从所属组抽出，挂到 Default 组的 Merge Conflicts 子组顶部
    // （稳定版只把冲突子组挂在默认组下）。
    for group in groups.iter_mut() {
        let conflicted: Vec<usize> = group
            .files
            .iter()
            .copied()
            .filter(|index| conflicts.contains(&changes[*index].path))
            .collect();
        if !conflicted.is_empty() {
            group.files.retain(|index| !conflicted.contains(index));
            group.conflicts = conflicted;
        }
    }
    // 保留全部组（包括空组，对齐稳定版与 PyCharm 语义：新创建的空列表必须可见）
    if !unversioned.is_empty() {
        groups.push(ScGroup {
            name: crate::tr!(model, "Unversioned Files", "未版本控制的文件").to_string(),
            key: "__unversioned__".to_string(),
            files: unversioned,
            conflicts: Vec::new(),
            unversioned: true,
            is_ignored: false,
        });
    }
    if rt.commit_show_ignored && !ignored_indices.is_empty() {
        groups.push(ScGroup {
            name: crate::tr!(model, "Ignored Files", "被忽略的文件").to_string(),
            key: "__ignored__".to_string(),
            files: ignored_indices,
            conflicts: Vec::new(),
            unversioned: false,
            is_ignored: true,
        });
    }
    // ----- 折叠/展开全部（在构造 rows 之前处理，保证同帧即时反映） -----
    #[derive(Clone, Debug)]
    struct DirNode {
        seg: String,
        path: String,
        sub: Vec<DirNode>,
        files: Vec<usize>,
        total_file_count: usize,
    }

    fn build_project_dir_tree(
        file_indices: &[usize],
        changes: &[lazecode_git::Change],
    ) -> Vec<DirNode> {
        if file_indices.is_empty() {
            return Vec::new();
        }
        let mut root = DirNode {
            seg: String::new(),
            path: String::new(),
            sub: Vec::new(),
            files: Vec::new(),
            total_file_count: 0,
        };
        for &file_idx in file_indices {
            let path = &changes[file_idx].path;
            let mut segs: Vec<&str> = path.split('/').collect();
            segs.pop();
            if segs.is_empty() {
                root.files.push(file_idx);
                continue;
            }
            let mut cur = &mut root;
            let mut acc = String::new();
            for s in segs {
                if !acc.is_empty() {
                    acc.push('/');
                }
                acc.push_str(s);
                let pos = if let Some(i) = cur.sub.iter().position(|child| child.seg == s) {
                    i
                } else {
                    cur.sub.push(DirNode {
                        seg: s.to_string(),
                        path: acc.clone(),
                        sub: Vec::new(),
                        files: Vec::new(),
                        total_file_count: 0,
                    });
                    cur.sub.len() - 1
                };
                cur = &mut cur.sub[pos];
            }
            cur.files.push(file_idx);
        }

        fn calc_and_sort(node: &mut DirNode, changes: &[lazecode_git::Change]) -> usize {
            let mut count = node.files.len();
            for s in &mut node.sub {
                count += calc_and_sort(s, changes);
            }
            node.total_file_count = count;
            node.sub.sort_by(|a, b| a.seg.cmp(&b.seg));
            node.files
                .sort_by(|a, b| changes[*a].path.cmp(&changes[*b].path));
            count
        }
        calc_and_sort(&mut root, changes);

        fn compact_dir_nodes(mut nodes: Vec<DirNode>) -> Vec<DirNode> {
            for node in &mut nodes {
                let compacted = compact_dir_nodes(std::mem::take(&mut node.sub));
                node.sub = compacted;
                while node.files.is_empty() && node.sub.len() == 1 {
                    let only = node.sub.remove(0);
                    node.seg = format!("{}/{}", node.seg, only.seg);
                    node.path = only.path;
                    node.sub = only.sub;
                    node.files = only.files;
                    node.total_file_count = only.total_file_count;
                }
            }
            nodes
        }

        compact_dir_nodes(root.sub)
    }

    fn collect_all_dir_keys(nodes: &[DirNode], group_key: &str, acc: &mut HashSet<String>) {
        for n in nodes {
            acc.insert(format!("{}::{}", group_key, n.path));
            collect_all_dir_keys(&n.sub, group_key, acc);
        }
    }

    if expand_all {
        if let Some(project) = model.active_project_mut() {
            project.collapsed_lists.clear();
        }
    }
    if collapse_all {
        if let Some(project) = model.active_project_mut() {
            let mut keys = all_list_keys(project);
            for g in &groups {
                keys.insert(g.key.clone());
                let dir_nodes = build_project_dir_tree(&g.files, &changes);
                collect_all_dir_keys(&dir_nodes, &g.key, &mut keys);
            }
            keys.insert("__unversioned__".to_string());
            keys.insert("__ignored__".to_string());
            project.collapsed_lists = keys;
        }
    }
    let collapsed_lists: HashSet<String> = model.active_project().unwrap().collapsed_lists.clone();

    // ----- 扁平行模型（虚拟化 + Shift 范围选择基准顺序） -----
    #[derive(Clone, Copy)]
    enum RowKind {
        Header,
        ConflictHeader,
        File,
    }
    #[derive(Clone)]
    struct ScRow {
        group: usize,
        kind: RowKind,
        change: Option<usize>,
        depth: usize,
        dir_key: Option<String>,
        dir_name: Option<String>,
        dir_file_paths: Option<Vec<String>>,
        dir_count: usize,
    }
    let tree_view = rt.commit_tree_view;
    let show_dir_hint = !tree_view;
    let mut rows: Vec<ScRow> = Vec::new();
    let mut flat_order: Vec<usize> = Vec::new();
    for (group_index, group) in groups.iter().enumerate() {
        rows.push(ScRow {
            group: group_index,
            kind: RowKind::Header,
            change: None,
            depth: 0,
            dir_key: None,
            dir_name: None,
            dir_file_paths: None,
            dir_count: 0,
        });
        let collapsed_group = collapsed_lists.contains(&group.key);
        if collapsed_group {
            continue;
        }
        if !group.conflicts.is_empty() {
            let conflict_key = format!("__conflicts__::{}", group.key);
            rows.push(ScRow {
                group: group_index,
                kind: RowKind::ConflictHeader,
                change: None,
                depth: 1,
                dir_key: None,
                dir_name: None,
                dir_file_paths: None,
                dir_count: 0,
            });
            if !collapsed_lists.contains(&conflict_key) {
                for index in &group.conflicts {
                    rows.push(ScRow {
                        group: group_index,
                        kind: RowKind::File,
                        change: Some(*index),
                        depth: 2,
                        dir_key: None,
                        dir_name: None,
                        dir_file_paths: None,
                        dir_count: 0,
                    });
                    flat_order.push(*index);
                }
            }
        }
        if tree_view {
            // 稳定版 Group by Directory（先展示无目录的根文件，再展示递归压缩目录树）
            for &file_idx in &group.files {
                if !changes[file_idx].path.contains('/') {
                    rows.push(ScRow {
                        group: group_index,
                        kind: RowKind::File,
                        change: Some(file_idx),
                        depth: 1,
                        dir_key: None,
                        dir_name: None,
                        dir_file_paths: None,
                        dir_count: 0,
                    });
                    flat_order.push(file_idx);
                }
            }
            let dir_nodes = build_project_dir_tree(&group.files, &changes);
            #[allow(clippy::too_many_arguments)]
            fn flatten_dir_node(
                node: &DirNode,
                group_index: usize,
                group_key: &str,
                depth: usize,
                collapsed_lists: &HashSet<String>,
                changes: &[lazecode_git::Change],
                rows: &mut Vec<ScRow>,
                flat_order: &mut Vec<usize>,
            ) {
                let dir_key = format!("{}::{}", group_key, node.path);
                let mut all_paths = Vec::new();
                fn collect_paths(
                    n: &DirNode,
                    changes: &[lazecode_git::Change],
                    acc: &mut Vec<String>,
                ) {
                    for f in &n.files {
                        acc.push(changes[*f].path.clone());
                    }
                    for s in &n.sub {
                        collect_paths(s, changes, acc);
                    }
                }
                collect_paths(node, changes, &mut all_paths);

                rows.push(ScRow {
                    group: group_index,
                    kind: RowKind::File,
                    change: None,
                    depth,
                    dir_key: Some(dir_key.clone()),
                    dir_name: Some(node.seg.clone()),
                    dir_file_paths: Some(all_paths),
                    dir_count: node.total_file_count,
                });

                if !collapsed_lists.contains(&dir_key) {
                    for sub in &node.sub {
                        flatten_dir_node(
                            sub,
                            group_index,
                            group_key,
                            depth + 1,
                            collapsed_lists,
                            changes,
                            rows,
                            flat_order,
                        );
                    }
                    for &file_idx in &node.files {
                        rows.push(ScRow {
                            group: group_index,
                            kind: RowKind::File,
                            change: Some(file_idx),
                            depth: depth + 1,
                            dir_key: None,
                            dir_name: None,
                            dir_file_paths: None,
                            dir_count: 0,
                        });
                        flat_order.push(file_idx);
                    }
                }
            }
            for d in &dir_nodes {
                flatten_dir_node(
                    d,
                    group_index,
                    &group.key,
                    1,
                    &collapsed_lists,
                    &changes,
                    &mut rows,
                    &mut flat_order,
                );
            }
        } else {
            for index in &group.files {
                rows.push(ScRow {
                    group: group_index,
                    kind: RowKind::File,
                    change: Some(*index),
                    depth: 1,
                    dir_key: None,
                    dir_name: None,
                    dir_file_paths: None,
                    dir_count: 0,
                });
                flat_order.push(*index);
            }
        }
    }
    let header_bottom = toolbar.bottom() + 4.0;
    let rows_rect = Rect::from_min_max(Pos2::new(list.left(), header_bottom), list.max);
    let mut toggled: Option<(String, bool)> = None;
    let mut group_toggle: Option<(Vec<String>, bool)> = None;
    let mut open_diff: Option<String> = None;
    let mut menu_open: Option<(crate::runtime::MenuKind, Rect, String)> = None;
    let mut confirm_discard: Option<Vec<String>> = None;
    let modifiers = ui.input(|input| input.modifiers);

    super::app_ui::region(ui, rows_rect, "sc-rows", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("changes")
            .auto_shrink([false, false])
            .show_rows(ui, 21.0, rows.len(), |ui, range| {
                for row_index in range {
                    let Some(row) = rows.get(row_index) else {
                        continue;
                    };
                    let group = &groups[row.group];
                    match row.kind {
                        RowKind::Header => {
                            let (rect, _) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 21.0),
                                Sense::hover(),
                            );
                            let draw_rect = Rect::from_min_max(
                                Pos2::new(rect.left() + 4.0, rect.top()),
                                Pos2::new(rect.right() - 4.0, rect.bottom()),
                            );
                            let is_group_selected = selection.contains(&group.name);
                            let response = ui.interact(
                                rect,
                                ui.scope_id().with(("sc_header_row", row_index)),
                                Sense::click(),
                            );
                            if is_group_selected {
                                ui.painter().rect_filled(
                                    draw_rect,
                                    egui::CornerRadius::same(3),
                                    p.selected_row,
                                );
                            } else if response.hovered() {
                                ui.painter().rect_filled(
                                    draw_rect,
                                    egui::CornerRadius::same(3),
                                    p.hover,
                                );
                            }
                            let group_paths: Vec<String> = group
                                .files
                                .iter()
                                .chain(group.conflicts.iter())
                                .map(|index| changes[*index].path.clone())
                                .collect();
                            if response.secondary_clicked() {
                                menu_open = Some((
                                    crate::runtime::MenuKind::ChangelistHeaderContext {
                                        project: model.active_project.unwrap_or(0),
                                    },
                                    rect,
                                    std::iter::once(group.name.clone())
                                        .chain(group_paths.iter().cloned())
                                        .collect::<Vec<_>>()
                                        .join("\u{1}"),
                                ));
                            }
                            // 组头三态勾选：有未勾项→全勾，否则全不勾。
                            let mut paths: Vec<String> = group_paths;
                            let state = group_check_state(&checked, &paths);
                            let checkbox_rect = Rect::from_center_size(
                                Pos2::new(rect.left() + 26.0, rect.center().y),
                                Vec2::splat(16.0),
                            );
                            let chk_resp = checkbox_tri(ui, checkbox_rect, state, theme);
                            if chk_resp.clicked() {
                                paths.sort();
                                group_toggle = Some((paths, state != TriState::Checked));
                            }
                            let expanded = !collapsed_lists.contains(&group.key);
                            let chevron = if expanded {
                                Icon::ChevronDown
                            } else {
                                Icon::ChevronRight
                            };
                            let chevron_rect = Rect::from_center_size(
                                Pos2::new(rect.left() + 10.0, rect.center().y),
                                Vec2::splat(14.0),
                            );
                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(rect.left() + 10.0, rect.center().y),
                                    Vec2::splat(12.0),
                                ),
                                chevron,
                                p.text3,
                            );
                            let chev_resp = ui.interact(
                                chevron_rect,
                                ui.scope_id().with(("sc_chev", row_index)),
                                Sense::click(),
                            );
                            if chev_resp.clicked() {
                                let project = model.active_project_mut().unwrap();
                                if !project.collapsed_lists.remove(&group.key) {
                                    project.collapsed_lists.insert(group.key.clone());
                                }
                            }
                            let name_color = if group.unversioned { p.orange } else { p.text2 };
                            ui.painter().text(
                                Pos2::new(rect.left() + 40.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                &group.name,
                                FontId::proportional(12.5),
                                name_color,
                            );
                            let total = group.files.len() + group.conflicts.len();
                            ui.painter().text(
                                Pos2::new(rect.right() - 12.0, rect.center().y),
                                Align2::RIGHT_CENTER,
                                total.to_string(),
                                FontId::monospace(11.0),
                                p.text3,
                            );
                            if response.clicked() && !chk_resp.clicked() && !chev_resp.clicked() {
                                if response.double_clicked() {
                                    let project = model.active_project_mut().unwrap();
                                    if !project.collapsed_lists.remove(&group.key) {
                                        project.collapsed_lists.insert(group.key.clone());
                                    }
                                } else {
                                    let project = model.active_project_mut().unwrap();
                                    project.change_selection.clear();
                                    project.change_selection.insert(group.name.clone());
                                }
                            }
                        }
                        RowKind::ConflictHeader => {
                            let conflict_key = format!("__conflicts__::{}", group.key);
                            let (rect, _) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 21.0),
                                Sense::hover(),
                            );
                            let response = ui.interact(
                                rect,
                                ui.scope_id().with(("sc_conf_hdr", row_index)),
                                Sense::click(),
                            );
                            let mut paths: Vec<String> = group
                                .conflicts
                                .iter()
                                .map(|index| changes[*index].path.clone())
                                .collect();
                            let state = group_check_state(&checked, &paths);
                            let checkbox_rect = Rect::from_center_size(
                                Pos2::new(rect.left() + 40.0, rect.center().y),
                                Vec2::splat(16.0),
                            );
                            let chk_resp = checkbox_tri(ui, checkbox_rect, state, theme);
                            if chk_resp.clicked() {
                                paths.sort();
                                group_toggle = Some((paths, state != TriState::Checked));
                            }
                            let expanded = !collapsed_lists.contains(&conflict_key);
                            let chevron = if expanded {
                                Icon::ChevronDown
                            } else {
                                Icon::ChevronRight
                            };
                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(rect.left() + 24.0, rect.center().y),
                                    Vec2::splat(12.0),
                                ),
                                chevron,
                                p.text3,
                            );
                            ui.painter().text(
                                Pos2::new(rect.left() + 54.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                format!(
                                    "{} {}",
                                    crate::tr!(model, "Merge Conflicts", "合并冲突"),
                                    group.conflicts.len()
                                ),
                                FontId::proportional(12.5),
                                p.rose,
                            );
                            // Resolve 链接：打开首个冲突的三方解决对话框。
                            let resolve_right = rect.right() - 12.0;
                            let resolve_rect = Rect::from_min_size(
                                Pos2::new(resolve_right - 60.0, rect.center().y - 9.0),
                                Vec2::new(60.0, 18.0),
                            );
                            if ui
                                .interact(
                                    resolve_rect,
                                    ui.scope_id().with(("resolve", group.key.clone())),
                                    Sense::click(),
                                )
                                .clicked()
                            {
                                if let Some(first) = group.conflicts.first() {
                                    let path = changes[*first].path.clone();
                                    let root = model.active_project().unwrap().root.clone();
                                    if let Ok(content) =
                                        lazecode_git::merge_file_content(&root, &path)
                                    {
                                        rt.dialog = crate::runtime::DialogState::ConflictResolve {
                                            path,
                                            ours: content.ours,
                                            theirs: content.theirs,
                                            worktree: content.worktree,
                                        };
                                    }
                                }
                            }
                            ui.painter().text(
                                Pos2::new(resolve_rect.center().x, resolve_rect.center().y),
                                Align2::CENTER_CENTER,
                                crate::tr!(model, "Resolve…", "解决…"),
                                FontId::proportional(11.5),
                                p.accent,
                            );
                            if response.clicked() && !chk_resp.clicked() {
                                let project = model.active_project_mut().unwrap();
                                if !project.collapsed_lists.remove(&conflict_key) {
                                    project.collapsed_lists.insert(conflict_key);
                                }
                            }
                        }
                        RowKind::File => {
                            let Some(change_index) = row.change else {
                                // 树视图目录行（稳定版 commit-dir-row）。
                                let (rect, response) = ui.allocate_exact_size(
                                    Vec2::new(ui.available_width(), 21.0),
                                    Sense::click(),
                                );
                                if response.hovered() {
                                    ui.painter().rect_filled(
                                        rect.shrink2(Vec2::new(4.0, 0.0)),
                                        egui::CornerRadius::same(4),
                                        p.hover,
                                    );
                                }
                                if let Some(key) = &row.dir_key {
                                    let indent = rect.left() + 6.0 + row.depth as f32 * 14.0;
                                    let expanded = !collapsed_lists.contains(key);
                                    let chevron = if expanded {
                                        Icon::ChevronDown
                                    } else {
                                        Icon::ChevronRight
                                    };
                                    let chevron_rect = Rect::from_center_size(
                                        Pos2::new(indent + 4.0, rect.center().y),
                                        Vec2::splat(14.0),
                                    );
                                    paint_icon(
                                        ui.painter(),
                                        Rect::from_center_size(
                                            Pos2::new(indent + 4.0, rect.center().y),
                                            Vec2::splat(11.0),
                                        ),
                                        chevron,
                                        p.text3,
                                    );
                                    let chev_resp = ui.interact(
                                        chevron_rect,
                                        ui.scope_id().with(("dir_chev", row_index)),
                                        Sense::click(),
                                    );
                                    if chev_resp.clicked() {
                                        let project = model.active_project_mut().unwrap();
                                        if !project.collapsed_lists.remove(key) {
                                            project.collapsed_lists.insert(key.clone());
                                        }
                                    }

                                    let mut chk_clicked = false;
                                    let chk_offset = if !group.is_ignored {
                                        if let Some(paths) = &row.dir_file_paths {
                                            let state = group_check_state(&checked, paths);
                                            let chk_rect = Rect::from_center_size(
                                                Pos2::new(indent + 18.0, rect.center().y),
                                                Vec2::splat(14.0),
                                            );
                                            let chk_resp = checkbox_tri(ui, chk_rect, state, theme);
                                            if chk_resp.clicked() {
                                                chk_clicked = true;
                                                let mut sorted_paths = paths.clone();
                                                sorted_paths.sort();
                                                group_toggle = Some((
                                                    sorted_paths,
                                                    state != TriState::Checked,
                                                ));
                                            }
                                        }
                                        16.0
                                    } else {
                                        0.0
                                    };

                                    let icon_x = indent + 18.0 + chk_offset;
                                    paint_icon(
                                        ui.painter(),
                                        Rect::from_center_size(
                                            Pos2::new(icon_x, rect.center().y),
                                            Vec2::splat(13.0),
                                        ),
                                        if expanded {
                                            Icon::FolderOpen
                                        } else {
                                            Icon::Folder
                                        },
                                        p.text3,
                                    );

                                    let dir_name = row.dir_name.as_deref().unwrap_or("");
                                    let text_x = icon_x + 12.0;
                                    ui.painter().text(
                                        Pos2::new(text_x, rect.center().y),
                                        Align2::LEFT_CENTER,
                                        dir_name,
                                        FontId::proportional(12.0),
                                        p.text,
                                    );

                                    let count = row.dir_count;
                                    ui.painter().text(
                                        Pos2::new(rect.right() - 12.0, rect.center().y),
                                        Align2::RIGHT_CENTER,
                                        if model.settings.language.is_zh() {
                                            format!("{count} 个文件")
                                        } else if count == 1 {
                                            "1 file".to_string()
                                        } else {
                                            format!("{count} files")
                                        },
                                        FontId::proportional(11.0),
                                        p.text3,
                                    );

                                    if response.clicked() && !chk_clicked && !chev_resp.clicked() {
                                        let project = model.active_project_mut().unwrap();
                                        if !project.collapsed_lists.remove(key) {
                                            project.collapsed_lists.insert(key.clone());
                                        }
                                    }
                                }
                                continue;
                            };

                            let change = &changes[change_index];
                            let (rect, _) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 21.0),
                                Sense::hover(),
                            );
                            let rect = Rect::from_min_max(
                                Pos2::new(rect.left() + 4.0, rect.top()),
                                Pos2::new(rect.right() - 4.0, rect.bottom()),
                            );
                            let path = change.path.clone();
                            let is_checked = checked.contains(&path);
                            let is_selected = selection.contains(&path);
                            let prev_selected = if row_index > 0 {
                                rows.get(row_index - 1)
                                    .and_then(|r| r.change)
                                    .is_some_and(|idx| selection.contains(&changes[idx].path))
                            } else {
                                false
                            };
                            let next_selected = if row_index + 1 < rows.len() {
                                rows.get(row_index + 1)
                                    .and_then(|r| r.change)
                                    .is_some_and(|idx| selection.contains(&changes[idx].path))
                            } else {
                                false
                            };
                            let radius = match (prev_selected, next_selected) {
                                (true, true) => egui::CornerRadius::ZERO,
                                (false, true) => egui::CornerRadius {
                                    nw: 3,
                                    ne: 3,
                                    sw: 0,
                                    se: 0,
                                },
                                (true, false) => egui::CornerRadius {
                                    nw: 0,
                                    ne: 0,
                                    sw: 3,
                                    se: 3,
                                },
                                (false, false) => egui::CornerRadius::same(3),
                            };
                            let is_ignored = groups.get(row.group).is_some_and(|g| g.is_ignored);
                            let (new_checked, clicked, double_clicked, secondary, discard) =
                                change_row(
                                    ui,
                                    theme,
                                    rect,
                                    change,
                                    is_checked,
                                    is_selected,
                                    radius,
                                    row.depth,
                                    show_dir_hint,
                                    is_ignored,
                                );
                            if secondary {
                                // 右键行不在多选内→只选中它（稳定版规则）；
                                // 在多选内→对整个选择生效。
                                let mut payload_paths: Vec<String> = Vec::new();
                                if is_selected && selection.len() > 1 {
                                    for index in &flat_order {
                                        let candidate = &changes[*index].path;
                                        if selection.contains(candidate) {
                                            payload_paths.push(candidate.clone());
                                        }
                                    }
                                } else {
                                    payload_paths.push(path.clone());
                                    let project = model.active_project_mut().unwrap();
                                    project.change_selection.clear();
                                    project.change_selection.insert(path.clone());
                                    project.change_anchor = Some(path.clone());
                                }
                                menu_open = Some((
                                    crate::runtime::MenuKind::ChangeFileContext {
                                        project: model.active_project.unwrap_or(0),
                                    },
                                    rect,
                                    payload_paths.join("\u{1}"),
                                ));
                            }
                            if discard {
                                confirm_discard = Some(vec![path.clone()]);
                            }
                            if new_checked != is_checked {
                                toggled = Some((path.clone(), new_checked));
                            }
                            if double_clicked {
                                open_diff = Some(path.clone());
                            } else if clicked {
                                // 单击只选中行（对齐稳定版）：Cmd/Ctrl 增减，
                                // Shift 自锚点范围选择，普通点击重置为单选。
                                let project = model.active_project_mut().unwrap();
                                if modifiers.shift {
                                    if let Some(anchor) = project.change_anchor.clone() {
                                        // lo/hi 是扁平行序中的位置，不是 change 索引
                                        let start = flat_order
                                            .iter()
                                            .position(|index| changes[*index].path == anchor);
                                        let end = flat_order
                                            .iter()
                                            .position(|index| changes[*index].path == path);
                                        if let (Some(start), Some(end)) = (start, end) {
                                            let (lo, hi) = if start <= end {
                                                (start, end)
                                            } else {
                                                (end, start)
                                            };
                                            project.change_selection.clear();
                                            for pos in lo..=hi {
                                                project
                                                    .change_selection
                                                    .insert(changes[flat_order[pos]].path.clone());
                                            }
                                        } else {
                                            project.change_selection.clear();
                                            project.change_selection.insert(path.clone());
                                            project.change_anchor = Some(path.clone());
                                        }
                                    } else {
                                        project.change_selection.insert(path.clone());
                                        project.change_anchor = Some(path.clone());
                                    }
                                } else if modifiers.command || modifiers.ctrl {
                                    if !project.change_selection.remove(&path) {
                                        project.change_selection.insert(path.clone());
                                    }
                                    project.change_anchor = Some(path.clone());
                                } else {
                                    project.change_selection.clear();
                                    project.change_selection.insert(path.clone());
                                    project.change_anchor = Some(path.clone());
                                }
                            }
                        }
                    }
                }
            });
    });

    if let Some((paths, target)) = group_toggle {
        let project = model.active_project_mut().unwrap();
        for path in paths {
            if target {
                project.checked_paths.insert(path);
            } else {
                project.checked_paths.remove(&path);
            }
        }
    }
    if let Some((path, is_checked)) = toggled {
        let project = model.active_project_mut().unwrap();
        if is_checked {
            project.checked_paths.insert(path);
        } else {
            project.checked_paths.remove(&path);
        }
    }
    if let Some(paths) = confirm_discard {
        rt.dialog = crate::runtime::DialogState::ConfirmDiscard { paths };
    }
    if let Some(path) = open_diff {
        model.queue(Command::OpenDiff(path));
    }
    if let Some((kind, anchor_rect, payload)) = menu_open {
        rt.open_menu(kind, anchor_rect, Some(payload));
    }

    draw_commit_dock(
        ui,
        model,
        rt,
        theme,
        dock,
        &changes,
        &commit_message,
        amend,
        checked.len(),
        can_git,
    );
}

/// 组勾选三态：全勾 / 全不勾 / 半选。
fn group_check_state(checked: &HashSet<String>, paths: &[String]) -> TriState {
    let total = paths.len();
    let checked_count = paths.iter().filter(|path| checked.contains(*path)).count();
    if checked_count == 0 {
        TriState::Unchecked
    } else if checked_count == total {
        TriState::Checked
    } else {
        TriState::Partial
    }
}

/// Collapse All 时需要折叠的全部键：分组键 + 各组冲突子组键。
fn all_list_keys(project: &ProjectState) -> HashSet<String> {
    // 借助 changelist 存储推断分组键集合；Unversioned/冲突子组键固定。
    let mut keys: HashSet<String> = HashSet::new();
    let lists = lazecode_git::Changelists::open(&project.root);
    for name in lists.lists() {
        if !name.is_empty() {
            keys.insert(name.clone());
            keys.insert(format!("__conflicts__::{name}"));
        }
    }
    keys.insert("__unversioned__".to_string());
    keys.insert("__ignored__".to_string());
    keys.insert(format!(
        "__conflicts__::{}",
        lists
            .lists()
            .first()
            .cloned()
            .unwrap_or_else(|| "Default".to_string())
    ));
    keys
}

fn draw_clean_state(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    last: Option<&lazecode_git::HeadInfo>,
    lang: lazecode_core::Language,
) {
    let p = theme.palette;
    let center = Pos2::new(rect.center().x, rect.top() + 58.0);
    let circle = Rect::from_center_size(center, Vec2::splat(34.0));
    ui.painter().circle_filled(circle.center(), 17.0, p.panel2);
    paint_icon(ui.painter(), circle.shrink(9.0), Icon::Check, p.green);
    ui.painter().text(
        Pos2::new(center.x, center.y + 34.0),
        Align2::CENTER_CENTER,
        crate::i18n::t(lang, "Working tree clean", "工作区干净"),
        FontId::proportional(13.5),
        p.text2,
    );
    ui.painter().text(
        Pos2::new(center.x, center.y + 50.0),
        Align2::CENTER_CENTER,
        crate::i18n::t(lang, "Nothing to commit", "没有可提交内容"),
        FontId::proportional(12.0),
        p.text3,
    );
    if let Some(last) = last {
        let card = Rect::from_min_size(
            Pos2::new(rect.left() + 12.0, rect.top() + 128.0),
            Vec2::new(rect.width() - 24.0, 62.0),
        );
        ui.painter()
            .rect_filled(card, egui::CornerRadius::same(6), p.panel2);
        paint_icon(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(card.left() + 17.0, card.top() + 20.0),
                Vec2::splat(14.0),
            ),
            Icon::Commit,
            p.text3,
        );
        ui.painter().text(
            Pos2::new(card.left() + 30.0, card.top() + 20.0),
            Align2::LEFT_CENTER,
            &last.short,
            FontId::monospace(11.0),
            p.accent,
        );
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(card.left() + 10.0, card.top() + 26.0),
                Pos2::new(card.right() - 10.0, card.top() + 44.0),
            ),
            &last.message,
            FontId::proportional(12.5),
            p.text2,
            Align2::LEFT_CENTER,
        );
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(card.left() + 10.0, card.top() + 44.0),
                Pos2::new(card.right() - 10.0, card.bottom() - 4.0),
            ),
            &format!("{} · {}", last.author, last.short),
            FontId::proportional(11.5),
            p.text3,
            Align2::LEFT_CENTER,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_commit_dock(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    changes: &[lazecode_git::Change],
    message: &str,
    _amend: bool,
    checked: usize,
    can_commit: bool,
) {
    let p = theme.palette;
    let _ = changes;
    // Commit message box: internal separator, not a pane divider.
    ui.painter().hline(
        rect.x_range(),
        rect.top() + 0.5,
        Stroke::new(1.0, p.hairline),
    );
    // 稳定版 commit-bottom-box：padding 10 / gap 8。
    // 1) 选项行：提交信息历史按钮（右对齐，无历史时禁用态）。
    let options_row = Rect::from_min_size(
        Pos2::new(rect.left() + 10.0, rect.top() + 8.0),
        Vec2::new(rect.width() - 20.0, 20.0),
    );
    let history_empty = model.settings.commit_history.is_empty();
    let history_rect = Rect::from_center_size(
        Pos2::new(options_row.right() - 10.0, options_row.center().y),
        Vec2::splat(20.0),
    );
    let history_response = ui.interact(
        history_rect,
        ui.scope_id().with("dock-history"),
        Sense::click(),
    );
    if history_response.hovered() && !history_empty {
        ui.painter()
            .rect_filled(history_rect, egui::CornerRadius::same(4), p.hover);
    }
    paint_icon(
        ui.painter(),
        history_rect.shrink(3.0),
        Icon::History,
        if history_empty { p.text3 } else { p.text2 },
    );
    if history_response.clicked() && !history_empty {
        let anchor = Rect::from_min_size(
            Pos2::new(history_rect.left(), history_rect.top()),
            Vec2::splat(1.0),
        );
        rt.open_menu(
            crate::runtime::MenuKind::CommitHistory {
                project: model.active_project.unwrap_or(0),
            },
            anchor,
            None::<String>,
        );
    }
    // 2) 提交信息输入（80px，placeholder "Commit Message"）。
    let input = Rect::from_min_size(
        Pos2::new(rect.left() + 10.0, options_row.bottom() + 4.0),
        Vec2::new(rect.width() - 20.0, 80.0),
    );
    ui.painter()
        .rect_filled(input, egui::CornerRadius::same(6), p.titlebar);
    ui.painter().rect_stroke(
        input,
        egui::CornerRadius::same(6),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    let mut value = message.to_string();
    let editor_rect = input.shrink2(Vec2::new(7.0, 5.0));
    let response = ui.put(
        editor_rect,
        TextEdit::multiline(&mut value)
            .frame(egui::Frame::NONE)
            .hint_text("Commit Message")
            .font(FontId::proportional(12.0))
            .desired_width(editor_rect.width()),
    );
    if response.changed() {
        model.active_project_mut().unwrap().commit_message = value;
    }
    if response.lost_focus() {
        model.save_workspace();
    }
    // 3) 按钮行：Commit（主）+ Commit and Push...（次，flex:1）。
    let actions_y = input.bottom() + 8.0;
    let commit_w = 96.0f32;
    let commit_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 10.0, actions_y),
        Vec2::new(commit_w, 28.0),
    );
    let push_rect = Rect::from_min_max(
        Pos2::new(commit_rect.right() + 6.0, actions_y),
        Pos2::new(rect.right() - 10.0, actions_y + 28.0),
    );
    let enabled = can_commit && !message.trim().is_empty() && checked > 0;
    let label = crate::tr!(model, "Commit", "提交").to_string();
    let response = primary_button(ui, commit_rect, &label, None, enabled, theme);
    if response.clicked() && enabled {
        model.queue(Command::Commit);
    }
    let push_label = crate::tr!(model, "Commit and Push...", "提交并推送…").to_string();
    let push_response = secondary_button_enabled(ui, push_rect, &push_label, None, enabled, theme);
    if push_response.clicked() && enabled {
        // 提交成功后由 JobResult 消费点置位 UiState::open_push_dialog。
        if let Some(project) = model.active_project_mut() {
            project.open_push_after_commit = true;
        }
        model.queue(Command::Commit);
    }
}

fn consoles(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), HEAD_H));
    if let Some(action) = side_head(
        ui,
        theme,
        head,
        crate::tr!(model, "Consoles", "控制台"),
        &[
            SideAction {
                icon: Icon::Spark,
                id: "agent",
                active: false,
                tooltip: crate::tr!(model, "Coding Agent", "编码智能体").to_string(),
            },
            SideAction {
                icon: Icon::Plus,
                id: "new",
                active: false,
                tooltip: crate::tr!(model, "New Terminal", "新建终端").to_string(),
            },
        ],
    ) {
        match action {
            0 => {
                rt.toggle_agent_menu(Rect::from_min_size(
                    Pos2::new(head.right() - 36.0, head.bottom()),
                    Vec2::splat(1.0),
                ));
            }
            1 => model.queue(Command::NewTerminal),
            _ => {}
        }
    }
    let list = Rect::from_min_max(Pos2::new(rect.left(), head.bottom()), rect.max);
    let total = model.total_running();

    #[derive(Clone)]
    struct Row {
        project: usize,
        worktree: usize,
        terminal: u64,
        name: String,
        command: String,
        running: bool,
        elapsed: String,
        group: String,
        group_start: bool,
        group_running: usize,
    }
    let mut rows: Vec<Row> = Vec::new();
    for (project_index, project) in model.projects.iter().enumerate() {
        for (worktree_index, worktree) in project.worktrees.iter().enumerate() {
            let group_running = worktree
                .terminals
                .iter()
                .flat_map(|tab| tab.panes.iter())
                .filter(|pane| pane.session.is_running())
                .count();
            let label = if worktree.is_main {
                project.name.clone()
            } else {
                format!("{} · {}", project.name, worktree.name)
            };
            let mut pane_index = 0usize;
            for tab in &worktree.terminals {
                for terminal in &tab.panes {
                    let secs = terminal.started.elapsed().as_secs();
                    rows.push(Row {
                        project: project_index,
                        worktree: worktree_index,
                        terminal: terminal.id,
                        name: terminal.title.clone(),
                        command: if let Some(command) = terminal.spawn_command.clone() {
                            command
                        } else if terminal.title != terminal.session.command_label {
                            terminal.session.command_label.clone()
                        } else {
                            crate::tr!(model, "default shell", "默认 shell").to_string()
                        },
                        running: terminal.session.is_running(),
                        elapsed: format_elapsed(secs),
                        group: label.clone(),
                        group_start: pane_index == 0,
                        group_running,
                    });
                    pane_index += 1;
                }
            }
        }
    }

    super::app_ui::region(ui, list, "consoles", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("console-list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    live_dot(
                        ui.painter(),
                        Pos2::new(ui.cursor().left() + 3.0, ui.cursor().center().y),
                        total > 0,
                        if total > 0 { p.running } else { p.text3 },
                    );
                    ui.add_space(10.0);
                    ui.label(
                        egui::RichText::new(if model.settings.language.is_zh() {
                            format!("{} 个进程运行于 {} 个项目", total, model.projects.len())
                        } else {
                            format!(
                                "{total} processes running across {} projects",
                                model.projects.len()
                            )
                        })
                        .size(10.5)
                        .color(p.text3),
                    );
                });
                ui.add_space(6.0);
                for (index, row) in rows.iter().enumerate() {
                    if row.group_start {
                        if index > 0 {
                            ui.add_space(4.0);
                        }
                        let (group_rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 24.0),
                            Sense::hover(),
                        );
                        ui.painter().text(
                            Pos2::new(group_rect.left() + 14.0, group_rect.center().y),
                            Align2::LEFT_CENTER,
                            &row.group,
                            FontId::proportional(10.5),
                            p.text3,
                        );
                        ui.painter().text(
                            Pos2::new(group_rect.right() - 10.0, group_rect.center().y),
                            Align2::RIGHT_CENTER,
                            if row.group_running > 0 {
                                if model.settings.language.is_zh() {
                                    format!("{} 个运行中", row.group_running)
                                } else {
                                    format!("{} running", row.group_running)
                                }
                            } else {
                                crate::tr!(model, "idle", "空闲").to_string()
                            },
                            FontId::proportional(9.5),
                            if row.group_running > 0 {
                                p.running
                            } else {
                                p.text3
                            },
                        );
                    }
                    let (row_rect, response) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 44.0), Sense::click());
                    let active = model.active_project == Some(row.project)
                        && model.projects[row.project].worktrees[row.worktree].active_terminal
                            == Some(row.terminal);
                    if active {
                        ui.painter().rect_filled(
                            row_rect.shrink2(Vec2::new(6.0, 2.0)),
                            egui::CornerRadius::same(4),
                            p.active,
                        );
                    } else if response.hovered() {
                        ui.painter().rect_filled(
                            row_rect.shrink2(Vec2::new(6.0, 2.0)),
                            egui::CornerRadius::same(4),
                            p.hover,
                        );
                    }
                    live_dot(
                        ui.painter(),
                        Pos2::new(row_rect.left() + 18.0, row_rect.center().y),
                        row.running,
                        if row.running { p.running } else { p.text3 },
                    );
                    ui.painter().text(
                        Pos2::new(row_rect.left() + 28.0, row_rect.top() + 14.0),
                        Align2::LEFT_CENTER,
                        &row.name,
                        FontId::proportional(11.5),
                        p.text,
                    );
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(row_rect.left() + 28.0, row_rect.top() + 25.0),
                            Pos2::new(row_rect.right() - 70.0, row_rect.bottom() - 3.0),
                        ),
                        &row.command,
                        FontId::monospace(9.0),
                        p.text3,
                        Align2::LEFT_CENTER,
                    );
                    ui.painter().text(
                        Pos2::new(row_rect.right() - 42.0, row_rect.center().y),
                        Align2::RIGHT_CENTER,
                        &row.elapsed,
                        FontId::monospace(9.0),
                        if row.running { p.text2 } else { p.text3 },
                    );
                    let action_rect = Rect::from_center_size(
                        Pos2::new(row_rect.right() - 18.0, row_rect.center().y),
                        Vec2::splat(22.0),
                    );
                    let action = ui.interact(
                        action_rect,
                        ui.scope_id().with(("console-action", row.terminal)),
                        Sense::click(),
                    );
                    if action.hovered() {
                        ui.painter()
                            .rect_filled(action_rect, egui::CornerRadius::same(5), p.hover);
                    }
                    paint_icon(
                        ui.painter(),
                        action_rect.shrink(6.0),
                        if row.running { Icon::Stop } else { Icon::Play },
                        if row.running { p.rose } else { p.green },
                    );
                    if action.clicked() {
                        if row.running {
                            if let Some(terminal) =
                                model.projects[row.project].term_by_id(row.terminal)
                            {
                                terminal.session.kill();
                            }
                        } else {
                            model.queue(Command::RestartTerminal(row.terminal));
                        }
                    }
                    if response.clicked() {
                        model.switch_project(row.project);
                        model.projects[row.project].active_worktree = row.worktree;
                        let worktree = &mut model.projects[row.project].worktrees[row.worktree];
                        if let Some(tab) = worktree
                            .terminals
                            .iter_mut()
                            .find(|tab| tab.pane(row.terminal).is_some())
                        {
                            tab.active_pane = row.terminal;
                        }
                        worktree.active_terminal = Some(row.terminal);
                        model.ui.panel_visible = true;
                        model.ui.terminal_visible = true;
                        model.ui.panel_visible = false;
                        rt.clear_terminal_cache();
                        rt.set_terminal_focus(row.terminal);
                    }
                }
                ui.add_space(10.0);
            });
    });

    let foot = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - FOOT_H), rect.max);
    ui.painter().rect_filled(foot, 0.0, p.sidebar);
    ui.painter().hline(
        foot.x_range(),
        foot.top() + 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    ui.painter().text(
        Pos2::new(foot.left() + 12.0, foot.center().y),
        Align2::LEFT_CENTER,
        crate::tr!(
            model,
            "Each console stays with its project.",
            "每个控制台都跟随其项目。"
        ),
        FontId::proportional(9.5),
        p.text3,
    );
}

fn format_elapsed(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {:02}m", seconds / 3600, (seconds % 3600) / 60)
    }
}

/// 解析补丁文本里的文件路径（`+++ b/<path>` 行）。
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
fn shelf_view(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    let root = model.active_project().unwrap().root.clone();
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

    super::app_ui::region(ui, rect, "shelf-rows", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("shelf-scroll")
            .auto_shrink([false, false])
            .show_rows(ui, 24.0, rows.len(), |ui, range| {
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
                                crate::tr!(
                                    model,
                                    "Recently Deleted · {count}",
                                    "最近删除 · {count}"
                                )
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

// ===================== Compare Versions 抽屉（稳定版 CompareDrawer） =====================

const CMP_HEADER_H: f32 = 34.0;
const CMP_TOOLBAR_H: f32 = 28.0;
const CMP_ROOT_H: f32 = 26.0;
const CMP_ROW_H: f32 = 24.0;

struct CmpRow {
    dir: Option<(String, String, bool)>, // (name, path, open)
    path: String,
    depth: usize,
    is_root: bool,
    status: char,
    additions: u32,
    deletions: u32,
    selected: bool,
}

pub fn compare_drawer(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
) {
    let p = theme.palette;
    let Some(index) = model.active_project else {
        return;
    };
    let Some(session) = model.projects[index].compare_session.clone() else {
        return;
    };
    let zh = model.settings.language.is_zh();
    // 不透明 bg-app（稳定版 .compare-drawer），右缘 1px 分隔。
    ui.painter().rect_filled(rect, 0.0, p.bg);
    ui.painter().vline(
        rect.right() - 0.5,
        rect.top()..=rect.bottom(),
        Stroke::new(1.0, p.border),
    );

    // ---- 1. 头部 34px：Changes Between pill + close，右侧 ⋮ / — ----
    let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), CMP_HEADER_H));
    ui.painter().rect_filled(head, 0.0, p.bg);
    ui.painter().hline(
        head.x_range(),
        head.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    let from8: String = session.from.chars().take(8).collect();
    let to8: String = session.to.chars().take(8).collect();
    let title = if zh {
        format!("{from8} 与 {to8} 之间的变更")
    } else {
        format!("Changes Between {from8} and {to8}")
    };
    let title_rect = Rect::from_min_max(
        Pos2::new(head.left() + 12.0, head.top() + 4.0),
        Pos2::new(head.right() - 56.0, head.bottom() - 4.0),
    );
    text_clipped(
        ui.painter(),
        title_rect,
        &title,
        FontId::proportional(12.0),
        p.text_bright,
        Align2::LEFT_CENTER,
    );
    let close_btn = Rect::from_min_size(
        Pos2::new(title_rect.right() + 4.0, head.center().y - 8.0),
        Vec2::new(16.0, 16.0),
    );
    if icon_16(ui, "cmp-close", close_btn, Icon::X, p.text3) {
        model.queue(Command::CompareClose);
        return;
    }
    let more_btn = Rect::from_min_size(
        Pos2::new(head.right() - 44.0, head.center().y - 9.0),
        Vec2::new(20.0, 18.0),
    );
    let _ = icon_16(ui, "cmp-more", more_btn, Icon::More, p.text3);
    let min_btn = Rect::from_min_size(
        Pos2::new(head.right() - 24.0, head.center().y - 9.0),
        Vec2::new(20.0, 18.0),
    );
    if icon_16(ui, "cmp-min", min_btn, Icon::Minimize, p.text3) {
        if let Some(session) = model.projects[index].compare_session.as_mut() {
            session.minimized = true;
        }
        model.ui.sidebar_view = SidebarView::Git;
        return;
    }

    // ---- 2. 工具栏 28px：⇆ 交换 + 视图选项；右侧 展开/折叠 ----
    let tb = Rect::from_min_size(
        Pos2::new(rect.left(), head.bottom()),
        Vec2::new(rect.width(), CMP_TOOLBAR_H),
    );
    ui.painter().rect_filled(tb, 0.0, p.bg);
    ui.painter().hline(
        tb.x_range(),
        tb.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    let swap_btn = Rect::from_center_size(
        Pos2::new(tb.left() + 18.0, tb.center().y),
        Vec2::new(20.0, 20.0),
    );
    if icon_16(ui, "cmp-swap", swap_btn, Icon::Compare, p.text2) {
        model.queue(Command::CompareSwap);
    }
    let view_btn = Rect::from_center_size(
        Pos2::new(tb.left() + 40.0, tb.center().y),
        Vec2::new(20.0, 20.0),
    );
    let is_cmp_view_open = rt
        .menu
        .as_ref()
        .is_some_and(|m| matches!(m.kind, crate::runtime::MenuKind::CompareViewOptions { .. }));
    if icon_16_active(
        ui,
        "cmp-view",
        view_btn,
        Icon::ViewOptions,
        p.text2,
        is_cmp_view_open,
    ) {
        rt.toggle_menu(
            crate::runtime::MenuKind::CompareViewOptions { project: index },
            view_btn,
            None,
        );
    }
    let expand_btn = Rect::from_center_size(
        Pos2::new(tb.right() - 40.0, tb.center().y),
        Vec2::new(20.0, 20.0),
    );
    let collapse_btn = Rect::from_center_size(
        Pos2::new(tb.right() - 18.0, tb.center().y),
        Vec2::new(20.0, 20.0),
    );
    let can_tree = session.group_by_dir;
    if icon_16_enabled(
        ui,
        "cmp-expand",
        expand_btn,
        Icon::ExpandAll,
        p.text2,
        can_tree,
    ) {
        if let Some(session) = model.projects[index].compare_session.as_mut() {
            session.root_expanded = true;
            session.collapsed.clear();
        }
    }
    if icon_16_enabled(
        ui,
        "cmp-collapse",
        collapse_btn,
        Icon::CollapseAll,
        p.text2,
        can_tree,
    ) {
        if let Some(session) = model.projects[index].compare_session.as_mut() {
            session.root_expanded = false;
        }
    }

    // ---- 3. 文件树 ----
    let body = Rect::from_min_max(Pos2::new(rect.left(), tb.bottom()), rect.max);
    if session.loading {
        ui.painter().text(
            body.center(),
            Align2::CENTER_CENTER,
            if zh {
                "正在加载差异…"
            } else {
                "Loading differences…"
            },
            FontId::proportional(11.0),
            p.text3,
        );
        return;
    }
    if session.files.is_empty() {
        ui.painter().text(
            body.center(),
            Align2::CENTER_CENTER,
            if zh {
                "两个版本之间没有差异"
            } else {
                "No differences between versions"
            },
            FontId::proportional(11.0),
            p.text3,
        );
        return;
    }

    let root_name = model.projects[index].name.clone();

    // 组装可见行（目录优先、文件在后；与稳定版 renderDirNodes 一致）。
    let mut rows: Vec<CmpRow> = Vec::new();
    if !session.group_by_dir {
        for file in &session.files {
            rows.push(CmpRow {
                dir: None,
                path: file.path.clone(),
                depth: 0,
                is_root: false,
                status: file.status.to_ascii_uppercase(),
                additions: file.additions,
                deletions: file.deletions,
                selected: session.selected.as_deref() == Some(file.path.as_str()),
            });
        }
    } else {
        rows.push(CmpRow {
            dir: Some((String::new(), String::new(), session.root_expanded)),
            path: String::new(),
            depth: 0,
            is_root: true,
            status: 'M',
            additions: 0,
            deletions: 0,
            selected: false,
        });
        if session.root_expanded {
            push_cmp_dirs(
                &session.files,
                &session.collapsed,
                "",
                1,
                &mut rows,
                session.selected.as_deref(),
            );
        }
    }

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(body)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    egui::ScrollArea::vertical()
        .id_salt("compare-drawer-tree")
        .auto_shrink([false, false])
        .show(&mut child, |ui| {
            let pointer = ui.input(|i| i.pointer.latest_pos().unwrap_or_default());
            for row in &rows {
                let h = if row.is_root { CMP_ROOT_H } else { CMP_ROW_H };
                let (row_rect, _) =
                    ui.allocate_exact_size(Vec2::new(body.width(), h), Sense::click());
                let indent = if row.is_root {
                    10.0
                } else {
                    10.0 + row.depth as f32 * 14.0 + if row.dir.is_none() { 16.0 } else { 0.0 }
                };
                let hover = row_rect.contains(pointer) && !row.is_root;
                let should_reveal = row.dir.is_none()
                    && row.selected
                    && session.selected.as_ref() != rt.last_revealed_cmp_file.as_ref();
                if row.dir.is_none() && row.selected {
                    ui.painter().rect_filled(
                        row_rect.shrink2(Vec2::new(4.0, 1.0)),
                        egui::CornerRadius::same(4),
                        p.jb_accent,
                    );
                    if should_reveal {
                        ui.scroll_to_rect(row_rect, Some(egui::Align::Center));
                    }
                } else if hover {
                    ui.painter().rect_filled(
                        row_rect.shrink2(Vec2::new(4.0, 1.0)),
                        egui::CornerRadius::same(4),
                        p.hover,
                    );
                }
                if let Some((name, _path, open)) = &row.dir {
                    let chev = Pos2::new(row_rect.left() + indent, row_rect.center().y);
                    paint_chevron(ui.painter(), chev, *open, p.text3, 11.0);
                    let folder = Rect::from_center_size(
                        Pos2::new(row_rect.left() + indent + 14.0, row_rect.center().y),
                        Vec2::new(16.0, 16.0),
                    );
                    paint_icon(
                        ui.painter(),
                        folder,
                        Icon::Folder,
                        Color32::from_rgb(0xdc, 0xd7, 0xaa),
                    );
                    if row.is_root {
                        let name_x = row_rect.left() + indent + 26.0;
                        ui.painter().text(
                            Pos2::new(name_x, row_rect.center().y),
                            Align2::LEFT_CENTER,
                            &root_name,
                            FontId::proportional(12.0),
                            p.text,
                        );
                        let count = if session.files.len() == 1 {
                            "1 file".to_string()
                        } else {
                            format!("{} files", session.files.len())
                        };
                        let count_x = name_x + root_name.chars().count() as f32 * 6.9 + 8.0;
                        ui.painter().text(
                            Pos2::new(count_x, row_rect.center().y),
                            Align2::LEFT_CENTER,
                            count,
                            FontId::proportional(11.0),
                            p.text3,
                        );
                    } else {
                        ui.painter().text(
                            Pos2::new(row_rect.left() + indent + 26.0, row_rect.center().y),
                            Align2::LEFT_CENTER,
                            name,
                            FontId::proportional(12.0),
                            p.text,
                        );
                    }
                    if response_clicked(ui, row_rect, ("cmp-dir", row.path.clone())) {
                        if row.is_root {
                            if let Some(session) = model.projects[index].compare_session.as_mut() {
                                session.root_expanded = !session.root_expanded;
                            }
                        } else if let Some(session) = model.projects[index].compare_session.as_mut()
                        {
                            if session.collapsed.contains(&row.path) {
                                session.collapsed.remove(&row.path);
                            } else {
                                session.collapsed.insert(row.path.clone());
                            }
                        }
                    }
                } else {
                    // 文件行。
                    let name_color = if row.selected {
                        Color32::WHITE
                    } else if row.status == 'A' || row.status == '?' || row.status == 'R' {
                        p.green
                    } else if row.status == 'D' {
                        p.rose
                    } else {
                        p.blue
                    };
                    let icon_rect = Rect::from_center_size(
                        Pos2::new(row_rect.left() + indent, row_rect.center().y),
                        Vec2::new(14.0, 14.0),
                    );
                    paint_icon(
                        ui.painter(),
                        icon_rect,
                        Icon::File,
                        if row.selected {
                            Color32::from_rgba_unmultiplied(255, 255, 255, 230)
                        } else {
                            p.text3
                        },
                    );
                    let base = row.path.rsplit('/').next().unwrap_or(&row.path);
                    ui.painter().text(
                        Pos2::new(row_rect.left() + indent + 12.0, row_rect.center().y),
                        Align2::LEFT_CENTER,
                        base,
                        FontId::proportional(12.0),
                        name_color,
                    );
                    let mut nx = row_rect.right() - 12.0;
                    if row.deletions > 0 {
                        let label = format!("-{}", row.deletions);
                        nx -= label.chars().count() as f32 * 6.2;
                        ui.painter().text(
                            Pos2::new(nx, row_rect.center().y),
                            Align2::LEFT_CENTER,
                            label,
                            FontId::proportional(11.0),
                            if row.selected {
                                Color32::from_rgba_unmultiplied(255, 255, 255, 220)
                            } else {
                                p.rose
                            },
                        );
                        nx -= 6.0;
                    }
                    if row.additions > 0 || row.deletions == 0 {
                        let label = format!("+{}", row.additions);
                        nx -= label.chars().count() as f32 * 6.2;
                        ui.painter().text(
                            Pos2::new(nx, row_rect.center().y),
                            Align2::LEFT_CENTER,
                            label,
                            FontId::proportional(11.0),
                            if row.selected {
                                Color32::from_rgba_unmultiplied(255, 255, 255, 220)
                            } else {
                                p.green
                            },
                        );
                    }
                    let response = ui.interact(
                        row_rect,
                        ui.scope_id().with(("cmp-file", row.path.as_str())),
                        Sense::click(),
                    );
                    if response.clicked() {
                        if let Some(session) = model.projects[index].compare_session.as_mut() {
                            session.selected = Some(row.path.clone());
                        }
                    }
                    if response.double_clicked() {
                        let target = row.path.clone();
                        model.queue(Command::OpenCompareCommitsDiff {
                            from: session.from.clone(),
                            to: session.to.clone(),
                            path: target.clone(),
                        });
                        if let Some(session) = model.projects[index].compare_session.as_mut() {
                            session.selected = Some(target);
                        }
                    }
                }
            }
        });
    rt.last_revealed_cmp_file = session.selected.clone();
}

/// 递归压平目录行 + 文件行（目录在前、文件在后，同层字典序）。
fn push_cmp_dirs(
    files: &[lazecode_git::CompareFile],
    collapsed: &HashSet<String>,
    prefix: &str,
    depth: usize,
    rows: &mut Vec<CmpRow>,
    selected: Option<&str>,
) {
    let mut dirs: Vec<String> = Vec::new();
    for file in files {
        let Some(rest) = file.path.strip_prefix(prefix) else {
            // 不属于该前缀的文件（理论上不会发生）——跳过，绝不回退到全路径，
            // 否则 dir key 会每层重复拼接造成无限递归。
            continue;
        };
        let rest = rest.strip_prefix('/').unwrap_or(rest);
        if let Some(head) = rest.split('/').next() {
            if rest.contains('/') && !head.is_empty() {
                let dir_key = if prefix.is_empty() {
                    head.to_string()
                } else {
                    format!("{prefix}/{head}")
                };
                if !dirs.contains(&dir_key) {
                    dirs.push(dir_key);
                }
            }
        }
    }
    dirs.sort();
    for dir in dirs {
        let open = !collapsed.contains(&dir);
        let name = dir.rsplit('/').next().unwrap_or(&dir).to_string();
        rows.push(CmpRow {
            dir: Some((name, dir.clone(), open)),
            path: dir.clone(),
            depth,
            is_root: false,
            status: 'M',
            additions: 0,
            deletions: 0,
            selected: false,
        });
        if open {
            push_cmp_dirs(files, collapsed, &dir, depth + 1, rows, selected);
        }
    }
    let mut level_files: Vec<&lazecode_git::CompareFile> = files
        .iter()
        .filter(|file| {
            let rest = file.path.strip_prefix(prefix).unwrap_or(&file.path);
            let rest = rest.strip_prefix('/').unwrap_or(rest);
            !rest.contains('/')
        })
        .collect();
    level_files.sort_by(|a, b| a.path.cmp(&b.path));
    for file in level_files {
        rows.push(CmpRow {
            dir: None,
            path: file.path.clone(),
            depth,
            is_root: false,
            status: file.status.to_ascii_uppercase(),
            additions: file.additions,
            deletions: file.deletions,
            selected: selected == Some(file.path.as_str()),
        });
    }
}

/// 行点击辅助（独立 scope id）。
fn response_clicked(ui: &mut Ui, rect: Rect, key: (&'static str, String)) -> bool {
    ui.interact(rect, ui.scope_id().with(key), Sense::click())
        .clicked()
}

/// 20px 图标按钮（hover 填充），返回是否本帧被点击。
fn icon_16(ui: &mut Ui, key: &'static str, rect: Rect, icon: Icon, color: Color32) -> bool {
    icon_16_impl(ui, key, rect, icon, color, false, false)
}

fn icon_16_active(
    ui: &mut Ui,
    key: &'static str,
    rect: Rect,
    icon: Icon,
    color: Color32,
    active: bool,
) -> bool {
    icon_16_impl(ui, key, rect, icon, color, active, false)
}

fn icon_16_enabled(
    ui: &mut Ui,
    key: &'static str,
    rect: Rect,
    icon: Icon,
    color: Color32,
    enabled: bool,
) -> bool {
    icon_16_impl(ui, key, rect, icon, color, false, !enabled)
}

fn icon_16_impl(
    ui: &mut Ui,
    key: &'static str,
    rect: Rect,
    icon: Icon,
    color: Color32,
    active: bool,
    disabled: bool,
) -> bool {
    let response = ui.interact(rect, ui.scope_id().with(key), Sense::click());
    let hover = response.hovered() && !disabled;
    if hover || active {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(6), p_hover());
    }
    let tint = if disabled {
        color.gamma_multiply(0.4)
    } else {
        color
    };
    paint_icon(ui.painter(), rect.shrink(3.0), icon, tint);
    response.clicked() && !disabled
}

fn p_hover() -> Color32 {
    crate::theme::Palette::dark().hover
}

/// 小折叠箭头（10-11px）。
fn paint_chevron(painter: &egui::Painter, center: Pos2, open: bool, color: Color32, size: f32) {
    let r = size * 0.32;
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
