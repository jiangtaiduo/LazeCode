mod body;
mod chrome;
mod diff;
mod find;
mod input;
mod prelude;
mod welcome;

use body::*;
use chrome::*;
use diff::*;
use find::*;
use input::*;
use prelude::*;
use welcome::*;

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
