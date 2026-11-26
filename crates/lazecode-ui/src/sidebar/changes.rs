//! Source-control sidebar: changes list, changelist groups, clean state and
//! the commit dock.

use super::prelude::*;
use super::{active, active_mut};

/// Position of a file-only index among all rendered rows. Group headers and
/// directory rows have no change index and are skipped by the position count.
fn file_row_index(
    mut rows: impl Iterator<Item = Option<usize>>,
    file_index: usize,
) -> Option<usize> {
    rows.position(|change| change == Some(file_index))
}
#[allow(clippy::too_many_arguments)]
pub(super) fn change_row(
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
    let name_right = rect.right() - 30.0;
    let name_font = FontId::proportional(13.0);
    let name_width = text_width(ui.painter(), name, name_font.clone());
    let parent = change
        .path
        .rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or("");
    if text_left + name_width > name_right {
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(text_left, rect.top()),
                Pos2::new(name_right, rect.bottom()),
            ),
            name,
            name_font,
            name_color,
            Align2::LEFT_CENTER,
        );
    } else {
        ui.painter().text(
            Pos2::new(text_left, rect.center().y),
            Align2::LEFT_CENTER,
            name,
            name_font,
            name_color,
        );
        if show_dir_hint && !parent.is_empty() {
            let path_left = text_left + name_width + 8.0;
            if path_left + 4.0 < name_right {
                text_clipped(
                    ui.painter(),
                    Rect::from_min_max(
                        Pos2::new(path_left, rect.top()),
                        Pos2::new(name_right, rect.bottom()),
                    ),
                    parent,
                    FontId::proportional(11.5),
                    p.text3,
                    Align2::LEFT_CENTER,
                );
            }
        }
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

pub(super) fn source_control(
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

    // ----- Stash 页签可见性：抽屉可见时按仓库加载一次（只读 reflog，无子进程） -----
    let stash_root = active(model).active_root().to_path_buf();
    if rt.stash_loaded.as_deref() != Some(stash_root.as_path()) {
        rt.stash_entries = lazecode_git::stash_list(&stash_root).unwrap_or_default();
        rt.stash_loaded = Some(stash_root.clone());
        rt.stash_selected = None;
        rt.stash_expanded.clear();
        rt.stash_collapsed.clear();
        rt.stash_file_sel = None;
        if let Some(project) = model.active_project_mut() {
            project.invalidate_stash_details();
        }
    }
    // 贮藏被清空后不能停留在隐藏的页签上。
    if rt.commit_tab == CommitTab::Stash && rt.stash_entries.is_empty() {
        rt.commit_tab = CommitTab::Commit;
    }

    // ----- 稳定版 CommitDrawer 头部（34px）：Commit/Shelf(/Stash) 页签 + ⋮ + ——
    let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 34.0));
    let tab = rt.commit_tab;
    // Stash 只在有贮藏记录时出现（PyCharm 行为）。
    let mut tab_specs: Vec<(String, CommitTab)> = vec![
        (
            crate::tr!(model, "Commit", "提交").to_string(),
            CommitTab::Commit,
        ),
        (
            crate::tr!(model, "Shelf", "架藏").to_string(),
            CommitTab::Shelf,
        ),
    ];
    if !rt.stash_entries.is_empty() {
        tab_specs.push((
            crate::tr!(model, "Stash", "贮藏").to_string(),
            CommitTab::Stash,
        ));
    }
    let tab_x = |index: usize| head.left() + 10.0 + index as f32 * 62.0;
    for (index, (label, kind)) in tab_specs.into_iter().enumerate() {
        let active = tab == kind;
        let tab_rect = Rect::from_min_size(
            Pos2::new(tab_x(index), head.top()),
            Vec2::new(56.0, head.height()),
        );
        let response = ui.interact(
            tab_rect,
            ui.scope_id().with((
                "commit-tab",
                match kind {
                    CommitTab::Commit => 0u8,
                    CommitTab::Shelf => 1u8,
                    CommitTab::Stash => 2u8,
                },
            )),
            Sense::click(),
        );
        ui.painter().text(
            Pos2::new(tab_rect.center().x, tab_rect.center().y - 1.0),
            Align2::CENTER_CENTER,
            label.as_str(),
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
        // 抽屉级菜单：与 Shelf/Commit 页签完全一致（不再按页签切换）。
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
        let project_now = active(model);
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
    } else if tab == CommitTab::Shelf {
        // Shelf 工具栏：静默恢复 / 视图选项。
        let unshelve_silent = tool_btn(ui, 0, Icon::Undo, true, theme);
        let eye = tool_btn(ui, 1, Icon::ViewOptions, true, theme);
        expand_all |= tool_btn(ui, 2, Icon::ExpandAll, true, theme);
        collapse_all |= tool_btn(ui, 3, Icon::CollapseAll, true, theme);
        if unshelve_silent {
            // ⌥⌘U：恢复到首个变更列表，不弹对话框（稳定版 Unshelve Silently）。
            if let Some(name) = rt.shelf_selected.clone() {
                let default_list = {
                    let project = active(model);
                    lazecode_git::Changelists::open(project.active_root())
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
    } else {
        // Stash 工具栏：刷新 / 全部展开 / 全部折叠。
        let refresh = tool_btn(ui, 0, Icon::Refresh, true, theme);
        if tool_btn(ui, 1, Icon::ExpandAll, true, theme) {
            rt.stash_collapsed.clear();
            let names: Vec<String> = rt
                .stash_entries
                .iter()
                .map(|entry| entry.name.clone())
                .collect();
            rt.stash_expanded.extend(names);
        }
        if tool_btn(ui, 2, Icon::CollapseAll, true, theme) {
            rt.stash_expanded.clear();
            rt.stash_collapsed.clear();
        }
        if refresh {
            rt.stash_loaded = None;
            if let Some(project) = model.active_project_mut() {
                project.invalidate_stash_details();
            }
        }
    }
    ui.painter().hline(
        toolbar.x_range(),
        toolbar.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );

    let project = active(model);
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
    let last = project.active_git().and_then(|git| git.head.clone());
    let can_git = project.active_git().is_some();
    let git_error = project.active_git_error().map(str::to_string);
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

    if can_git {
        if let Some(err) = &git_error {
            // A live repository can still fail to refresh (locked index, bad
            // permissions). Keep it to one clipped line so it never spills
            // into the editor pane.
            text_clipped(
                ui.painter(),
                Rect::from_min_size(
                    Pos2::new(list.left() + 12.0, list.top() + 10.0),
                    Vec2::new((list.width() - 24.0).max(1.0), 20.0),
                ),
                err.lines().next().unwrap_or(err),
                FontId::proportional(10.5),
                p.rose,
                Align2::LEFT_CENTER,
            );
        }
    }

    if !can_git {
        // 非 Git 项目：对齐稳定版 notRepoNotice，并提供初始化入口
        // （原 dock 内的初始化按钮放在空态之外，实际不可达）。
        // A missing repository is an expected state; the red refresh error is
        // deliberately suppressed above so this is the only message shown.
        let notice = crate::tr!(
            model,
            "This folder is not a Git repository. Initialize one to track changes.",
            "当前目录不是 Git 仓库，初始化后即可跟踪更改。"
        );
        let galley = ui.painter().layout(
            notice.to_string(),
            FontId::proportional(12.0),
            p.text3,
            (list.width() - 28.0).max(120.0),
        );
        let text_h = galley.size().y;
        let group_top = list.top() + ((list.height() - text_h - 44.0) * 0.5).max(18.0);
        ui.painter().with_clip_rect(list).galley(
            Pos2::new(list.center().x - galley.size().x * 0.5, group_top),
            galley,
            p.text3,
        );
        let init_w = 170.0_f32.min((list.width() - 32.0).max(140.0));
        let init = Rect::from_center_size(
            Pos2::new(list.center().x, group_top + text_h + 22.0 + 13.0),
            Vec2::new(init_w, 26.0),
        );
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

    if tab == CommitTab::Stash {
        let body = Rect::from_min_max(
            Pos2::new(rect.left(), toolbar.bottom()),
            Pos2::new(rect.right(), rect.bottom()),
        );
        crate::sidebar::stash_view(ui, model, rt, theme, body);
        return;
    }

    if changes.is_empty() {
        if project.active_git_loading() {
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
    let lists = lazecode_git::Changelists::open(&active(model).root);
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
    let collapsed_lists: HashSet<String> = active(model).collapsed_lists.clone();

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

    // Keyboard: ↑/↓/Home/End move the single file selection while the pointer
    // is over the list. Group and directory rows stay mouse-only (clicking a
    // group selects the whole changelist), so only file rows are stepped.
    let mut nav_scroll: Option<f32> = None;
    if crate::list_nav::keys_free(ui, model, rt) {
        if let Some(mv) = crate::list_nav::hovered_move(ui, rows_rect) {
            let current = {
                let project = active(model);
                flat_order.iter().position(|index| {
                    let path = &changes[*index].path;
                    project.change_anchor.as_deref() == Some(path.as_str())
                        || project.change_selection.contains(path)
                })
            };
            if let Some(next) = crate::list_nav::stepped(current, flat_order.len(), mv) {
                let path = changes[flat_order[next]].path.clone();
                let project = active_mut(model);
                project.change_selection.clear();
                project.change_selection.insert(path.clone());
                project.change_anchor = Some(path);
                // `show_rows` counts headers and directory rows, so scrolling
                // must use the row position, not the file-only index.
                nav_scroll = file_row_index(rows.iter().map(|row| row.change), flat_order[next])
                    .map(|row| crate::list_nav::centered_scroll(row, 21.0, rows_rect.height()));
            }
        }
    }

    crate::app_ui::region(ui, rows_rect, "sc-rows", |ui| {
        let mut area = egui::ScrollArea::vertical()
            .id_salt("changes")
            .auto_shrink([false, false]);
        if let Some(offset) = nav_scroll {
            area = area.vertical_scroll_offset(offset);
        }
        area.show_rows(ui, 21.0, rows.len(), |ui, range| {
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
                            let project = active_mut(model);
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
                                let project = active_mut(model);
                                if !project.collapsed_lists.remove(&group.key) {
                                    project.collapsed_lists.insert(group.key.clone());
                                }
                            } else {
                                let project = active_mut(model);
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
                                let root = active(model).active_root().to_path_buf();
                                if let Ok(content) = lazecode_git::merge_file_content(&root, &path)
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
                            let project = active_mut(model);
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
                                    let project = active_mut(model);
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
                                            group_toggle =
                                                Some((sorted_paths, state != TriState::Checked));
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
                                    Icon::Folder,
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
                                    let project = active_mut(model);
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
                        let (new_checked, clicked, double_clicked, secondary, discard) = change_row(
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
                                let project = active_mut(model);
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
                            let project = active_mut(model);
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
        let project = active_mut(model);
        for path in paths {
            if target {
                project.checked_paths.insert(path);
            } else {
                project.checked_paths.remove(&path);
            }
        }
    }
    if let Some((path, is_checked)) = toggled {
        let project = active_mut(model);
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
        model.ui.reveal_editor_area();
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
pub(super) fn group_check_state(checked: &HashSet<String>, paths: &[String]) -> TriState {
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
pub(super) fn all_list_keys(project: &ProjectState) -> HashSet<String> {
    // 借助 changelist 存储推断分组键集合；Unversioned/冲突子组键固定。
    let mut keys: HashSet<String> = HashSet::new();
    let lists = lazecode_git::Changelists::open(project.active_root());
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

pub(super) fn draw_clean_state(
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
pub(super) fn draw_commit_dock(
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
        active_mut(model).commit_message = value;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_row_index_skips_header_and_directory_rows() {
        // Header, file 0, directory, file 1, file 2.
        let rows = [None, Some(0), None, Some(1), Some(2)];
        assert_eq!(file_row_index(rows.iter().copied(), 0), Some(1));
        assert_eq!(file_row_index(rows.iter().copied(), 1), Some(3));
        assert_eq!(file_row_index(rows.iter().copied(), 2), Some(4));
        assert_eq!(file_row_index(rows.iter().copied(), 7), None);
    }
}
