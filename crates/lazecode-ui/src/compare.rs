//! 分支比较对话框的纯函数辅助：提交过滤与文件树扁平化。
//! 移植自稳定版 `services/compareBranches.ts`（搜索/正则/大小写/作者/
//! 引用/日期过滤与目录树递归计数、稳定排序）。

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
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2};
use lazecode_core::{AppModel, Command};

const SPLITTER_H: f32 = 8.0;
const BANNER_H: f32 = 28.0;
const TOOLBAR_H: f32 = 30.0;
const ROW_H: f32 = 26.0;
const DETAILS_HEAD_H: f32 = 26.0;
const ROOT_ROW_H: f32 = 24.0;
const FILE_ROW_H: f32 = 24.0;
const AUTHOR_W: f32 = 80.0;
const DATE_W: f32 = 108.0;
const RULER_GAP: f32 = 10.0;

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

/// 单侧 pane：banner（方向语义，left 侧带 Swap）→ 过滤工具行 →
/// [提交日志 | 详情列]（详情列宽两侧共用、可拖）。
#[allow(clippy::too_many_arguments)]
fn draw_pane(
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
    now: i64,
) {
    let p = theme.palette;
    let zh = model.settings.language.is_zh();
    let side_index_us = usize::from(side_index);

    // ---- banner：28px 琥珀色提示条（rgba(227,179,65,0.1)）----
    let banner = Rect::from_min_size(rect.min, Vec2::new(rect.width(), BANNER_H));
    ui.painter()
        .rect_filled(banner, 0.0, p.amber.gamma_multiply(0.10));
    ui.painter().hline(
        banner.x_range(),
        banner.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(banner.left() + 16.0, banner.center().y),
            Vec2::splat(13.0),
        ),
        Icon::Alert,
        p.amber,
    );
    let banner_text = if zh {
        format!("存在于 {ref_name} 但不存在于 {other_ref} 的提交")
    } else {
        format!("Commits that exist in {ref_name} but don't exist in {other_ref}")
    };
    let text_right = if side_index == 0 {
        banner.right() - 100.0
    } else {
        banner.right() - 10.0
    };
    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(banner.left() + 28.0, banner.top()),
            Pos2::new(text_right, banner.bottom()),
        ),
        &banner_text,
        FontId::proportional(13.0),
        p.amber,
        Align2::LEFT_CENTER,
    );
    if side_index == 0 {
        // 稳定版 Swap 只出现在 left（上）pane：cb-swap-btn [⇄ Swap Branches]。
        let label = if zh {
            "⇄ 交换分支"
        } else {
            "⇄ Swap Branches"
        };
        let w = label.chars().count() as f32 * 6.0 + 18.0;
        let swap = Rect::from_min_size(
            Pos2::new(banner.right() - 8.0 - w, banner.center().y - 10.0),
            Vec2::new(w, 20.0),
        );
        let resp = ui.interact(swap, ui.scope_id().with("cb-swap"), Sense::click());
        if resp.hovered() {
            ui.painter()
                .rect_filled(swap, egui::CornerRadius::same(4), p.hover);
            ui.painter().rect_stroke(
                swap,
                egui::CornerRadius::same(4),
                Stroke::new(1.0, p.border_soft),
                StrokeKind::Inside,
            );
        }
        ui.painter().text(
            swap.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(12.5),
            if resp.hovered() {
                p.text_bright
            } else {
                p.text2
            },
        );
        if resp.clicked() {
            {
                let state = &mut model.ui.compare_branches;
                std::mem::swap(&mut state.left_ref, &mut state.right_ref);
                std::mem::swap(&mut state.left_path, &mut state.right_path);
                state.sides = None;
                state.error = None;
                // 换向后旧选择/详情全部作废（回填成功也会再清一次兜底）。
                for view in [&mut state.left_view, &mut state.right_view] {
                    view.selected.clear();
                    view.anchor = None;
                    view.select_all = false;
                    view.selected_file = None;
                    view.detail = None;
                }
                state.loading = true;
            }
            // 换向后同步标签页身份（标题/refs），下一帧整页重查。
            let (l, r, title) = {
                let state = &model.ui.compare_branches;
                let tab_title = if zh {
                    format!("比较：{} 和 {}", state.left_ref, state.right_ref)
                } else {
                    format!("Compare: {} and {}", state.left_ref, state.right_ref)
                };
                (state.left_ref.clone(), state.right_ref.clone(), tab_title)
            };
            model
                .active_project_mut()
                .unwrap()
                .open_compare_branches(l, r, title);
        }
    }

    // ---- 过滤工具行（30px，bg-app，搜索框内嵌 .*/Cc）----
    let toolbar = Rect::from_min_size(
        Pos2::new(rect.left(), banner.bottom()),
        Vec2::new(rect.width(), TOOLBAR_H),
    );
    ui.painter().rect_filled(toolbar, 0.0, p.bg);
    ui.painter().hline(
        toolbar.x_range(),
        toolbar.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );

    let view_ref = if side_index == 0 {
        &model.ui.compare_branches.left_view
    } else {
        &model.ui.compare_branches.right_view
    };
    let user_active = view_ref.author.is_some();
    let date_active = view_ref.date.is_some();
    let ref_active = view_ref.selected_ref_filter.is_some();
    let path_active = !view_ref.path.is_empty();
    let filter_count =
        usize::from(user_active) + usize::from(date_active) + usize::from(ref_active);
    let show_graph = rt.cb_show_graph[side_index_us];
    let has_side_data = !side.commits.is_empty() || !side.files.is_empty();
    let _ = has_side_data;
    let _ = view_ref;

    // 右组：refresh 20 + graph 20（margin-left auto 语义，从右往左布）。
    let mut rx = toolbar.right() - 6.0;
    let refresh_rect =
        Rect::from_min_size(Pos2::new(rx - 20.0, toolbar.top() + 5.0), Vec2::splat(20.0));
    rx -= 22.0;
    let graph_rect =
        Rect::from_min_size(Pos2::new(rx - 20.0, toolbar.top() + 5.0), Vec2::splat(20.0));
    rx -= 20.0;
    let clear_rect =
        Rect::from_min_size(Pos2::new(rx - 18.0, toolbar.top() + 6.0), Vec2::splat(18.0));
    let clear_w = if filter_count > 0 { 20.0 } else { 0.0 };
    // 从右往左的按钮组（先画右侧固定组）。
    if cb_icon_btn(
        ui,
        refresh_rect,
        ("cb-refresh", side_index),
        Icon::Refresh,
        false,
        true,
    ) {
        let state = &mut model.ui.compare_branches;
        state.loading = true;
        let (l, r, lp, rp) = (
            state.left_ref.clone(),
            state.right_ref.clone(),
            state.left_path.clone(),
            state.right_path.clone(),
        );
        model.queue(Command::CompareBranches {
            left: l,
            right: r,
            left_path: lp,
            right_path: rp,
        });
        return;
    }
    if cb_icon_btn(
        ui,
        graph_rect,
        ("cb-graph", side_index),
        Icon::GitLog,
        show_graph,
        true,
    ) {
        rt.cb_show_graph[side_index_us] = !show_graph;
    }
    let lx_left = toolbar.left() + 6.0;
    if filter_count > 0
        && cb_icon_btn(
            ui,
            clear_rect,
            ("cb-clear", side_index),
            Icon::X,
            false,
            true,
        )
    {
        // 稳定版 Clear 只清 author/date/ref/path，不动搜索词。
        let view = if side_index == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.author = None;
        view.date = None;
        view.selected_ref_filter = None;
        if !view.path.is_empty() {
            view.path.clear();
            state_reload(model);
            return;
        }
    }
    // Paths / User / Date / ⑂（从右往左排）。
    let paths_label = crate::tr!(model, "Paths", "路径");
    let user_label = crate::tr!(model, "User", "用户");
    let date_label = crate::tr!(model, "Date", "日期");
    let btn_w = |label: &str| label.chars().count() as f32 * 7.2 + 18.0;
    let paths_w = btn_w(paths_label);
    let user_w = btn_w(user_label);
    let date_w = btn_w(date_label);
    let ref_w = 26.0;
    let mut fx = rx - clear_w - 2.0;
    let paths_rect = Rect::from_min_size(
        Pos2::new(fx - paths_w, toolbar.top() + 3.0),
        Vec2::new(paths_w, 24.0),
    );
    fx -= paths_w + 3.0;
    let date_rect = Rect::from_min_size(
        Pos2::new(fx - date_w, toolbar.top() + 3.0),
        Vec2::new(date_w, 24.0),
    );
    fx -= date_w + 3.0;
    let user_rect = Rect::from_min_size(
        Pos2::new(fx - user_w, toolbar.top() + 3.0),
        Vec2::new(user_w, 24.0),
    );
    fx -= user_w + 3.0;
    let ref_rect = Rect::from_min_size(
        Pos2::new(fx - ref_w, toolbar.top() + 3.0),
        Vec2::new(ref_w, 24.0),
    );
    let search_right = ref_rect.left() - 3.0;

    if cb_filter_btn(
        ui,
        paths_rect,
        ("cb-paths", side_index),
        paths_label,
        path_active,
    ) {
        let view = if side_index == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.path_edit_open = !view.path_edit_open;
        view.path_edit = view.path.clone();
    }
    if cb_filter_btn(
        ui,
        user_rect,
        ("cb-user", side_index),
        user_label,
        user_active,
    ) {
        rt.open_menu(
            crate::runtime::MenuKind::CompareUserFilter { side: side_index },
            user_rect,
            None,
        );
    }
    if cb_filter_btn(
        ui,
        date_rect,
        ("cb-date", side_index),
        date_label,
        date_active,
    ) {
        rt.open_menu(
            crate::runtime::MenuKind::CompareDateFilter { side: side_index },
            date_rect,
            None,
        );
    }
    // Ref 过滤：仅图标按钮（GitBranchIcon 11）。
    if cb_icon_btn(
        ui,
        ref_rect,
        ("cb-ref", side_index),
        Icon::Branch,
        ref_active,
        true,
    ) {
        rt.open_menu(
            crate::runtime::MenuKind::CompareRefFilter { side: side_index },
            ref_rect,
            None,
        );
    }

    // 搜索框（cb-search-box：flex 1 1 120px，h22，内嵌 Q▾ + .*/Cc）。
    let search_rect = Rect::from_min_max(
        Pos2::new(lx_left, toolbar.top() + 4.0),
        Pos2::new(search_right, toolbar.bottom() - 4.0),
    );
    if search_rect.width() > 60.0 {
        ui.painter()
            .rect_filled(search_rect, egui::CornerRadius::same(4), p.hairline);
        paint_icon(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(search_rect.left() + 10.0, search_rect.center().y),
                Vec2::splat(12.0),
            ),
            Icon::Search,
            p.text3,
        );
        let toggle_w = 22.0;
        let mut query = {
            let view = if side_index == 0 {
                &model.ui.compare_branches.left_view
            } else {
                &model.ui.compare_branches.right_view
            };
            view.query.clone()
        };
        let input = Rect::from_min_max(
            Pos2::new(search_rect.left() + 20.0, search_rect.top() + 1.0),
            Pos2::new(
                search_rect.right() - toggle_w * 2.0 - 4.0,
                search_rect.bottom() - 1.0,
            ),
        );
        let resp = ui.put(
            input,
            TextEdit::singleline(&mut query)
                .frame(egui::Frame::NONE)
                .align(egui::Align2::LEFT_CENTER)
                .hint_text(crate::tr!(model, "Text or hash", "搜索提交、哈希或作者"))
                .font(FontId::proportional(12.5)),
        );
        if resp.changed() {
            let view = if side_index == 0 {
                &mut model.ui.compare_branches.left_view
            } else {
                &mut model.ui.compare_branches.right_view
            };
            view.query = query;
        }
        let (use_regex, match_case) = {
            let view = if side_index == 0 {
                &model.ui.compare_branches.left_view
            } else {
                &model.ui.compare_branches.right_view
            };
            (view.regex, view.match_case)
        };
        let regex_rect = Rect::from_min_size(
            Pos2::new(
                search_rect.right() - toggle_w * 2.0 - 2.0,
                search_rect.center().y - 9.0,
            ),
            Vec2::new(toggle_w, 18.0),
        );
        let case_rect = Rect::from_min_size(
            Pos2::new(
                search_rect.right() - toggle_w - 2.0,
                search_rect.center().y - 9.0,
            ),
            Vec2::new(toggle_w, 18.0),
        );
        let _ = has_side_data;
        if cb_toggle(ui, regex_rect, ("cb-regex", side_index), ".*", use_regex) {
            let view = if side_index == 0 {
                &mut model.ui.compare_branches.left_view
            } else {
                &mut model.ui.compare_branches.right_view
            };
            view.regex = !view.regex;
        }
        if cb_toggle(ui, case_rect, ("cb-case", side_index), "Cc", match_case) {
            let view = if side_index == 0 {
                &mut model.ui.compare_branches.left_view
            } else {
                &mut model.ui.compare_branches.right_view
            };
            view.match_case = !view.match_case;
        }
    }

    // ---- 过滤后的可见提交（搜索/作者/日期/引用过滤，保持显示顺序）----
    let visible: Vec<usize> = {
        let view = if side_index == 0 {
            &model.ui.compare_branches.left_view
        } else {
            &model.ui.compare_branches.right_view
        };
        let mut indices = filter_commits(&side.commits, view, now);
        if let Some(filter_ref) = &view.selected_ref_filter {
            indices.retain(|index| {
                side.commits[*index]
                    .refs
                    .iter()
                    .any(|reference| &reference.label == filter_ref)
            });
        }
        indices
    };

    let body = Rect::from_min_max(Pos2::new(rect.left(), toolbar.bottom()), rect.max);

    // ---- [提交日志 | 详情列]（详情宽两侧共用、可拖，稳定版 24-62%）----
    let details_frac = (model.settings.compare_details_width / 100.0).clamp(0.24, 0.62);
    let details_w = (body.width() * details_frac).max(170.0);
    let list = Rect::from_min_max(body.min, Pos2::new(body.right() - details_w, body.bottom()));
    let details = Rect::from_min_max(Pos2::new(list.right() + 1.0, body.min.y), body.max);
    let divider = Rect::from_min_size(
        Pos2::new(list.right() - 4.0, body.min.y),
        Vec2::new(8.0, body.height()),
    );

    // 提交日志列（无表头；空态居中偏上提示）。
    let filter_active = {
        let view = if side_index == 0 {
            &model.ui.compare_branches.left_view
        } else {
            &model.ui.compare_branches.right_view
        };
        !view.query.trim().is_empty()
            || view.author.is_some()
            || view.date.is_some()
            || view.selected_ref_filter.is_some()
    };
    let empty_reason: String = if filter_active {
        crate::tr!(model, "No matching commits", "没有匹配的提交").to_string()
    } else if zh {
        format!("{ref_name} 没有独有提交")
    } else {
        format!("No commits unique to {ref_name}")
    };
    let (selected, anchor) = {
        let view = if side_index == 0 {
            &model.ui.compare_branches.left_view
        } else {
            &model.ui.compare_branches.right_view
        };
        (view.selected.clone(), view.anchor.clone())
    };

    let mut click: Option<(String, bool, bool)> = None;
    let mut double_click: Option<String> = None;
    let footer_row = side.truncated;
    let row_total = visible.len() + usize::from(footer_row);
    let show_graph = rt.cb_show_graph[side_index_us];
    // 稳定版每 pane 独立 graphLayout（computeGraphLayout）。
    let dag = if show_graph {
        lazecode_git::compute_dag(&side.commits)
    } else {
        lazecode_git::DagLayout::default()
    };
    crate::app_ui::region(ui, list, "cb-list", |ui| {
        if visible.is_empty() {
            ui.painter().text(
                Pos2::new(list.left() + 16.0, list.top() + 30.0),
                Align2::LEFT_CENTER,
                empty_reason,
                FontId::proportional(11.0),
                p.text3,
            );
        }
        egui::ScrollArea::vertical()
            .id_salt(("cb-list", side_index))
            .auto_shrink([false, false])
            .show_rows(ui, ROW_H, row_total.max(1), |ui, range| {
                for row_index in range {
                    if row_index == visible.len() && footer_row {
                        let (row, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), ROW_H),
                            Sense::hover(),
                        );
                        // cb-load-more-wrap：居中按钮 + 截断提示。
                        let loading_more = model.ui.compare_branches.loading_more[side_index_us];
                        let more_label = if loading_more {
                            crate::tr!(model, "Loading…", "加载中…").to_string()
                        } else {
                            crate::tr!(model, "Load more", "加载更多").to_string()
                        };
                        let more_w = more_label.chars().count() as f32 * 6.2 + 24.0;
                        let note = if zh {
                            format!("仅显示前 {} 个提交", side.commits.len())
                        } else {
                            format!("Showing the first {} commits", side.commits.len())
                        };
                        let total_w = more_w + 8.0 + note.chars().count() as f32 * 5.6;
                        let mut bx = row.center().x - total_w / 2.0;
                        let more = Rect::from_min_size(
                            Pos2::new(bx, row.center().y - 11.0),
                            Vec2::new(more_w, 22.0),
                        );
                        bx += more_w + 8.0;
                        if !loading_more {
                            let resp = ui.interact(
                                more,
                                ui.scope_id().with(("cb-more", side_index)),
                                Sense::click(),
                            );
                            ui.painter().rect_stroke(
                                more,
                                egui::CornerRadius::same(4),
                                Stroke::new(1.0, p.border_soft),
                                StrokeKind::Inside,
                            );
                            if resp.hovered() {
                                ui.painter().rect_filled(
                                    more,
                                    egui::CornerRadius::same(4),
                                    p.hover,
                                );
                            }
                            ui.painter().text(
                                more.center(),
                                Align2::CENTER_CENTER,
                                more_label,
                                FontId::proportional(11.0),
                                if resp.hovered() {
                                    p.text_bright
                                } else {
                                    p.text2
                                },
                            );
                            if resp.clicked() {
                                let path = {
                                    let state = &mut model.ui.compare_branches;
                                    state.loading_more[side_index_us] = true;
                                    if side_index == 0 {
                                        state.left_path.clone()
                                    } else {
                                        state.right_path.clone()
                                    }
                                };
                                model.queue(Command::CompareBranchesMore {
                                    side: side_index,
                                    exclude: other_ref.to_string(),
                                    include: ref_name.to_string(),
                                    limit: side.commits.len() + 500,
                                    path,
                                });
                            }
                        } else {
                            ui.painter().text(
                                more.center(),
                                Align2::CENTER_CENTER,
                                more_label,
                                FontId::proportional(11.0),
                                p.text3,
                            );
                        }
                        ui.painter().text(
                            Pos2::new(bx, row.center().y),
                            Align2::LEFT_CENTER,
                            note,
                            FontId::proportional(10.0),
                            p.text3,
                        );
                        continue;
                    }
                    let Some(commit_index) = visible.get(row_index) else {
                        continue;
                    };
                    let commit = &side.commits[*commit_index];
                    let (row, response) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), ROW_H),
                        Sense::click(),
                    );
                    let is_selected = selected.contains(&commit.id);
                    if is_selected {
                        // .log-row.active → #26415f。
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::ZERO, p.selected_row);
                    } else if response.hovered() {
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::ZERO, p.hover);
                    }
                    ui.painter().hline(
                        row.x_range(),
                        row.bottom() - 0.5,
                        Stroke::new(1.0, p.hairline),
                    );

                    // DAG cell（26px，与行高等高）。
                    if show_graph {
                        let cell_rect = Rect::from_min_size(row.min, Vec2::new(36.0, ROW_H));
                        crate::panel::draw_dag_cell(
                            ui.painter(),
                            cell_rect,
                            dag.rows.get(&commit.id),
                            dag.breaks.get(&commit.id),
                            commit,
                        );
                    }

                    // 主题 + merge 图标 + 引用章。
                    let mut msg_x = row.left() + RULER_GAP + if show_graph { 30.0 } else { 0.0 };
                    if commit.parents.len() > 1 {
                        paint_icon(
                            ui.painter(),
                            Rect::from_center_size(
                                Pos2::new(msg_x + 6.0, row.center().y),
                                Vec2::splat(12.0),
                            ),
                            Icon::Merge,
                            p.text3,
                        );
                        msg_x += 16.0;
                    }
                    let content_right = (row.right() - AUTHOR_W - DATE_W - 12.0).max(msg_x + 60.0);
                    let refs_max_w = ((content_right - msg_x) / 3.0).clamp(24.0, 240.0);
                    let refs_cluster =
                        crate::log_refs::prepare(ui.painter(), &commit.refs, refs_max_w);
                    let refs_w = refs_cluster
                        .as_ref()
                        .map(|cluster| cluster.width + 6.0)
                        .unwrap_or(0.0);
                    let subject_right = (content_right - refs_w).max(msg_x + 60.0);
                    let subject_font = FontId::proportional(13.0);
                    let subject_galley = ui.painter().layout_no_wrap(
                        commit.message.clone(),
                        subject_font.clone(),
                        p.text,
                    );
                    let actual_subject_w = subject_galley.size().x.min(subject_right - msg_x);
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(msg_x, row.top()),
                            Pos2::new(msg_x + actual_subject_w, row.bottom()),
                        ),
                        &commit.message,
                        subject_font,
                        p.text,
                        Align2::LEFT_CENTER,
                    );
                    if let Some(cluster) = refs_cluster {
                        let badge_x = msg_x + actual_subject_w + 6.0;
                        let id = ui.make_persistent_id(("compare-log-refs", commit.id.as_str()));
                        crate::log_refs::paint(
                            ui,
                            model.settings.language,
                            &p,
                            row,
                            badge_x,
                            &cluster,
                            id,
                        );
                    }
                    // 作者（76px 右对齐）+ 日期（104px 右对齐）。
                    let author_rect = Rect::from_min_max(
                        Pos2::new(row.right() - AUTHOR_W - DATE_W, row.top()),
                        Pos2::new(row.right() - DATE_W, row.bottom()),
                    );
                    text_clipped(
                        ui.painter(),
                        author_rect,
                        &commit.author,
                        FontId::proportional(12.0),
                        p.text2,
                        Align2::RIGHT_CENTER,
                    );
                    let date_str = crate::panel::format_commit_datetime(commit.time);
                    ui.painter().text(
                        Pos2::new(row.right() - 10.0, row.center().y),
                        Align2::RIGHT_CENTER,
                        date_str,
                        FontId::proportional(12.0),
                        p.text3,
                    );

                    // 选择（单击/Shift 范围/⌘ 多选）+ 双击开整提交 diff。
                    if response.clicked() {
                        let cmd = ui.input(|i| i.modifiers.command || i.modifiers.ctrl);
                        let shift = ui.input(|i| i.modifiers.shift);
                        if shift && anchor.is_some() {
                            click = Some((commit.id.clone(), true, cmd));
                        } else {
                            click = Some((commit.id.clone(), false, cmd));
                        }
                    }
                    if response.double_clicked() {
                        double_click = Some(commit.id.clone());
                    }
                }
            });
    });

    // ---- 选择落地（对齐稳定版 handleCommitClick）----
    if let Some((hash, shift, cmd)) = click {
        let state = &mut model.ui.compare_branches;
        let view = if side_index == 0 {
            &mut state.left_view
        } else {
            &mut state.right_view
        };
        view.select_all = false;
        if shift && view.anchor.is_some() {
            let anchor_id = view.anchor.clone().unwrap_or_default();
            let pos_anchor = visible
                .iter()
                .position(|index| side.commits[*index].id == anchor_id);
            let pos_curr = visible
                .iter()
                .position(|index| side.commits[*index].id == hash);
            if let (Some(a), Some(c)) = (pos_anchor, pos_curr) {
                let (lo, hi) = if a < c { (a, c) } else { (c, a) };
                view.selected = visible[lo..=hi]
                    .iter()
                    .map(|index| side.commits[*index].id.clone())
                    .collect();
            } else {
                view.selected = vec![hash.clone()];
            }
            view.anchor = Some(hash);
        } else if cmd {
            if let Some(pos) = view.selected.iter().position(|id| id == &hash) {
                if view.selected.len() > 1 {
                    view.selected.remove(pos);
                }
            } else {
                view.selected.push(hash.clone());
            }
            view.anchor = Some(hash);
        } else {
            view.selected = vec![hash.clone()];
            view.anchor = Some(hash);
        }
        view.selected_file = None;
        view.detail = None;
    }
    if let Some(id) = double_click {
        model.queue(Command::OpenCommitDiff {
            commit: id,
            path: String::new(),
        });
    }

    // 详情文件加载触发（对齐稳定版：选择即加载）。
    {
        let state = &mut model.ui.compare_branches;
        let view = if side_index == 0 {
            &mut state.left_view
        } else {
            &mut state.right_view
        };
        if view.select_all {
            view.detail = None;
        } else {
            let key = if view.selected.len() == 1 {
                format!("c:{}", view.selected[0])
            } else if view.selected.len() >= 2 {
                let positions: Vec<usize> = view
                    .selected
                    .iter()
                    .filter_map(|id| side.commits.iter().position(|commit| &commit.id == id))
                    .collect();
                match (positions.iter().min(), positions.iter().max()) {
                    (Some(newest), Some(oldest)) => {
                        const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
                        let from = side.commits[*oldest]
                            .parents
                            .first()
                            .cloned()
                            .unwrap_or_else(|| EMPTY_TREE.to_string());
                        format!("r:{from}..{}", side.commits[*newest].id)
                    }
                    _ => String::new(),
                }
            } else {
                String::new()
            };
            if !key.is_empty() {
                // key 变化即重查（含加载中切换提交的场景）：旧请求的结果
                // 回填时按 key 匹配会被丢弃，不会污染新选择。
                let mismatch = match &view.detail {
                    Some(detail) => detail.key != key,
                    None => true,
                };
                if mismatch {
                    view.detail = Some(lazecode_core::CompareDetail {
                        key: key.clone(),
                        loading: true,
                        ..Default::default()
                    });
                    let (commit, from, to) = if let Some(commit) = key.strip_prefix("c:") {
                        (commit.to_string(), String::new(), String::new())
                    } else {
                        let range = key.trim_start_matches("r:").to_string();
                        let (from, to) = range.split_once("..").unwrap_or(("", ""));
                        (String::new(), from.to_string(), to.to_string())
                    };
                    model.queue(Command::CompareFiles {
                        key,
                        commit,
                        from,
                        to,
                    });
                }
            }
        }
    }

    // ---- 详情列 ----
    let (select_all, collapsed, selected_file) = {
        let state = &model.ui.compare_branches;
        let view = if side_index == 0 {
            &state.left_view
        } else {
            &state.right_view
        };
        (
            view.select_all,
            view.collapsed_dirs.clone(),
            view.selected_file.clone(),
        )
    };
    draw_details(
        ui,
        model,
        rt,
        theme,
        details,
        ref_name,
        other_ref,
        side,
        merge_base,
        side_index,
        &selected,
        select_all,
        &collapsed,
        &selected_file,
    );

    // 详情列宽度分隔条（8px strip，hover 时左缘 2px accent 线）。
    let drag = ui.interact(
        divider,
        ui.scope_id().with(("cb-details", side_index)),
        Sense::drag(),
    );
    let hovered = drag.hovered() || drag.dragged();
    if hovered {
        ui.painter().vline(
            divider.left() + 3.0,
            divider.y_range(),
            Stroke::new(2.0, p.accent),
        );
    }
    if drag.dragged() {
        model.settings.compare_details_width = (model.settings.compare_details_width
            - drag.drag_delta().x / body.width() * 100.0)
            .clamp(24.0, 62.0);
        rt.compare_details_drag = true;
    } else if rt.compare_details_drag {
        rt.compare_details_drag = false;
        model.save_settings();
    }

    // 路径过滤浮层（cb-path-form：置于 draw_pane 终点绘制，确保层级绝对置顶，绝不被提交列表与文件列表遮盖穿透）。
    let path_open = {
        let view = if side_index == 0 {
            &model.ui.compare_branches.left_view
        } else {
            &model.ui.compare_branches.right_view
        };
        view.path_edit_open
    };
    if path_open {
        let form_w = 260.0;
        let form = Rect::from_min_size(
            Pos2::new(
                paths_rect.left().min(toolbar.right() - form_w - 4.0),
                toolbar.bottom() + 2.0,
            ),
            Vec2::new(form_w, 82.0),
        );
        crate::overlays::menu_frame(ui, form, theme);
        rt.menu_hit_rects.push(form);
        let input = Rect::from_min_size(
            Pos2::new(form.left() + 8.0, form.top() + 8.0),
            Vec2::new(form_w - 16.0, 26.0),
        );
        ui.painter()
            .rect_filled(input, egui::CornerRadius::same(4), p.scrim_subtle);
        ui.painter().rect_stroke(
            input,
            egui::CornerRadius::same(4),
            Stroke::new(1.0, p.border_soft),
            StrokeKind::Inside,
        );
        let mut edit = {
            let view = if side_index == 0 {
                &model.ui.compare_branches.left_view
            } else {
                &model.ui.compare_branches.right_view
            };
            view.path_edit.clone()
        };
        let resp = ui.put(
            input.shrink2(Vec2::new(6.0, 2.0)),
            TextEdit::singleline(&mut edit)
                .frame(egui::Frame::NONE)
                .align(egui::Align2::LEFT_CENTER)
                .hint_text(crate::tr!(model, "path", "路径"))
                .font(FontId::proportional(12.5)),
        );
        if resp.changed() {
            let view = if side_index == 0 {
                &mut model.ui.compare_branches.left_view
            } else {
                &mut model.ui.compare_branches.right_view
            };
            view.path_edit = edit.clone();
        }
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let view = if side_index == 0 {
                &mut model.ui.compare_branches.left_view
            } else {
                &mut model.ui.compare_branches.right_view
            };
            view.path = view.path_edit.trim().to_string();
            view.path_edit_open = false;
            state_reload(model);
            return;
        }
        let apply = Rect::from_min_size(
            Pos2::new(form.right() - 128.0, form.bottom() - 32.0),
            Vec2::new(56.0, 24.0),
        );
        let clear = Rect::from_min_size(
            Pos2::new(form.right() - 64.0, form.bottom() - 32.0),
            Vec2::new(56.0, 24.0),
        );
        if secondary_button(ui, apply, crate::tr!(model, "Apply", "应用"), None, theme).clicked()
        {
            let view = if side_index == 0 {
                &mut model.ui.compare_branches.left_view
            } else {
                &mut model.ui.compare_branches.right_view
            };
            view.path = view.path_edit.trim().to_string();
            view.path_edit_open = false;
            state_reload(model);
            return;
        }
        if secondary_button(ui, clear, crate::tr!(model, "Clear", "清除"), None, theme).clicked()
        {
            let view = if side_index == 0 {
                &mut model.ui.compare_branches.left_view
            } else {
                &mut model.ui.compare_branches.right_view
            };
            view.path_edit.clear();
            view.path.clear();
            view.path_edit_open = false;
            state_reload(model);
            return;
        }
        let clicked_outside = ui.input(|i| {
            i.pointer.any_click()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|pos| !form.contains(pos) && !paths_rect.contains(pos))
        });
        if clicked_outside || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            let view = if side_index == 0 {
                &mut model.ui.compare_branches.left_view
            } else {
                &mut model.ui.compare_branches.right_view
            };
            view.path_edit_open = false;
        }
    }
}

/// 过滤条件变化后的整页重查（稳定版 onPathFilterChange 触发重载）。
fn state_reload(model: &mut AppModel) {
    let state = &mut model.ui.compare_branches;
    state.loading = true;
    let (l, r, lp, rp) = (
        state.left_ref.clone(),
        state.right_ref.clone(),
        state.left_path.clone(),
        state.right_path.clone(),
    );
    model.queue(Command::CompareBranches {
        left: l,
        right: r,
        left_path: lp,
        right_path: rp,
    });
}

/// h18 内嵌小开关（cb-toggle：.* / Cc，active accent-soft+蓝）。
fn cb_toggle(ui: &mut Ui, rect: Rect, key: (&'static str, u8), label: &str, active: bool) -> bool {
    let p = crate::theme::Palette::dark();
    let response = ui.interact(rect, ui.scope_id().with(key), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(3), p.hover);
    } else if active {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(3), p.accent_soft);
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::monospace(10.0),
        if active {
            p.blue
        } else if response.hovered() {
            p.text_bright
        } else {
            p.text3
        },
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
