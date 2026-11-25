//! 分支比较对话框的纯函数辅助：提交过滤与文件树扁平化。
//! 移植自稳定版 `services/compareBranches.ts`（搜索/正则/大小写/作者/
//! 引用/日期过滤与目录树递归计数、稳定排序）。

mod pane;
mod prelude;

use pane::*;
use pane::{DETAILS_HEAD_H, FILE_ROW_H, ROOT_ROW_H};

use std::collections::HashSet;

use lazecode_core::{CompareBranchesSideView, CompareDateFilter};
use lazecode_git::CommitInfo;

/// 本地时区「今天零点」的 UTC 时间戳（libc localtime_r 取 gmtoff，
/// 避免引入 chrono）。
fn local_midnight(now: i64) -> i64 {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let seconds = now as libc::time_t;
    unsafe {
        if libc::localtime_r(&seconds, &mut tm).is_null() {
            return now;
        }
    }
    let offset = tm.tm_gmtoff as i64;
    let local = now + offset;
    let secs_of_day = local.rem_euclid(86_400);
    local - secs_of_day - offset
}

/// 日期过滤的 cutoff（UTC 秒）；`None` = 不过滤。
pub fn date_cutoff(filter: Option<CompareDateFilter>, now: i64) -> Option<i64> {
    match filter {
        None => None,
        Some(CompareDateFilter::Today) => Some(local_midnight(now)),
        Some(CompareDateFilter::Last7Days) => Some(now - 7 * 86_400),
        Some(CompareDateFilter::Last30Days) => Some(now - 30 * 86_400),
    }
}

enum Matcher {
    /// (needle, case-sensitive)
    Substr(String, bool),
    Regex(regex::Regex),
    /// 非法正则：对齐稳定版语义 = 无匹配，而不是抛异常。
    Never,
}

fn build_matcher(query: &str, use_regex: bool, match_case: bool) -> Option<Matcher> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    if use_regex {
        let mut builder = regex::RegexBuilder::new(query);
        builder.case_insensitive(!match_case);
        return Some(match builder.build() {
            Ok(regex) => Matcher::Regex(regex),
            Err(_) => Matcher::Never,
        });
    }
    Some(Matcher::Substr(
        if match_case {
            query.to_string()
        } else {
            query.to_lowercase()
        },
        match_case,
    ))
}

fn commit_matches(commit: &CommitInfo, matcher: &Matcher) -> bool {
    let mut haystack = String::new();
    haystack.push_str(&commit.message);
    haystack.push(' ');
    haystack.push_str(&commit.short);
    haystack.push(' ');
    haystack.push_str(&commit.id);
    haystack.push(' ');
    haystack.push_str(&commit.author);
    haystack.push(' ');
    haystack.push_str(&commit.email);
    for reference in &commit.refs {
        haystack.push(' ');
        haystack.push_str(&reference.label);
    }
    match matcher {
        Matcher::Never => false,
        Matcher::Substr(needle, case_sensitive) => {
            if *case_sensitive {
                haystack.contains(needle.as_str())
            } else {
                haystack.to_lowercase().contains(needle)
            }
        }
        Matcher::Regex(regex) => regex.is_match(&haystack),
    }
}

/// 过滤后的提交索引（保持显示顺序）。搜索范围对齐稳定版：message、
/// short/hash、author、authorEmail、refs 名。
pub fn filter_commits(
    commits: &[CommitInfo],
    view: &CompareBranchesSideView,
    now: i64,
) -> Vec<usize> {
    let matcher = build_matcher(&view.query, view.regex, view.match_case);
    let cutoff = date_cutoff(view.date, now);
    commits
        .iter()
        .enumerate()
        .filter(|(_, commit)| {
            if let Some(author) = &view.author {
                if &commit.author != author {
                    return false;
                }
            }
            if let Some(cutoff) = cutoff {
                if commit.time < cutoff {
                    return false;
                }
            }
            match &matcher {
                None => true,
                Some(matcher) => commit_matches(commit, matcher),
            }
        })
        .map(|(index, _)| index)
        .collect()
}

/// 提交侧里出现过的作者名（首见序），供 User 过滤菜单。
pub fn unique_authors(commits: &[CommitInfo]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for commit in commits {
        if !commit.author.is_empty() && !out.contains(&commit.author) {
            out.push(commit.author.clone());
        }
    }
    out
}

/// 提交侧里出现过的引用名（首见序），供 Branch 过滤菜单。
pub fn unique_refs(commits: &[CommitInfo]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for commit in commits {
        for reference in &commit.refs {
            let name = &reference.label;
            if !name.is_empty() && !out.contains(name) {
                out.push(name.clone());
            }
        }
    }
    out
}

/// 扁平化的文件树行（目录行带递归文件数）。
#[derive(Clone, Debug)]
pub struct FileRow {
    /// 目录 = 目录路径；文件 = 文件路径（作 id_salt / 折叠键）。
    pub key: String,
    pub label: String,
    pub depth: usize,
    pub is_dir: bool,
    /// 目录的递归文件数。
    pub count: usize,
    pub file: Option<lazecode_git::CompareFile>,
}

fn insert_into_tree(
    root: &mut DirNode,
    parts: &[&str],
    path: &str,
    file: lazecode_git::CompareFile,
) {
    if parts.len() == 1 {
        root.files.push((path.to_string(), file));
        return;
    }
    let head = parts[0];
    let child = root.dirs.entry(head.to_string()).or_default();
    insert_into_tree(child, &parts[1..], path, file);
}

#[derive(Default)]
struct DirNode {
    dirs: std::collections::BTreeMap<String, DirNode>,
    files: Vec<(String, lazecode_git::CompareFile)>,
}

fn count_files(node: &DirNode) -> usize {
    node.files.len() + node.dirs.values().map(count_files).sum::<usize>()
}

fn flatten(
    node: &DirNode,
    prefix: &str,
    depth: usize,
    collapsed: &HashSet<String>,
    out: &mut Vec<FileRow>,
) {
    // 目录在前、文件在后，各自按名称稳定排序（对齐稳定版树序）。
    for (name, child) in &node.dirs {
        let dir_path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let count = count_files(child);
        out.push(FileRow {
            key: dir_path.clone(),
            label: name.clone(),
            depth,
            is_dir: true,
            count,
            file: None,
        });
        if !collapsed.contains(&dir_path) {
            flatten(child, &dir_path, depth + 1, collapsed, out);
        }
    }
    for (path, file) in &node.files {
        let label = path.rsplit('/').next().unwrap_or(path).to_string();
        out.push(FileRow {
            key: path.clone(),
            label,
            depth,
            is_dir: false,
            count: 0,
            file: Some(file.clone()),
        });
    }
}

/// 把文件清单折叠成树行。`collapsed` 是折叠目录路径集合。
pub fn flatten_file_tree(
    files: &[lazecode_git::CompareFile],
    collapsed: &HashSet<String>,
) -> Vec<FileRow> {
    let mut root = DirNode::default();
    for file in files {
        let parts: Vec<&str> = file.path.split('/').collect();
        insert_into_tree(&mut root, &parts, &file.path, file.clone());
    }
    let mut out = Vec::new();
    flatten(&root, "", 0, collapsed, &mut out);
    out
}

// Page rendering: port of the stable CompareBranchesView (two panes, amber
// banner, filter row, commit log + detail column).

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::UiRuntime;
use crate::theme::Theme;
use crate::widgets::{secondary_button, text_clipped};
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use lazecode_core::{AppModel, Command};

const SPLITTER_H: f32 = 8.0;

/// 分支比较页主入口（TabKind::Compare 的内容区）。
pub fn draw_page(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    left: &str,
    right: &str,
) {
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.bg);

    // ---- 会话同步：项目/方向/路径不一致（新会话）时重置并触发后台比较。
    // root 必须参与：不同项目可能有同名分支，refs 相同不等于同一会话。----
    let session_root = model
        .active_project()
        .map(|project| project.active_root().to_path_buf())
        .unwrap_or_default();
    {
        let state = &model.ui.compare_branches;
        let stale = state.root != session_root
            || state.left_ref != left
            || state.right_ref != right
            || (!state.loading && state.sides.is_none() && state.error.is_none());
        if stale {
            model.ui.compare_branches = lazecode_core::CompareBranchesState {
                root: session_root.clone(),
                left_ref: left.to_string(),
                right_ref: right.to_string(),
                loading: true,
                ..Default::default()
            };
            model.queue(Command::CompareBranches {
                left: left.to_string(),
                right: right.to_string(),
                left_path: String::new(),
                right_path: String::new(),
            });
        }
    }

    // ---- 全局 loading / error+Retry（覆盖整页，cb-state 居中态）----
    if model.ui.compare_branches.loading {
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            crate::tr!(model, "Comparing branches…", "正在比较分支…"),
            FontId::proportional(12.0),
            p.text3,
        );
        return;
    }
    if let Some(err) = model.ui.compare_branches.error.clone() {
        ui.painter().text(
            Pos2::new(rect.center().x, rect.center().y - 16.0),
            Align2::CENTER_CENTER,
            crate::tr!(model, "Could not compare branches", "无法比较分支"),
            FontId::proportional(12.5),
            p.rose,
        );
        ui.painter().text(
            Pos2::new(rect.center().x, rect.center().y + 4.0),
            Align2::CENTER_CENTER,
            err,
            FontId::proportional(10.0),
            p.text3,
        );
        let retry = Rect::from_center_size(
            Pos2::new(rect.center().x, rect.center().y + 34.0),
            Vec2::new(80.0, 24.0),
        );
        if secondary_button(ui, retry, crate::tr!(model, "Retry", "重试"), None, theme).clicked()
        {
            let (l, r, lp, rp) = {
                let state = &mut model.ui.compare_branches;
                state.error = None;
                state.loading = true;
                (
                    state.left_ref.clone(),
                    state.right_ref.clone(),
                    state.left_path.clone(),
                    state.right_path.clone(),
                )
            };
            model.queue(Command::CompareBranches {
                left: l,
                right: r,
                left_path: lp,
                right_path: rp,
            });
        }
        return;
    }
    let Some(sides) = model.ui.compare_branches.sides.clone() else {
        return;
    };

    // ---- 键盘：⌘A 全选 / Esc 清空 / ↑↓ 移动单选（文本框聚焦时让位）----
    let text_focused = ui.memory(|memory| memory.focused().is_some());
    let now = ui.input(|input| input.time) as i64;
    if !text_focused {
        let (cmd, cmd_a, esc, up, down) = ui.input(|input| {
            (
                input.modifiers.command,
                input.key_pressed(egui::Key::A),
                input.key_pressed(egui::Key::Escape),
                input.key_pressed(egui::Key::ArrowUp),
                input.key_pressed(egui::Key::ArrowDown),
            )
        });
        if cmd && cmd_a {
            // 稳定版 ⌘A 只全选「焦点所在侧」；无焦点模型时与 ↑↓ 同规则：
            // 有选中的一侧优先，否则左侧。
            let state = &mut model.ui.compare_branches;
            if let Some(sides) = &state.sides {
                let use_right =
                    state.left_view.selected.is_empty() && !state.right_view.selected.is_empty();
                let (commits, view) = if use_right {
                    (&sides.right.commits, &mut state.right_view)
                } else {
                    (&sides.left.commits, &mut state.left_view)
                };
                view.select_all = true;
                view.selected = commits.iter().map(|c| c.id.clone()).collect();
                view.detail = None;
                view.selected_file = None;
            }
        }
        if esc {
            let state = &mut model.ui.compare_branches;
            for view in [&mut state.left_view, &mut state.right_view] {
                view.selected.clear();
                view.anchor = None;
                view.select_all = false;
                view.selected_file = None;
                view.detail = None;
            }
        }
        if up || down {
            let state = &mut model.ui.compare_branches;
            if let Some(sides) = &state.sides {
                let use_right =
                    state.left_view.selected.is_empty() && !state.right_view.selected.is_empty();
                let (commits, view) = if use_right {
                    (&sides.right.commits, &mut state.right_view)
                } else {
                    (&sides.left.commits, &mut state.left_view)
                };
                if !commits.is_empty() {
                    let current = view
                        .selected
                        .first()
                        .and_then(|id| commits.iter().position(|commit| &commit.id == id));
                    let next = match current {
                        Some(index) => (index as i64 + if down { 1 } else { -1 })
                            .clamp(0, commits.len() as i64 - 1)
                            as usize,
                        None => 0,
                    };
                    view.selected = vec![commits[next].id.clone()];
                    view.anchor = Some(commits[next].id.clone());
                    view.select_all = false;
                    view.selected_file = None;
                    // 键盘移动同样要丢弃上一提交的详情，避免加载间隙显示旧文件。
                    view.detail = None;
                }
            }
        }
    }

    // ---- 上下两个 pane + 8px 拖拽分隔（split 持久化）----
    let split = (model.settings.compare_split / 100.0).clamp(0.2, 0.8);
    let top_h = ((rect.height() - SPLITTER_H) * split).max(180.0);
    let top = Rect::from_min_size(rect.min, Vec2::new(rect.width(), top_h));
    let splitter = Rect::from_min_size(
        Pos2::new(rect.left(), top.bottom()),
        Vec2::new(rect.width(), SPLITTER_H),
    );
    let bottom = Rect::from_min_max(Pos2::new(rect.left(), splitter.bottom()), rect.max);
    draw_pane(
        ui,
        model,
        rt,
        theme,
        top,
        left,
        right,
        &sides.left,
        &sides.merge_base,
        0,
        now,
    );
    draw_pane(
        ui,
        model,
        rt,
        theme,
        bottom,
        right,
        left,
        &sides.right,
        &sides.merge_base,
        1,
        now,
    );

    // 8px 分隔条（.cb-splitter：bg-app + 居中 2px 发丝线，hover 变 accent）。
    let drag = ui.interact(splitter, ui.scope_id().with("cb-split"), Sense::drag());
    let hovered = drag.hovered() || drag.dragged();
    ui.painter().rect_filled(splitter, 0.0, p.bg);
    let line_y = splitter.top() + 3.0;
    ui.painter().hline(
        splitter.x_range(),
        line_y,
        Stroke::new(2.0, if hovered { p.accent } else { p.border_soft }),
    );
    if drag.dragged() {
        let total = (rect.height() - SPLITTER_H).max(1.0);
        model.settings.compare_split =
            (model.settings.compare_split + drag.drag_delta().y / total * 100.0).clamp(20.0, 80.0);
        rt.compare_split_drag = true;
    } else if rt.compare_split_drag {
        rt.compare_split_drag = false;
        model.save_settings();
    }
}

/// 20px 图标按钮（cb-icon-btn：hover bg-hover，active accent-soft+蓝）。
fn cb_icon_btn(
    ui: &mut Ui,
    rect: Rect,
    key: (&'static str, u8),
    icon: Icon,
    active: bool,
    enabled: bool,
) -> bool {
    let p = crate::theme::Palette::dark();
    let response = ui.interact(rect, ui.scope_id().with(key), Sense::click());
    if enabled && response.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(4), p.hover);
    } else if active {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(4), p.accent_soft);
    }
    let tint = if active {
        p.blue
    } else if enabled {
        p.text2
    } else {
        p.text3.gamma_multiply(0.4)
    };
    paint_icon(ui.painter(), rect.shrink(3.0), icon, tint);
    response.clicked() && enabled
}

/// h22 过滤文字按钮（cb-filter-btn）。
fn cb_filter_btn(
    ui: &mut Ui,
    rect: Rect,
    key: (&'static str, u8),
    label: &str,
    active: bool,
) -> bool {
    let p = crate::theme::Palette::dark();
    let response = ui.interact(rect, ui.scope_id().with(key), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(4), p.hover);
    } else if active {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(4), p.accent_soft);
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(12.5),
        if active { p.blue } else { p.text2 },
    );
    response.clicked()
}

/// 详情列：26px 头部（计数 + Expand/Collapse All + Diff）→ 文件树 → 元信息。
#[allow(clippy::too_many_arguments)]
fn draw_details(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    ref_name: &str,
    other_ref: &str,
    side: &lazecode_git::CompareSide,
    merge_base: &Option<String>,
    side_index: u8,
    selected: &[String],
    select_all: bool,
    collapsed: &std::collections::HashSet<String>,
    selected_file: &Option<String>,
) {
    let p = theme.palette;
    let zh = model.settings.language.is_zh();
    let side_index_us = usize::from(side_index);
    // .cb-details：bg-sidebar + 左缘分隔。
    ui.painter().rect_filled(rect, 0.0, p.sidebar);

    let view = if side_index == 0 {
        &model.ui.compare_branches.left_view
    } else {
        &model.ui.compare_branches.right_view
    };
    let detail = view.detail.clone();
    let _ = view;

    // 详情文件清单：⌘A = 整侧；单选/范围 = detail.files。
    let details_files: Vec<lazecode_git::CompareFile> = if select_all {
        side.files.clone()
    } else {
        match &detail {
            Some(detail) if !detail.loading && detail.error.is_none() => detail.files.clone(),
            _ => Vec::new(),
        }
    };
    let files_loading =
        !select_all && matches!(&detail, Some(detail) if detail.loading) && !selected.is_empty();

    // 头部 26px。
    let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), DETAILS_HEAD_H));
    ui.painter().hline(
        head.x_range(),
        head.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    let files_label = if zh {
        format!("{} 个文件变更", details_files.len())
    } else if details_files.len() == 1 {
        "1 file changed".to_string()
    } else {
        format!("{} files changed", details_files.len())
    };
    let title = if files_loading {
        crate::tr!(model, "Loading…", "加载中…").to_string()
    } else if select_all {
        let all = if zh {
            format!("已选择全部 {} 个提交 · ", side.commits.len())
        } else {
            format!("All {} selected · ", side.commits.len())
        };
        format!("{all}{files_label}")
    } else if selected.len() > 1 {
        let all = if zh {
            format!("已选择 {} 个提交 · ", selected.len())
        } else {
            format!("{} selected · ", selected.len())
        };
        format!("{all}{files_label}")
    } else if selected.len() == 1 && detail.as_ref().is_some_and(|d| d.error.is_some()) {
        crate::tr!(model, "Load failed", "加载失败").to_string()
    } else {
        files_label
    };
    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(head.left() + 10.0, head.top()),
            Pos2::new(head.right() - 70.0, head.bottom()),
        ),
        &title,
        FontId::proportional(11.0),
        p.text2,
        Align2::LEFT_CENTER,
    );
    // 展开全部目录路径（禁用判定：无目录时 Expand/Collapse 置灰）。
    let all_rows =
        crate::compare::flatten_file_tree(&details_files, &std::collections::HashSet::new());
    let has_dirs = all_rows.iter().any(|row| row.is_dir);
    let mut rx = head.right() - 4.0;
    let diff_rect = Rect::from_min_size(
        Pos2::new(rx - 20.0, head.center().y - 10.0),
        Vec2::splat(20.0),
    );
    rx -= 21.0;
    let collapse_rect = Rect::from_min_size(
        Pos2::new(rx - 20.0, head.center().y - 10.0),
        Vec2::splat(20.0),
    );
    rx -= 21.0;
    let expand_rect = Rect::from_min_size(
        Pos2::new(rx - 20.0, head.center().y - 10.0),
        Vec2::splat(20.0),
    );
    let diff_enabled = selected_file.is_some() || !selected.is_empty() || select_all;
    if cb_icon_btn(
        ui,
        diff_rect,
        ("cb-diff", side_index),
        Icon::Diff,
        false,
        diff_enabled,
    ) {
        if let Some(path) = selected_file {
            open_file_diff(
                model, side_index, side, merge_base, select_all, selected, path, other_ref,
            );
        } else {
            open_file_diff(
                model, side_index, side, merge_base, select_all, selected, "", other_ref,
            );
        }
    }
    if cb_icon_btn(
        ui,
        collapse_rect,
        ("cb-collapse", side_index),
        Icon::CollapseAll,
        false,
        has_dirs,
    ) {
        let view = if side_index == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.collapsed_dirs = all_rows
            .iter()
            .filter(|row| row.is_dir)
            .map(|row| row.key.clone())
            .collect();
        rt.cb_root_collapsed[side_index_us] = true;
    }
    if cb_icon_btn(
        ui,
        expand_rect,
        ("cb-expand", side_index),
        Icon::ExpandAll,
        false,
        has_dirs,
    ) {
        let view = if side_index == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.collapsed_dirs.clear();
        rt.cb_root_collapsed[side_index_us] = false;
    }

    // ---- 文件树（root 行 24px + 目录/文件行 21px）----
    let tree_rect = Rect::from_min_max(
        Pos2::new(rect.left(), head.bottom()),
        Pos2::new(rect.right(), rect.bottom()),
    );
    // 元信息高度（内容自适应，最大 44%）。
    let meta = meta_rect_content(
        model,
        side_index,
        ref_name,
        &side.commits,
        select_all,
        selected,
    );
    let meta_h = meta.1.min(tree_rect.height() * 0.44);
    let list_rect = Rect::from_min_max(
        tree_rect.min,
        Pos2::new(tree_rect.right(), tree_rect.bottom() - meta_h),
    );

    let root_collapsed = rt.cb_root_collapsed[side_index_us];
    let mut rows: Vec<crate::compare::FileRow> = Vec::new();
    if !details_files.is_empty() && !root_collapsed {
        rows = crate::compare::flatten_file_tree(&details_files, collapsed);
    }
    crate::app_ui::region(ui, list_rect, "cb-tree", |ui| {
        if details_files.is_empty() && !files_loading {
            ui.painter().text(
                Pos2::new(rect.left() + 10.0, list_rect.top() + 24.0),
                Align2::LEFT_CENTER,
                crate::tr!(model, "No changed files", "没有变更文件"),
                FontId::proportional(11.0),
                p.text3,
            );
        }
        let mut open_click: Option<String> = None;
        let mut select_click: Option<String> = None;
        let mut toggle_click: Option<String> = None;
        egui::ScrollArea::vertical()
            .id_salt(("cb-tree", side_index))
            .auto_shrink([false, false])
            .show_rows(
                ui,
                FILE_ROW_H,
                rows.len() + usize::from(!details_files.is_empty()),
                |ui, range| {
                    for row_index in range.clone() {
                        if row_index == 0 {
                            // 根行（cb-root-row）：chevron + folder + 仓库名 + 计数。
                            let (row, response) = ui.allocate_exact_size(
                                Vec2::new(list_rect.width(), ROOT_ROW_H),
                                Sense::click(),
                            );
                            if response.hovered() {
                                ui.painter()
                                    .rect_filled(row, egui::CornerRadius::ZERO, p.hover);
                            }
                            draw_chevron(
                                ui.painter(),
                                Pos2::new(row.left() + 11.0, row.center().y),
                                root_collapsed,
                                p.text3,
                            );
                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(row.left() + 24.0, row.center().y),
                                    Vec2::splat(13.0),
                                ),
                                Icon::Folder,
                                Color32::from_rgb(0xdc, 0xd7, 0xaa),
                            );
                            let repo_name = model
                                .active_project()
                                .map(|pr| pr.name.clone())
                                .unwrap_or_else(|| "Repository".to_string());
                            ui.painter().text(
                                Pos2::new(row.left() + 36.0, row.center().y),
                                Align2::LEFT_CENTER,
                                repo_name,
                                FontId::proportional(13.0),
                                p.text_bright,
                            );
                            let count = if zh {
                                format!("{} 个文件", details_files.len())
                            } else if details_files.len() == 1 {
                                "1 file".to_string()
                            } else {
                                format!("{} files", details_files.len())
                            };
                            ui.painter().text(
                                Pos2::new(row.right() - 8.0, row.center().y),
                                Align2::RIGHT_CENTER,
                                count,
                                FontId::proportional(11.5),
                                p.text3,
                            );
                            if response.clicked() {
                                rt.cb_root_collapsed[side_index_us] =
                                    !rt.cb_root_collapsed[side_index_us];
                            }
                            if range.end <= 1 {
                                break;
                            }
                            continue;
                        }
                        let Some(file_row) = rows.get(row_index - 1) else {
                            continue;
                        };
                        let (row, response) = ui.allocate_exact_size(
                            Vec2::new(list_rect.width(), FILE_ROW_H),
                            Sense::click(),
                        );
                        let indent = 8.0 + file_row.depth as f32 * 12.0;
                        let is_selected = !file_row.is_dir
                            && selected_file.as_deref() == Some(file_row.key.as_str());
                        if is_selected {
                            ui.painter()
                                .rect_filled(row, egui::CornerRadius::ZERO, p.jb_accent);
                        } else if response.hovered() {
                            ui.painter()
                                .rect_filled(row, egui::CornerRadius::ZERO, p.hover);
                        }
                        if file_row.is_dir {
                            let open = !collapsed.contains(&file_row.key);
                            draw_chevron(
                                ui.painter(),
                                Pos2::new(row.left() + indent + 5.0, row.center().y),
                                open,
                                p.text3,
                            );
                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(row.left() + indent + 18.0, row.center().y),
                                    Vec2::splat(13.0),
                                ),
                                Icon::Folder,
                                Color32::from_rgb(0xdc, 0xd7, 0xaa),
                            );
                            ui.painter().text(
                                Pos2::new(row.left() + indent + 30.0, row.center().y),
                                Align2::LEFT_CENTER,
                                &file_row.label,
                                FontId::proportional(13.0),
                                p.text,
                            );
                            let count = if zh {
                                format!("{} 个文件", file_row.count)
                            } else if file_row.count == 1 {
                                "1 file".to_string()
                            } else {
                                format!("{} files", file_row.count)
                            };
                            ui.painter().text(
                                Pos2::new(row.right() - 8.0, row.center().y),
                                Align2::RIGHT_CENTER,
                                count,
                                FontId::proportional(11.5),
                                p.text3,
                            );
                            if response.clicked() {
                                toggle_click = Some(file_row.key.clone());
                            }
                        } else {
                            let Some(file) = &file_row.file else { continue };
                            let status = file.status.to_ascii_uppercase();
                            let name_color = if is_selected {
                                Color32::WHITE
                            } else if status == 'A' || status == '?' || status == 'R' {
                                p.green
                            } else if status == 'D' {
                                p.rose
                            } else {
                                p.blue
                            };
                            paint_icon(
                                ui.painter(),
                                Rect::from_center_size(
                                    Pos2::new(row.left() + indent + 7.0, row.center().y),
                                    Vec2::splat(13.0),
                                ),
                                Icon::File,
                                if is_selected { Color32::WHITE } else { p.text3 },
                            );
                            ui.painter().text(
                                Pos2::new(row.left() + indent + 19.0, row.center().y),
                                Align2::LEFT_CENTER,
                                &file_row.label,
                                FontId::proportional(13.0),
                                name_color,
                            );
                            // numstat（mono 10px，+绿 / -红，仅 >0）。
                            let mut nx = row.right() - 8.0;
                            if file.deletions > 0 {
                                let label = format!("-{}", file.deletions);
                                nx -= label.chars().count() as f32 * 6.0;
                                ui.painter().text(
                                    Pos2::new(nx, row.center().y),
                                    Align2::LEFT_CENTER,
                                    label,
                                    FontId::monospace(11.5),
                                    if is_selected {
                                        Color32::from_rgba_unmultiplied(255, 255, 255, 219)
                                    } else {
                                        p.rose
                                    },
                                );
                                nx -= 5.0;
                            }
                            if file.additions > 0 || file.deletions == 0 {
                                let label = format!("+{}", file.additions);
                                nx -= label.chars().count() as f32 * 6.0;
                                ui.painter().text(
                                    Pos2::new(nx, row.center().y),
                                    Align2::LEFT_CENTER,
                                    label,
                                    FontId::monospace(11.5),
                                    if is_selected {
                                        Color32::from_rgba_unmultiplied(255, 255, 255, 219)
                                    } else {
                                        p.green
                                    },
                                );
                            }
                            if response.clicked() {
                                select_click = Some(file_row.key.clone());
                            }
                            if response.double_clicked() {
                                open_click = Some(file_row.key.clone());
                            }
                        }
                    }
                },
            );
        if let Some(key) = toggle_click {
            let view = if side_index == 0 {
                &mut model.ui.compare_branches.left_view
            } else {
                &mut model.ui.compare_branches.right_view
            };
            if view.collapsed_dirs.contains(&key) {
                view.collapsed_dirs.remove(&key);
            } else {
                view.collapsed_dirs.insert(key);
            }
        }
        if let Some(path) = select_click {
            let view = if side_index == 0 {
                &mut model.ui.compare_branches.left_view
            } else {
                &mut model.ui.compare_branches.right_view
            };
            view.selected_file = Some(path);
        }
        if let Some(path) = open_click {
            open_file_diff(
                model, side_index, side, merge_base, select_all, selected, &path, other_ref,
            );
        }
    });

    // ---- 元信息（cb-details-meta：message/hash/email/date/inBranches）----
    if meta_h > 0.0 {
        let meta_rect = Rect::from_min_max(Pos2::new(rect.left(), list_rect.bottom()), rect.max);
        ui.painter().hline(
            meta_rect.x_range(),
            meta_rect.top() + 0.5,
            Stroke::new(1.0, p.border_soft),
        );
        let (message, hash, email, date_line, branches_line) = meta.0;
        let mut y = meta_rect.top() + 7.0;
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(meta_rect.left() + 10.0, y),
                Pos2::new(meta_rect.right() - 10.0, y + 17.0),
            ),
            &message,
            FontId::proportional(13.0),
            p.text_bright,
            Align2::LEFT_TOP,
        );
        y += 20.0;
        if !hash.is_empty() {
            ui.painter().text(
                Pos2::new(meta_rect.left() + 10.0, y + 7.0),
                Align2::LEFT_CENTER,
                &hash,
                FontId::monospace(12.0),
                p.accent,
            );
            let hash_w = hash.chars().count() as f32 * 7.2 + 10.0;
            ui.painter().text(
                Pos2::new(meta_rect.left() + 10.0 + hash_w, y + 7.0),
                Align2::LEFT_CENTER,
                &email,
                FontId::proportional(12.0),
                p.text2,
            );
            y += 16.0;
        }
        if !date_line.is_empty() {
            ui.painter().text(
                Pos2::new(meta_rect.left() + 10.0, y + 7.0),
                Align2::LEFT_CENTER,
                &date_line,
                FontId::proportional(12.0),
                p.text3,
            );
            y += 16.0;
        }
        if !branches_line.is_empty() {
            ui.painter().hline(
                meta_rect.x_range(),
                y + 2.0,
                Stroke::new(1.0, p.border_soft),
            );
            ui.painter().text(
                Pos2::new(meta_rect.left() + 10.0, y + 12.0),
                Align2::LEFT_CENTER,
                &branches_line,
                FontId::proportional(12.0),
                p.text3,
            );
        }
    }
}

type MetaContent = (String, String, String, String, String);

/// 元信息内容 + 估算高度（cb-details-meta）。
#[allow(clippy::type_complexity)]
fn meta_rect_content(
    model: &AppModel,
    _side_index: u8,
    ref_name: &str,
    commits: &[lazecode_git::CommitInfo],
    select_all: bool,
    selected: &[String],
) -> (MetaContent, f32) {
    let zh = model.settings.language.is_zh();
    if select_all {
        let message = if zh {
            format!("已选择全部 {} 个提交", commits.len())
        } else {
            format!("All {} commits selected", commits.len())
        };
        return (
            (
                message,
                String::new(),
                String::new(),
                ref_name.to_string(),
                String::new(),
            ),
            46.0,
        );
    }
    // 详情展示第一个被选中的提交（显示序 = 最新在前）。
    let Some(details_commit) = selected
        .first()
        .and_then(|id| commits.iter().find(|commit| &commit.id == id))
    else {
        return (
            (
                crate::tr!(model, "No commit selected", "未选择提交").to_string(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            ),
            32.0,
        );
    };
    let message = details_commit.message.clone();
    let hash = details_commit.short.clone();
    let email = details_commit.email.clone();
    let date_line = if zh {
        format!(
            "{email} 于 {}",
            crate::panel::format_commit_datetime(details_commit.time)
        )
    } else {
        format!(
            "{email} on {}",
            crate::panel::format_commit_datetime(details_commit.time)
        )
    };
    let branches: Vec<String> = details_commit
        .refs
        .iter()
        .filter(|r| {
            matches!(
                r.kind,
                lazecode_git::RefKind::LocalBranch
                    | lazecode_git::RefKind::Head
                    | lazecode_git::RefKind::RemoteBranch
            )
        })
        .map(|r| r.label.clone())
        .collect();
    let branches_line = if branches.is_empty() {
        String::new()
    } else if zh {
        format!("存在于 {} 个分支：{}", branches.len(), branches.join(", "))
    } else if branches.len() == 1 {
        format!("In 1 branch: {}", branches.join(", "))
    } else {
        format!("In {} branches: {}", branches.len(), branches.join(", "))
    };
    let height = 27.0
        + 16.0
        + usize::from(!date_line.is_empty()) as f32 * 16.0
        + usize::from(!branches_line.is_empty()) as f32 * 20.0;
    ((message, hash, email, date_line, branches_line), height)
}

/// 10px 折叠箭头。
fn draw_chevron(painter: &egui::Painter, center: Pos2, open: bool, color: Color32) {
    use egui::Stroke;
    let r = 3.2;
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

/// File-diff source, stable rules: ⌘A whole side = merge-base..side; multi
/// selection = parent of oldest..newest (empty tree for a root); else the
/// commit itself.
#[allow(clippy::too_many_arguments)]
fn open_file_diff(
    model: &mut AppModel,
    side_index: u8,
    side: &lazecode_git::CompareSide,
    merge_base: &Option<String>,
    select_all: bool,
    selected: &[String],
    path: &str,
    other_ref: &str,
) {
    const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
    if select_all || selected.is_empty() {
        let from = merge_base.clone().unwrap_or_else(|| other_ref.to_string());
        let to = if side_index == 0 {
            model.ui.compare_branches.left_ref.clone()
        } else {
            model.ui.compare_branches.right_ref.clone()
        };
        model.queue(Command::OpenCompareCommitsDiff {
            from,
            to,
            path: path.to_string(),
        });
        return;
    }
    if selected.len() == 1 {
        model.queue(Command::OpenCommitDiff {
            commit: selected[0].clone(),
            path: path.to_string(),
        });
        return;
    }
    let positions: Vec<usize> = selected
        .iter()
        .filter_map(|id| side.commits.iter().position(|commit| &commit.id == id))
        .collect();
    let (newest, oldest) = match (positions.iter().min(), positions.iter().max()) {
        (Some(newest), Some(oldest)) => (*newest, *oldest),
        _ => return,
    };
    let from = side.commits[oldest]
        .parents
        .first()
        .cloned()
        .unwrap_or_else(|| EMPTY_TREE.to_string());
    let to = side.commits[newest].id.clone();
    model.queue(Command::OpenCompareCommitsDiff {
        from,
        to,
        path: path.to_string(),
    });
}
