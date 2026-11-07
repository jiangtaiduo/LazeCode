use std::collections::{HashMap, HashSet};

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2};
use lazecode_core::{AppModel, Command, PanelTab, ProjectState, SidebarView, ToastKind};
use lazecode_fs::tree::{visible_rows, NodeKind, VisibleRow};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::UiRuntime;
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
        FontId::proportional(12.5),
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

    let foot = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - FOOT_H), rect.max);
    let list = Rect::from_min_max(
        Pos2::new(rect.left(), head.bottom()),
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
            .font(FontId::proportional(12.5)),
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
                    FontId::proportional(13.0),
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
                        FontId::monospace(11.0),
                        match letter {
                            'U' => p.green,
                            'D' => p.rose,
                            'A' => p.green,
                            _ => p.amber,
                        },
                    );
                }
                if response.secondary_clicked() {
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
        FontId::proportional(11.5),
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
        FontId::proportional(11.5),
        if running > 0 { p.running } else { p.text3 },
    );
}

/// 源控列表文件行。返回 (勾选翻转, 单击, 双击, 右键, hover-Rollback 点击)。
/// 选中态/修饰键判定由调用方处理（需要跨行的 Shift 范围信息）。
fn change_row(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    change: &lazecode_git::Change,
    checked: bool,
    selected: bool,
) -> (bool, bool, bool, bool, bool) {
    let p = theme.palette;
    let response = ui.interact(
        rect,
        ui.scope_id().with(("change", change.path.clone())),
        Sense::click(),
    );
    if response.hovered() || selected {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(4),
            if selected { p.selection } else { p.hover },
        );
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
    let name_color = if change.untracked { p.orange } else { p.text };
    ui.painter().text(
        Pos2::new(text_left, rect.center().y - 5.0),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(13.0),
        name_color,
    );
    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(text_left, rect.center().y + 2.0),
            Pos2::new(rect.right() - 108.0, rect.bottom()),
        ),
        &dir,
        FontId::proportional(11.0),
        p.text3,
        Align2::LEFT_CENTER,
    );
    let letter = change.letter();
    let del = if change.deletions > 0 {
        Some(format!("\u{2212}{}", change.deletions))
    } else {
        None
    };
    let del_width = del
        .as_ref()
        .map(|text| crate::widgets::text_width(ui.painter(), text, FontId::monospace(10.5)))
        .unwrap_or(0.0);
    let add_right = rect.right() - 24.0 - if del.is_some() { del_width + 6.0 } else { 0.0 };
    ui.painter().text(
        Pos2::new(add_right, rect.center().y),
        Align2::RIGHT_CENTER,
        format!("+{}", change.additions),
        FontId::monospace(10.5),
        p.green,
    );
    if let Some(del) = del {
        ui.painter().text(
            Pos2::new(rect.right() - 24.0, rect.center().y),
            Align2::RIGHT_CENTER,
            del,
            FontId::monospace(10.5),
            p.rose,
        );
    }
    // 行尾：hover 时把状态字母换成 Rollback 小按钮（稳定版 hover 回滚的
    // 等价物；行宽有限，复用字母位避免布局挤压）。未跟踪文件没有可回滚
    // 的基线，与稳定版一致不显示。
    let rollback_rect = Rect::from_center_size(
        Pos2::new(rect.right() - 8.0, rect.center().y),
        Vec2::splat(16.0),
    );
    let mut discard_clicked = false;
    if response.hovered() && !change.untracked {
        if icon_button(ui, rollback_rect, Icon::Undo, false, theme).clicked() {
            discard_clicked = true;
        }
    } else {
        ui.painter().text(
            Pos2::new(rect.right() - 8.0, rect.center().y),
            Align2::CENTER_CENTER,
            letter.to_string(),
            FontId::monospace(11.0),
            match letter {
                'U' => p.green,
                'D' => p.rose,
                'A' => p.green,
                _ => p.amber,
            },
        );
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
        &[
            (Icon::Refresh, "refresh"),
            (Icon::GitLog, "log"),
            (Icon::CollapseAll, "collapse"),
            (Icon::ExpandAll, "expand"),
        ],
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
            // Expand / Collapse All（对齐稳定版）：作用于分组头 + 冲突子组。
            2 => {
                if let Some(project) = model.active_project_mut() {
                    project.collapsed_lists.clear();
                    project.collapsed_lists = all_list_keys(project);
                }
            }
            3 => {
                if let Some(project) = model.active_project_mut() {
                    project.collapsed_lists.clear();
                }
            }
            _ => {}
        }
    }

    let project = model.active_project().unwrap();
    let changes = project.changes().to_vec();
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
    let collapsed_lists: HashSet<String> = project.collapsed_lists.clone();
    let selection: HashSet<String> = project.change_selection.clone();

    let dock_height = if changes.is_empty() { 0.0 } else { 196.0 };
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
        FontId::proportional(12.0),
        p.text3,
    );
    ui.painter().text(
        Pos2::new(summary.right(), summary.center().y),
        Align2::RIGHT_CENTER,
        format!("+{total_add} \u{2212}{total_del}"),
        FontId::monospace(11.0),
        p.text3,
    );

    // ----- 工具按钮行（流式布局：窄侧栏/中文标签下自动换行，避免裁剪） -----
    let mut button_x = list.left() + 12.0;
    let mut button_y = summary.bottom() + 3.0;
    let place = |button_x: &mut f32, button_y: &mut f32, width: f32| -> Rect {
        if *button_x + width > list.right() - 12.0 {
            *button_x = list.left() + 12.0;
            *button_y += 25.0;
        }
        let rect = Rect::from_min_size(Pos2::new(*button_x, *button_y), Vec2::new(width, 21.0));
        *button_x += width + 4.0;
        rect
    };
    let stage_rect = place(&mut button_x, &mut button_y, 76.0);
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
    let unstage_rect = place(&mut button_x, &mut button_y, 88.0);
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
    let stash_rect = place(&mut button_x, &mut button_y, 56.0);
    if crate::widgets::secondary_button(
        ui,
        stash_rect,
        crate::tr!(model, "Stash", "贮藏"),
        None,
        theme,
    )
    .clicked()
    {
        // 对齐稳定版 StashDialog：先弹说明输入 + 未跟踪选项，而不是直接 push。
        let has_untracked = changes.iter().any(|change| change.untracked);
        rt.dialog = crate::runtime::DialogState::StashCreate {
            message: String::new(),
            include_untracked: has_untracked,
            error: None,
        };
    }
    let stashes_rect = place(&mut button_x, &mut button_y, 60.0);
    if crate::widgets::secondary_button(
        ui,
        stashes_rect,
        crate::tr!(model, "List…", "列表…"),
        None,
        theme,
    )
    .clicked()
    {
        rt.dialog = crate::runtime::DialogState::StashManager {
            stashes: Vec::new(),
            selected: 0,
            error: None,
            loaded: false,
        };
    }
    // Shelve：只架藏勾选文件；默认名单文件取文件名、多文件取默认列表名
    // （稳定版规则：单文件=文件名，多文件='Changes'，此处默认列表名为
    // Default）。无勾选时禁用。
    let checked_paths: Vec<String> = changes
        .iter()
        .filter(|change| checked.contains(&change.path))
        .map(|change| change.path.clone())
        .collect();
    let shelf_default_name = if checked_paths.len() == 1 {
        checked_paths[0]
            .rsplit('/')
            .next()
            .unwrap_or("Default")
            .to_string()
    } else {
        "Default".to_string()
    };
    let shelf_rect = place(&mut button_x, &mut button_y, 60.0);
    if secondary_button_enabled(
        ui,
        shelf_rect,
        crate::tr!(model, "Shelve", "架藏"),
        None,
        !checked_paths.is_empty(),
        theme,
    )
    .clicked()
    {
        rt.dialog = crate::runtime::DialogState::NewShelf {
            value: shelf_default_name,
            error: None,
            paths: checked_paths.clone(),
        };
    }
    let shelves_rect = place(&mut button_x, &mut button_y, 72.0);
    if crate::widgets::secondary_button(
        ui,
        shelves_rect,
        crate::tr!(model, "Shelves…", "架藏库…"),
        None,
        theme,
    )
    .clicked()
    {
        rt.dialog = crate::runtime::DialogState::ShelfManager {
            shelves: Vec::new(),
            selected: 0,
            error: None,
            loaded: false,
        };
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
    groups.push(ScGroup {
        name: default_name.clone(),
        key: default_name.clone(),
        files: Vec::new(),
        conflicts: Vec::new(),
        unversioned: false,
    });
    let mut unversioned: Vec<usize> = Vec::new();
    for (index, change) in changes.iter().enumerate() {
        if change.untracked {
            // 未跟踪文件不参与 changelist 分组，固定进 Unversioned Files
            // （对齐稳定版语义）。
            unversioned.push(index);
            continue;
        }
        let assigned = lists.list_of(&change.path);
        if assigned == default_name || assigned.is_empty() {
            groups[0].files.push(index);
        } else if let Some(group) = groups
            .iter_mut()
            .find(|group| group.name == assigned && !group.unversioned)
        {
            group.files.push(index);
        } else {
            groups.push(ScGroup {
                name: assigned.clone(),
                key: assigned,
                files: vec![index],
                conflicts: Vec::new(),
                unversioned: false,
            });
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
    groups.retain(|group| !group.files.is_empty() || !group.conflicts.is_empty());
    // Default 组必须保持在首位（即便暂空也由下方空态兜底）。
    if !unversioned.is_empty() {
        groups.push(ScGroup {
            name: crate::tr!(model, "Unversioned Files", "未版本控制的文件").to_string(),
            key: "__unversioned__".to_string(),
            files: unversioned,
            conflicts: Vec::new(),
            unversioned: true,
        });
    }

    // ----- 扁平行模型（虚拟化 + Shift 范围选择基准顺序） -----
    #[derive(Clone, Copy)]
    enum RowKind {
        Header,
        ConflictHeader,
        File,
    }
    struct ScRow {
        group: usize,
        kind: RowKind,
        change: Option<usize>,
    }
    let mut rows: Vec<ScRow> = Vec::new();
    let mut flat_order: Vec<usize> = Vec::new();
    for (group_index, group) in groups.iter().enumerate() {
        rows.push(ScRow {
            group: group_index,
            kind: RowKind::Header,
            change: None,
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
            });
            if !collapsed_lists.contains(&conflict_key) {
                for index in &group.conflicts {
                    rows.push(ScRow {
                        group: group_index,
                        kind: RowKind::File,
                        change: Some(*index),
                    });
                    flat_order.push(*index);
                }
            }
        }
        for index in &group.files {
            rows.push(ScRow {
                group: group_index,
                kind: RowKind::File,
                change: Some(*index),
            });
            flat_order.push(*index);
        }
    }

    let header_bottom = shelves_rect.bottom() + 4.0;
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
            .show_rows(ui, 38.0, rows.len(), |ui, range| {
                for row_index in range {
                    let Some(row) = rows.get(row_index) else {
                        continue;
                    };
                    let group = &groups[row.group];
                    match row.kind {
                        RowKind::Header => {
                            let (rect, response) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 38.0),
                                Sense::click(),
                            );
                            // 组头右键 payload = 组名 + 全组文件路径（稳定版
                            // 组头菜单对整组文件提供与文件行一致的操作）。
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
                            if checkbox_tri(ui, checkbox_rect, state, theme).clicked() {
                                paths.sort();
                                group_toggle = Some((paths, state != TriState::Unchecked));
                            }
                            let expanded = !collapsed_lists.contains(&group.key);
                            let chevron = if expanded {
                                Icon::ChevronDown
                            } else {
                                Icon::ChevronRight
                            };
                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(rect.left() + 10.0, rect.center().y),
                                    Vec2::splat(12.0),
                                ),
                                chevron,
                                p.text3,
                            );
                            let name_color = if group.unversioned { p.orange } else { p.text2 };
                            ui.painter().text(
                                Pos2::new(rect.left() + 40.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                &group.name,
                                FontId::proportional(10.5),
                                name_color,
                            );
                            let total = group.files.len() + group.conflicts.len();
                            ui.painter().text(
                                Pos2::new(rect.right() - 12.0, rect.center().y),
                                Align2::RIGHT_CENTER,
                                total.to_string(),
                                FontId::monospace(9.0),
                                p.text3,
                            );
                            // 单击组头（除勾选框）切换折叠。
                            if response.clicked() {
                                let project = model.active_project_mut().unwrap();
                                if !project.collapsed_lists.remove(&group.key) {
                                    project.collapsed_lists.insert(group.key.clone());
                                }
                            }
                        }
                        RowKind::ConflictHeader => {
                            let conflict_key = format!("__conflicts__::{}", group.key);
                            let (rect, response) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 38.0),
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
                            if checkbox_tri(ui, checkbox_rect, state, theme).clicked() {
                                paths.sort();
                                group_toggle = Some((paths, state != TriState::Unchecked));
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
                                FontId::proportional(10.5),
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
                                FontId::proportional(10.0),
                                p.accent,
                            );
                            if response.clicked() {
                                let project = model.active_project_mut().unwrap();
                                if !project.collapsed_lists.remove(&conflict_key) {
                                    project.collapsed_lists.insert(conflict_key);
                                }
                            }
                        }
                        RowKind::File => {
                            let change_index = row.change.expect("File row carries a change");
                            let change = &changes[change_index];
                            let (rect, _) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 38.0),
                                Sense::hover(),
                            );
                            let rect = rect.shrink2(Vec2::new(4.0, 1.0));
                            let path = change.path.clone();
                            let is_checked = checked.contains(&path);
                            let is_selected = selection.contains(&path);
                            let (new_checked, clicked, double_clicked, secondary, discard) =
                                change_row(ui, theme, rect, change, is_checked, is_selected);
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
    amend: bool,
    checked: usize,
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
            .font(FontId::proportional(12.5))
            .desired_width(editor_rect.width()),
    );
    if response.changed() {
        model.active_project_mut().unwrap().commit_message = value;
    }

    let amend_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 12.0, input.bottom() + 8.0),
        Vec2::new(70.0, 20.0),
    );
    // 提交信息历史入口（对齐稳定版 useCommitMessageHistory 下拉）。
    let history_rect = Rect::from_center_size(
        Pos2::new(amend_rect.left() + 10.0, amend_rect.center().y),
        Vec2::splat(20.0),
    );
    if icon_button(ui, history_rect, Icon::Clock, false, theme).clicked() {
        let anchor = Rect::from_min_size(
            Pos2::new(history_rect.left(), amend_rect.bottom()),
            Vec2::splat(1.0),
        );
        rt.toggle_menu(
            crate::runtime::MenuKind::CommitHistory {
                project: model.active_project.unwrap_or(0),
            },
            anchor,
            None,
        );
    }
    let mut amend_value = amend;
    let checkbox_rect = Rect::from_center_size(
        Pos2::new(amend_rect.left() + 28.0, amend_rect.center().y),
        Vec2::splat(16.0),
    );
    if checkbox(ui, checkbox_rect, amend_value, theme).clicked() {
        amend_value = !amend_value;
        model.active_project_mut().unwrap().amend = amend_value;
    }
    ui.painter().text(
        Pos2::new(amend_rect.left() + 40.0, amend_rect.center().y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Amend", "修订"),
        FontId::proportional(12.0),
        p.text3,
    );

    ui.painter().text(
        Pos2::new(rect.right() - 12.0, amend_rect.center().y),
        Align2::RIGHT_CENTER,
        if model.settings.language.is_zh() {
            format!("{} / {} 个文件", checked, changes.len())
        } else {
            format!("{checked} / {} files", changes.len())
        },
        FontId::proportional(11.5),
        p.text3,
    );

    // Commit + Commit & Push 两按钮（对齐稳定版 commit-bottom-box）。
    let push_w = 96.0f32;
    let commit_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 12.0, amend_rect.bottom() + 8.0),
        Vec2::new(rect.width() - 24.0 - push_w - 6.0, 30.0),
    );
    let push_rect = Rect::from_min_size(
        Pos2::new(commit_rect.right() + 6.0, commit_rect.top()),
        Vec2::new(push_w, 30.0),
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
    let response = primary_button(ui, commit_rect, &label, Some(Icon::Commit), enabled, theme);
    if response.clicked() && enabled {
        model.queue(Command::Commit);
    }
    let push_label = crate::tr!(model, "Commit & Push", "提交并推送").to_string();
    let push_response = secondary_button_enabled(ui, push_rect, &push_label, None, enabled, theme);
    if push_response.clicked() && enabled {
        // 提交成功后由 JobResult 消费点置位 UiState::open_push_dialog，
        // shell 随后打开 Push 对话框（对齐稳定版 Commit and Push...）。
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
        &[(Icon::Spark, "agent"), (Icon::Plus, "new")],
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
