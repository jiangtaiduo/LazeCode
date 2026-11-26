//! Git panel branch list.

use super::prelude::*;

/// PyCharm branch-tree geometry: 20px rows, 16px indent, and the SVG tree
/// columns measured from the reference (chevron / icon / text).
const ROW_H: f32 = 20.0;
const ROW_MARGIN: f32 = 4.0;
const TREE_X: f32 = 18.0;
const HEADER_TEXT_X: f32 = 30.0;
const INDENT_STEP: f32 = 16.0;
const CHEVRON_COL: f32 = 13.0;
const ICON_COL: f32 = 30.0;
const TEXT_COL: f32 = 45.0;

/// The tree already gives a ref its parent folder's depth: a root-level ref is
/// depth 0 and sits with folders, a ref inside `backup` is depth 1 and sits one
/// column deeper. The current branch is pinned to column 0.
fn ref_shift(depth: usize, is_current: bool) -> f32 {
    if is_current {
        0.0
    } else {
        depth as f32 * INDENT_STEP
    }
}

fn ref_entry_name(entry: &lazecode_core::gitref::RefTreeEntry) -> &str {
    match entry {
        lazecode_core::gitref::RefTreeEntry::Dir(dir) => &dir.name,
        lazecode_core::gitref::RefTreeEntry::Ref(branch) => &branch.name,
        lazecode_core::gitref::RefTreeEntry::Tag { name, .. } => name,
    }
}

/// PyCharm puts the pinned/favourite ref first, then folders, then other refs.
fn sort_ref_entries(
    node: &mut lazecode_core::gitref::RefTreeNode,
    priority: &dyn Fn(&lazecode_core::gitref::RefTreeEntry) -> u8,
) {
    use lazecode_core::gitref::RefTreeEntry;
    node.entries.sort_by(|a, b| {
        priority(a)
            .cmp(&priority(b))
            .then_with(|| ref_entry_name(a).cmp(ref_entry_name(b)))
    });
    for entry in &mut node.entries {
        if let RefTreeEntry::Dir(dir) = entry {
            sort_ref_entries(dir, priority);
        }
    }
}

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
    // Sections: Local / Remote / Tags; the roots are pre-built above.
    let expanded_dirs: std::collections::HashSet<String> =
        model.projects[index].ref_dirs_expanded.clone();
    let mut toggle_dir: Option<String> = None;
    let mut toggle_section: Option<String> = None;
    let mut open_menu: Option<(String, bool, Rect)> = None; // branch name + remote flag + anchor
    let mut open_tag: Option<(String, Rect)> = None;

    let local_open = expanded_dirs.contains("Local");
    let remote_open = expanded_dirs.contains("Remote");
    let tags_open = expanded_dirs.contains("Tags");
    let mut local_tree = lazecode_core::gitref::build_ref_tree(&local);
    sort_ref_entries(&mut local_tree, &|entry| match entry {
        lazecode_core::gitref::RefTreeEntry::Ref(branch) if branch.name == current => 0,
        lazecode_core::gitref::RefTreeEntry::Dir(_) => 1,
        _ => 2,
    });
    let local_flat = lazecode_core::gitref::flatten_ref_tree(&local_tree, &expanded_dirs);
    let mut remote_tree = lazecode_core::gitref::build_ref_tree(&remote);
    sort_ref_entries(&mut remote_tree, &|entry| match entry {
        lazecode_core::gitref::RefTreeEntry::Ref(branch) => {
            if model.is_favorite_branch(&favorite_root, &branch.name) {
                0
            } else {
                2
            }
        }
        lazecode_core::gitref::RefTreeEntry::Dir(_) => 1,
        _ => 2,
    });
    let remote_flat = lazecode_core::gitref::flatten_ref_tree(&remote_tree, &expanded_dirs);

    let mut total_h = ROW_H; // HEAD (Current Branch)
    if local_open {
        total_h += ROW_H + local_flat.len() as f32 * ROW_H;
    }
    if remote_open && !remote.is_empty() {
        total_h += ROW_H + remote_flat.len() as f32 * ROW_H;
    }
    if tags_open && !tags.is_empty() {
        total_h += ROW_H + tags.len() as f32 * ROW_H;
    }
    total_h += ROW_MARGIN;

    crate::app_ui::region(ui, rows_rect, "branch-tree-region", |ui| {
        egui::ScrollArea::vertical()
            .id_salt("branch-tree-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let (container_rect, _) = ui
                    .allocate_exact_size(Vec2::new(ui.available_width(), total_h), Sense::hover());
                let mut y = container_rect.top();

                let tree_x = container_rect.left() + TREE_X;
                let draw_header = |ui: &mut Ui, label: &str, key: &str, y: &mut f32| {
                    let row = Rect::from_min_size(
                        Pos2::new(container_rect.left() + ROW_MARGIN, *y),
                        Vec2::new(container_rect.width() - 2.0 * ROW_MARGIN, ROW_H),
                    );
                    let response = ui.interact(
                        row,
                        ui.scope_id().with(("refsection", key.to_string())),
                        Sense::click(),
                    );
                    if response.hovered() {
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::same(4), p.hover);
                    }
                    let open = expanded_dirs.contains(key);
                    paint_icon(
                        ui.painter(),
                        Rect::from_center_size(
                            Pos2::new(container_rect.left() + 15.0, row.center().y),
                            Vec2::splat(12.0),
                        ),
                        if open {
                            Icon::ChevronDown
                        } else {
                            Icon::ChevronRight
                        },
                        p.text3,
                    );
                    ui.painter().text(
                        Pos2::new(container_rect.left() + HEADER_TEXT_X, row.center().y),
                        Align2::LEFT_CENTER,
                        label,
                        FontId::proportional(12.0),
                        p.text3,
                    );
                    *y += ROW_H;
                    response.clicked()
                };
                let draw_dir_row = |ui: &mut Ui,
                                    label: &str,
                                    path: &str,
                                    depth: usize,
                                    expanded: bool,
                                    y: &mut f32|
                 -> bool {
                    let row = Rect::from_min_size(
                        Pos2::new(container_rect.left() + ROW_MARGIN, *y),
                        Vec2::new(container_rect.width() - 2.0 * ROW_MARGIN, ROW_H),
                    );
                    let response = ui.interact(
                        row,
                        ui.scope_id().with(("refdir", path.to_string())),
                        Sense::click(),
                    );
                    if response.hovered() {
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::same(4), p.hover);
                    }
                    let shift = depth as f32 * INDENT_STEP;
                    let cy = row.center().y;
                    paint_icon(
                        ui.painter(),
                        Rect::from_center_size(
                            Pos2::new(tree_x + CHEVRON_COL + shift, cy),
                            Vec2::splat(12.0),
                        ),
                        if expanded {
                            Icon::ChevronDown
                        } else {
                            Icon::ChevronRight
                        },
                        p.text3,
                    );
                    paint_icon(
                        ui.painter(),
                        Rect::from_center_size(
                            Pos2::new(tree_x + ICON_COL + shift, cy),
                            Vec2::splat(13.0),
                        ),
                        Icon::Folder,
                        p.text3,
                    );
                    ui.painter().text(
                        Pos2::new(tree_x + TEXT_COL + shift, cy),
                        Align2::LEFT_CENTER,
                        label,
                        FontId::proportional(12.0),
                        p.text2,
                    );
                    *y += ROW_H;
                    response.clicked()
                };

                // HEAD (Current Branch)：纯标签行，和 PyCharm 一样 20px。
                ui.painter().text(
                    Pos2::new(container_rect.left() + HEADER_TEXT_X, y + ROW_H / 2.0),
                    Align2::LEFT_CENTER,
                    "HEAD (Current Branch)",
                    FontId::proportional(12.0),
                    p.text2,
                );
                y += ROW_H;
                if draw_header(ui, crate::tr!(model, "Local", "本地"), "Local", &mut y) {
                    toggle_section = Some("Local".to_string());
                }
                if local_open {
                    for row_ref in local_flat.iter() {
                        if row_ref.dir {
                            if draw_dir_row(
                                ui,
                                &row_ref.label,
                                &row_ref.path,
                                row_ref.depth,
                                expanded_dirs.contains(&row_ref.path),
                                &mut y,
                            ) {
                                toggle_dir = Some(row_ref.path.clone());
                            }
                        } else if row_ref.is_tag {
                            // Tags live in their own section below.
                        } else if let Some(branch) = row_ref.branch.as_ref() {
                            let row = Rect::from_min_size(
                                Pos2::new(container_rect.left() + ROW_MARGIN, y),
                                Vec2::new(container_rect.width() - 2.0 * ROW_MARGIN, ROW_H),
                            );
                            let is_current = branch.name == current;
                            let shift = ref_shift(row_ref.depth, is_current);
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
                            // PyCharm: favourite → amber star, current → amber tag,
                            // every other branch → muted fork glyph.
                            let is_fav = model.is_favorite_branch(&favorite_root, &branch.name);
                            let amber = Color32::from_rgb(0xe3, 0xb3, 0x41);
                            let (icon, tint) = if is_fav && !is_current {
                                (Icon::Star, amber)
                            } else if is_current {
                                (Icon::Tag, amber)
                            } else {
                                (Icon::Branch, p.text3)
                            };
                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(tree_x + ICON_COL + shift, row.center().y),
                                    Vec2::splat(12.0),
                                ),
                                icon,
                                tint,
                            );
                            let name_label = row_ref.label.clone();
                            // b-up-arrow：↑n ↓n 绿色 10px，margin-left auto。
                            let mut badge = String::new();
                            if branch.ahead > 0 {
                                badge.push_str(&format!("↑{}", branch.ahead));
                            }
                            if branch.behind > 0 {
                                badge.push_str(&format!(" ↓{}", branch.behind));
                            }
                            let text_right =
                                row.right() - if badge.is_empty() { 6.0 } else { 42.0 };
                            let cy = row.center().y;
                            text_clipped(
                                ui.painter(),
                                Rect::from_min_max(
                                    Pos2::new(tree_x + TEXT_COL + shift, cy - 8.0),
                                    Pos2::new(text_right, cy + 8.0),
                                ),
                                &name_label,
                                FontId::proportional(12.0),
                                p.text2,
                                Align2::LEFT_CENTER,
                            );
                            let response = response.on_hover_text(&branch.name);
                            if !badge.is_empty() {
                                ui.painter().text(
                                    Pos2::new(row.right() - 6.0, cy),
                                    Align2::RIGHT_CENTER,
                                    badge,
                                    FontId::proportional(10.0),
                                    p.text3,
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
                                    if let Some(pos) =
                                        selection.iter().position(|n| n == &branch.name)
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
                                    let pos1 =
                                        all_names.iter().position(|n| n == &anchor).unwrap_or(0);
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
                            y += ROW_H;
                        }
                    }
                }

                // Remote section.
                if !remote.is_empty()
                    && draw_header(ui, crate::tr!(model, "Remote", "远程"), "Remote", &mut y)
                {
                    toggle_section = Some("Remote".to_string());
                }
                if remote_open && !remote.is_empty() {
                    for row_ref in remote_flat.iter() {
                        if row_ref.dir {
                            if draw_dir_row(
                                ui,
                                &row_ref.label,
                                &row_ref.path,
                                row_ref.depth,
                                expanded_dirs.contains(&row_ref.path),
                                &mut y,
                            ) {
                                toggle_dir = Some(row_ref.path.clone());
                            }
                        } else if let Some(branch) = row_ref.branch.as_ref() {
                            let row = Rect::from_min_size(
                                Pos2::new(container_rect.left() + ROW_MARGIN, y),
                                Vec2::new(container_rect.width() - 2.0 * ROW_MARGIN, ROW_H),
                            );
                            let is_current = branch.name == current;
                            let shift = ref_shift(row_ref.depth, is_current);
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
                            let is_fav = model.is_favorite_branch(&favorite_root, &branch.name);
                            let is_current = branch.name == current;
                            let amber = Color32::from_rgb(0xe3, 0xb3, 0x41);
                            let (icon, tint) = if is_fav && !is_current {
                                (Icon::Star, amber)
                            } else if is_current {
                                (Icon::Tag, amber)
                            } else {
                                (Icon::Branch, p.text3)
                            };
                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(tree_x + ICON_COL + shift, row.center().y),
                                    Vec2::splat(12.0),
                                ),
                                icon,
                                tint,
                            );
                            let cy = row.center().y;
                            text_clipped(
                                ui.painter(),
                                Rect::from_min_max(
                                    Pos2::new(tree_x + TEXT_COL + shift, cy - 8.0),
                                    Pos2::new(row.right() - 6.0, cy + 8.0),
                                ),
                                &row_ref.label,
                                FontId::proportional(12.0),
                                p.text2,
                                Align2::LEFT_CENTER,
                            );
                            let response = response.on_hover_text(&branch.name);
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
                            y += ROW_H;
                        }
                    }
                }

                // Tags section：PyCharm 同样是可折叠 group row + 黄 tag 图标。
                if !tags.is_empty()
                    && draw_header(ui, crate::tr!(model, "Tags", "标签"), "Tags", &mut y)
                {
                    toggle_section = Some("Tags".to_string());
                }
                if tags_open && !tags.is_empty() {
                    for tag in tags.iter() {
                        let row = Rect::from_min_size(
                            Pos2::new(container_rect.left() + ROW_MARGIN, y),
                            Vec2::new(container_rect.width() - 2.0 * ROW_MARGIN, ROW_H),
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
                        // Tag rows share the folder/current-branch icon column.
                        let shift = 0.0;
                        paint_icon(
                            ui.painter(),
                            Rect::from_center_size(
                                Pos2::new(tree_x + ICON_COL + shift, row.center().y),
                                Vec2::splat(12.0),
                            ),
                            Icon::Tag,
                            p.amber,
                        );
                        ui.painter().text(
                            Pos2::new(tree_x + TEXT_COL + shift, row.center().y),
                            Align2::LEFT_CENTER,
                            tag,
                            FontId::proportional(12.0),
                            p.text2,
                        );
                        if response.secondary_clicked() {
                            let anchor = ui
                                .input(|i| i.pointer.hover_pos())
                                .map(|p| Rect::from_min_size(p, Vec2::splat(1.0)))
                                .unwrap_or(row);
                            open_tag = Some((tag.clone(), anchor));
                        }
                        y += ROW_H;
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
    if let Some(section) = toggle_section {
        if expanded_dirs.contains(&section) {
            model.projects[index].ref_dirs_expanded.remove(&section);
        } else {
            model.projects[index].ref_dirs_expanded.insert(section);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_refs_share_column_with_folders() {
        assert_eq!(ref_shift(0, true), 0.0);
        assert_eq!(ref_shift(0, false), 0.0);
        assert_eq!(ref_shift(1, false), INDENT_STEP);
        assert_eq!(ref_shift(4, false), 4.0 * INDENT_STEP);
    }
}
