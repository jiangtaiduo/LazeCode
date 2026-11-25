//! VT-to-cell conversion: styles, colours, rows, virtual images and the
//! scrolled-viewport anchor.

use super::*;

pub(super) fn color_spec(color: AnsiColor) -> ColorSpec {
    match color {
        AnsiColor::Spec(rgb) => ColorSpec::Rgb(rgb.r, rgb.g, rgb.b),
        AnsiColor::Indexed(index) => ColorSpec::Indexed(index),
        AnsiColor::Named(named) => {
            let index = match named {
                NamedColor::Black => 0,
                NamedColor::Red => 1,
                NamedColor::Green => 2,
                NamedColor::Yellow => 3,
                NamedColor::Blue => 4,
                NamedColor::Magenta => 5,
                NamedColor::Cyan => 6,
                NamedColor::White => 7,
                NamedColor::LightBlack => 8,
                NamedColor::LightRed => 9,
                NamedColor::LightGreen => 10,
                NamedColor::LightYellow => 11,
                NamedColor::LightBlue => 12,
                NamedColor::LightMagenta => 13,
                NamedColor::LightCyan => 14,
                NamedColor::LightWhite => 15,
                NamedColor::Background => return ColorSpec::DefaultBg,
                NamedColor::Cursor => return ColorSpec::Cursor,
                NamedColor::Foreground
                | NamedColor::LightForeground
                | NamedColor::DimBlack
                | NamedColor::DimRed
                | NamedColor::DimGreen
                | NamedColor::DimYellow
                | NamedColor::DimBlue
                | NamedColor::DimMagenta
                | NamedColor::DimCyan
                | NamedColor::DimWhite
                | NamedColor::DimForeground => return ColorSpec::DefaultFg,
            };
            ColorSpec::Indexed(index)
        }
    }
}

pub(super) fn style_with_defaults(fg: AnsiColor, bg: AnsiColor) -> Style {
    Style {
        fg,
        bg,
        underline_color: None,
        flags: rio_vt::crosswords::style::StyleFlags::empty(),
    }
}

pub(super) fn style_spec(style: &Style) -> (ColorSpec, ColorSpec, u16) {
    let fg = color_spec(style.fg);
    let bg = color_spec(style.bg);
    let flags = style.flags;
    let mut attrs = 0u16;
    if flags.contains(rio_vt::crosswords::style::StyleFlags::BOLD) {
        attrs |= ATTR_BOLD;
    }
    if flags.contains(rio_vt::crosswords::style::StyleFlags::ITALIC) {
        attrs |= ATTR_ITALIC;
    }
    if flags.intersects(rio_vt::crosswords::style::StyleFlags::ALL_UNDERLINES) {
        attrs |= ATTR_UNDERLINE;
    }
    if flags.contains(rio_vt::crosswords::style::StyleFlags::STRIKEOUT) {
        attrs |= ATTR_STRIKEOUT;
    }
    if flags.contains(rio_vt::crosswords::style::StyleFlags::INVERSE) {
        attrs |= ATTR_INVERSE;
    }
    if flags.intersects(rio_vt::crosswords::style::StyleFlags::DIM_BOLD)
        && !flags.contains(rio_vt::crosswords::style::StyleFlags::BOLD)
    {
        attrs |= ATTR_DIM;
    }
    if flags.contains(rio_vt::crosswords::style::StyleFlags::HIDDEN) {
        attrs |= ATTR_HIDDEN;
    }
    (fg, bg, attrs)
}

/// Resolve a cell's (fg, bg, style-attrs). Bg-only cells synthesize a style
/// from the inline background bits instead of misreading them as a style id.
pub(super) fn style_of(
    cell: rio_vt::crosswords::square::Square,
    styles: &[Style],
) -> (ColorSpec, ColorSpec, u16) {
    let style = match cell.content_tag() {
        ContentTag::Codepoint => styles
            .get(cell.style_id() as usize)
            .cloned()
            .unwrap_or_else(|| {
                style_with_defaults(
                    AnsiColor::Named(NamedColor::Foreground),
                    AnsiColor::Named(NamedColor::Background),
                )
            }),
        ContentTag::BgPalette => style_with_defaults(
            AnsiColor::Named(NamedColor::Foreground),
            AnsiColor::Indexed(cell.bg_palette_index()),
        ),
        ContentTag::BgRgb => {
            let (r, g, b) = cell.bg_rgb();
            style_with_defaults(
                AnsiColor::Named(NamedColor::Foreground),
                AnsiColor::Spec(rio_vt::config::colors::ColorRgb { r, g, b }),
            )
        }
    };
    style_spec(&style)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn virtual_runs_into(
    row: &Row<Square>,
    styles: &[Style],
    extras: &ExtrasTable,
    graphics: &rio_vt::ansi::graphics::Graphics,
    screen_line: usize,
    cell_w: f32,
    cell_h: f32,
    out: &mut Vec<VirtualImageRun>,
) {
    let mut current: Option<(IncompletePlacement, usize)> = None;
    let finish = |current: &mut Option<(IncompletePlacement, usize)>,
                  out: &mut Vec<VirtualImageRun>| {
        let Some((incomplete, start_col)) = current.take() else {
            return;
        };
        let run = incomplete.complete();
        let Some(placement) = resolve_virtual_placement(
            &graphics.kitty_virtual_placements,
            run.image_id,
            run.placement_id,
        ) else {
            return;
        };
        let Some(image) = graphics.kitty_images.get(&run.image_id) else {
            return;
        };
        let Some(geometry) = compute_run_geometry(
            &run,
            placement.columns,
            placement.rows,
            image.data.width as u32,
            image.data.height as u32,
            (placement.x, placement.y, placement.width, placement.height),
            cell_w,
            cell_h,
            0.0,
            0.0,
            screen_line,
            start_col,
        ) else {
            return;
        };
        out.push(VirtualImageRun {
            image_id: run.image_id,
            col: geometry.x / cell_w,
            row: geometry.y / cell_h,
            columns: geometry.width / cell_w,
            rows: geometry.height / cell_h,
            source_uv: geometry.source_rect,
        });
    };

    for col in 0..row.len() {
        let cell = row[Column(col)];
        if cell.c() != PLACEHOLDER {
            finish(&mut current, out);
            continue;
        }
        let Some(style) = styles.get(cell.style_id() as usize) else {
            finish(&mut current, out);
            continue;
        };
        let combining = cell
            .extras_id()
            .and_then(|id| extras.get(id))
            .map_or(&[][..], |extras| extras.zerowidth.as_slice());
        let incomplete = IncompletePlacement::from_cell(style.fg, style.underline_color, combining);
        if let Some((run, _)) = current.as_mut() {
            if run.can_append(&incomplete) {
                run.append();
                continue;
            }
            finish(&mut current, out);
        }
        current = Some((incomplete, col));
    }
    finish(&mut current, out);
}

/// Copy one grid row into `out`, expanding graphemes and wide-cell markers.
pub(super) fn fill_row(
    row: &Row<Square>,
    styles: &[Style],
    extras: &ExtrasTable,
    out: &mut [CellData],
) {
    for (col, slot) in out.iter_mut().enumerate() {
        *slot = CellData::default();
        if col >= row.len() {
            continue;
        }
        let cell = row[Column(col)];
        let ch = cell.c();
        if ch == '\0' && cell.content_tag() != ContentTag::Codepoint {
            continue;
        }
        let mut attrs = 0u16;
        match cell.wide() {
            Wide::Wide => attrs |= ATTR_WIDE,
            Wide::Spacer => attrs |= ATTR_SPACER,
            Wide::LeadingSpacer => attrs |= ATTR_LEADING_SPACER,
            Wide::Narrow => {}
        }
        if cell.contains_cell_flag(CellFlags::GRAPHEME) {
            if let Some(ex) = extras.get(cell.extras_id().unwrap_or(0)) {
                if !ex.zerowidth.is_empty() {
                    let mut buffer = String::with_capacity(4 + 4 * ex.zerowidth.len());
                    buffer.push(ch);
                    for zw in &ex.zerowidth {
                        buffer.push(*zw);
                    }
                    let (fg, bg, _) = style_of(cell, styles);
                    *slot = CellData {
                        ch,
                        text: Some(Arc::<str>::from(buffer)),
                        fg,
                        bg,
                        attrs,
                    };
                    continue;
                }
            }
        }
        let (fg, bg, style_attrs) = style_of(cell, styles);
        *slot = CellData {
            ch,
            text: None,
            fg,
            bg,
            attrs: attrs | style_attrs,
        };
    }
}

/// Fill `cells` with the renderable characters of one grid line and `text`
/// with their non-blank subsequence. Both buffers are reused so the anchor
/// scan below stays allocation-free.
pub(super) fn row_text(
    term: &Crosswords<Proxy>,
    line: i32,
    cells: &mut Vec<CellData>,
    text: &mut Vec<char>,
) {
    cells.clear();
    cells.resize(term.grid.columns(), CellData::default());
    fill_row(
        &term.grid[Line(line)],
        term.grid.styles(),
        &term.grid.extras_table,
        cells,
    );
    text.clear();
    text.extend(
        cells
            .iter()
            .map(|cell| cell.ch)
            .filter(|ch| !ch.is_whitespace() && *ch != '\0'),
    );
}

/// `true` when `line` continues the row above it (that row ends with a
/// wrapline flag). Continuation rows merge into their predecessor on a
/// widening reflow, so their text no longer starts a row and a prefix match
/// would miss them.
pub(super) fn is_wrap_continuation(term: &Crosswords<Proxy>, line: i32, hot: usize) -> bool {
    if -line >= hot as i32 {
        // Nothing above; the oldest row cannot continue anything.
        return false;
    }
    let prev = &term.grid[Line(line - 1)];
    let last = prev.len().saturating_sub(1);
    prev.len() > 0 && prev[Column(last)].wrapline()
}

/// Choose the re-anchor target for a scrolled viewport top (`pinned`).
///
/// Returns `(rows_below_top, signature, span)`: the first row at or below the
/// viewport top that neither continues the row above it nor is blank, its
/// signature, and the normalized text of the rows between the viewport top
/// and that anchor. A logical-line start keeps its text at the row start
/// through any rewrap, so the post-resize scan can match it by prefix; the
/// span lets the exact viewport top be restored afterwards even when the
/// rows between top and anchor merge or split.
pub(super) fn viewport_anchor(
    term: &Crosswords<Proxy>,
    pinned: usize,
    hot: usize,
    cells: &mut Vec<CellData>,
    text: &mut Vec<char>,
) -> Option<(usize, Vec<char>, Vec<char>)> {
    // Line(0) is the top live row; the viewport top sits `pinned` rows above
    // it. Only history rows (Line ≤ -1) can carry the anchor.
    let limit = (term.screen_lines() - 1).min(pinned - 1);
    let mut span = Vec::new();
    for below in 0..=limit {
        let line = -(pinned as i32) + below as i32;
        row_text(term, line, cells, text);
        // Continuation rows merge into their predecessor on a widening
        // reflow, and blank rows carry nothing to match; both are skipped via
        // the span so the anchor stays a row whose text keeps starting the
        // row through any rewrap.
        if is_wrap_continuation(term, line, hot) || text.len() < 2 {
            span.extend(text.iter().copied());
            continue;
        }
        let signature = text.iter().take(8).copied().collect();
        return Some((below, signature, span));
    }
    None
}

/// Offset of the row still carrying `signature`, scanning outward from
/// `stabilized` (where rio-vt's reflow left the viewport).
///
/// The reflow keeps the live screen anchored, so the anchor row moves by at
/// most the net history change — far past any fixed window on a long
/// scrollback, which is why the caller derives the radius from it. Rows are
/// matched by prefix first and by contained characters second: a merged row
/// only holds its old text mid-row. Nearest to rio's position wins, so
/// repeated log lines resolve to the closest occurrence.
pub(super) fn anchored_row(
    term: &Crosswords<Proxy>,
    stabilized: usize,
    hot: usize,
    radius: usize,
    signature: &[char],
    cells: &mut Vec<CellData>,
    text: &mut Vec<char>,
) -> Option<usize> {
    for require_prefix in [true, false] {
        for delta in 0..=radius as i64 {
            for &signed in &[delta, -delta] {
                let candidate = stabilized as i64 + signed;
                if candidate <= 0 || candidate > hot as i64 {
                    continue;
                }
                row_text(term, -(candidate as i32), cells, text);
                if text.len() < signature.len() {
                    continue;
                }
                let hit = if require_prefix {
                    text.starts_with(signature)
                } else {
                    text.windows(signature.len()).any(|w| w == signature)
                };
                if hit {
                    return Some(candidate as usize);
                }
            }
        }
    }
    None
}
