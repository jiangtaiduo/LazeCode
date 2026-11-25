//! Editor chrome: tab bar, breadcrumbs, markdown toolbar, file icons.

use super::*;

pub(super) fn tab_icon(path: &str, theme: &Theme) -> (Icon, Color32) {
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

pub(super) fn draw_tabbar(
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
    crate::app_ui::region(ui, rect, "editor-tabbar-scroll-region", |ui| {
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

pub(super) fn draw_crumbs(
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

pub(super) fn token_color(kind: TokenKind, theme: &Theme) -> Color32 {
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
pub(super) fn draw_markdown_toolbar(
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
