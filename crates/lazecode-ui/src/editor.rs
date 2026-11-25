mod body;
mod diff;
mod find;
mod input;
mod prelude;

use body::*;
use diff::*;
use find::*;
use input::*;
use prelude::*;

pub(crate) use input::word_at;

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
