use std::path::PathBuf;

use egui::{
    Align2, Color32, Event, FontId, ImeEvent, Key, Modifiers, Pos2, Rect, Sense, Stroke,
    StrokeKind, TextEdit, Ui, Vec2,
};
use lazecode_core::{AppModel, Command, DiffSource, DiffTab, DiffViewMode, ProjectState, TabKind};
use lazecode_text::{Cursor, Document, MatchOptions, Motion, Selection, TokenKind};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::{DialogState, UiRuntime};
use crate::theme::{Theme, MONO};
use crate::widgets::{mono_char_width, secondary_button, text_clipped, text_width};

const TABBAR_H: f32 = 30.0;
const CRUMBS_H: f32 = 26.0;
const GUTTER: f32 = 52.0;
const SCROLLBAR_W: f32 = 9.0;

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
    if rect.height() < 35.0 || rect.width() < 10.0 {
        return;
    }
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.editor);
    let Some(index) = model.active_project else {
        draw_welcome(ui, model, rt, theme, rect, None);
        return;
    };
    let tab_count = model.projects[index].active_worktree_tabs().count();
    if tab_count == 0 {
        // The virtual Single files checkout may exist only to host a console
        // opened from the empty home screen. Keep the home screen intact.
        let welcome_index = if model.projects[index].is_single_file {
            None
        } else {
            Some(index)
        };
        draw_welcome(ui, model, rt, theme, rect, welcome_index);
        return;
    }

    let tabbar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), TABBAR_H));
    draw_tabbar(ui, model, rt, theme, tabbar, index);

    // Closing the last tab can end the checkout mid-frame:
    // settle_single_files_workspace drops an emptied Single files workspace or
    // switches to another project, so the index captured above may now point
    // past the vec. Re-resolve the target instead of using the stale index.
    let Some(index) = model.active_project else {
        draw_welcome(ui, model, rt, theme, rect, None);
        return;
    };
    let active = model.projects[index].active_tab;
    let Some(active) = active else {
        draw_welcome(ui, model, rt, theme, rect, Some(index));
        return;
    };
    let content = Rect::from_min_max(Pos2::new(rect.left(), tabbar.bottom()), rect.max);
    enum ActiveKind {
        Compare(String, String),
        File,
        Image(lazecode_core::ImageTab),
        Diff,
    }
    let kind = model.projects[index]
        .tab_by_id(active)
        .map(|tab| match &tab.kind {
            lazecode_core::TabKind::Compare(c) => {
                ActiveKind::Compare(c.left.clone(), c.right.clone())
            }
            lazecode_core::TabKind::File(_) => ActiveKind::File,
            lazecode_core::TabKind::Image(img) => ActiveKind::Image(img.clone()),
            lazecode_core::TabKind::Diff(_) => ActiveKind::Diff,
        });
    if let Some(kind) = kind {
        match kind {
            ActiveKind::Compare(left, right) => {
                crate::compare::draw_page(ui, model, rt, theme, content, &left, &right);
            }
            ActiveKind::File => {
                let is_md =
                    model.projects[index]
                        .tab_by_id(active)
                        .is_some_and(|tab| match &tab.kind {
                            TabKind::File(file) => file
                                .doc
                                .path
                                .extension()
                                .is_some_and(|ext| ext == "md" || ext == "markdown"),
                            _ => false,
                        });
                if is_md {
                    let toolbar_rect =
                        Rect::from_min_size(content.min, Vec2::new(content.width(), 32.0));
                    draw_markdown_toolbar(ui, model, theme, toolbar_rect, index, active);
                    let body = Rect::from_min_max(
                        Pos2::new(content.left(), toolbar_rect.bottom()),
                        content.max,
                    );
                    draw_file(ui, model, rt, theme, body, index, active);
                } else {
                    let crumbs =
                        Rect::from_min_size(content.min, Vec2::new(content.width(), CRUMBS_H));
                    draw_crumbs(ui, model, theme, crumbs, index, active);
                    let body =
                        Rect::from_min_max(Pos2::new(content.left(), crumbs.bottom()), content.max);
                    draw_file(ui, model, rt, theme, body, index, active);
                }
            }
            ActiveKind::Image(img) => {
                let crumbs = Rect::from_min_size(content.min, Vec2::new(content.width(), CRUMBS_H));
                draw_crumbs(ui, model, theme, crumbs, index, active);
                let body =
                    Rect::from_min_max(Pos2::new(content.left(), crumbs.bottom()), content.max);
                draw_image_tab(ui, model, rt, p, theme, body, &img, active);
            }
            ActiveKind::Diff => {
                draw_diff(ui, model, rt, theme, content, index, active);
            }
        }
    }
}

/// Run the built-in JSON formatter on one file tab. This does not call an LSP;
/// JSON stays usable when no language server is configured.
pub(crate) fn format_json_tab(
    model: &mut AppModel,
    project_index: usize,
    tab_id: u64,
    compact: bool,
) {
    let zh = model.settings.language.is_zh();
    let is_json = model
        .projects
        .get(project_index)
        .and_then(|project| project.tab_by_id(tab_id))
        .is_some_and(|tab| match &tab.kind {
            TabKind::File(file) => lazecode_core::is_json_path(&file.doc.path),
            _ => false,
        });
    if !is_json {
        model.toast(
            if zh {
                "当前文件不是 JSON"
            } else {
                "The active file is not JSON"
            },
            lazecode_core::ToastKind::Warning,
        );
        return;
    }
    let result = model
        .projects
        .get_mut(project_index)
        .and_then(|project| project.tab_mut_by_id(tab_id))
        .and_then(|tab| match &mut tab.kind {
            TabKind::File(file) => Some(file.doc.format_json(compact)),
            _ => None,
        });
    if let Some(Err(err)) = result {
        model.toast(
            if zh {
                format!("JSON 格式错误：{err}")
            } else {
                format!("Invalid JSON: {err}")
            },
            lazecode_core::ToastKind::Error,
        );
    }
}

/// 打开（或激活）分支比较页，标题按当前语言生成。
pub fn open_compare_page(model: &mut AppModel, left: String, right: String) {
    let title = if model.settings.language.is_zh() {
        format!("比较：{left} 和 {right}")
    } else {
        format!("Compare: {left} and {right}")
    };
    if let Some(project) = model.active_project_mut() {
        project.open_compare_branches(left, right, title);
    }
}

fn tab_icon(path: &str, theme: &Theme) -> (Icon, Color32) {
    let p = theme.palette;
    let color = match path.rsplit('.').next().unwrap_or_default() {
        "rs" => p.orange,
        "ts" | "tsx" => p.blue,
        "js" | "jsx" => p.amber,
        "json" => p.amber,
        "css" | "scss" => p.violet,
        "html" => p.orange,
        "md" => p.text2,
        "py" => p.blue,
        "go" => p.green,
        "sh" | "zsh" => p.green,
        _ => p.text3,
    };
    (Icon::File, color)
}

fn draw_tabbar(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    index: usize,
) {
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.tabstrip);
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    let project = &model.projects[index];
    /// (tab id, title, path, dirty, badge label)
    type TabBadge = (u64, String, Option<String>, bool, Option<&'static str>);
    let tabs: Vec<TabBadge> = project
        .active_worktree_tabs()
        .map(|tab| {
            let path = tab.path().map(|path| path.to_string());
            let badge = match &tab.kind {
                TabKind::File(file) if file.external => Some("diff"),
                TabKind::Diff(_) => Some("diff"),
                TabKind::Compare(_) => Some("cmp"),
                TabKind::Image(_) => None,
                TabKind::File(_) => None,
            };
            (tab.id, tab.title(), path, tab.is_dirty(), badge)
        })
        .collect();
    let active = project.active_tab;
    let mut pending: Option<Command> = None;
    let mut close: Option<u64> = None;
    super::app_ui::region(ui, rect, "editor-tabbar-scroll-region", |ui| {
        egui::ScrollArea::horizontal()
            .id_salt(("editor-tabs-scroll", index))
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                let total_width: f32 = tabs
                    .iter()
                    .map(|(_, title, _, _, badge)| {
                        let extra = if badge.is_some() { 36.0 } else { 0.0 };
                        (title.chars().count() as f32 * 6.8 + 62.0 + extra).clamp(96.0, 240.0) + 1.0
                    })
                    .sum();
                let bar_width = total_width.max(rect.width());
                let (canvas_rect, _) =
                    ui.allocate_exact_size(Vec2::new(bar_width, rect.height()), Sense::hover());
                let mut x = canvas_rect.left();
                for (id, title, path, dirty, badge) in tabs {
                    let extra = if badge.is_some() { 36.0 } else { 0.0 };
                    let width =
                        (title.chars().count() as f32 * 6.8 + 62.0 + extra).clamp(96.0, 240.0);
                    let tab_rect = Rect::from_min_size(
                        Pos2::new(x, canvas_rect.top()),
                        Vec2::new(width, canvas_rect.height()),
                    );
                    // Only reveal an active tab that is actually clipped.
                    // Calling scroll_to_rect every frame keeps a scroll target
                    // alive forever (especially when the last tab cannot be
                    // centered), which used to repaint the whole window at the
                    // compositor rate.
                    if active == Some(id) && !ui.clip_rect().contains_rect(tab_rect) {
                        ui.scroll_to_rect(tab_rect, Some(egui::Align::Center));
                    }
                    let is_active = active == Some(id);
                    let body = Rect::from_min_max(
                        tab_rect.min,
                        Pos2::new(tab_rect.right() - 24.0, tab_rect.bottom()),
                    );
                    let close_rect = Rect::from_min_max(
                        Pos2::new(tab_rect.right() - 24.0, tab_rect.top()),
                        tab_rect.max,
                    );
                    let response =
                        ui.interact(body, ui.scope_id().with(("tab", id)), Sense::click());
                    let close_response = ui.interact(
                        close_rect,
                        ui.scope_id().with(("tab-close", id)),
                        Sense::click(),
                    );
                    if is_active {
                        ui.painter()
                            .rect_filled(tab_rect, egui::CornerRadius::ZERO, p.tabstrip);
                        ui.painter()
                            .rect_filled(tab_rect, egui::CornerRadius::ZERO, p.editor);
                        // Current file status line at the bottom of the tab
                        ui.painter().rect_filled(
                            Rect::from_min_size(
                                Pos2::new(tab_rect.left(), tab_rect.bottom() - 2.0),
                                Vec2::new(tab_rect.width(), 2.0),
                            ),
                            egui::CornerRadius::ZERO,
                            p.accent,
                        );
                    } else if response.hovered() || close_response.hovered() {
                        ui.painter()
                            .rect_filled(tab_rect, egui::CornerRadius::ZERO, p.hover);
                    } else {
                        ui.painter().rect_filled(
                            tab_rect,
                            egui::CornerRadius::ZERO,
                            p.tabstrip.gamma_multiply(0.98),
                        );
                    }
                    let (icon, tone) = tab_icon(path.as_deref().unwrap_or_default(), theme);
                    paint_icon(
                        ui.painter(),
                        Rect::from_center_size(
                            Pos2::new(tab_rect.left() + 14.0, tab_rect.center().y),
                            Vec2::splat(13.0),
                        ),
                        icon,
                        if is_active {
                            tone
                        } else {
                            tone.gamma_multiply(0.8)
                        },
                    );
                    let text_right = if badge.is_some() {
                        close_rect.left() - 38.0
                    } else if dirty {
                        close_rect.left() - 18.0
                    } else {
                        close_rect.left() - 4.0
                    };
                    let text_rect = Rect::from_min_max(
                        Pos2::new(tab_rect.left() + 24.0, tab_rect.top()),
                        Pos2::new(text_right, tab_rect.bottom()),
                    );
                    text_clipped(
                        ui.painter(),
                        text_rect,
                        &title,
                        FontId::proportional(13.0),
                        if is_active { p.text } else { p.text2 },
                        Align2::LEFT_CENTER,
                    );
                    if let Some(badge) = badge {
                        let badge_rect = Rect::from_center_size(
                            Pos2::new(close_rect.left() - 20.0, tab_rect.center().y),
                            Vec2::new(30.0, 14.0),
                        );
                        ui.painter().rect_filled(
                            badge_rect,
                            egui::CornerRadius::same(3),
                            p.accent_soft,
                        );
                        ui.painter().text(
                            badge_rect.center(),
                            Align2::CENTER_CENTER,
                            badge,
                            FontId::monospace(10.0),
                            p.accent,
                        );
                    } else if dirty {
                        ui.painter().circle_filled(
                            Pos2::new(close_rect.left() - 10.0, tab_rect.center().y),
                            2.6,
                            p.accent,
                        );
                    }
                    if close_response.hovered() || (is_active && !dirty) {
                        paint_icon(
                            ui.painter(),
                            Rect::from_center_size(close_rect.center(), Vec2::splat(12.0)),
                            Icon::X,
                            if close_response.hovered() {
                                p.text
                            } else {
                                p.text3
                            },
                        );
                    }
                    if close_response.clicked() || response.middle_clicked() {
                        close = Some(id);
                    } else if response.clicked() {
                        model.projects[index].activate_tab(id);
                        rt.set_editor_focus(id);
                        if model.ui.auto_locate {
                            model.locate_active_file(true);
                        }
                    } else if response.secondary_clicked() {
                        rt.open_menu(
                            crate::runtime::MenuKind::EditorTabContext {
                                project: index,
                                tab_id: id,
                            },
                            tab_rect,
                            path.clone(),
                        );
                    }
                    x = tab_rect.right() + 1.0;
                }
                // Keep the rest of the bar draggable / empty.
                let rest = Rect::from_min_max(Pos2::new(x, canvas_rect.top()), canvas_rect.max);
                if rest.width() > 4.0 {
                    let drag = ui.interact(
                        rest,
                        ui.scope_id().with("tabbar-drag"),
                        Sense::click_and_drag(),
                    );
                    if drag.dragged() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                }
            });
    });
    if let Some(id) = close {
        if let Some(tab) = model.projects[index].tab_by_id(id) {
            if tab.is_dirty() {
                rt.dialog = crate::runtime::DialogState::CloseDirtyTab {
                    tab_id: id,
                    name: tab.title(),
                };
            } else {
                let was_compare = matches!(tab.kind, lazecode_core::TabKind::Compare(_));
                model.close_tab(index, id);
                if was_compare {
                    // 关闭比较页即结束会话（稳定版关闭标签页同理）。
                    model.ui.compare_branches = lazecode_core::CompareBranchesState::default();
                }
                if rt.editor_focus == Some(id) {
                    rt.editor_focus = None;
                }
            }
        }
    }
    if let Some(command) = pending.take() {
        model.queue(command);
    }
}

fn draw_crumbs(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    index: usize,
    tab_id: u64,
) {
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.editor);
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    let project_root = model.projects[index].active_root().to_path_buf();
    let (is_annotated, _cur_mode, path, file_path) = {
        let Some(tab) = model.projects[index].tab_by_id(tab_id) else {
            return;
        };
        let p_str = tab.path().unwrap_or_default().to_string();
        let (ann, mode, fp) = match &tab.kind {
            TabKind::File(f) => (f.annotate, f.md_mode, f.doc.path.clone()),
            TabKind::Image(img) => (
                false,
                lazecode_core::MarkdownViewMode::Code,
                img.path.clone(),
            ),
            _ => (false, lazecode_core::MarkdownViewMode::Code, PathBuf::new()),
        };
        (ann, mode, p_str, fp)
    };
    let root = model.projects[index].active_root().to_path_buf();
    let relative = std::path::Path::new(&path)
        .strip_prefix(&root)
        .map(|rel| rel.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.clone());
    let parts: Vec<&str> = relative
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let mut x = rect.left() + 14.0;
    for (part_index, part) in parts.iter().enumerate() {
        let last = part_index + 1 == parts.len();
        let width = part.chars().count() as f32 * 6.5 + 4.0;
        if x + width > rect.right() - 112.0 {
            break;
        }
        ui.painter().text(
            Pos2::new(x, rect.center().y),
            Align2::LEFT_CENTER,
            *part,
            FontId::proportional(12.0),
            if last { p.text2 } else { p.text3 },
        );
        x += width;
        if !last {
            paint_icon(
                ui.painter(),
                Rect::from_center_size(Pos2::new(x + 4.0, rect.center().y), Vec2::splat(9.0)),
                Icon::ChevronRight,
                p.text3,
            );
            x += 12.0;
        }
    }
    let mut btn_x = rect.right() - 36.0;

    // Blame toggle button
    btn_x -= 76.0;
    let blame_rect = Rect::from_min_size(
        Pos2::new(btn_x, rect.center().y - 10.0),
        Vec2::new(72.0, 20.0),
    );
    let b_label = if is_annotated {
        "✓ Blame"
    } else {
        "✍ Blame"
    };
    let mut toggle_blame = false;
    if secondary_button(ui, blame_rect, b_label, None, theme).clicked() {
        toggle_blame = true;
    }

    let zh = model.settings.language.is_zh();
    let mut blame_notice = None;
    if toggle_blame {
        if let Some(tab) = model.projects[index].tab_mut_by_id(tab_id) {
            if let TabKind::File(file) = &mut tab.kind {
                file.annotate = !file.annotate;
                if file.annotate && file.blame.is_none() {
                    let rel = file_path
                        .strip_prefix(&project_root)
                        .unwrap_or(&file_path)
                        .to_string_lossy()
                        .to_string();
                    match lazecode_git::blame(&project_root, &rel) {
                        Ok(lines) if lines.is_empty() => {
                            blame_notice = Some(if zh {
                                "该文件没有可用的 Blame 数据".to_string()
                            } else {
                                "No blame data for this file".to_string()
                            });
                        }
                        Ok(lines) => file.blame = Some(lines),
                        Err(err) => {
                            blame_notice = Some(format!(
                                "{}: {err}",
                                if zh { "Blame 失败" } else { "Blame failed" }
                            ));
                        }
                    }
                }
            }
        }
    }
    if let Some(message) = blame_notice {
        model.toast(message, lazecode_core::ToastKind::Warning);
    }

    let toggle_rect = Rect::from_center_size(
        Pos2::new(rect.right() - 18.0, rect.center().y),
        Vec2::splat(20.0),
    );
    let response = ui.interact(
        toggle_rect,
        ui.scope_id().with("minimap-toggle"),
        Sense::click(),
    );
    if response.hovered() {
        ui.painter()
            .rect_filled(toggle_rect, egui::CornerRadius::same(4), p.hover);
    }
    paint_icon(
        ui.painter(),
        toggle_rect.shrink(5.0),
        Icon::Layers,
        if model.ui.show_minimap {
            p.text2
        } else {
            p.text3
        },
    );
    if response.clicked() {
        model.ui.show_minimap = !model.ui.show_minimap;
        model.settings.editor.show_minimap = model.ui.show_minimap;
        model.save_settings();
    }
    response.on_hover_text("Toggle minimap");
}

fn token_color(kind: TokenKind, theme: &Theme) -> Color32 {
    let p = theme.palette;
    match kind {
        TokenKind::Text => p.text,
        TokenKind::Keyword => p.tok_kw,
        TokenKind::Type => p.tok_type,
        TokenKind::String => p.tok_str,
        TokenKind::Number => p.tok_num,
        TokenKind::Function => p.tok_fn,
        TokenKind::Comment => p.tok_com,
        TokenKind::Punctuation => p.tok_punc,
        TokenKind::Literal => p.tok_lit,
        TokenKind::Variable => p.tok_var,
        TokenKind::Property => p.tok_prop,
        TokenKind::Attribute => p.tok_attr,
    }
}
fn draw_markdown_toolbar(
    ui: &mut Ui,
    model: &mut AppModel,
    theme: &Theme,
    rect: Rect,
    index: usize,
    tab_id: u64,
) {
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.panel);
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );

    // Left title: "Markdown"
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, rect.center().y),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Markdown", "Markdown"),
        FontId::proportional(12.0),
        p.text2,
    );

    let cur_mode = model.projects[index]
        .tab_by_id(tab_id)
        .and_then(|tab| match &tab.kind {
            TabKind::File(file) => Some(file.md_mode),
            _ => None,
        })
        .unwrap_or(lazecode_core::MarkdownViewMode::Preview);

    let modes = [
        (
            lazecode_core::MarkdownViewMode::Code,
            crate::tr!(model, "Source", "源码"),
        ),
        (
            lazecode_core::MarkdownViewMode::Preview,
            crate::tr!(model, "Preview", "预览"),
        ),
        (
            lazecode_core::MarkdownViewMode::Split,
            crate::tr!(model, "Split", "分栏"),
        ),
    ];

    let pill_w = 186.0;
    let pill_h = 24.0;
    let pill_rect = Rect::from_min_size(
        Pos2::new(rect.right() - pill_w - 12.0, rect.center().y - pill_h * 0.5),
        Vec2::new(pill_w, pill_h),
    );
    ui.painter()
        .rect_filled(pill_rect, egui::CornerRadius::same(4), p.panel);
    ui.painter().rect_stroke(
        pill_rect,
        egui::CornerRadius::same(4),
        Stroke::new(1.0, p.border_soft),
        StrokeKind::Inside,
    );

    let btn_w = (pill_w - 4.0) / 3.0;
    let mut change_mode = None;
    for (i, (m, label)) in modes.into_iter().enumerate() {
        let btn_rect = Rect::from_min_size(
            Pos2::new(
                pill_rect.left() + 2.0 + i as f32 * btn_w,
                pill_rect.top() + 2.0,
            ),
            Vec2::new(btn_w, pill_h - 4.0),
        );
        let is_active = cur_mode == m;
        let resp = ui.interact(btn_rect, ui.scope_id().with(("md-mode", i)), Sense::click());
        if is_active {
            ui.painter()
                .rect_filled(btn_rect, egui::CornerRadius::same(3), p.active);
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(btn_rect, egui::CornerRadius::same(3), p.hover);
        }
        ui.painter().text(
            btn_rect.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(11.5),
            if is_active { p.text_bright } else { p.text2 },
        );
        if resp.clicked() {
            change_mode = Some(m);
        }
    }

    if let Some(m) = change_mode {
        if let Some(tab) = model.projects[index].tab_mut_by_id(tab_id) {
            if let TabKind::File(file) = &mut tab.kind {
                file.md_mode = m;
            }
        }
    }
}

fn draw_markdown_view(
    ui: &mut Ui,
    rt: &mut UiRuntime,
    p: crate::theme::Palette,
    theme: &Theme,
    rect: Rect,
    text: &str,
    tab_id: u64,
) {
    ui.painter().rect_filled(rect, 0.0, p.editor);
    super::app_ui::region(ui, rect, "md-view", |ui| {
        let stored = rt.md_scroll.get(&tab_id).copied().unwrap_or(0.0);
        let output = egui::ScrollArea::vertical()
            .id_salt(("md-preview-scroll", tab_id))
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .vertical_scroll_offset(stored)
            .show(ui, |ui| {
                // Reserve the custom scrollbar gutter so text never runs
                // underneath the handle in narrow/split layouts.
                let avail_w = (ui.available_width() - SCROLLBAR_W).max(160.0);
                let doc_w = avail_w.min(960.0);
                let min_pad = if avail_w < 720.0 { 20.0 } else { 38.0 };
                let side_pad = ((avail_w - doc_w) * 0.5).max(min_pad);
                let content_w = (avail_w - side_pad * 2.0).max(160.0);
                ui.horizontal(|ui| {
                    ui.add_space(side_pad);
                    ui.vertical(|ui| {
                        ui.set_max_width(content_w);
                        ui.add_space(26.0);
                        ui.scope(|ui| {
                            // Mature Markdown previews use a larger reading
                            // scale than the surrounding 13px UI chrome.
                            let styles = &mut ui.style_mut().text_styles;
                            styles.insert(egui::TextStyle::Body, FontId::proportional(14.5));
                            styles.insert(egui::TextStyle::Heading, FontId::proportional(27.0));
                            styles.insert(egui::TextStyle::Monospace, FontId::monospace(13.0));
                            ui.spacing_mut().item_spacing.y = 7.0;
                            let source_id = ui.make_persistent_id(("md-preview", tab_id));
                            egui_commonmark::CommonMarkViewer::new()
                                .indentation_spaces(2)
                                .max_image_width(Some(content_w as usize))
                                .default_width(Some(content_w as usize))
                                .enable_scroll_to_heading(true)
                                .show_with_id(source_id, ui, &mut rt.md_cache, text);
                        });
                        ui.add_space(72.0);
                    });
                    ui.add_space(side_pad);
                });
            });
        let current = output.state.clamped_offset().y;
        rt.md_scroll.insert(tab_id, current);
        let track = Rect::from_min_max(Pos2::new(rect.right() - SCROLLBAR_W, rect.top()), rect.max);
        let mut offset = current;
        if crate::widgets::scrollbar(
            ui,
            track,
            output.content_size.y,
            output.inner_rect.height(),
            &mut offset,
            theme,
        ) {
            rt.md_scroll.insert(tab_id, offset);
            ui.ctx().request_repaint();
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn draw_image_tab(
    ui: &mut Ui,
    model: &AppModel,
    rt: &mut UiRuntime,
    p: crate::theme::Palette,
    _theme: &Theme,
    rect: Rect,
    img: &lazecode_core::ImageTab,
    tab_id: u64,
) {
    ui.painter().rect_filled(rect, 0.0, p.editor);

    let file_size_str = std::fs::metadata(&img.path)
        .map(|m| {
            let bytes = m.len();
            if bytes >= 1024 * 1024 {
                format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
            } else if bytes >= 1024 {
                format!("{:.1} KB", bytes as f64 / 1024.0)
            } else {
                format!("{} B", bytes)
            }
        })
        .unwrap_or_default();

    let bottom_h = 26.0;
    let canvas_rect =
        Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.bottom() - bottom_h));
    let bar_rect = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - bottom_h), rect.max);
    let max_side = ((canvas_rect.width().max(canvas_rect.height()) * ui.ctx().pixels_per_point())
        .ceil() as usize)
        .clamp(256, 4096);

    let is_gif = img
        .path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("gif"));
    let (dims_str, texture, uri, image_error, loading) = if is_gif {
        // The GIF loader animates frames; its cache is flushed whenever the set
        // of GIF tabs changes.
        let dims = image::image_dimensions(&img.path)
            .ok()
            .map(|(w, h)| format!("{} × {} px", w, h))
            .unwrap_or_default();
        (
            dims,
            None,
            Some(format!("file://{}", img.path.display())),
            None,
            false,
        )
    } else {
        let loaded = rt
            .image_cache
            .get_or_load(ui.ctx(), &img.path, max_side, rt.frame_count);
        match loaded {
            crate::image_cache::ImageLoad::Ready(cached) => {
                let dims = cached.original_size;
                let tex_size = cached.texture.size_vec2();
                (
                    format!("{} × {} px", dims[0], dims[1]),
                    Some((cached.texture.id(), tex_size)),
                    None,
                    None,
                    false,
                )
            }
            crate::image_cache::ImageLoad::Loading => {
                let dims = image::image_dimensions(&img.path)
                    .ok()
                    .map(|(w, h)| format!("{} × {} px", w, h))
                    .unwrap_or_default();
                (dims, None, None, None, true)
            }
            crate::image_cache::ImageLoad::Failed(err) => {
                let dims = image::image_dimensions(&img.path)
                    .ok()
                    .map(|(w, h)| format!("{} × {} px", w, h))
                    .unwrap_or_default();
                (dims, None, None, Some(err), false)
            }
        }
    };

    ui.painter().hline(
        bar_rect.x_range(),
        bar_rect.top() + 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    ui.painter().rect_filled(bar_rect, 0.0, p.panel);

    let info_text = if !dims_str.is_empty() && !file_size_str.is_empty() {
        format!("{}  ·  {}", dims_str, file_size_str)
    } else if !dims_str.is_empty() {
        dims_str
    } else {
        file_size_str
    };

    ui.painter().text(
        Pos2::new(bar_rect.left() + 14.0, bar_rect.center().y),
        Align2::LEFT_CENTER,
        &info_text,
        FontId::proportional(11.5),
        p.text3,
    );

    super::app_ui::region(ui, canvas_rect, "image-viewer", |ui| {
        egui::ScrollArea::both()
            .id_salt(ui.scope_id().with(("img-scroll", tab_id)))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.centered_and_justified(|ui| {
                    let max_size = (canvas_rect.size() - Vec2::splat(32.0)).max(Vec2::splat(10.0));
                    if let Some((texture_id, tex_size)) = texture {
                        let ppp = ui.ctx().pixels_per_point().max(0.5);
                        let natural = tex_size / ppp;
                        let scale = (max_size.x / natural.x.max(1.0))
                            .min(max_size.y / natural.y.max(1.0))
                            .min(1.0);
                        let display = (natural * scale).max(Vec2::splat(1.0));
                        ui.add(
                            egui::Image::new(egui::load::SizedTexture::new(texture_id, display))
                                .maintain_aspect_ratio(true),
                        );
                    } else if let Some(uri) = uri.as_deref() {
                        ui.add(
                            egui::Image::new(uri)
                                .max_size(max_size)
                                .maintain_aspect_ratio(true),
                        );
                    } else if let Some(err) = image_error.as_deref() {
                        ui.label(
                            egui::RichText::new(format!(
                                "{}: {err}",
                                crate::tr!(model, "Could not load image", "无法加载图片")
                            ))
                            .color(p.rose)
                            .font(FontId::proportional(12.5)),
                        );
                    } else if loading {
                        ui.label(
                            egui::RichText::new(crate::tr!(
                                model,
                                "Loading image…",
                                "正在加载图片…"
                            ))
                            .color(p.text3)
                            .font(FontId::proportional(12.5)),
                        );
                    }
                });
            });
    });
}

#[allow(clippy::too_many_arguments)]
fn draw_file(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    index: usize,
    tab_id: u64,
) {
    let p = theme.palette;
    let (is_md, md_mode, annotate, blame) = {
        let Some(tab) = model.projects[index].tab_by_id(tab_id) else {
            return;
        };
        let TabKind::File(file) = &tab.kind else {
            return;
        };
        let is_md = file
            .doc
            .path
            .extension()
            .is_some_and(|ext| ext == "md" || ext == "markdown");
        (is_md, file.md_mode, file.annotate, file.blame.clone())
    };

    if is_md && md_mode == lazecode_core::MarkdownViewMode::Preview {
        let Some(tab) = model.projects[index].tab_by_id(tab_id) else {
            return;
        };
        let TabKind::File(file) = &tab.kind else {
            return;
        };
        let text = file.doc.rope.to_string();
        draw_markdown_view(ui, rt, p, theme, rect, &text, tab_id);
        return;
    }

    if is_md && md_mode == lazecode_core::MarkdownViewMode::Split {
        let half_w = ((rect.width() - 1.0) / 2.0).floor();
        let left_rect = Rect::from_min_size(rect.min, Vec2::new(half_w, rect.height()));
        let right_rect = Rect::from_min_size(
            Pos2::new(rect.min.x + half_w + 1.0, rect.min.y),
            Vec2::new(half_w, rect.height()),
        );
        ui.painter().vline(
            rect.min.x + half_w + 0.5,
            rect.y_range(),
            Stroke::new(1.0, p.border_soft),
        );

        draw_file_inner(
            ui, model, rt, theme, left_rect, index, tab_id, annotate, &blame,
        );

        let Some(tab) = model.projects[index].tab_by_id(tab_id) else {
            return;
        };
        let TabKind::File(file) = &tab.kind else {
            return;
        };
        let text = file.doc.rope.to_string();
        draw_markdown_view(ui, rt, p, theme, right_rect, &text, tab_id);
        return;
    }

    draw_file_inner(ui, model, rt, theme, rect, index, tab_id, annotate, &blame);
}

#[allow(clippy::too_many_arguments)]
fn draw_file_inner(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    index: usize,
    tab_id: u64,
    annotate: bool,
    blame: &Option<Vec<lazecode_git::BlameLine>>,
) {
    let p = theme.palette;
    let settings = model.settings.editor.clone();
    let Some(path) = model
        .projects
        .get(index)
        .and_then(|project| project.tab_by_id(tab_id))
        .and_then(|tab| match &tab.kind {
            TabKind::File(file) => Some(file.doc.path.clone()),
            _ => None,
        })
    else {
        return;
    };
    if rt
        .lsp
        .completion
        .as_ref()
        .is_some_and(|state| state.tab_id != tab_id)
    {
        if let Some(state) = rt.lsp.completion.take() {
            if let Some(id) = state.request {
                model.lsp.cancel(id);
            }
        }
    }
    if rt
        .lsp
        .symbols
        .as_ref()
        .is_some_and(|state| state.tab_id != tab_id)
    {
        if let Some(state) = rt.lsp.symbols.take() {
            if let Some(id) = state.request {
                model.lsp.cancel(id);
            }
        }
    }
    let diagnostics = model.lsp.diagnostics(&path);
    let (font_size, tab_width, line_numbers, current_line, minimap) = (
        settings.font_size,
        settings.tab_width,
        settings.show_line_numbers,
        settings.highlight_current_line,
        model.ui.show_minimap,
    );
    let line_h = (font_size * 1.54).round().max(18.0);
    let char_w = mono_char_width(ui.ctx(), font_size);
    let font = FontId::monospace(font_size);

    let annotate_w = if annotate { 130.0 } else { 0.0 };
    let gutter_w = (if line_numbers { GUTTER } else { 10.0 }) + annotate_w;
    let minimap_w = if minimap { 38.0 } else { 0.0 };
    let content = Rect::from_min_max(
        Pos2::new(rect.left() + gutter_w, rect.top()),
        Pos2::new(rect.right() - SCROLLBAR_W - minimap_w, rect.bottom()),
    );
    let gutter = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top()),
        Pos2::new(rect.left() + gutter_w, rect.bottom()),
    );
    let minimap_rect = Rect::from_min_max(
        Pos2::new(rect.right() - SCROLLBAR_W - minimap_w, rect.top()),
        Pos2::new(rect.right() - SCROLLBAR_W, rect.bottom()),
    );
    ui.painter().rect_filled(gutter, 0.0, p.editor);
    ui.painter().vline(
        gutter.right() - 0.5,
        gutter.y_range(),
        Stroke::new(1.0, p.hairline),
    );
    if minimap {
        ui.painter()
            .rect_filled(minimap_rect, 0.0, p.editor.gamma_multiply(0.98));
        ui.painter().vline(
            minimap_rect.left() + 0.5,
            minimap_rect.y_range(),
            Stroke::new(1.0, p.border_soft),
        );
    }

    let focused = rt.editor_focus == Some(tab_id);
    if focused {
        // Keep the caret blink alive without repainting the whole app when
        // nothing else is happening.
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
    }
    // A modal surface owns the keyboard even without a text field; otherwise
    // typing would edit the focused buffer behind it.
    let overlays_open = rt.blocks_input(model);
    let mut pending: Vec<Command> = Vec::new();
    let mut hover_cursor: Option<Cursor> = None;
    let mut diag_tooltip: Option<(String, lazecode_core::lsp::DiagnosticSeverity)> = None;

    {
        let project = &mut model.projects[index];
        let Some(tab) = project.tab_mut_by_id(tab_id) else {
            return;
        };
        let TabKind::File(file_tab) = &mut tab.kind else {
            return;
        };
        let doc = &mut file_tab.doc;
        let lines = doc.line_count();
        let view_h = content.height();
        let visible_rows = (view_h / line_h).ceil() as usize + 2;
        let scroll_offset = doc.scroll_line * line_h;
        let max_scroll = ((lines as f32 * line_h) - view_h).max(0.0);
        doc.scroll_line = (scroll_offset / line_h).clamp(0.0, max_scroll / line_h);
        let first = doc.scroll_line.floor() as usize;
        let last = (first + visible_rows).min(lines.saturating_sub(1));
        doc.ensure_highlight(
            first.saturating_sub(20),
            (last + 20).min(lines.saturating_sub(1)),
        );

        let input_rect = Rect::from_min_max(
            Pos2::new(gutter.left(), rect.top()),
            Pos2::new(minimap_rect.left(), rect.bottom()),
        );
        let response = ui
            .interact(
                input_rect,
                ui.scope_id().with(("editor", tab_id)),
                Sense::click_and_drag(),
            )
            .on_hover_cursor(egui::CursorIcon::Text);
        if !overlays_open
            && ui
                .input(|i| i.pointer.hover_pos())
                .is_some_and(|pos| input_rect.contains(pos))
        {
            // `Response::hovered` can be shadowed by a transparent interaction
            // layer; containment decides the I-beam cursor.
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        }
        if response.clicked() || response.dragged() {
            rt.set_editor_focus(tab_id);
        }
        let pointer = response.interact_pointer_pos();
        let command_click = response.clicked() && ui.input(|input| input.modifiers.command);
        if let Some(pos) = pointer {
            if response.clicked() || response.dragged() {
                let cursor = cursor_from_pos(pos, content, doc, char_w, line_h, tab_width);
                let extend = response.dragged() || ui.input(|input| input.modifiers.shift);
                doc.set_cursor(cursor, extend);
                if command_click {
                    rt.lsp.goto_request = Some((tab_id, cursor));
                }
                if response.dragged() {
                    rt.editor_drag = true;
                }
            }
        }
        if response.double_clicked() {
            if let Some(pos) = pointer {
                let cursor = cursor_from_pos(pos, content, doc, char_w, line_h, tab_width);
                select_word(doc, cursor);
            }
        }
        if response.secondary_clicked() {
            let path_str = doc.path.to_string_lossy().to_string();
            let anchor = ui
                .input(|i| i.pointer.hover_pos())
                .map(|p| Rect::from_min_size(p, Vec2::splat(1.0)))
                .unwrap_or(input_rect);
            rt.open_menu(
                crate::runtime::MenuKind::EditorContext { project: index },
                anchor,
                Some(path_str),
            );
        }
        // Wheel scrolling.
        if response.hovered() {
            let (dy, dx, shift) = ui.input(|input| {
                (
                    input.smooth_scroll_delta.y,
                    input.smooth_scroll_delta.x,
                    input.modifiers.shift,
                )
            });
            if dy.abs() > 0.0 || dx.abs() > 0.0 {
                if shift {
                    doc.scroll_x = (doc.scroll_x - dy).max(0.0);
                } else {
                    doc.scroll_line =
                        (doc.scroll_line - dy / line_h).clamp(0.0, (max_scroll / line_h).max(0.0));
                    doc.scroll_x = (doc.scroll_x - dx).max(0.0);
                }
            }
        }

        // `text_edit_focused` (not `egui_wants_keyboard_input`): egui's Tab
        // navigation moving focus onto some button must not mute the editor.
        if focused && !overlays_open && !ui.ctx().text_edit_focused() {
            handle_editor_input(ui, doc, &mut pending, tab_id, rt);
        }

        let scroll_rect =
            Rect::from_min_max(Pos2::new(rect.right() - SCROLLBAR_W, rect.top()), rect.max);
        let mut offset = doc.scroll_line * line_h;
        if crate::widgets::scrollbar(
            ui,
            scroll_rect,
            lines as f32 * line_h,
            view_h,
            &mut offset,
            theme,
        ) {
            doc.scroll_line = (offset / line_h).clamp(0.0, (max_scroll / line_h).max(0.0));
        }

        // Text is clipped to the content column; the gutter needs its own clip
        // because it lives to the left of `content`.
        let painter = ui.painter().with_clip_rect(content);
        let gutter_painter = ui.painter().with_clip_rect(gutter);
        let sel = doc.selection_char_range();
        let cursor = doc.cursor;
        let scroll_line = doc.scroll_line;
        let scroll_x = doc.scroll_x;
        let tab_w = tab_width.max(1);
        let mut line_chars: Vec<char> = Vec::new();
        let mut line_kinds: Vec<TokenKind> = Vec::new();
        for line_index in first..=last {
            let y = content.top() + (line_index as f32 - scroll_line) * line_h;
            if line_index == first && scroll_line.fract() > 0.0 {
                // still draw; clipping handles the partial row
            }
            if current_line && line_index == cursor.line && focused {
                painter.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(gutter.left(), y),
                        Pos2::new(rect.right() - SCROLLBAR_W, y + line_h),
                    ),
                    0.0,
                    p.current_line,
                );
            }
            if let Some((start, end)) = sel {
                let line_start = doc.rope.line_to_char(line_index);
                let line_len = doc.line_content_len(line_index);
                let a = start.max(line_start);
                let b = end.min(line_start + line_len);
                if a < b {
                    let line_text = doc.line_content(line_index);
                    let v0 = Document::visual_col(&line_text, a - line_start, tab_w);
                    let v1 = Document::visual_col(&line_text, b - line_start, tab_w);
                    painter.rect_filled(
                        Rect::from_min_size(
                            Pos2::new(content.left() + v0 as f32 * char_w - scroll_x, y),
                            Vec2::new(((v1 - v0) as f32 * char_w).max(2.0), line_h),
                        ),
                        0.0,
                        p.selection,
                    );
                }
            }
            if annotate {
                if let Some(blame_lines) = blame {
                    if let Some(bl) = blame_lines.get(line_index) {
                        gutter_painter.text(
                            Pos2::new(gutter.left() + 6.0, y + line_h * 0.5),
                            Align2::LEFT_CENTER,
                            format!("{} {}", bl.short, bl.author),
                            FontId::monospace(11.0),
                            p.accent,
                        );
                    }
                }
            }
            if line_numbers {
                gutter_painter.text(
                    Pos2::new(gutter.right() - 10.0, y + line_h * 0.5),
                    Align2::RIGHT_CENTER,
                    (line_index + 1).to_string(),
                    FontId::monospace((font_size - 1.5).max(11.5)),
                    if line_index == cursor.line && focused {
                        p.text3
                    } else {
                        p.text3.gamma_multiply(0.72)
                    },
                );
            }
            let text = doc.line_content(line_index);
            let spans = doc.highlighter.spans(line_index);
            draw_line_runs(
                &painter,
                &text,
                spans,
                content.left() - scroll_x,
                y,
                line_h,
                char_w,
                tab_w,
                &font,
                theme,
                &mut line_chars,
                &mut line_kinds,
            );
            // LSP diagnostics: one underline per diagnostic per visible line.
            // Multi-line ranges clip to the current row.
            if !diagnostics.is_empty() {
                let text_len = text.chars().count();
                let line_u32 = line_index as u32;
                for diagnostic in diagnostics {
                    let start = diagnostic.range.start;
                    let end = diagnostic.range.end;
                    if line_u32 < start.line || line_u32 > end.line {
                        continue;
                    }
                    let start_col = if line_u32 == start.line {
                        lazecode_text::lsp::char_col_for_utf16(&text, start.character)
                    } else {
                        0
                    }
                    .min(text_len);
                    let mut end_col = if line_u32 == end.line {
                        lazecode_text::lsp::char_col_for_utf16(&text, end.character)
                    } else {
                        text_len
                    }
                    .min(text_len);
                    if end_col <= start_col {
                        if start.line != end.line {
                            continue;
                        }
                        end_col = (start_col + 1).min(text_len);
                    }
                    let v0 = Document::visual_col(&text, start_col, tab_w);
                    let v1 = Document::visual_col(&text, end_col, tab_w);
                    let x0 = (content.left() + v0 as f32 * char_w - scroll_x).max(content.left());
                    let x1 = (content.left() + v1 as f32 * char_w - scroll_x)
                        .max(x0 + 2.0)
                        .min(content.right());
                    if x1 <= content.left() || x0 >= content.right() {
                        continue;
                    }
                    let y_line = y + line_h - 2.0;
                    painter.line_segment(
                        [Pos2::new(x0, y_line), Pos2::new(x1, y_line)],
                        Stroke::new(
                            1.5,
                            crate::lsp_ui::diagnostic_color(theme, diagnostic.severity),
                        ),
                    );
                }
            }
            let leading_spaces = text.chars().take_while(|c| *c == ' ').count();
            let leading_tabs = text.chars().take_while(|c| *c == '\t').count();
            let total_indent_cols = leading_tabs * tab_w + leading_spaces;
            if total_indent_cols >= tab_w {
                for col in (tab_w..total_indent_cols).step_by(tab_w) {
                    let gx = content.left() + col as f32 * char_w - scroll_x;
                    if gx >= content.left() && gx <= content.right() {
                        painter.vline(
                            gx,
                            egui::Rangef::new(y, y + line_h),
                            Stroke::new(1.0, p.hairline),
                        );
                    }
                }
            }
        }

        // Cursor + preedit + IME activation.
        if focused && !overlays_open {
            let line = cursor.line.min(lines.saturating_sub(1));
            let line_text = doc.line_content(line);
            let vcol = Document::visual_col(&line_text, cursor.col, tab_w);
            let x = content.left() + vcol as f32 * char_w - doc.scroll_x;
            let y = content.top() + (line as f32 - doc.scroll_line) * line_h;
            let cursor_rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(char_w, line_h));
            if let Some(state) = rt.lsp.completion.as_mut() {
                if state.tab_id == tab_id {
                    state.anchor = Pos2::new(x, y);
                }
            }
            ui.ctx().output_mut(|o| {
                o.ime = Some(egui::output::IMEOutput {
                    purpose: egui::IMEPurpose::Normal,
                    rect: content,
                    cursor_rect,
                    should_interrupt_composition: false,
                });
            });
            if y + line_h > content.top() && y < content.bottom() && x >= content.left() - 1.0 {
                let time = ui.input(|input| input.time);
                let blink = ((time * 1.4) as i64 % 2) == 0;
                if blink || !rt.ime_preedit.is_empty() {
                    ui.painter().rect_filled(
                        Rect::from_min_size(Pos2::new(x, y + 1.0), Vec2::new(1.8, line_h - 2.0)),
                        0.0,
                        p.accent,
                    );
                }
                if !rt.ime_preedit.is_empty() {
                    let preedit_rect = Rect::from_min_size(
                        Pos2::new(x, y),
                        Vec2::new(char_w * rt.ime_preedit.chars().count() as f32, line_h),
                    );
                    ui.painter().text(
                        Pos2::new(x, y + line_h * 0.5),
                        Align2::LEFT_CENTER,
                        &rt.ime_preedit,
                        font.clone(),
                        p.text,
                    );
                    ui.painter().line_segment(
                        [
                            Pos2::new(preedit_rect.left(), preedit_rect.bottom() - 1.0),
                            Pos2::new(preedit_rect.right(), preedit_rect.bottom() - 1.0),
                        ],
                        Stroke::new(1.0, p.accent),
                    );
                }
            }
        }

        if minimap {
            draw_minimap(ui, doc, theme, minimap_rect, view_h, line_h);
        }
        if !overlays_open {
            hover_cursor = ui
                .input(|input| input.pointer.hover_pos())
                .filter(|pos| content.contains(*pos) && !ui.input(|input| input.pointer.any_down()))
                .map(|pos| cursor_from_pos(pos, content, doc, char_w, line_h, tab_width));
        }
        if let Some(cursor) = hover_cursor {
            diag_tooltip = crate::lsp_ui::diagnostic_at(doc, diagnostics, cursor)
                .map(|diagnostic| (diagnostic.message.clone(), diagnostic.severity));
        }
        let _ = MONO;
    }

    for command in pending {
        model.queue(command);
    }
    crate::lsp_ui::update_hover(ui.ctx(), model, rt, tab_id, hover_cursor);
    if rt
        .lsp
        .hover
        .as_ref()
        .and_then(|state| state.result.as_ref())
        .is_none()
    {
        if let Some((message, severity)) = diag_tooltip {
            let color = crate::lsp_ui::diagnostic_color(theme, severity);
            egui::Tooltip::always_open(
                ui.ctx().clone(),
                ui.layer_id(),
                egui::Id::unique(("lsp-diagnostic", tab_id)),
                egui::PopupAnchor::Pointer,
            )
            .show(|ui| {
                ui.set_max_width(520.0);
                ui.label(egui::RichText::new(message).size(12.0).color(color));
            });
        }
    }
    let _ = (p, rect, tab_id);
    // Find overlay sits above the text.
    if rt.find.open {
        draw_find(
            ui,
            theme,
            rect,
            &mut model.projects[index],
            tab_id,
            rt,
            model.settings.language,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_line_runs(
    painter: &egui::Painter,
    text: &str,
    spans: &[lazecode_text::HighlightSpan],
    origin_x: f32,
    y: f32,
    line_h: f32,
    char_w: f32,
    tab_width: usize,
    font: &FontId,
    theme: &Theme,
    chars: &mut Vec<char>,
    kinds: &mut Vec<TokenKind>,
) {
    if text.is_empty() {
        return;
    }
    chars.clear();
    chars.extend(text.chars());
    kinds.clear();
    kinds.resize(chars.len(), TokenKind::Text);
    for span in spans {
        for kind in kinds
            .iter_mut()
            .take(span.end.min(chars.len()))
            .skip(span.start)
        {
            *kind = span.kind;
        }
    }
    let mut visual = 0usize;
    let mut index = 0usize;
    while index < chars.len() {
        let kind = kinds[index];
        let start_visual = visual;
        let mut run = String::new();
        while index < chars.len() && kinds[index] == kind {
            let ch = chars[index];
            if ch == '\t' {
                let width = tab_width - (visual % tab_width);
                for _ in 0..width {
                    run.push(' ');
                    visual += 1;
                }
            } else {
                let width = unicode_width::UnicodeWidthChar::width(ch)
                    .unwrap_or(1)
                    .max(1);
                run.push(ch);
                visual += width;
            }
            index += 1;
        }
        if !run.is_empty() {
            painter.text(
                Pos2::new(origin_x + start_visual as f32 * char_w, y + line_h * 0.5),
                Align2::LEFT_CENTER,
                run,
                font.clone(),
                token_color(kind, theme),
            );
        }
    }
}

fn cursor_from_pos(
    pos: Pos2,
    content: Rect,
    doc: &Document,
    char_w: f32,
    line_h: f32,
    tab_width: usize,
) -> Cursor {
    let line = ((pos.y - content.top()) / line_h + doc.scroll_line).floor();
    let line = (line.max(0.0) as usize).min(doc.line_count().saturating_sub(1));
    let visual_col = (((pos.x - content.left() + doc.scroll_x) / char_w).round()).max(0.0) as usize;
    let text = doc.line_content(line);
    let mut visual = 0usize;
    let mut col = 0usize;
    for ch in text.chars() {
        if visual >= visual_col {
            break;
        }
        if ch == '\t' {
            let width = tab_width.max(1) - (visual % tab_width.max(1));
            visual += width;
        } else {
            visual += unicode_width::UnicodeWidthChar::width(ch)
                .unwrap_or(1)
                .max(1);
        }
        col += 1;
    }
    Cursor { line, col }
}

fn select_word(doc: &mut Document, cursor: Cursor) {
    let text = doc.line_content(cursor.line);
    let chars: Vec<char> = text.chars().collect();
    let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
    let mut start = cursor.col.min(chars.len());
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let mut end = cursor.col.min(chars.len());
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    if start == end && end < chars.len() {
        end += 1;
    }
    doc.selection = Some(Selection {
        anchor: Cursor {
            line: cursor.line,
            col: start,
        },
        head: Cursor {
            line: cursor.line,
            col: end,
        },
    });
    doc.cursor = Cursor {
        line: cursor.line,
        col: end,
    };
}

fn handle_editor_input(
    ui: &mut Ui,
    doc: &mut Document,
    pending: &mut Vec<Command>,
    tab_id: u64,
    rt: &mut UiRuntime,
) {
    let events = ui.input(|input| input.events.clone());
    for event in events {
        // Read-only snapshots (repository version tabs) allow navigation and
        // copy but no mutation, so the buffer never diverges from the blob.
        if doc.read_only {
            match &event {
                Event::Copy => {
                    if let Some(text) = doc.selected_text() {
                        ui.ctx().copy_text(text);
                    }
                }
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let navigates = matches!(
                        key,
                        Key::ArrowUp
                            | Key::ArrowDown
                            | Key::ArrowLeft
                            | Key::ArrowRight
                            | Key::Home
                            | Key::End
                            | Key::PageUp
                            | Key::PageDown
                    ) || (modifiers.command && matches!(key, Key::A | Key::C));
                    if navigates {
                        let _ = handle_editor_key(ui, doc, pending, tab_id, rt, *key, *modifiers);
                    }
                }
                _ => {}
            }
            continue;
        }
        if handle_completion_event(doc, rt, tab_id, &event) {
            continue;
        }
        if handle_symbol_event(doc, rt, tab_id, &event) {
            continue;
        }
        match event {
            Event::Text(text) => {
                doc.insert_text(&text);
                if crate::lsp_ui::is_completion_trigger(&text)
                    && lazecode_core::language_id_for_path(&doc.path).is_some()
                {
                    rt.lsp.completion_due = Some((tab_id, std::time::Instant::now()));
                }
                if rt.lsp.completion_due.is_some() {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(160));
                }
            }
            Event::Paste(text) => {
                doc.insert_text(&text);
            }
            Event::Copy => {
                if let Some(text) = doc.selected_text() {
                    ui.ctx().copy_text(text);
                }
            }
            Event::Cut => {
                if let Some(text) = doc.selected_text() {
                    ui.ctx().copy_text(text);
                    doc.delete_selection();
                }
            }
            Event::Ime(ImeEvent::Preedit { text, .. }) => {
                rt.ime_preedit = text;
            }
            Event::Ime(ImeEvent::Commit(text)) => {
                rt.ime_preedit.clear();
                doc.insert_text(&text);
            }
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } if handle_editor_key(ui, doc, pending, tab_id, rt, key, modifiers) => {
                ui.ctx().request_repaint();
            }
            _ => {}
        }
    }
    if doc.dirty && doc.highlighter.is_enabled() {
        ui.ctx().request_repaint();
    }
    let _ = (tab_id, pending);
}

/// Keys while the completion popup is open; `true` consumes the event.
fn handle_completion_event(
    doc: &mut Document,
    rt: &mut UiRuntime,
    tab_id: u64,
    event: &Event,
) -> bool {
    let Some(mut state) = rt.lsp.completion.take() else {
        return false;
    };
    if state.tab_id != tab_id {
        rt.lsp.completion = Some(state);
        return false;
    }
    let mut keep = true;
    let mut consumed = false;
    match event {
        Event::Key {
            key, pressed: true, ..
        } => match key {
            Key::ArrowDown => {
                if !state.items.is_empty() {
                    state.selected = (state.selected + 1) % state.items.len();
                }
                consumed = true;
            }
            Key::ArrowUp => {
                if !state.items.is_empty() {
                    state.selected = if state.selected == 0 {
                        state.items.len() - 1
                    } else {
                        state.selected - 1
                    };
                }
                consumed = true;
            }
            Key::Enter | Key::Tab => {
                crate::lsp_ui::accept_completion(doc, &state);
                keep = false;
                consumed = true;
            }
            Key::Escape => {
                keep = false;
                consumed = true;
            }
            Key::ArrowLeft | Key::ArrowRight | Key::Delete => {
                keep = false;
            }
            Key::Backspace => {
                keep = false;
                rt.lsp.completion_due = Some((tab_id, std::time::Instant::now()));
            }
            _ => {}
        },
        Event::Text(text) => {
            keep = false;
            // Keep filtering: re-request after the character is inserted.
            if text.chars().all(|ch| ch.is_alphanumeric() || ch == '_') {
                rt.lsp.completion_due = Some((tab_id, std::time::Instant::now()));
            }
        }
        Event::Paste(_) => keep = false,
        _ => {}
    }
    if keep {
        rt.lsp.completion = Some(state);
    }
    consumed
}

/// The symbol picker is modal-ish: it swallows typing so a stray character
/// cannot edit the buffer behind it.
fn handle_symbol_event(doc: &mut Document, rt: &mut UiRuntime, tab_id: u64, event: &Event) -> bool {
    let Some(mut state) = rt.lsp.symbols.take() else {
        return false;
    };
    if state.tab_id != tab_id {
        rt.lsp.symbols = Some(state);
        return false;
    }
    let mut keep = true;
    match event {
        Event::Key {
            key, pressed: true, ..
        } => match key {
            Key::ArrowDown => {
                if !state.items.is_empty() {
                    state.selected = (state.selected + 1) % state.items.len();
                }
            }
            Key::ArrowUp => {
                if !state.items.is_empty() {
                    state.selected = if state.selected == 0 {
                        state.items.len() - 1
                    } else {
                        state.selected - 1
                    };
                }
            }
            Key::Enter | Key::Tab => {
                crate::lsp_ui::accept_symbol(doc, &state);
                keep = false;
            }
            Key::Escape => keep = false,
            _ => {}
        },
        Event::Text(_) | Event::Paste(_) => keep = false,
        _ => {
            rt.lsp.symbols = Some(state);
            return false;
        }
    }
    if keep {
        rt.lsp.symbols = Some(state);
    }
    true
}

pub(crate) fn word_at(doc: &Document, cursor: Cursor) -> String {
    let line = doc.line_content(cursor.line);
    let chars: Vec<char> = line.chars().collect();
    let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
    let mut start = cursor.col.min(chars.len());
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let mut end = cursor.col.min(chars.len());
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    chars[start..end].iter().collect()
}

#[allow(clippy::too_many_arguments)]
fn handle_editor_key(
    ui: &mut Ui,
    doc: &mut Document,
    pending: &mut Vec<Command>,
    tab_id: u64,
    rt: &mut UiRuntime,
    key: Key,
    modifiers: Modifiers,
) -> bool {
    let cmd = modifiers.command;
    if cmd && modifiers.alt && key == Key::L {
        rt.lsp.format_request = Some(tab_id);
        return true;
    }
    if cmd && modifiers.alt && key == Key::O {
        rt.lsp.symbols_request = Some(tab_id);
        return true;
    }
    if modifiers.ctrl && key == Key::Space {
        rt.lsp.completion_due = Some((tab_id, std::time::Instant::now()));
        return true;
    }
    if key == Key::F12 || (cmd && !modifiers.shift && !modifiers.alt && key == Key::B) {
        rt.lsp.goto_request = Some((tab_id, doc.cursor));
        return true;
    }
    if key == Key::F2 {
        let word = word_at(doc, doc.cursor);
        rt.dialog = DialogState::RenameSymbol {
            tab_id,
            path: doc.path.to_string_lossy().to_string(),
            line: doc.cursor.line,
            col: doc.cursor.col,
            value: word,
            error: None,
        };
        return true;
    }
    if cmd {
        match key {
            Key::Z => {
                if modifiers.shift {
                    doc.redo();
                } else {
                    doc.undo();
                }
                return true;
            }
            Key::Y => {
                doc.redo();
                return true;
            }
            Key::A => {
                doc.select_all();
                return true;
            }
            Key::C => {
                if let Some(text) = doc.selected_text() {
                    ui.ctx().copy_text(text);
                }
                return true;
            }
            Key::X => {
                if let Some(text) = doc.selected_text() {
                    ui.ctx().copy_text(text);
                    doc.delete_selection();
                }
                return true;
            }
            Key::S => {
                return false;
            }
            _ => {}
        }
    }
    match key {
        Key::Enter => doc.newline(),
        Key::Backspace => doc.backspace(),
        Key::Delete => doc.delete_forward(),
        Key::Tab => {
            if modifiers.shift {
                doc.outdent_selection();
            } else {
                doc.indent_selection();
            }
        }
        Key::ArrowLeft => doc.move_cursor(
            if cmd {
                Motion::LineStart
            } else if modifiers.alt {
                Motion::WordLeft
            } else {
                Motion::Left
            },
            modifiers.shift,
        ),
        Key::ArrowRight => doc.move_cursor(
            if cmd {
                Motion::LineEnd
            } else if modifiers.alt {
                Motion::WordRight
            } else {
                Motion::Right
            },
            modifiers.shift,
        ),
        Key::ArrowUp => doc.move_cursor(
            if cmd { Motion::DocStart } else { Motion::Up },
            modifiers.shift,
        ),
        Key::ArrowDown => doc.move_cursor(
            if cmd { Motion::DocEnd } else { Motion::Down },
            modifiers.shift,
        ),
        Key::Home => doc.move_cursor(Motion::Home, modifiers.shift),
        Key::End => doc.move_cursor(Motion::End, modifiers.shift),
        Key::PageUp => doc.move_cursor(Motion::PageUp(24), modifiers.shift),
        Key::PageDown => doc.move_cursor(Motion::PageDown(24), modifiers.shift),
        Key::Escape => {
            doc.selection = None;
        }
        _ => return false,
    }
    let _ = pending;
    true
}

fn draw_minimap(
    ui: &mut Ui,
    doc: &mut Document,
    theme: &Theme,
    rect: Rect,
    view_h: f32,
    line_h: f32,
) {
    let p = theme.palette;
    let total = doc.line_count().max(1);
    let visible_lines = (view_h / line_h).max(1.0);
    let painter = ui.painter().with_clip_rect(rect);

    // 对齐 Monaco Editor Minimap 核心机制：
    // 每行固定行高 2.0px（Monaco scale: 1 默认值），缩进与代码宽度等比投影。
    const MINIMAP_LINE_H: f32 = 2.0;
    let minimap_content_h = total as f32 * MINIMAP_LINE_H;
    let max_editor_scroll = ((total as f32 * line_h) - view_h).max(0.0) / line_h;
    let scroll_ratio = if max_editor_scroll > 0.0 {
        (doc.scroll_line / max_editor_scroll).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let (minimap_scroll_y, slider_top, slider_h) = if minimap_content_h <= rect.height() {
        // 短文件：Minimap 不滚动，代码从顶部画起；滑块高度与当前可见行严格同比例覆盖
        let s_top = rect.top() + doc.scroll_line * MINIMAP_LINE_H;
        let s_h = (visible_lines * MINIMAP_LINE_H).clamp(18.0, minimap_content_h.max(18.0));
        let s_top = s_top.clamp(
            rect.top(),
            (rect.top() + minimap_content_h - s_h).max(rect.top()),
        );
        (0.0, s_top, s_h)
    } else {
        // 长文件：Minimap 随编辑器滚动同步平滑滚动（Monaco 滚动映射）
        let max_minimap_scroll = minimap_content_h - rect.height();
        let m_scroll = scroll_ratio * max_minimap_scroll;
        let s_h = (visible_lines * MINIMAP_LINE_H).clamp(20.0, rect.height());
        let s_top = rect.top() + doc.scroll_line * MINIMAP_LINE_H - m_scroll;
        let s_top = s_top.clamp(rect.top(), (rect.bottom() - s_h).max(rect.top()));
        (m_scroll, s_top, s_h)
    };

    // 绘制代码微缩行（带缩进投影，与编辑器代码轮廓一比一对应）
    for line_index in 0..total {
        let y = rect.top() + line_index as f32 * MINIMAP_LINE_H - minimap_scroll_y;
        if y + MINIMAP_LINE_H < rect.top() {
            continue;
        }
        if y > rect.bottom() {
            break;
        }
        let text = doc.line_content(line_index);
        let trimmed = text.trim_start();
        let indent = text.len() - trimmed.len();
        let indent_x = (indent as f32 * 0.8).min(rect.width() * 0.35);
        let width = (trimmed.trim_end().chars().count() as f32 * 0.65)
            .clamp(0.0, rect.width() - 8.0 - indent_x);
        if width > 1.0 {
            let color = if trimmed.starts_with("//") || trimmed.starts_with('#') {
                p.tok_com
            } else {
                p.text3
            };
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(rect.left() + 4.0 + indent_x, y),
                    Vec2::new(width, 1.5),
                ),
                0.0,
                color.gamma_multiply(0.32),
            );
        }
    }

    let slider_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 1.0, slider_top),
        Vec2::new(rect.width() - 2.0, slider_h),
    );

    // 交互：点击或拖拽 minimap 滚动编辑器
    let response = ui.interact(rect, ui.scope_id().with("minimap"), Sense::click_and_drag());
    if (response.clicked() || response.dragged()) && max_editor_scroll > 0.0 {
        if let Some(pos) = response.interact_pointer_pos() {
            if minimap_content_h <= rect.height() {
                let clicked_line = ((pos.y - rect.top()) / MINIMAP_LINE_H).max(0.0);
                let target_line = clicked_line - visible_lines * 0.5;
                doc.scroll_line = target_line.clamp(0.0, max_editor_scroll);
            } else {
                let rel_y = ((pos.y - rect.top() - slider_h * 0.5)
                    / (rect.height() - slider_h).max(1.0))
                .clamp(0.0, 1.0);
                doc.scroll_line = (rel_y * max_editor_scroll).clamp(0.0, max_editor_scroll);
            }
        }
    }

    let slider_bg = if response.dragged() {
        Color32::from_rgba_unmultiplied(255, 255, 255, 36)
    } else if response.hovered() {
        Color32::from_rgba_unmultiplied(255, 255, 255, 26)
    } else {
        Color32::from_rgba_unmultiplied(255, 255, 255, 16)
    };
    ui.painter()
        .rect_filled(slider_rect, egui::CornerRadius::same(3), slider_bg);
    ui.painter().rect_stroke(
        slider_rect,
        egui::CornerRadius::same(3),
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 45)),
        StrokeKind::Inside,
    );
}

fn draw_diff(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    index: usize,
    tab_id: u64,
) {
    let p = theme.palette;
    let Some(tab) = model.projects[index].tab_by_id(tab_id) else {
        return;
    };
    let TabKind::Diff(diff) = &tab.kind else {
        return;
    };
    let path = diff.path.clone();
    let diff_source = diff.source.clone();
    let view_mode = diff.view_mode;
    let current_group = diff.current_hunk;
    let edge_pending = diff.edge_pending;
    let revealed = diff.revealed;
    let status = diff.diff.as_ref().map(|d| d.kind);
    let additions = diff.diff.as_ref().map(|d| d.diff.additions).unwrap_or(0);
    let deletions = diff.diff.as_ref().map(|d| d.diff.deletions).unwrap_or(0);
    let error = diff.error.clone();
    let hunks = diff.diff.as_ref().map(|d| d.diff.hunks.clone());
    let binary = diff.diff.as_ref().map(|d| d.binary).unwrap_or(false);
    let loading = diff.loading;

    // Monaco 语义：变更组（连续增删行）驱动导航/蓝框，git hunk 数驱动统计。
    let (rows, groups) = hunks
        .as_deref()
        .map_or_else(|| (Vec::new(), 0usize), crate::diff_view::build_rows);

    // ---- 跨文件导航清单（Compare 抽屉 / 提交详情文件树） ----
    let nav_files: Vec<String> = match &diff_source {
        DiffSource::CompareCommits { from, to, .. } => model.projects[index]
            .compare_session
            .as_ref()
            .filter(|s| &s.from == from && &s.to == to)
            .map(|s| s.nav_order.clone())
            .unwrap_or_default(),
        DiffSource::Stash { stash, .. } => model.projects[index]
            .stash_details
            .get(stash)
            .map(|details| {
                details
                    .tracked
                    .iter()
                    .chain(details.untracked.iter())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default(),
        DiffSource::StashBeforeLocal { stash, .. } => model.projects[index]
            .stash_details
            .get(stash)
            .map(|details| {
                details
                    .tracked
                    .iter()
                    .chain(details.untracked.iter())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default(),
        DiffSource::Commit { commit, .. } => model.projects[index]
            .commit_details
            .as_ref()
            .filter(|d| &d.info.id == commit)
            .map(|d| d.files.iter().map(|f| f.path.clone()).collect())
            .unwrap_or_default(),
        DiffSource::Worktree { .. } => {
            let project = &model.projects[index];
            let lists = lazecode_git::Changelists::open(project.active_root());
            let changes = project.changes();
            let mut ordered = Vec::new();
            for list_name in lists.lists() {
                for c in changes {
                    if !c.untracked && lists.list_of(&c.path) == *list_name {
                        ordered.push(c.path.clone());
                    }
                }
            }
            for c in changes {
                if !c.untracked && !ordered.contains(&c.path) {
                    ordered.push(c.path.clone());
                }
            }
            for c in changes {
                if c.untracked && !ordered.contains(&c.path) {
                    ordered.push(c.path.clone());
                }
            }
            ordered
        }
        _ => Vec::new(),
    };
    let has_nav = !nav_files.is_empty();
    let nav_index = nav_files.iter().position(|f| *f == path);
    let can_prev_file = has_nav && nav_index.is_some_and(|i| i > 0);
    let can_next_file = has_nav && nav_index.is_some_and(|i| i + 1 < nav_files.len());

    // F7 / Shift+F7（稳定版快捷键）。
    let (f7, f7_shift) = ui.input(|input| {
        (
            input.key_pressed(egui::Key::F7),
            input.modifiers.shift && input.key_pressed(egui::Key::F7),
        )
    });
    let mut nav_request: i32 = 0;
    if f7_shift {
        nav_request = -1;
    } else if f7 {
        nav_request = 1;
    }

    let zh = model.settings.language.is_zh();
    let worktree_mode = matches!(diff_source, DiffSource::Worktree { .. });

    // ----- 稳定版 DiffViewer：28px 工具栏 + 28px 等宽 pane 标题栏 -----
    let toolbar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 28.0));
    ui.painter().rect_filled(toolbar, 0.0, p.sidebar);
    ui.painter().hline(
        toolbar.x_range(),
        toolbar.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    let titlebar = Rect::from_min_size(
        Pos2::new(rect.left(), toolbar.bottom()),
        Vec2::new(rect.width(), 28.0),
    );
    ui.painter().rect_filled(titlebar, 0.0, p.sidebar);
    ui.painter().hline(
        titlebar.x_range(),
        titlebar.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );

    let mut nav_target: Option<String> = None;
    let mut lx = toolbar.left() + 8.0;
    let icon_btn = |ui: &mut Ui, x: &mut f32, icon: Icon, enabled: bool| -> bool {
        let rect =
            Rect::from_center_size(Pos2::new(*x + 10.0, toolbar.center().y), Vec2::splat(20.0));
        *x += 22.0;
        if !enabled {
            paint_icon(
                ui.painter(),
                rect.shrink(3.5),
                icon,
                p.text3.gamma_multiply(0.5),
            );
            return false;
        }
        let response = ui.interact(
            rect,
            ui.scope_id().with(("diff-tb", *x as u32, icon as u8)),
            Sense::click(),
        );
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(6), p.hover);
        }
        paint_icon(ui.painter(), rect.shrink(3.5), icon, p.text2);
        response.clicked()
    };
    // 文件间导航 ←/→（清单来自 Compare 抽屉或提交详情文件树）。
    let _ = icon_btn(ui, &mut lx, Icon::ArrowLeft, can_prev_file);
    let _ = icon_btn(ui, &mut lx, Icon::ArrowRight, can_next_file);
    if has_nav && nav_index.is_some() {
        // 命中检测在下方单独处理（icon_btn 已渲染）；用位置重新交互一次。
    }
    let prev_file_rect = Rect::from_center_size(
        Pos2::new(toolbar.left() + 8.0 + 10.0, toolbar.center().y),
        Vec2::splat(20.0),
    );
    let next_file_rect = Rect::from_center_size(
        Pos2::new(toolbar.left() + 8.0 + 22.0 + 10.0, toolbar.center().y),
        Vec2::splat(20.0),
    );
    if can_prev_file
        && ui
            .interact(
                prev_file_rect,
                ui.scope_id().with("diff-prev-file"),
                Sense::click(),
            )
            .clicked()
    {
        if let Some(i) = nav_index {
            nav_target = Some(nav_files[i - 1].clone());
        }
    }
    if can_next_file
        && ui
            .interact(
                next_file_rect,
                ui.scope_id().with("diff-next-file"),
                Sense::click(),
            )
            .clicked()
    {
        if let Some(i) = nav_index {
            nav_target = Some(nav_files[i + 1].clone());
        }
    }
    lx += 3.0;
    ui.painter().vline(
        lx,
        (toolbar.center().y - 7.0)..=(toolbar.center().y + 7.0),
        Stroke::new(1.0, p.border_soft),
    );
    lx += 5.0;
    let can_hunk_nav = groups > 0;
    let _ = icon_btn(ui, &mut lx, Icon::ChevronUp, can_hunk_nav);
    let _ = icon_btn(ui, &mut lx, Icon::ChevronDown, can_hunk_nav);
    let up_rect = Rect::from_center_size(
        Pos2::new(lx - 44.0 + 10.0, toolbar.center().y),
        Vec2::splat(20.0),
    );
    let down_rect = Rect::from_center_size(
        Pos2::new(lx - 22.0 + 10.0, toolbar.center().y),
        Vec2::splat(20.0),
    );
    if can_hunk_nav {
        if ui
            .interact(
                up_rect,
                ui.scope_id().with("diff-prev-hunk"),
                Sense::click(),
            )
            .clicked()
        {
            nav_request -= 1;
        }
        if ui
            .interact(
                down_rect,
                ui.scope_id().with("diff-next-hunk"),
                Sense::click(),
            )
            .clicked()
        {
            nav_request += 1;
        }
    }
    if groups > 0 {
        // hunk 计数药丸："n / m"（Monaco 变更组数）。
        let counter = Rect::from_center_size(
            Pos2::new(lx + 24.0, toolbar.center().y),
            Vec2::new(44.0, 20.0),
        );
        lx += 50.0;
        ui.painter()
            .rect_filled(counter, egui::CornerRadius::same(4), p.panel2);
        ui.painter().rect_stroke(
            counter,
            egui::CornerRadius::same(4),
            Stroke::new(1.0, p.border_soft),
            StrokeKind::Inside,
        );
        let hunk_text = format!(
            "{} / {}",
            current_group.min(groups.saturating_sub(1)) + 1,
            groups
        );
        ui.painter().text(
            counter.center(),
            Align2::CENTER_CENTER,
            hunk_text,
            FontId::proportional(11.0),
            p.text,
        );
        // 工作区模式：当前组所在 hunk 的 Stage / Rollback（稳定版工具栏按钮）。
        if worktree_mode {
            let mut hunk_btn = |ui: &mut Ui,
                                x: &mut f32,
                                label: &str,
                                patch: Option<String>,
                                cached: bool,
                                reverse: bool| {
                let w = label.chars().count() as f32 * 6.2 + 14.0;
                let rect = Rect::from_center_size(
                    Pos2::new(*x + w / 2.0, toolbar.center().y),
                    Vec2::new(w, 18.0),
                );
                *x += w + 4.0;
                let response = ui.interact(
                    rect,
                    ui.scope_id().with(("diff-hunk-btn", label)),
                    Sense::click(),
                );
                let bg = if response.hovered() {
                    p.hover
                } else {
                    Color32::TRANSPARENT
                };
                ui.painter()
                    .rect_filled(rect, egui::CornerRadius::same(4), bg);
                ui.painter().rect_stroke(
                    rect,
                    egui::CornerRadius::same(4),
                    Stroke::new(1.0, p.border_soft),
                    StrokeKind::Inside,
                );
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    label,
                    FontId::proportional(10.0),
                    if response.hovered() {
                        p.text_bright
                    } else {
                        p.text2
                    },
                );
                if response.clicked() {
                    if let Some(patch) = patch {
                        model.queue(Command::ApplyPatch {
                            patch,
                            cached,
                            reverse,
                            root: model.projects[index].active_root().to_path_buf(),
                        });
                    }
                }
            };
            let patch = hunks.as_ref().and_then(|h| {
                let hi = crate::diff_view::hunk_of_group(h, current_group)?;
                Some(lazecode_text::unified_patch(
                    &path,
                    &lazecode_text::TextDiff {
                        hunks: vec![h[hi].clone()],
                        ..Default::default()
                    },
                ))
            });
            let label_stage = if zh { "暂存此块" } else { "Stage hunk" };
            let label_rollback = if zh { "回滚此块" } else { "Rollback hunk" };
            let patch2 = patch.clone();
            hunk_btn(ui, &mut lx, label_stage, patch, true, false);
            hunk_btn(ui, &mut lx, label_rollback, patch2, false, true);
        }
    }
    // 视图选择（select 样式 20px，点击弹 Unified/Side by Side 两项）。
    let mode_label = match view_mode {
        DiffViewMode::Unified => crate::tr!(model, "Unified view", "统一视图"),
        DiffViewMode::SideBySide => crate::tr!(model, "Side by Side", "并排对比"),
    };
    let select_rect = Rect::from_center_size(
        Pos2::new(lx + 58.0, toolbar.center().y),
        Vec2::new(112.0, 20.0),
    );
    lx += 122.0;
    let select_open = rt.diff_mode_open == Some(tab_id);
    ui.painter()
        .rect_filled(select_rect, egui::CornerRadius::same(4), p.panel2);
    ui.painter().rect_stroke(
        select_rect,
        egui::CornerRadius::same(4),
        Stroke::new(1.0, if select_open { p.accent } else { p.border_soft }),
        StrokeKind::Inside,
    );
    text_clipped(
        ui.painter(),
        select_rect.shrink2(Vec2::new(8.0, 0.0)),
        mode_label,
        FontId::proportional(11.0),
        p.text,
        Align2::LEFT_CENTER,
    );
    // 原生 select 的上下双箭头。
    let chev_x = select_rect.right() - 11.0;
    let chev_cy = select_rect.center().y;
    for (dy, up) in [(-3.0, true), (3.0, false)] {
        let c = Pos2::new(chev_x, chev_cy + dy);
        let (a, b, d) = if up {
            (
                Pos2::new(c.x - 3.0, c.y + 1.5),
                Pos2::new(c.x, c.y - 1.0),
                Pos2::new(c.x + 3.0, c.y + 1.5),
            )
        } else {
            (
                Pos2::new(c.x - 3.0, c.y - 1.5),
                Pos2::new(c.x, c.y + 1.0),
                Pos2::new(c.x + 3.0, c.y - 1.5),
            )
        };
        ui.painter().line_segment([a, b], Stroke::new(1.0, p.text3));
        ui.painter().line_segment([b, d], Stroke::new(1.0, p.text3));
    }
    if ui
        .interact(select_rect, ui.scope_id().with("diff-mode"), Sense::click())
        .clicked()
    {
        rt.diff_mode_open = if select_open { None } else { Some(tab_id) };
    }
    // 统计：git hunk 数（对齐稳定版 t().diff.stats(a, d, hunkOffsets.length)）。
    let total_hunks = hunks.as_ref().map_or(0, |h| h.len());
    let stats = if loading {
        crate::tr!(model, "Loading…", "加载中…").to_string()
    } else if zh {
        format!("{additions} 增 / {deletions} 删 · {total_hunks} 处差异")
    } else {
        format!("{additions} additions / {deletions} deletions · {total_hunks} differences")
    };
    ui.painter().text(
        Pos2::new(lx + 8.0, toolbar.center().y),
        Align2::LEFT_CENTER,
        stats,
        FontId::proportional(11.0),
        p.text3,
    );

    // 右组：比较标签（muted 11px）。
    let comparison = match &diff_source {
        DiffSource::Commit { commit, .. } => {
            let hash: String = commit.chars().take(8).collect();
            if zh {
                format!("提交 {hash}")
            } else {
                format!("Commit {hash}")
            }
        }
        DiffSource::CompareWithLocal { commit, .. } => {
            let hash: String = commit.chars().take(8).collect();
            if zh {
                format!("{hash} → 工作区")
            } else {
                format!("{hash} → working tree")
            }
        }
        DiffSource::CompareCommits { from, to, .. } => {
            let f: String = from.chars().take(8).collect();
            let to8: String = to.chars().take(8).collect();
            if zh {
                format!("提交 {f} ↔ {to8}")
            } else {
                format!("{f} ↔ {to8}")
            }
        }
        DiffSource::Files { .. } => crate::tr!(model, "File vs File", "两文件对比").to_string(),
        DiffSource::Stash { stash, .. } => {
            if zh {
                format!("{stash} 中的更改")
            } else {
                format!("Changes in {stash}")
            }
        }
        DiffSource::StashBeforeLocal { .. } => {
            if zh {
                "更改前 → 工作区".to_string()
            } else {
                "Before → working tree".to_string()
            }
        }
        DiffSource::Worktree { .. } => {
            crate::tr!(model, "Working tree vs index", "工作区 vs 索引").to_string()
        }
    };
    ui.painter().text(
        Pos2::new(toolbar.right() - 10.0, toolbar.center().y),
        Align2::RIGHT_CENTER,
        comparison,
        FontId::proportional(11.0),
        p.text3,
    );

    // 边缘气球（PyCharm Balloon：工具栏下方悬浮，1.8s 自动消失）。
    // 采用 Order::Tooltip 顶层绘制，绝对不透明背景 + 实心箭头 + 阴影，避免被底层代码穿透。
    if let Some((text, at)) = &rt.diff_balloon {
        if at.elapsed().as_secs_f32() < 1.8 {
            let painter = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                ui.scope_id().with("diff-balloon"),
            ));
            let w = text.chars().count() as f32 * 7.2 + 32.0;
            let b = Rect::from_min_size(
                Pos2::new(rect.left() + 10.0, toolbar.bottom() + 6.0),
                Vec2::new(w, 28.0),
            );
            let solid_bg = if theme.dark {
                Color32::from_rgb(0x2b, 0x2d, 0x30)
            } else {
                Color32::from_rgb(0xff, 0xff, 0xff)
            };
            let text_color = if theme.dark {
                Color32::from_rgb(0xdf, 0xe1, 0xe5)
            } else {
                Color32::from_rgb(0x1e, 0x1f, 0x22)
            };
            // 阴影
            let shape = egui::epaint::RectShape {
                rect: b.translate(egui::vec2(0.0, 3.0)).expand(4.0),
                corner_radius: egui::CornerRadius::same(6),
                fill: Color32::from_black_alpha(150),
                stroke: Stroke::NONE,
                stroke_kind: egui::StrokeKind::Inside,
                round_to_pixels: None,
                blur_width: 6.0,
                brush: None,
                angle: 0.0,
            };
            painter.add(egui::Shape::Rect(shape));
            painter.rect_filled(b, egui::CornerRadius::same(5), solid_bg);
            painter.rect_stroke(
                b,
                egui::CornerRadius::same(5),
                Stroke::new(1.0, p.jb_accent),
                StrokeKind::Inside,
            );
            painter.text(
                b.center(),
                Align2::CENTER_CENTER,
                text,
                FontId::proportional(12.5),
                text_color,
            );
            // 顶部实心小三角形
            let tip = Pos2::new(b.left() + 20.0, b.top() - 4.0);
            let arrow_poly = vec![
                tip,
                Pos2::new(tip.x - 4.5, b.top() + 0.5),
                Pos2::new(tip.x + 4.5, b.top() + 0.5),
            ];
            painter.add(egui::Shape::convex_polygon(
                arrow_poly,
                solid_bg,
                Stroke::NONE,
            ));
            painter.line_segment(
                [Pos2::new(tip.x - 4.5, b.top()), tip],
                Stroke::new(1.0, p.jb_accent),
            );
            painter.line_segment(
                [tip, Pos2::new(tip.x + 4.5, b.top())],
                Stroke::new(1.0, p.jb_accent),
            );
        }
    }

    // ----- pane 标题栏：hash（或 "diff"）+ 路径 + 状态 tag -----
    let hash_text = match &diff_source {
        DiffSource::Commit { commit, .. } => commit.chars().take(8).collect::<String>(),
        _ => "diff".to_string(),
    };
    let hash_w = hash_text.chars().count() as f32 * 6.6 + 8.0;
    ui.painter().text(
        Pos2::new(titlebar.left() + 8.0, titlebar.center().y),
        Align2::LEFT_CENTER,
        &hash_text,
        FontId::monospace(12.0),
        p.text3,
    );
    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(titlebar.left() + 8.0 + hash_w, titlebar.top()),
            Pos2::new(titlebar.right() - 60.0, titlebar.bottom()),
        ),
        &path,
        FontId::monospace(12.5),
        p.text_bright,
        Align2::LEFT_CENTER,
    );
    let tag_kind = status.filter(|kind| {
        matches!(
            kind,
            lazecode_git::ChangeKind::Added | lazecode_git::ChangeKind::Deleted
        )
    });
    if let Some(kind) = tag_kind {
        // 稳定版只渲染 Added/Deleted 标签（git-tag git-staged / git-del）。
        let label = kind.label();
        let tag_w = label.chars().count() as f32 * 10.0 + 12.0;
        let tag = Rect::from_center_size(
            Pos2::new(titlebar.right() - 10.0 - tag_w * 0.5, titlebar.center().y),
            Vec2::new(tag_w, 15.0),
        );
        let (tag_color, tag_bg, tag_border) = match kind {
            lazecode_git::ChangeKind::Added | lazecode_git::ChangeKind::Renamed => {
                (p.green, p.diff_add, p.diff_add_strong)
            }
            lazecode_git::ChangeKind::Deleted => (p.rose, p.diff_del, p.diff_del_strong),
            _ => (p.text3, p.hover, p.border_soft),
        };
        ui.painter()
            .rect_filled(tag, egui::CornerRadius::same(3), tag_bg);
        ui.painter().rect_stroke(
            tag,
            egui::CornerRadius::same(3),
            Stroke::new(1.0, tag_border),
            StrokeKind::Inside,
        );
        ui.painter().text(
            tag.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(10.0),
            tag_color,
        );
    }

    // ----- 导航（点击 + F7）：组游标、边缘气球、二次点击跨文件 -----
    if nav_request != 0 && groups > 0 {
        let next = current_group as isize + nav_request as isize;
        if (0..groups as isize).contains(&next) {
            let next = next as usize;
            diff_mut(model, index, tab_id, |d| {
                d.current_hunk = next;
                d.edge_pending = 0;
            });
            rt.diff_scroll = Some((tab_id, next));
        } else if edge_pending == 0 {
            let msg = if has_nav {
                if nav_request > 0 {
                    crate::tr!(
                        model,
                        "Already at the last difference — click again to open the next file",
                        "已是最后一处差异，再次点击查看下一个文件的差异"
                    )
                } else {
                    crate::tr!(
                        model,
                        "Already at the first difference — click again to open the previous file",
                        "已是第一处差异，再次点击查看上一个文件的差异"
                    )
                }
            } else if nav_request > 0 {
                crate::tr!(model, "Already at the last difference", "已是最后一处差异")
            } else {
                crate::tr!(model, "Already at the first difference", "已是第一处差异")
            };
            rt.diff_balloon = Some((msg.to_string(), std::time::Instant::now()));
            let sign = nav_request.signum() as i8;
            diff_mut(model, index, tab_id, |d| d.edge_pending = sign);
        } else if has_nav && nav_index.is_some() {
            let target_i = if nav_request > 0 {
                nav_index.unwrap_or(0) + 1
            } else {
                nav_index.unwrap_or(0).wrapping_sub(1)
            };
            if let Some(target) = nav_files.get(target_i) {
                nav_target = Some(target.clone());
            }
        }
    }

    if let Some(target) = nav_target.clone() {
        if let Some(project) = model.projects.get_mut(index) {
            if let Some(session) = project.compare_session.as_mut() {
                session.selected = Some(target.clone());
                let parts: Vec<&str> = target.split('/').collect();
                for i in 1..parts.len() {
                    let parent = parts[..i].join("/");
                    session.collapsed.remove(&parent);
                }
                session.root_expanded = true;
            }
        }
        let cmd = match &diff_source {
            DiffSource::CompareCommits { from, to, .. } => Some(Command::OpenCompareCommitsDiff {
                from: from.clone(),
                to: to.clone(),
                path: target,
            }),
            DiffSource::Commit { commit, .. } => Some(Command::OpenCommitDiff {
                commit: commit.clone(),
                path: target,
            }),
            DiffSource::Worktree { .. } => Some(Command::OpenDiff(target)),
            _ => None,
        };
        if let Some(cmd) = cmd {
            model.queue(cmd);
            diff_mut(model, index, tab_id, |d| {
                d.edge_pending = 0;
                d.revealed = true;
            });
            rt.diff_balloon = None;
        }
    }

    // 首次显示：定位第一处差异（stable revealedRef 语义）。
    if !revealed && !rows.is_empty() {
        rt.diff_scroll = Some((tab_id, 0));
        diff_mut(model, index, tab_id, |d| d.revealed = true);
    }

    let body = Rect::from_min_max(Pos2::new(rect.left(), titlebar.bottom()), rect.max);
    if loading {
        ui.painter().text(
            body.center(),
            Align2::CENTER_CENTER,
            crate::tr!(model, "Loading diff…", "正在读取差异…"),
            FontId::proportional(11.5),
            p.text3,
        );
        return;
    }
    if let Some(error) = error {
        ui.painter().text(
            Pos2::new(body.left() + 20.0, body.top() + 30.0),
            Align2::LEFT_CENTER,
            error,
            FontId::proportional(11.0),
            p.rose,
        );
        return;
    }
    if binary {
        ui.painter().text(
            body.center(),
            Align2::CENTER_CENTER,
            crate::tr!(
                model,
                "Cannot show diff for an unknown file type",
                "无法显示未知文件类型的差异"
            ),
            FontId::proportional(11.5),
            p.text3,
        );
        return;
    }
    if hunks.is_none() || rows.is_empty() {
        let empty = match &diff_source {
            DiffSource::Worktree { .. } => {
                crate::tr!(
                    model,
                    "This file has no uncommitted changes",
                    "该文件没有未提交的改动"
                )
            }
            _ => crate::tr!(
                model,
                "This commit has no text difference",
                "该提交没有文本差异"
            ),
        };
        ui.painter().text(
            body.center(),
            Align2::CENTER_CENTER,
            empty,
            FontId::proportional(11.5),
            p.text3,
        );
        return;
    }

    // 视图模式下拉（两行小菜单）。
    if select_open {
        let menu_w = 150.0;
        let menu_rect = Rect::from_min_size(
            Pos2::new(select_rect.left(), toolbar.bottom() + 2.0),
            Vec2::new(menu_w, 48.0),
        );
        // Register for the shell's wheel interception (next frame).
        rt.menu_hit_rects.push(menu_rect);
        ui.painter()
            .rect_filled(menu_rect, egui::CornerRadius::same(6), p.menu_bg);
        ui.painter().rect_stroke(
            menu_rect,
            egui::CornerRadius::same(6),
            Stroke::new(1.0, p.menu_border),
            StrokeKind::Inside,
        );
        for (row, mode, label) in [
            (
                0usize,
                DiffViewMode::Unified,
                crate::tr!(model, "Unified view", "统一视图"),
            ),
            (
                1,
                DiffViewMode::SideBySide,
                crate::tr!(model, "Side by Side", "并排对比"),
            ),
        ] {
            let row_rect = Rect::from_min_size(
                Pos2::new(
                    menu_rect.left() + 4.0,
                    menu_rect.top() + 4.0 + row as f32 * 20.0,
                ),
                Vec2::new(menu_w - 8.0, 20.0),
            );
            let hovered =
                row_rect.contains(ui.input(|i| i.pointer.latest_pos().unwrap_or_default()));
            if hovered {
                ui.painter()
                    .rect_filled(row_rect, egui::CornerRadius::same(4), p.menu_item_hover);
            }
            ui.painter().text(
                Pos2::new(row_rect.left() + 8.0, row_rect.center().y),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(11.0),
                if view_mode == mode {
                    p.text_bright
                } else {
                    p.text2
                },
            );
            if view_mode == mode {
                paint_icon(
                    ui.painter(),
                    Rect::from_center_size(
                        Pos2::new(row_rect.right() - 12.0, row_rect.center().y),
                        Vec2::splat(12.0),
                    ),
                    Icon::Check,
                    p.text_bright,
                );
            }
            if ui
                .interact(
                    row_rect,
                    ui.scope_id().with(("diff-mode-item", row)),
                    Sense::click(),
                )
                .clicked()
            {
                if view_mode != mode {
                    if let Some(tab) = model.projects[index].tab_mut_by_id(tab_id) {
                        if let TabKind::Diff(d) = &mut tab.kind {
                            d.view_mode = mode;
                        }
                    }
                }
                rt.diff_mode_open = None;
            }
        }
        if ui
            .interact(
                rect.expand(40.0),
                ui.scope_id().with("diff-mode-dismiss"),
                Sense::click(),
            )
            .clicked()
        {
            rt.diff_mode_open = None;
        }
    }

    let ctx = crate::diff_view::DiffBodyCtx {
        tab_id,
        rows: &rows,
        groups,
        current_group: current_group.min(groups.saturating_sub(1)),
        scroll_to_group: rt.diff_scroll,
        can_rollback: worktree_mode,
    };
    let rollback_group = match view_mode {
        DiffViewMode::Unified => crate::diff_view::draw_unified(ui, theme, body, ctx),
        DiffViewMode::SideBySide => crate::diff_view::draw_split(ui, theme, body, ctx),
    };
    if let Some(gi) = rollback_group {
        if let Some(hunks) = &hunks {
            if let Some(hi) = crate::diff_view::hunk_of_group(hunks, gi) {
                let patch = lazecode_text::unified_patch(
                    &path,
                    &lazecode_text::TextDiff {
                        hunks: vec![hunks[hi].clone()],
                        ..Default::default()
                    },
                );
                model.queue(Command::ApplyPatch {
                    patch,
                    cached: false,
                    reverse: true,
                    root: model.projects[index].active_root().to_path_buf(),
                });
            }
        }
    }
    // 滚动请求为单帧语义。
    rt.diff_scroll = None;
}

fn diff_mut(model: &mut AppModel, index: usize, tab_id: u64, f: impl FnOnce(&mut DiffTab)) {
    if let Some(tab) = model.projects[index].tab_mut_by_id(tab_id) {
        if let TabKind::Diff(diff) = &mut tab.kind {
            f(diff);
        }
    }
}

fn draw_find(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    project: &mut ProjectState,
    tab_id: u64,
    rt: &mut UiRuntime,
    lang: lazecode_core::Language,
) {
    let p = theme.palette;
    let Some(tab) = project.tab_mut_by_id(tab_id) else {
        return;
    };
    let TabKind::File(file_tab) = &mut tab.kind else {
        return;
    };
    let doc = &mut file_tab.doc;
    let panel = Rect::from_min_size(
        Pos2::new(rect.right() - 446.0, rect.top() + 8.0),
        Vec2::new(434.0, 106.0),
    );
    ui.painter()
        .rect_filled(panel, egui::CornerRadius::same(8), p.elev);
    ui.painter().rect_stroke(
        panel,
        egui::CornerRadius::same(8),
        Stroke::new(1.0, p.border_strong),
        StrokeKind::Inside,
    );

    // Row 1: Find input + count + close button
    let input_rect = Rect::from_min_size(
        Pos2::new(panel.left() + 10.0, panel.top() + 8.0),
        Vec2::new(panel.width() - 44.0, 24.0),
    );
    ui.painter()
        .rect_filled(input_rect, egui::CornerRadius::same(5), p.panel2);
    let query_rect = Rect::from_min_max(
        Pos2::new(input_rect.left() + 8.0, input_rect.top() + 2.0),
        Pos2::new(input_rect.right() - 70.0, input_rect.bottom() - 2.0),
    );
    let response = ui.put(
        query_rect,
        TextEdit::singleline(&mut rt.find.query)
            .frame(egui::Frame::NONE)
            .align(egui::Align2::LEFT_CENTER)
            .hint_text(crate::i18n::t(lang, "Find", "查找"))
            .font(FontId::proportional(12.5)),
    );
    if rt.find.focus_requested {
        response.request_focus();
        rt.find.focus_requested = false;
    }
    let signature = format!(
        "{}|{}|{}|{}|{}",
        rt.find.query, rt.find.case_sensitive, rt.find.regex, rt.find.whole_word, doc.revision
    );
    if signature != rt.find.last_signature {
        rt.find.last_signature = signature;
        rt.find.matches = doc.find_all(
            &rt.find.query,
            MatchOptions {
                case_sensitive: rt.find.case_sensitive,
                regex: rt.find.regex,
                whole_word: rt.find.whole_word,
            },
        );
        rt.find.index = 0;
    }
    let count = rt.find.matches.len();
    ui.painter().text(
        Pos2::new(input_rect.right() - 60.0, input_rect.center().y),
        Align2::LEFT_CENTER,
        if count > 0 {
            format!("{} / {}", (rt.find.index % count) + 1, count)
        } else if rt.find.query.is_empty() {
            String::new()
        } else {
            "0".to_string()
        },
        FontId::monospace(11.5),
        p.text3,
    );
    let close = Rect::from_center_size(
        Pos2::new(panel.right() - 18.0, panel.top() + 20.0),
        Vec2::splat(18.0),
    );
    let close_response = ui.interact(close, ui.scope_id().with("find-close"), Sense::click());
    paint_icon(ui.painter(), close.shrink(3.0), Icon::X, p.text3);
    if close_response.clicked() {
        rt.find.open = false;
    }

    // Row 2: Replace input + [Replace] + [Replace All]
    let replace_rect = Rect::from_min_size(
        Pos2::new(panel.left() + 10.0, panel.top() + 38.0),
        Vec2::new(panel.width() - 176.0, 24.0),
    );
    ui.painter()
        .rect_filled(replace_rect, egui::CornerRadius::same(5), p.panel2);
    let field = Rect::from_min_max(
        Pos2::new(replace_rect.left() + 8.0, replace_rect.top() + 2.0),
        Pos2::new(replace_rect.right() - 8.0, replace_rect.bottom() - 2.0),
    );
    ui.put(
        field,
        TextEdit::singleline(&mut rt.find.replacement)
            .frame(egui::Frame::NONE)
            .align(egui::Align2::LEFT_CENTER)
            .hint_text(crate::i18n::t(lang, "Replace", "替换为"))
            .font(FontId::proportional(12.5)),
    );

    let replace_btn_rect = Rect::from_min_size(
        Pos2::new(replace_rect.right() + 6.0, panel.top() + 38.0),
        Vec2::new(72.0, 24.0),
    );
    if secondary_button(
        ui,
        replace_btn_rect,
        crate::i18n::t(lang, "Replace", "替换"),
        None,
        theme,
    )
    .clicked()
        && count > 0
    {
        if let Some((start, end)) = rt.find.matches.get(rt.find.index).copied() {
            doc.replace_match(start, end, &rt.find.replacement);
            rt.find.last_signature.clear();
        }
    }

    let replace_all_rect = Rect::from_min_size(
        Pos2::new(replace_btn_rect.right() + 6.0, panel.top() + 38.0),
        Vec2::new(82.0, 24.0),
    );
    if secondary_button(
        ui,
        replace_all_rect,
        crate::i18n::t(lang, "Replace all", "全部替换"),
        None,
        theme,
    )
    .clicked()
    {
        doc.replace_all(
            &rt.find.query,
            &rt.find.replacement,
            MatchOptions {
                case_sensitive: rt.find.case_sensitive,
                regex: rt.find.regex,
                whole_word: rt.find.whole_word,
            },
        );
        rt.find.last_signature.clear();
    }

    // Row 3: Toggles (Aa, ab, .*) + Prev (↑) / Next (↓)
    let mut x = input_rect.left();
    for (label, active, id) in [
        ("Aa", rt.find.case_sensitive, "case"),
        ("ab", rt.find.whole_word, "word"),
        (".*", rt.find.regex, "regex"),
    ] {
        let toggle = Rect::from_min_size(Pos2::new(x, panel.top() + 72.0), Vec2::new(28.0, 22.0));
        x += 32.0;
        let response = ui.interact(
            toggle,
            ui.scope_id().with(("find-toggle", id)),
            Sense::click(),
        );
        ui.painter().rect_filled(
            toggle,
            egui::CornerRadius::same(4),
            if active {
                p.accent_soft
            } else {
                Color32::TRANSPARENT
            },
        );
        ui.painter().text(
            toggle.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::monospace(11.5),
            if active { p.accent } else { p.text3 },
        );
        if response.clicked() {
            match id {
                "case" => rt.find.case_sensitive = !rt.find.case_sensitive,
                "word" => rt.find.whole_word = !rt.find.whole_word,
                _ => rt.find.regex = !rt.find.regex,
            }
            rt.find.last_signature.clear();
        }
    }
    let nav_rect = Rect::from_min_size(
        Pos2::new(panel.left() + 116.0, panel.top() + 72.0),
        Vec2::new(28.0, 22.0),
    );
    if secondary_button(ui, nav_rect, "↑", None, theme).clicked() && count > 0 {
        rt.find.index = (rt.find.index + count - 1) % count;
        jump_to_match(doc, rt);
    }
    let nav_rect2 = Rect::from_min_size(
        Pos2::new(nav_rect.right() + 4.0, panel.top() + 72.0),
        Vec2::new(28.0, 22.0),
    );
    if secondary_button(ui, nav_rect2, "↓", None, theme).clicked() && count > 0 {
        rt.find.index = (rt.find.index + 1) % count;
        jump_to_match(doc, rt);
    }
    let _ = (rect, tab_id);
}

fn jump_to_match(doc: &mut Document, rt: &mut UiRuntime) {
    if let Some((start, end)) = rt.find.matches.get(rt.find.index).copied() {
        doc.selection = Some(Selection {
            anchor: start,
            head: end,
        });
        doc.cursor = end;
        if start.line < doc.scroll_line as usize || start.line > doc.scroll_line as usize + 30 {
            doc.scroll_line = (start.line.saturating_sub(6)) as f32;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn welcome_action_button(
    ui: &mut Ui,
    rect: Rect,
    id: &'static str,
    label: &str,
    icon: Icon,
    primary: bool,
    theme: &Theme,
) -> egui::Response {
    let p = theme.palette;
    let response = ui.interact(
        rect,
        ui.scope_id().with(("welcome-action", id)),
        Sense::click(),
    );
    let hovered = response.hovered();
    let (fill, text_color) = if primary {
        (
            if hovered {
                p.accent.gamma_multiply(1.06)
            } else {
                p.accent
            },
            p.accent_on_fill,
        )
    } else {
        (
            if hovered {
                p.hover
            } else {
                Color32::TRANSPARENT
            },
            p.text2,
        )
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(6), fill);
    if !primary {
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(6),
            Stroke::new(1.0, p.border),
            StrokeKind::Inside,
        );
    }
    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(rect.left() + 16.0, rect.center().y),
            Vec2::splat(14.0),
        ),
        icon,
        if primary { text_color } else { p.text2 },
    );
    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(rect.left() + 34.0, rect.top()),
            Pos2::new(rect.right() - 8.0, rect.bottom()),
        ),
        label,
        FontId::proportional(12.5),
        text_color,
        Align2::LEFT_CENTER,
    );
    response
}

fn welcome_kbd(painter: &egui::Painter, rect: Rect, label: &str, theme: &Theme) {
    let p = theme.palette;
    painter.rect_filled(rect, egui::CornerRadius::same(4), p.panel2);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(4),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    text_clipped(
        painter,
        rect.shrink2(Vec2::new(4.0, 0.0)),
        label,
        FontId::proportional(13.0),
        p.text,
        Align2::CENTER_CENTER,
    );
}

fn welcome_recent_card(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    title: &str,
    empty: &str,
    items: &[(PathBuf, bool)],
) -> Option<(PathBuf, bool)> {
    let p = theme.palette;
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(8), p.panel);
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, rect.top() + 18.0),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(13.0),
        p.text,
    );
    if items.is_empty() {
        ui.painter().text(
            Pos2::new(rect.left() + 14.0, rect.top() + 44.0),
            Align2::LEFT_CENTER,
            empty,
            FontId::proportional(12.5),
            p.text3,
        );
        return None;
    }
    let mut clicked = None;
    for (index, (path, is_project)) in items.iter().take(5).enumerate() {
        let row_y = rect.top() + 32.0 + index as f32 * 27.0;
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, row_y),
            Vec2::new(rect.width() - 16.0, 24.0),
        );
        let response = ui.interact(
            row,
            ui.scope_id()
                .with(("welcome-recent", path.clone(), *is_project)),
            Sense::click(),
        );
        if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(4), p.hover);
        }
        let (icon, tone) = if *is_project {
            (Icon::Folder, p.accent)
        } else {
            tab_icon(path.to_str().unwrap_or_default(), theme)
        };
        paint_icon(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(row.left() + 10.0, row.center().y),
                Vec2::splat(12.0),
            ),
            icon,
            tone,
        );
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(row.left() + 22.0, row.top()),
                Pos2::new(row.right() - 8.0, row.bottom()),
            ),
            &name,
            FontId::proportional(13.0),
            p.text,
            Align2::LEFT_CENTER,
        );
        let response = response.on_hover_text(path.display().to_string());
        if response.clicked() {
            clicked = Some((path.clone(), *is_project));
        }
    }
    clicked
}

fn draw_welcome(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    index: Option<usize>,
) {
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.editor);
    let card = Rect::from_center_size(
        rect.center(),
        Vec2::new(
            720.0_f32.min(rect.width() - 60.0),
            460.0_f32.min(rect.height() - 60.0),
        ),
    );
    let left = card.left() + 34.0;
    let mut y = card.top() + 40.0;

    // Use the packaged application icon rather than a synthetic `>_` tile.
    let mark = Rect::from_min_size(Pos2::new(left, y), Vec2::splat(84.0));
    let icon_texture = rt.app_icon_texture(ui.ctx());
    ui.painter().image(
        icon_texture,
        mark,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
    y += 96.0;
    let name = index
        .and_then(|index| model.projects.get(index))
        .map(|project| project.name.clone())
        .unwrap_or_else(|| "LazeCode".to_string());
    ui.painter().text(
        Pos2::new(left, y),
        Align2::LEFT_CENTER,
        &name,
        FontId::proportional(24.0),
        p.text,
    );
    y += 28.0;
    let subtitle = if index.is_some() {
        if model.settings.language.is_zh() {
            format!("{} 个项目已打开。所有进程一目了然。", model.projects.len())
        } else {
            format!(
                "{} projects open. Every process in view.",
                model.projects.len()
            )
        }
    } else {
        crate::tr!(
            model,
            "A quiet place for many projects. Open a folder or files to begin.",
            "一个安静的工作空间。打开目录或文件即可开始。"
        )
        .to_string()
    };
    ui.painter().text(
        Pos2::new(left, y),
        Align2::LEFT_CENTER,
        subtitle,
        FontId::proportional(13.5),
        p.text2,
    );
    y += 30.0;

    let actions = [
        (
            crate::tr!(model, "Open", "打开"),
            Icon::FolderOpen,
            "open-project",
        ),
        (
            crate::tr!(model, "Clone repo", "克隆仓库"),
            Icon::Worktree,
            "clone-repo",
        ),
        (
            crate::tr!(model, "New file", "新建文件"),
            Icon::Plus,
            "new-file",
        ),
        (
            crate::tr!(model, "New console", "新建控制台"),
            Icon::Terminal,
            "new-console",
        ),
        (
            crate::tr!(model, "Find in files", "在文件中查找"),
            Icon::Search,
            "search",
        ),
    ];
    let mut x = left;
    // Keep the five actions on one row on narrow windows as well.
    let action_width = ((card.width() - 68.0 - 32.0) / 5.0).clamp(96.0, 124.0);
    for (label, icon, id) in actions {
        let width = action_width;
        let button = Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, 30.0));
        let primary = id == "open-project";
        let response = welcome_action_button(ui, button, id, label, icon, primary, theme);
        if response.clicked() {
            match id {
                "open-project" => model.queue(Command::OpenProjectDialog),
                "clone-repo" => {
                    rt.dialog = crate::runtime::DialogState::CloneRepo {
                        url: String::new(),
                        dest: String::new(),
                        error: None,
                    };
                }
                "new-file" => {
                    let dir = model
                        .active_project()
                        .map(|p| p.active_root().to_path_buf())
                        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")));
                    rt.dialog = crate::runtime::DialogState::NewFile {
                        dir,
                        value: String::new(),
                        error: None,
                    };
                }
                "new-console" => model.queue(Command::NewTerminal),
                _ => {
                    rt.search.open = true;
                    rt.search.focus_requested = true;
                }
            }
        }
        x += width + 8.0;
    }
    y += 52.0;

    // Recent / open files and shortcuts, two columns when there is room.
    let column_w = (card.width() - 88.0) * 0.56;
    let recent_rect = Rect::from_min_size(Pos2::new(left, y), Vec2::new(column_w, 168.0));
    ui.painter()
        .rect_filled(recent_rect, egui::CornerRadius::same(8), p.panel);
    let home_groups = index.is_none();
    let left_title = if home_groups {
        crate::tr!(model, "Recent projects", "最近项目")
    } else {
        crate::tr!(model, "Open files", "打开的文件")
    };
    let left_empty = if home_groups {
        crate::tr!(model, "No recent projects", "暂无最近项目")
    } else {
        crate::tr!(
            model,
            "Click a file in the explorer",
            "在资源管理器中点击文件"
        )
    };
    let left_items: Vec<(PathBuf, bool)> = if let Some(index) = index {
        model.projects[index]
            .active_worktree_tabs()
            .filter_map(|tab| match &tab.kind {
                TabKind::File(file) => Some((file.doc.path.clone(), false)),
                TabKind::Image(img) => Some((img.path.clone(), false)),
                _ => None,
            })
            .take(5)
            .collect()
    } else {
        model
            .recent
            .iter()
            .take(5)
            .cloned()
            .map(|path| (path, true))
            .collect()
    };
    if let Some((path, is_project)) =
        welcome_recent_card(ui, theme, recent_rect, left_title, left_empty, &left_items)
    {
        if is_project {
            model.queue(Command::OpenProject(path));
        } else if let Some(active) = model.active_project {
            let _ = model.open_file_in(active, path);
        } else if let Err(err) = model.open_files(vec![path]) {
            model.toast(err, lazecode_core::ToastKind::Error);
        }
    }

    let right_rect = Rect::from_min_size(
        Pos2::new(recent_rect.right() + 16.0, y),
        Vec2::new(card.right() - 34.0 - (recent_rect.right() + 16.0), 168.0),
    );
    if home_groups {
        let recent_files: Vec<(PathBuf, bool)> = model
            .recent_files
            .iter()
            .take(5)
            .cloned()
            .map(|path| (path, false))
            .collect();
        if let Some((path, _)) = welcome_recent_card(
            ui,
            theme,
            right_rect,
            crate::tr!(model, "Recent files", "最近文件"),
            crate::tr!(model, "No recent files", "暂无最近文件"),
            &recent_files,
        ) {
            if let Err(err) = model.open_files(vec![path]) {
                model.toast(err, lazecode_core::ToastKind::Error);
            }
        }
    }

    let shortcuts = right_rect;
    if !home_groups {
        ui.painter().text(
            Pos2::new(shortcuts.left() + 14.0, shortcuts.top() + 18.0),
            Align2::LEFT_CENTER,
            crate::tr!(model, "Keyboard", "快捷键"),
            FontId::proportional(13.0),
            p.text,
        );
        let entries = [
            (crate::tr!(model, "Command palette", "命令面板"), "⌘⇧A"),
            (crate::tr!(model, "Toggle explorer", "切换资源管理器"), "⌘1"),
            (crate::tr!(model, "Commit", "提交"), "⌘K"),
            (crate::tr!(model, "Open Git log", "打开 Git 日志"), "⌘9"),
            (crate::tr!(model, "Find in files", "在文件中查找"), "⌘⇧F"),
        ];
        let key_font = FontId::proportional(13.0);
        let mut row_y = shortcuts.top() + 36.0;
        for (label, key) in entries.iter() {
            let key_width = text_width(ui.painter(), key, key_font.clone()) + 16.0;
            let kbd_rect = Rect::from_center_size(
                Pos2::new(shortcuts.right() - 14.0 - key_width * 0.5, row_y),
                Vec2::new(key_width, 22.0),
            );
            text_clipped(
                ui.painter(),
                Rect::from_min_max(
                    Pos2::new(shortcuts.left() + 14.0, row_y - 10.0),
                    Pos2::new(kbd_rect.left() - 8.0, row_y + 10.0),
                ),
                label,
                FontId::proportional(12.5),
                p.text2,
                Align2::LEFT_CENTER,
            );
            welcome_kbd(ui.painter(), kbd_rect, key, theme);
            row_y += 27.0;
        }
    }

    let foot_y = card.bottom() - 20.0;
    ui.painter().text(
        Pos2::new(left, foot_y),
        Align2::LEFT_CENTER,
        if model.settings.language.is_zh() {
            format!(
                "{} 个运行中 · {} 个项目",
                rt.busy_count(),
                model.projects.len()
            )
        } else {
            format!(
                "{} running across {} projects",
                rt.busy_count(),
                model.projects.len()
            )
        },
        FontId::proportional(12.0),
        p.text3,
    );
    if let Some(index) = index {
        ui.painter().text(
            Pos2::new(card.right() - 34.0, foot_y),
            Align2::RIGHT_CENTER,
            format!(
                "{} · {}",
                model.projects[index].name,
                model.projects[index].branch()
            ),
            FontId::proportional(10.0),
            p.text3,
        );
    }
    let _ = rt;
}
