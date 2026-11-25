//! Branch tree context menu and the branch/global picker.

use super::prelude::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_branch_context_menu(
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
pub(super) fn draw_branch_menu(
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
