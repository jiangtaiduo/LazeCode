//! 分支比较对话框的纯函数辅助：提交过滤与文件树扁平化。
//! 移植自稳定版 `services/compareBranches.ts`（搜索/正则/大小写/作者/
//! 引用/日期过滤与目录树递归计数、稳定排序）。

mod details;
mod pane;
mod prelude;

use details::*;
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
use crate::widgets::secondary_button;
use egui::{Align2, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2};
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
