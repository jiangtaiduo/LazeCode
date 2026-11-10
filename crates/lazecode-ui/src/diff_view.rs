//! Monaco DiffEditor replication — unified & side-by-side bodies.
//!
//! Stable renders the diff page through Monaco (`DiffEditor`) with the
//! theme.css overrides `.line-insert #2c4134`, `.line-delete #482b2b`,
//! `.char-insert #385542`, `.char-delete #5e3436`, ~20px rows, a two-column
//! gutter, the current-change outline (`.diff-hunk-active-box`, 1px #3574f0 +
//! glow) and the right-edge overview ruler. Rows measure
//! `round(fontSize * 1.5)` with the default `editorFontSize: 13` → 20px.

use egui::{Align2, Color32, FontId, Pos2, Rect, ScrollArea, Sense, Stroke, StrokeKind, Vec2};
use lazecode_text::{count_change_groups, inline_diff, DiffHunk, DiffLineKind, InlineChunkKind};

use crate::theme::Theme;

pub const ROW_H: f32 = 20.0;
const MONO_SIZE: f32 = 13.0;
/// Overview ruler width at the right edge (Monaco `renderOverviewRuler`).
pub const RULER_W: f32 = 10.0;

type ChunkCache = std::collections::HashMap<String, (Vec<(usize, usize)>, Vec<(usize, usize)>)>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Context,
    Add,
    Del,
}

pub struct DiffRow {
    pub kind: RowKind,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    pub text: String,
    /// Contiguous change-group index (`u32::MAX` for context rows).
    pub group: u32,
}

/// Flatten hunks into rows and label the contiguous change groups.
pub fn build_rows(hunks: &[DiffHunk]) -> (Vec<DiffRow>, usize) {
    let mut rows = Vec::new();
    let mut group = 0u32;
    let mut in_group = false;
    for hunk in hunks {
        for line in &hunk.lines {
            let kind = match line.kind {
                DiffLineKind::Context => RowKind::Context,
                DiffLineKind::Add => RowKind::Add,
                DiffLineKind::Delete => RowKind::Del,
            };
            if kind == RowKind::Context {
                in_group = false;
            } else if !in_group {
                group += 1;
                in_group = true;
            }
            rows.push(DiffRow {
                kind,
                old_no: line.old_lineno,
                new_no: line.new_lineno,
                text: line.text.clone(),
                group: if kind == RowKind::Context {
                    u32::MAX
                } else {
                    group - 1
                },
            });
        }
        in_group = false;
    }
    (rows, group as usize)
}

/// First row index of each change group (for navigation/reveal).
pub fn group_starts(rows: &[DiffRow], groups: usize) -> Vec<usize> {
    let mut starts = vec![rows.len(); groups];
    for (i, row) in rows.iter().enumerate() {
        let g = row.group as usize;
        if row.group != u32::MAX && g < groups && starts[g] == rows.len() {
            starts[g] = i;
        }
    }
    starts
}

/// Index pairs (del_row, add_row) by position within each contiguous run —
/// the char-highlight pairing for the unified view.
pub fn unified_pairs(rows: &[DiffRow]) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        if rows[i].kind == RowKind::Context {
            i += 1;
            continue;
        }
        let del_start = i;
        while i < rows.len() && rows[i].kind == RowKind::Del {
            i += 1;
        }
        let add_start = i;
        while i < rows.len() && rows[i].kind == RowKind::Add {
            i += 1;
        }
        let n = (add_start - del_start).min(i - add_start);
        for k in 0..n {
            pairs.push((del_start + k, add_start + k));
        }
    }
    pairs
}

/// Pair up runs for the side-by-side view: `(left, right)` rows of equal
/// length, `None` where one side has no line (filler).
pub fn split_pairs(rows: &[DiffRow]) -> Vec<(Option<usize>, Option<usize>)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        if rows[i].kind == RowKind::Context {
            out.push((Some(i), Some(i)));
            i += 1;
            continue;
        }
        let del_start = i;
        while i < rows.len() && rows[i].kind == RowKind::Del {
            i += 1;
        }
        let add_start = i;
        while i < rows.len() && rows[i].kind == RowKind::Add {
            i += 1;
        }
        let dels = del_start..add_start;
        let adds = add_start..i;
        let n = dels.len().max(adds.len());
        for k in 0..n {
            out.push((dels.clone().nth(k), adds.clone().nth(k)));
        }
    }
    out
}

fn gutter_w(rows: &[DiffRow], char_w: f32) -> f32 {
    let mut digits = 1usize;
    for row in rows {
        for no in [row.old_no, row.new_no].into_iter().flatten() {
            digits = digits.max(no.to_string().len());
        }
    }
    let digits = digits.min(5);
    (digits as f32 * char_w + 18.0).max(30.0)
}

/// Word-level changed byte ranges for one side of a del/add pair.
fn chunks_of(cache: &mut ChunkCache, del: &str, add: &str, want_old: bool) -> Vec<(usize, usize)> {
    let entry = cache
        .entry(del.to_string() + "\u{0}" + add)
        .or_insert_with(|| {
            let (old_chunks, new_chunks) = inline_diff(del, add);
            let fold = |chunks: &[lazecode_text::InlineChunk]| {
                let mut ranges = Vec::new();
                let mut pos = 0usize;
                let mut open: Option<usize> = None;
                for chunk in chunks {
                    match chunk.kind {
                        InlineChunkKind::Unchanged => {
                            if let Some(start) = open.take() {
                                ranges.push((start, pos));
                            }
                        }
                        _ if open.is_none() => open = Some(pos),
                        _ => {}
                    }
                    pos += chunk.text.len();
                }
                if let Some(start) = open {
                    ranges.push((start, pos));
                }
                ranges
            };
            (fold(&old_chunks), fold(&new_chunks))
        });
    if want_old {
        entry.0.clone()
    } else {
        entry.1.clone()
    }
}

pub struct DiffBodyCtx<'a> {
    pub tab_id: u64,
    pub rows: &'a [DiffRow],
    pub groups: usize,
    pub current_group: usize,
    /// (tab, group index) queued by toolbar navigation → reveal centered.
    pub scroll_to_group: Option<(u64, usize)>,
    pub can_rollback: bool,
}

/// Unified (inline) body — Monaco inline diff: deletions directly above the
/// paired additions inside the change group, two-column gutter, char-level
/// highlights, current-group outline, overview ruler.
pub fn draw_unified(
    ui: &mut egui::Ui,
    theme: &Theme,
    body: Rect,
    ctx: DiffBodyCtx<'_>,
) -> Option<usize> {
    let p = theme.palette;
    let font = FontId::monospace(MONO_SIZE);
    let char_w = crate::widgets::mono_char_width(ui.ctx(), MONO_SIZE).max(7.0);
    let col_w = gutter_w(ctx.rows, char_w);
    let icon_gutter = if ctx.can_rollback { 20.0 } else { 0.0 };
    let gutter_x0 = body.left() + 6.0 + icon_gutter;
    let gutter_end = gutter_x0 + col_w * 2.0;
    let content_x = gutter_end + 6.0;
    let content_right = body.right() - RULER_W - 2.0;
    let pairs = unified_pairs(ctx.rows);
    let mut pair_of = vec![usize::MAX; ctx.rows.len()];
    for (pi, (d, a)) in pairs.iter().enumerate() {
        pair_of[*d] = pi;
        pair_of[*a] = pi;
    }
    let mut cache: ChunkCache = ChunkCache::new();

    // Monaco revealLineInCenter 是瞬时定位：关掉 egui 滚动动画。
    let mut style = egui::Style::clone(ui.style());
    style.scroll_animation = egui::style::ScrollAnimation::none();
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(body)
            .style(style)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    // 导航定位：直接给 ScrollArea 设初始 offset（同帧生效，Monaco 的
    // revealLineInCenter 也是瞬时定位）。offset 以「视口内居中」计算。
    let jump_offset = ctx
        .scroll_to_group
        .filter(|(tab, _)| *tab == ctx.tab_id)
        .and_then(|(_, group)| {
            let starts = group_starts(ctx.rows, ctx.groups);
            starts.get(group).copied().map(|row_idx| {
                let top = row_idx as f32 * ROW_H;
                (top - (body.height() - ROW_H) / 2.0).max(0.0)
            })
        });
    let mut scroll_area = ScrollArea::vertical()
        .id_salt(("diff-body", ctx.tab_id))
        .auto_shrink([false, false]);
    if let Some(offset) = jump_offset {
        scroll_area = scroll_area.vertical_scroll_offset(offset);
    }
    let scroll = scroll_area.show(&mut child, |ui| {
        let mut rollback_clicked = None;
        let total_h = ctx.rows.len() as f32 * ROW_H;
        let (_, inner) = ui.allocate_exact_size(Vec2::new(body.width(), total_h), Sense::hover());
        let content_top = inner.rect.top();
        let num_font = FontId::monospace(12.0);
        let starts = group_starts(ctx.rows, ctx.groups);
        for (i, row) in ctx.rows.iter().enumerate() {
            let row_rect = Rect::from_min_size(
                Pos2::new(body.left(), content_top + i as f32 * ROW_H),
                Vec2::new(body.width(), ROW_H),
            );
            let y = row_rect.center().y;
            let line_bg = match row.kind {
                RowKind::Add => Some(p.diff_line_add),
                RowKind::Del => Some(p.diff_line_del),
                RowKind::Context => None,
            };
            if let Some(bg) = line_bg {
                ui.painter().rect_filled(row_rect, 0.0, bg);
            }
            if ctx.can_rollback
                && row.group != u32::MAX
                && starts.get(row.group as usize) == Some(&i)
            {
                let btn_rect =
                    Rect::from_center_size(Pos2::new(body.left() + 10.0, y), Vec2::splat(18.0));
                let resp = ui.interact(
                    btn_rect,
                    ui.scope_id().with(("rb-unified", ctx.tab_id, row.group)),
                    Sense::click(),
                );
                let icon_color = if resp.hovered() {
                    ui.painter()
                        .rect_filled(btn_rect, egui::CornerRadius::same(3), p.hover);
                    resp.show_tooltip_text("回滚此变更块 (Rollback this change)");
                    p.rose
                } else {
                    p.text2
                };
                let icon_rect = Rect::from_center_size(btn_rect.center(), Vec2::splat(14.0));
                crate::icons_svg::paint_svg(ui.painter(), icon_rect, "rollback", icon_color);
                if resp.clicked() {
                    rollback_clicked = Some(row.group as usize);
                }
            }
            ui.painter().text(
                Pos2::new(gutter_x0 + col_w - 8.0, y),
                Align2::RIGHT_CENTER,
                row.old_no.map(|n| n.to_string()).unwrap_or_default(),
                num_font.clone(),
                p.text3,
            );
            ui.painter().text(
                Pos2::new(gutter_x0 + col_w * 2.0 - 8.0, y),
                Align2::RIGHT_CENTER,
                row.new_no.map(|n| n.to_string()).unwrap_or_default(),
                num_font.clone(),
                p.text3,
            );
            let chunks = match row.kind {
                RowKind::Del => pair_of[i]
                    .ne(&usize::MAX)
                    .then(|| pairs[pair_of[i]])
                    .filter(|(d, _)| *d == i)
                    .map(|(_, a)| chunks_of(&mut cache, &row.text, &ctx.rows[a].text, true))
                    .unwrap_or_default(),
                RowKind::Add => pair_of[i]
                    .ne(&usize::MAX)
                    .then(|| pairs[pair_of[i]])
                    .filter(|(_, a)| *a == i)
                    .map(|(d, _)| chunks_of(&mut cache, &ctx.rows[d].text, &row.text, false))
                    .unwrap_or_default(),
                RowKind::Context => Vec::new(),
            };
            let char_bg = match row.kind {
                RowKind::Add => p.diff_char_add,
                RowKind::Del => p.diff_char_del,
                RowKind::Context => Color32::TRANSPARENT,
            };
            paint_line(
                ui.painter(),
                content_x,
                y,
                &row.text,
                char_w,
                &font,
                p.tok_punc,
                &chunks,
                char_bg,
            );
        }
        // 当前组蓝框：画在滚动内容坐标系内（与行同一 content_top），
        // 随内容滚动、被视口裁剪 —— 稳定版 box 挂在 lines-content 上。
        draw_group_box(
            ui,
            theme,
            &ctx,
            gutter_end,
            content_right + 2.0,
            content_top,
        );
        rollback_clicked
    });

    // Overview ruler: del marks left half (rose), add marks right half
    // (green); current group outlined; viewport band overlaid.
    let ruler = Rect::from_min_max(
        Pos2::new(body.right() - RULER_W, body.top()),
        Pos2::new(body.right(), body.bottom()),
    );
    let painter = ui.painter();
    let total = ctx.rows.len().max(1);
    for (i, row) in ctx.rows.iter().enumerate() {
        let y = body.top() + (i as f32 + 0.5) / total as f32 * body.height();
        match row.kind {
            RowKind::Del => {
                painter.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(ruler.left() + 1.0, y - 1.0),
                        Pos2::new(ruler.center().x, y + 1.0),
                    ),
                    1.0,
                    p.rose,
                );
            }
            RowKind::Add => {
                painter.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(ruler.center().x, y - 1.0),
                        Pos2::new(ruler.right() - 1.0, y + 1.0),
                    ),
                    1.0,
                    p.green,
                );
            }
            RowKind::Context => {}
        }
    }
    let band = current_group_band(&ctx, body, total);
    if let Some((y0, y1)) = band {
        painter.rect_stroke(
            Rect::from_min_max(
                Pos2::new(ruler.left(), y0),
                Pos2::new(ruler.right(), y1.max(y0 + 2.0)),
            ),
            1.0,
            Stroke::new(1.0, p.jb_accent),
            StrokeKind::Inside,
        );
    }
    // Viewport band (driven by the scroll state captured above).
    let content_h = ctx.rows.len() as f32 * ROW_H;
    if content_h > scroll.content_size.y.max(1.0) {
        let view_h = scroll.inner_rect.height();
        let frac = (view_h / content_h).min(1.0);
        let offset_frac = (scroll.state.clamped_offset().y / content_h).clamp(0.0, 1.0);
        let band = Rect::from_min_max(
            Pos2::new(ruler.left(), body.top() + offset_frac * body.height()),
            Pos2::new(
                ruler.right(),
                body.top() + (offset_frac + frac).min(1.0) * body.height(),
            ),
        );
        painter.rect_filled(
            band,
            0.0,
            Color32::from_rgba_unmultiplied(255, 255, 255, 14),
        );
        painter.rect_stroke(
            band,
            0.0,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 36)),
            StrokeKind::Inside,
        );
    }
    scroll.inner
}

/// Vertical band of the current group within a proportional `total` rows.
fn current_group_band(ctx: &DiffBodyCtx<'_>, body: Rect, total: usize) -> Option<(f32, f32)> {
    if ctx.current_group >= ctx.groups || total == 0 {
        return None;
    }
    let starts = group_starts(ctx.rows, ctx.groups);
    let first = starts[ctx.current_group];
    let last = ctx
        .rows
        .iter()
        .rposition(|row| row.group == ctx.current_group as u32)?;
    if first > last || first >= ctx.rows.len() {
        return None;
    }
    let y0 = body.top() + first as f32 / total as f32 * body.height();
    let y1 = body.top() + (last as f32 + 1.0) / total as f32 * body.height();
    Some((y0, y1))
}

/// Current-group outline (`.diff-hunk-active-box`): 1px #3574f0 + glow.
/// `split` draws one box per pane between `x0..x1` pairs instead.
fn draw_group_box(
    ui: &mut egui::Ui,
    theme: &Theme,
    ctx: &DiffBodyCtx<'_>,
    x0: f32,
    x1: f32,
    content_top: f32,
) {
    let p = theme.palette;
    let total = ctx.rows.len();
    if ctx.current_group >= ctx.groups || total == 0 {
        return;
    }
    let starts = group_starts(ctx.rows, ctx.groups);
    let first = starts[ctx.current_group];
    let Some(last) = ctx
        .rows
        .iter()
        .rposition(|row| row.group == ctx.current_group as u32)
    else {
        return;
    };
    if first > last || first >= total {
        return;
    }
    // content_top = 视口内第 0 行的屏幕 y（随滚动移动），框必须画在同一
    // 坐标系里才会跟着内容滚动（稳定版 box 挂在 lines-content 上）。
    let top = content_top + first as f32 * ROW_H;
    let bottom = content_top + (last as f32 + 1.0) * ROW_H;
    let rects: Vec<Rect> = vec![Rect::from_min_max(
        Pos2::new(x0, top),
        Pos2::new(x1, bottom),
    )];
    for rect in rects {
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::ZERO,
            Stroke::new(1.0, p.jb_accent.gamma_multiply(0.25)),
            StrokeKind::Outside,
        );
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::ZERO,
            Stroke::new(1.0, p.jb_accent),
            StrokeKind::Outside,
        );
    }
}

/// One text run painter: paints changed-chunk backgrounds then the text in
/// segments. Falls back to whole-line paint when the line is not ASCII.
#[allow(clippy::too_many_arguments)]
fn paint_line(
    painter: &egui::Painter,
    x: f32,
    y: f32,
    text: &str,
    char_w: f32,
    font: &FontId,
    color: Color32,
    chunks: &[(usize, usize)],
    char_bg: Color32,
) {
    if chunks.is_empty() || !text.is_ascii() {
        if !chunks.is_empty() && !text.is_ascii() {
            for (start, end) in chunks {
                let s = text[..*start].chars().count();
                let e = text[..(*end).min(text.len())].chars().count();
                painter.rect_filled(
                    Rect::from_min_size(
                        Pos2::new(x + s as f32 * char_w, y - ROW_H / 2.0),
                        Vec2::new((e - s) as f32 * char_w, ROW_H),
                    ),
                    0.0,
                    char_bg,
                );
            }
        }
        painter.text(
            Pos2::new(x, y),
            Align2::LEFT_CENTER,
            text,
            font.clone(),
            color,
        );
        return;
    }
    let mut segs: Vec<(usize, usize, bool)> = Vec::new();
    let mut cursor = 0usize;
    for (start, end) in chunks {
        if *start > cursor {
            segs.push((cursor, *start, false));
        }
        if *end > cursor {
            segs.push((*start, *end, true));
            cursor = *end;
        }
    }
    if cursor < text.len() {
        segs.push((cursor, text.len(), false));
    }
    for (start, end, changed) in segs {
        if end <= start {
            continue;
        }
        let cx = x + start as f32 * char_w;
        let w = (end - start) as f32 * char_w;
        if changed {
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(cx, y - ROW_H / 2.0), Vec2::new(w, ROW_H)),
                0.0,
                char_bg,
            );
        }
        painter.text(
            Pos2::new(cx, y),
            Align2::LEFT_CENTER,
            &text[start..end],
            font.clone(),
            color,
        );
    }
}

/// Side-by-side body: left column shows context+del, right shows context+add,
/// aligned per change group; filler rows where one side has none.
pub fn draw_split(
    ui: &mut egui::Ui,
    theme: &Theme,
    body: Rect,
    ctx: DiffBodyCtx<'_>,
) -> Option<usize> {
    let p = theme.palette;
    let font = FontId::monospace(MONO_SIZE);
    let char_w = crate::widgets::mono_char_width(ui.ctx(), MONO_SIZE).max(7.0);
    let pairs = split_pairs(ctx.rows);
    let ruler_left = body.right() - RULER_W;
    let half = ((ruler_left - body.left()) / 2.0).round();
    let mid = body.left() + half;

    let col_l = gutter_w(ctx.rows, char_w).min(half - 12.0);
    let col_r = gutter_w(ctx.rows, char_w).min((body.right() - mid) - 12.0);
    let num_l = body.left() + 6.0 + col_l;
    let num_r = mid + 4.0 + col_r;
    let text_l = num_l + 6.0;
    let text_r = num_r + 6.0;

    let scroll_to_row = ctx
        .scroll_to_group
        .filter(|(tab, _)| *tab == ctx.tab_id)
        .and_then(|(_, group)| {
            pairs.iter().position(|(l, r)| {
                l.map(|idx| ctx.rows[idx].group as usize) == Some(group)
                    || r.map(|idx| ctx.rows[idx].group as usize) == Some(group)
            })
        });
    let mut cache: ChunkCache = ChunkCache::new();

    let mut style = egui::Style::clone(ui.style());
    style.scroll_animation = egui::style::ScrollAnimation::none();
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(body)
            .style(style)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let jump_offset = scroll_to_row.map(|row_idx| {
        let top = row_idx as f32 * ROW_H;
        (top - (body.height() - ROW_H) / 2.0).max(0.0)
    });
    let mut scroll_area = ScrollArea::vertical()
        .id_salt(("diff-split", ctx.tab_id))
        .auto_shrink([false, false]);
    if let Some(offset) = jump_offset {
        scroll_area = scroll_area.vertical_scroll_offset(offset);
    }
    let scroll = scroll_area.show(&mut child, |ui| {
        let mut rollback_clicked = None;
        let total_h = pairs.len() as f32 * ROW_H;
        let (_, inner) = ui.allocate_exact_size(Vec2::new(body.width(), total_h), Sense::hover());
        let content_top = inner.rect.top();
        let num_font = FontId::monospace(12.0);
        for (i, (left, right)) in pairs.iter().enumerate() {
            let y_top = content_top + i as f32 * ROW_H;
            let y = y_top + ROW_H / 2.0;
            if ctx.can_rollback {
                let gi = left
                    .map(|li| ctx.rows[li].group)
                    .or_else(|| right.map(|ri| ctx.rows[ri].group))
                    .unwrap_or(u32::MAX);
                if gi != u32::MAX {
                    let is_start = pairs.iter().position(|(l, r)| {
                        l.map(|idx| ctx.rows[idx].group) == Some(gi)
                            || r.map(|idx| ctx.rows[idx].group) == Some(gi)
                    }) == Some(i);
                    if is_start {
                        let btn_rect = Rect::from_center_size(Pos2::new(mid, y), Vec2::splat(18.0));
                        let resp = ui.interact(
                            btn_rect,
                            ui.scope_id().with(("rb-split", ctx.tab_id, gi)),
                            Sense::click(),
                        );
                        let icon_color = if resp.hovered() {
                            ui.painter().rect_filled(
                                btn_rect,
                                egui::CornerRadius::same(3),
                                p.hover,
                            );
                            resp.show_tooltip_text("回滚此变更块 (Rollback this change)");
                            p.rose
                        } else {
                            p.text2
                        };
                        let icon_rect =
                            Rect::from_center_size(btn_rect.center(), Vec2::splat(14.0));
                        crate::icons_svg::paint_svg(
                            ui.painter(),
                            icon_rect,
                            "rollback",
                            icon_color,
                        );
                        if resp.clicked() {
                            rollback_clicked = Some(gi as usize);
                        }
                    }
                }
            }
            // Left pane.
            let l_rect = Rect::from_min_size(Pos2::new(body.left(), y_top), Vec2::new(half, ROW_H));
            if let Some(li) = left {
                let row = &ctx.rows[*li];
                if row.kind == RowKind::Del {
                    ui.painter().rect_filled(l_rect, 0.0, p.diff_line_del);
                }
                ui.painter().text(
                    Pos2::new(num_l - 8.0, y),
                    Align2::RIGHT_CENTER,
                    row.old_no.map(|n| n.to_string()).unwrap_or_default(),
                    num_font.clone(),
                    p.text3,
                );
                let chunks = match row.kind {
                    RowKind::Del => right
                        .map(|ri| chunks_of(&mut cache, &row.text, &ctx.rows[ri].text, true))
                        .unwrap_or_default(),
                    _ => Vec::new(),
                };
                paint_line(
                    ui.painter(),
                    text_l,
                    y,
                    &row.text,
                    char_w,
                    &font,
                    p.tok_punc,
                    &chunks,
                    p.diff_char_del,
                );
            } else {
                ui.painter().rect_filled(l_rect, 0.0, p.hover);
            }
            ui.painter()
                .vline(mid, y_top..=y_top + ROW_H, Stroke::new(1.0, p.border));
            // Right pane.
            let r_rect =
                Rect::from_min_size(Pos2::new(mid, y_top), Vec2::new(ruler_left - mid, ROW_H));
            if let Some(ri) = right {
                let row = &ctx.rows[*ri];
                if row.kind == RowKind::Add {
                    ui.painter().rect_filled(r_rect, 0.0, p.diff_line_add);
                }
                ui.painter().text(
                    Pos2::new(num_r - 8.0, y),
                    Align2::RIGHT_CENTER,
                    row.new_no.map(|n| n.to_string()).unwrap_or_default(),
                    num_font.clone(),
                    p.text3,
                );
                let chunks = match row.kind {
                    RowKind::Add => left
                        .map(|li| chunks_of(&mut cache, &ctx.rows[li].text, &row.text, false))
                        .unwrap_or_default(),
                    _ => Vec::new(),
                };
                paint_line(
                    ui.painter(),
                    text_r,
                    y,
                    &row.text,
                    char_w,
                    &font,
                    p.tok_punc,
                    &chunks,
                    p.diff_char_add,
                );
            } else {
                ui.painter().rect_filled(r_rect, 0.0, p.hover);
            }
        }
        // 当前组蓝框（两栏各一个）：滚动内容坐标系，随内容滚动。
        if ctx.current_group < ctx.groups && !ctx.rows.is_empty() {
            if let Some((top, bottom)) = split_group_band(&ctx, &pairs) {
                let top = content_top + top;
                let bottom = content_top + bottom;
                for (x0, x1) in [
                    (body.left() + 1.0, mid - 1.0),
                    (mid + 1.0, ruler_left - 1.0),
                ] {
                    let rect = Rect::from_min_max(Pos2::new(x0, top), Pos2::new(x1, bottom));
                    ui.painter().rect_stroke(
                        rect,
                        egui::CornerRadius::ZERO,
                        Stroke::new(1.0, p.jb_accent.gamma_multiply(0.25)),
                        StrokeKind::Outside,
                    );
                    ui.painter().rect_stroke(
                        rect,
                        egui::CornerRadius::ZERO,
                        Stroke::new(1.0, p.jb_accent),
                        StrokeKind::Outside,
                    );
                }
            }
        }
        rollback_clicked
    });
    let _ = scroll;

    // Overview ruler.
    let ruler = Rect::from_min_max(
        Pos2::new(ruler_left, body.top()),
        Pos2::new(body.right(), body.bottom()),
    );
    let painter = ui.painter();
    let total = pairs.len().max(1);
    for (i, (left, right)) in pairs.iter().enumerate() {
        let y = body.top() + (i as f32 + 0.5) / total as f32 * body.height();
        let is_del = (*left).is_some_and(|li| ctx.rows[li].kind == RowKind::Del);
        let is_add = (*right).is_some_and(|ri| ctx.rows[ri].kind == RowKind::Add);
        if is_del {
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(ruler.left() + 1.0, y - 1.0),
                    Pos2::new(ruler.center().x, y + 1.0),
                ),
                1.0,
                p.rose,
            );
        }
        if is_add {
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(ruler.center().x, y - 1.0),
                    Pos2::new(ruler.right() - 1.0, y + 1.0),
                ),
                1.0,
                p.green,
            );
        }
    }
    if ctx.current_group < ctx.groups {
        if let Some((y0, y1)) = split_group_band(&ctx, &pairs) {
            let frac = |y: f32| (y - body.top()) / (pairs.len() as f32 * ROW_H);
            painter.rect_stroke(
                Rect::from_min_max(
                    Pos2::new(ruler.left(), body.top() + frac(y0) * body.height()),
                    Pos2::new(
                        ruler.right(),
                        (body.top() + frac(y1) * body.height()).max(body.top() + 2.0),
                    ),
                ),
                1.0,
                Stroke::new(1.0, p.jb_accent),
                StrokeKind::Inside,
            );
        }
    }
    scroll.inner
}

/// Vertical pixel band of the current group in the split view.
fn split_group_band(
    ctx: &DiffBodyCtx<'_>,
    pairs: &[(Option<usize>, Option<usize>)],
) -> Option<(f32, f32)> {
    let mut top: Option<f32> = None;
    let mut bottom = 0.0f32;
    for (i, (l, r)) in pairs.iter().enumerate() {
        let in_group = l.map(|idx| ctx.rows[idx].group) == Some(ctx.current_group as u32)
            || r.map(|idx| ctx.rows[idx].group) == Some(ctx.current_group as u32);
        if in_group {
            let y0 = i as f32 * ROW_H;
            let y1 = y0 + ROW_H;
            top = Some(top.unwrap_or(y0).min(y0));
            bottom = bottom.max(y1);
        }
    }
    top.map(|top| (top, bottom))
}

/// Count groups (re-exported for the toolbar).
pub fn groups(hunks: &[DiffHunk]) -> usize {
    count_change_groups(hunks)
}

/// Hunk index containing change group `group` (for per-hunk Stage/Rollback).
pub fn hunk_of_group(hunks: &[DiffHunk], group: usize) -> Option<usize> {
    let mut g = 0usize;
    for (hi, hunk) in hunks.iter().enumerate() {
        let mut in_group = false;
        for line in &hunk.lines {
            if line.kind == DiffLineKind::Context {
                in_group = false;
            } else if !in_group {
                if g == group {
                    return Some(hi);
                }
                g += 1;
                in_group = true;
            }
        }
    }
    None
}
