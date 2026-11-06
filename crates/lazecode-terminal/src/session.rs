use std::io::{Read, Write};
#[cfg(unix)]
use std::os::fd::FromRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crossbeam_channel::Sender;
use parking_lot::{FairMutex, Mutex};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize};
use rio_vt::ansi::CursorShape as RioCursorShape;
use rio_vt::config::colors::{AnsiColor, NamedColor};
use rio_vt::crosswords::grid::{Dimensions, Scroll};
use rio_vt::crosswords::pos::{Column, Line, Pos};
use rio_vt::crosswords::search::RegexSearch;
use rio_vt::crosswords::square::{CellFlags, ContentTag, Wide};
use rio_vt::crosswords::style::Style;
use rio_vt::crosswords::{Crosswords, CrosswordsSize, Mode as RioMode};
use rio_vt::event::{EventListener, RioEvent, WindowId};
use rio_vt::performer::handler::Processor;

use crate::next_id;

pub trait Notifier: Send + Sync {
    fn notify(&self);
}

/// Fixed identifier for the single-window embed. `rio-vt` routes events by
/// `WindowId`; LazeCode keeps one window per process, and sessions are
/// addressed via the app's own session ids.
static WINDOW_ID: std::sync::LazyLock<WindowId> = std::sync::LazyLock::new(|| WindowId::from(0));
const ROUTE_ID: usize = 0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SessionKind {
    #[default]
    Shell,
    Agent,
    Task,
}

pub struct SpawnOptions {
    pub shell: Option<String>,
    pub command: Option<String>,
    pub cwd: PathBuf,
    pub cols: u16,
    pub rows: u16,
    pub scrollback: usize,
    pub notifier: Option<Arc<dyn Notifier>>,
    pub kind: SessionKind,
}

impl Default for SpawnOptions {
    fn default() -> Self {
        Self {
            shell: None,
            command: None,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            cols: 100,
            rows: 30,
            scrollback: 10_000,
            notifier: None,
            kind: SessionKind::Shell,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modes {
    pub alt_screen: bool,
    pub app_cursor: bool,
    pub bracketed_paste: bool,
    pub mouse_click: bool,
    pub mouse_drag: bool,
    pub mouse_motion: bool,
    pub sgr_mouse: bool,
    pub show_cursor: bool,
    pub kitty_keyboard: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorSpec {
    #[default]
    DefaultFg,
    DefaultBg,
    Cursor,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

pub const ATTR_BOLD: u16 = 1 << 0;
pub const ATTR_ITALIC: u16 = 1 << 1;
pub const ATTR_UNDERLINE: u16 = 1 << 2;
pub const ATTR_STRIKEOUT: u16 = 1 << 3;
pub const ATTR_INVERSE: u16 = 1 << 4;
pub const ATTR_DIM: u16 = 1 << 5;
pub const ATTR_HIDDEN: u16 = 1 << 6;
pub const ATTR_WIDE: u16 = 1 << 7;
pub const ATTR_SPACER: u16 = 1 << 8;
pub const ATTR_LEADING_SPACER: u16 = 1 << 9;

#[derive(Clone, Default)]
pub struct CellData {
    pub ch: char,
    /// Full grapheme for cells carrying combining marks / ZWJ sequences.
    pub text: Option<Arc<str>>,
    pub fg: ColorSpec,
    pub bg: ColorSpec,
    pub attrs: u16,
}

impl CellData {
    pub fn display_text(&self) -> &str {
        self.text.as_deref().unwrap_or("")
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CursorView {
    pub row: usize,
    pub col: usize,
    pub shape: rio_vt::ansi::CursorShape,
    pub blinking: bool,
}

#[derive(Clone, Default)]
pub struct Snapshot {
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<CellData>,
    pub cursor: Option<CursorView>,
    pub history_size: usize,
    pub display_offset: usize,
    pub modes: Modes,
    pub title: Option<String>,
    pub bell: u32,
}

impl Snapshot {
    pub fn cell(&self, row: usize, col: usize) -> &CellData {
        static EMPTY: CellData = CellData {
            ch: ' ',
            text: None,
            fg: ColorSpec::DefaultFg,
            bg: ColorSpec::DefaultBg,
            attrs: 0,
        };
        if row >= self.rows || col >= self.cols {
            return &EMPTY;
        }
        self.cells.get(row * self.cols + col).unwrap_or(&EMPTY)
    }

    /// Absolute terminal line for a visible row (negative values are history).
    pub fn absolute_line(&self, row: usize) -> i32 {
        row as i32 - self.display_offset as i32
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TerminalMatch {
    /// Absolute grid line (negative = scrollback).
    pub line: i32,
    pub col: usize,
    pub len: usize,
}

pub(crate) struct Shared {
    title: Mutex<Option<String>>,
    bell: AtomicU32,
    running: AtomicBool,
    exit_code: AtomicI32,
    last_output_ms: AtomicU64,
    last_notify_ms: AtomicU64,
    wake: AtomicU64,
    size: Mutex<(u16, u16)>,
    notifier: Option<Arc<dyn Notifier>>,
}

impl Shared {
    /// Wake the UI for output. High-throughput sessions can produce tens of
    /// thousands of PTY reads per second; unthrottled wakes saturate the event
    /// loop and starve input handling (the "long output freezes" symptom).
    /// Reader-batch wakes are coalesced to at most one per 16ms — the frame
    /// rate the UI repaints at anyway — while explicit state changes (swap-in
    /// of a child exit, kill, scroll) always go through immediately.
    fn notify_output(&self) {
        self.wake.fetch_add(1, Ordering::Relaxed);
        let now = now_ms();
        let last = self.last_notify_ms.load(Ordering::Relaxed);
        if now.saturating_sub(last) < 16 {
            return;
        }
        if self
            .last_notify_ms
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            if let Some(notifier) = &self.notifier {
                notifier.notify();
            }
        }
    }

    fn notify(&self) {
        self.wake.fetch_add(1, Ordering::Relaxed);
        if let Some(notifier) = &self.notifier {
            notifier.notify();
        }
    }
}

/// rio-vt calls this proxy on the parser thread with OSC/PTY replies. It
/// forwards title changes, bells, wakeup and PTY writes back into `Shared`.
struct Proxy {
    shared: Arc<Shared>,
    writer: Sender<Vec<u8>>,
}

impl Proxy {
    fn reply(&self, bytes: Vec<u8>) {
        // The writer thread is bounded; a terminal query reply is tiny and
        // must never block the reader thread if the UI is stalled.
        let _ = self.writer.try_send(bytes);
    }
}

impl EventListener for Proxy {
    fn send_global_event(&self, event: RioEvent) {
        match event {
            RioEvent::Title(_, title) => {
                *self.shared.title.lock() = Some(title);
                self.shared.notify();
            }
            RioEvent::ResetTitle => {
                *self.shared.title.lock() = None;
                self.shared.notify();
            }
            RioEvent::Bell(_) => {
                self.shared.bell.fetch_add(1, Ordering::Relaxed);
                self.shared.notify();
            }
            RioEvent::ChildExited(_, _) => {
                self.shared.running.store(false, Ordering::Relaxed);
                self.shared.notify();
            }
            RioEvent::PtyWrite(_, text) => self.reply(text.into_bytes()),
            RioEvent::CurrentDirectoryChanged(_) => self.shared.notify(),
            // OSC 52 writes/reads stay disabled: the product does not allow a
            // program to read or write the system clipboard silently.
            _ => {}
        }
    }

    fn send_event(&self, event: RioEvent, _id: WindowId) {
        self.send_global_event(event);
    }
}

// OSC 4-style color query coding (OSC 4 replies) stays in rio-vt's handler;
// this palette is kept for the future OSC 4 mapping.

fn color_spec(color: AnsiColor) -> ColorSpec {
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

fn style_with_defaults(fg: AnsiColor, bg: AnsiColor) -> Style {
    Style {
        fg,
        bg,
        underline_color: None,
        flags: rio_vt::crosswords::style::StyleFlags::empty(),
    }
}

fn style_spec(style: &Style) -> (ColorSpec, ColorSpec, u16) {
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
fn style_of(
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

fn fg_of(cell: rio_vt::crosswords::square::Square, styles: &[Style]) -> ColorSpec {
    style_of(cell, styles).0
}

fn bg_of(cell: rio_vt::crosswords::square::Square, styles: &[Style]) -> ColorSpec {
    style_of(cell, styles).1
}

pub struct Session {
    pub id: u64,
    pub kind: SessionKind,
    pub command_label: String,
    pub shell: String,
    pub cwd: PathBuf,
    created_ms: u64,
    term: Arc<FairMutex<Crosswords<Proxy>>>,
    dirty_rows: Mutex<Vec<i32>>,
    shared: Arc<Shared>,
    writer: Sender<Vec<u8>>,
    reader_cancel: Arc<AtomicBool>,
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    pgid: Option<i32>,
    size: Mutex<(u16, u16)>,
}

fn modes_from(mode: RioMode) -> Modes {
    Modes {
        alt_screen: mode.contains(RioMode::ALT_SCREEN),
        app_cursor: mode.contains(RioMode::APP_CURSOR),
        bracketed_paste: mode.contains(RioMode::BRACKETED_PASTE),
        mouse_click: mode.contains(RioMode::MOUSE_REPORT_CLICK),
        mouse_drag: mode.contains(RioMode::MOUSE_DRAG),
        mouse_motion: mode.contains(RioMode::MOUSE_MOTION),
        sgr_mouse: mode.contains(RioMode::SGR_MOUSE),
        show_cursor: mode.contains(RioMode::SHOW_CURSOR),
        kitty_keyboard: mode.contains(RioMode::DISAMBIGUATE_ESC_CODES)
            || mode.contains(RioMode::REPORT_ALL_KEYS_AS_ESC),
    }
}

impl Session {
    pub fn is_running(&self) -> bool {
        self.shared.running.load(Ordering::Relaxed)
    }

    pub fn exit_code(&self) -> Option<i32> {
        let code = self.shared.exit_code.load(Ordering::Relaxed);
        if self.is_running() || code == i32::MIN {
            None
        } else {
            Some(code)
        }
    }

    pub fn title(&self) -> Option<String> {
        self.shared.title.lock().clone()
    }

    pub fn bell(&self) -> u32 {
        self.shared.bell.load(Ordering::Relaxed)
    }

    pub fn wake_counter(&self) -> u64 {
        self.shared.wake.load(Ordering::Relaxed)
    }

    pub fn last_output_ms(&self) -> Option<u64> {
        let value = self.shared.last_output_ms.load(Ordering::Relaxed);
        if value == 0 {
            None
        } else {
            Some(now_ms().saturating_sub(value))
        }
    }

    pub fn age_secs(&self) -> u64 {
        now_ms().saturating_sub(self.created_ms) / 1000
    }

    pub fn size(&self) -> (u16, u16) {
        *self.size.lock()
    }

    pub fn modes(&self) -> Modes {
        modes_from(self.term.lock().mode())
    }

    pub fn write(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        // Never block the UI thread on a full PTY input queue. The bounded
        // writer queue is drained by a dedicated thread.
        if self.writer.try_send(bytes.to_vec()).is_err() {
            log::warn!(
                "terminal input queue is full; dropping {} bytes",
                bytes.len()
            );
        }
    }

    pub fn send_text(&self, text: &str) {
        let bracketed = self.modes().bracketed_paste;
        self.write(&crate::input::encode_paste(text, bracketed));
    }

    pub fn send_key(&self, stroke: crate::input::KeyStroke) {
        let modes = self.modes();
        if let Some(bytes) = crate::input::encode_key(stroke, &modes) {
            self.write(&bytes);
        }
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let cols = cols.max(10);
        let rows = rows.max(2);
        if *self.size.lock() == (cols, rows) {
            return;
        }
        {
            let mut term = self.term.lock();
            term.resize(CrosswordsSize::new(cols as usize, rows as usize));
            // Re-render everything once the grid is reflowed.
            *self.dirty_rows.lock() = Vec::new();
            term.mark_fully_damaged();
        }
        if let Some(master) = self.master.lock().as_ref() {
            let _ = master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
        *self.size.lock() = (cols, rows);
        *self.shared.size.lock() = (cols, rows);
        self.shared.notify();
    }

    pub fn snapshot_into(&self, snap: &mut Snapshot) {
        let term = self.term.lock();
        let grid = &term.grid;
        let cols = grid.columns();
        let rows = grid.screen_lines();
        let display_offset = grid.display_offset();
        snap.cols = cols;
        snap.rows = rows;
        snap.history_size = term.history_size();
        snap.display_offset = display_offset;
        snap.bell = self.shared.bell.load(Ordering::Relaxed);
        snap.title = self.shared.title.lock().clone();
        snap.modes = modes_from(term.mode());
        snap.cells.resize(cols * rows, CellData::default());
        snap.cells.fill(CellData {
            ch: '\0',
            text: None,
            fg: ColorSpec::DefaultFg,
            bg: ColorSpec::DefaultBg,
            attrs: 0,
        });
        let styles = grid.styles();
        let extras = &grid.extras_table;
        for row in 0..rows {
            let line = Line(row as i32 - display_offset as i32);
            let grid_row = &grid[line];
            for col in 0..cols {
                let cell = grid_row[Column(col)];
                let ch = cell.c();
                if ch != '\0' || cell.content_tag() == ContentTag::Codepoint {
                    let mut attrs = 0u16;
                    let wide = cell.wide();
                    match wide {
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
                                snap.cells[row * cols + col] = CellData {
                                    ch,
                                    text: Some(Arc::<str>::from(buffer)),
                                    fg: fg_of(cell, styles),
                                    bg: bg_of(cell, styles),
                                    attrs,
                                };
                                continue;
                            }
                        }
                    }
                    let (fg, bg, style_attrs) = style_of(cell, styles);
                    snap.cells[row * cols + col] = CellData {
                        ch,
                        text: None,
                        fg,
                        bg,
                        attrs: attrs | style_attrs,
                    };
                }
            }
        }

        let cursor = term.cursor();
        let cursor_row = cursor.pos.row.0 + display_offset as i32;
        let cursor_visible = term.mode().contains(RioMode::SHOW_CURSOR)
            && cursor_row >= 0
            && (cursor_row as usize) < rows;
        snap.cursor = if cursor_visible {
            Some(CursorView {
                row: cursor_row as usize,
                col: cursor.pos.col.0.min(cols.saturating_sub(1)),
                shape: term.cursor_shape,
                blinking: term.blinking_cursor,
            })
        } else {
            None
        };
    }

    pub fn scroll_delta(&self, delta: i32) {
        if delta == 0 {
            return;
        }
        {
            let mut term = self.term.lock();
            term.scroll_display(Scroll::Delta(delta));
        }
        self.shared.notify();
    }

    pub fn scroll_page(&self, up: bool) {
        {
            let mut term = self.term.lock();
            term.scroll_display(if up { Scroll::PageUp } else { Scroll::PageDown });
        }
        self.shared.notify();
    }

    pub fn scroll_top(&self) {
        {
            let mut term = self.term.lock();
            term.scroll_display(Scroll::Top);
        }
        self.shared.notify();
    }

    pub fn scroll_bottom(&self) {
        {
            let mut term = self.term.lock();
            term.scroll_display(Scroll::Bottom);
        }
        self.shared.notify();
    }

    pub fn scroll_to_line(&self, absolute_line: i32) {
        let mut term = self.term.lock();
        let history = term.history_size() as i32;
        let target = (-absolute_line).clamp(0, history);
        let current = term.grid.display_offset() as i32;
        if target != current {
            term.scroll_display(Scroll::Delta(target - current));
        }
    }

    /// Case-insensitive search across scrollback + screen.
    ///
    /// Walks the whole grid with rio-vt's DFAs (`regex_search_right` from
    /// the topmost history line through the last screen line); each hit maps
    /// back to an absolute line index (negative = history) for the UI.
    pub fn search(&self, query: &str) -> Vec<TerminalMatch> {
        if query.is_empty() {
            return Vec::new();
        }
        let term = self.term.lock();
        let cols = term.grid.columns();
        let rows = term.grid.screen_lines();
        let history = term.history_size() as i32;
        let mut regex = match RegexSearch::new(query) {
            Ok(regex) => regex,
            // Invalid regex: fall back to plain literal scan.
            Err(_) => return self.search_literal(query),
        };
        let start = Pos {
            row: Line(-history),
            col: Column(0),
        };
        let end = Pos {
            row: Line(rows as i32 - 1),
            col: Column(cols.saturating_sub(1)),
        };
        let mut matches = Vec::new();
        // Origin: scan starts at `start` and stops at `end`; the ring wraps
        // once past `end`, so a loop that always advances and clamps to the
        // [start, end] window bounds the enumeration deterministically.
        let mut origin = start;
        while let Some(hit) = term.regex_search_right(&mut regex, origin, end) {
            let line = hit.start().row.0;
            if line < start.row.0 || line > end.row.0 {
                break;
            }
            let start_col = hit.start().col.0;
            let len = hit.end().col.0.saturating_sub(start_col) + 1;
            matches.push(TerminalMatch {
                line,
                col: start_col.min(cols.saturating_sub(1)),
                len,
            });
            if matches.len() >= 2_000 {
                break;
            }
            // Advance origin just past this hit; same-row hits chain, and one
            // past the end column rolls to the next line's column 0 via the
            // grid's own cursor math in regex_search_right's reverse DFA.
            if hit.end().col.0 + 1 >= cols {
                if hit.end().row.0 >= end.row.0 {
                    break;
                }
                origin = Pos {
                    row: Line(hit.end().row.0 + 1),
                    col: Column(0),
                };
            } else {
                origin = Pos {
                    row: hit.end().row,
                    col: Column(hit.end().col.0 + 1),
                };
            }
            if origin.row.0 > end.row.0 {
                break;
            }
        }
        matches
    }

    /// Literal byte scan used when the query is not a valid regex.
    fn search_literal(&self, query: &str) -> Vec<TerminalMatch> {
        let needle = query.to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        let term = self.term.lock();
        let cols = term.grid.columns();
        let rows = term.grid.screen_lines();
        let history = term.history_size() as i32;
        let mut matches = Vec::new();
        for line_index in -history..rows as i32 {
            let line = Line(line_index);
            let grid_row = &term.grid[line];
            let mut text = String::with_capacity(cols);
            let mut columns = Vec::with_capacity(cols);
            for col in 0..cols {
                let cell = grid_row[Column(col)];
                if cell.wide() == Wide::Spacer {
                    continue;
                }
                let ch = cell.c();
                columns.push(col);
                text.push(ch);
                if cell.contains_cell_flag(CellFlags::GRAPHEME) {
                    if let Some(ex) = term.grid.extras_table.get(cell.extras_id().unwrap_or(0)) {
                        for zw in &ex.zerowidth {
                            text.push(*zw);
                        }
                    }
                }
            }
            let hay_l = text.to_lowercase();
            if hay_l.len() != text.len() {
                for (start_col, _) in text.char_indices() {
                    if text[start_col..].to_lowercase().starts_with(&needle) {
                        matches.push(TerminalMatch {
                            line: line_index,
                            col: columns
                                .get(text[..start_col].chars().count())
                                .copied()
                                .unwrap_or(0),
                            len: needle.chars().count(),
                        });
                        if matches.len() >= 2_000 {
                            return matches;
                        }
                    }
                }
            } else {
                for (byte_pos, _) in hay_l.match_indices(&needle) {
                    let char_pos = hay_l[..byte_pos].chars().count();
                    matches.push(TerminalMatch {
                        line: line_index,
                        col: columns.get(char_pos).copied().unwrap_or(0),
                        len: needle.chars().count(),
                    });
                    if matches.len() >= 2_000 {
                        return matches;
                    }
                }
            }
        }
        matches
    }

    fn terminate(&self) {
        let was_running = self.shared.running.swap(false, Ordering::SeqCst);
        self.reader_cancel.store(true, Ordering::SeqCst);
        if was_running {
            let mut groups = Vec::new();
            if let Some(pgid) = self.pgid.filter(|pgid| *pgid > 1) {
                groups.push(pgid);
            }
            if let Some(pgid) = self
                .master
                .lock()
                .as_ref()
                .and_then(|master| master.process_group_leader())
                .filter(|pgid| *pgid > 1)
            {
                groups.push(pgid);
            }
            groups.sort_unstable();
            groups.dedup();
            for pgid in groups {
                unsafe {
                    libc::kill(-pgid, libc::SIGHUP);
                    libc::kill(-pgid, libc::SIGKILL);
                }
            }
            let _ = self.killer.lock().kill();
        }
        // Closing the master forces a terminal hangup for processes that
        // ignored the signals above; the reader has its own cancellation flag.
        let _ = self.master.lock().take();
        self.shared.notify();
    }

    pub fn kill(&self) {
        self.terminate();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.terminate();
    }
}

pub fn spawn(options: SpawnOptions) -> anyhow::Result<Session> {
    let cols = options.cols.max(10);
    let rows = options.rows.max(2);
    let pty_system = portable_pty::native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let shell = options
        .shell
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("SHELL").ok().filter(|s| !s.trim().is_empty()))
        .unwrap_or_else(|| "/bin/zsh".to_string());

    let mut command = CommandBuilder::new(&shell);
    if let Some(task) = options.command.as_deref().filter(|s| !s.trim().is_empty()) {
        command.arg("-lc");
        command.arg(task);
    } else {
        command.arg("-l");
    }
    command.cwd(&options.cwd);
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command.env("TERM_PROGRAM", "LazeCode");
    command.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
    command.env("LAZECODE", "1");
    command.env("SHELL", &shell);

    let mut child = pair.slave.spawn_command(command)?;
    drop(pair.slave);
    let killer = child.clone_killer();
    let pgid = pair.master.process_group_leader();

    #[cfg(unix)]
    let reader_fd = {
        let fd = pair
            .master
            .as_raw_fd()
            .ok_or_else(|| anyhow::anyhow!("PTY has no file descriptor"))?;
        let dup = unsafe { libc::dup(fd) };
        if dup < 0 {
            return Err(anyhow::anyhow!(
                "cannot duplicate PTY reader: {}",
                std::io::Error::last_os_error()
            ));
        }
        let flags = unsafe { libc::fcntl(dup, libc::F_GETFL) };
        if flags >= 0 {
            unsafe {
                libc::fcntl(dup, libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
        }
        dup
    };
    #[cfg(unix)]
    let reader = unsafe { std::fs::File::from_raw_fd(reader_fd) };
    #[cfg(not(unix))]
    let reader = pair.master.try_clone_reader()?;

    let writer: Box<dyn Write + Send> = pair.master.take_writer()?;
    let (writer_tx, writer_rx) = crossbeam_channel::bounded::<Vec<u8>>(1_024);
    std::thread::Builder::new()
        .name("lazecode-term-write".into())
        .spawn(move || {
            let mut writer = writer;
            while let Ok(bytes) = writer_rx.recv() {
                if writer.write_all(&bytes).is_err() {
                    break;
                }
                let _ = writer.flush();
            }
        })?;

    let shared = Arc::new(Shared {
        title: Mutex::new(None),
        bell: AtomicU32::new(0),
        running: AtomicBool::new(true),
        exit_code: AtomicI32::new(i32::MIN),
        last_output_ms: AtomicU64::new(0),
        last_notify_ms: AtomicU64::new(0),
        wake: AtomicU64::new(0),
        size: Mutex::new((cols, rows)),
        notifier: options.notifier.clone(),
    });

    let reader_cancel = Arc::new(AtomicBool::new(false));
    let proxy = Proxy {
        shared: shared.clone(),
        writer: writer_tx.clone(),
    };
    let term = Crosswords::new(
        CrosswordsSize::new(cols as usize, rows as usize),
        RioCursorShape::Block,
        proxy,
        *WINDOW_ID,
        ROUTE_ID,
        options.scrollback.clamp(1_000, 200_000),
    );
    let term = Arc::new(FairMutex::new(term));
    let dirty_rows = Mutex::new(Vec::new());

    // Reader thread: owns the parser and drives the Crosswords grid. On Unix
    // the reader fd is non-blocking and polled with a timeout so teardown can
    // cancel it even when a daemonized child keeps the slave side open.
    let reader_term = term.clone();
    let reader_shared = shared.clone();
    let reader_cancel_thread = reader_cancel.clone();
    std::thread::Builder::new()
        .name("lazecode-term-read".to_string())
        .spawn(move || {
            let mut processor: Processor = Processor::default();
            let mut reader = reader;
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                if reader_cancel_thread.load(Ordering::Relaxed) {
                    break;
                }
                #[cfg(unix)]
                {
                    let mut pollfd = libc::pollfd {
                        fd: reader_fd,
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    let ready = unsafe { libc::poll(&mut pollfd, 1, 100) };
                    if ready < 0 {
                        if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
                        {
                            continue;
                        }
                        break;
                    }
                    if ready == 0 {
                        continue;
                    }
                    if pollfd.revents
                        & (libc::POLLIN | libc::POLLHUP | libc::POLLERR | libc::POLLNVAL)
                        == 0
                    {
                        continue;
                    }
                }
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        reader_shared
                            .last_output_ms
                            .store(now_ms().max(1), Ordering::Relaxed);
                        {
                            let mut term = reader_term.lock();
                            processor.advance(&mut *term, &buffer[..n]);
                            drop(term);
                        }
                        reader_shared.notify_output();
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => continue,
                    Err(_) => break,
                }
            }
            reader_shared.running.store(false, Ordering::Relaxed);
            reader_shared.notify();
        })?;

    // Wait thread: reaps the child so no zombie survives a closed session.
    let wait_shared = shared.clone();
    std::thread::Builder::new()
        .name("lazecode-term-wait".into())
        .spawn(move || {
            let status = child.wait();
            let code = match status {
                Ok(status) => status.exit_code() as i32,
                Err(_) => -1,
            };
            wait_shared.exit_code.store(code, Ordering::Relaxed);
            wait_shared.running.store(false, Ordering::Relaxed);
            wait_shared.notify();
        })?;

    let command_label = options.command.clone().unwrap_or_else(|| {
        std::path::Path::new(&shell)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "shell".to_string())
    });

    let master: Box<dyn MasterPty + Send> = pair.master;
    Ok(Session {
        id: next_id(),
        kind: options.kind,
        command_label,
        shell,
        cwd: options.cwd,
        created_ms: now_ms(),
        term,
        dirty_rows,
        shared,
        writer: writer_tx,
        reader_cancel,
        master: Mutex::new(Some(master)),
        killer: Mutex::new(killer),
        pgid,
        size: Mutex::new((cols, rows)),
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawns_shell_and_renders_prompt() {
        let options = SpawnOptions {
            cwd: std::env::temp_dir(),
            cols: 80,
            rows: 12,
            command: Some("printf 'hello-terminal\\n'".to_string()),
            ..Default::default()
        };
        let session = spawn(options).expect("spawn");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        let mut snap = Snapshot::default();
        let mut found = false;
        while std::time::Instant::now() < deadline {
            session.snapshot_into(&mut snap);
            let mut text = String::new();
            for row in 0..snap.rows {
                for col in 0..snap.cols {
                    text.push(snap.cell(row, col).ch);
                }
                text.push('\n');
            }
            if text.contains("hello-terminal") {
                found = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(found, "terminal output not observed");
    }

    #[test]
    fn cjk_wide_character_attrs() {
        let options = SpawnOptions {
            cwd: std::env::temp_dir(),
            cols: 80,
            rows: 12,
            command: Some("printf '这是一段测试\\n'".to_string()),
            ..Default::default()
        };
        let session = spawn(options).expect("spawn");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        let mut snap = Snapshot::default();
        let mut found = false;
        while std::time::Instant::now() < deadline {
            session.snapshot_into(&mut snap);
            for row in 0..snap.rows {
                if snap.cell(row, 0).ch == '这' {
                    println!(
                        "cell(0, 0): ch='{}', attrs={:#b}",
                        snap.cell(row, 0).ch,
                        snap.cell(row, 0).attrs
                    );
                    println!(
                        "cell(0, 1): ch='{}', attrs={:#b}",
                        snap.cell(row, 1).ch,
                        snap.cell(row, 1).attrs
                    );
                    println!(
                        "cell(0, 2): ch='{}', attrs={:#b}",
                        snap.cell(row, 2).ch,
                        snap.cell(row, 2).attrs
                    );
                    found = true;
                    break;
                }
            }
            if found {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(found);
    }

    #[test]
    fn scrolls_into_history() {
        let options = SpawnOptions {
            cwd: std::env::temp_dir(),
            cols: 60,
            rows: 5,
            command: Some("for i in $(seq 1 40); do echo row-$i; done".to_string()),
            ..Default::default()
        };
        let session = spawn(options).expect("spawn");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            if !session.search("row-40").is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let mut snap = Snapshot::default();
        session.scroll_delta(20);
        session.snapshot_into(&mut snap);
        assert!(snap.display_offset > 0, "history should be scrolled");
        let mut text = String::new();
        for row in 0..snap.rows {
            for col in 0..snap.cols {
                text.push(snap.cell(row, col).ch);
            }
            text.push('\n');
        }
        assert!(
            text.contains("row-"),
            "scrolled viewport should show history: {text}"
        );
        session.scroll_bottom();
        session.snapshot_into(&mut snap);
        assert_eq!(snap.display_offset, 0);
    }

    #[test]
    fn search_regex_classes_match_in_scrollback() {
        // The search box feeds the rio DFA directly: character classes and
        // case-folding work without a lowercase round-trip.
        let options = SpawnOptions {
            cwd: std::env::temp_dir(),
            cols: 60,
            rows: 6,
            command: Some(
                "for i in 1 2 3 4 5 6 7 8 9 10; do echo line-$i; done; echo done-marker"
                    .to_string(),
            ),
            ..Default::default()
        };
        let session = spawn(options).expect("spawn");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            if !session.search("done-marker").is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let hits = session.search("line-[7-9]");
        assert!(!hits.is_empty(), "regex class should match line-7..9");
        // rio-vt smart-case (Alacritty/less convention): a lowercase query is
        // case-insensitive, an uppercase query matches case-sensitively.
        let folded = session.search("done-mark");
        assert!(!folded.is_empty(), "lowercase query should fold case");
        let sensitive = session.search("done-MARK");
        assert!(
            sensitive.is_empty(),
            "uppercase query is case-sensitive: must NOT hit lowercase text"
        );
    }

    #[test]
    fn search_lowercase_length_change_does_not_panic() {
        // İ lowercases to i̇ (byte growth); the snapshot haystack path must
        // not slice by byte offsets that no longer align with chars.
        let options = SpawnOptions {
            cwd: std::env::temp_dir(),
            cols: 40,
            rows: 6,
            command: Some("printf 'İ marker\n'".to_string()),
            ..Default::default()
        };
        let session = spawn(options).expect("spawn");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            let hits = session.search("marker");
            if !hits.is_empty() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("marker not found");
    }

    #[test]
    fn search_finds_text_in_scrollback() {
        let options = SpawnOptions {
            cwd: std::env::temp_dir(),
            cols: 60,
            rows: 6,
            command: Some(
                "for i in 1 2 3 4 5 6 7 8; do echo line-$i; done; echo marker-xyz".to_string(),
            ),
            ..Default::default()
        };
        let session = spawn(options).expect("spawn");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            let hits = session.search("marker-xyz");
            if !hits.is_empty() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("marker not found");
    }
}
