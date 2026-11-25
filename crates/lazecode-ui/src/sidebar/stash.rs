//! Stash sidebar: collapsible stash tree and apply/pop actions.

use super::active;
use super::prelude::*;

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

pub(super) fn stash_view(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
) {
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
    crate::app_ui::region(ui, list, "stash-rows", |ui| {
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
