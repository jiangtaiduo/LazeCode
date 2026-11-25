//! Side-by-side branch compare drawer.

use super::prelude::*;

const CMP_HEADER_H: f32 = 34.0;
const CMP_TOOLBAR_H: f32 = 28.0;
const CMP_ROOT_H: f32 = 26.0;
const CMP_ROW_H: f32 = 24.0;

pub(super) struct CmpRow {
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
    // The parent sidebar already paints the Commit/Shelf surface; do not
    // overlay another translucent fill here or Compare turns darker.
    ui.painter().vline(
        rect.right() - 0.5,
        rect.top()..=rect.bottom(),
        Stroke::new(1.0, p.border),
    );

    // ---- 1. 头部 34px：Changes Between pill + close，右侧 ⋮ / — ----
    let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), CMP_HEADER_H));
    ui.painter().hline(
        head.x_range(),
        head.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    let from8: String = session.from.chars().take(8).collect();
    let to8: String = session.to.chars().take(8).collect();
    let title = if session.local {
        if zh {
            format!("{from8} 与本地之间的变更")
        } else {
            format!("Changes Between {from8} and Local")
        }
    } else if zh {
        format!("{from8} 与 {to8} 之间的变更")
    } else {
        format!("Changes Between {from8} and {to8}")
    };
    let title_rect = Rect::from_min_max(
        Pos2::new(head.left() + 12.0, head.top() + 4.0),
        Pos2::new(head.right() - 76.0, head.bottom() - 4.0),
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
        Pos2::new(head.right() - 70.0, head.center().y - 8.0),
        Vec2::new(16.0, 16.0),
    );
    if icon_16(ui, "cmp-close", close_btn, Icon::X, p.text3) {
        model.queue(Command::CompareClose);
        return;
    }
    let min_btn = Rect::from_min_size(
        Pos2::new(head.right() - 26.0, head.center().y - 9.0),
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
    ui.painter().hline(
        tb.x_range(),
        tb.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    let swap_btn = Rect::from_center_size(
        Pos2::new(tb.left() + 18.0, tb.center().y),
        Vec2::new(20.0, 20.0),
    );
    if icon_16_enabled(
        ui,
        "cmp-swap",
        swap_btn,
        Icon::Compare,
        p.text2,
        !session.local,
    ) {
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
            if session.local {
                if zh {
                    "该提交与本地工作区没有差异"
                } else {
                    "No differences between commit and local working tree"
                }
            } else if zh {
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

    // Keyboard: ↑/↓ step the file rows (directories stay click-only). The
    // change lands in the model, so the existing reveal-on-change logic
    // scrolls the new file into view on the next frame.
    let keys_free = !rt.blocks_input(model) && !rt.typing_surface_owns_keys(model, ui.ctx());
    if keys_free {
        if let Some(mv) = crate::list_nav::hovered_move(ui, body) {
            let files: Vec<&str> = rows
                .iter()
                .filter(|row| row.dir.is_none() && !row.is_root)
                .map(|row| row.path.as_str())
                .collect();
            let current = session
                .selected
                .as_deref()
                .and_then(|sel| files.iter().position(|path| *path == sel));
            if let Some(next) = crate::list_nav::stepped(current, files.len(), mv) {
                let path = files[next].to_string();
                if let Some(session) = model.projects[index].compare_session.as_mut() {
                    session.selected = Some(path);
                }
            }
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
                    let parent = row
                        .path
                        .rsplit_once('/')
                        .map(|(parent, _)| parent)
                        .unwrap_or("");
                    let name_font = FontId::proportional(12.0);
                    let path_font = FontId::proportional(10.5);
                    let num_font = FontId::monospace(10.5);
                    let add_label = (row.additions > 0 || row.deletions == 0)
                        .then(|| format!("+{}", row.additions));
                    let del_label = (row.deletions > 0).then(|| format!("-{}", row.deletions));
                    let add_w = add_label
                        .as_deref()
                        .map(|label| text_width(ui.painter(), label, num_font.clone()))
                        .unwrap_or(0.0);
                    let del_w = del_label
                        .as_deref()
                        .map(|label| text_width(ui.painter(), label, num_font.clone()))
                        .unwrap_or(0.0);
                    let num_w = add_w + del_w + if add_w > 0.0 && del_w > 0.0 { 6.0 } else { 0.0 };
                    let num_right = row_rect.right() - 10.0;
                    let num_left = num_right - num_w;
                    let text_left = row_rect.left() + indent + 12.0;
                    let text_right = num_left - if num_w > 0.0 { 8.0 } else { 2.0 };
                    let name_color = if row.selected {
                        Color32::WHITE
                    } else if row.status == 'A' || row.status == '?' || row.status == 'R' {
                        p.green
                    } else if row.status == 'D' {
                        p.rose
                    } else {
                        p.blue
                    };
                    if text_right > text_left {
                        let name_w = text_width(ui.painter(), base, name_font.clone());
                        if text_left + name_w > text_right {
                            text_clipped(
                                ui.painter(),
                                Rect::from_min_max(
                                    Pos2::new(text_left, row_rect.top()),
                                    Pos2::new(text_right, row_rect.bottom()),
                                ),
                                base,
                                name_font,
                                name_color,
                                Align2::LEFT_CENTER,
                            );
                        } else {
                            ui.painter().text(
                                Pos2::new(text_left, row_rect.center().y),
                                Align2::LEFT_CENTER,
                                base,
                                name_font,
                                name_color,
                            );
                            if !parent.is_empty() {
                                let path_left = text_left + name_w + 8.0;
                                if path_left + 4.0 < text_right {
                                    text_clipped(
                                        ui.painter(),
                                        Rect::from_min_max(
                                            Pos2::new(path_left, row_rect.top()),
                                            Pos2::new(text_right, row_rect.bottom()),
                                        ),
                                        parent,
                                        path_font,
                                        if row.selected {
                                            Color32::from_rgba_unmultiplied(255, 255, 255, 165)
                                        } else {
                                            p.text3
                                        },
                                        Align2::LEFT_CENTER,
                                    );
                                }
                            }
                        }
                    }
                    // Numstat is painted last so it always sits above the
                    // filename/path that was clipped to its left.
                    if let Some(label) = add_label {
                        ui.painter().text(
                            Pos2::new(num_left, row_rect.center().y),
                            Align2::LEFT_CENTER,
                            label,
                            num_font.clone(),
                            if row.selected {
                                Color32::from_rgba_unmultiplied(255, 255, 255, 220)
                            } else {
                                p.green
                            },
                        );
                    }
                    if let Some(label) = del_label {
                        ui.painter().text(
                            Pos2::new(
                                num_left + add_w + if add_w > 0.0 { 6.0 } else { 0.0 },
                                row_rect.center().y,
                            ),
                            Align2::LEFT_CENTER,
                            label,
                            num_font,
                            if row.selected {
                                Color32::from_rgba_unmultiplied(255, 255, 255, 220)
                            } else {
                                p.rose
                            },
                        );
                    }
                    let response = ui
                        .interact(
                            row_rect,
                            ui.scope_id().with(("cmp-file", row.path.as_str())),
                            Sense::click(),
                        )
                        .on_hover_text(format!(
                            "{} (+{} −{})",
                            row.path, row.additions, row.deletions
                        ));
                    if response.clicked() {
                        if let Some(session) = model.projects[index].compare_session.as_mut() {
                            session.selected = Some(row.path.clone());
                        }
                    }
                    if response.double_clicked() {
                        let target = row.path.clone();
                        model.ui.reveal_editor_area();
                        if session.local {
                            model.queue(Command::OpenCompareWithLocalDiff {
                                commit: session.from.clone(),
                                path: target.clone(),
                            });
                        } else {
                            model.queue(Command::OpenCompareCommitsDiff {
                                from: session.from.clone(),
                                to: session.to.clone(),
                                path: target.clone(),
                            });
                        }
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
pub(super) fn push_cmp_dirs(
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
            // 只收本层直接文件：不属于 prefix 的文件（尤其是根级
            // CHANGELOG/latest.json）不能在每一层重复出现，否则同一
            // 路径会生成重复的 egui ID。
            let Some(rest) = file.path.strip_prefix(prefix) else {
                return false;
            };
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
pub(super) fn response_clicked(ui: &mut Ui, rect: Rect, key: (&'static str, String)) -> bool {
    ui.interact(rect, ui.scope_id().with(key), Sense::click())
        .clicked()
}

/// 20px 图标按钮（hover 填充），返回是否本帧被点击。
pub(super) fn icon_16(
    ui: &mut Ui,
    key: &'static str,
    rect: Rect,
    icon: Icon,
    color: Color32,
) -> bool {
    icon_16_impl(ui, key, rect, icon, color, false, false)
}

pub(super) fn icon_16_active(
    ui: &mut Ui,
    key: &'static str,
    rect: Rect,
    icon: Icon,
    color: Color32,
    active: bool,
) -> bool {
    icon_16_impl(ui, key, rect, icon, color, active, false)
}

pub(super) fn icon_16_enabled(
    ui: &mut Ui,
    key: &'static str,
    rect: Rect,
    icon: Icon,
    color: Color32,
    enabled: bool,
) -> bool {
    icon_16_impl(ui, key, rect, icon, color, false, !enabled)
}

pub(super) fn icon_16_impl(
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

pub(super) fn p_hover() -> Color32 {
    crate::theme::Palette::dark().hover
}

/// 小折叠箭头（10-11px）。
pub(super) fn paint_chevron(
    painter: &egui::Painter,
    center: Pos2,
    open: bool,
    color: Color32,
    size: f32,
) {
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

// ---------------- Stash 页签（贮藏列表 + 可折叠文件树 + Apply/Pop） ----------------

#[cfg(test)]
mod compare_tree_tests {
    use super::*;

    fn file(path: &str) -> lazecode_git::CompareFile {
        lazecode_git::CompareFile {
            path: path.to_string(),
            status: 'M',
            additions: 1,
            deletions: 0,
        }
    }

    #[test]
    fn compare_tree_keeps_root_files_out_of_child_directories() {
        let files = vec![
            file("CHANGELOG.md"),
            file("src/a.rs"),
            file("src/nested/b.rs"),
        ];
        let mut rows = Vec::new();
        push_cmp_dirs(&files, &HashSet::new(), "", 1, &mut rows, None);

        let file_rows: Vec<&CmpRow> = rows.iter().filter(|row| row.dir.is_none()).collect();
        assert_eq!(file_rows.len(), 3);
        assert_eq!(
            file_rows
                .iter()
                .filter(|row| row.path == "CHANGELOG.md")
                .count(),
            1
        );
        assert_eq!(
            file_rows
                .iter()
                .filter(|row| row.path == "src/a.rs")
                .count(),
            1
        );
    }
}
