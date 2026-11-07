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

// ================= 页面渲染（稳定版 CompareBranchesView 的移植） =================
//
// 页面设计对齐稳定版：编辑器区常驻标签页；上下两个 pane（各是一条独立
// 提交日志），中间 8px 拖拽分隔条（split 持久化到 settings.compare_split）；
// 每个 pane = banner（方向语义 + Swap）→ 过滤工具行 → [提交日志 | 详情列]，
// 详情列宽度两侧共用、可拖（settings.compare_details_width）。视觉使用新版
// 设计令牌（palette/widgets/i18n tr!）。

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::UiRuntime;
use crate::theme::Theme;
use crate::widgets::{icon_button, secondary_button, secondary_button_enabled, text_clipped};
use egui::{Align2, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2};
use lazecode_core::{AppModel, Command};

const SPLITTER_H: f32 = 8.0;
const BANNER_H: f32 = 24.0;
const TOOLBAR_H: f32 = 26.0;
const LIST_HEADER_H: f32 = 20.0;
const ROW_H: f32 = 24.0;
const FILE_ROW_H: f32 = 20.0;
const META_H: f32 = 18.0;

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
    ui.painter().rect_filled(rect, 0.0, p.editor);

    // ---- 会话同步：项目/方向/路径不一致（新会话）时重置并触发后台比较。
    // root 必须参与：不同项目可能有同名分支，refs 相同不等于同一会话。----
    let session_root = model
        .active_project()
        .map(|p| p.root.clone())
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

    // ---- 全局 loading / error+Retry（覆盖整页）----
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

    // 拖拽分隔条（稳定版 resizeSplit：调整两条提交日志的高度）。
    let drag = ui.interact(splitter, ui.scope_id().with(("cb-split",)), Sense::drag());
    let hovered = drag.hovered() || drag.dragged();
    ui.painter().rect_filled(
        splitter,
        0.0,
        if hovered { p.accent_soft } else { p.panel2 },
    );
    ui.painter().hline(
        splitter.x_range(),
        splitter.center().y,
        Stroke::new(1.0, p.border),
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
    let accent = if side_index == 0 { p.accent } else { p.rose };

    // ---- banner：方向语义（对齐稳定版 commitsOnlyIn）+ left 侧 Swap ----
    let banner = Rect::from_min_size(rect.min, Vec2::new(rect.width(), BANNER_H));
    ui.painter().rect_filled(banner, 0.0, p.panel2);
    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(banner.left() + 12.0, banner.center().y),
            Vec2::splat(12.0),
        ),
        Icon::Alert,
        p.amber,
    );
    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(banner.left() + 26.0, banner.top()),
            Pos2::new(banner.right() - 8.0, banner.bottom()),
        ),
        &if zh {
            format!("存在于 {ref_name} 但不存在于 {other_ref} 的提交")
        } else {
            format!("Commits that exist in {ref_name} but don't exist in {other_ref}")
        },
        FontId::proportional(10.5),
        p.text2,
        Align2::LEFT_CENTER,
    );
    if side_index == 0 {
        // 稳定版 Swap 只出现在 left（上）pane。
        let swap = Rect::from_min_size(
            Pos2::new(banner.right() - 96.0, banner.center().y - 9.0),
            Vec2::new(90.0, 18.0),
        );
        if secondary_button(ui, swap, crate::tr!(model, "⇄ Swap", "⇄ 交换"), None, theme).clicked()
        {
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

    // ---- 过滤工具行 ----
    let toolbar = Rect::from_min_size(
        Pos2::new(rect.left(), banner.bottom()),
        Vec2::new(rect.width(), TOOLBAR_H),
    );
    let mut need_reload = false;
    {
        let view = if side_index == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        let search_right = toolbar.right() - 8.0;
        let paths_w = 48.0;
        let ref_w = 52.0;
        let date_w = 46.0;
        let user_w = 46.0;
        let toggle_w = 24.0;
        let menus_w = user_w + 4.0 + date_w + 4.0 + ref_w + 4.0 + paths_w + 24.0 + 8.0;
        let search_rect = Rect::from_min_max(
            Pos2::new(toolbar.left() + 8.0, toolbar.top() + 3.0),
            Pos2::new(search_right - menus_w, toolbar.bottom() - 3.0),
        );
        let mut query = view.query.clone();
        let search_resp = ui.put(
            search_rect,
            TextEdit::singleline(&mut query)
                .frame(egui::Frame::NONE)
                .hint_text(crate::tr!(model, "Filter commits", "过滤提交"))
                .font(FontId::proportional(10.5)),
        );
        if search_resp.changed() {
            view.query = query;
        }
        let regex_rect = Rect::from_min_size(
            Pos2::new(search_rect.right() + 4.0, toolbar.top() + 3.0),
            Vec2::new(toggle_w, 20.0),
        );
        if secondary_button_enabled(ui, regex_rect, ".*", None, view.regex, theme).clicked() {
            view.regex = !view.regex;
        }
        let case_rect = Rect::from_min_size(
            Pos2::new(regex_rect.right() + 4.0, toolbar.top() + 3.0),
            Vec2::new(toggle_w, 20.0),
        );
        if secondary_button_enabled(ui, case_rect, "Cc", None, view.match_case, theme).clicked() {
            view.match_case = !view.match_case;
        }
        let user_rect = Rect::from_min_size(
            Pos2::new(case_rect.right() + 4.0, toolbar.top() + 3.0),
            Vec2::new(user_w, 20.0),
        );
        let user_active = view.author.is_some();
        if secondary_button_enabled(
            ui,
            user_rect,
            crate::tr!(model, "User", "作者"),
            None,
            user_active,
            theme,
        )
        .clicked()
        {
            rt.open_menu(
                crate::runtime::MenuKind::CompareUserFilter { side: side_index },
                user_rect,
                None,
            );
        }
        let date_rect = Rect::from_min_size(
            Pos2::new(user_rect.right() + 4.0, toolbar.top() + 3.0),
            Vec2::new(date_w, 20.0),
        );
        let date_active = view.date.is_some();
        if secondary_button_enabled(
            ui,
            date_rect,
            crate::tr!(model, "Date", "日期"),
            None,
            date_active,
            theme,
        )
        .clicked()
        {
            rt.open_menu(
                crate::runtime::MenuKind::CompareDateFilter { side: side_index },
                date_rect,
                None,
            );
        }
        let ref_rect = Rect::from_min_size(
            Pos2::new(date_rect.right() + 4.0, toolbar.top() + 3.0),
            Vec2::new(ref_w, 20.0),
        );
        let ref_active = view.selected_ref_filter.is_some();
        if secondary_button_enabled(
            ui,
            ref_rect,
            crate::tr!(model, "Branch", "分支"),
            None,
            ref_active,
            theme,
        )
        .clicked()
        {
            rt.open_menu(
                crate::runtime::MenuKind::CompareRefFilter { side: side_index },
                ref_rect,
                None,
            );
        }
        let paths_rect = Rect::from_min_size(
            Pos2::new(ref_rect.right() + 4.0, toolbar.top() + 3.0),
            Vec2::new(paths_w, 20.0),
        );
        let paths_active = !view.path.is_empty() || view.path_edit_open;
        if secondary_button_enabled(
            ui,
            paths_rect,
            crate::tr!(model, "Paths", "路径"),
            None,
            paths_active,
            theme,
        )
        .clicked()
        {
            view.path_edit_open = !view.path_edit_open;
            view.path_edit = view.path.clone();
        }
        if user_active || date_active || ref_active || !view.query.is_empty() {
            let clear_rect = Rect::from_min_size(
                Pos2::new(paths_rect.right() + 4.0, toolbar.top() + 3.0),
                Vec2::new(20.0, 20.0),
            );
            if icon_button(ui, clear_rect, Icon::X, false, theme).clicked() {
                view.author = None;
                view.date = None;
                view.selected_ref_filter = None;
                view.query.clear();
            }
        }
        // 路径过滤输入行（展开时占一行）。
        if view.path_edit_open {
            let path_row = Rect::from_min_size(
                Pos2::new(rect.left(), toolbar.bottom()),
                Vec2::new(rect.width(), TOOLBAR_H),
            );
            let input = Rect::from_min_max(
                Pos2::new(path_row.left() + 8.0, path_row.top() + 3.0),
                Pos2::new(path_row.right() - 116.0, path_row.bottom() - 3.0),
            );
            ui.painter()
                .rect_filled(input, egui::CornerRadius::same(4), p.panel);
            ui.painter().rect_stroke(
                input,
                egui::CornerRadius::same(4),
                Stroke::new(1.0, p.border),
                StrokeKind::Inside,
            );
            let mut edit = view.path_edit.clone();
            let resp = ui.put(
                input.shrink2(Vec2::new(6.0, 2.0)),
                TextEdit::singleline(&mut edit)
                    .frame(egui::Frame::NONE)
                    .hint_text(crate::tr!(model, "path", "路径"))
                    .font(FontId::monospace(10.5)),
            );
            if resp.changed() {
                view.path_edit = edit.clone();
            }
            let apply = Rect::from_min_size(
                Pos2::new(input.right() + 6.0, path_row.top() + 3.0),
                Vec2::new(50.0, 20.0),
            );
            let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
            let apply_clicked =
                secondary_button(ui, apply, crate::tr!(model, "Apply", "应用"), None, theme)
                    .clicked();
            let clear_clicked = secondary_button(
                ui,
                Rect::from_min_size(
                    Pos2::new(apply.right() + 4.0, path_row.top() + 3.0),
                    Vec2::new(50.0, 20.0),
                ),
                crate::tr!(model, "Clear", "清空"),
                None,
                theme,
            )
            .clicked();
            if apply_clicked || (enter && !edit.trim().is_empty()) {
                view.path_edit_open = false;
                if view.path_edit != view.path {
                    view.path = view.path_edit.clone();
                    need_reload = true;
                }
            }
            if clear_clicked {
                view.path_edit_open = false;
                if !view.path.is_empty() {
                    view.path.clear();
                    view.path_edit.clear();
                    need_reload = true;
                }
            }
        }
    }
    if need_reload {
        let state = &mut model.ui.compare_branches;
        state.sides = None;
        for view in [&mut state.left_view, &mut state.right_view] {
            view.selected.clear();
            view.anchor = None;
            view.select_all = false;
            view.selected_file = None;
            view.detail = None;
        }
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

    // 过滤后的可见提交（搜索/作者/日期/引用过滤，保持显示顺序）。
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

    let path_edit_open = {
        let view = if side_index == 0 {
            &model.ui.compare_branches.left_view
        } else {
            &model.ui.compare_branches.right_view
        };
        view.path_edit_open
    };
    let body_top = if path_edit_open {
        toolbar.bottom() + TOOLBAR_H
    } else {
        toolbar.bottom()
    };
    let body = Rect::from_min_max(Pos2::new(rect.left(), body_top), rect.max);

    // ---- [提交日志 | 详情列]（详情宽两侧共用、可拖，稳定版 24-62%）----
    let details_frac = (model.settings.compare_details_width / 100.0).clamp(0.24, 0.62);
    let details_w = (body.width() * details_frac).max(180.0);
    let list = Rect::from_min_max(body.min, Pos2::new(body.right() - details_w, body.bottom()));
    let details = Rect::from_min_max(Pos2::new(list.right() + 1.0, body.min.y), body.max);
    let divider_x = list.right();
    let divider = Rect::from_min_size(
        Pos2::new(divider_x - 3.0, body.min.y),
        Vec2::new(7.0, body.height()),
    );

    // 提交日志列。
    let list_header = Rect::from_min_size(list.min, Vec2::new(list.width(), LIST_HEADER_H));
    ui.painter().rect_filled(list_header, 0.0, p.panel2);
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
    ui.painter().text(
        Pos2::new(list_header.left() + 10.0, list_header.center().y),
        Align2::LEFT_CENTER,
        format!(
            "{} · {} {}",
            ref_name,
            visible.len(),
            if zh {
                "个特有提交"
            } else if visible.len() == 1 {
                "commit"
            } else {
                "commits"
            }
        ),
        FontId::proportional(10.0),
        accent,
    );
    let list_body = Rect::from_min_max(
        Pos2::new(list.left(), list_header.bottom()),
        Pos2::new(list.right(), list.bottom()),
    );
    let (selected, _anchor) = {
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
    crate::app_ui::region(ui, list_body, "cb-list", |ui| {
        egui::ScrollArea::vertical()
            .id_salt(("cb-list", side_index))
            .auto_shrink([false, false])
            .show_rows(ui, ROW_H, row_total.max(1), |ui, range| {
                if visible.is_empty() {
                    ui.painter().text(
                        Pos2::new(list_body.left() + 10.0, list_body.top() + 30.0),
                        Align2::LEFT_CENTER,
                        empty_reason,
                        FontId::proportional(10.0),
                        p.text3,
                    );
                }
                for row_index in range {
                    if row_index == visible.len() && footer_row {
                        let (row, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), ROW_H),
                            Sense::hover(),
                        );
                        if model.ui.compare_branches.loading_more[side_index as usize] {
                            ui.painter().text(
                                Pos2::new(row.left() + 8.0, row.center().y),
                                Align2::LEFT_CENTER,
                                crate::tr!(model, "Loading…", "加载中…"),
                                FontId::proportional(10.0),
                                p.text3,
                            );
                        } else {
                            let more = Rect::from_min_size(
                                Pos2::new(row.left() + 8.0, row.center().y - 10.0),
                                Vec2::new(90.0, 20.0),
                            );
                            if secondary_button(
                                ui,
                                more,
                                crate::tr!(model, "Load more", "加载更多"),
                                None,
                                theme,
                            )
                            .clicked()
                            {
                                let path = {
                                    let state = &mut model.ui.compare_branches;
                                    state.loading_more[side_index as usize] = true;
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
                            ui.painter().text(
                                Pos2::new(row.left() + 106.0, row.center().y),
                                Align2::LEFT_CENTER,
                                if zh {
                                    format!("仅显示前 {} 个提交", side.commits.len())
                                } else {
                                    format!("Showing the first {} commits", side.commits.len())
                                },
                                FontId::proportional(9.0),
                                p.text3,
                            );
                        }
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
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::same(3), p.accent_soft);
                    } else if response.hovered() {
                        ui.painter()
                            .rect_filled(row, egui::CornerRadius::same(3), p.hover);
                    }
                    ui.painter().text(
                        Pos2::new(row.left() + 8.0, row.center().y),
                        Align2::LEFT_CENTER,
                        &commit.short,
                        FontId::monospace(9.5),
                        if is_selected { p.accent } else { p.text3 },
                    );
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(row.left() + 52.0, row.top()),
                            Pos2::new(row.right() - 160.0, row.bottom()),
                        ),
                        &commit.message,
                        FontId::proportional(10.5),
                        p.text,
                        Align2::LEFT_CENTER,
                    );
                    text_clipped(
                        ui.painter(),
                        Rect::from_min_max(
                            Pos2::new(row.right() - 156.0, row.top()),
                            Pos2::new(row.right() - 64.0, row.bottom()),
                        ),
                        &commit.author,
                        FontId::proportional(9.5),
                        p.text3,
                        Align2::LEFT_CENTER,
                    );
                    ui.painter().text(
                        Pos2::new(row.right() - 8.0, row.center().y),
                        Align2::RIGHT_CENTER,
                        crate::panel::time_ago(commit.time, zh),
                        FontId::proportional(9.0),
                        p.text3,
                    );
                    if response.double_clicked() {
                        double_click = Some(commit.id.clone());
                    } else if response.clicked() {
                        let (cmd, shift) =
                            ui.input(|input| (input.modifiers.command, input.modifiers.shift));
                        click = Some((commit.id.clone(), cmd, shift));
                    }
                }
            });
    });

    // 选择语义（稳定版）：普通点击单选、Cmd 切换、Shift 自锚点范围。
    if let Some((id, cmd, shift)) = click {
        {
            let state = &mut model.ui.compare_branches;
            let view = if side_index == 0 {
                &mut state.left_view
            } else {
                &mut state.right_view
            };
            view.select_all = false;
            if cmd {
                if let Some(pos) = view.selected.iter().position(|probe| probe == &id) {
                    view.selected.remove(pos);
                } else {
                    view.selected.push(id.clone());
                }
                view.anchor = Some(id);
            } else if shift {
                let anchor_pos = view.anchor.as_ref().and_then(|anchor| {
                    visible
                        .iter()
                        .position(|index| &side.commits[*index].id == anchor)
                });
                let end = visible
                    .iter()
                    .position(|index| side.commits[*index].id == id);
                if let (Some(start), Some(end)) = (anchor_pos, end) {
                    let (lo, hi) = if start <= end {
                        (start, end)
                    } else {
                        (end, start)
                    };
                    view.selected = (lo..=hi)
                        .map(|pos| side.commits[visible[pos]].id.clone())
                        .collect();
                } else {
                    view.selected = vec![id.clone()];
                    view.anchor = Some(id);
                }
            } else {
                view.selected = vec![id.clone()];
                view.anchor = Some(id);
            }
            view.selected_file = None;
            view.detail = None;
        }
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
                // 回填时按 key 匹配会被丢弃，不会污染新选择。`&& !loading`
                // 会让「加载中换选」永远卡在加载态（旧结果被丢、新请求
                // 又不发），是本次审计修掉的竞态。
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

    // 详情列宽度分隔条（稳定版 resizeDetails：两侧共用一个宽度）。
    let drag = ui.interact(
        divider,
        ui.scope_id().with(("cb-details", side_index)),
        Sense::drag(),
    );
    let hovered = drag.hovered() || drag.dragged();
    if hovered {
        ui.painter().rect_filled(divider, 0.0, p.accent_soft);
    }
    ui.painter().vline(
        divider.center().x,
        divider.y_range(),
        Stroke::new(1.0, if hovered { p.accent } else { p.border }),
    );
    if drag.dragged() {
        model.settings.compare_details_width = (model.settings.compare_details_width
            - drag.drag_delta().x / body.width() * 100.0)
            .clamp(24.0, 62.0);
        rt.compare_details_drag = true;
    } else if rt.compare_details_drag {
        rt.compare_details_drag = false;
        model.save_settings();
    }
}

/// 详情列：标题行（计数 + Expand/Collapse All + Diff）→ 文件树 → 元信息。
#[allow(clippy::too_many_arguments)]
fn draw_details(
    ui: &mut Ui,
    model: &mut AppModel,
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
    ui.painter().rect_filled(rect, 0.0, p.panel);
    ui.painter()
        .hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, p.border));
    ui.painter()
        .vline(rect.left(), rect.y_range(), Stroke::new(1.0, p.border));

    let head = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 22.0));
    let view = if side_index == 0 {
        &model.ui.compare_branches.left_view
    } else {
        &model.ui.compare_branches.right_view
    };
    let detail = view.detail.clone();
    let title = if select_all {
        if zh {
            format!("已选择整侧 · {} 个文件", side.files.len())
        } else {
            format!("Whole side · {} files", side.files.len())
        }
    } else if selected.len() > 1 {
        if zh {
            format!("已选择 {} 个提交", selected.len())
        } else {
            format!("{} commits selected", selected.len())
        }
    } else if selected.len() == 1 {
        match &detail {
            Some(detail) if detail.loading => crate::tr!(model, "Loading…", "加载中…").to_string(),
            Some(detail) if detail.error.is_some() => {
                crate::tr!(model, "Load failed", "加载失败").to_string()
            }
            _ => crate::tr!(model, "Files changed", "变更文件").to_string(),
        }
    } else {
        crate::tr!(model, "No commit selected", "未选择提交").to_string()
    };
    ui.painter().text(
        Pos2::new(head.left() + 10.0, head.center().y),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(10.0),
        p.text2,
    );
    let expand_rect = Rect::from_min_size(
        Pos2::new(head.right() - 66.0, head.center().y - 9.0),
        Vec2::splat(18.0),
    );
    if icon_button(ui, expand_rect, Icon::ExpandAll, false, theme).clicked() {
        let view = if side_index == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.collapsed_dirs.clear();
    }
    let collapse_rect = Rect::from_min_size(
        Pos2::new(head.right() - 44.0, head.center().y - 9.0),
        Vec2::splat(18.0),
    );
    if icon_button(ui, collapse_rect, Icon::CollapseAll, false, theme).clicked() {
        let dirs: std::collections::HashSet<String> =
            flatten_file_tree(&side.files, &std::collections::HashSet::new())
                .into_iter()
                .filter(|row| row.is_dir)
                .map(|row| row.key)
                .collect();
        let view = if side_index == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.collapsed_dirs = dirs;
    }
    let diff_rect = Rect::from_min_size(
        Pos2::new(head.right() - 62.0, head.center().y - 9.0),
        Vec2::new(54.0, 18.0),
    );
    let diff_enabled = selected_file.is_some();
    if secondary_button_enabled(
        ui,
        diff_rect,
        crate::tr!(model, "Diff", "差异"),
        None,
        diff_enabled,
        theme,
    )
    .clicked()
        && diff_enabled
    {
        if let Some(path) = selected_file {
            open_file_diff(
                model, side_index, side, merge_base, select_all, selected, path, other_ref,
            );
        }
    }

    let rows_rect = Rect::from_min_max(
        Pos2::new(rect.left(), head.bottom()),
        Pos2::new(rect.right(), rect.bottom() - META_H),
    );
    // 文件来源：⌘A=整侧清单；选中=本侧详情缓存；**未选中=必空**（详情
    // 缓存是每侧独立的，未选择时不得回退到缓存——那会画出另一侧的文件）。
    let (files, loading, error): (Vec<lazecode_git::CompareFile>, bool, Option<String>) =
        if select_all {
            (side.files.clone(), false, None)
        } else if selected.is_empty() {
            (Vec::new(), false, None)
        } else {
            match &detail {
                Some(detail) => (detail.files.clone(), detail.loading, detail.error.clone()),
                None => (Vec::new(), false, None),
            }
        };
    if let Some(err) = error {
        ui.painter().text(
            Pos2::new(rows_rect.left() + 10.0, rows_rect.center().y),
            Align2::LEFT_CENTER,
            err,
            FontId::proportional(9.5),
            p.rose,
        );
        return;
    }
    if loading {
        ui.painter().text(
            Pos2::new(rows_rect.left() + 10.0, rows_rect.center().y),
            Align2::LEFT_CENTER,
            crate::tr!(model, "Loading…", "加载中…"),
            FontId::proportional(10.0),
            p.text3,
        );
        return;
    }
    if files.is_empty() {
        // 未选择提交时标题与页脚已表达状态，正文留白（避免三重重复文案）。
        if !select_all && selected.is_empty() {
            return;
        }
        ui.painter().text(
            Pos2::new(rows_rect.left() + 10.0, rows_rect.center().y),
            Align2::LEFT_CENTER,
            crate::tr!(model, "No changed files", "没有变更文件"),
            FontId::proportional(10.0),
            p.text3,
        );
        return;
    }
    let rows = flatten_file_tree(&files, collapsed);
    let mut file_clicked: Option<String> = None;
    let mut file_double_clicked: Option<String> = None;
    let mut dir_toggled: Option<String> = None;
    crate::app_ui::region(ui, rows_rect, "cb-files", |ui| {
        egui::ScrollArea::vertical()
            .id_salt(("cb-files", side_index))
            .auto_shrink([false, false])
            .show_rows(ui, FILE_ROW_H, rows.len(), |ui, range| {
                for row_index in range {
                    let Some(row) = rows.get(row_index) else {
                        continue;
                    };
                    let (rect, response) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), FILE_ROW_H),
                        Sense::click(),
                    );
                    let indent = 8.0 + row.depth as f32 * 14.0;
                    if row.is_dir {
                        if response.hovered() {
                            ui.painter()
                                .rect_filled(rect, egui::CornerRadius::same(3), p.hover);
                        }
                        let expanded = !collapsed.contains(&row.key);
                        paint_icon(
                            ui.painter(),
                            Rect::from_center_size(
                                Pos2::new(rect.left() + indent + 5.0, rect.center().y),
                                Vec2::splat(10.0),
                            ),
                            if expanded {
                                Icon::ChevronDown
                            } else {
                                Icon::ChevronRight
                            },
                            p.text3,
                        );
                        ui.painter().text(
                            Pos2::new(rect.left() + indent + 16.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            &row.label,
                            FontId::proportional(10.0),
                            p.text2,
                        );
                        ui.painter().text(
                            Pos2::new(rect.right() - 8.0, rect.center().y),
                            Align2::RIGHT_CENTER,
                            row.count.to_string(),
                            FontId::monospace(9.0),
                            p.text3,
                        );
                        if response.clicked() {
                            dir_toggled = Some(row.key.clone());
                        }
                    } else {
                        let is_sel = selected_file.as_deref() == Some(row.key.as_str());
                        if is_sel || response.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                egui::CornerRadius::same(3),
                                if is_sel { p.selection } else { p.hover },
                            );
                        }
                        let file = row.file.clone().expect("file row carries a file");
                        let status_color = match file.status {
                            'A' | '?' => p.green,
                            'D' => p.rose,
                            _ => p.blue,
                        };
                        ui.painter().text(
                            Pos2::new(rect.left() + indent + 8.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            file.status.to_string(),
                            FontId::monospace(9.5),
                            status_color,
                        );
                        text_clipped(
                            ui.painter(),
                            Rect::from_min_max(
                                Pos2::new(rect.left() + indent + 24.0, rect.top()),
                                Pos2::new(rect.right() - 90.0, rect.bottom()),
                            ),
                            &row.label,
                            FontId::proportional(10.0),
                            p.text,
                            Align2::LEFT_CENTER,
                        );
                        let stats = format!("+{} −{}", file.additions, file.deletions);
                        ui.painter().text(
                            Pos2::new(rect.right() - 8.0, rect.center().y),
                            Align2::RIGHT_CENTER,
                            stats,
                            FontId::monospace(9.0),
                            p.text3,
                        );
                        if response.double_clicked() {
                            file_double_clicked = Some(row.key.clone());
                        } else if response.clicked() {
                            file_clicked = Some(row.key.clone());
                        }
                    }
                }
            });
    });
    if let Some(dir) = dir_toggled {
        let view = if side_index == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        if !view.collapsed_dirs.remove(&dir) {
            view.collapsed_dirs.insert(dir);
        }
    }
    if let Some(path) = file_clicked {
        let view = if side_index == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.selected_file = Some(path);
    }
    if let Some(path) = file_double_clicked {
        open_file_diff(
            model, side_index, side, merge_base, select_all, selected, &path, other_ref,
        );
    }

    let footer = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - META_H), rect.max);
    let meta = if select_all {
        if zh {
            format!("整侧差异 {ref_name} ⟷ {other_ref}")
        } else {
            format!("Side diff {ref_name} ⟷ {other_ref}")
        }
    } else if selected.len() >= 2 {
        if zh {
            format!("范围差异 {} 个提交", selected.len())
        } else {
            format!("Range diff over {} commits", selected.len())
        }
    } else if selected.len() == 1 {
        let id = &selected[0];
        match side.commits.iter().find(|commit| &commit.id == id) {
            Some(commit) => {
                if zh {
                    format!(
                        "{} · {} 于 {}",
                        commit.author,
                        commit.email,
                        crate::panel::time_ago(commit.time, zh)
                    )
                } else {
                    format!(
                        "{} · {} on {}",
                        commit.author,
                        commit.email,
                        crate::panel::time_ago(commit.time, zh)
                    )
                }
            }
            None => String::new(),
        }
    } else {
        crate::tr!(model, "No commit selected", "未选择提交").to_string()
    };
    ui.painter().text(
        Pos2::new(footer.left() + 10.0, footer.center().y),
        Align2::LEFT_CENTER,
        meta,
        FontId::proportional(9.0),
        p.text3,
    );
}

/// 按稳定版规则打开文件 diff：
/// - ⌘A 整侧：merge-base..side（无 merge-base 退化对侧端点）；
/// - 多选：最老选中提交的父提交..最新选中提交（根提交用空树）；
/// - 单选：该提交自身。
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
