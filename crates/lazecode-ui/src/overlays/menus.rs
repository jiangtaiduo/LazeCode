//! Context menus: the dispatcher plus one module per menu family.
mod log;
mod prelude;
mod project;
use log::*;
use prelude::*;
use project::*;

#[allow(clippy::too_many_arguments)]
fn draw_branch_context_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    index: usize,
    payload: Option<&str>,
) {
    let Some((name, remote_flag)) = payload.and_then(|p| {
        let (name, kind) = p.split_once('\u{1}')?;
        Some((name.to_string(), kind.to_string()))
    }) else {
        return;
    };
    let remote = remote_flag == "remote";
    let _p = theme.palette;
    let selected_refs: Vec<String> = model.projects[index].ref_selection.clone();
    let is_multi = selected_refs.len() > 1;
    let items: Vec<(&str, u8)> = if is_multi {
        let mut list = Vec::new();
        if selected_refs.len() == 2 {
            list.push((
                crate::tr!(model, "Compare branches…", "比较所选两个分支…"),
                70,
            ));
            list.push((crate::tr!(model, "Show files diff…", "显示文件差异…"), 73));
        }
        list.push((crate::tr!(model, "Update selected", "更新选中项"), 72));
        list.push((
            crate::tr!(model, "Delete selected branches", "删除选中的分支"),
            71,
        ));
        list
    } else if remote {
        vec![
            (crate::tr!(model, "Checkout branch", "检出分支"), 0),
            (
                crate::tr!(model, "New branch from here", "从这里新建分支"),
                1,
            ),
            (crate::tr!(model, "Compare branches…", "比较分支…"), 7),
            (crate::tr!(model, "Merge into current", "合并到当前分支"), 8),
            (
                crate::tr!(model, "Rebase current onto", "变基当前分支到此处"),
                9,
            ),
            (crate::tr!(model, "Set log filter", "设为日志过滤"), 6),
            (crate::tr!(model, "Delete branch", "删除分支"), 3),
        ]
    } else {
        vec![
            (crate::tr!(model, "Checkout branch", "检出分支"), 0),
            (
                crate::tr!(model, "New branch from here", "从这里新建分支"),
                1,
            ),
            (crate::tr!(model, "Rename branch", "重命名"), 2),
            (crate::tr!(model, "Delete branch", "删除分支"), 3),
            (crate::tr!(model, "Compare branches…", "比较分支…"), 7),
            (crate::tr!(model, "Merge into current", "合并到当前分支"), 8),
            (
                crate::tr!(model, "Rebase current onto", "变基当前分支到此处"),
                9,
            ),
            (crate::tr!(model, "Set log filter", "设为日志过滤"), 6),
        ]
    };
    let favorite_root = model.projects[index].root.clone();
    let is_fav = model.is_favorite_branch(&favorite_root, &name);
    let fav_label = if is_fav {
        crate::tr!(model, "Remove favorite", "取消收藏")
    } else {
        crate::tr!(model, "Mark favorite", "标记收藏")
    };
    // The favorite row is drawn after `items`, so include it in the frame.
    let extra_row = if is_multi { 0.0 } else { MENU_ROW_H };
    let height = items.len() as f32 * MENU_ROW_H + extra_row + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 260.0, height);
    menu_frame(ui, rect, theme);
    // 稳定版分支右键菜单没有标题行（BranchContextMenu 无 header）。
    let mut action: Option<u8> = None;
    let mut toggle_fav = false;
    let row_start = rect.top() + 4.0;
    for (i, (label, code)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, row_start + i as f32 * MENU_ROW_H),
            Vec2::new(rect.width() - 8.0, MENU_ROW_H),
        );
        let danger = matches!(*code, 3 | 71);
        if menu_row(
            ui,
            theme,
            row,
            ui.scope_id().with(("bctx", i)),
            label,
            danger,
            true,
        ) {
            action = Some(*code);
        }
    }
    // favorite row last.
    if !is_multi {
        let fav_row = Rect::from_min_size(
            Pos2::new(
                rect.left() + 4.0,
                row_start + items.len() as f32 * MENU_ROW_H,
            ),
            Vec2::new(rect.width() - 8.0, MENU_ROW_H),
        );
        if menu_row(
            ui,
            theme,
            fav_row,
            ui.scope_id().with("bctx-fav"),
            fav_label,
            false,
            true,
        ) {
            toggle_fav = true;
        }
    }
    let mut close = false;
    if toggle_fav {
        model.toggle_favorite_branch(&favorite_root, &name);
        model.save_workspace();
        close = true;
    }
    if let Some(code) = action {
        match code {
            0 => model.queue(Command::CheckoutBranch(name.clone())),
            1 => {
                rt.dialog = DialogState::NewBranch {
                    value: name.clone(),
                    start_point: Some(name.clone()),
                    checkout: true,
                    error: None,
                };
            }
            2 => {
                rt.dialog = DialogState::RenameBranch {
                    from: name.clone(),
                    value: name.clone(),
                    error: None,
                };
            }
            3 => {
                rt.dialog = DialogState::DeleteBranch {
                    name: name.clone(),
                    force: false,
                    remote,
                    error: None,
                };
            }
            4 => {
                model.queue(Command::OpenDiff(name.clone()));
            }
            6 => {
                if let Some(project) = model.projects.get_mut(index) {
                    project.log_query.branch = name.clone();
                    project.log_query.all_refs = false;
                    project.log_loading = true;
                }
                model.queue(Command::LogFilterChanged);
            }
            7 => {
                let current = model.projects[index].branch();
                crate::editor::open_compare_page(model, current, name.clone());
            }
            8 => {
                model.queue(Command::Merge {
                    ref_name: name.clone(),
                });
            }
            9 => {
                model.queue(Command::Rebase { onto: name.clone() });
            }
            70 => {
                if selected_refs.len() == 2 {
                    crate::editor::open_compare_page(
                        model,
                        selected_refs[0].clone(),
                        selected_refs[1].clone(),
                    );
                }
            }
            72 => {
                model.queue(Command::FetchAll);
            }
            73 => {
                if selected_refs.len() == 2 {
                    model.queue(Command::CompareOpen {
                        from: selected_refs[1].clone(),
                        to: selected_refs[0].clone(),
                    });
                    model.ui.sidebar_view = lazecode_core::SidebarView::Compare;
                }
            }
            71 => {
                for b in selected_refs {
                    if model.projects[index]
                        .branches
                        .iter()
                        .any(|branch| branch.name == b && branch.remote)
                    {
                        model.queue(Command::DeleteRemoteBranch(b));
                    } else if model.projects[index].tags.iter().any(|tag| tag == &b) {
                        model.queue(Command::DeleteTag(b));
                    } else {
                        model.queue(Command::DeleteBranch {
                            name: b,
                            force: false,
                        });
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
fn draw_tree_context_menu(
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
fn draw_editor_tab_context_menu(
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
fn draw_terminal_tab_context_menu(
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
fn draw_changelist_header_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    payload: &str,
) {
    // payload: 组名 \u{1} 组内全部文件路径（稳定版组头菜单 = New/Delete
    // Changelist + 整组文件菜单，条目与文件行右键完全一致）。
    let mut segments = payload.split('\u{1}');
    let Some(name) = segments.next() else {
        return;
    };
    let paths: Vec<String> = segments
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect();
    let Some(project) = model.projects.get(project_idx) else {
        return;
    };
    let is_default = name.eq_ignore_ascii_case("default") || name.eq_ignore_ascii_case("changes");
    let unversioned = crate::tr!(model, "Unversioned Files", "未版本控制的文件");
    let ignored = crate::tr!(model, "Ignored Files", "被忽略的文件");
    let is_pseudo = name == unversioned || name == ignored;
    let zh = model.settings.language.is_zh();
    let mut items: Vec<ChangeMenuItem> = Vec::new();
    items.push(change_menu_item(
        if zh {
            "新建变更列表…".to_string()
        } else {
            "New Changelist…".to_string()
        },
        100,
        false,
        true,
    ));
    if !is_default && !is_pseudo {
        items.push(change_menu_item(
            if zh {
                "删除变更列表…".to_string()
            } else {
                "Delete Changelist…".to_string()
            },
            101,
            true,
            true,
        ));
    }
    let mut menu_ctx_holder: Option<ChangeMenuCtx> = None;
    if !paths.is_empty() {
        change_menu_sep(&mut items);
        let (file_items, menu_ctx) = change_menu_items(model, project, &paths);
        items.extend(file_items);
        menu_ctx_holder = Some(menu_ctx);
    } else {
        change_menu_sep(&mut items);
        items.push(change_menu_item(
            if zh {
                "刷新 VCS 更改".to_string()
            } else {
                "Refresh VCS Changes".to_string()
            },
            7,
            false,
            true,
        ));
    }

    let height = items.len() as f32 * MENU_ROW_H + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 260.0, height);
    menu_frame(ui, rect, theme);

    // 稳定版右键菜单没有标题行，直接列菜单项。
    let mut close = false;
    if let Some(code) =
        render_change_menu_items(ui, theme, rect, rect.top() + 8.0, "clist_h_ctx", &items)
    {
        match code {
            100 => {
                rt.dialog = DialogState::NewChangelist {
                    value: String::new(),
                    error: None,
                };
                close = true;
            }
            101 => {
                // 删除前确认（稳定版语义）：移入默认列表 or 回滚丢弃。
                if let Some(project) = model.projects.get(project_idx) {
                    let lists = lazecode_git::Changelists::open(project.active_root());
                    let count = lists.files_of(name).len();
                    rt.dialog = DialogState::DeleteChangelist {
                        name: name.to_string(),
                        count,
                        rollback: false,
                    };
                }
                close = true;
            }
            7 => {
                model.queue(Command::ReloadGit);
                close = true;
            }
            other => {
                if let Some(ctx) = &menu_ctx_holder {
                    close = apply_change_menu_action(ui, model, rt, ctx, other);
                }
            }
        }
    }

    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_editor_context_menu(
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

/// Change-file menu codes: 0..10 generic, 50..200 move-to lists, 20/21 untracked,
/// 22..24 new actions, 30..37 extended actions, SEP divider, 100/101
/// changelist headers.
struct ChangeMenuItem {
    label: String,
    code: u8,
    danger: bool,
    enabled: bool,
}

const CHANGE_MENU_SEP: u8 = u8::MAX;

fn change_menu_sep(items: &mut Vec<ChangeMenuItem>) {
    items.push(ChangeMenuItem {
        label: String::new(),
        code: CHANGE_MENU_SEP,
        danger: false,
        enabled: false,
    });
}

fn change_menu_item(label: String, code: u8, danger: bool, enabled: bool) -> ChangeMenuItem {
    ChangeMenuItem {
        label,
        code,
        danger,
        enabled,
    }
}

struct ChangeMenuCtx {
    project_root: std::path::PathBuf,
    conflicts: Vec<lazecode_git::ConflictEntry>,
    paths: Vec<String>,
    untracked: std::collections::HashSet<String>,
    single: bool,
    first: String,
    display_name: String,
    all_lists: Vec<String>,
}

/// 构建变更文件菜单条目（文件行右键与变更列表组头右键共用，稳定版
/// CommitDrawer 菜单一比一：词条连 zh-CN 都保持原文，不走近路 tr!）。
fn change_menu_items(
    model: &AppModel,
    project: &lazecode_core::ProjectState,
    paths: &[String],
) -> (Vec<ChangeMenuItem>, ChangeMenuCtx) {
    let single = paths.len() == 1;
    let first = paths.first().cloned().unwrap_or_default();
    let lists = lazecode_git::Changelists::open(project.active_root());
    let all_lists: Vec<String> = lists.lists().to_vec();
    let untracked: std::collections::HashSet<String> = project
        .active_git()
        .map(|git| {
            git.changes
                .iter()
                .filter(|change| change.untracked)
                .map(|change| change.path.clone())
                .collect()
        })
        .unwrap_or_default();
    let ctx = ChangeMenuCtx {
        project_root: project.active_root().to_path_buf(),
        conflicts: project.conflicts.clone(),
        paths: paths.to_vec(),
        untracked: untracked.clone(),
        single,
        first: first.clone(),
        display_name: first.rsplit('/').next().unwrap_or(&first).to_string(),
        all_lists: all_lists.clone(),
    };
    let conflicted = project
        .conflicts
        .iter()
        .any(|entry| paths.iter().any(|path| path == &entry.path));
    // 稳定版 allUntracked 规则：所选文件全部是未跟踪才走未跟踪菜单。
    let all_untracked = paths.iter().all(|path| untracked.contains(path));

    let mut items: Vec<ChangeMenuItem> = Vec::new();
    let zh = model.settings.language.is_zh();
    if all_untracked {
        items.push(change_menu_item(
            if zh {
                if single {
                    "添加到 VCS".to_string()
                } else {
                    format!("添加 {} 个文件到 VCS", paths.len())
                }
            } else {
                if single {
                    "Add to VCS".to_string()
                } else {
                    format!("Add {} Files to VCS", paths.len())
                }
            },
            20,
            false,
            true,
        ));
        if single {
            items.push(change_menu_item(
                if zh {
                    "跳转到源代码".to_string()
                } else {
                    "Jump to Source".to_string()
                },
                1,
                false,
                true,
            ));
        }
        change_menu_sep(&mut items);
        items.push(change_menu_item(
            if zh {
                if single {
                    "删除…".to_string()
                } else {
                    format!("删除 {} 个文件…", paths.len())
                }
            } else {
                if single {
                    "Delete…".to_string()
                } else {
                    format!("Delete {} Files…", paths.len())
                }
            },
            21,
            true,
            true,
        ));
        return (items, ctx);
    }

    items.push(change_menu_item(
        if zh {
            if single {
                "提交文件…".to_string()
            } else {
                "提交所选文件…".to_string()
            }
        } else {
            if single {
                "Commit File...".to_string()
            } else {
                "Commit Selected Files…".to_string()
            }
        },
        3,
        false,
        true,
    ));
    if single {
        items.push(change_menu_item(
            if zh {
                "查看差异".to_string()
            } else {
                "Show Diff".to_string()
            },
            0,
            false,
            true,
        ));
        items.push(change_menu_item(
            if zh {
                "在新标签页查看差异".to_string()
            } else {
                "Show Diff in a New Tab".to_string()
            },
            30,
            false,
            true,
        ));
        items.push(change_menu_item(
            if zh {
                "与本地比较".to_string()
            } else {
                "Compare with Local".to_string()
            },
            31,
            false,
            true,
        ));
        items.push(change_menu_item(
            if zh {
                "与本地的前一版本比较".to_string()
            } else {
                "Compare Before with Local".to_string()
            },
            22,
            false,
            true,
        ));
        items.push(change_menu_item(
            if zh {
                "跳转到源代码".to_string()
            } else {
                "Jump to Source".to_string()
            },
            1,
            false,
            true,
        ));
        items.push(change_menu_item(
            if zh {
                "打开仓库版本".to_string()
            } else {
                "Open Repository Version".to_string()
            },
            32,
            false,
            true,
        ));
        if conflicted {
            items.push(change_menu_item(
                if zh {
                    "解决…".to_string()
                } else {
                    "Resolve…".to_string()
                },
                2,
                false,
                true,
            ));
        }
    }
    change_menu_sep(&mut items);
    items.push(change_menu_item(
        if model.settings.language.is_zh() {
            "还原选中更改".to_string()
        } else {
            "Revert Selected Changes".to_string()
        },
        4,
        true,
        true,
    ));
    items.push(change_menu_item(
        if model.settings.language.is_zh() {
            "拣选选中更改".to_string()
        } else {
            "Cherry-Pick Selected Changes".to_string()
        },
        33,
        false,
        true,
    ));
    items.push(change_menu_item(
        if model.settings.language.is_zh() {
            "提取选中更改到独立提交…".to_string()
        } else {
            "Extract Selected Changes to Separate Commit…".to_string()
        },
        34,
        false,
        true,
    ));
    items.push(change_menu_item(
        if model.settings.language.is_zh() {
            "丢弃选中更改".to_string()
        } else {
            "Drop Selected Changes".to_string()
        },
        35,
        true,
        true,
    ));
    if single {
        items.push(change_menu_item(
            if zh {
                "从本地更改创建补丁…".to_string()
            } else {
                "Create Patch from Local Changes...".to_string()
            },
            23,
            false,
            true,
        ));
        items.push(change_menu_item(
            if model.settings.language.is_zh() {
                "从版本获取…".to_string()
            } else {
                "Get from Revision…".to_string()
            },
            36,
            false,
            true,
        ));
        items.push(change_menu_item(
            if model.settings.language.is_zh() {
                "到此为止的历史".to_string()
            } else {
                "History Up to Here".to_string()
            },
            24,
            false,
            true,
        ));
        items.push(change_menu_item(
            if model.settings.language.is_zh() {
                "显示与父提交的差异".to_string()
            } else {
                "Show Changes to Parents".to_string()
            },
            37,
            false,
            true,
        ));
    }
    change_menu_sep(&mut items);
    for (idx, list_name) in all_lists.iter().enumerate() {
        // 稳定版规则：所选文件已全部在该列表中 → 禁用（多选不再跳过）。
        let all_in_list = paths.iter().all(|path| lists.list_of(path) == *list_name);
        items.push(change_menu_item(
            if zh {
                format!("移动到 {list_name}")
            } else {
                format!("Move to {list_name}")
            },
            50 + idx as u8,
            false,
            !all_in_list,
        ));
    }
    if single {
        items.push(change_menu_item(
            if zh {
                "复制补丁到剪贴板".to_string()
            } else {
                "Copy as Patch to Clipboard".to_string()
            },
            5,
            false,
            true,
        ));
    }
    items.push(change_menu_item(
        if zh {
            "贮藏更改…".to_string()
        } else {
            "Shelve Changes...".to_string()
        },
        6,
        false,
        true,
    ));
    items.push(change_menu_item(
        if zh {
            "刷新 VCS 更改".to_string()
        } else {
            "Refresh VCS Changes".to_string()
        },
        7,
        false,
        true,
    ));
    items.push(change_menu_item(
        if zh {
            "新建变更列表…".to_string()
        } else {
            "New Changelist…".to_string()
        },
        8,
        false,
        true,
    ));
    (items, ctx)
}

fn discardable_paths(ctx: &ChangeMenuCtx) -> Vec<String> {
    // The untracked branch has its own Delete-to-trash confirmation. A mixed
    // selection must not silently delete an untracked file through Rollback.
    ctx.paths
        .iter()
        .filter(|path| !ctx.untracked.contains(*path))
        .cloned()
        .collect()
}

/// 变更菜单动作分发（文件行与组头菜单共用；返回是否应关闭菜单）。
fn apply_change_menu_action(
    ui: &Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    ctx: &ChangeMenuCtx,
    code: u8,
) -> bool {
    let project_idx = model.active_project.unwrap_or(0);
    match code {
        0 => model.queue(Command::OpenDiff(ctx.first.clone())),
        1 => {
            // Jump to Source：在编辑器打开（仓库相对 → 绝对路径）。
            let abs = ctx.project_root.join(&ctx.first);
            if let Err(err) = model.open_file_in(project_idx, abs) {
                model.toast(err, ToastKind::Error);
            }
        }
        2 => {
            if ctx.conflicts.iter().any(|entry| entry.path == ctx.first) {
                if let Ok(content) = lazecode_git::merge_file_content(&ctx.project_root, &ctx.first)
                {
                    rt.dialog = DialogState::ConflictResolve {
                        path: ctx.first.clone(),
                        ours: content.ours,
                        theirs: content.theirs,
                        worktree: content.worktree,
                    };
                }
            }
        }
        3 => {
            rt.dialog = DialogState::CommitFiles {
                paths: ctx.paths.clone(),
                value: String::new(),
                error: None,
            };
        }
        4 => {
            // 统一确认弹窗（对齐稳定版 rollbackFileConfirm）。Mixed
            // selections must not delete untracked files via Rollback.
            let paths = discardable_paths(ctx);
            if paths.is_empty() {
                model.toast(
                    crate::tr!(
                        model,
                        "Use Delete for untracked files",
                        "未跟踪文件请使用「删除」"
                    )
                    .to_string(),
                    ToastKind::Warning,
                );
            } else {
                rt.dialog = DialogState::ConfirmDiscard { paths };
            }
        }
        5 => {
            // Copy as Patch：HEAD→工作区补丁（含未跟踪文件）。
            match lazecode_git::diff_patch_paths(&ctx.project_root, &ctx.paths) {
                Ok(patch) => {
                    ui.ctx().copy_text(patch);
                    model.toast(
                        crate::tr!(model, "Patch copied to clipboard", "已将补丁复制到剪贴板")
                            .to_string(),
                        ToastKind::Info,
                    );
                }
                Err(err) => model.toast(err.to_string(), ToastKind::Error),
            }
        }
        6 => {
            // Shelve Changes...：架藏所选文件，默认名对齐稳定版。
            let default_name = if ctx.single {
                ctx.display_name.clone()
            } else {
                "Default".to_string()
            };
            rt.dialog = DialogState::NewShelf {
                value: default_name,
                error: None,
                paths: ctx.paths.clone(),
            };
        }
        7 => model.queue(Command::ReloadGit),
        8 => {
            rt.dialog = DialogState::NewChangelist {
                value: String::new(),
                error: None,
            };
        }
        20 => {
            // Add to VCS：git add 走后台任务，回填后刷新快照。
            model.queue(Command::AddToVcs(ctx.paths.clone()));
        }
        21 => {
            // 未跟踪文件 Delete…：全部移入废纸篓（稳定版 fs_delete 语义）。
            if ctx.single {
                rt.dialog = DialogState::DeletePath {
                    path: ctx.project_root.join(&ctx.first),
                    is_dir: false,
                };
            } else {
                rt.dialog = DialogState::ConfirmDeleteUntracked {
                    paths: ctx.paths.iter().map(|p| ctx.project_root.join(p)).collect(),
                };
            }
        }
        22 => {
            // Compare Before with Local：HEAD ↔ 工作区。
            model.queue(Command::OpenCompareWithLocalDiff {
                commit: "HEAD".to_string(),
                path: ctx.first.clone(),
            });
        }
        23 => {
            // Create Patch from Local Changes：先弹保存对话框再后台写文件。
            model.queue(Command::SavePatch(ctx.paths.clone()));
        }
        24 => {
            rt.dialog = DialogState::FileHistory {
                path: ctx.first.clone(),
                entries: Vec::new(),
                selected: 0,
                loaded: false,
            };
        }
        32 => model.queue(Command::OpenRepositoryVersion {
            path: ctx.first.clone(),
        }),
        30 => model.queue(Command::OpenDiffNewTab(ctx.first.clone())),
        31 => model.queue(Command::OpenIndexDiff(ctx.first.clone())),
        33 => {
            rt.dialog = DialogState::CherryPickChanges {
                paths: ctx.paths.clone(),
                target: String::new(),
                error: None,
            };
        }
        34 => {
            rt.dialog = DialogState::ExtractChanges {
                paths: ctx.paths.clone(),
                value: String::new(),
                error: None,
            };
        }
        35 => {
            let paths = discardable_paths(ctx);
            if paths.is_empty() {
                model.toast(
                    crate::tr!(
                        model,
                        "Use Delete for untracked files",
                        "未跟踪文件请使用「删除」"
                    )
                    .to_string(),
                    ToastKind::Warning,
                );
            } else {
                rt.dialog = DialogState::ConfirmDiscard { paths };
            }
        }
        36 => {
            rt.dialog = DialogState::GetFileFromRevision {
                path: ctx.first.clone(),
                revision: "HEAD".to_string(),
                error: None,
            };
        }
        37 => {
            let project = &model.projects[project_idx];
            let head_id = project
                .git
                .as_ref()
                .and_then(|git| git.head.as_ref())
                .map(|head| head.id.as_str());
            let head_row = head_id.and_then(|id| project.log.iter().find(|commit| commit.id == id));
            let parent = match head_row {
                Some(row) => row.parents.first().cloned(),
                // Log not loaded/filtered: let git resolve HEAD^ itself.
                None => Some("HEAD^".to_string()),
            };
            if let Some(parent) = parent {
                model.queue(Command::OpenCompareWithLocalDiff {
                    commit: parent,
                    path: ctx.first.clone(),
                });
            } else {
                model.toast(
                    crate::tr!(
                        model,
                        "The current branch has no parent commit",
                        "当前分支没有父提交"
                    )
                    .to_string(),
                    ToastKind::Warning,
                );
            }
        }
        idx if (50..=200).contains(&idx) => {
            let target_idx = (idx - 50) as usize;
            if let Some(target_list) = ctx.all_lists.get(target_idx) {
                model.queue(Command::ChangelistAssign {
                    paths: ctx.paths.clone(),
                    list: target_list.clone(),
                });
            }
        }
        _ => {}
    }
    true
}

fn render_change_menu_items(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    row_start: f32,
    id_salt: &str,
    items: &[ChangeMenuItem],
) -> Option<u8> {
    let p = theme.palette;
    let mut action: Option<u8> = None;
    for (i, item) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, row_start + i as f32 * MENU_ROW_H),
            Vec2::new(rect.width() - 8.0, MENU_ROW_H),
        );
        if item.code == CHANGE_MENU_SEP {
            ui.painter().line_segment(
                [
                    Pos2::new(row.left() + 8.0, row.center().y),
                    Pos2::new(row.right() - 8.0, row.center().y),
                ],
                egui::Stroke::new(1.0, p.menu_border),
            );
            continue;
        }
        if menu_row(
            ui,
            theme,
            row,
            ui.scope_id().with((id_salt, i)),
            &item.label,
            item.danger,
            item.enabled,
        ) {
            action = Some(item.code);
        }
    }
    action
}

#[allow(clippy::too_many_arguments)]
/// 提交抽屉 ⋮ 扩展入口：稳定版抽屉没有这些按钮，保留功能入口
/// （Stash Changes… / Stash Manager…）。
fn draw_commit_extras_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
) {
    let items = [
        (crate::tr!(model, "Stash Changes…", "贮藏更改…"), 0u8, false),
        (crate::tr!(model, "Stash Manager…", "贮藏管理器…"), 1, false),
        (crate::tr!(model, "Shelf Manager…", "架藏管理器…"), 2, false),
        (
            crate::tr!(model, "Import into Shelf…", "导入架藏…"),
            3,
            false,
        ),
    ];
    let height = items.len() as f32 * MENU_ROW_H + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 220.0, height);
    menu_frame(ui, rect, theme);
    let mut picked: Option<u8> = None;
    for (i, (label, code, danger)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, rect.top() + 4.0 + i as f32 * MENU_ROW_H),
            Vec2::new(rect.width() - 8.0, MENU_ROW_H),
        );
        if menu_row(
            ui,
            theme,
            row,
            ui.scope_id().with(("cextras", i)),
            label,
            *danger,
            true,
        ) {
            picked = Some(*code);
        }
    }
    let mut close = false;
    match picked {
        Some(0) => {
            let has_untracked = model
                .active_project()
                .is_some_and(|project| project.changes().iter().any(|change| change.untracked));
            rt.dialog = crate::runtime::DialogState::StashCreate {
                message: String::new(),
                include_untracked: has_untracked,
                error: None,
            };
            close = true;
        }
        Some(1) => {
            rt.dialog = crate::runtime::DialogState::StashManager {
                stashes: Vec::new(),
                selected: 0,
                error: None,
                loaded: false,
            };
            close = true;
        }
        Some(2) => {
            rt.dialog = crate::runtime::DialogState::ShelfManager {
                shelves: Vec::new(),
                selected: 0,
                error: None,
                loaded: false,
            };
            close = true;
        }
        Some(3) => {
            #[cfg(target_os = "macos")]
            {
                if let Ok(output) = std::process::Command::new("osascript")
                    .arg("-e")
                    .arg("POSIX path of (choose file with prompt \"Select patch file to import into Shelf\")")
                    .output()
                {
                    if output.status.success() {
                        let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        if !path_str.is_empty() {
                            let root = model
                                .active_project()
                                .map(|p| p.active_root().to_path_buf())
                                .unwrap_or_default();
                            match lazecode_git::shelf_import_patch(&root, std::path::Path::new(&path_str), None) {
                                Ok(name) => {
                                    model.toast(
                                        if model.settings.language.is_zh() {
                                            format!("已导入架藏: {name}")
                                        } else {
                                            format!("Imported shelf: {name}")
                                        },
                                        lazecode_core::ToastKind::Success,
                                    );
                                }
                                Err(err) => model.toast(err.to_string(), lazecode_core::ToastKind::Error),
                            }
                        }
                    }
                }
            }
            close = true;
        }
        _ => {}
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}
/// 贮藏条目右键菜单：弹出 / 应用 / Unstash… / 丢弃 / 清空 / 显示差异。
#[allow(clippy::too_many_arguments)]
fn draw_stash_entry_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    payload: Option<&str>,
) {
    let p = theme.palette;
    let stash = payload
        .map(str::to_string)
        .or_else(|| rt.stash_selected.clone());
    let message = stash
        .as_ref()
        .and_then(|stash| {
            rt.stash_entries
                .iter()
                .find(|entry| &entry.name == stash)
                .map(|entry| entry.message())
        })
        .unwrap_or_else(|| stash.clone().unwrap_or_default());
    // 显示差异需要一个文件：优先用该贮藏下当前选中的文件，否则第一条。
    let diff_target = stash.as_ref().and_then(|stash| {
        let paths = model.projects.get_mut(project_idx)?.stash_paths_for(stash);
        rt.stash_file_sel
            .as_ref()
            .filter(|path| paths.iter().any(|known| known == *path))
            .cloned()
            .or_else(|| paths.first().cloned())
    });
    let branch = model
        .projects
        .get(project_idx)
        .map(|project| project.branch())
        .unwrap_or_default();

    // (label, code, danger, enabled)
    let items: [(&str, u8, bool, bool); 7] = [
        (crate::tr!(model, "Pop", "弹出"), 0, false, stash.is_some()),
        (
            crate::tr!(model, "Apply", "应用"),
            1,
            false,
            stash.is_some(),
        ),
        (
            crate::tr!(model, "Unstash…", "Unstash…"),
            2,
            false,
            stash.is_some(),
        ),
        (crate::tr!(model, "Drop", "丢弃"), 3, true, stash.is_some()),
        (crate::tr!(model, "Clear", "清空"), 4, true, true),
        (
            crate::tr!(model, "Show Diff", "显示差异"),
            5,
            false,
            diff_target.is_some(),
        ),
        (
            crate::tr!(model, "Show Diff in a New Tab", "在新标签页显示差异"),
            6,
            false,
            diff_target.is_some(),
        ),
    ];
    let sep_after = 4usize;
    let height = items.len() as f32 * MENU_ROW_H + 12.0;
    let rect = popover_rect(rt, screen, menu.anchor, 230.0, height);
    menu_frame(ui, rect, theme);
    let mut picked: Option<u8> = None;
    let mut y = rect.top() + 4.0;
    for (i, (label, code, danger, enabled)) in items.iter().enumerate() {
        if i == sep_after {
            ui.painter().hline(
                (rect.left() + 8.0)..=(rect.right() - 8.0),
                y + 3.0,
                Stroke::new(1.0, p.border_soft),
            );
            y += 6.0;
        }
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, y),
            Vec2::new(rect.width() - 8.0, MENU_ROW_H),
        );
        if menu_row(
            ui,
            theme,
            row,
            ui.scope_id().with(("stash-entry", i)),
            label,
            *danger,
            *enabled,
        ) {
            picked = Some(*code);
        }
        y += MENU_ROW_H;
    }
    let mut close = false;
    match picked {
        Some(0) => {
            if let Some(stash) = stash {
                model.queue(Command::StashPop(stash));
            }
            close = true;
        }
        Some(1) => {
            if let Some(stash) = stash {
                model.queue(Command::StashApply(stash));
            }
            close = true;
        }
        Some(2) => {
            if let Some(stash) = stash {
                rt.dialog = crate::runtime::DialogState::Unstash {
                    stash,
                    message,
                    branch,
                    as_new_branch: String::new(),
                    pop: false,
                    reinstate_index: false,
                    error: None,
                };
            }
            close = true;
        }
        Some(3) => {
            if let Some(stash) = stash {
                model.queue(Command::StashDrop(stash));
            }
            close = true;
        }
        Some(4) => {
            model.queue(Command::StashClear);
            close = true;
        }
        Some(5) => {
            if let (Some(stash), Some(path)) = (stash, diff_target) {
                model.queue(Command::OpenStashDiff { stash, path });
            }
            close = true;
        }
        Some(6) => {
            if let (Some(stash), Some(path)) = (stash, diff_target) {
                model.queue(Command::OpenStashDiffNewTab { stash, path });
            }
            close = true;
        }
        _ => {}
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

/// 贮藏文件 / 目录右键菜单（对齐参考实现）：
/// Show Diff · Show Diff in a New Tab · Compare Before with Local ·
/// Edit Source · Open Repository Version · Cherry-Pick Selected Changes。
#[allow(clippy::too_many_arguments)]
fn draw_stash_file_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    payload: &str,
) {
    let mut segments = payload.split('\u{1}');
    let Some(stash) = segments.next().filter(|stash| !stash.is_empty()) else {
        return;
    };
    let kind = segments.next().unwrap_or("f");
    let path = segments.next().unwrap_or("").to_string();
    let stash = stash.to_string();

    // 目录 / 树根展开为该范围内的文件清单；文件就是它自己。
    let mut files: Vec<String> = Vec::new();
    if let Some(details) = model
        .projects
        .get_mut(project_idx)
        .and_then(|project| project.stash_details_for(&stash))
    {
        match kind {
            "t" => files.extend(details.tracked.iter().cloned()),
            "u" => files.extend(details.untracked.iter().cloned()),
            "d" => {
                let prefix = if path.is_empty() {
                    String::new()
                } else {
                    format!("{path}/")
                };
                files.extend(
                    details
                        .tracked
                        .iter()
                        .chain(details.untracked.iter())
                        .filter(|file| !prefix.is_empty() && file.starts_with(&prefix))
                        .cloned(),
                );
            }
            _ => {}
        }
    }
    if kind == "f" && !path.is_empty() {
        files = vec![path.clone()];
    }
    let target = files.first().cloned();
    let worktree_target = target.as_ref().and_then(|target| {
        model
            .active_project()
            .map(|project| project.active_root().join(target))
    });
    let target_exists = worktree_target.as_ref().is_some_and(|path| path.is_file());

    // (label, code, danger, enabled)
    let items: [(&str, u8, bool, bool); 6] = [
        (
            crate::tr!(model, "Show Diff", "显示差异"),
            0,
            false,
            target.is_some(),
        ),
        (
            crate::tr!(model, "Show Diff in a New Tab", "在新标签页显示差异"),
            1,
            false,
            target.is_some(),
        ),
        (
            crate::tr!(model, "Compare Before with Local", "与本地的前一版本比较"),
            2,
            false,
            target.is_some(),
        ),
        (
            crate::tr!(model, "Edit Source", "编辑源文件"),
            3,
            false,
            target_exists,
        ),
        (
            crate::tr!(model, "Open Repository Version", "打开仓库版本"),
            4,
            false,
            target.is_some(),
        ),
        (
            crate::tr!(model, "Cherry-Pick Selected Changes", "拣选选中更改"),
            5,
            false,
            !files.is_empty(),
        ),
    ];
    let height = items.len() as f32 * MENU_ROW_H + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 250.0, height);
    menu_frame(ui, rect, theme);
    let mut picked: Option<u8> = None;
    for (i, (label, code, danger, enabled)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, rect.top() + 4.0 + i as f32 * MENU_ROW_H),
            Vec2::new(rect.width() - 8.0, MENU_ROW_H),
        );
        if menu_row(
            ui,
            theme,
            row,
            ui.scope_id().with(("stash-file", i)),
            label,
            *danger,
            *enabled,
        ) {
            picked = Some(*code);
        }
    }
    let mut close = false;
    match picked {
        Some(0) => {
            if let Some(path) = target {
                model.queue(Command::OpenStashDiff { stash, path });
            }
            close = true;
        }
        Some(1) => {
            if let Some(path) = target {
                model.queue(Command::OpenStashDiffNewTab { stash, path });
            }
            close = true;
        }
        Some(2) => {
            if let Some(path) = target {
                model.queue(Command::OpenStashBeforeLocalDiff { stash, path });
            }
            close = true;
        }
        Some(3) => {
            if let Some(path) = worktree_target {
                model.queue(Command::OpenFileAt {
                    path,
                    line: 0,
                    character: 0,
                });
            }
            close = true;
        }
        Some(4) => {
            if let Some(path) = target {
                model.queue(Command::OpenRepositoryVersion { path });
            }
            close = true;
        }
        Some(5) => {
            if !files.is_empty() {
                model.queue(Command::CherryPickStashPaths {
                    stash,
                    paths: files,
                });
            }
            close = true;
        }
        _ => {}
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

fn draw_commit_view_options_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
) {
    let p = theme.palette;
    let menu_w = 210.0;
    let menu_h = 100.0;
    let rect = popover_rect(rt, screen, menu.anchor, menu_w, menu_h);
    menu_frame(ui, rect, theme);

    ui.painter().text(
        Pos2::new(rect.left() + 12.0, rect.top() + 12.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Group By", "分组依据"),
        FontId::proportional(10.5),
        p.text3,
    );

    let item_tree = Rect::from_min_size(
        Pos2::new(rect.left() + 4.0, rect.top() + 22.0),
        Vec2::new(rect.width() - 8.0, 24.0),
    );
    let mut close = false;
    if menu_row(
        ui,
        theme,
        item_tree,
        ui.scope_id().with("commit-view-tree"),
        &*crate::tr!(model, "Directory", "目录"),
        false,
        true,
    ) {
        rt.commit_tree_view = !rt.commit_tree_view;
        close = true;
    }
    if rt.commit_tree_view {
        ui.painter().text(
            Pos2::new(item_tree.right() - 34.0, item_tree.center().y),
            Align2::RIGHT_CENTER,
            "✓",
            FontId::proportional(11.0),
            p.menu_item_text,
        );
    }
    ui.painter().text(
        Pos2::new(item_tree.right() - 10.0, item_tree.center().y),
        Align2::RIGHT_CENTER,
        "^P",
        FontId::proportional(10.5),
        p.text3,
    );

    ui.painter().hline(
        (rect.left() + 6.0)..=(rect.right() - 6.0),
        rect.top() + 52.0,
        Stroke::new(1.0, p.border_soft),
    );

    ui.painter().text(
        Pos2::new(rect.left() + 12.0, rect.top() + 62.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Show", "显示"),
        FontId::proportional(10.5),
        p.text3,
    );

    let item_ignored = Rect::from_min_size(
        Pos2::new(rect.left() + 4.0, rect.top() + 70.0),
        Vec2::new(rect.width() - 8.0, 24.0),
    );
    if menu_row(
        ui,
        theme,
        item_ignored,
        ui.scope_id().with("commit-view-ignored"),
        &*crate::tr!(model, "Show Ignored Files", "显示被忽略的文件"),
        false,
        true,
    ) {
        rt.commit_show_ignored = !rt.commit_show_ignored;
        close = true;
    }
    if rt.commit_show_ignored {
        ui.painter().text(
            Pos2::new(item_ignored.right() - 12.0, item_ignored.center().y),
            Align2::RIGHT_CENTER,
            "✓",
            FontId::proportional(11.0),
            p.menu_item_text,
        );
    }

    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}
fn draw_compare_view_options_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
) {
    let p = theme.palette;
    let zh = model.settings.language.is_zh();
    let menu_w = 180.0;
    let menu_h = 56.0;
    let rect = popover_rect(rt, screen, menu.anchor, menu_w, menu_h);
    menu_frame(ui, rect, theme);

    ui.painter().text(
        Pos2::new(rect.left() + 10.0, rect.top() + 11.0),
        Align2::LEFT_CENTER,
        if zh { "分组方式" } else { "Group by" },
        FontId::proportional(10.5),
        p.text3,
    );

    let item = Rect::from_min_size(
        Pos2::new(rect.left() + 4.0, rect.top() + 20.0),
        Vec2::new(menu_w - 8.0, 24.0),
    );
    let mut close = false;
    let is_grouped = model
        .projects
        .get(project_idx)
        .and_then(|p| p.compare_session.as_ref())
        .map(|s| s.group_by_dir)
        .unwrap_or(false);
    if menu_row(
        ui,
        theme,
        item,
        ui.scope_id().with("cmp-view-dir"),
        if zh { "目录" } else { "Directory" },
        false,
        true,
    ) {
        if let Some(session) = model
            .projects
            .get_mut(project_idx)
            .and_then(|p| p.compare_session.as_mut())
        {
            session.group_by_dir = !session.group_by_dir;
        }
        close = true;
    }
    if is_grouped {
        ui.painter().text(
            Pos2::new(item.left() + 10.0, item.center().y),
            Align2::LEFT_CENTER,
            "✓",
            FontId::proportional(11.0),
            p.menu_item_text,
        );
    }
    ui.painter().text(
        Pos2::new(item.right() - 8.0, item.center().y),
        Align2::RIGHT_CENTER,
        "^P",
        FontId::proportional(10.5),
        p.text3,
    );

    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

/// 架藏右键菜单（稳定版 Shelf 页签）：恢复 / 补丁 / 重命名 / 删除。
#[allow(clippy::too_many_arguments)]
fn draw_shelf_context_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    payload: &str,
) {
    let mut segments = payload.split('\u{1}');
    let Some(name) = segments.next().filter(|name| !name.is_empty()) else {
        return;
    };
    let name = name.to_string();
    let deleted = segments.next() == Some("1");
    let paths: Vec<String> = segments
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect();
    let _ = project_idx;

    // (label, code, danger, enabled)
    let mut items: Vec<(String, u8, bool, bool)> = Vec::new();
    if deleted {
        items.push((
            crate::tr!(model, "Restore", "恢复").to_string(),
            0,
            false,
            true,
        ));
        items.push((
            crate::tr!(model, "Delete…", "删除…").to_string(),
            3,
            true,
            true,
        ));
    } else {
        items.push((
            crate::tr!(model, "Unshelve…", "恢复贮藏…").to_string(),
            1,
            false,
            true,
        ));
        items.push((
            crate::tr!(model, "Copy as Patch to Clipboard", "以补丁复制到剪贴板").to_string(),
            2,
            false,
            true,
        ));
        items.push((
            crate::tr!(model, "Rename…", "重命名…").to_string(),
            4,
            false,
            paths.is_empty(),
        ));
        items.push((
            crate::tr!(model, "Delete…", "删除…").to_string(),
            3,
            true,
            true,
        ));
    }
    let height = items.len() as f32 * MENU_ROW_H + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 260.0, height);
    menu_frame(ui, rect, theme);
    let mut picked: Option<u8> = None;
    for (i, (label, code, danger, enabled)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 4.0, rect.top() + 4.0 + i as f32 * MENU_ROW_H),
            Vec2::new(rect.width() - 8.0, MENU_ROW_H),
        );
        if menu_row(
            ui,
            theme,
            row,
            ui.scope_id().with(("shelf-ctx", i)),
            label,
            *danger,
            *enabled,
        ) {
            picked = Some(*code);
        }
    }
    let mut close = false;
    match picked {
        // Restore（最近删除 → 恢复）
        Some(0) => {
            model.queue(Command::ShelfRestoreDeleted(name.clone()));
            rt.shelf_loaded = None;
            close = true;
        }
        // Unshelve…
        Some(1) => {
            {
                let files: Vec<(String, bool)> =
                    paths.iter().map(|path| (path.clone(), true)).collect();
                rt.dialog = crate::runtime::DialogState::Unshelve {
                    name: name.clone(),
                    files,
                    target: None,
                    new_name: name.clone(),
                    error: None,
                };
            }
            close = true;
        }
        // Copy as Patch（补丁读取是本地小文件，同步可接受）
        Some(2) => {
            if let Some(project) = model.projects.get(model.active_project.unwrap_or(0)) {
                match lazecode_git::shelf_patch(project.active_root(), &name) {
                    Ok(patch) => {
                        ui.ctx().copy_text(patch);
                        let zh = model.settings.language.is_zh();
                        model.toast(
                            if zh {
                                "补丁已复制到剪贴板"
                            } else {
                                "Patch copied to clipboard"
                            },
                            ToastKind::Success,
                        );
                    }
                    Err(err) => model.toast(err.to_string(), ToastKind::Error),
                }
            }
            close = true;
        }
        // Delete…（确认）
        Some(3) => {
            rt.dialog = crate::runtime::DialogState::ConfirmDeleteShelf {
                name: name.clone(),
                purge: deleted,
            };
            close = true;
        }
        // Rename…
        Some(4) => {
            rt.dialog = crate::runtime::DialogState::RenameShelf {
                from: name.clone(),
                value: name.clone(),
                error: None,
            };
            close = true;
        }
        _ => {}
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_change_file_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
    payload: &str,
) {
    let Some(project) = model.projects.get(project_idx) else {
        return;
    };
    // payload: 多选时以 \u{1} 连接的仓库相对路径。
    let paths: Vec<String> = payload
        .split('\u{1}')
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect();
    if paths.is_empty() {
        return;
    }
    let (items, ctx) = change_menu_items(model, project, &paths);

    let height = items.len() as f32 * MENU_ROW_H + 8.0;
    let rect = popover_rect(rt, screen, menu.anchor, 260.0, height);
    menu_frame(ui, rect, theme);

    // 稳定版右键菜单没有标题行，直接列菜单项。
    let mut close = false;
    if let Some(code) =
        render_change_menu_items(ui, theme, rect, rect.top() + 8.0, "cfile_ctx", &items)
    {
        close = apply_change_menu_action(ui, model, rt, &ctx, code);
    }

    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

fn draw_branch_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
) {
    let p = theme.palette;
    let Some(index) = model.active_project else {
        return;
    };
    let current_branch = model.projects[index].branch();
    let mut query = model.projects[index].branch_query.clone();
    let query_lower = query.trim().to_lowercase();
    let has_filter = !query_lower.is_empty();

    let all_branches = model.projects[index].branches.clone();
    let all_tags = model.projects[index].tags.clone();

    let local_branches: Vec<lazecode_git::BranchInfo> = all_branches
        .iter()
        .filter(|b| {
            !b.remote && (query_lower.is_empty() || b.name.to_lowercase().contains(&query_lower))
        })
        .cloned()
        .collect();

    let remote_branches: Vec<lazecode_git::BranchInfo> = all_branches
        .iter()
        .filter(|b| {
            b.remote && (query_lower.is_empty() || b.name.to_lowercase().contains(&query_lower))
        })
        .cloned()
        .collect();

    let filtered_tags: Vec<String> = all_tags
        .iter()
        .filter(|t| query_lower.is_empty() || t.to_lowercase().contains(&query_lower))
        .cloned()
        .collect();

    let recent_branches: Vec<lazecode_git::BranchInfo> = {
        let mut list = Vec::new();
        if let Some(cur) = local_branches.iter().find(|b| b.name == current_branch) {
            list.push(cur.clone());
        }
        for b in &local_branches {
            if b.name != current_branch && list.len() < 4 {
                list.push(b.clone());
            }
        }
        list
    };

    // 下拉菜单最大高度为app窗口高度的80%，内容未超限时用内容撑起高度
    let top_h = if has_filter { 42.0 } else { 148.0 };
    let mut sections_h = 0.0f32;
    if !has_filter && !recent_branches.is_empty() {
        sections_h += 22.0;
        if !rt.branch_sections_collapsed.contains("recent") {
            sections_h += recent_branches.len() as f32 * 26.0 + 4.0;
        }
    }
    if !local_branches.is_empty() {
        sections_h += 22.0;
        if has_filter || !rt.branch_sections_collapsed.contains("local") {
            sections_h += local_branches.len() as f32 * 26.0 + 4.0;
        }
    }
    if !remote_branches.is_empty() {
        sections_h += 22.0;
        if has_filter || !rt.branch_sections_collapsed.contains("remote") {
            sections_h += remote_branches.len() as f32 * 26.0 + 4.0;
        }
    }
    if !filtered_tags.is_empty() {
        sections_h += 22.0;
        if has_filter || !rt.branch_sections_collapsed.contains("tags") {
            sections_h += filtered_tags.len() as f32 * 26.0 + 4.0;
        }
    }
    let max_height = (screen.height() * 0.8).max(180.0);
    let needed_height = top_h + sections_h + 16.0;
    let height = needed_height.clamp(140.0, max_height);
    let rect = popover_rect(rt, screen, menu.anchor, 330.0, height);
    menu_frame(ui, rect, theme);

    let mut close = false;

    // 1. Search Bar Header
    let input_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 10.0, rect.top() + 8.0),
        Vec2::new(rect.width() - 20.0, 28.0),
    );
    ui.painter()
        .rect_filled(input_rect, egui::CornerRadius::same(5), p.panel2);
    ui.painter().rect_stroke(
        input_rect,
        egui::CornerRadius::same(5),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );

    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(input_rect.left() + 14.0, input_rect.center().y),
            Vec2::splat(12.0),
        ),
        Icon::Search,
        p.text3,
    );

    let gear_rect = Rect::from_center_size(
        Pos2::new(input_rect.right() - 14.0, input_rect.center().y),
        Vec2::splat(18.0),
    );
    let gear_resp = ui.interact(
        gear_rect,
        ui.scope_id().with("branch-menu-gear"),
        Sense::click(),
    );
    if gear_resp.hovered() {
        ui.painter()
            .rect_filled(gear_rect, egui::CornerRadius::same(3), p.hover);
    }
    paint_icon(
        ui.painter(),
        gear_rect.shrink(3.0),
        Icon::Settings,
        if gear_resp.hovered() { p.text } else { p.text3 },
    );
    if gear_resp.clicked() {
        rt.settings_open = true;
        close = true;
    }

    let text_rect = Rect::from_min_max(
        Pos2::new(input_rect.left() + 26.0, input_rect.top() + 2.0),
        Pos2::new(input_rect.right() - 26.0, input_rect.bottom() - 2.0),
    );
    let query_resp = ui.put(
        text_rect,
        TextEdit::singleline(&mut query)
            .frame(egui::Frame::NONE)
            .align(egui::Align2::LEFT_CENTER)
            .hint_text(crate::tr!(
                model,
                "Search for branches and actions",
                "过滤分支与操作"
            ))
            .font(FontId::proportional(12.0)),
    );
    if query_resp.changed() {
        model.projects[index].branch_query = query.clone();
        rt.branch_submenu = None;
    }
    let mut cur_y = input_rect.bottom() + 6.0;

    // 2. Top Action Rows (when filter is empty)
    if !has_filter {
        struct TopAction {
            label: &'static str,
            label_zh: &'static str,
            icon: Icon,
            icon_accent: bool,
            shortcut: &'static str,
            id: u8,
        }
        let actions = [
            TopAction {
                label: "Update Project...",
                label_zh: "更新项目...",
                icon: Icon::ArrowDown,
                icon_accent: true,
                shortcut: "⌘T",
                id: 1,
            },
            TopAction {
                label: "Commit...",
                label_zh: "提交...",
                icon: Icon::Commit,
                icon_accent: false,
                shortcut: "⌘K",
                id: 2,
            },
            TopAction {
                label: "Push...",
                label_zh: "推送...",
                icon: Icon::ArrowUp,
                icon_accent: false,
                shortcut: "⇧⌘K",
                id: 3,
            },
            TopAction {
                label: "New Branch...",
                label_zh: "新建分支...",
                icon: Icon::Plus,
                icon_accent: false,
                shortcut: "⌥N",
                id: 4,
            },
        ];

        for act in &actions {
            let row = Rect::from_min_size(
                Pos2::new(rect.left() + 6.0, cur_y),
                Vec2::new(rect.width() - 12.0, 24.0),
            );
            let resp = ui.interact(row, ui.scope_id().with(("top-act", act.id)), Sense::click());
            if resp.hovered() {
                ui.painter()
                    .rect_filled(row, egui::CornerRadius::same(3), p.hover);
            }
            paint_icon(
                ui.painter(),
                Rect::from_center_size(
                    Pos2::new(row.left() + 14.0, row.center().y),
                    Vec2::splat(13.0),
                ),
                act.icon,
                if act.icon_accent { p.accent } else { p.text2 },
            );
            ui.painter().text(
                Pos2::new(row.left() + 28.0, row.center().y),
                Align2::LEFT_CENTER,
                crate::tr!(model, act.label, act.label_zh),
                FontId::proportional(12.5),
                p.text,
            );
            if !act.shortcut.is_empty() {
                ui.painter().text(
                    Pos2::new(row.right() - 8.0, row.center().y),
                    Align2::RIGHT_CENTER,
                    act.shortcut,
                    FontId::proportional(11.0),
                    p.text3,
                );
            }
            if resp.clicked() {
                match act.id {
                    1 => {
                        rt.dialog = DialogState::Pull {
                            mode: lazecode_git::PullMode::Merge,
                            status: None,
                        };
                        close = true;
                    }
                    2 => {
                        model.ui.sidebar_view = lazecode_core::SidebarView::Git;
                        rt.commit_tab = crate::runtime::CommitTab::Commit;
                        close = true;
                    }
                    3 => {
                        if let Some(project) = model.active_project() {
                            rt.dialog =
                                push_dialog_state(project, "origin".into(), current_branch.clone());
                        }
                        close = true;
                    }
                    4 => {
                        rt.dialog = DialogState::NewBranch {
                            value: String::new(),
                            start_point: None,
                            checkout: true,
                            error: None,
                        };
                        close = true;
                    }
                    _ => {}
                }
            }
            cur_y += 24.0;
        }

        ui.painter().line_segment(
            [
                Pos2::new(rect.left() + 6.0, cur_y + 3.0),
                Pos2::new(rect.right() - 6.0, cur_y + 3.0),
            ],
            Stroke::new(1.0, p.border),
        );
        cur_y += 8.0;
    }

    // 3. Scrollable Sections (Recent, Local, Remote, Tags)
    let scroll_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 6.0, cur_y),
        Pos2::new(rect.right() - 6.0, rect.bottom() - 6.0),
    );

    let mut hovered_branch_row: Option<(lazecode_git::BranchInfo, Rect)> = None;
    let mut delete_tag: Option<String> = None;
    let mut checkout_tag: Option<String> = None;

    crate::app_ui::region(ui, scroll_rect, "branch-menu-sections", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("branch-menu-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut draw_section =
                    |title: &str, branches: &[lazecode_git::BranchInfo], sec_key: &'static str| {
                        if branches.is_empty() {
                            return;
                        }
                        let is_collapsed =
                            !has_filter && rt.branch_sections_collapsed.contains(sec_key);
                        let (hdr_rect, hdr_resp) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 20.0),
                            Sense::click(),
                        );
                        if hdr_resp.hovered() {
                            ui.painter().rect_filled(
                                hdr_rect,
                                egui::CornerRadius::same(3),
                                p.hover,
                            );
                        }
                        paint_icon(
                            ui.painter(),
                            Rect::from_center_size(
                                Pos2::new(hdr_rect.left() + 6.0, hdr_rect.center().y),
                                Vec2::splat(9.0),
                            ),
                            if is_collapsed {
                                Icon::ChevronRight
                            } else {
                                Icon::ChevronDown
                            },
                            p.text3,
                        );
                        ui.painter().text(
                            Pos2::new(hdr_rect.left() + 16.0, hdr_rect.center().y),
                            Align2::LEFT_CENTER,
                            title,
                            FontId::proportional(11.0),
                            p.text3,
                        );
                        if hdr_resp.clicked() {
                            if rt.branch_sections_collapsed.contains(sec_key) {
                                rt.branch_sections_collapsed.remove(sec_key);
                            } else {
                                rt.branch_sections_collapsed.insert(sec_key.to_string());
                            }
                        }

                        if is_collapsed {
                            ui.add_space(2.0);
                            return;
                        }

                        for b in branches {
                            let (row_rect, resp) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 26.0),
                                Sense::click(),
                            );
                            let is_current = b.name == current_branch;
                            let row_screen = Rect::from_min_size(row_rect.min, row_rect.size());
                            let active_submenu = rt
                                .branch_submenu
                                .as_ref()
                                .map(|s| s.anchor == row_screen)
                                .unwrap_or(false);

                            if is_current {
                                ui.painter().rect_filled(
                                    row_rect,
                                    egui::CornerRadius::same(3),
                                    Color32::from_rgb(33, 66, 131),
                                );
                            } else if resp.hovered() || active_submenu {
                                ui.painter().rect_filled(
                                    row_rect,
                                    egui::CornerRadius::same(3),
                                    p.hover,
                                );
                            }

                            if is_current {
                                paint_icon(
                                    ui.painter(),
                                    Rect::from_center_size(
                                        Pos2::new(row_rect.left() + 10.0, row_rect.center().y),
                                        Vec2::splat(12.0),
                                    ),
                                    Icon::Tag,
                                    Color32::from_rgb(229, 192, 123),
                                );
                            } else {
                                paint_icon(
                                    ui.painter(),
                                    Rect::from_center_size(
                                        Pos2::new(row_rect.left() + 10.0, row_rect.center().y),
                                        Vec2::splat(12.0),
                                    ),
                                    Icon::Branch,
                                    if b.remote { p.text3 } else { p.text2 },
                                );
                            }

                            let name_x = row_rect.left() + 22.0;
                            let mut end_x = row_rect.right() - 18.0;

                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(row_rect.right() - 8.0, row_rect.center().y),
                                    Vec2::splat(10.0),
                                ),
                                Icon::ChevronRight,
                                p.text3,
                            );

                            if let Some(ref tr) = b.tracking {
                                let tr_font = FontId::proportional(11.0);
                                let tr_w = ui
                                    .painter()
                                    .layout_no_wrap(tr.clone(), tr_font.clone(), p.text3)
                                    .size()
                                    .x;
                                end_x -= tr_w + 6.0;
                                ui.painter().text(
                                    Pos2::new(end_x, row_rect.center().y),
                                    Align2::LEFT_CENTER,
                                    tr,
                                    tr_font,
                                    p.text3,
                                );
                            }

                            if b.ahead > 0 {
                                let badge_text = format!("↗{}", b.ahead);
                                let badge_font = FontId::proportional(11.0);
                                let badge_w = ui
                                    .painter()
                                    .layout_no_wrap(
                                        badge_text.clone(),
                                        badge_font.clone(),
                                        Color32::from_rgb(46, 160, 67),
                                    )
                                    .size()
                                    .x;
                                end_x -= badge_w + 6.0;
                                ui.painter().text(
                                    Pos2::new(end_x, row_rect.center().y),
                                    Align2::LEFT_CENTER,
                                    badge_text,
                                    badge_font,
                                    Color32::from_rgb(46, 160, 67),
                                );
                            }

                            text_clipped(
                                ui.painter(),
                                Rect::from_min_max(
                                    Pos2::new(name_x, row_rect.top()),
                                    Pos2::new(end_x.max(name_x + 10.0), row_rect.bottom()),
                                ),
                                &b.name,
                                FontId::proportional(12.5),
                                if is_current {
                                    Color32::WHITE
                                } else if b.remote {
                                    p.text2
                                } else {
                                    p.text
                                },
                                Align2::LEFT_CENTER,
                            );

                            let row_screen = Rect::from_min_size(row_rect.min, row_rect.size());
                            if resp.clicked() {
                                let _ = sec_key;
                                hovered_branch_row = Some((b.clone(), row_screen));
                            }
                        }
                        ui.add_space(4.0);
                    };

                if !has_filter && !recent_branches.is_empty() {
                    draw_section(
                        crate::tr!(model, "Recent", "最近"),
                        &recent_branches,
                        "recent",
                    );
                }

                draw_section(crate::tr!(model, "Local", "本地"), &local_branches, "local");

                draw_section(
                    crate::tr!(model, "Remote", "远程"),
                    &remote_branches,
                    "remote",
                );

                if !filtered_tags.is_empty() {
                    let is_collapsed = !has_filter && rt.branch_sections_collapsed.contains("tags");
                    let (hdr_rect, hdr_resp) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::click());
                    if hdr_resp.hovered() {
                        ui.painter()
                            .rect_filled(hdr_rect, egui::CornerRadius::same(3), p.hover);
                    }
                    paint_icon(
                        ui.painter(),
                        Rect::from_center_size(
                            Pos2::new(hdr_rect.left() + 6.0, hdr_rect.center().y),
                            Vec2::splat(9.0),
                        ),
                        if is_collapsed {
                            Icon::ChevronRight
                        } else {
                            Icon::ChevronDown
                        },
                        p.text3,
                    );
                    ui.painter().text(
                        Pos2::new(hdr_rect.left() + 16.0, hdr_rect.center().y),
                        Align2::LEFT_CENTER,
                        crate::tr!(model, "Tags", "标签"),
                        FontId::proportional(11.0),
                        p.text3,
                    );
                    if hdr_resp.clicked() {
                        if rt.branch_sections_collapsed.contains("tags") {
                            rt.branch_sections_collapsed.remove("tags");
                        } else {
                            rt.branch_sections_collapsed.insert("tags".to_string());
                        }
                    }

                    if !is_collapsed {
                        for tag in &filtered_tags {
                            let (row_rect, resp) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 26.0),
                                Sense::click(),
                            );
                            if resp.hovered() {
                                ui.painter().rect_filled(
                                    row_rect,
                                    egui::CornerRadius::same(3),
                                    p.hover,
                                );
                            }

                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(row_rect.left() + 10.0, row_rect.center().y),
                                    Vec2::splat(12.0),
                                ),
                                Icon::Tag,
                                Color32::from_rgb(229, 192, 123),
                            );

                            text_clipped(
                                ui.painter(),
                                Rect::from_min_max(
                                    Pos2::new(row_rect.left() + 22.0, row_rect.top()),
                                    Pos2::new(row_rect.right() - 26.0, row_rect.bottom()),
                                ),
                                tag,
                                FontId::proportional(12.5),
                                p.text,
                                Align2::LEFT_CENTER,
                            );

                            let trash_rect = Rect::from_center_size(
                                Pos2::new(row_rect.right() - 10.0, row_rect.center().y),
                                Vec2::splat(16.0),
                            );
                            let trash_resp = ui.interact(
                                trash_rect,
                                ui.scope_id().with(("del-tag", tag)),
                                Sense::click(),
                            );
                            let trash_color = if trash_resp.hovered() {
                                ui.painter().rect_filled(
                                    trash_rect,
                                    egui::CornerRadius::same(3),
                                    p.hover,
                                );
                                p.rose
                            } else {
                                p.text3
                            };
                            paint_icon(
                                ui.painter(),
                                trash_rect.shrink(3.0),
                                Icon::Trash,
                                trash_color,
                            );
                            if trash_resp.clicked() {
                                delete_tag = Some(tag.clone());
                            } else if resp.secondary_clicked() {
                                rt.open_menu(
                                    MenuKind::TagContext { project: index },
                                    row_rect,
                                    Some(tag.clone()),
                                );
                                close = true;
                            } else if resp.clicked() {
                                checkout_tag = Some(tag.clone());
                            }
                        }
                    }
                    ui.add_space(4.0);
                }
            });
    });

    if let Some((b, row_rect)) = hovered_branch_row {
        rt.branch_submenu = Some(crate::runtime::BranchSubmenuState {
            branch_name: b.name.clone(),
            is_current: b.name == current_branch,
            is_remote: b.remote,
            anchor: row_rect,
        });
    }

    if let Some(tag) = delete_tag {
        model.queue(Command::DeleteTag(tag));
        close = true;
    }
    if let Some(tag) = checkout_tag {
        model.queue(Command::CheckoutRevision(tag));
        close = true;
    }

    // 4. Cascade Submenu Rendering
    let mut sub_rect_opt: Option<Rect> = None;
    if let Some(ref sub) = rt.branch_submenu.clone() {
        let bname = sub.branch_name.clone();
        let is_curr = sub.is_current;
        let is_rem = sub.is_remote;

        enum SubItem {
            Row(&'static str, &'static str, u8, bool, &'static str),
            Sep,
        }

        let items: Vec<SubItem> = if is_curr {
            vec![
                SubItem::Row("New Branch from", "基于此新建分支", 1, false, ""),
                SubItem::Row(
                    "Show Diff with Working Tree",
                    "与工作区比较差异",
                    2,
                    false,
                    "",
                ),
                SubItem::Sep,
                SubItem::Row("Update", "更新", 3, false, ""),
                SubItem::Row("Push...", "推送...", 4, false, ""),
                SubItem::Sep,
                SubItem::Row("Rename...", "重命名...", 5, false, "F2"),
            ]
        } else if is_rem {
            vec![
                SubItem::Row("Checkout", "检出", 10, false, ""),
                SubItem::Row("New Branch from", "基于此新建分支", 1, false, ""),
                SubItem::Row("Checkout and Rebase onto", "检出并变基到", 11, false, ""),
                SubItem::Sep,
                SubItem::Row("Compare with Current", "与当前分支比较", 12, false, ""),
                SubItem::Row(
                    "Show Diff with Working Tree",
                    "与工作区比较差异",
                    2,
                    false,
                    "",
                ),
                SubItem::Sep,
                SubItem::Row("Rebase onto", "变基到当前分支", 13, false, ""),
                SubItem::Row("Merge into current", "合并到当前分支", 14, false, ""),
                SubItem::Sep,
                SubItem::Row(
                    "Pull into current Using Rebase",
                    "使用变基拉取到当前分支",
                    15,
                    false,
                    "",
                ),
                SubItem::Row(
                    "Pull into current Using Merge",
                    "使用合并拉取到当前分支",
                    16,
                    false,
                    "",
                ),
                SubItem::Sep,
                SubItem::Row("Delete", "删除", 20, true, ""),
            ]
        } else {
            vec![
                SubItem::Row("Checkout", "检出", 10, false, ""),
                SubItem::Row("New Branch from", "基于此新建分支", 1, false, ""),
                SubItem::Row("Checkout and Rebase onto", "检出并变基到", 11, false, ""),
                SubItem::Sep,
                SubItem::Row("Compare with Current", "与当前分支比较", 12, false, ""),
                SubItem::Row(
                    "Show Diff with Working Tree",
                    "与工作区比较差异",
                    2,
                    false,
                    "",
                ),
                SubItem::Sep,
                SubItem::Row("Rebase onto", "变基到当前分支", 13, false, ""),
                SubItem::Row("Merge into current", "合并到当前分支", 14, false, ""),
                SubItem::Sep,
                SubItem::Row("Update", "更新", 3, false, ""),
                SubItem::Row("Push...", "推送...", 4, false, ""),
                SubItem::Sep,
                SubItem::Row("Rename...", "重命名...", 5, false, "F2"),
                SubItem::Row("Delete", "删除", 20, true, ""),
            ]
        };

        let sub_w = 340.0f32.min((screen.width() - 16.0).max(220.0));
        let sub_h = items
            .iter()
            .map(|it| match it {
                SubItem::Row(..) => 24.0,
                SubItem::Sep => 9.0,
            })
            .sum::<f32>()
            + 12.0;

        let sub_x = if rect.right() + 4.0 + sub_w <= screen.right() - 4.0 {
            rect.right() + 4.0
        } else {
            (rect.left() - sub_w - 4.0).max(screen.left() + 4.0)
        };
        let sub_y = sub.anchor.top().clamp(
            screen.top() + 8.0,
            (screen.bottom() - sub_h - 8.0).max(screen.top() + 8.0),
        );
        let sub_rect = Rect::from_min_size(Pos2::new(sub_x, sub_y), Vec2::new(sub_w, sub_h));
        sub_rect_opt = Some(sub_rect);
        // Register for the shell's wheel interception (next frame), matching
        // the tree context submenu.
        rt.menu_hit_rects.push(sub_rect);

        menu_frame(ui, sub_rect, theme);

        let mut action: Option<u8> = None;
        let mut sy = sub_rect.top() + 6.0;

        for (i, item) in items.iter().enumerate() {
            match item {
                SubItem::Sep => {
                    ui.painter().line_segment(
                        [
                            Pos2::new(sub_rect.left() + 6.0, sy + 4.0),
                            Pos2::new(sub_rect.right() - 6.0, sy + 4.0),
                        ],
                        Stroke::new(1.0, p.border),
                    );
                    sy += 9.0;
                }
                SubItem::Row(label, label_zh, code, is_danger, shortcut) => {
                    let row = Rect::from_min_size(
                        Pos2::new(sub_rect.left() + 6.0, sy),
                        Vec2::new(sub_rect.width() - 12.0, 24.0),
                    );
                    let resp =
                        ui.interact(row, ui.scope_id().with(("bsub", i, *code)), Sense::click());
                    if resp.hovered() {
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::same(3), p.hover);
                    }

                    let display_text = match *code {
                        1 => format!(
                            "{} '{bname}'...",
                            crate::tr!(model, "New Branch from", "基于此新建分支")
                        ),
                        10 => format!("{} '{bname}'", crate::tr!(model, "Checkout", "检出")),
                        11 => format!(
                            "{} '{current_branch}'...",
                            crate::tr!(model, "Checkout and Rebase onto", "检出并变基到")
                        ),
                        13 => format!(
                            "{} '{current_branch}' onto '{bname}'...",
                            crate::tr!(model, "Rebase", "变基")
                        ),
                        14 => format!(
                            "{} '{bname}' into '{current_branch}'",
                            crate::tr!(model, "Merge", "合并")
                        ),
                        15 => format!(
                            "{} '{}' {}",
                            crate::tr!(model, "Pull into", "拉取到"),
                            current_branch,
                            crate::tr!(model, "Using Rebase", "使用变基")
                        ),
                        16 => format!(
                            "{} '{}' {}",
                            crate::tr!(model, "Pull into", "拉取到"),
                            current_branch,
                            crate::tr!(model, "Using Merge", "使用合并")
                        ),
                        _ => crate::tr!(model, *label, *label_zh).to_string(),
                    };

                    let text_color = if *is_danger { p.rose } else { p.text };
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(row.left() + 10.0, row.top()),
                            Pos2::new(row.right() - 8.0, row.bottom()),
                        ),
                        &display_text,
                        FontId::proportional(12.0),
                        text_color,
                        Align2::LEFT_CENTER,
                    );

                    if !shortcut.is_empty() {
                        ui.painter().text(
                            Pos2::new(row.right() - 8.0, row.center().y),
                            Align2::RIGHT_CENTER,
                            *shortcut,
                            FontId::proportional(11.0),
                            p.text3,
                        );
                    }

                    if resp.clicked() {
                        action = Some(*code);
                    }
                    sy += 24.0;
                }
            }
        }

        if let Some(code) = action {
            match code {
                1 => {
                    rt.dialog = DialogState::NewBranch {
                        value: String::new(),
                        start_point: Some(bname.clone()),
                        checkout: true,
                        error: None,
                    };
                    close = true;
                }
                2 => {
                    model.queue(Command::OpenCompareWithLocal {
                        commit: bname.clone(),
                    });
                    close = true;
                }
                3 => {
                    if is_curr {
                        rt.dialog = DialogState::Pull {
                            mode: lazecode_git::PullMode::Merge,
                            status: None,
                        };
                    } else {
                        model.queue(Command::FetchAll);
                    }
                    close = true;
                }
                4 => {
                    if let Some(project) = model.active_project() {
                        rt.dialog = push_dialog_state(project, "origin".into(), bname.clone());
                    }
                    close = true;
                }
                5 => {
                    rt.dialog = DialogState::RenameBranch {
                        from: bname.clone(),
                        value: bname.clone(),
                        error: None,
                    };
                    close = true;
                }
                10 => {
                    model.queue(Command::CheckoutBranch(bname.clone()));
                    close = true;
                }
                11 => {
                    model.queue(Command::CheckoutBranch(bname.clone()));
                    model.queue(Command::Rebase {
                        onto: current_branch.clone(),
                    });
                    close = true;
                }
                12 => {
                    crate::editor::open_compare_page(model, current_branch.clone(), bname.clone());
                    close = true;
                }
                13 => {
                    model.queue(Command::Rebase {
                        onto: bname.clone(),
                    });
                    close = true;
                }
                14 => {
                    model.queue(Command::Merge {
                        ref_name: bname.clone(),
                    });
                    close = true;
                }
                15 => {
                    rt.dialog = DialogState::Pull {
                        mode: lazecode_git::PullMode::Rebase,
                        status: None,
                    };
                    close = true;
                }
                16 => {
                    rt.dialog = DialogState::Pull {
                        mode: lazecode_git::PullMode::Merge,
                        status: None,
                    };
                    close = true;
                }
                20 => {
                    rt.dialog = DialogState::DeleteBranch {
                        name: bname.clone(),
                        force: false,
                        remote: is_rem,
                        error: None,
                    };
                    close = true;
                }
                _ => {}
            }
            rt.branch_submenu = None;
        }
    }

    let dismiss_main = should_dismiss(ui, rect, menu.opened_frame, rt.frame_count);
    let in_sub = sub_rect_opt.is_some_and(|sr| {
        ui.input(|i| i.pointer.hover_pos())
            .is_some_and(|p| sr.contains(p))
    });
    if dismiss_main && !in_sub {
        close = true;
        rt.branch_submenu = None;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

fn draw_agent_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    anchor: Rect,
) {
    let p = theme.palette;
    let options = model.settings.agents.options.clone();
    let visible_rows = options.len().min(9);
    let height = 52.0 + visible_rows as f32 * 46.0 + 24.0;
    let rect = popover_rect(rt, screen, anchor, 320.0, height);
    menu_frame(ui, rect, theme);
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, rect.top() + 20.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Start agent", "启动 agent"),
        FontId::proportional(11.0),
        p.text3,
    );
    let rows = Rect::from_min_max(
        Pos2::new(rect.left() + 6.0, rect.top() + 34.0),
        Pos2::new(rect.right() - 6.0, rect.bottom() - 10.0),
    );
    let mut launch: Option<(String, String)> = None;
    let mut close = false;
    crate::app_ui::region(ui, rows, "agent-menu", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("agent-menu-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for option in &options {
                    let (row, response) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 44.0), Sense::click());
                    if response.hovered() {
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::same(5), p.hover);
                    }
                    let icon_rect = Rect::from_center_size(
                        Pos2::new(row.left() + 20.0, row.center().y),
                        Vec2::splat(24.0),
                    );
                    ui.painter()
                        .rect_filled(icon_rect, egui::CornerRadius::same(6), p.accent_soft);
                    paint_icon(ui.painter(), icon_rect.shrink(5.0), Icon::Spark, p.accent);
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(row.left() + 38.0, row.top() + 5.0),
                            Pos2::new(row.right() - 12.0, row.center().y + 3.0),
                        ),
                        &option.name,
                        FontId::proportional(11.5),
                        p.text,
                        Align2::LEFT_CENTER,
                    );
                    let description = if option.description.is_empty() {
                        if option.command.is_empty() {
                            crate::tr!(model, "Start a plain login shell", "启动登录 Shell")
                                .to_string()
                        } else {
                            format!("Run {}", option.command)
                        }
                    } else {
                        option.description.clone()
                    };
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(row.left() + 38.0, row.center().y + 3.0),
                            Pos2::new(row.right() - 12.0, row.bottom() - 3.0),
                        ),
                        &description,
                        FontId::proportional(9.5),
                        p.text3,
                        Align2::LEFT_CENTER,
                    );
                    if !option.command.is_empty() {
                        text_clipped(
                            ui.painter(),
                            Rect::from_min_max(
                                Pos2::new(row.right() - 120.0, row.top()),
                                Pos2::new(row.right() - 12.0, row.center().y - 2.0),
                            ),
                            &option.command,
                            FontId::monospace(8.5),
                            p.text3,
                            Align2::RIGHT_CENTER,
                        );
                    }
                    if response.clicked() {
                        launch = Some((option.name.clone(), option.command.clone()));
                        close = true;
                    }
                }
            });
    });
    if let Some((title, command)) = launch {
        if command.is_empty() {
            model.queue(Command::NewTerminal);
        } else {
            model.queue(Command::NewTerminalCommand { title, command });
        }
    }
    if should_dismiss(ui, rect, rt.agent_menu_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.agent_menu = Some(anchor);
    }
}

#[allow(clippy::too_many_arguments)]
/// User 过滤菜单：本侧出现过的作者（首见序）。
fn draw_compare_user_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    side: u8,
) {
    let p = theme.palette;
    let commits = match model.ui.compare_branches.sides.as_ref() {
        Some(sides) if side == 0 => sides.left.commits.clone(),
        Some(sides) => sides.right.commits.clone(),
        None => Vec::new(),
    };
    let mut authors = crate::compare::unique_authors(&commits);
    authors.insert(0, crate::tr!(model, "All authors", "全部作者").to_string());
    let height = (authors.len() as f32 * 24.0 + 20.0).min(320.0);
    let rect = popover_rect(rt, screen, menu.anchor, 200.0, height);
    menu_frame(ui, rect, theme);
    let mut picked: Option<Option<String>> = None;
    for (i, author) in authors.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, rect.top() + 10.0 + i as f32 * 24.0),
            Vec2::new(rect.width() - 16.0, 22.0),
        );
        let response = ui.interact(row, ui.scope_id().with(("cbuser", i)), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
        }
        text_clipped(
            ui.painter(),
            row,
            author,
            FontId::proportional(11.0),
            p.text,
            Align2::LEFT_CENTER,
        );
        if response.clicked() {
            picked = Some(if i == 0 { None } else { Some(author.clone()) });
        }
    }
    if let Some(pick) = picked {
        let view = if side == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.author = pick;
        return;
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        return;
    }
    rt.menu = Some(menu);
}

/// Date 过滤菜单：All / Today / Last 7 days / Last 30 days。
fn draw_compare_date_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    side: u8,
) {
    let p = theme.palette;
    let items: Vec<(String, Option<lazecode_core::CompareDateFilter>)> = vec![
        (crate::tr!(model, "All", "全部").to_string(), None),
        (
            crate::tr!(model, "Today", "今天").to_string(),
            Some(lazecode_core::CompareDateFilter::Today),
        ),
        (
            crate::tr!(model, "Last 7 days", "最近 7 天").to_string(),
            Some(lazecode_core::CompareDateFilter::Last7Days),
        ),
        (
            crate::tr!(model, "Last 30 days", "最近 30 天").to_string(),
            Some(lazecode_core::CompareDateFilter::Last30Days),
        ),
    ];
    let height = items.len() as f32 * 24.0 + 20.0;
    let rect = popover_rect(rt, screen, menu.anchor, 160.0, height);
    menu_frame(ui, rect, theme);
    let mut picked: Option<Option<lazecode_core::CompareDateFilter>> = None;
    for (i, (label, value)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, rect.top() + 10.0 + i as f32 * 24.0),
            Vec2::new(rect.width() - 16.0, 22.0),
        );
        let response = ui.interact(row, ui.scope_id().with(("cbdate", i)), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
        }
        ui.painter().text(
            Pos2::new(row.left() + 10.0, row.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(11.0),
            p.text,
        );
        if response.clicked() {
            picked = Some(*value);
        }
    }
    if let Some(pick) = picked {
        let view = if side == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.date = pick;
        return;
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        return;
    }
    rt.menu = Some(menu);
}

/// Branch(ref) 过滤菜单：本侧提交所属的引用名（首见序）。
fn draw_compare_ref_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    side: u8,
) {
    let p = theme.palette;
    let commits = match model.ui.compare_branches.sides.as_ref() {
        Some(sides) if side == 0 => sides.left.commits.clone(),
        Some(sides) => sides.right.commits.clone(),
        None => Vec::new(),
    };
    let mut refs = crate::compare::unique_refs(&commits);
    refs.insert(0, crate::tr!(model, "All Branches", "全部分支").to_string());
    let height = (refs.len() as f32 * 24.0 + 20.0).min(320.0);
    let rect = popover_rect(rt, screen, menu.anchor, 220.0, height);
    menu_frame(ui, rect, theme);
    let mut picked: Option<Option<String>> = None;
    for (i, name) in refs.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, rect.top() + 10.0 + i as f32 * 24.0),
            Vec2::new(rect.width() - 16.0, 22.0),
        );
        let response = ui.interact(row, ui.scope_id().with(("cbref", i)), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
        }
        text_clipped(
            ui.painter(),
            row,
            name,
            FontId::monospace(10.0),
            p.text,
            Align2::LEFT_CENTER,
        );
        if response.clicked() {
            picked = Some(if i == 0 { None } else { Some(name.clone()) });
        }
    }
    if let Some(pick) = picked {
        let view = if side == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.selected_ref_filter = pick;
        return;
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        return;
    }
    rt.menu = Some(menu);
}

/// Draw the open context menu, if any. Called by `overlays::draw`.
pub(super) fn draw_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
) {
    if let Some(menu) = rt.menu.take() {
        match menu.kind {
            MenuKind::Project => draw_project_menu(ui, model, rt, theme, screen, menu),
            MenuKind::Branch => draw_branch_menu(ui, model, rt, theme, screen, menu),
            MenuKind::Worktree => draw_worktree_menu(ui, model, rt, theme, screen, menu),
            MenuKind::LogBranchFilter { project } => {
                draw_log_branch_menu(ui, model, rt, theme, screen, menu, project)
            }
            MenuKind::LogDateFilter { project } => {
                draw_log_date_menu(ui, model, rt, theme, screen, menu, project)
            }
            MenuKind::LogCommitMenu { project } => {
                let payload = menu.payload.clone();
                draw_commit_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref(),
                )
            }
            MenuKind::BranchContext { project } => {
                let payload = menu.payload.clone();
                draw_branch_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref(),
                )
            }
            MenuKind::TagContext { project } => {
                let payload = menu.payload.clone();
                draw_tag_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref(),
                )
            }
            MenuKind::TreeContext { project, is_dir } => {
                let payload = menu.payload.clone();
                draw_tree_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                    is_dir,
                )
            }
            MenuKind::EditorTabContext { project, tab_id } => {
                let payload = menu.payload.clone();
                draw_editor_tab_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    tab_id,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::TerminalTabContext {
                project,
                tab_id,
                terminal_id,
            } => {
                let payload = menu.payload.clone();
                draw_terminal_tab_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    tab_id,
                    terminal_id,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::ChangelistHeaderContext { project } => {
                let payload = menu.payload.clone();
                draw_changelist_header_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::ChangeFileContext { project } => {
                let payload = menu.payload.clone();
                draw_change_file_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::CommitHistory { project } => {
                draw_commit_history_menu(ui, model, rt, theme, screen, menu, project);
            }
            MenuKind::CommitExtras => {
                draw_commit_extras_menu(ui, model, rt, theme, screen, menu);
            }
            MenuKind::StashEntryMenu { project } => {
                let payload = menu.payload.clone();
                draw_stash_entry_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref(),
                );
            }
            MenuKind::StashFileMenu { project } => {
                let payload = menu.payload.clone();
                draw_stash_file_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::CommitViewOptions => {
                draw_commit_view_options_menu(ui, model, rt, theme, screen, menu);
            }
            MenuKind::CompareViewOptions { project } => {
                draw_compare_view_options_menu(ui, model, rt, theme, screen, menu, project);
            }
            MenuKind::ShelfContext { project } => {
                let payload = menu.payload.clone();
                draw_shelf_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::CompareUserFilter { side } => {
                draw_compare_user_menu(ui, model, rt, theme, screen, menu, side);
            }
            MenuKind::CompareDateFilter { side } => {
                draw_compare_date_menu(ui, model, rt, theme, screen, menu, side);
            }
            MenuKind::CompareRefFilter { side } => {
                draw_compare_ref_menu(ui, model, rt, theme, screen, menu, side);
            }
            MenuKind::LogSortOptions { project } => {
                draw_log_sort_options_menu(ui, model, rt, theme, screen, menu, project);
            }
            MenuKind::LogPathFilter { project } => {
                let payload = menu.payload.clone();
                draw_log_path_filter_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::CommitDetailsFileContext { project } => {
                let payload = menu.payload.clone();
                draw_commit_details_file_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::EditorContext { project } => {
                let payload = menu.payload.clone();
                draw_editor_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                );
            }
        }
    }
    if let Some(anchor) = rt.agent_menu.take() {
        draw_agent_menu(ui, model, rt, theme, screen, anchor);
    }
}
