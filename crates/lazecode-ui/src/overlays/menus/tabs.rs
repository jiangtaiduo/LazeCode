//! Tree, editor-tab, terminal-tab and editor context menus.

use super::prelude::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_tree_context_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    rel_path: &str,
    is_dir: bool,
) {
    let p = theme.palette;
    let project = &model.projects[project_idx];
    let abs_path = if rel_path.is_empty() {
        project.active_root().to_path_buf()
    } else {
        project.active_root().join(rel_path)
    };
    let dir_for_new = if is_dir {
        abs_path.clone()
    } else {
        abs_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| project.active_root().to_path_buf())
    };

    let zh = model.settings.language.is_zh();
    let is_batch = model.ui.tree_selection.len() > 1
        && model
            .ui
            .tree_selection
            .contains(&std::path::PathBuf::from(rel_path));

    enum Item {
        Row(&'static str, &'static str, u8, bool, bool),
        Sub(&'static str, &'static str, &'static str),
        Sep,
    }

    let items: Vec<Item> = if is_batch {
        vec![Item::Row("Delete…", "删除…", 50, true, false)]
    } else {
        let mut list = vec![
            Item::Sub("New", "新建", "new"),
            Item::Row("Cut", "剪切", 101, false, false),
            Item::Row("Copy", "复制", 102, false, false),
            Item::Sub("Copy Path/Reference...", "复制路径/引用…", "copy-path"),
            Item::Row("Paste", "粘贴", 103, false, true),
            Item::Sep,
            Item::Row("Rename...", "重命名…", 4, false, false),
            Item::Sep,
            Item::Row("Find in Files...", "在文件中查找…", 104, false, false),
            Item::Sep,
            Item::Sub("Git", "Git", "git"),
            Item::Sep,
            Item::Row("Delete", "删除", 5, true, false),
            Item::Sep,
        ];
        if is_dir {
            list.push(Item::Row("Expand All", "全部展开", 105, false, false));
            list.push(Item::Row("Collapse All", "全部折叠", 106, false, false));
        }
        list.push(Item::Row(
            "Reload from Disk",
            "从磁盘重新加载",
            107,
            false,
            false,
        ));
        list.push(Item::Sub("Open In", "打开方式", "open-in"));
        if !is_dir {
            list.push(Item::Sep);
            list.push(Item::Row("Compare With...", "与本地比较…", 9, false, false));
        }
        list
    };

    let row_count = items.iter().filter(|i| !matches!(i, Item::Sep)).count();
    let sep_count = items.iter().filter(|i| matches!(i, Item::Sep)).count();
    let height = 12.0 + row_count as f32 * 24.0 + sep_count as f32 * 8.0;
    let width = if is_batch { 160.0 } else { 200.0 };
    let rect = popover_rect(rt, screen, menu.anchor, width, height);
    menu_frame(ui, rect, theme);

    let mut action: Option<u8> = None;
    let mut close = false;
    let mut y = rect.top() + 6.0;
    let mut hovered_sub: Option<(&'static str, Rect)> = None;

    for item in &items {
        match item {
            Item::Sep => {
                ui.painter().hline(
                    (rect.left() + 8.0)..=(rect.right() - 8.0),
                    y + 3.5,
                    Stroke::new(1.0, p.border_soft),
                );
                y += 8.0;
            }
            Item::Row(en, cn, code, danger, disabled) => {
                let row = Rect::from_min_size(
                    Pos2::new(rect.left() + 4.0, y),
                    Vec2::new(rect.width() - 8.0, 22.0),
                );
                let resp = ui.interact(row, ui.scope_id().with(("tctx", *code)), Sense::click());
                if !*disabled && resp.hovered() {
                    ui.painter()
                        .rect_filled(row, egui::CornerRadius::same(4), p.hover);
                }
                let text_col = if *disabled {
                    p.text3
                } else if *danger {
                    p.rose
                } else {
                    p.text
                };
                let label = if *code == 50 {
                    if zh {
                        format!("删除 ({} 项)", model.ui.tree_selection.len())
                    } else {
                        format!("Delete ({} items)", model.ui.tree_selection.len())
                    }
                } else {
                    (if zh { *cn } else { *en }).to_string()
                };
                ui.painter().text(
                    Pos2::new(row.left() + 10.0, row.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    FontId::proportional(12.5),
                    text_col,
                );
                if !*disabled && resp.clicked() {
                    action = Some(*code);
                }
                y += 24.0;
            }
            Item::Sub(en, cn, sub_id) => {
                let row = Rect::from_min_size(
                    Pos2::new(rect.left() + 4.0, y),
                    Vec2::new(rect.width() - 8.0, 22.0),
                );
                let resp = ui.interact(
                    row,
                    ui.scope_id().with(("tctx-sub", *sub_id)),
                    Sense::hover(),
                );
                let is_active_sub = rt.tree_submenu.as_ref().is_some_and(|(s, _)| s == *sub_id);
                if resp.hovered() || is_active_sub {
                    ui.painter()
                        .rect_filled(row, egui::CornerRadius::same(4), p.hover);
                }
                ui.painter().text(
                    Pos2::new(row.left() + 10.0, row.center().y),
                    Align2::LEFT_CENTER,
                    if zh { *cn } else { *en },
                    FontId::proportional(12.5),
                    p.text,
                );
                paint_icon(
                    ui.painter(),
                    Rect::from_center_size(
                        Pos2::new(row.right() - 12.0, row.center().y),
                        Vec2::splat(10.0),
                    ),
                    Icon::ChevronRight,
                    p.text3,
                );
                if resp.hovered() {
                    hovered_sub = Some((*sub_id, row));
                }
                y += 24.0;
            }
        }
    }

    if let Some((sub_id, sub_row)) = hovered_sub {
        rt.tree_submenu = Some((sub_id.to_string(), sub_row));
    }

    let mut sub_rect_opt: Option<Rect> = None;
    if let Some((sub_id, anchor_row)) = rt.tree_submenu.clone() {
        let sub_items: Vec<(&str, &str, u8)> = match sub_id.as_str() {
            "new" => vec![("File…", "文件…", 0), ("Directory…", "目录…", 1)],
            "copy-path" => vec![
                ("Absolute Path", "绝对路径", 2),
                ("Relative Path", "相对路径", 3),
                ("Path from Repository Root", "从仓库根目录的相对路径", 201),
            ],
            "git" => {
                if is_dir {
                    vec![("Add", "添加到暂存区", 210), ("Rollback…", "回滚…", 11)]
                } else {
                    vec![
                        ("Add", "添加到暂存区", 210),
                        ("Show Diff", "显示差异", 8),
                        ("Show History", "显示历史", 10),
                        ("Annotate with Git Blame", "显示代码追溯 (Blame)", 211),
                        ("Rollback…", "回滚…", 11),
                    ]
                }
            }
            "open-in" => vec![
                ("Finder", "访达 (Finder)", 6),
                ("Terminal", "终端 (Terminal)", 7),
            ],
            _ => Vec::new(),
        };

        if !sub_items.is_empty() {
            let sub_w = 200.0;
            let sub_h = 12.0 + sub_items.len() as f32 * 24.0;
            let sub_x = if anchor_row.right() + sub_w < screen.right() - 8.0 {
                anchor_row.right() + 2.0
            } else {
                anchor_row.left() - sub_w - 2.0
            };
            let sub_y = anchor_row
                .top()
                .clamp(screen.top() + 8.0, screen.bottom() - sub_h - 8.0);
            let sub_rect = Rect::from_min_size(Pos2::new(sub_x, sub_y), Vec2::new(sub_w, sub_h));
            sub_rect_opt = Some(sub_rect);
            rt.menu_hit_rects.push(sub_rect);
            menu_frame(ui, sub_rect, theme);

            let mut sy = sub_rect.top() + 6.0;
            for (sub_en, sub_cn, sub_code) in sub_items {
                let s_row = Rect::from_min_size(
                    Pos2::new(sub_rect.left() + 4.0, sy),
                    Vec2::new(sub_rect.width() - 8.0, 22.0),
                );
                let s_resp = ui.interact(
                    s_row,
                    ui.scope_id().with(("tsub", sub_code)),
                    Sense::click(),
                );
                if s_resp.hovered() {
                    ui.painter()
                        .rect_filled(s_row, egui::CornerRadius::same(4), p.hover);
                }
                ui.painter().text(
                    Pos2::new(s_row.left() + 10.0, s_row.center().y),
                    Align2::LEFT_CENTER,
                    if zh { sub_cn } else { sub_en },
                    FontId::proportional(12.5),
                    p.text,
                );
                if s_resp.clicked() {
                    action = Some(sub_code);
                }
                sy += 24.0;
            }
        }
    }

    if let Some(code) = action {
        match code {
            0 => {
                rt.dialog = DialogState::NewFile {
                    dir: dir_for_new,
                    value: String::new(),
                    error: None,
                };
            }
            1 => {
                rt.dialog = DialogState::NewDirectory {
                    dir: dir_for_new,
                    value: String::new(),
                    error: None,
                };
            }
            2 => {
                ui.ctx().copy_text(abs_path.to_string_lossy().to_string());
                model.toast(
                    if zh {
                        "已复制绝对路径"
                    } else {
                        "Absolute path copied"
                    },
                    ToastKind::Info,
                );
            }
            3 | 201 => {
                ui.ctx().copy_text(rel_path.to_string());
                model.toast(
                    if zh {
                        "已复制相对路径"
                    } else {
                        "Relative path copied"
                    },
                    ToastKind::Info,
                );
            }
            4 => {
                let name = abs_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                rt.dialog = DialogState::RenamePath {
                    old_path: abs_path,
                    value: name,
                    error: None,
                };
            }
            5 => {
                rt.dialog = DialogState::DeletePath {
                    path: abs_path,
                    is_dir,
                };
            }
            6 => {
                #[cfg(target_os = "macos")]
                {
                    let _ = std::process::Command::new("open")
                        .arg("-R")
                        .arg(&abs_path)
                        .spawn();
                }
                #[cfg(not(target_os = "macos"))]
                {
                    if let Some(parent) = abs_path.parent() {
                        let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
                    }
                }
            }
            7 => {
                let term_cwd = if is_dir {
                    abs_path
                } else {
                    abs_path.parent().unwrap_or(&abs_path).to_path_buf()
                };
                model.queue(Command::NewTerminalCommand {
                    title: term_cwd
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("shell")
                        .to_string(),
                    command: format!("cd {:?}", term_cwd),
                });
            }
            8 => {
                model.queue(Command::OpenDiff(rel_path.to_string()));
            }
            9 => {
                model.queue(Command::OpenCompareWithLocalDiff {
                    commit: "HEAD".to_string(),
                    path: rel_path.to_string(),
                });
            }
            10 => {
                rt.dialog = DialogState::FileHistory {
                    path: rel_path.to_string(),
                    entries: Vec::new(),
                    selected: 0,
                    loaded: false,
                };
            }
            11 => {
                let prefix = format!("{rel_path}/");
                let changes: Vec<(String, bool)> = model.projects[project_idx]
                    .active_git()
                    .map(|git| {
                        git.changes
                            .iter()
                            .map(|change| (change.path.clone(), change.untracked))
                            .collect()
                    })
                    .unwrap_or_default();
                if is_dir {
                    // `discard_file_changes` removes a directory recursively
                    // when it has no HEAD entry. Expand the directory to its
                    // changed tracked files instead, and never touch untracked
                    // content from a Rollback command.
                    let paths: Vec<String> = changes
                        .iter()
                        .filter(|(path, untracked)| {
                            !untracked
                                && (rel_path.is_empty()
                                    || path == rel_path
                                    || path.starts_with(&prefix))
                        })
                        .map(|(path, _)| path.clone())
                        .collect();
                    if paths.is_empty() {
                        model.toast(
                            crate::tr!(
                                model,
                                "No tracked changes under this folder",
                                "该目录下没有已跟踪的更改"
                            )
                            .to_string(),
                            ToastKind::Warning,
                        );
                    } else {
                        rt.dialog = DialogState::ConfirmDiscard { paths };
                    }
                } else if changes
                    .iter()
                    .any(|(path, untracked)| *untracked && path == rel_path)
                {
                    // Untracked files have a trash-based Delete flow; Rollback
                    // must not permanently unlink them.
                    rt.dialog = DialogState::ConfirmDeleteUntracked {
                        paths: vec![abs_path],
                    };
                } else {
                    rt.dialog = DialogState::ConfirmDiscard {
                        paths: vec![rel_path.to_string()],
                    };
                }
            }
            50 => {
                let paths: Vec<std::path::PathBuf> = model
                    .ui
                    .tree_selection
                    .iter()
                    .map(|p| project.active_root().join(p))
                    .collect();
                rt.dialog = DialogState::ConfirmDeleteUntracked { paths };
            }
            101 | 102 => {
                ui.ctx().copy_text(abs_path.to_string_lossy().to_string());
            }
            104 => {
                rt.search.open = true;
                rt.search.focus_requested = true;
                rt.search.file_mask = rel_path.to_string();
            }
            105 => {
                if let Some(project) = model.projects.get_mut(project_idx) {
                    fn expand_rec(
                        nodes: &[lazecode_fs::tree::TreeNode],
                        expanded: &mut std::collections::HashSet<std::path::PathBuf>,
                    ) {
                        for node in nodes {
                            if node.kind == lazecode_fs::tree::NodeKind::Folder {
                                expanded.insert(node.path.clone());
                                expand_rec(&node.children, expanded);
                            }
                        }
                    }
                    let base = std::path::Path::new(rel_path);
                    project.expanded.insert(base.to_path_buf());
                    let nodes = project.active_tree().nodes.clone();
                    expand_rec(&nodes, &mut project.expanded);
                }
            }
            106 => {
                if let Some(project) = model.projects.get_mut(project_idx) {
                    let base = std::path::Path::new(rel_path);
                    project
                        .expanded
                        .retain(|p| !p.starts_with(base) || p == base);
                    project.expanded.remove(base);
                }
            }
            107 => {
                if let Some(project) = model.projects.get_mut(project_idx) {
                    let root = project.active_root().to_path_buf();
                    project.active_worktree_mut().tree = lazecode_fs::tree::load_tree(&root);
                    model.queue(Command::ReloadGit);
                }
            }
            210 => {
                model.queue(Command::Stage(vec![rel_path.to_string()]));
            }
            211 => {
                if let Ok(id) = model.open_file_in(project_idx, abs_path.clone()) {
                    rt.set_editor_focus(id);
                    let root = model.projects[project_idx].active_root().to_path_buf();
                    let blame = lazecode_git::blame(&root, rel_path).ok();
                    if let Some(tab) = model.projects[project_idx].tab_mut_by_id(id) {
                        if let lazecode_core::TabKind::File(file_tab) = &mut tab.kind {
                            file_tab.annotate = true;
                            if file_tab.blame.is_none() {
                                file_tab.blame = blame;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        close = true;
        rt.tree_submenu = None;
    }

    let dismiss_main = should_dismiss(ui, rect, menu.opened_frame, rt.frame_count);
    let in_sub = sub_rect_opt.is_some_and(|sr| {
        ui.input(|i| i.pointer.hover_pos())
            .is_some_and(|p| sr.contains(p))
    });
    if dismiss_main && !in_sub {
        close = true;
        rt.tree_submenu = None;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_editor_tab_context_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    tab_id: u64,
    path: &str,
) {
    let p = theme.palette;
    // 条目与禁用规则对齐稳定版 EditorTabBar 菜单：关闭左侧在首标签禁用，
    // 关闭右侧在末标签禁用；「关闭未修改」与路径三件套是新版补充项。
    let (is_first, is_last) = model
        .projects
        .get(project_idx)
        .map(|project| {
            let count = project.active_worktree_tabs().count();
            let pos = project
                .active_worktree_tabs()
                .position(|tab| tab.id == tab_id);
            (pos == Some(0), pos == Some(count.saturating_sub(1)))
        })
        .unwrap_or((false, false));
    let items: Vec<(String, u8, bool)> = vec![
        (crate::tr!(model, "Close", "关闭").to_string(), 0, true),
        (
            crate::tr!(model, "Close others", "关闭其他").to_string(),
            1,
            true,
        ),
        (
            crate::tr!(model, "Close to the left", "关闭左侧标签").to_string(),
            8,
            !is_first,
        ),
        (
            crate::tr!(model, "Close to the right", "关闭右侧标签").to_string(),
            2,
            !is_last,
        ),
        (String::new(), u8::MAX, true),
        (
            crate::tr!(model, "Close all", "关闭全部").to_string(),
            4,
            true,
        ),
        (
            crate::tr!(model, "Close unmodified", "关闭未修改标签").to_string(),
            3,
            true,
        ),
        (String::new(), u8::MAX, true),
        (
            crate::tr!(model, "Copy path", "复制绝对路径").to_string(),
            5,
            true,
        ),
        (
            crate::tr!(model, "Copy relative path", "复制相对路径").to_string(),
            6,
            true,
        ),
        (
            crate::tr!(model, "Reveal in Finder", "在访达中显示").to_string(),
            7,
            true,
        ),
    ];

    let height = items.len() as f32 * MENU_ROW_H + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 200.0, height);
    menu_frame(ui, rect, theme);

    // 稳定版右键菜单没有标题行（对齐 EditorTabBar ctx menu），直接列菜单项。
    let mut action: Option<u8> = None;
    let mut close = false;
    let row_start = rect.top() + 4.0;
    for (i, (label, code, enabled)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, row_start + i as f32 * MENU_ROW_H),
            Vec2::new(rect.width() - 16.0, 26.0),
        );
        if *code == u8::MAX {
            // 分隔线（稳定版菜单在关闭组与全部组之间有分隔线）。
            ui.painter().line_segment(
                [
                    Pos2::new(row.left() + 8.0, row.center().y),
                    Pos2::new(row.right() - 8.0, row.center().y),
                ],
                egui::Stroke::new(1.0, p.border),
            );
            continue;
        }
        let response = ui.interact(row, ui.scope_id().with(("etab_ctx", i)), Sense::click());
        if response.hovered() && *enabled {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
        }
        ui.painter().text(
            Pos2::new(row.left() + 10.0, row.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(12.5),
            if *enabled { p.text } else { p.text3 },
        );
        if response.clicked() && *enabled {
            action = Some(*code);
        }
    }

    if let Some(code) = action {
        let relative_root = model
            .projects
            .get(project_idx)
            .map(|project| project.active_root().to_path_buf());
        match code {
            0 => model.close_tab(project_idx, tab_id),
            1 => model.close_other_tabs(project_idx, tab_id),
            2 => model.close_tabs_to_right(project_idx, tab_id),
            3 => model.close_unmodified_tabs(project_idx),
            4 => model.close_all_tabs(project_idx),
            8 => model.close_tabs_to_left(project_idx, tab_id),
            5 => {
                ui.ctx().copy_text(path.to_string());
                model.toast(
                    crate::tr!(model, "Path copied", "已复制路径").to_string(),
                    ToastKind::Info,
                );
            }
            6 => {
                let rel = relative_root
                    .as_deref()
                    .and_then(|root| std::path::Path::new(path).strip_prefix(root).ok())
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string());
                ui.ctx().copy_text(rel);
                model.toast(
                    crate::tr!(model, "Relative path copied", "已复制相对路径").to_string(),
                    ToastKind::Info,
                );
            }
            7 => {
                #[cfg(target_os = "macos")]
                {
                    let _ = std::process::Command::new("open")
                        .arg("-R")
                        .arg(path)
                        .spawn();
                }
                #[cfg(not(target_os = "macos"))]
                {
                    if let Some(parent) = std::path::Path::new(path).parent() {
                        let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
                    }
                }
            }
            _ => {}
        }
        close = true;
    }

    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_terminal_tab_context_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    tab_id: u64,
    terminal_id: u64,
    title: &str,
) {
    let p = theme.palette;
    // 与稳定版 TerminalTabContextMenu 同序：Copy / 分屏 / 关闭 / 标签批量关闭 / 重命名。
    let tab_info = model.projects.get(project_idx).and_then(|project| {
        project.worktrees.iter().find_map(|worktree| {
            worktree
                .tab_by_id(tab_id)
                .map(|tab| (tab.panes.len(), worktree.path.clone()))
        })
    });
    let pane_count = tab_info.as_ref().map_or(0, |(count, _)| *count);
    let tab_path = tab_info.map(|(_, path)| path).unwrap_or_default();
    let can_split = pane_count < lazecode_core::MAX_PANES_PER_TAB;
    let can_close_pane = pane_count > 1;

    #[derive(Clone, Copy)]
    enum Item {
        Row(&'static str, &'static str, u8, bool),
        Sep,
    }
    let items = [
        Item::Row(
            "Copy Working Directory",
            "复制工作目录",
            0,
            tab_path.as_os_str().is_empty(),
        ),
        Item::Sep,
        Item::Row("Split Right", "向右分屏", 1, !can_split),
        Item::Row("Split Down", "向下分屏", 2, !can_split),
        Item::Row("Close Pane", "关闭分屏", 3, !can_close_pane),
        Item::Sep,
        Item::Row("Close", "关闭", 4, false),
        Item::Sep,
        Item::Row("Close Others", "关闭其他", 5, false),
        Item::Row("Close All", "关闭全部", 6, false),
        Item::Sep,
        Item::Row("Rename…", "重命名…", 7, false),
    ];

    let rows = items
        .iter()
        .filter(|item| !matches!(item, Item::Sep))
        .count();
    let seps = items.len() - rows;
    let height = rows as f32 * MENU_ROW_H + seps as f32 * 8.0 + 12.0;
    let rect = popover_rect(rt, screen, menu.anchor, 200.0, height);
    menu_frame(ui, rect, theme);

    let zh = model.settings.language.is_zh();
    let mut action: Option<u8> = None;
    let mut close = false;
    let mut y = rect.top() + 6.0;
    let mut row_index = 0usize;
    for item in &items {
        match item {
            Item::Sep => {
                ui.painter().hline(
                    (rect.left() + 8.0)..=(rect.right() - 8.0),
                    y + 3.5,
                    Stroke::new(1.0, p.border_soft),
                );
                y += 8.0;
            }
            Item::Row(en, cn, code, disabled) => {
                let row = Rect::from_min_size(
                    Pos2::new(rect.left() + 4.0, y),
                    Vec2::new(rect.width() - 8.0, 22.0),
                );
                let response = ui.interact(
                    row,
                    ui.scope_id().with(("ttab_ctx", row_index)),
                    Sense::click(),
                );
                if !*disabled && response.hovered() {
                    ui.painter()
                        .rect_filled(row, egui::CornerRadius::same(4), p.hover);
                }
                ui.painter().text(
                    Pos2::new(row.left() + 10.0, row.center().y),
                    Align2::LEFT_CENTER,
                    if zh { *cn } else { *en },
                    FontId::proportional(12.5),
                    if *disabled { p.text3 } else { p.text },
                );
                if !*disabled && response.clicked() {
                    action = Some(*code);
                }
                y += MENU_ROW_H;
                row_index += 1;
            }
        }
    }

    if let Some(code) = action {
        // Collect the panes a close operation tears down so their cached
        // render state can be dropped afterwards.
        let settings = model.settings.clone();
        let notifier = model.notifier.clone();
        let mut notice: Option<String> = None;
        let mut focus: Option<u64> = None;
        let mut terminal_action: Option<crate::runtime::TerminalCloseAction> = None;
        if let Some(project) = model.projects.get_mut(project_idx) {
            match code {
                0 => {
                    if tab_path.as_os_str().is_empty() {
                        // Disabled row; nothing to copy.
                    } else {
                        ui.ctx().copy_text(tab_path.display().to_string());
                    }
                }
                1 | 2 => {
                    let direction = if code == 1 {
                        lazecode_core::PaneSplit::Row
                    } else {
                        lazecode_core::PaneSplit::Column
                    };
                    match project.split_terminal(terminal_id, direction, &settings, notifier) {
                        Ok(id) => focus = Some(id),
                        Err(err) => notice = Some(err),
                    }
                }
                3 => {
                    terminal_action = Some(crate::runtime::TerminalCloseAction::Pane(terminal_id));
                }
                4 => {
                    terminal_action = Some(crate::runtime::TerminalCloseAction::Tab(tab_id));
                }
                5 => {
                    terminal_action =
                        Some(crate::runtime::TerminalCloseAction::Others(terminal_id));
                }
                6 => {
                    terminal_action = Some(crate::runtime::TerminalCloseAction::All);
                }
                7 => {
                    rt.dialog = DialogState::RenameTerminal {
                        terminal_id,
                        value: title.to_string(),
                        error: None,
                    };
                }
                _ => {}
            }
        }
        if let Some(action) = terminal_action {
            rt.request_terminal_close(model, action);
        }
        if let Some(id) = focus {
            rt.set_terminal_focus(id);
        }
        if code == 0 && !tab_path.as_os_str().is_empty() {
            model.toast(
                crate::tr!(model, "Working directory copied", "已复制工作目录").to_string(),
                ToastKind::Info,
            );
        }
        if let Some(message) = notice {
            model.toast(message, ToastKind::Error);
        }
        close = true;
    }

    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_editor_context_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    path_str: &str,
) {
    let zh = model.settings.language.is_zh();
    let mut items = vec![
        (if zh { "剪切" } else { "Cut" }, 0),
        (if zh { "复制" } else { "Copy" }, 1),
        (if zh { "粘贴" } else { "Paste" }, 2),
        (if zh { "全选" } else { "Select All" }, 3),
        (
            if zh {
                "跳转到行…"
            } else {
                "Go to Line…"
            },
            4,
        ),
        (
            if zh {
                "切换行注释"
            } else {
                "Toggle Line Comment"
            },
            5,
        ),
        (if zh { "文件历史" } else { "File History" }, 6),
        (
            if zh {
                "显示 Git 追溯标注"
            } else {
                "Annotate with Git Blame"
            },
            7,
        ),
    ];
    if lazecode_core::is_json_path(std::path::Path::new(path_str)) {
        items.push((if zh { "格式化 JSON" } else { "Format JSON" }, 8));
        items.push((if zh { "压缩 JSON" } else { "Minify JSON" }, 9));
    }
    let height = items.len() as f32 * MENU_ROW_H + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 200.0, height);
    menu_frame(ui, rect, theme);
    let mut action: Option<u8> = None;
    let row_start = rect.top() + 4.0;
    for (i, (label, code)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, row_start + i as f32 * MENU_ROW_H),
            Vec2::new(rect.width() - 8.0, MENU_ROW_H),
        );
        if menu_row(
            ui,
            theme,
            row,
            ui.scope_id().with(("ed_ctx", i)),
            label,
            false,
            true,
        ) {
            action = Some(*code);
        }
    }
    let mut close = false;
    if let Some(code) = action {
        let active_tab_id = model.projects[project_idx].active_tab;
        match code {
            0 => {
                if let Some(id) = active_tab_id {
                    if let Some(tab) = model.projects[project_idx].tab_mut_by_id(id) {
                        if let lazecode_core::TabKind::File(file) = &mut tab.kind {
                            if let Some(text) = file.doc.selected_text() {
                                ui.ctx().copy_text(text);
                                file.doc.delete_selection();
                            }
                        }
                    }
                }
            }
            1 => {
                if let Some(id) = active_tab_id {
                    if let Some(tab) = model.projects[project_idx].tab_by_id(id) {
                        if let lazecode_core::TabKind::File(file) = &tab.kind {
                            if let Some(text) = file.doc.selected_text() {
                                ui.ctx().copy_text(text);
                            } else {
                                let line = file.doc.cursor_line_text();
                                ui.ctx().copy_text(line);
                            }
                        }
                    }
                }
            }
            2 => {
                if let Some(id) = active_tab_id {
                    #[cfg(target_os = "macos")]
                    let text = std::process::Command::new("pbpaste")
                        .output()
                        .ok()
                        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                        .unwrap_or_default();
                    #[cfg(not(target_os = "macos"))]
                    let text = String::new();
                    if !text.is_empty() {
                        if let Some(tab) = model.projects[project_idx].tab_mut_by_id(id) {
                            if let lazecode_core::TabKind::File(file) = &mut tab.kind {
                                file.doc.insert_text(&text);
                            }
                        }
                    }
                }
            }
            3 => {
                if let Some(id) = active_tab_id {
                    if let Some(tab) = model.projects[project_idx].tab_mut_by_id(id) {
                        if let lazecode_core::TabKind::File(file) = &mut tab.kind {
                            file.doc.select_all();
                        }
                    }
                }
            }
            4 => {
                model.ui.command_palette_open = true;
                model.ui.palette_query = ":".to_string();
            }
            5 => {
                if let Some(id) = active_tab_id {
                    if let Some(tab) = model.projects[project_idx].tab_mut_by_id(id) {
                        if let lazecode_core::TabKind::File(file) = &mut tab.kind {
                            file.doc.toggle_comment();
                        }
                    }
                }
            }
            6 => {
                let root = model.projects[project_idx].active_root().to_path_buf();
                let rel = std::path::Path::new(path_str)
                    .strip_prefix(&root)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path_str.to_string());
                rt.dialog = DialogState::FileHistory {
                    path: rel,
                    entries: Vec::new(),
                    selected: 0,
                    loaded: false,
                };
            }
            7 => {
                if let Some(id) = active_tab_id {
                    if let Some(tab) = model.projects[project_idx].tab_mut_by_id(id) {
                        if let lazecode_core::TabKind::File(file) = &mut tab.kind {
                            file.annotate = !file.annotate;
                        }
                    }
                }
            }
            8 | 9 => {
                if let Some(id) = active_tab_id {
                    crate::editor::format_json_tab(model, project_idx, id, code == 9);
                }
            }
            _ => {}
        }
        close = true;
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}
