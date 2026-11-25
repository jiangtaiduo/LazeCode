//! Git panel branch list.

use super::prelude::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_branch_list(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    index: usize,
    branches: &[lazecode_git::BranchInfo],
    current: &str,
    tags: &[String],
    _tag_targets: &[(String, String)],
) {
    let p = theme.palette;
    let favorite_root = model.projects[index].root.clone();
    ui.painter().rect_filled(rect, 0.0, p.panel);
    ui.painter().vline(
        rect.right() - 0.5,
        rect.y_range(),
        Stroke::new(1.0, p.border_soft),
    );

    // 分支搜索框：稳定版 branchSearchOpen，🔍 工具条按钮切换显示。
    let search_open = model.ui.branch_search_open;
    let search_h = if search_open { 26.0 } else { 4.0 };
    let search_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 6.0, rect.top() + 5.0),
        Vec2::new(rect.width() - 12.0, 22.0),
    );
    if search_open {
        ui.painter()
            .rect_filled(search_rect, egui::CornerRadius::same(4), p.scrim_subtle);
        ui.painter().rect_stroke(
            search_rect,
            egui::CornerRadius::same(4),
            Stroke::new(1.0, p.border_soft),
            StrokeKind::Inside,
        );
        paint_icon(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(search_rect.left() + 12.0, search_rect.center().y),
                Vec2::splat(11.0),
            ),
            Icon::Search,
            p.text3,
        );
        let mut query_value = model.projects[index].branch_query.clone();
        let response = ui.put(
            Rect::from_min_max(
                Pos2::new(search_rect.left() + 24.0, search_rect.top() + 2.0),
                Pos2::new(search_rect.right() - 6.0, search_rect.bottom() - 2.0),
            ),
            TextEdit::singleline(&mut query_value)
                .frame(egui::Frame::NONE)
                .align(egui::Align2::LEFT_CENTER)
                .hint_text(crate::tr!(model, "Filter branches", "过滤分支"))
                .font(FontId::proportional(11.0)),
        );
        if response.changed() {
            model.projects[index].branch_query = query_value;
        }
    }
    let needle = model.projects[index].branch_query.to_lowercase();
    let matches = |name: &str| needle.is_empty() || name.to_lowercase().contains(&needle);

    // Build the ref tree from the branches that pass the filter.
    let local: Vec<lazecode_git::BranchInfo> = branches
        .iter()
        .filter(|branch| !branch.remote && matches(&branch.name))
        .cloned()
        .collect();
    let remote: Vec<lazecode_git::BranchInfo> = branches
        .iter()
        .filter(|branch| branch.remote && matches(&branch.name))
        .cloned()
        .collect();
    let tags: Vec<String> = tags.iter().filter(|tag| matches(tag)).cloned().collect();

    let rows_rect = Rect::from_min_max(
        Pos2::new(rect.left(), search_rect.top() + search_h),
        Pos2::new(rect.right(), rect.bottom()),
    );
    let row_h = 22.0;

    // Sections: Local / Remote / Tags; the roots are pre-built above.
    let expanded_dirs: std::collections::HashSet<String> =
        model.projects[index].ref_dirs_expanded.clone();
    let mut toggle_dir: Option<String> = None;
    let mut open_menu: Option<(String, bool, Rect)> = None; // branch name + remote flag + anchor
    let mut open_tag: Option<(String, Rect)> = None;

    let curr_branch = branches.iter().find(|b| b.name == current);
    let local_tree = lazecode_core::gitref::build_ref_tree(&local);
    let local_flat = lazecode_core::gitref::flatten_ref_tree(&local_tree, &expanded_dirs);
    let remote_tree = lazecode_core::gitref::build_ref_tree(&remote);
    let remote_flat = lazecode_core::gitref::flatten_ref_tree(&remote_tree, &expanded_dirs);

    let mut total_h = 24.0; // HEAD title
    if curr_branch.is_some() {
        total_h += row_h + 6.0;
    }
    total_h += 24.0 + local_flat.len() as f32 * row_h;
    if !remote.is_empty() {
        total_h += 24.0 + remote_flat.len() as f32 * row_h;
    }
    if !tags.is_empty() {
        total_h += 24.0 + tags.len() as f32 * row_h;
    }
    total_h += 24.0;

    crate::app_ui::region(ui, rows_rect, "branch-tree-region", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("branch-tree-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let (container_rect, _) = ui
                    .allocate_exact_size(Vec2::new(ui.available_width(), total_h), Sense::hover());
                let mut y = container_rect.top() + 6.0;

                let draw_dir_row =
                    |ui: &mut Ui, label: &str, path: &str, expanded: bool, y: &mut f32| -> bool {
                        let row = Rect::from_min_size(
                            Pos2::new(container_rect.left() + 4.0, *y),
                            Vec2::new(container_rect.width() - 8.0, 20.0),
                        );
                        let response = ui.interact(
                            row,
                            ui.scope_id().with(("refdir", path.to_string())),
                            Sense::click(),
                        );
                        if response.hovered() {
                            ui.painter()
                                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
                        }
                        // chevron.
                        let cx = row.left() + 12.0;
                        let cy = row.center().y;
                        let color = p.text3;
                        let (dy, dx) = if expanded { (0.0, 3.0) } else { (-3.0, 0.0) };
                        ui.painter().line_segment(
                            [
                                Pos2::new(cx - 3.5 + dx, cy - 3.5 + dy),
                                Pos2::new(cx + 3.5 + dx, cy + 3.5 + dy),
                            ],
                            Stroke::new(1.4, color),
                        );
                        ui.painter().line_segment(
                            [
                                Pos2::new(cx + 3.5 + dx, cy + 3.5 + dy),
                                Pos2::new(cx - 3.5 + dx * 2.0, cy - 3.5 + dy * 2.0),
                            ],
                            Stroke::new(1.4, color),
                        );
                        ui.painter().text(
                            Pos2::new(row.left() + 26.0, row.center().y),
                            Align2::LEFT_CENTER,
                            label,
                            FontId::proportional(12.5),
                            p.text2,
                        );
                        *y += row_h;
                        response.clicked()
                    };

                // HEAD (Current Branch)：纯标签行（稳定版 b-head-label，
                // 22px、padding-left 24、weight 500）；当前分支行在 Local 组里。
                ui.painter().text(
                    Pos2::new(container_rect.left() + 24.0, y + 8.0),
                    Align2::LEFT_CENTER,
                    "HEAD (Current Branch)",
                    FontId::proportional(12.0),
                    p.text2,
                );
                y += 22.0;
                // Local tree.
                ui.painter().text(
                    Pos2::new(container_rect.left() + 12.0, y + 8.0),
                    Align2::LEFT_CENTER,
                    crate::tr!(model, "Local", "本地"),
                    FontId::proportional(12.5),
                    p.text3,
                );
                y += 24.0;
                for row_ref in local_flat.iter() {
                    if row_ref.dir {
                        if draw_dir_row(
                            ui,
                            &row_ref.label,
                            &row_ref.path,
                            expanded_dirs.contains(&row_ref.path),
                            &mut y,
                        ) {
                            toggle_dir = Some(row_ref.path.clone());
                        }
                    } else if row_ref.is_tag {
                        // Tags live in their own section below.
                    } else if let Some(branch) = row_ref.branch.as_ref() {
                        let row = Rect::from_min_size(
                            Pos2::new(container_rect.left() + 4.0, y),
                            Vec2::new(container_rect.width() - 8.0, 20.0),
                        );
                        let indent = 10.0 + 12.0 * row_ref.depth as f32;
                        let is_current = branch.name == current;
                        let is_fav = model.is_favorite_branch(&favorite_root, &branch.name);
                        let response = ui.interact(
                            row,
                            ui.scope_id().with(("refrow", branch.name.clone())),
                            Sense::click(),
                        );
                        let is_selected =
                            model.projects[index].ref_selection.contains(&branch.name);
                        // b-item：margin 1px 4px、radius 4；选中 #26415f。
                        let row = row.shrink2(Vec2::new(2.0, 1.0));
                        if is_selected {
                            ui.painter().rect_filled(
                                row,
                                egui::CornerRadius::same(4),
                                p.selected_row,
                            );
                        } else if response.hovered() {
                            ui.painter()
                                .rect_filled(row, egui::CornerRadius::same(4), p.hover);
                        }
                        // 图标规则（renderRefRow）：收藏→黄星；tag/当前→黄 tag；
                        // 远程→云；本地→绿分支。
                        let green = Color32::from_rgb(0x3f, 0xb9, 0x50);
                        let amber = Color32::from_rgb(0xe3, 0xb3, 0x41);
                        let (icon, tint, isize) = if is_fav && !is_current {
                            (Icon::Star, amber, 12.0)
                        } else if is_current {
                            (Icon::Tag, amber, 12.0)
                        } else {
                            (Icon::Branch, green, 13.0)
                        };
                        paint_icon(
                            ui.painter(),
                            Rect::from_center_size(
                                Pos2::new(row.left() + indent + 6.0, row.center().y),
                                Vec2::splat(isize),
                            ),
                            icon,
                            tint,
                        );
                        let name_label = row_ref.label.clone();
                        ui.painter().text(
                            Pos2::new(row.left() + indent + 18.0, row.center().y),
                            Align2::LEFT_CENTER,
                            &name_label,
                            FontId::proportional(12.5),
                            if is_current { p.text_bright } else { p.text },
                        );
                        // b-up-arrow：↑n ↓n 绿色 10px，margin-left auto。
                        let mut badge = String::new();
                        if branch.ahead > 0 {
                            badge.push_str(&format!("↑{}", branch.ahead));
                        }
                        if branch.behind > 0 {
                            badge.push_str(&format!(" ↓{}", branch.behind));
                        }
                        if !badge.is_empty() {
                            ui.painter().text(
                                Pos2::new(row.right() - 6.0, row.center().y),
                                Align2::RIGHT_CENTER,
                                badge,
                                FontId::proportional(11.0),
                                green,
                            );
                        }
                        if response.secondary_clicked() {
                            if !model.projects[index].ref_selection.contains(&branch.name) {
                                model.projects[index].ref_selection.clear();
                                model.projects[index]
                                    .ref_selection
                                    .push(branch.name.clone());
                            }
                            let anchor = ui
                                .input(|i| i.pointer.hover_pos())
                                .map(|p| Rect::from_min_size(p, Vec2::splat(1.0)))
                                .unwrap_or(row);
                            open_menu = Some((branch.name.clone(), branch.remote, anchor));
                        }
                        if response.clicked() {
                            // 分支树成为键盘焦点：编辑器/控制台让位，Cmd+C
                            // 复制选中分支全名。
                            rt.editor_focus = None;
                            rt.terminal_focus = None;
                            let cmd = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
                            let shift = ui.input(|i| i.modifiers.shift);
                            if cmd {
                                let selection = &mut model.projects[index].ref_selection;
                                if let Some(pos) = selection.iter().position(|n| n == &branch.name)
                                {
                                    selection.remove(pos);
                                } else {
                                    selection.push(branch.name.clone());
                                }
                            } else if shift && !model.projects[index].ref_selection.is_empty() {
                                let all_names: Vec<String> = local_flat
                                    .iter()
                                    .filter_map(|r| r.branch.as_ref().map(|b| b.name.clone()))
                                    .collect();
                                let anchor = model.projects[index]
                                    .ref_selection
                                    .first()
                                    .cloned()
                                    .unwrap_or_else(|| branch.name.clone());
                                let pos1 = all_names.iter().position(|n| n == &anchor).unwrap_or(0);
                                let pos2 = all_names
                                    .iter()
                                    .position(|n| n == &branch.name)
                                    .unwrap_or(0);
                                let start = pos1.min(pos2);
                                let end = pos1.max(pos2);
                                for n in &all_names[start..=end] {
                                    let selection = &mut model.projects[index].ref_selection;
                                    if !selection.contains(n) {
                                        selection.push(n.clone());
                                    }
                                }
                            } else {
                                model.projects[index].ref_selection.clear();
                                model.projects[index]
                                    .ref_selection
                                    .push(branch.name.clone());
                            }
                        }
                        if response.double_clicked() {
                            model.queue(Command::CheckoutBranch(branch.name.clone()));
                        }
                        y += row_h;
                    }
                }

                // Remote section.
                if !remote.is_empty() {
                    ui.painter().text(
                        Pos2::new(container_rect.left() + 12.0, y + 8.0),
                        Align2::LEFT_CENTER,
                        crate::tr!(model, "Remote", "远程"),
                        FontId::proportional(12.5),
                        p.text3,
                    );
                    y += 24.0;
                    for row_ref in remote_flat.iter() {
                        if row_ref.dir {
                            if draw_dir_row(
                                ui,
                                &row_ref.label,
                                &row_ref.path,
                                expanded_dirs.contains(&row_ref.path),
                                &mut y,
                            ) {
                                toggle_dir = Some(row_ref.path.clone());
                            }
                        } else if let Some(branch) = row_ref.branch.as_ref() {
                            let row = Rect::from_min_size(
                                Pos2::new(container_rect.left() + 4.0, y),
                                Vec2::new(container_rect.width() - 8.0, 20.0),
                            );
                            let indent = 10.0 + 12.0 * row_ref.depth as f32;
                            let response = ui.interact(
                                row,
                                ui.scope_id().with(("refrow", branch.name.clone())),
                                Sense::click(),
                            );
                            let is_selected =
                                model.projects[index].ref_selection.contains(&branch.name);
                            let row = row.shrink2(Vec2::new(2.0, 1.0));
                            if is_selected {
                                ui.painter().rect_filled(
                                    row,
                                    egui::CornerRadius::same(4),
                                    p.selected_row,
                                );
                            } else if response.hovered() {
                                ui.painter()
                                    .rect_filled(row, egui::CornerRadius::same(4), p.hover);
                            }
                            // 远程分支：云图标 muted（renderRefRow）。
                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(row.left() + indent + 6.0, row.center().y),
                                    Vec2::splat(13.0),
                                ),
                                Icon::Cloud,
                                p.text3,
                            );
                            ui.painter().text(
                                Pos2::new(row.left() + indent + 18.0, row.center().y),
                                Align2::LEFT_CENTER,
                                &row_ref.label,
                                FontId::proportional(12.5),
                                p.text,
                            );
                            if response.secondary_clicked() {
                                if !model.projects[index].ref_selection.contains(&branch.name) {
                                    model.projects[index].ref_selection.clear();
                                    model.projects[index]
                                        .ref_selection
                                        .push(branch.name.clone());
                                }
                                let anchor = ui
                                    .input(|i| i.pointer.hover_pos())
                                    .map(|p| Rect::from_min_size(p, Vec2::splat(1.0)))
                                    .unwrap_or(row);
                                open_menu = Some((branch.name.clone(), true, anchor));
                            }
                            if response.clicked() {
                                rt.editor_focus = None;
                                rt.terminal_focus = None;
                                let cmd = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
                                if cmd {
                                    let selection = &mut model.projects[index].ref_selection;
                                    if let Some(pos) =
                                        selection.iter().position(|n| n == &branch.name)
                                    {
                                        selection.remove(pos);
                                    } else {
                                        selection.push(branch.name.clone());
                                    }
                                } else {
                                    model.projects[index].ref_selection.clear();
                                    model.projects[index]
                                        .ref_selection
                                        .push(branch.name.clone());
                                }
                            }
                            y += row_h;
                        }
                    }
                }

                // Tags section（b-group-row + tag 行：黄 tag 图标 + 11px 名字）。
                if !tags.is_empty() {
                    ui.painter().text(
                        Pos2::new(container_rect.left() + 12.0, y + 8.0),
                        Align2::LEFT_CENTER,
                        crate::tr!(model, "Tags", "标签"),
                        FontId::proportional(12.5),
                        p.text2,
                    );
                    y += 24.0;
                    for tag in tags.iter() {
                        let row = Rect::from_min_size(
                            Pos2::new(container_rect.left() + 4.0, y),
                            Vec2::new(container_rect.width() - 8.0, 20.0),
                        );
                        let response = ui.interact(
                            row,
                            ui.scope_id().with(("tagrow", tag.clone())),
                            Sense::click(),
                        );
                        let is_selected = model.projects[index].ref_selection.contains(tag);
                        let row = row.shrink2(Vec2::new(2.0, 1.0));
                        if is_selected {
                            ui.painter().rect_filled(
                                row,
                                egui::CornerRadius::same(4),
                                p.selected_row,
                            );
                        } else if response.hovered() {
                            ui.painter()
                                .rect_filled(row, egui::CornerRadius::same(4), p.hover);
                        }
                        paint_icon(
                            ui.painter(),
                            Rect::from_center_size(
                                Pos2::new(row.left() + 16.0, row.center().y),
                                Vec2::splat(12.0),
                            ),
                            Icon::Tag,
                            p.amber,
                        );
                        ui.painter().text(
                            Pos2::new(row.left() + 28.0, row.center().y),
                            Align2::LEFT_CENTER,
                            tag,
                            FontId::proportional(12.5),
                            p.text,
                        );
                        if response.secondary_clicked() {
                            let anchor = ui
                                .input(|i| i.pointer.hover_pos())
                                .map(|p| Rect::from_min_size(p, Vec2::splat(1.0)))
                                .unwrap_or(row);
                            open_tag = Some((tag.clone(), anchor));
                        }
                        y += row_h;
                    }
                }
            });
    });
    // Cmd+C / Ctrl+C：复制选中分支的全名（保留 a/b 路径；多选按行拼接）。
    // 文本面（编辑器/终端）持有焦点时让位给它们自己的复制。
    let copy_event = ui.input(|input| {
        input
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::Copy))
    });
    if copy_event && rt.editor_focus.is_none() && rt.terminal_focus.is_none() {
        let selection = model.projects[index].ref_selection.clone();
        if !selection.is_empty() {
            ui.ctx().copy_text(selection.join("\n"));
        }
    }
    // Apply deferred actions.
    if let Some(dir) = toggle_dir {
        if expanded_dirs.contains(&dir) {
            model.projects[index].ref_dirs_expanded.remove(&dir);
        } else {
            model.projects[index].ref_dirs_expanded.insert(dir);
        }
    }
    if let Some((tag, anchor)) = open_tag {
        rt.open_menu(
            crate::runtime::MenuKind::TagContext { project: index },
            anchor,
            Some(tag),
        );
    } else if let Some((name, remote, anchor)) = open_menu {
        rt.open_menu(
            crate::runtime::MenuKind::BranchContext { project: index },
            anchor,
            Some(format!(
                "{name}\u{1}{}",
                if remote { "remote" } else { "local" }
            )),
        );
    }
}
