//! Sidebar: explorer, source control, consoles, shelf, stash and compare
//! drawers, split into one module per view.

mod changes;
mod compare;
mod prelude;

use changes::*;
use compare::*;
use prelude::*;

const HEAD_H: f32 = 36.0;
const FOOT_H: f32 = 25.0;
const ROW_H: f32 = 25.0;

/// The sidebar only draws when a project is open, so `active_project` is an
/// invariant; route accesses through these helpers for one clear panic site.
fn active(model: &AppModel) -> &lazecode_core::ProjectState {
    model
        .active_project()
        .expect("sidebar drawn without an active project")
}

fn active_mut(model: &mut AppModel) -> &mut lazecode_core::ProjectState {
    model
        .active_project_mut()
        .expect("sidebar drawn without an active project")
}

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
    super::app_ui::region(ui, list_rect, "tree-scroll", |ui| {
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
    let total = rt.busy_count();

    #[derive(Clone)]
    struct ConsoleRow {
        project: usize,
        worktree: usize,
        terminal: u64,
        name: String,
        command: String,
        busy: bool,
        outputting: bool,
        elapsed: String,
    }
    struct WorktreeGroup {
        name: String,
        key: String,
        outputting: usize,
        rows: Vec<ConsoleRow>,
    }
    struct ProjectGroup {
        name: String,
        key: String,
        outputting: usize,
        worktrees: Vec<WorktreeGroup>,
    }
    let mut groups: Vec<ProjectGroup> = Vec::new();
    for (project_index, project) in model.projects.iter().enumerate() {
        let mut group = ProjectGroup {
            name: project.name.clone(),
            key: format!("p:{}", project.root.display()),
            outputting: 0,
            worktrees: Vec::new(),
        };
        for (worktree_index, worktree) in project.worktrees.iter().enumerate() {
            let mut worktree_group = WorktreeGroup {
                name: worktree.name.clone(),
                key: format!("w:{}", worktree.path.display()),
                outputting: 0,
                rows: Vec::new(),
            };
            for terminal in worktree.terminals.iter().flat_map(|tab| tab.panes.iter()) {
                // 与项目条 marquee 同源的状态：busy = 有前台进程，
                // outputting = console_outputting() 判定的“正在输出”。
                let state = rt.terminal_busy.get(&terminal.id);
                let busy = state.is_some_and(|state| state.busy);
                let outputting = busy && state.is_some_and(|state| state.outputting);
                // 运行中把标题换成探针看到的前台进程名（如 omp），空闲回落。
                let name = match state.filter(|state| state.busy && !state.commands.is_empty()) {
                    Some(state) => state.commands.join(" "),
                    None => terminal.title.clone(),
                };
                worktree_group.rows.push(ConsoleRow {
                    project: project_index,
                    worktree: worktree_index,
                    terminal: terminal.id,
                    name,
                    command: if let Some(command) = terminal.spawn_command.clone() {
                        command
                    } else if terminal.title != terminal.session.command_label {
                        terminal.session.command_label.clone()
                    } else {
                        crate::tr!(model, "default shell", "默认 shell").to_string()
                    },
                    busy,
                    outputting,
                    elapsed: format_elapsed(terminal.started.elapsed().as_secs()),
                });
            }
            worktree_group.outputting = worktree_group
                .rows
                .iter()
                .filter(|row| row.outputting)
                .count();
            group.outputting += worktree_group.outputting;
            group.worktrees.push(worktree_group);
        }
        groups.push(group);
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
                            let process = if total == 1 { "process" } else { "processes" };
                            let project = if model.projects.len() == 1 {
                                "project"
                            } else {
                                "projects"
                            };
                            format!(
                                "{total} {process} running across {} {project}",
                                model.projects.len()
                            )
                        })
                        .size(10.5)
                        .color(p.text3),
                    );
                });
                ui.add_space(6.0);
                // 项目 → 工作树 → 控制台：两级分组，组头点击折叠，默认全展开。
                for group in &groups {
                    let collapsed = rt.consoles_collapsed.contains(&group.key);
                    let (head_rect, head) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::click());
                    if head.hovered() {
                        ui.painter().rect_filled(
                            head_rect.shrink2(Vec2::new(6.0, 1.0)),
                            egui::CornerRadius::same(4),
                            p.hover,
                        );
                    }
                    if head.clicked() {
                        if collapsed {
                            rt.consoles_collapsed.remove(&group.key);
                        } else {
                            rt.consoles_collapsed.insert(group.key.clone());
                        }
                    }
                    paint_icon(
                        ui.painter(),
                        Rect::from_center_size(
                            Pos2::new(head_rect.left() + 16.0, head_rect.center().y),
                            Vec2::splat(14.0),
                        ),
                        if collapsed {
                            Icon::ChevronRight
                        } else {
                            Icon::ChevronDown
                        },
                        p.text3,
                    );
                    ui.painter().text(
                        Pos2::new(head_rect.left() + 28.0, head_rect.center().y),
                        Align2::LEFT_CENTER,
                        &group.name,
                        FontId::proportional(11.5),
                        p.text,
                    );
                    group_summary(
                        ui,
                        model,
                        head_rect.right() - 10.0,
                        head_rect.center().y,
                        group.outputting,
                        p,
                    );
                    if collapsed {
                        continue;
                    }
                    for worktree in &group.worktrees {
                        let collapsed = rt.consoles_collapsed.contains(&worktree.key);
                        let (sub_rect, sub) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 22.0),
                            Sense::click(),
                        );
                        if sub.hovered() {
                            ui.painter().rect_filled(
                                sub_rect.shrink2(Vec2::new(6.0, 1.0)),
                                egui::CornerRadius::same(4),
                                p.hover,
                            );
                        }
                        if sub.clicked() {
                            if collapsed {
                                rt.consoles_collapsed.remove(&worktree.key);
                            } else {
                                rt.consoles_collapsed.insert(worktree.key.clone());
                            }
                        }
                        paint_icon(
                            ui.painter(),
                            Rect::from_center_size(
                                Pos2::new(sub_rect.left() + 30.0, sub_rect.center().y),
                                Vec2::splat(12.0),
                            ),
                            if collapsed {
                                Icon::ChevronRight
                            } else {
                                Icon::ChevronDown
                            },
                            p.text3,
                        );
                        ui.painter().text(
                            Pos2::new(sub_rect.left() + 44.0, sub_rect.center().y),
                            Align2::LEFT_CENTER,
                            &worktree.name,
                            FontId::proportional(10.5),
                            p.text2,
                        );
                        group_summary(
                            ui,
                            model,
                            sub_rect.right() - 10.0,
                            sub_rect.center().y,
                            worktree.outputting,
                            p,
                        );
                        if collapsed {
                            continue;
                        }
                        for row in &worktree.rows {
                            let (row_rect, response) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 44.0),
                                Sense::click(),
                            );
                            let active = model.active_project == Some(row.project)
                                && model.projects[row.project].worktrees[row.worktree]
                                    .active_terminal
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
                            // 状态：空闲灰点 / 运行中绿对勾 / 输出中红圈旋转。
                            let glyph = Pos2::new(row_rect.left() + 46.0, row_rect.center().y);
                            if row.outputting {
                                paint_output_spinner(ui, glyph, 5.0, p.rose);
                            } else if row.busy {
                                paint_icon(
                                    ui.painter(),
                                    Rect::from_center_size(glyph, Vec2::splat(14.0)),
                                    Icon::Check,
                                    p.green,
                                );
                            } else {
                                live_dot(ui.painter(), glyph, false, p.text3);
                            }
                            text_clipped(
                                ui.painter(),
                                Rect::from_min_max(
                                    Pos2::new(row_rect.left() + 56.0, row_rect.top() + 4.0),
                                    Pos2::new(row_rect.right() - 62.0, row_rect.top() + 24.0),
                                ),
                                &row.name,
                                FontId::proportional(11.5),
                                p.text,
                                Align2::LEFT_CENTER,
                            );
                            text_clipped(
                                ui.painter(),
                                Rect::from_min_max(
                                    Pos2::new(row_rect.left() + 56.0, row_rect.top() + 25.0),
                                    Pos2::new(row_rect.right() - 62.0, row_rect.bottom() - 3.0),
                                ),
                                &row.command,
                                FontId::monospace(9.0),
                                p.text3,
                                Align2::LEFT_CENTER,
                            );
                            ui.painter().text(
                                Pos2::new(row_rect.right() - 12.0, row_rect.center().y),
                                Align2::RIGHT_CENTER,
                                &row.elapsed,
                                FontId::monospace(9.0),
                                if row.busy { p.text2 } else { p.text3 },
                            );
                            if response.clicked() {
                                model.switch_project(row.project);
                                model.projects[row.project].active_worktree = row.worktree;
                                let worktree =
                                    &mut model.projects[row.project].worktrees[row.worktree];
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

/// 组头右侧的状态摘要：只统计控制台条目前面红色转圈（正在输出）的数量。
/// 没有红圈时显示灰色 `idle`。
fn group_summary(
    ui: &Ui,
    model: &AppModel,
    right: f32,
    center_y: f32,
    outputting: usize,
    p: crate::theme::Palette,
) {
    let (text, color) = if outputting > 0 {
        (
            if model.settings.language.is_zh() {
                format!("{outputting} 个运行中")
            } else {
                format!("{outputting} running")
            },
            p.rose,
        )
    } else {
        (crate::tr!(model, "idle", "空闲").to_string(), p.text3)
    };
    let font = FontId::proportional(9.5);
    let width = ui
        .painter()
        .layout_no_wrap(text.clone(), font.clone(), color)
        .size()
        .x;
    ui.painter().text(
        Pos2::new(right, center_y),
        Align2::RIGHT_CENTER,
        &text,
        font,
        color,
    );
    if outputting > 0 {
        paint_output_spinner(ui, Pos2::new(right - width - 11.0, center_y), 4.5, p.rose);
    }
}

/// 红色旋转圆弧，语义与项目条 marquee 的“正在输出”一致；按 30 fps 重绘。
fn paint_output_spinner(ui: &Ui, center: Pos2, radius: f32, color: Color32) {
    let time = ui.ctx().input(|input| input.time) as f32;
    let start = (time * 1.8).rem_euclid(std::f32::consts::TAU);
    let sweep = std::f32::consts::TAU * 0.72;
    let steps = 20;
    let points: Vec<Pos2> = (0..=steps)
        .map(|step| {
            let angle = start + sweep * step as f32 / steps as f32;
            center + Vec2::new(angle.cos() * radius, angle.sin() * radius)
        })
        .collect();
    ui.painter()
        .add(egui::Shape::line(points, Stroke::new(1.8, color)));
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(33));
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

// ---------------- Stash 页签（贮藏列表 + 可折叠文件树 + Apply/Pop） ----------------

#[derive(Default)]
struct StashDirNode {
    /// 子树中的文件数（目录行展示）。
    count: usize,
    sub: std::collections::BTreeMap<String, StashDirNode>,
    files: Vec<String>,
}

enum StashRow {
    Stash {
        name: String,
        message: String,
        branch: Option<String>,
        selected: bool,
        expanded: bool,
    },
    Root {
        stash: String,
        key: String,
        kind: String,
        label: String,
        count: usize,
        expanded: bool,
    },
    Dir {
        stash: String,
        key: String,
        /// 仓库相对目录路径（右键菜单的作用范围）。
        path: String,
        label: String,
        count: usize,
        depth: usize,
        expanded: bool,
    },
    File {
        stash: String,
        path: String,
        name: String,
        depth: usize,
    },
}

fn insert_stash_file(node: &mut StashDirNode, parts: &[&str], full: &str) {
    if parts.len() <= 1 {
        node.files.push(full.to_string());
        return;
    }
    let child = node.sub.entry(parts[0].to_string()).or_default();
    child.count += 1;
    insert_stash_file(child, &parts[1..], full);
}

#[allow(clippy::too_many_arguments)]
fn flatten_stash_dirs(
    stash: &str,
    root_key: &str,
    nodes: &std::collections::BTreeMap<String, StashDirNode>,
    prefix: &str,
    depth: usize,
    collapsed: &HashSet<String>,
    rows: &mut Vec<StashRow>,
) {
    for (name, node) in nodes {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let key = format!("{root_key}:{path}");
        let expanded = !collapsed.contains(&key);
        rows.push(StashRow::Dir {
            stash: stash.to_string(),
            key: key.clone(),
            path: path.clone(),
            label: name.clone(),
            count: node.count,
            depth,
            expanded,
        });
        if expanded {
            flatten_stash_dirs(
                stash,
                root_key,
                &node.sub,
                &path,
                depth + 1,
                collapsed,
                rows,
            );
            for file in &node.files {
                let name = file.rsplit('/').next().unwrap_or(file).to_string();
                rows.push(StashRow::File {
                    stash: stash.to_string(),
                    path: file.clone(),
                    name,
                    depth: depth + 1,
                });
            }
        }
    }
}

/// 一条贮藏下的一个树根（已跟踪改动 / 未跟踪快照）。
/// key 带贮藏名前缀，多条贮藏同时展开时目录折叠状态互不影响。
fn push_stash_root(
    rows: &mut Vec<StashRow>,
    stash: &str,
    collapsed: &HashSet<String>,
    kind: &str,
    label: &str,
    files: &[String],
) {
    let root_key = format!("{stash}:{kind}");
    let expanded = !collapsed.contains(&root_key);
    rows.push(StashRow::Root {
        stash: stash.to_string(),
        key: root_key.clone(),
        kind: kind.to_string(),
        label: label.to_string(),
        count: files.len(),
        expanded,
    });
    if !expanded || files.is_empty() {
        return;
    }
    let mut tree = StashDirNode::default();
    for file in files {
        let parts: Vec<&str> = file.split('/').collect();
        insert_stash_file(&mut tree, &parts, file);
    }
    // 树根下的目录与散文件比根行深一级（PyCharm 的缩进层级）。
    flatten_stash_dirs(stash, &root_key, &tree.sub, "", 1, collapsed, rows);
    // 根目录下的散文件排在目录之后。
    for file in &tree.files {
        let name = file.rsplit('/').next().unwrap_or(file).to_string();
        rows.push(StashRow::File {
            stash: stash.to_string(),
            path: file.clone(),
            name,
            depth: 1,
        });
    }
}

/// 右键菜单 payload：`<stash>\x01<kind>\x01<path>`，kind 为 f/d/t/u
/// （文件 / 目录 / 已跟踪树根 / 未跟踪树根）。
fn stash_menu_payload(stash: &str, kind: &str, path: &str) -> String {
    format!("{stash}\u{1}{kind}\u{1}{path}")
}

fn stash_view(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    let zh = model.settings.language.is_zh();
    let project_index = model.active_project.unwrap_or(0);

    if rt.stash_entries.is_empty() {
        return;
    }
    // 选中项失效（被丢弃/切换仓库）→ 回退到最新一条并展开它。
    let selection_valid = rt
        .stash_selected
        .as_ref()
        .is_some_and(|name| rt.stash_entries.iter().any(|entry| &entry.name == name));
    let mut reveal_selected = false;
    if !selection_valid {
        rt.stash_selected = rt.stash_entries.first().map(|entry| entry.name.clone());
        rt.stash_file_sel = None;
        reveal_selected = true;
    }
    if let Some(name) = rt.stash_selected.clone() {
        if reveal_selected {
            rt.stash_expanded.insert(name);
        }
    }
    // 丢弃的贮藏不再占用展开状态。
    let known: HashSet<String> = rt
        .stash_entries
        .iter()
        .map(|entry| entry.name.clone())
        .collect();
    rt.stash_expanded.retain(|name| known.contains(name));

    // 每条展开的贮藏独立加载文件分组（model 缓存，git 只跑一次）。
    let entries = rt.stash_entries.clone();
    let mut details_by_stash: std::collections::HashMap<String, lazecode_git::StashDetails> =
        std::collections::HashMap::new();
    for entry in &entries {
        if rt.stash_expanded.contains(&entry.name) {
            if let Some(details) = model.projects[project_index].stash_details_for(&entry.name) {
                details_by_stash.insert(entry.name.clone(), details);
            }
        }
    }
    let project_name = active(model).name.clone();

    let mut rows: Vec<StashRow> = Vec::new();
    for entry in &entries {
        rows.push(StashRow::Stash {
            name: entry.name.clone(),
            message: entry.message(),
            branch: entry.branch.clone(),
            selected: rt.stash_selected.as_deref() == Some(entry.name.as_str()),
            expanded: rt.stash_expanded.contains(&entry.name),
        });
        let Some(details) = details_by_stash.get(&entry.name) else {
            continue;
        };
        if !details.tracked.is_empty() {
            push_stash_root(
                &mut rows,
                &entry.name,
                &rt.stash_collapsed,
                "tracked",
                &project_name,
                &details.tracked,
            );
        }
        if !details.untracked.is_empty() {
            let label = match entry.branch.as_deref() {
                Some(branch) if zh => format!("{branch} 上的未跟踪文件"),
                Some(branch) => format!("Untracked files on {branch}"),
                None if zh => "未跟踪文件".to_string(),
                None => "Untracked files".to_string(),
            };
            push_stash_root(
                &mut rows,
                &entry.name,
                &rt.stash_collapsed,
                "untracked",
                &label,
                &details.untracked,
            );
        }
    }

    let bar = Rect::from_min_size(
        Pos2::new(rect.left(), rect.bottom() - 34.0),
        Vec2::new(rect.width(), 34.0),
    );
    let list = Rect::from_min_max(rect.min, Pos2::new(rect.max.x, bar.top()));
    let count_suffix = if zh { "个文件" } else { "files" };

    let mut toggle: Option<String> = None;
    let mut select_stash: Option<String> = None;
    let mut select_file: Option<(String, String)> = None;
    let mut open_diff: Option<(String, String)> = None;
    super::app_ui::region(ui, list, "stash-rows", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("stash-scroll")
            .auto_shrink([false, false])
            .show_rows(ui, 24.0, rows.len(), |ui, range| {
                for row_index in range {
                    let Some(row) = rows.get(row_index) else {
                        continue;
                    };
                    match row {
                        StashRow::Stash {
                            name,
                            message,
                            branch,
                            selected,
                            expanded,
                        } => {
                            let (rect, response) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 24.0),
                                Sense::click(),
                            );
                            if *selected || response.hovered() {
                                ui.painter().rect_filled(
                                    rect.shrink2(Vec2::new(2.0, 1.0)),
                                    egui::CornerRadius::same(4),
                                    if *selected { p.selected_row } else { p.hover },
                                );
                            }
                            // 条目折叠箭头：多条贮藏各自展开/折叠。
                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(rect.left() + 10.0, rect.center().y),
                                    Vec2::splat(11.0),
                                ),
                                if *expanded {
                                    Icon::ChevronDown
                                } else {
                                    Icon::ChevronRight
                                },
                                p.text3,
                            );
                            let badge_w = branch
                                .as_ref()
                                .map(|name| {
                                    (name.chars().count() as f32 * 6.2 + 30.0)
                                        .min(rect.width() * 0.45)
                                })
                                .unwrap_or(0.0);
                            text_clipped(
                                ui.painter(),
                                Rect::from_min_max(
                                    Pos2::new(rect.left() + 24.0, rect.top()),
                                    Pos2::new(rect.right() - badge_w - 8.0, rect.bottom()),
                                ),
                                message,
                                FontId::proportional(12.5),
                                if *selected { p.text_bright } else { p.text },
                                Align2::LEFT_CENTER,
                            );
                            if let Some(branch) = branch {
                                let badge_rect = Rect::from_min_max(
                                    Pos2::new(rect.right() - badge_w - 6.0, rect.top() + 3.0),
                                    Pos2::new(rect.right() - 6.0, rect.bottom() - 3.0),
                                );
                                ui.painter().rect_filled(
                                    badge_rect,
                                    egui::CornerRadius::same(4),
                                    p.panel2,
                                );
                                paint_icon(
                                    ui.painter(),
                                    Rect::from_center_size(
                                        Pos2::new(badge_rect.left() + 10.0, badge_rect.center().y),
                                        Vec2::splat(11.0),
                                    ),
                                    Icon::Branch,
                                    p.text3,
                                );
                                text_clipped(
                                    ui.painter(),
                                    Rect::from_min_max(
                                        Pos2::new(badge_rect.left() + 18.0, badge_rect.top()),
                                        Pos2::new(badge_rect.right() - 4.0, badge_rect.bottom()),
                                    ),
                                    branch,
                                    FontId::proportional(10.5),
                                    p.text2,
                                    Align2::LEFT_CENTER,
                                );
                            }
                            if response.clicked() {
                                select_stash = Some(name.clone());
                                toggle = Some(name.clone());
                            }
                            if response.secondary_clicked() {
                                rt.stash_selected = Some(name.clone());
                                rt.stash_file_sel = None;
                                rt.open_menu(
                                    crate::runtime::MenuKind::StashEntryMenu {
                                        project: project_index,
                                    },
                                    rect,
                                    Some(name.clone()),
                                );
                            }
                        }
                        StashRow::Root { .. } | StashRow::Dir { .. } => {
                            let (stash, key, kind, label, count, depth, expanded) = match row {
                                StashRow::Root {
                                    stash,
                                    key,
                                    kind,
                                    label,
                                    count,
                                    expanded,
                                } => (stash, key, Some(kind), label, count, 0usize, expanded),
                                StashRow::Dir {
                                    stash,
                                    key,
                                    label,
                                    count,
                                    depth,
                                    expanded,
                                    ..
                                } => (stash, key, None, label, count, *depth, expanded),
                                _ => unreachable!(),
                            };
                            let (rect, response) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 24.0),
                                Sense::click(),
                            );
                            if response.hovered() {
                                ui.painter().rect_filled(
                                    rect.shrink2(Vec2::new(2.0, 1.0)),
                                    egui::CornerRadius::same(4),
                                    p.hover,
                                );
                            }
                            let x = rect.left() + 10.0 + depth as f32 * 12.0;
                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(x + 4.0, rect.center().y),
                                    Vec2::splat(11.0),
                                ),
                                if *expanded {
                                    Icon::ChevronDown
                                } else {
                                    Icon::ChevronRight
                                },
                                p.text3,
                            );
                            let galley = ui.painter().layout_no_wrap(
                                label.clone(),
                                FontId::proportional(12.0),
                                p.text,
                            );
                            ui.painter().galley(
                                Pos2::new(x + 16.0, rect.center().y - galley.size().y * 0.5),
                                galley.clone(),
                                p.text,
                            );
                            ui.painter().text(
                                Pos2::new(x + 22.0 + galley.size().x, rect.center().y),
                                Align2::LEFT_CENTER,
                                format!("{count} {count_suffix}"),
                                FontId::proportional(10.5),
                                p.text3,
                            );
                            if response.clicked() {
                                toggle = Some(key.clone());
                            }
                            if response.secondary_clicked() {
                                // 目录 / 树根与文件共用同一套右键菜单。
                                let payload = match kind {
                                    Some(kind) => stash_menu_payload(stash, kind, ""),
                                    None => match row {
                                        StashRow::Dir { path, .. } => {
                                            stash_menu_payload(stash, "d", path)
                                        }
                                        _ => String::new(),
                                    },
                                };
                                rt.open_menu(
                                    crate::runtime::MenuKind::StashFileMenu {
                                        project: project_index,
                                    },
                                    rect,
                                    Some(payload),
                                );
                            }
                        }
                        StashRow::File {
                            stash,
                            path,
                            name,
                            depth,
                        } => {
                            let (rect, response) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 24.0),
                                Sense::click(),
                            );
                            let selected = rt.stash_selected.as_deref() == Some(stash.as_str())
                                && rt.stash_file_sel.as_deref() == Some(path.as_str());
                            if selected || response.hovered() {
                                ui.painter().rect_filled(
                                    rect.shrink2(Vec2::new(2.0, 1.0)),
                                    egui::CornerRadius::same(4),
                                    if selected { p.selected_row } else { p.hover },
                                );
                            }
                            let x = rect.left() + 10.0 + *depth as f32 * 12.0 + 12.0;
                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(x + 4.0, rect.center().y),
                                    Vec2::splat(11.0),
                                ),
                                Icon::File,
                                p.text3,
                            );
                            let text_rect = Rect::from_min_max(
                                Pos2::new(x + 14.0, rect.top()),
                                Pos2::new(rect.right() - 6.0, rect.bottom()),
                            );
                            text_clipped(
                                ui.painter(),
                                text_rect,
                                name,
                                FontId::proportional(12.0),
                                if selected { p.text_bright } else { p.text2 },
                                Align2::LEFT_CENTER,
                            );
                            if response.clicked() {
                                select_file = Some((stash.clone(), path.clone()));
                            }
                            if response.double_clicked() {
                                open_diff = Some((stash.clone(), path.clone()));
                            }
                            if response.secondary_clicked() {
                                rt.stash_selected = Some(stash.clone());
                                rt.stash_file_sel = Some(path.clone());
                                rt.open_menu(
                                    crate::runtime::MenuKind::StashFileMenu {
                                        project: project_index,
                                    },
                                    rect,
                                    Some(stash_menu_payload(stash, "f", path)),
                                );
                            }
                        }
                    }
                }
            });
    });

    if let Some(key) = toggle {
        // 贮藏条目自身：切换展开集合（键即贮藏名）；树根/目录：切换折叠集合。
        if rt.stash_entries.iter().any(|entry| entry.name == key) {
            if !rt.stash_expanded.remove(&key) {
                rt.stash_expanded.insert(key);
            }
        } else if !rt.stash_collapsed.remove(&key) {
            rt.stash_collapsed.insert(key);
        }
    }
    if let Some(name) = select_stash {
        if rt.stash_selected.as_deref() != Some(name.as_str()) {
            rt.stash_selected = Some(name);
            rt.stash_file_sel = None;
        }
    }
    if let Some((stash, path)) = select_file {
        if rt.stash_selected.as_deref() != Some(stash.as_str()) {
            rt.stash_selected = Some(stash);
        }
        rt.stash_file_sel = Some(path);
    }
    if let Some((stash, path)) = open_diff {
        model.queue(Command::OpenStashDiff { stash, path });
    }

    // ----- 底部操作栏（PyCharm：Apply / Pop） -----
    ui.painter().hline(
        bar.x_range(),
        bar.top() + 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    let apply_rect = Rect::from_min_size(
        Pos2::new(bar.left() + 10.0, bar.top() + 5.0),
        Vec2::new(74.0, 24.0),
    );
    let pop_rect = Rect::from_min_size(
        Pos2::new(apply_rect.right() + 8.0, bar.top() + 5.0),
        Vec2::new(64.0, 24.0),
    );
    let enabled = rt.stash_selected.is_some();
    if primary_button(
        ui,
        apply_rect,
        crate::tr!(model, "Apply", "应用"),
        None,
        enabled,
        theme,
    )
    .clicked()
    {
        if let Some(stash) = rt.stash_selected.clone() {
            model.queue(Command::StashApply(stash));
        }
    }
    if secondary_button_enabled(
        ui,
        pop_rect,
        crate::tr!(model, "Pop", "弹出"),
        None,
        enabled,
        theme,
    )
    .clicked()
    {
        if let Some(stash) = rt.stash_selected.clone() {
            model.queue(Command::StashPop(stash));
        }
    }
}
