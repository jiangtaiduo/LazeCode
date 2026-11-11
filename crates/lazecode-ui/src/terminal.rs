use std::collections::HashMap;

use egui::{
    Align2, Color32, Event, FontId, ImeEvent, Key, Modifiers, Pos2, Rect, Sense, Stroke,
    StrokeKind, TextEdit, Ui, Vec2,
};
use lazecode_terminal::{
    ColorSpec, ImagePlacement, KeyEvent, Session, Snapshot, TerminalKey, ATTR_BOLD, ATTR_DIM,
    ATTR_HIDDEN, ATTR_INVERSE, ATTR_LEADING_SPACER, ATTR_SPACER, ATTR_STRIKEOUT, ATTR_UNDERLINE,
    ATTR_WIDE,
};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::{TermSearchState, TermSelection};
use crate::theme::Theme;
use crate::widgets::mono_char_width;

const PAD_X: f32 = 8.0;
const PAD_Y: f32 = 6.0;
const SCROLLBAR: f32 = 8.0;

pub struct TerminalOutput {
    pub focus_requested: bool,
}

/// Per-pane cache of egui textures for kitty graphics images.
///
/// Pixels are uploaded once per generation and dropped when the terminal
/// evicts or deletes an image, so the cache mirrors the terminal's own byte
/// cap instead of growing on its own.
#[derive(Default)]
pub struct GraphicsCache {
    images: HashMap<u32, CachedImage>,
    /// A full re-upload was already requested for the current empty state.
    resynced: bool,
}

struct CachedImage {
    generation: u64,
    texture: egui::TextureHandle,
}

impl GraphicsCache {
    /// Upload decoded images and prune textures the terminal no longer holds.
    pub fn sync(&mut self, ctx: &egui::Context, session: &Session, snapshot: &Snapshot) {
        if self.images.is_empty() && !snapshot.graphics_images.is_empty() && !self.resynced {
            // The frontend lost its textures (a fresh context, a cleared cache)
            // while the terminal still holds pixels: ask for a full re-upload
            // once, then accept the terminal's answer.
            self.resynced = true;
            session.resync_graphics();
        }
        let images = session.take_graphics_updates();
        if !images.is_empty() {
            self.resynced = false;
        }
        self.apply(ctx, images, &snapshot.graphics_images);
    }

    /// Upload `images` and drop every texture not in `live_ids`.
    fn apply(
        &mut self,
        ctx: &egui::Context,
        images: Vec<lazecode_terminal::KittyImage>,
        live_ids: &[u32],
    ) {
        let max_side = ctx.input(|input| input.max_texture_side);
        for image in images {
            if image.width == 0
                || image.height == 0
                || image.width > max_side
                || image.height > max_side
            {
                continue;
            }
            if self
                .images
                .get(&image.id)
                .is_some_and(|cached| cached.generation == image.generation)
            {
                continue;
            }
            let pixels =
                egui::ColorImage::from_rgba_unmultiplied([image.width, image.height], &image.rgba);
            let texture = ctx.load_texture(
                format!("kitty-image-{}", image.id),
                pixels,
                egui::TextureOptions::LINEAR,
            );
            self.images.insert(
                image.id,
                CachedImage {
                    generation: image.generation,
                    texture,
                },
            );
        }
        self.images.retain(|id, _| live_ids.contains(id));
    }

    /// Paint the placements that intersect the visible viewport.
    fn paint(
        &self,
        painter: &egui::Painter,
        snapshot: &Snapshot,
        inner: Rect,
        char_w: f32,
        line_h: f32,
        rows: usize,
    ) {
        if snapshot.graphics.is_empty() {
            return;
        }
        for placement in &snapshot.graphics {
            let Some(cached) = self.images.get(&placement.image_id) else {
                continue;
            };
            let row = placement.row as f32 + snapshot.display_offset as f32;
            if row + placement.rows as f32 <= 0.0 || row >= rows as f32 {
                continue;
            }
            let rect = Rect::from_min_size(
                Pos2::new(
                    inner.left() + placement.col as f32 * char_w,
                    inner.top() + row * line_h,
                ),
                Vec2::new(
                    placement.columns as f32 * char_w,
                    placement.rows as f32 * line_h,
                ),
            );
            let size = cached.texture.size_vec2();
            let uv = source_uv(placement, size);
            painter.image(cached.texture.id(), rect, uv, Color32::WHITE);
        }
    }
}

/// Normalise a kitty source rectangle (`x`,`y`,`w`,`h`; 0 = to the edge) into
/// the texture's UV space.
fn source_uv(placement: &ImagePlacement, size: Vec2) -> Rect {
    let (source_x, source_y, source_w, source_h) = placement.source;
    let width = size.x.max(1.0);
    let height = size.y.max(1.0);
    let x = (source_x as f32).min(width);
    let y = (source_y as f32).min(height);
    let crop_w = if source_w > 0 {
        (source_w as f32).min(width - x)
    } else {
        width - x
    };
    let crop_h = if source_h > 0 {
        (source_h as f32).min(height - y)
    } else {
        height - y
    };
    if crop_w <= 0.0 || crop_h <= 0.0 {
        return Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    }
    Rect::from_min_max(
        Pos2::new(x / width, y / height),
        Pos2::new((x + crop_w) / width, (y + crop_h) / height),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    ui: &mut Ui,
    session: &Session,
    snapshot: &mut Snapshot,
    selection: &mut Option<TermSelection>,
    search: &mut TermSearchState,
    focus: bool,
    preedit: &mut String,
    wheel_accum: &mut f32,
    graphics: &mut GraphicsCache,
    refresh: bool,
    theme: &Theme,
    rect: Rect,
    session_id: u64,
    font_size: f32,
) -> TerminalOutput {
    let p = theme.palette;
    if focus {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
    } else {
        preedit.clear();
    }
    let refresh = refresh || snapshot.rows == 0;
    if refresh {
        session.snapshot_into(snapshot);
    }
    // A session keeps producing output; only request a capped 60 fps while
    // output is actually recent. `last_output_ms` is an age, not a timestamp.
    if focus && session.last_output_ms().is_some_and(|age| age < 120) {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(16));
    }
    let line_h = (font_size * 1.36).round().max(12.0);
    let char_w = mono_char_width(ui.ctx(), font_size).max(5.0);
    let inner = Rect::from_min_max(
        Pos2::new(rect.left() + PAD_X, rect.top() + PAD_Y),
        Pos2::new(rect.right() - SCROLLBAR, rect.bottom() - PAD_Y),
    );
    let cols = ((inner.width() / char_w).floor() as usize).max(2);
    let rows = ((inner.height() / line_h).floor() as usize).max(2);
    if cols != snapshot.cols || rows != snapshot.rows {
        session.resize(cols as u16, rows as u16);
        // Absolute selection endpoints cannot survive reflow.
        *selection = None;
        *wheel_accum = 0.0;
        session.snapshot_into(snapshot);
    }
    let cols = snapshot.cols.max(1);
    let rows = snapshot.rows.max(1);
    // Kitty graphics needs the cell pixel size to size a placement.
    session.set_cell_size(char_w, line_h);

    let mut focus_requested = false;
    let input_rect = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top()),
        Pos2::new(rect.right() - SCROLLBAR, rect.bottom()),
    );
    let response = ui.interact(
        input_rect,
        ui.scope_id().with(("terminal", session_id)),
        Sense::click_and_drag(),
    );
    if response.clicked() || response.dragged() {
        focus_requested = true;
    }
    // Screen readers see the visible viewport text, cursor and selection. The
    // mirror is bounded to the viewport, so no history is cloned per frame.
    if ui.ctx().options(|options| options.screen_reader) {
        let mirror = a11y_mirror(snapshot, selection.as_ref(), focus);
        response.widget_info(|| egui::WidgetInfo {
            role: egui::accesskit::Role::Terminal,
            enabled: true,
            label: Some(mirror.label.clone()),
            current_text_value: Some(mirror.text.clone()),
            ..egui::WidgetInfo::new(egui::accesskit::Role::Terminal)
        });
        if !mirror.selection.is_empty() {
            ui.ctx().accesskit_node_builder(response.id, |node| {
                node.set_description(mirror.selection.clone());
            });
        }
    }

    let modes = snapshot.modes;
    let pointer = response.interact_pointer_pos();
    let selection_end = |pos: Pos2| -> (i32, usize) {
        let row = ((pos.y - inner.top()) / line_h).floor().max(0.0) as usize;
        let row = row.min(rows.saturating_sub(1));
        let col = ((pos.x - inner.left()) / char_w).round().max(0.0) as usize;
        let col = col.min(cols.saturating_sub(1));
        (snapshot.absolute_line(row), col)
    };

    let mouse_reporting = modes.mouse_click || modes.mouse_drag || modes.mouse_motion;
    if let Some(pos) = pointer {
        let shift = ui.input(|input| input.modifiers.shift);
        if mouse_reporting && !shift {
            let (primary_pressed, primary_released, secondary_pressed, secondary_released) = ui
                .input(|input| {
                    (
                        input.pointer.primary_pressed(),
                        input.pointer.primary_released(),
                        input.pointer.secondary_pressed(),
                        input.pointer.secondary_released(),
                    )
                });
            let (middle_pressed, middle_released) = ui.input(|input| {
                (
                    input.pointer.button_pressed(egui::PointerButton::Middle),
                    input.pointer.button_released(egui::PointerButton::Middle),
                )
            });
            let sgr = snapshot.modes.sgr_mouse;
            if primary_pressed {
                send_mouse(session, snapshot, pos, inner, line_h, char_w, 0, true, sgr);
            }
            if primary_released {
                send_mouse(session, snapshot, pos, inner, line_h, char_w, 0, false, sgr);
            }
            if middle_pressed {
                send_mouse(session, snapshot, pos, inner, line_h, char_w, 1, true, sgr);
            }
            if middle_released {
                send_mouse(session, snapshot, pos, inner, line_h, char_w, 1, false, sgr);
            }
            if secondary_pressed {
                send_mouse(session, snapshot, pos, inner, line_h, char_w, 2, true, sgr);
            }
            if secondary_released {
                send_mouse(session, snapshot, pos, inner, line_h, char_w, 2, false, sgr);
            }
            // Drag motion uses button code 32 (motion with left held).
            if snapshot.modes.mouse_drag && response.dragged() {
                send_mouse(session, snapshot, pos, inner, line_h, char_w, 32, true, sgr);
            }
        } else if response.clicked() {
            let point = selection_end(pos);
            *selection = Some(TermSelection {
                start: point,
                end: point,
            });
        } else if response.dragged() {
            if let Some(sel) = selection.as_mut() {
                sel.end = selection_end(pos);
            }
        }
    }

    if focus && !search.open && !ui.ctx().egui_wants_keyboard_input() {
        handle_input(ui, session, snapshot, preedit, session_id);
        if let Some(sel) = selection.as_ref() {
            if ui.input(|input| input.modifiers.command && input.key_pressed(Key::C)) {
                let text = selection_text(snapshot, sel);
                if !text.is_empty() {
                    ui.ctx().copy_text(text);
                }
            }
        }
        if ui.input(|input| input.modifiers.command && input.key_pressed(Key::F)) {
            search.open = true;
            search.focus_requested = true;
            search.query.clear();
            search.matches.clear();
            search.index = 0;
        }
    }

    // Wheel: scroll history unless the program owns the mouse. Fractional
    // trackpad deltas are accumulated so slow gestures are not lost.
    if response.hovered() {
        let dy = ui.input(|input| input.smooth_scroll_delta.y);
        if dy.abs() > 0.0 {
            if mouse_reporting && !ui.input(|input| input.modifiers.shift) {
                let button = if dy > 0.0 { 64 } else { 65 };
                send_wheel(
                    session,
                    snapshot,
                    &inner,
                    line_h,
                    char_w,
                    pos_or(pointer, inner),
                    button,
                );
            } else {
                *wheel_accum += dy / line_h;
                let whole = wheel_accum.trunc() as i32;
                if whole != 0 {
                    *wheel_accum -= whole as f32;
                    session.scroll_delta(whole);
                    session.snapshot_into(snapshot);
                }
            }
        }
    }

    // Backgrounds, selection and text.
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, p.terminal_bg);
    let (sel_start, sel_end) = if let Some(sel) = selection.as_ref() {
        let (start, end) = sel.normalised();
        (Some(start), Some(end))
    } else {
        (None, None)
    };
    let search_match = search.matches.get(search.index).copied();
    let default_fg = p.text;
    let default_bg = p.terminal_bg;

    for row in 0..rows {
        let abs_line = snapshot.absolute_line(row);
        let y = inner.top() + row as f32 * line_h;

        // Background pass first, so text is never painted over by a later
        // background run.
        for col in 0..cols {
            let cell = snapshot.cell(row, col);
            if cell.attrs & ATTR_SPACER != 0 || cell.attrs & ATTR_LEADING_SPACER != 0 {
                continue;
            }
            let (mut fg, mut bg) = (resolve(cell.fg, &p), resolve(cell.bg, &p));
            if cell.attrs & ATTR_INVERSE != 0 {
                std::mem::swap(&mut fg, &mut bg);
            }
            if bg != default_bg {
                let width = if cell.attrs & ATTR_WIDE != 0 {
                    2.0 * char_w
                } else {
                    char_w
                };
                painter.rect_filled(
                    Rect::from_min_size(
                        Pos2::new(inner.left() + col as f32 * char_w, y),
                        Vec2::new(
                            width.min(inner.right() - (inner.left() + col as f32 * char_w)),
                            line_h,
                        ),
                    ),
                    0.0,
                    bg,
                );
            }
        }

        // Selection and search are overlays on top of cell backgrounds.
        if let (Some(start), Some(end)) = (sel_start, sel_end) {
            if abs_line >= start.0 && abs_line <= end.0 {
                let first_col = if abs_line == start.0 { start.1 } else { 0 };
                let last_col = if abs_line == end.0 {
                    (end.1 + 1).min(cols)
                } else {
                    cols
                };
                if last_col > first_col {
                    painter.rect_filled(
                        Rect::from_min_size(
                            Pos2::new(inner.left() + first_col as f32 * char_w, y),
                            Vec2::new((last_col - first_col) as f32 * char_w, line_h),
                        ),
                        0.0,
                        p.selection,
                    );
                }
            }
        }
        if search.open {
            if let Some(hit) = search_match {
                if hit.line == abs_line {
                    painter.rect_filled(
                        Rect::from_min_size(
                            Pos2::new(inner.left() + hit.col as f32 * char_w, y),
                            Vec2::new(hit.len as f32 * char_w, line_h),
                        ),
                        0.0,
                        p.accent_soft,
                    );
                }
            }
        }

        let mut run_start = 0usize;
        let mut run_text = String::new();
        let mut run_color = default_fg;
        let mut run_attrs = 0u16;
        for col in 0..cols {
            let cell = snapshot.cell(row, col);
            let attrs = cell.attrs;
            if attrs & ATTR_SPACER != 0 || attrs & ATTR_LEADING_SPACER != 0 {
                continue;
            }
            let (mut fg, mut bg) = (resolve(cell.fg, &p), resolve(cell.bg, &p));
            if attrs & ATTR_INVERSE != 0 {
                std::mem::swap(&mut fg, &mut bg);
            }
            if attrs & ATTR_DIM != 0 {
                fg = fg.gamma_multiply(0.62);
            }
            if attrs & ATTR_BOLD != 0 {
                fg = brighten(fg, cell.fg, &p);
            }
            if attrs & ATTR_HIDDEN != 0 {
                continue;
            }
            let wide = attrs & ATTR_WIDE != 0;
            let text = cell.text.as_deref();
            if wide || text.is_some() {
                flush_run(
                    &painter,
                    &mut run_text,
                    run_start,
                    col,
                    run_color,
                    run_attrs,
                    y,
                    line_h,
                    inner,
                    char_w,
                    font_size,
                    &mut run_color,
                );
                run_attrs = attrs;
                let display = text
                    .map(|text| text.to_string())
                    .unwrap_or_else(|| cell.ch.to_string());
                let cell_w = if wide { 2.0 * char_w } else { char_w };
                painter.text(
                    Pos2::new(inner.left() + col as f32 * char_w, y + line_h * 0.5),
                    Align2::LEFT_CENTER,
                    display,
                    FontId::monospace(font_size),
                    fg,
                );
                if attrs & ATTR_UNDERLINE != 0 {
                    painter.line_segment(
                        [
                            Pos2::new(inner.left() + col as f32 * char_w, y + line_h - 1.5),
                            Pos2::new(
                                inner.left() + col as f32 * char_w + cell_w,
                                y + line_h - 1.5,
                            ),
                        ],
                        Stroke::new(1.0, fg),
                    );
                }
                run_start = col + if wide { 2 } else { 1 };
                run_text.clear();
                run_color = fg;
                continue;
            }
            if run_color != fg || run_attrs != attrs {
                flush_run(
                    &painter,
                    &mut run_text,
                    run_start,
                    col,
                    run_color,
                    run_attrs,
                    y,
                    line_h,
                    inner,
                    char_w,
                    font_size,
                    &mut run_color,
                );
                run_start = col;
                run_color = fg;
                run_attrs = attrs;
            }
            run_text.push(cell.ch);
        }
        if !run_text.is_empty() {
            flush_run(
                &painter,
                &mut run_text,
                run_start,
                cols,
                run_color,
                run_attrs,
                y,
                line_h,
                inner,
                char_w,
                font_size,
                &mut run_color,
            );
        }
    }

    // Kitty graphics overlays sit on top of the cell grid and below the
    // cursor.
    graphics.sync(ui.ctx(), session, snapshot);
    graphics.paint(&painter, snapshot, inner, char_w, line_h, rows);

    // Cursor & IME activation.
    if focus {
        let (cx, cy) = if let Some(cursor) = snapshot.cursor {
            let x = inner.left() + cursor.col as f32 * char_w;
            let y = inner.top() + (cursor.row.min(rows.saturating_sub(1))) as f32 * line_h;
            (x, y)
        } else {
            (inner.left(), inner.top())
        };
        let cursor_rect = Rect::from_min_size(Pos2::new(cx, cy), Vec2::new(char_w, line_h));
        ui.ctx().output_mut(|o| {
            o.ime = Some(egui::output::IMEOutput {
                purpose: egui::IMEPurpose::Normal,
                rect: inner,
                cursor_rect,
                should_interrupt_composition: false,
            });
        });

        if let Some(cursor) = snapshot.cursor {
            if cursor.row < rows {
                let x = inner.left() + cursor.col as f32 * char_w;
                let y = inner.top() + cursor.row as f32 * line_h;
                let blink = ((ui.input(|input| input.time) * 1.6) as i64 % 2) == 0;
                if blink || !preedit.is_empty() {
                    match cursor.shape {
                        lazecode_terminal::CursorShape::Beam => {
                            painter.rect_filled(
                                Rect::from_min_size(Pos2::new(x, y), Vec2::new(1.6, line_h)),
                                0.0,
                                p.accent,
                            );
                        }
                        lazecode_terminal::CursorShape::Underline => {
                            painter.rect_filled(
                                Rect::from_min_size(
                                    Pos2::new(x, y + line_h - 2.0),
                                    Vec2::new(char_w, 2.0),
                                ),
                                0.0,
                                p.accent,
                            );
                        }
                        _ => {
                            painter.rect_filled(
                                Rect::from_min_size(Pos2::new(x, y), Vec2::new(char_w, line_h)),
                                0.0,
                                p.accent.gamma_multiply(0.65),
                            );
                            let cell = snapshot.cell(cursor.row, cursor.col);
                            let text = cell
                                .text
                                .as_deref()
                                .map(|text| text.to_string())
                                .unwrap_or_else(|| cell.ch.to_string());
                            painter.text(
                                Pos2::new(x + char_w * 0.5, y + line_h * 0.5),
                                Align2::CENTER_CENTER,
                                text,
                                FontId::monospace(font_size),
                                p.accent_on_fill,
                            );
                        }
                    }
                }
                if !preedit.is_empty() {
                    let width = char_w * preedit.chars().count() as f32;
                    painter.rect_filled(
                        Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, line_h)),
                        0.0,
                        p.terminal_bg,
                    );
                    painter.text(
                        Pos2::new(x, y + line_h * 0.5),
                        Align2::LEFT_CENTER,
                        preedit.as_str(),
                        FontId::monospace(font_size),
                        p.text,
                    );
                    painter.line_segment(
                        [
                            Pos2::new(x, y + line_h - 1.0),
                            Pos2::new(x + width, y + line_h - 1.0),
                        ],
                        Stroke::new(1.0, p.accent),
                    );
                }
            }
        }
    }

    // Scrollback.
    if snapshot.history_size > 0 {
        let track = Rect::from_min_max(
            Pos2::new(rect.right() - SCROLLBAR, rect.top()),
            Pos2::new(rect.right(), rect.bottom()),
        );
        let history = snapshot.history_size;
        let content_h = (history + rows) as f32 * line_h;
        let viewport_h = rows as f32 * line_h;
        let mut offset = (history.saturating_sub(snapshot.display_offset)) as f32 * line_h;
        if crate::widgets::scrollbar(ui, track, content_h, viewport_h, &mut offset, theme) {
            let target = (offset / line_h).round() as i32;
            let current = history.saturating_sub(snapshot.display_offset) as i32;
            // `Scroll::Delta` is positive-up: dragging the thumb toward the
            // top must increase display_offset, i.e. decrease the offset from
            // the top.
            session.scroll_delta(current - target);
            session.snapshot_into(snapshot);
        }
    }

    // Terminal search overlay.
    if search.open {
        draw_search(ui, theme, rect, session, search);
    }

    TerminalOutput { focus_requested }
}

#[allow(clippy::too_many_arguments)]
fn flush_run(
    painter: &egui::Painter,
    run_text: &mut String,
    start: usize,
    end: usize,
    color: Color32,
    attrs: u16,
    y: f32,
    line_h: f32,
    inner: Rect,
    char_w: f32,
    font_size: f32,
    run_color: &mut Color32,
) {
    if run_text.is_empty() || end <= start {
        run_text.clear();
        *run_color = color;
        return;
    }
    painter.text(
        Pos2::new(inner.left() + start as f32 * char_w, y + line_h * 0.5),
        Align2::LEFT_CENTER,
        run_text.as_str(),
        FontId::monospace(font_size),
        color,
    );
    if attrs & ATTR_UNDERLINE != 0 {
        painter.line_segment(
            [
                Pos2::new(inner.left() + start as f32 * char_w, y + line_h - 1.5),
                Pos2::new(inner.left() + end as f32 * char_w, y + line_h - 1.5),
            ],
            Stroke::new(1.0, color),
        );
    }
    if attrs & ATTR_STRIKEOUT != 0 {
        painter.line_segment(
            [
                Pos2::new(inner.left() + start as f32 * char_w, y + line_h * 0.5),
                Pos2::new(inner.left() + end as f32 * char_w, y + line_h * 0.5),
            ],
            Stroke::new(1.0, color),
        );
    }
    run_text.clear();
    *run_color = color;
}

fn resolve(color: ColorSpec, p: &crate::theme::Palette) -> Color32 {
    match color {
        ColorSpec::DefaultFg => p.text,
        ColorSpec::DefaultBg => p.terminal_bg,
        ColorSpec::Cursor => p.accent,
        ColorSpec::Indexed(index) => indexed_color(index, p),
        ColorSpec::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    }
}

/// xterm 256-color palette: 0-15 come from the app's ANSI theme, 16-231 are
/// the 6×6×6 cube, 232-255 the grayscale ramp.
fn indexed_color(index: u8, p: &crate::theme::Palette) -> Color32 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    if index < 16 {
        return p.ansi[index as usize];
    }
    if index < 232 {
        let offset = (index - 16) as usize;
        let r = LEVELS[offset / 36];
        let g = LEVELS[(offset % 36) / 6];
        let b = LEVELS[offset % 6];
        return Color32::from_rgb(r, g, b);
    }
    let value = 8 + (index - 232) * 10;
    Color32::from_rgb(value, value, value)
}

fn brighten(fg: Color32, spec: ColorSpec, p: &crate::theme::Palette) -> Color32 {
    if let ColorSpec::Indexed(index) = spec {
        if index < 8 {
            return p.ansi[index as usize + 8];
        }
    }
    fg.gamma_multiply(1.12)
}

fn handle_input(
    ui: &mut Ui,
    session: &Session,
    snapshot: &Snapshot,
    preedit: &mut String,
    _id: u64,
) {
    let events = ui.input(|input| input.events.clone());
    let alt_down = ui.input(|input| input.modifiers.alt);
    let mut bytes = Vec::new();
    for event in events {
        match event {
            Event::Text(text) => {
                // With Option-as-Alt, the Key event carries the meta prefix.
                // Do not also insert the composed/unmodified text.
                if !alt_down {
                    bytes.extend_from_slice(text.as_bytes());
                }
            }
            Event::Paste(text) => {
                bytes.extend(lazecode_terminal::encode_paste(
                    &text,
                    snapshot.modes.bracketed_paste,
                ));
            }
            Event::Ime(ImeEvent::Preedit { text, .. }) => {
                *preedit = text;
            }
            Event::Ime(ImeEvent::Commit(text)) => {
                preedit.clear();
                bytes.extend_from_slice(text.as_bytes());
            }
            Event::Key {
                key,
                pressed,
                repeat,
                modifiers,
                ..
            } => {
                if let Some(mut stroke) = key_to_stroke(key, modifiers) {
                    stroke.event = match (pressed, repeat) {
                        (true, true) => KeyEvent::Repeat,
                        (true, false) => KeyEvent::Press,
                        (false, _) => KeyEvent::Release,
                    };
                    if let Some(encoded) = encode_stroke_for(stroke, snapshot) {
                        bytes.extend_from_slice(&encoded);
                    }
                }
            }
            Event::Copy => {
                // Copy handled by the caller, which owns the selection.
            }
            _ => {}
        }
    }
    if !bytes.is_empty() {
        session.write(&bytes);
    }
}

fn encode_stroke_for(stroke: lazecode_terminal::KeyStroke, snapshot: &Snapshot) -> Option<Vec<u8>> {
    lazecode_terminal::encode_key(stroke, &snapshot.modes)
}

fn key_to_stroke(key: Key, modifiers: Modifiers) -> Option<lazecode_terminal::KeyStroke> {
    use lazecode_terminal::KeyStroke;
    if modifiers.command {
        return None;
    }
    if modifiers.ctrl || modifiers.alt {
        if let Some(mut ch) = key_char(key) {
            if modifiers.ctrl {
                ch = ch.to_ascii_lowercase();
            }
            return Some(KeyStroke {
                key: TerminalKey::Char(ch),
                ctrl: modifiers.ctrl,
                alt: modifiers.alt,
                shift: modifiers.shift,
                event: KeyEvent::Press,
            });
        }
    }
    let terminal_key = match key {
        Key::Enter => TerminalKey::Enter,
        Key::Tab => TerminalKey::Tab,
        Key::Backspace => TerminalKey::Backspace,
        Key::Escape => TerminalKey::Escape,
        Key::ArrowUp => TerminalKey::Up,
        Key::ArrowDown => TerminalKey::Down,
        Key::ArrowLeft => TerminalKey::Left,
        Key::ArrowRight => TerminalKey::Right,
        Key::Home => TerminalKey::Home,
        Key::End => TerminalKey::End,
        Key::PageUp => TerminalKey::PageUp,
        Key::PageDown => TerminalKey::PageDown,
        Key::Delete => TerminalKey::Delete,
        Key::Insert => TerminalKey::Insert,
        Key::F1 => TerminalKey::F(1),
        Key::F2 => TerminalKey::F(2),
        Key::F3 => TerminalKey::F(3),
        Key::F4 => TerminalKey::F(4),
        Key::F5 => TerminalKey::F(5),
        Key::F6 => TerminalKey::F(6),
        Key::F7 => TerminalKey::F(7),
        Key::F8 => TerminalKey::F(8),
        Key::F9 => TerminalKey::F(9),
        Key::F10 => TerminalKey::F(10),
        Key::F11 => TerminalKey::F(11),
        Key::F12 => TerminalKey::F(12),
        _ => return None,
    };
    if modifiers.command {
        return None;
    }
    Some(KeyStroke {
        key: terminal_key,
        ctrl: modifiers.ctrl,
        alt: modifiers.alt,
        shift: modifiers.shift,
        event: KeyEvent::Press,
    })
}

fn key_char(key: Key) -> Option<char> {
    Some(match key {
        Key::A => 'a',
        Key::B => 'b',
        Key::C => 'c',
        Key::D => 'd',
        Key::E => 'e',
        Key::F => 'f',
        Key::G => 'g',
        Key::H => 'h',
        Key::I => 'i',
        Key::J => 'j',
        Key::K => 'k',
        Key::L => 'l',
        Key::M => 'm',
        Key::N => 'n',
        Key::O => 'o',
        Key::P => 'p',
        Key::Q => 'q',
        Key::R => 'r',
        Key::S => 's',
        Key::T => 't',
        Key::U => 'u',
        Key::V => 'v',
        Key::W => 'w',
        Key::X => 'x',
        Key::Y => 'y',
        Key::Z => 'z',
        Key::Num0 => '0',
        Key::Num1 => '1',
        Key::Num2 => '2',
        Key::Num3 => '3',
        Key::Num4 => '4',
        Key::Num5 => '5',
        Key::Num6 => '6',
        Key::Num7 => '7',
        Key::Num8 => '8',
        Key::Num9 => '9',
        Key::Space => ' ',
        Key::Colon => ':',
        Key::Comma => ',',
        Key::Backslash => '\\',
        Key::Slash => '/',
        Key::Pipe => '|',
        Key::Questionmark => '?',
        Key::Exclamationmark => '!',
        Key::OpenBracket => '[',
        Key::CloseBracket => ']',
        Key::OpenCurlyBracket => '{',
        Key::CloseCurlyBracket => '}',
        Key::Backtick => '`',
        Key::Minus => '-',
        Key::Period => '.',
        Key::Plus => '+',
        Key::Equals => '=',
        Key::Semicolon => ';',
        Key::Quote => '\'',
        _ => return None,
    })
}

fn selection_text(snapshot: &Snapshot, selection: &TermSelection) -> String {
    let ((start_line, start_col), (end_line, end_col)) = selection.normalised();
    let mut out = String::new();
    for line in start_line..=end_line {
        let row = line - snapshot.absolute_line(0);
        if row < 0 || row as usize >= snapshot.rows {
            continue;
        }
        let row = row as usize;
        let first = if line == start_line { start_col } else { 0 };
        let last = if line == end_line {
            (end_col + 1).min(snapshot.cols)
        } else {
            snapshot.cols
        };
        let mut text = String::new();
        for col in first..last {
            let cell = snapshot.cell(row, col);
            if cell.attrs & ATTR_SPACER != 0 || cell.attrs & ATTR_LEADING_SPACER != 0 {
                continue;
            }
            if let Some(cluster) = &cell.text {
                text.push_str(cluster);
            } else if cell.ch != '\0' {
                text.push(cell.ch);
            }
        }
        out.push_str(text.trim_end());
        if line != end_line {
            out.push('\n');
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn send_mouse(
    session: &Session,
    snapshot: &Snapshot,
    pos: Pos2,
    inner: Rect,
    line_h: f32,
    char_w: f32,
    button: u16,
    press: bool,
    sgr: bool,
) {
    let row = ((pos.y - inner.top()) / line_h).floor().max(0.0) as usize;
    let col = ((pos.x - inner.left()) / char_w).floor().max(0.0) as usize;
    let row = row.min(snapshot.rows.saturating_sub(1)) + 1;
    let col = col.min(snapshot.cols.saturating_sub(1)) + 1;
    if sgr {
        let suffix = if press { 'M' } else { 'm' };
        let sequence = format!("\x1b[<{button};{col};{row}{suffix}");
        session.write(sequence.as_bytes());
    } else {
        // Legacy X10 encoding: ESC [ M Cb Cx Cy with 32-biased bytes. A
        // release is button 3 regardless of which button was held.
        let legacy_button = if press { button } else { 3 };
        let bytes = [
            0x1b,
            b'[',
            b'M',
            (32 + legacy_button).min(255) as u8,
            (32 + col).min(255) as u8,
            (32 + row).min(255) as u8,
        ];
        session.write(&bytes);
    }
}

fn send_wheel(
    session: &Session,
    snapshot: &Snapshot,
    inner: &Rect,
    line_h: f32,
    char_w: f32,
    pos: Pos2,
    button: u16,
) {
    let row = (((pos.y - inner.top()) / line_h).floor().max(0.0) as usize)
        .min(snapshot.rows.saturating_sub(1))
        + 1;
    let col = (((pos.x - inner.left()) / char_w).floor().max(0.0) as usize)
        .min(snapshot.cols.saturating_sub(1))
        + 1;
    if snapshot.modes.sgr_mouse {
        let sequence = format!("\x1b[<{button};{col};{row}M");
        session.write(sequence.as_bytes());
    } else {
        let bytes = [
            0x1b,
            b'[',
            b'M',
            (32 + button).min(255) as u8,
            (32 + col).min(255) as u8,
            (32 + row).min(255) as u8,
        ];
        session.write(&bytes);
    }
}

fn pos_or(pos: Option<Pos2>, inner: Rect) -> Pos2 {
    pos.unwrap_or_else(|| inner.center())
}

#[allow(clippy::too_many_arguments)]
fn draw_search(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    session: &Session,
    state: &mut TermSearchState,
) {
    let p = theme.palette;
    let panel = Rect::from_min_size(
        Pos2::new(rect.right() - 316.0, rect.top() + 6.0),
        Vec2::new(304.0, 34.0),
    );
    // 稳定版 `.terminal-find-bar`：悬浮右上，bg-header + 6px 圆角。
    ui.painter()
        .rect_filled(panel, egui::CornerRadius::same(6), p.titlebar);
    ui.painter().rect_stroke(
        panel,
        egui::CornerRadius::same(6),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(panel.left() + 15.0, panel.center().y),
            Vec2::splat(12.0),
        ),
        Icon::Search,
        p.text3,
    );
    let field = Rect::from_min_max(
        Pos2::new(panel.left() + 26.0, panel.top() + 4.0),
        Pos2::new(panel.right() - 96.0, panel.bottom() - 4.0),
    );
    let response = ui.put(
        field,
        TextEdit::singleline(&mut state.query)
            .frame(egui::Frame::NONE)
            .hint_text("Find in terminal")
            .font(FontId::proportional(11.0)),
    );
    if state.focus_requested {
        response.request_focus();
        state.focus_requested = false;
    }
    if response.changed() {
        state.matches = session.search(&state.query);
        state.index = 0;
        if let Some(hit) = state.matches.first() {
            session.scroll_to_line(hit.line);
        }
    }
    if ui.input(|input| input.key_pressed(Key::Escape)) {
        state.open = false;
        state.matches.clear();
        return;
    }
    if ui.input(|input| input.key_pressed(Key::Enter)) && !state.matches.is_empty() {
        state.index = (state.index + 1) % state.matches.len();
        session.scroll_to_line(state.matches[state.index].line);
    }
    // ↑ ↓ 匹配导航（稳定版 find 按钮）。
    let nav = |ui: &mut Ui, rect: Rect, id: &'static str, icon: Icon| -> bool {
        let response = ui.interact(rect, ui.scope_id().with(id), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(4), p.hover);
        }
        paint_icon(ui.painter(), rect.shrink(4.0), icon, p.text2);
        response.clicked()
    };
    let prev_rect = Rect::from_center_size(
        Pos2::new(panel.right() - 62.0, panel.center().y),
        Vec2::splat(20.0),
    );
    let next_rect = Rect::from_center_size(
        Pos2::new(panel.right() - 40.0, panel.center().y),
        Vec2::splat(20.0),
    );
    let mut jump: i64 = 0;
    if nav(ui, prev_rect, "term-find-prev", Icon::ChevronUp) && !state.matches.is_empty() {
        jump = -1;
    }
    if nav(ui, next_rect, "term-find-next", Icon::ChevronDown) && !state.matches.is_empty() {
        jump = 1;
    }
    if jump != 0 {
        let total = state.matches.len() as i64;
        state.index = ((state.index as i64 + jump).rem_euclid(total)) as usize;
        session.scroll_to_line(state.matches[state.index].line);
    }
    ui.painter().text(
        Pos2::new(panel.right() - 90.0, panel.center().y),
        Align2::LEFT_CENTER,
        if state.matches.is_empty() {
            if state.query.is_empty() {
                String::new()
            } else {
                "none".to_string()
            }
        } else {
            format!("{} / {}", state.index + 1, state.matches.len())
        },
        FontId::monospace(9.0),
        p.text3,
    );
    let close = Rect::from_center_size(
        Pos2::new(panel.right() - 14.0, panel.center().y),
        Vec2::splat(18.0),
    );
    let close_response = ui.interact(
        close,
        ui.scope_id().with("term-search-close"),
        Sense::click(),
    );
    paint_icon(ui.painter(), close.shrink(3.0), Icon::X, p.text3);
    if close_response.clicked() {
        state.open = false;
        state.matches.clear();
    }
}

/// Bounded accessibility mirror of the visible viewport.
pub struct A11yMirror {
    /// Accessible name: geometry, cursor and scroll/focus state.
    pub label: String,
    /// Visible grid text, one line per row, trailing blanks trimmed.
    pub text: String,
    /// Selected text (empty when there is no selection).
    pub selection: String,
}

/// Build the screen-reader view of the terminal.
///
/// Only the visible viewport is walked: no scrollback is cloned, so the cost
/// is the same order as one rendered frame.
pub fn a11y_mirror(
    snapshot: &Snapshot,
    selection: Option<&TermSelection>,
    focused: bool,
) -> A11yMirror {
    let text = a11y_text(snapshot);
    let mut label = format!(
        "Terminal, {} columns by {} rows",
        snapshot.cols, snapshot.rows
    );
    if let Some(cursor) = snapshot.cursor {
        label.push_str(&format!(
            ", cursor line {} column {}",
            cursor.row + 1,
            cursor.col + 1
        ));
    }
    if snapshot.display_offset > 0 {
        label.push_str(&format!(
            ", scrolled back {} of {} history lines",
            snapshot.display_offset, snapshot.history_size
        ));
    }
    label.push_str(if focused { ", focused" } else { ", unfocused" });

    let selection = selection
        .map(|selection| selection_text(snapshot, selection))
        .unwrap_or_default();
    A11yMirror {
        label,
        text,
        selection,
    }
}

/// Visible grid text with trailing blanks trimmed per row.
pub fn a11y_text(snapshot: &Snapshot) -> String {
    let mut out = String::with_capacity(snapshot.rows * (snapshot.cols + 1));
    for row in 0..snapshot.rows {
        for col in 0..snapshot.cols {
            let cell = snapshot.cell(row, col);
            if cell.attrs & (ATTR_SPACER | ATTR_LEADING_SPACER) != 0 {
                continue;
            }
            match &cell.text {
                Some(cluster) => out.push_str(cluster),
                None if cell.ch != '\0' => out.push(cell.ch),
                None => out.push(' '),
            }
        }
        // Trim the blank tail so screen readers do not announce padding.
        while out.ends_with(' ') {
            out.pop();
        }
        if row + 1 < snapshot.rows {
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use lazecode_terminal::CellData;

    use super::*;

    fn text_snapshot(rows: &[&str], cols: usize) -> Snapshot {
        let mut snapshot = Snapshot {
            cols,
            rows: rows.len(),
            ..Snapshot::default()
        };
        snapshot.cells = vec![CellData::default(); cols * rows.len()];
        for (row, line) in rows.iter().enumerate() {
            for (col, ch) in line.chars().enumerate().take(cols) {
                snapshot.cells[row * cols + col].ch = ch;
            }
        }
        snapshot
    }

    #[test]
    fn a11y_text_mirrors_the_visible_viewport_only() {
        let snapshot = text_snapshot(&["hello", "world"], 8);
        assert_eq!(a11y_text(&snapshot), "hello\nworld");

        // History size does not change the mirror size: only viewport cells
        // are exposed, so a screen reader never receives the whole scrollback.
        let mut scrolled = snapshot.clone();
        scrolled.history_size = 5_000;
        scrolled.display_offset = 120;
        scrolled.hot_history_size = 400;
        assert_eq!(a11y_text(&scrolled), "hello\nworld");
        assert_eq!(scrolled.cells.len(), scrolled.cols * scrolled.rows);
    }

    #[test]
    fn a11y_label_reports_cursor_scroll_and_focus() {
        let mut snapshot = text_snapshot(&["$ ls"], 8);
        snapshot.cursor = Some(lazecode_terminal::CursorView {
            row: 0,
            col: 2,
            shape: lazecode_terminal::CursorShape::Block,
            blinking: false,
        });
        snapshot.history_size = 40;
        snapshot.display_offset = 7;
        let mirror = a11y_mirror(&snapshot, None, true);
        assert!(
            mirror.label.contains("cursor line 1 column 3"),
            "{}",
            mirror.label
        );
        assert!(
            mirror.label.contains("scrolled back 7 of 40"),
            "{}",
            mirror.label
        );
        assert!(mirror.label.contains("focused"), "{}", mirror.label);
        assert!(mirror.selection.is_empty());

        // Absolute selection lines: the viewport starts at line -7.
        let selection = TermSelection {
            start: (-7, 0),
            end: (-7, 4),
        };
        let mirror = a11y_mirror(&snapshot, Some(&selection), false);
        assert_eq!(mirror.selection, "$ ls");
        assert!(mirror.label.contains("unfocused"));
    }

    #[test]
    fn kitty_source_rect_maps_to_texture_uv() {
        let placement = ImagePlacement {
            image_id: 1,
            col: 0,
            row: 0,
            columns: 1,
            rows: 1,
            source: (0, 0, 0, 0),
            z_index: 0,
        };
        let uv = source_uv(&placement, Vec2::new(10.0, 20.0));
        assert_eq!(uv, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)));

        let cropped = ImagePlacement {
            source: (5, 10, 5, 10),
            ..placement
        };
        let uv = source_uv(&cropped, Vec2::new(10.0, 20.0));
        assert_eq!(
            uv,
            Rect::from_min_max(Pos2::new(0.5, 0.5), Pos2::new(1.0, 1.0))
        );

        // A crop larger than the image clamps to the remaining pixels.
        let clamped = ImagePlacement {
            source: (5, 10, 500, 500),
            ..placement
        };
        let uv = source_uv(&clamped, Vec2::new(10.0, 20.0));
        assert_eq!(
            uv,
            Rect::from_min_max(Pos2::new(0.5, 0.5), Pos2::new(1.0, 1.0))
        );
    }

    #[test]
    fn graphics_cache_uploads_once_and_prunes_evicted_images() {
        let ctx = egui::Context::default();
        let mut cache = GraphicsCache::default();
        let image = lazecode_terminal::KittyImage {
            id: 7,
            width: 2,
            height: 2,
            rgba: std::sync::Arc::from(vec![0u8; 16].into_boxed_slice()),
            generation: 1,
        };
        cache.apply(&ctx, vec![image.clone()], &[7]);
        assert_eq!(cache.images.len(), 1);
        assert_eq!(cache.images[&7].generation, 1);

        // A stale upload is ignored; a new generation replaces the texture.
        cache.apply(&ctx, vec![image.clone()], &[7]);
        assert_eq!(cache.images[&7].generation, 1);
        let newer = lazecode_terminal::KittyImage {
            generation: 2,
            ..image
        };
        cache.apply(&ctx, vec![newer], &[7]);
        assert_eq!(cache.images[&7].generation, 2);

        // Eviction in the terminal drops the texture here too.
        cache.apply(&ctx, Vec::new(), &[]);
        assert!(cache.images.is_empty());
    }

    #[test]
    fn ctrl_and_alt_character_keys_become_strokes() {
        let ctrl = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        let stroke = key_to_stroke(Key::C, ctrl).expect("ctrl+c stroke");
        assert!(matches!(stroke.key, TerminalKey::Char('c')));
        assert!(stroke.ctrl);

        let alt = Modifiers {
            alt: true,
            ..Default::default()
        };
        let stroke = key_to_stroke(Key::B, alt).expect("alt+b stroke");
        assert!(matches!(stroke.key, TerminalKey::Char('b')));
        assert!(stroke.alt);

        assert!(key_to_stroke(Key::A, Modifiers::default()).is_none());
    }

    #[test]
    fn test_cjk_metrics() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        let mut out = ctx.run_ui(Default::default(), |_| {});
        out.textures_delta.clear();
        let size = 13.0;
        let char_w = crate::widgets::mono_char_width(&ctx, size);
        let ascii_galley = ctx.fonts_mut(|f| {
            f.layout_no_wrap(
                "M".to_string(),
                egui::FontId::monospace(size),
                egui::Color32::WHITE,
            )
        });
        let cjk_galley = ctx.fonts_mut(|f| {
            f.layout_no_wrap(
                "你好".to_string(),
                egui::FontId::monospace(size),
                egui::Color32::WHITE,
            )
        });
        assert!(cjk_galley.size().x > char_w);
        assert!(ascii_galley.size().x > 0.0);
        assert!(char_w > 0.0);
    }

    #[test]
    fn indexed_colors_cover_the_256_palette() {
        let palette = crate::theme::Theme::for_mode(lazecode_core::ThemeMode::Dark).palette;
        assert_eq!(
            indexed_color(196, &palette),
            egui::Color32::from_rgb(255, 0, 0)
        );
        assert_eq!(
            indexed_color(46, &palette),
            egui::Color32::from_rgb(0, 255, 0)
        );
        assert_eq!(
            indexed_color(232, &palette),
            egui::Color32::from_rgb(8, 8, 8)
        );
        assert_eq!(indexed_color(15, &palette), palette.ansi[15]);
    }
}
