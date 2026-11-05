use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2};
use lazecode_core::{AppModel, Command, PanelTab, ProjectState, SidebarView, TabKind, ToastKind};
use lazecode_fs::tree::{visible_rows, NodeKind, VisibleRow};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::UiRuntime;
use crate::theme::{Theme, SMALL, TINY};
use crate::widgets::{checkbox, icon_button, live_dot, primary_button, text_clipped};

const HEAD_H: f32 = 36.0;
const BAR_H: f32 = 32.0;
const FOOT_H: f32 = 25.0;
const ROW_H: f32 = 23.0;

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.sidebar);
    let Some(_) = model.active_project() else {
        let text = crate::tr!(model, "No project open", "未打开项目");
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(11.5),
            p.text3,
        );
        return;
    };
    match model.ui.sidebar_view {
        SidebarView::Files => explorer(ui, model, rt, theme, rect),
        SidebarView::Git => source_control(ui, model, rt, theme, rect),
        SidebarView::Consoles => consoles(ui, model, rt, theme, rect),
    }
}

fn side_head(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    title: &str,
    actions: &[(Icon, &'static str)],
) -> Option<usize> {
    let p = theme.palette;
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(11.0),
        p.text3,
    );
    let mut right = rect.right() - 8.0;
    let mut clicked = None;
    for (index, (icon, id)) in actions.iter().enumerate().rev() {
        let button =
            Rect::from_center_size(Pos2::new(right - 11.0, rect.center().y), Vec2::splat(24.0));
        right -= 25.0;
        if icon_button(ui, button, *icon, false, theme).clicked() {
            clicked = Some(index);
        }
        let _ = id;
    }
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    clicked
}

fn project_bar(ui: &mut Ui, model: &AppModel, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    let project = model.active_project().unwrap();
    let changes = project.changes().len();
    let running = project
        .worktrees
        .iter()
        .flat_map(|worktree| worktree.terminals.iter())
        .filter(|terminal| terminal.session.is_running())
        .count();
    let tone = Color32::from_rgb(project.tone.0, project.tone.1, project.tone.2);
    ui.painter()
        .rect_filled(rect, 0.0, p.panel2.gamma_multiply(0.55));
    let icon_rect = Rect::from_center_size(
        Pos2::new(rect.left() + 17.0, rect.center().y),
        Vec2::splat(15.0),
    );
    paint_icon(ui.painter(), icon_rect, Icon::Worktree, tone);
    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(rect.left() + 30.0, rect.top()),
            Pos2::new(rect.left() + 120.0, rect.bottom()),
        ),
        &project.name,
        FontId::proportional(11.5),
        p.text2,
        Align2::LEFT_CENTER,
    );
    let count_rect = Rect::from_center_size(
        Pos2::new(rect.left() + 132.0, rect.center().y),
        Vec2::new(22.0, 15.0),
    );
    ui.painter().rect_filled(
        count_rect,
        egui::CornerRadius::same(7),
        if changes > 0 { p.accent_soft } else { p.panel },
    );
    ui.painter().text(
        count_rect.center(),
        Align2::CENTER_CENTER,
        changes.to_string(),
        FontId::monospace(9.0),
        if changes > 0 { p.accent } else { p.text3 },
    );
    if running > 0 {
        live_dot(
            ui.painter(),
            Pos2::new(rect.right() - 62.0, rect.center().y),
            true,
            p.running,
        );
        ui.painter().text(
            Pos2::new(rect.right() - 53.0, rect.center().y),
            Align2::LEFT_CENTER,
            running.to_string(),
            FontId::monospace(9.5),
            p.running,
        );
    }
    let branch_rect = Rect::from_min_max(
        Pos2::new(rect.right() - 160.0, rect.top() + 4.0),
        Pos2::new(rect.right() - 8.0, rect.bottom() - 4.0),
    );
    text_clipped(
        ui.painter(),
        branch_rect,
        &project.branch(),
        FontId::proportional(10.5),
        p.text3,
        Align2::RIGHT_CENTER,
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );
}

fn file_tone(path: &str, theme: &Theme) -> Color32 {
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
    if let Some(action) = side_head(
        ui,
        theme,
        head,
        crate::tr!(model, "Explorer", "资源管理器"),
        &[(Icon::ChevronUp, "collapse"), (Icon::Refresh, "refresh")],
    ) {
        if let Some(project) = model.active_project_mut() {
            match action {
                0 => project.expanded.clear(),
                1 => {
                    project.tree = lazecode_fs::tree::load_tree(&project.root);
                    model.queue(Command::ReloadGit);
                }
                _ => {}
            }
        }
    }
    let bar = Rect::from_min_size(
        Pos2::new(rect.left(), head.bottom()),
        Vec2::new(rect.width(), BAR_H),
    );
    project_bar(ui, model, theme, bar);

    let foot = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - FOOT_H), rect.max);
    let list = Rect::from_min_max(
        Pos2::new(rect.left(), bar.bottom()),
        Pos2::new(rect.right(), foot.top()),
    );
    ui.painter().rect_filled(list, 0.0, p.sidebar);
    draw_tree(ui, model, rt, theme, list);
    draw_tree_foot(ui, model, theme, foot);
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
        if filter.is_empty() {
            (visible_rows(&project.tree, &project.expanded), changes)
        } else {
            let mut rows = Vec::new();
            collect_matches(&project.tree.nodes, &filter, 0, &mut rows);
            (rows, changes)
        }
    };

    let input_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 10.0, rect.top() + 8.0),
        Vec2::new(rect.width() - 20.0, 26.0),
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
            .font(FontId::proportional(11.0)),
    );
    if response.changed() {
        model.ui.tree_filter = filter_value;
    }
    crate::widgets::kbd(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(input_rect.right() - 14.0, input_rect.center().y),
            Vec2::new(17.0, 16.0),
        ),
        "/",
        theme,
    );

    let list_rect = Rect::from_min_max(Pos2::new(rect.left(), input_rect.bottom() + 6.0), rect.max);
    let active_tab = model.projects[active_index].active_tab;
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
    let reveal_offset = if rt.last_revealed_tab != active_tab {
        active_rel
            .as_ref()
            .and_then(|rel| rows.iter().position(|row| &row.path == rel))
            .map(|position| (position as f32 * ROW_H - 80.0).max(0.0))
    } else {
        None
    };
    if reveal_offset.is_some() {
        rt.last_revealed_tab = active_tab;
    }
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
                let selected = active_rel.as_ref().is_some_and(|rel| rel == &row.path);
                if selected {
                    ui.painter()
                        .rect_filled(draw_rect, egui::CornerRadius::same(4), p.active);
                } else if response.hovered() {
                    ui.painter()
                        .rect_filled(draw_rect, egui::CornerRadius::same(4), p.hover);
                }
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
                        p.text3,
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
                        p.text3,
                    ),
                    NodeKind::File => paint_icon(
                        ui.painter(),
                        icon_rect,
                        Icon::File,
                        file_tone(&row.name, theme),
                    ),
                }
                let letter = changes.get(row.path.to_string_lossy().as_ref()).copied();
                let text_rect = Rect::from_min_max(
                    Pos2::new(draw_rect.left() + pad + 34.0, draw_rect.top()),
                    Pos2::new(
                        draw_rect.right() - if letter.is_some() { 30.0 } else { 8.0 },
                        draw_rect.bottom(),
                    ),
                );
                text_clipped(
                    ui.painter(),
                    text_rect,
                    &row.name,
                    FontId::proportional(11.5),
                    if row.kind == NodeKind::Folder {
                        p.text2
                    } else {
                        p.text
                    },
                    Align2::LEFT_CENTER,
                );
                if let Some(letter) = letter {
                    ui.painter().text(
                        Pos2::new(draw_rect.right() - 14.0, draw_rect.center().y),
                        Align2::CENTER_CENTER,
                        letter.to_string(),
                        FontId::monospace(9.5),
                        match letter {
                            'U' => p.green,
                            'D' => p.rose,
                            'A' => p.green,
                            _ => p.amber,
                        },
                    );
                }
                if response.clicked() {
                    match row.kind {
                        NodeKind::Folder => {
                            let project = &mut model.projects[active_index];
                            if project.expanded.contains(&row.path) {
                                project.expanded.remove(&row.path);
                            } else {
                                project.expanded.insert(row.path.clone());
                            }
                        }
                        NodeKind::File => {
                            let settings = model.settings.clone();
                            let path = model.projects[active_index].root.join(&row.path);
                            if let Err(err) =
                                model.projects[active_index].open_file(path, &settings)
                            {
                                model.toast(err, ToastKind::Error);
                            }
                            rt.set_editor_focus(
                                model.projects[active_index].active_tab.unwrap_or_default(),
                            );
                        }
                    }
                }
            }
        });
    });
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

fn draw_tree_foot(ui: &mut Ui, model: &mut AppModel, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    let project = model.active_project().unwrap();
    ui.painter().rect_filled(rect, 0.0, p.sidebar);
    ui.painter().hline(
        rect.x_range(),
        rect.top() + 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(rect.left() + 15.0, rect.center().y),
            Vec2::splat(12.0),
        ),
        Icon::Git,
        p.text3,
    );
    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(rect.left() + 25.0, rect.top()),
            Pos2::new(rect.right() - 70.0, rect.bottom()),
        ),
        &if project.tree.truncated {
            format!(
                "{} · {}",
                project.branch(),
                crate::tr!(model, "truncated", "已截断")
            )
        } else {
            project.branch()
        },
        FontId::proportional(10.5),
        p.text3,
        Align2::LEFT_CENTER,
    );
    let running = project
        .worktrees
        .iter()
        .flat_map(|worktree| worktree.terminals.iter())
        .filter(|terminal| terminal.session.is_running())
        .count();
    ui.painter().text(
        Pos2::new(rect.right() - 10.0, rect.center().y),
        Align2::RIGHT_CENTER,
        if running > 0 {
            if model.settings.language.is_zh() {
                format!("{running} 个运行中")
            } else {
                format!("{running} running")
            }
        } else {
            crate::tr!(model, "idle", "空闲").to_string()
        },
        FontId::proportional(10.0),
        if running > 0 { p.running } else { p.text3 },
    );
}

fn change_row(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    change: &lazecode_git::Change,
    checked: bool,
) -> (bool, bool) {
    let p = theme.palette;
    let response = ui.interact(
        rect,
        ui.scope_id().with(("change", change.path.clone())),
        Sense::click(),
    );
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(4), p.hover);
    }
    let checkbox_rect = Rect::from_center_size(
        Pos2::new(rect.left() + 16.0, rect.center().y),
        Vec2::splat(16.0),
    );
    let mut checked = checked;
    let toggle = checkbox(ui, checkbox_rect, checked, theme);
    if toggle.clicked() {
        checked = !checked;
    }
    let name = change.path.rsplit('/').next().unwrap_or(&change.path);
    let dir = {
        let path = change.path.trim_end_matches(name);
        path.trim_end_matches('/').to_string()
    };
    let text_left = rect.left() + 30.0;
    ui.painter().text(
        Pos2::new(text_left, rect.center().y - 5.0),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(11.5),
        p.text,
    );
    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(text_left, rect.center().y + 2.0),
            Pos2::new(rect.right() - 108.0, rect.bottom()),
        ),
        &dir,
        FontId::proportional(9.0),
        p.text3,
        Align2::LEFT_CENTER,
    );
    let letter = change.letter();
    let del = if change.deletions > 0 {
        Some(format!("−{}", change.deletions))
    } else {
        None
    };
    let del_width = del
        .as_ref()
        .map(|text| crate::widgets::text_width(ui.painter(), text, FontId::monospace(8.5)))
        .unwrap_or(0.0);
    let add_right = rect.right() - 24.0 - if del.is_some() { del_width + 6.0 } else { 0.0 };
    ui.painter().text(
        Pos2::new(add_right, rect.center().y),
        Align2::RIGHT_CENTER,
        format!("+{}", change.additions),
        FontId::monospace(8.5),
        p.green,
    );
    if let Some(del) = del {
        ui.painter().text(
            Pos2::new(rect.right() - 24.0, rect.center().y),
            Align2::RIGHT_CENTER,
            del,
            FontId::monospace(8.5),
            p.rose,
        );
    }
    ui.painter().text(
        Pos2::new(rect.right() - 8.0, rect.center().y),
        Align2::CENTER_CENTER,
        letter.to_string(),
        FontId::monospace(10.0),
        match letter {
            'U' => p.green,
            'D' => p.rose,
            'A' => p.green,
            _ => p.amber,
        },
    );
    (checked, response.double_clicked())
}

fn source_control(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
) {
    let p = theme.palette;
    let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), HEAD_H));
    if let Some(action) = side_head(
        ui,
        theme,
        head,
        crate::tr!(model, "Source Control", "版本控制"),
        &[(Icon::Refresh, "refresh"), (Icon::GitLog, "log")],
    ) {
        match action {
            0 => {
                model.queue(Command::ReloadGit);
                model.queue(Command::ReloadTree);
            }
            1 => {
                model.ui.panel_visible = true;
                model.ui.panel_tab = PanelTab::Git;
                model.queue(Command::RefreshLog);
            }
            _ => {}
        }
    }
    let bar = Rect::from_min_size(
        Pos2::new(rect.left(), head.bottom()),
        Vec2::new(rect.width(), BAR_H),
    );
    project_bar(ui, model, theme, bar);

    let project = model.active_project().unwrap();
    let changes = project.changes().to_vec();
    let last = project.git.as_ref().and_then(|git| git.head.clone());
    let can_git = project.git.is_some();
    let git_error = project.git_error.clone();
    let commit_message = project.commit_message.clone();
    let amend = project.amend;
    let unchecked: HashSet<String> = project.unchecked_paths.clone();

    let dock_height = if changes.is_empty() { 0.0 } else { 196.0 };
    let dock = Rect::from_min_max(
        Pos2::new(rect.left(), rect.bottom() - dock_height),
        rect.max,
    );
    let list = Rect::from_min_max(
        Pos2::new(rect.left(), bar.bottom()),
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

    if changes.is_empty() {
        draw_clean_state(ui, theme, list, last.as_ref(), model.settings.language);
    } else {
        let total_add: u32 = changes.iter().map(|change| change.additions).sum();
        let total_del: u32 = changes.iter().map(|change| change.deletions).sum();
        let summary = Rect::from_min_size(
            Pos2::new(list.left() + 12.0, list.top() + 8.0),
            Vec2::new(list.width() - 24.0, 18.0),
        );
        ui.painter().text(
            Pos2::new(summary.left(), summary.center().y),
            Align2::LEFT_CENTER,
            if model.settings.language.is_zh() {
                format!(
                    "{} 个文件 · {}",
                    changes.len(),
                    model.active_project().unwrap().name
                )
            } else {
                format!(
                    "{} files in {}",
                    changes.len(),
                    model.active_project().unwrap().name
                )
            },
            FontId::proportional(10.5),
            p.text3,
        );
        ui.painter().text(
            Pos2::new(summary.right(), summary.center().y),
            Align2::RIGHT_CENTER,
            format!("+{total_add} −{total_del}"),
            FontId::monospace(9.0),
            p.text3,
        );
        // Stage / unstage all.
        let stage_rect = Rect::from_min_size(
            Pos2::new(list.left() + 12.0, summary.bottom() + 3.0),
            Vec2::new(76.0, 21.0),
        );
        if crate::widgets::secondary_button(
            ui,
            stage_rect,
            crate::tr!(model, "Stage all", "全部暂存"),
            None,
            theme,
        )
        .clicked()
        {
            let paths: Vec<String> = changes.iter().map(|change| change.path.clone()).collect();
            model.queue(Command::Stage(paths));
        }
        let unstage_rect = Rect::from_min_size(
            Pos2::new(stage_rect.right() + 6.0, stage_rect.top()),
            Vec2::new(84.0, 21.0),
        );
        if crate::widgets::secondary_button(
            ui,
            unstage_rect,
            crate::tr!(model, "Unstage all", "全部取消暂存"),
            None,
            theme,
        )
        .clicked()
        {
            let paths: Vec<String> = changes.iter().map(|change| change.path.clone()).collect();
            model.queue(Command::Unstage(paths));
        }
        let header_bottom = stage_rect.bottom() + 4.0;
        let rows_rect = Rect::from_min_max(Pos2::new(list.left(), header_bottom), list.max);
        let mut toggled: Option<(String, bool)> = None;
        let mut open_diff: Option<String> = None;
        super::app_ui::region(ui, rows_rect, "sc-rows", |ui| {
            egui::ScrollArea::vertical()
                .id_salt("changes")
                .auto_shrink([false, false])
                .show_rows(ui, 38.0, changes.len(), |ui, range| {
                    for index in range {
                        let change = &changes[index];
                        let (rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 38.0),
                            Sense::hover(),
                        );
                        let checked = !unchecked.contains(&change.path);
                        let (new_checked, open) = change_row(
                            ui,
                            theme,
                            rect.shrink2(Vec2::new(4.0, 1.0)),
                            change,
                            checked,
                        );
                        if new_checked != checked {
                            toggled = Some((change.path.clone(), new_checked));
                        }
                        if open {
                            open_diff = Some(change.path.clone());
                        }
                    }
                });
        });
        if let Some((path, checked)) = toggled {
            let project = model.active_project_mut().unwrap();
            if checked {
                project.unchecked_paths.remove(&path);
            } else {
                project.unchecked_paths.insert(path);
            }
        }
        if let Some(path) = open_diff {
            model.queue(Command::OpenDiff(path));
        }
    }

    if !changes.is_empty() {
        draw_commit_dock(
            ui,
            model,
            theme,
            dock,
            &changes,
            &commit_message,
            amend,
            &unchecked,
            can_git,
        );
    }

    let _ = (rt, SMALL, TINY);
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
        FontId::proportional(12.0),
        p.text2,
    );
    ui.painter().text(
        Pos2::new(center.x, center.y + 50.0),
        Align2::CENTER_CENTER,
        crate::i18n::t(lang, "Nothing to commit", "没有可提交内容"),
        FontId::proportional(10.5),
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
            FontId::monospace(9.5),
            p.accent,
        );
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(card.left() + 10.0, card.top() + 26.0),
                Pos2::new(card.right() - 10.0, card.top() + 44.0),
            ),
            &last.message,
            FontId::proportional(11.0),
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
            FontId::proportional(9.5),
            p.text3,
            Align2::LEFT_CENTER,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_commit_dock(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    changes: &[lazecode_git::Change],
    message: &str,
    amend: bool,
    unchecked: &HashSet<String>,
    can_commit: bool,
) {
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.sidebar);
    ui.painter()
        .hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, p.border));
    let input = Rect::from_min_size(
        Pos2::new(rect.left() + 12.0, rect.top() + 10.0),
        Vec2::new(rect.width() - 24.0, 66.0),
    );
    ui.painter()
        .rect_filled(input, egui::CornerRadius::same(6), p.panel);
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
            .hint_text(crate::tr!(model, "Commit message", "提交信息"))
            .font(FontId::proportional(11.5))
            .desired_width(editor_rect.width()),
    );
    if response.changed() {
        model.active_project_mut().unwrap().commit_message = value;
    }

    let amend_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 12.0, input.bottom() + 8.0),
        Vec2::new(70.0, 20.0),
    );
    let mut amend_value = amend;
    let checkbox_rect = Rect::from_center_size(
        Pos2::new(amend_rect.left() + 8.0, amend_rect.center().y),
        Vec2::splat(16.0),
    );
    if checkbox(ui, checkbox_rect, amend_value, theme).clicked() {
        amend_value = !amend_value;
        model.active_project_mut().unwrap().amend = amend_value;
    }
    ui.painter().text(
        Pos2::new(amend_rect.left() + 20.0, amend_rect.center().y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Amend", "修订"),
        FontId::proportional(10.5),
        p.text3,
    );

    let checked = changes
        .iter()
        .filter(|change| !unchecked.contains(&change.path))
        .count();
    ui.painter().text(
        Pos2::new(rect.right() - 12.0, amend_rect.center().y),
        Align2::RIGHT_CENTER,
        if model.settings.language.is_zh() {
            format!("{} / {} 个文件", checked, changes.len())
        } else {
            format!("{checked} / {} files", changes.len())
        },
        FontId::proportional(10.0),
        p.text3,
    );

    let button = Rect::from_min_size(
        Pos2::new(rect.left() + 12.0, amend_rect.bottom() + 8.0),
        Vec2::new(rect.width() - 24.0, 30.0),
    );
    let enabled =
        can_commit && (!message.trim().is_empty() || amend_value) && (checked > 0 || amend_value);
    let label = if amend_value {
        crate::tr!(model, "Amend commit", "修订提交").to_string()
    } else {
        if model.settings.language.is_zh() {
            format!("提交 {} 个文件", checked)
        } else {
            format!("Commit {checked} files")
        }
    };
    let response = primary_button(ui, button, &label, Some(Icon::Commit), enabled, theme);
    if response.clicked() && enabled {
        model.queue(Command::Commit);
    }
    if !can_commit {
        ui.painter().text(
            Pos2::new(rect.center().x, rect.bottom() - 8.0),
            Align2::CENTER_CENTER,
            crate::tr!(model, "No Git repository", "非 Git 仓库"),
            FontId::proportional(9.5),
            p.text3,
        );
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
        &[(Icon::Spark, "agent"), (Icon::Plus, "new")],
    ) {
        match action {
            0 => {
                rt.agent_menu = Some(Rect::from_min_size(
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
                .filter(|terminal| terminal.session.is_running())
                .count();
            let label = if worktree.is_main {
                project.name.clone()
            } else {
                format!("{} · {}", project.name, worktree.name)
            };
            for (terminal_index, terminal) in worktree.terminals.iter().enumerate() {
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
                    group_start: terminal_index == 0,
                    group_running,
                });
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
                        model.projects[row.project].worktrees[row.worktree].active_terminal =
                            Some(row.terminal);
                        model.ui.panel_visible = true;
                        model.ui.panel_tab = PanelTab::Console;
                        rt.clear_terminal_cache();
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

#[allow(dead_code)]
fn _unused(_: &ProjectState, _: &PathBuf, _: TabKind) {}
