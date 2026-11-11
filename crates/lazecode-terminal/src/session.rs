use std::io::{Read, Write};
#[cfg(unix)]
use std::os::fd::FromRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crossbeam_channel::Sender;
use parking_lot::{FairMutex, Mutex};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize};
use rio_vt::ansi::graphics::UpdateQueues;
use rio_vt::ansi::CursorShape as RioCursorShape;
use rio_vt::ansi::KeyboardModes as RioKeyboardModes;
use rio_vt::config::colors::{AnsiColor, NamedColor};
use rio_vt::crosswords::grid::row::Row;
use rio_vt::crosswords::grid::{Dimensions, ExtrasTable};
use rio_vt::crosswords::pos::{Column, Line, Pos};
use rio_vt::crosswords::search::RegexSearch;
use rio_vt::crosswords::square::{CellFlags, ContentTag, Square, Wide};
use rio_vt::crosswords::style::Style;
use rio_vt::crosswords::{Crosswords, CrosswordsSize, Mode as RioMode};
use rio_vt::event::{EventListener, RioEvent, WindowId};
use rio_vt::performer::handler::Processor;

use crate::cold::ColdStore;
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

/// KitKat keyboard protocol flags, mirroring the spec's bit values so a
/// program's `CSI > flags u` is reproduced verbatim in key reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KittyKeyboard(u8);

impl KittyKeyboard {
    pub const DISAMBIGUATE_ESC_CODES: Self = Self(1 << 0);
    pub const REPORT_EVENT_TYPES: Self = Self(1 << 1);
    pub const REPORT_ALTERNATE_KEYS: Self = Self(1 << 2);
    pub const REPORT_ALL_KEYS_AS_ESC: Self = Self(1 << 3);
    pub const REPORT_ASSOCIATED_TEXT: Self = Self(1 << 4);

    pub fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    pub fn bits(self) -> u8 {
        self.0
    }

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Any flag is set: the program opted into the protocol.
    pub fn is_active(self) -> bool {
        self.0 != 0
    }
}

impl std::ops::BitOr for KittyKeyboard {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOrAssign for KittyKeyboard {
    fn bitor_assign(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

pub struct SpawnOptions {
    pub shell: Option<String>,
    pub command: Option<String>,
    pub cwd: PathBuf,
    pub cols: u16,
    pub rows: u16,
    /// In-memory scrollback ("hot") line budget for the VT grid.
    pub scrollback: usize,
    /// Cold (disk-spooled) scrollback line budget; 0 disables the spool.
    pub cold_lines: usize,
    /// Cold scrollback byte budget.
    pub cold_bytes: usize,
    /// Kitty graphics image cache byte cap.
    pub graphics_bytes: usize,
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
            cold_lines: 500_000,
            cold_bytes: 64 * 1024 * 1024,
            graphics_bytes: 32 * 1024 * 1024,
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
    /// True when the program enabled a kitty flag that changes key reporting.
    pub kitty_keyboard: bool,
    /// Full kitty keyboard flag set (`CSI > n u` / `CSI = n ; m u` stack top).
    pub kitty: KittyKeyboard,
    /// xterm `modifyOtherKeys` level (0 = disabled, 1 = ambiguous keys,
    /// 2 = all modified keys).
    pub modify_other_keys: u8,
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

#[derive(Clone)]
pub struct CellData {
    pub ch: char,
    /// Full grapheme for cells carrying combining marks / ZWJ sequences.
    pub text: Option<Arc<str>>,
    pub fg: ColorSpec,
    pub bg: ColorSpec,
    pub attrs: u16,
}

impl Default for CellData {
    /// A blank cell: transparent background, default foreground. (`ColorSpec`'s
    /// own default is `DefaultFg`, which would paint the whole cell with the
    /// text color if it were used for `bg`.)
    fn default() -> Self {
        Self {
            ch: '\0',
            text: None,
            fg: ColorSpec::DefaultFg,
            bg: ColorSpec::DefaultBg,
            attrs: 0,
        }
    }
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

/// One kitty graphics image placement in the visible grid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImagePlacement {
    pub image_id: u32,
    /// Grid column of the top-left corner.
    pub col: usize,
    /// Absolute grid line of the top-left corner (see
    /// [`Snapshot::absolute_line`]).
    pub row: i64,
    /// Display size in cells.
    pub columns: u32,
    pub rows: u32,
    /// Source rectangle within the image (raw `x`/`y`/`w`/`h`; 0 = to edge).
    pub source: (u32, u32, u32, u32),
    pub z_index: i32,
}

/// A decoded kitty graphics image ready for texture upload.
#[derive(Clone, Debug)]
pub struct KittyImage {
    pub id: u32,
    pub width: usize,
    pub height: usize,
    /// RGBA8 pixels, `width * height * 4` bytes.
    pub rgba: Arc<[u8]>,
    /// Monotonic generation; changes whenever the pixels change.
    pub generation: u64,
}

#[derive(Clone, Default)]
pub struct Snapshot {
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<CellData>,
    pub cursor: Option<CursorView>,
    /// Total scrollback (cold + hot) lines addressable above the screen.
    pub history_size: usize,
    /// Lines still resident in the VT grid.
    pub hot_history_size: usize,
    /// Lines available from the disk spool.
    pub cold_lines: usize,
    pub display_offset: usize,
    pub modes: Modes,
    pub title: Option<String>,
    pub bell: u32,
    /// Kitty image placements, sorted by z-index (back to front).
    pub graphics: Vec<ImagePlacement>,
    /// Ids of images currently cached by the terminal, for texture pruning.
    pub graphics_images: Vec<u32>,
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

/// Hot/cold scrollback split state.
///
/// The VT grid keeps at most `hot_cap` history lines; anything older is
/// written to `store`. `slack` is the transient headroom the ring may grow
/// into between two observation points (one parser slice can scroll at most
/// one line per byte), so no row can be evicted before it is spooled.
struct SpoolState {
    store: Option<ColdStore>,
    spooled_through: u64,
    hot_cap: usize,
    slack: usize,
}

/// Textures the frontend still has to upload (or drop).
#[derive(Default)]
struct GraphicsQueue {
    images: Vec<KittyImage>,
    generation: u64,
}

pub(crate) struct Shared {
    title: Mutex<Option<String>>,
    bell: AtomicU32,
    running: AtomicBool,
    exit_code: AtomicI32,
    last_output_ms: AtomicU64,
    last_notify_ms: AtomicU64,
    pending_notify: AtomicBool,
    timer_running: AtomicBool,
    wake: AtomicU64,
    size: Mutex<(u16, u16)>,
    /// Lines scrolled above the live screen (0 = live view). Owned here so the
    /// reader thread can pin it while history keeps growing.
    view_offset: AtomicUsize,
    graphics: Mutex<GraphicsQueue>,
    notifier: Option<Arc<dyn Notifier>>,
}

impl Shared {
    /// Wake the UI for output. High-throughput sessions can produce tens of
    /// thousands of PTY reads per second; unthrottled wakes saturate the event
    /// loop and starve input handling (the "long output freezes" symptom).
    /// Reader-batch wakes are coalesced to at most one per 16ms — the frame
    /// rate the UI repaints at anyway — while explicit state changes (swap-in
    /// of a child exit, kill, scroll) always go through immediately.
    ///
    /// A trailing batch inside the window arms a one-shot timer so the final
    /// frame is not lost when output stops (an unfocused console would
    /// otherwise stay visually stale).
    fn notify_output(self: &Arc<Self>) {
        self.wake.fetch_add(1, Ordering::Relaxed);
        let now = now_ms();
        let last = self.last_notify_ms.load(Ordering::Relaxed);
        if now.saturating_sub(last) < 16 {
            self.pending_notify.store(true, Ordering::Relaxed);
            if !self.timer_running.swap(true, Ordering::Relaxed) {
                let shared = Arc::clone(self);
                let _ = std::thread::Builder::new()
                    .name("lazecode-term-wake".to_string())
                    .spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(16));
                        if shared.pending_notify.swap(false, Ordering::Relaxed) {
                            shared.last_notify_ms.store(now_ms(), Ordering::Relaxed);
                            if let Some(notifier) = &shared.notifier {
                                notifier.notify();
                            }
                        }
                        shared.timer_running.store(false, Ordering::Relaxed);
                    });
            }
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
    /// Convert rio's queued kitty images into texture payloads for the UI.
    fn queue_graphics(&self, queues: UpdateQueues) {
        if queues.pending_images.is_empty() {
            return;
        }
        let mut queue = self.shared.graphics.lock();
        for (id, data) in queues.pending_images {
            let width = data.width;
            let height = data.height;
            let rgba: Vec<u8> = match data.color_type {
                rio_graphics::ColorType::Rgba => data.pixels,
                rio_graphics::ColorType::Rgb => {
                    let mut rgba = Vec::with_capacity(data.pixels.len() / 3 * 4);
                    for rgb in data.pixels.chunks(3) {
                        if let [r, g, b] = rgb {
                            rgba.extend_from_slice(&[*r, *g, *b, 0xff]);
                        }
                    }
                    rgba
                }
            };
            if width == 0
                || height == 0
                || rgba.len() < width.saturating_mul(height).saturating_mul(4)
            {
                continue;
            }
            queue.generation += 1;
            let generation = queue.generation;
            queue.images.push(KittyImage {
                id,
                width,
                height,
                rgba: Arc::from(rgba.into_boxed_slice()),
                generation,
            });
        }
        self.shared.notify();
    }

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
            RioEvent::UpdateGraphics { queues, .. } => self.queue_graphics(queues),
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

/// Copy one grid row into `out`, expanding graphemes and wide-cell markers.
fn fill_row(row: &Row<Square>, styles: &[Style], extras: &ExtrasTable, out: &mut [CellData]) {
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

/// Maximum number of bytes fed to the parser between two scrollback spool
/// observations. One byte can scroll at most one line, so a slice this small
/// can never overflow the ring's slack and lose a row before it is spooled.
const PARSE_SLICE: usize = 1024;

/// Move history that fell out of the hot window into the disk spool and shrink
/// the in-memory ring back to its budget.
fn spool_scrollback(
    term: &mut Crosswords<Proxy>,
    spool: &mut SpoolState,
    view_offset: &AtomicUsize,
    total_before: u64,
) {
    let Some(store) = spool.store.as_mut() else {
        return;
    };
    let hot = term.history_size();
    let evicted = term.grid.lines_evicted();
    if hot > spool.hot_cap {
        let floor = evicted + (hot - spool.hot_cap) as u64;
        if spool.spooled_through < evicted {
            // A burst outran the slack (or the alt screen was active): jump
            // the cursor forward so ids stay ordered, leaving a blank gap.
            spool.spooled_through = evicted;
        }
        let first = spool.spooled_through;
        let cols = term.grid.columns();
        let mut rows: Vec<Vec<u8>> = Vec::with_capacity(floor.saturating_sub(first) as usize);
        {
            let grid = &term.grid;
            let styles = grid.styles();
            let extras = &grid.extras_table;
            let mut cells = vec![CellData::default(); cols];
            let mut id = first;
            while id < floor {
                let index = id as i64 - (evicted + hot as u64) as i64;
                if index >= 0 || index < -(hot as i64) {
                    break;
                }
                fill_row(&grid[Line(index as i32)], styles, extras, &mut cells);
                rows.push(crate::cold::encode_row(&cells));
                id += 1;
            }
            spool.spooled_through = id;
        }
        if !rows.is_empty() {
            store.append_rows(first, &rows);
            spool.spooled_through = first + rows.len() as u64;
        }
        // Shrink the ring back to the hot budget. `update_history` also resets
        // the growth limit, so restore the slack headroom right after.
        term.grid.update_history(spool.hot_cap);
        term.grid.update_history(spool.hot_cap + spool.slack);
    }
    // Shrinking the ring leaves `evicted + history` (the absolute id of the
    // first screen row) unchanged, so this only counts lines the slice pushed
    // into history. A viewport scrolled back stays pinned to the same content
    // by following that growth.
    let total_after = term.grid.lines_evicted() + term.history_size() as u64;
    if total_after > total_before && view_offset.load(Ordering::Relaxed) > 0 {
        view_offset.fetch_add((total_after - total_before) as usize, Ordering::Relaxed);
    }
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
    /// Applied grid geometry: columns, rows, cell width, cell height.
    geom: Mutex<(u16, u16, u32, u32)>,
    spool: Arc<Mutex<SpoolState>>,
}

/// Scrollback occupancy across the hot grid and the disk spool.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HistoryStats {
    /// Lines resident in the VT grid.
    pub hot_lines: usize,
    /// Lines available from the disk spool.
    pub cold_lines: usize,
    /// Payload bytes of the spooled lines.
    pub cold_bytes: usize,
    /// Bytes of the spool file on disk (includes not-yet-compacted slack).
    pub cold_file_bytes: u64,
    /// Addressable scrollback lines (hot + cold).
    pub total_lines: usize,
}

fn modes_from(term: &Crosswords<Proxy>) -> Modes {
    let mode = term.mode();
    let keyboard = term.keyboard_mode();
    Modes {
        alt_screen: mode.contains(RioMode::ALT_SCREEN),
        app_cursor: mode.contains(RioMode::APP_CURSOR),
        bracketed_paste: mode.contains(RioMode::BRACKETED_PASTE),
        mouse_click: mode.contains(RioMode::MOUSE_REPORT_CLICK),
        mouse_drag: mode.contains(RioMode::MOUSE_DRAG),
        mouse_motion: mode.contains(RioMode::MOUSE_MOTION),
        sgr_mouse: mode.contains(RioMode::SGR_MOUSE),
        show_cursor: mode.contains(RioMode::SHOW_CURSOR),
        kitty_keyboard: keyboard.contains(RioKeyboardModes::DISAMBIGUATE_ESC_CODES)
            || keyboard.contains(RioKeyboardModes::REPORT_ALL_KEYS_AS_ESC),
        kitty: KittyKeyboard::from_bits(keyboard.bits()),
        modify_other_keys: term.modify_other_keys().unwrap_or(0),
    }
}

/// Fill `out` with the visible kitty image placements in the snapshot's
/// absolute line space. Reuses the caller's buffer so a repainting UI does not
/// allocate per frame.
fn graphics_placements_into(term: &Crosswords<Proxy>, out: &mut Vec<ImagePlacement>) {
    let base = term.grid.lines_evicted() as i64 + term.history_size() as i64;
    out.clear();
    out.extend(
        term.graphics
            .kitty_placements
            .values()
            .filter(|placement| placement.columns > 0 && placement.rows > 0)
            .map(|placement| ImagePlacement {
                image_id: placement.image_id,
                col: placement.dest_col,
                row: placement.dest_row - base,
                columns: placement.columns,
                rows: placement.rows,
                source: (
                    placement.source_x,
                    placement.source_y,
                    placement.source_width,
                    placement.source_height,
                ),
                z_index: placement.z_index,
            }),
    );
    out.sort_by_key(|placement| (placement.z_index, placement.row, placement.col));
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
        let geom = self.geom.lock();
        (geom.0, geom.1)
    }

    pub fn modes(&self) -> Modes {
        modes_from(&self.term.lock())
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
        let (_, _, cell_w, cell_h) = *self.geom.lock();
        self.apply_geometry(cols.max(10), rows.max(2), cell_w, cell_h);
    }

    /// Report the pixel size of one cell. Kitty graphics needs it to turn an
    /// image's pixel size into a cell span; a zero size keeps placements
    /// disabled (rio's behaviour without a renderer).
    pub fn set_cell_size(&self, width: f32, height: f32) {
        let cell_w = width.round().max(1.0) as u32;
        let cell_h = height.round().max(1.0) as u32;
        let (cols, rows, _, _) = *self.geom.lock();
        self.apply_geometry(cols, rows, cell_w, cell_h);
    }

    fn apply_geometry(&self, cols: u16, rows: u16, cell_w: u32, cell_h: u32) {
        let mut geom = self.geom.lock();
        if *geom == (cols, rows, cell_w, cell_h) {
            return;
        }
        let size_changed = geom.0 != cols || geom.1 != rows;
        *geom = (cols, rows, cell_w, cell_h);
        drop(geom);
        {
            let mut term = self.term.lock();
            term.resize(CrosswordsSize::new_with_dimensions(
                cols as usize,
                rows as usize,
                (cols as u32).saturating_mul(cell_w),
                (rows as u32).saturating_mul(cell_h),
                cell_w,
                cell_h,
            ));
            // Re-render everything once the grid is reflowed.
            *self.dirty_rows.lock() = Vec::new();
            term.mark_fully_damaged();
        }
        if size_changed {
            if let Some(master) = self.master.lock().as_ref() {
                let _ = master.resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                });
            }
            *self.shared.size.lock() = (cols, rows);
        }
        self.shared.notify();
    }

    /// Read the grid, the spool and the visible viewport into `snap`.
    ///
    /// Rows older than the hot history are rehydrated from the disk spool, so
    /// the UI addresses one continuous history space no matter where a line
    /// lives.
    pub fn snapshot_into(&self, snap: &mut Snapshot) {
        let term = self.term.lock();
        let grid = &term.grid;
        let cols = grid.columns();
        let rows = grid.screen_lines();
        let hot = term.history_size();
        let hot_base = grid.lines_evicted() + hot as u64;
        let mut spool = self.spool.lock();
        let (cold_lines, first_cold) = match spool.store.as_ref() {
            Some(store) => (store.len(), store.first_id()),
            None => (0, None),
        };
        let total_history = match first_cold {
            Some(first) => hot_base.saturating_sub(first) as usize,
            None => hot,
        };
        let offset = self
            .shared
            .view_offset
            .load(Ordering::Relaxed)
            .min(total_history);
        snap.cols = cols;
        snap.rows = rows;
        snap.hot_history_size = hot;
        snap.cold_lines = cold_lines;
        snap.history_size = total_history;
        snap.display_offset = offset;
        snap.bell = self.shared.bell.load(Ordering::Relaxed);
        snap.title = self.shared.title.lock().clone();
        snap.modes = modes_from(&term);
        graphics_placements_into(&term, &mut snap.graphics);
        snap.graphics_images.clear();
        snap.graphics_images
            .extend(term.graphics.kitty_images.keys().copied());
        snap.graphics_images.sort_unstable();
        snap.cells.resize(cols * rows, CellData::default());

        let styles = grid.styles();
        let extras = &grid.extras_table;
        for row in 0..rows {
            let abs = row as i64 - offset as i64;
            let target = &mut snap.cells[row * cols..(row + 1) * cols];
            if abs < -(hot as i64) {
                target.fill(CellData::default());
                let id = abs + hot_base as i64;
                if id >= 0 {
                    if let Some(store) = spool.store.as_mut() {
                        if let Some(cells) = store.get_by_id(id as u64) {
                            for (slot, cell) in target.iter_mut().zip(cells) {
                                *slot = cell;
                            }
                        }
                    }
                }
            } else {
                fill_row(&grid[Line(abs as i32)], styles, extras, target);
            }
        }

        let cursor = term.cursor();
        let cursor_row = cursor.pos.row.0 + offset as i32;
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

    /// Total addressable history (hot grid + disk spool).
    fn history_span(&self) -> usize {
        let (hot_base, hot) = {
            let term = self.term.lock();
            (
                term.grid.lines_evicted() + term.history_size() as u64,
                term.history_size(),
            )
        };
        let spool = self.spool.lock();
        spool
            .store
            .as_ref()
            .and_then(|store| store.first_id())
            .map(|first| hot_base.saturating_sub(first) as usize)
            .unwrap_or(hot)
    }

    fn store_view_offset(&self, offset: usize) {
        if self.shared.view_offset.swap(offset, Ordering::Relaxed) != offset {
            self.shared.notify();
        }
    }

    pub fn scroll_delta(&self, delta: i32) {
        if delta == 0 {
            return;
        }
        let total = self.history_span();
        let current = self.shared.view_offset.load(Ordering::Relaxed).min(total);
        let next = (current as i64 + delta as i64).clamp(0, total as i64) as usize;
        self.store_view_offset(next);
    }

    pub fn scroll_page(&self, up: bool) {
        let lines = {
            let term = self.term.lock();
            term.screen_lines() as i32
        };
        self.scroll_delta(if up { lines } else { -lines });
    }

    pub fn scroll_top(&self) {
        let total = self.history_span();
        self.store_view_offset(total);
    }

    pub fn scroll_bottom(&self) {
        self.store_view_offset(0);
    }

    pub fn scroll_to_line(&self, absolute_line: i32) {
        let total = self.history_span() as i32;
        let target = (-absolute_line).clamp(0, total) as usize;
        self.store_view_offset(target);
    }

    /// Case-insensitive search across hot scrollback + screen + cold spool.
    ///
    /// Walks the whole grid with rio-vt's DFAs (`regex_search_right` from
    /// the topmost history line through the last screen line), then scans the
    /// spooled rows. Every hit maps back to an absolute line index (negative =
    /// history, including cold history) for the UI.
    pub fn search(&self, query: &str) -> Vec<TerminalMatch> {
        if query.is_empty() {
            return Vec::new();
        }
        let (mut matches, hot_base) = self.search_hot(query);
        let needle = query.to_lowercase();
        let mut spool = self.spool.lock();
        if let Some(store) = spool.store.as_mut() {
            let remaining = 2_000usize.saturating_sub(matches.len());
            store.search(&needle, remaining, |id, col, len| {
                let line = id as i64 - hot_base;
                matches.push(TerminalMatch {
                    line: line.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
                    col,
                    len,
                });
            });
        }
        matches.sort_by_key(|hit| (hit.line, hit.col));
        matches
    }

    /// Hot-grid scan; also returns the id of the first screen row so cold hits
    /// can be placed in the same absolute space.
    fn search_hot(&self, query: &str) -> (Vec<TerminalMatch>, i64) {
        let term = self.term.lock();
        let hot_base = term.grid.lines_evicted() as i64 + term.history_size() as i64;
        let cols = term.grid.columns();
        let rows = term.grid.screen_lines();
        let history = term.history_size() as i32;
        let mut regex = match RegexSearch::new(query) {
            Ok(regex) => regex,
            // Invalid regex: fall back to plain literal scan.
            Err(_) => {
                let needle = query.to_lowercase();
                return (self.search_literal_locked(&term, &needle), hot_base);
            }
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
        (matches, hot_base)
    }

    /// Literal scan used when the query is not a valid regex.
    fn search_literal_locked(&self, term: &Crosswords<Proxy>, needle: &str) -> Vec<TerminalMatch> {
        if needle.is_empty() {
            return Vec::new();
        }
        let cols = term.grid.columns();
        let rows = term.grid.screen_lines();
        let history = term.history_size() as i32;
        let mut matches = Vec::new();
        let mut cells = vec![CellData::default(); cols];
        let styles = term.grid.styles();
        let extras = &term.grid.extras_table;
        let mut spans = Vec::new();
        for line_index in -history..rows as i32 {
            fill_row(&term.grid[Line(line_index)], styles, extras, &mut cells);
            let (text, columns) = crate::cold::row_text(&cells);
            spans.clear();
            crate::cold::find_matches(
                &text,
                &columns,
                needle,
                2_000usize.saturating_sub(matches.len()),
                &mut spans,
            );
            for (col, len) in spans.iter().copied() {
                matches.push(TerminalMatch {
                    line: line_index,
                    col,
                    len,
                });
            }
            if matches.len() >= 2_000 {
                break;
            }
        }
        matches
    }

    /// Scrollback occupancy across the hot grid and the cold spool.
    pub fn history_stats(&self) -> HistoryStats {
        let (hot_base, hot) = {
            let term = self.term.lock();
            (
                term.grid.lines_evicted() + term.history_size() as u64,
                term.history_size(),
            )
        };
        let spool = self.spool.lock();
        match spool.store.as_ref() {
            Some(store) => HistoryStats {
                hot_lines: hot,
                cold_lines: store.len(),
                cold_bytes: store.bytes(),
                cold_file_bytes: store.file_bytes(),
                total_lines: store
                    .first_id()
                    .map(|first| hot_base.saturating_sub(first) as usize)
                    .unwrap_or(hot),
            },
            None => HistoryStats {
                hot_lines: hot,
                total_lines: hot,
                ..Default::default()
            },
        }
    }

    /// Path of the on-disk scrollback spool, when the spool is enabled.
    pub fn cold_path(&self) -> Option<PathBuf> {
        self.spool
            .lock()
            .store
            .as_ref()
            .map(|store| store.path().to_path_buf())
    }

    /// Drop hot and cold scrollback (used by the "clear history" command and
    /// verified by the close path, which removes the spool file).
    pub fn clear_scrollback(&self) {
        let evicted = {
            let mut term = self.term.lock();
            term.grid.clear_history();
            term.grid.lines_evicted()
        };
        let mut spool = self.spool.lock();
        if let Some(store) = spool.store.as_mut() {
            store.clear();
        }
        spool.spooled_through = evicted;
        drop(spool);
        self.shared.view_offset.store(0, Ordering::Relaxed);
        self.shared.notify();
    }

    /// Take newly decoded kitty images for texture upload.
    pub fn take_graphics_updates(&self) -> Vec<KittyImage> {
        std::mem::take(&mut self.shared.graphics.lock().images)
    }

    /// Re-queue the pixels of every *placed* image; the frontend calls this
    /// when it lost its textures (e.g. a fresh egui context) while the session
    /// still holds the image data.
    pub fn resync_graphics(&self) {
        let mut term = self.term.lock();
        if term.graphics.kitty_images.is_empty() {
            return;
        }
        term.graphics.kitty_texture_contents.clear();
        let mut ids: Vec<u32> = term
            .graphics
            .kitty_placements
            .keys()
            .chain(term.graphics.kitty_virtual_placements.keys())
            .map(|(image_id, _)| *image_id)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        for id in ids {
            term.graphics.queue_kitty_upload(id);
        }
        term.send_graphics_updates();
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

const ALLOWED_SHELLS: &[&str] = &["zsh", "bash", "fish", "sh", "nu", "pwsh"];

/// Console programs are user-configured, but only known login shells are
/// accepted; anything else falls back to `/bin/zsh` instead of executing an
/// arbitrary path supplied by repository content or a stale setting.
fn resolve_shell(requested: Option<String>) -> String {
    let requested = requested
        .filter(|shell| !shell.trim().is_empty())
        .or_else(|| {
            std::env::var("SHELL")
                .ok()
                .filter(|shell| !shell.trim().is_empty())
        });
    let Some(candidate) = requested else {
        return "/bin/zsh".to_string();
    };
    let name = std::path::Path::new(&candidate)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if !ALLOWED_SHELLS.contains(&name) {
        log::warn!("refusing unknown shell '{candidate}'; using /bin/zsh");
        return "/bin/zsh".to_string();
    }
    let path = std::path::Path::new(&candidate);
    if path.is_absolute() && path.is_file() {
        return candidate;
    }
    for dir in ["/bin", "/usr/bin", "/opt/homebrew/bin", "/usr/local/bin"] {
        let path = std::path::Path::new(dir).join(name);
        if path.is_file() {
            return path.to_string_lossy().into_owned();
        }
    }
    "/bin/zsh".to_string()
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

    let shell = resolve_shell(options.shell.clone());

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
    // Do not hand the IDE's own credentials to a spawned shell/agent.
    for key in [
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_SESSION_TOKEN",
        "AZURE_CLIENT_SECRET",
        "GOOGLE_APPLICATION_CREDENTIALS",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "GITLAB_TOKEN",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "NPM_TOKEN",
        "NUGET_API_KEY",
        "DOCKER_AUTH_CONFIG",
    ] {
        command.env_remove(key);
    }

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
        // NOTE: do not set O_NONBLOCK here. The reader fd shares its open file
        // description with the PTY master (dup), so `O_NONBLOCK` would also
        // apply to the writer and turn large writes into partial `EAGAIN`
        // writes. Readiness comes from the `poll` below instead.
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
                let mut offset = 0;
                while offset < bytes.len() {
                    match writer.write(&bytes[offset..]) {
                        Ok(0) => break,
                        Ok(written) => offset += written,
                        Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
                        // The PTY input queue is full; retry instead of
                        // discarding the rest of the paste/keystrokes.
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(std::time::Duration::from_millis(1));
                        }
                        Err(_) => return,
                    }
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
        pending_notify: AtomicBool::new(false),
        timer_running: AtomicBool::new(false),
        wake: AtomicU64::new(0),
        size: Mutex::new((cols, rows)),
        view_offset: AtomicUsize::new(0),
        graphics: Mutex::new(GraphicsQueue::default()),
        notifier: options.notifier.clone(),
    });

    let reader_cancel = Arc::new(AtomicBool::new(false));
    let proxy = Proxy {
        shared: shared.clone(),
        writer: writer_tx.clone(),
    };
    let hot_cap = options.scrollback.clamp(16, 200_000);
    // The ring may run ahead of the hot budget by one parse slice, where a
    // byte can scroll at most one line; `spool_scrollback` trims it back after
    // every slice, so no row is evicted before it is captured.
    let slack = PARSE_SLICE;
    let store = if options.cold_lines > 0 && options.cold_bytes > 0 {
        match ColdStore::create(options.cold_lines, options.cold_bytes) {
            Ok(store) => Some(store),
            Err(err) => {
                log::warn!("cold scrollback spool disabled: {err}");
                None
            }
        }
    } else {
        None
    };
    let spool = Arc::new(Mutex::new(SpoolState {
        store,
        spooled_through: 0,
        hot_cap,
        slack,
    }));
    let mut term = Crosswords::new(
        CrosswordsSize::new(cols as usize, rows as usize),
        RioCursorShape::Block,
        proxy,
        *WINDOW_ID,
        ROUTE_ID,
        hot_cap + slack,
    );
    // Bound the decoded kitty image cache to the configured byte cap.
    term.graphics.total_limit = options.graphics_bytes.clamp(256 * 1024, 320 * 1024 * 1024);
    let term = Arc::new(FairMutex::new(term));
    let dirty_rows = Mutex::new(Vec::new());

    // Reader thread: owns the parser and drives the Crosswords grid. On Unix
    // the reader fd is polled with a timeout so teardown can cancel it even
    // when a daemonized child keeps the slave side open.
    let reader_term = term.clone();
    let reader_shared = shared.clone();
    let reader_cancel_thread = reader_cancel.clone();
    let reader_spool = spool.clone();
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
                        let parsed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let mut term = reader_term.lock();
                            let mut spool = reader_spool.lock();
                            for slice in buffer[..n].chunks(PARSE_SLICE) {
                                let total_before =
                                    term.grid.lines_evicted() + term.history_size() as u64;
                                processor.advance(&mut *term, slice);
                                // Spool rows that fell out of the hot window
                                // before the ring needs to evict them itself.
                                spool_scrollback(
                                    &mut term,
                                    &mut spool,
                                    &reader_shared.view_offset,
                                    total_before,
                                );
                            }
                        }));
                        if parsed.is_err() {
                            // A VT parser panic must not leave the session
                            // marked running with a frozen screen.
                            log::error!("terminal parser panicked; closing session");
                            break;
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
        geom: Mutex::new((cols, rows, 0, 0)),
        spool,
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

    fn snapshot_text(snap: &Snapshot) -> String {
        let mut text = String::new();
        for row in 0..snap.rows {
            for col in 0..snap.cols {
                let ch = snap.cell(row, col).ch;
                text.push(if ch == '\0' { ' ' } else { ch });
            }
            text.push('\n');
        }
        text
    }

    fn wait_until(mut predicate: impl FnMut() -> bool, duration: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + duration;
        while std::time::Instant::now() < deadline {
            if predicate() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        false
    }

    fn base64(data: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
        for chunk in data.chunks(3) {
            let b = [
                chunk[0],
                chunk.get(1).copied().unwrap_or(0),
                chunk.get(2).copied().unwrap_or(0),
            ];
            let value = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
            for index in 0..4 {
                if index <= chunk.len() {
                    let bits = (value >> (18 - 6 * index)) & 0x3f;
                    out.push(ALPHABET[bits as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    fn kitty_apc(action: &str, id: u32, width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
        kitty_apc_extra(action, id, width, height, pixels, "")
    }

    fn kitty_apc_extra(
        action: &str,
        id: u32,
        width: u32,
        height: u32,
        pixels: &[u8],
        extra: &str,
    ) -> Vec<u8> {
        let payload = base64(pixels);
        // `q=2` silences the terminal's reply so the echo session used by the
        // tests cannot bounce responses back and forth.
        format!("\x1b_Ga={action}{extra},q=2,f=32,s={width},v={height},i={id};{payload}\x1b\\")
            .into_bytes()
    }

    /// Wait until `cat` is actually echoing, so protocol sequences written by
    /// the test are processed exactly once.
    fn await_echo(session: &Session, snap: &mut Snapshot) {
        let ready = wait_until(
            || {
                session.write(b"LZREADY");
                session.snapshot_into(snap);
                snapshot_text(snap).contains("LZREADY")
            },
            std::time::Duration::from_secs(6),
        );
        assert!(ready, "echo session never became ready");
    }

    #[test]
    fn cold_spool_extends_history_and_rehydrates() {
        let options = SpawnOptions {
            cwd: std::env::temp_dir(),
            cols: 40,
            rows: 8,
            command: Some("for i in $(seq 1 2000); do echo row-$i; done".to_string()),
            // Small hot budget so most history has to live on disk.
            scrollback: 32,
            cold_lines: 10_000,
            cold_bytes: 8 << 20,
            ..Default::default()
        };
        let session = spawn(options).expect("spawn");
        let mut snap = Snapshot::default();
        let done = wait_until(
            || {
                session.snapshot_into(&mut snap);
                snap.cold_lines > 1_500 && session.search("row-2000").iter().any(|m| m.line >= 0)
            },
            std::time::Duration::from_secs(15),
        );
        assert!(
            done,
            "spooled history did not fill: {:?}",
            session.history_stats()
        );

        let stats = session.history_stats();
        // Hot memory plateaus at the configured budget; the rest is on disk.
        assert!(stats.hot_lines <= 32, "hot lines grew: {stats:?}");
        assert!(
            snap.hot_history_size <= 32,
            "snapshot hot lines grew: {stats:?}"
        );
        assert!(stats.cold_lines > 1_500, "cold lines missing: {stats:?}");
        assert!(stats.cold_bytes > 0, "cold bytes missing: {stats:?}");
        assert!(stats.total_lines >= 1_990, "history lost: {stats:?}");
        assert!(session.cold_path().is_some_and(|path| path.exists()));

        // Search sees the cold rows: their absolute line is older than the
        // hot window's oldest line.
        let hot = snap.hot_history_size as i32;
        let hits = session.search("row-1500");
        let cold_hit = hits
            .iter()
            .find(|hit| hit.line < -hot)
            .unwrap_or_else(|| panic!("expected a cold hit, got {hits:?} (hot = {hot})"))
            .to_owned();
        // Jumping to a cold hit places that row at the top of the viewport.
        session.scroll_to_line(cold_hit.line);
        session.snapshot_into(&mut snap);
        let landed = snapshot_text(&snap);
        assert!(
            landed.starts_with("row-1500"),
            "cold hit navigation landed on: {landed:?}"
        );

        // Scrolling to the top rehydrates the oldest page from disk.
        session.scroll_top();
        session.snapshot_into(&mut snap);
        assert!(snap.display_offset > snap.hot_history_size);
        assert_eq!(snap.cold_lines, stats.cold_lines);
        let text = snapshot_text(&snap);
        assert!(
            text.starts_with("row-1"),
            "cold page not rehydrated: {text:?}"
        );
        session.scroll_bottom();
        session.snapshot_into(&mut snap);
        assert_eq!(snap.display_offset, 0);
        assert!(snapshot_text(&snap).contains("row-2000"));

        // Explicit clear drops both tiers.
        session.clear_scrollback();
        assert_eq!(session.history_stats().cold_lines, 0);
        let mut snap = Snapshot::default();
        session.snapshot_into(&mut snap);
        assert_eq!(snap.history_size, 0);
    }

    #[test]
    fn cold_spool_file_is_removed_on_close() {
        let options = SpawnOptions {
            cwd: std::env::temp_dir(),
            cols: 30,
            rows: 4,
            command: Some("for i in $(seq 1 400); do echo row-$i; done".to_string()),
            scrollback: 16,
            cold_lines: 10_000,
            cold_bytes: 8 << 20,
            ..Default::default()
        };
        let session = spawn(options).expect("spawn");
        assert!(
            wait_until(
                || session.history_stats().cold_lines > 0,
                std::time::Duration::from_secs(10)
            ),
            "nothing was spooled"
        );
        let path = session.cold_path().expect("spool enabled");
        assert!(path.exists());
        drop(session);
        assert!(!path.exists(), "closing the session must delete the spool");
    }

    fn echo_session(rows: u16) -> Session {
        let options = SpawnOptions {
            cwd: std::env::temp_dir(),
            cols: 40,
            rows,
            command: Some("stty raw -echo; cat".to_string()),
            ..Default::default()
        };
        spawn(options).expect("spawn")
    }

    #[test]
    fn scrolled_viewport_stays_pinned_while_output_continues() {
        let options = SpawnOptions {
            cwd: std::env::temp_dir(),
            cols: 40,
            rows: 8,
            command: Some("stty raw -echo; cat".to_string()),
            // Force most of the history onto disk so the pin spans tiers.
            scrollback: 32,
            cold_lines: 10_000,
            cold_bytes: 8 << 20,
            ..Default::default()
        };
        let session = spawn(options).expect("spawn");
        let mut snap = Snapshot::default();
        await_echo(&session, &mut snap);

        let mut blob = String::new();
        for line in 1..=200 {
            blob.push_str(&format!("row-{line}\r\n"));
        }
        session.write(blob.as_bytes());
        let arrived = wait_until(
            || {
                session.snapshot_into(&mut snap);
                session
                    .search("row-200")
                    .iter()
                    .any(|hit| hit.line >= -(snap.rows as i32))
            },
            std::time::Duration::from_secs(10),
        );
        assert!(
            arrived,
            "no row-200: hits={:?} text={:?} stats={:?}",
            session.search("row-200"),
            snapshot_text(&snap),
            session.history_stats()
        );

        session.scroll_delta(40);
        session.snapshot_into(&mut snap);
        assert_eq!(snap.display_offset, 40);
        let pinned = snapshot_text(&snap);
        let first_line = pinned.lines().next().unwrap_or("").to_string();
        assert!(first_line.starts_with("row-"), "pinned line: {pinned:?}");

        let mut more = String::new();
        for line in 201..=300 {
            more.push_str(&format!("row-{line}\r\n"));
        }
        session.write(more.as_bytes());
        assert!(wait_until(
            || {
                session.snapshot_into(&mut snap);
                session
                    .search("row-300")
                    .iter()
                    .any(|hit| hit.line >= -(snap.rows as i32))
            },
            std::time::Duration::from_secs(10)
        ));
        assert!(
            snap.display_offset > 40,
            "the viewport must follow the history growth: {}",
            snap.display_offset
        );
        assert_eq!(
            snapshot_text(&snap).lines().next().unwrap_or(""),
            first_line,
            "a scrolled viewport must stay pinned to its content"
        );
        assert!(session.history_stats().cold_lines > 0);
    }

    #[test]
    fn kitty_graphics_are_parsed_placed_and_deleted() {
        let session = echo_session(10);
        session.set_cell_size(8.0, 16.0);
        let pixels = vec![
            0xff, 0x00, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, //
            0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        ];
        // An explicit placement id makes a retransmission replace the same
        // placement instead of stacking another one (rio allocates a fresh
        // internal id for `p=0`).
        let sequence = kitty_apc_extra("T", 7, 2, 2, &pixels, ",p=1");
        let mut updates = Vec::new();
        let received = wait_until(
            || {
                session.write(&sequence);
                updates.extend(session.take_graphics_updates());
                updates.iter().any(|image| image.id == 7)
            },
            std::time::Duration::from_secs(6),
        );
        assert!(received, "kitty image was not decoded");
        let image = updates.iter().find(|image| image.id == 7).expect("image 7");
        assert_eq!((image.width, image.height), (2, 2));
        assert_eq!(image.rgba.len(), 16);
        assert_eq!(&image.rgba[..4], &pixels[..4]);

        let mut snap = Snapshot::default();
        session.snapshot_into(&mut snap);
        let placement = snap
            .graphics
            .iter()
            .find(|placement| placement.image_id == 7)
            .expect("placement for image 7");
        assert!(placement.columns >= 1 && placement.rows >= 1);
        assert_eq!(placement.source, (0, 0, 0, 0));
        assert!(snap.graphics_images.contains(&7));

        // A second image with a higher z-index is painted on top.
        let second = vec![0x40u8; 16];
        let received = wait_until(
            || {
                session.write(&kitty_apc_extra("T", 8, 2, 2, &second, ",p=2,z=5"));
                session.snapshot_into(&mut snap);
                snap.graphics_images.contains(&8)
            },
            std::time::Duration::from_secs(6),
        );
        assert!(received, "second image missing: {:?}", snap.graphics_images);
        let z_order: Vec<i32> = snap
            .graphics
            .iter()
            .map(|placement| placement.z_index)
            .collect();
        assert_eq!(z_order, vec![0, 5], "placements must be z-ordered");

        // `d=A` (uppercase) drops every placement *and* the image data.
        session.write(b"\x1b_Ga=d,d=A\x1b\\");
        let deleted = wait_until(
            || {
                session.snapshot_into(&mut snap);
                snap.graphics_images.is_empty() && snap.graphics.is_empty()
            },
            std::time::Duration::from_secs(6),
        );
        assert!(deleted, "images were not deleted");
    }

    #[test]
    fn kitty_graphics_reject_undecodable_png_payloads_safely() {
        let session = echo_session(8);
        session.set_cell_size(8.0, 16.0);
        let mut snap = Snapshot::default();
        await_echo(&session, &mut snap);
        // `f=100` is the PNG format code. Decoding compressed payloads needs
        // rio-vt's `graphics` feature, and even with it this body is not an
        // image: either way the request must be dropped without wedging the
        // parser or the session.
        let apc = format!(
            "\x1b_Ga=T,q=2,f=100,i=22;{}\x1b\\",
            base64(b"definitely not a png")
        );
        session.write(apc.as_bytes());

        // A raw image sent afterwards still decodes, proving the parser and
        // the graphics state survived.
        let pixels = vec![0x7fu8; 16];
        session.write(&kitty_apc_extra("T", 23, 2, 2, &pixels, ",p=1"));
        let mut images = Vec::new();
        let recovered = wait_until(
            || {
                images.extend(session.take_graphics_updates());
                images.iter().any(|image| image.id == 23)
            },
            std::time::Duration::from_secs(6),
        );
        assert!(recovered, "session stopped parsing after a bad png payload");
        session.snapshot_into(&mut snap);
        assert!(snap.graphics_images.contains(&23));
        assert!(!snap.graphics_images.contains(&22));
    }

    #[test]
    fn kitty_graphics_chunked_transmission_is_reassembled() {
        let session = echo_session(8);
        session.set_cell_size(8.0, 16.0);
        let mut snap = Snapshot::default();
        await_echo(&session, &mut snap);

        let pixels: Vec<u8> = (0..16u8).map(|value| value.wrapping_mul(16)).collect();
        let payload = base64(&pixels);
        let (head, tail) = payload.split_at(payload.len() / 2);
        // First chunk: `m=1` keeps the accumulator open. The continuation
        // repeats the id and omits the transmission metadata.
        let first = format!("\x1b_Ga=T,m=1,q=2,f=32,s=2,v=2,i=11;{head}\x1b\\");
        let second = format!("\x1b_Gm=0,q=2,i=11;{tail}\x1b\\");
        session.write(first.as_bytes());
        session.write(second.as_bytes());

        let mut images = Vec::new();
        let received = wait_until(
            || {
                images.extend(session.take_graphics_updates());
                images.iter().any(|image| image.id == 11)
            },
            std::time::Duration::from_secs(6),
        );
        assert!(received, "chunked image was not reassembled");
        let image = images
            .iter()
            .find(|image| image.id == 11)
            .expect("image 11");
        assert_eq!((image.width, image.height), (2, 2));
        assert_eq!(image.rgba.as_ref(), pixels.as_slice());
    }

    #[test]
    fn kitty_graphics_ignore_oversized_images() {
        let session = echo_session(8);
        session.set_cell_size(8.0, 16.0);
        // 20000 columns exceeds the protocol's 10000 limit.
        let oversized = b"\x1b_Ga=T,f=32,s=20000,v=2,i=9;AAAA\x1b\\";
        let mut snap = Snapshot::default();
        let ignored = wait_until(
            || {
                session.write(oversized);
                session.snapshot_into(&mut snap);
                !snap.graphics_images.contains(&9)
            },
            std::time::Duration::from_secs(3),
        );
        assert!(ignored);
        // A malformed command must not wedge the parser either.
        session.write(b"\x1b_Gnot-a-command\x1b\\");
        assert!(wait_until(
            || {
                session.snapshot_into(&mut snap);
                snap.rows > 0 && !snap.graphics_images.contains(&9)
            },
            std::time::Duration::from_secs(2)
        ));
    }

    #[test]
    fn kitty_graphics_cache_is_bounded_by_the_byte_cap() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first_path = dir.path().join("first.apc");
        let second_path = dir.path().join("second.apc");
        // Two 200x200 RGBA images are 160 KB each, more than the 256 KB cap.
        std::fs::write(
            &first_path,
            kitty_apc("T", 1, 200, 200, &vec![0x10u8; 200 * 200 * 4]),
        )
        .expect("write");
        std::fs::write(
            &second_path,
            kitty_apc("T", 2, 200, 200, &vec![0x20u8; 200 * 200 * 4]),
        )
        .expect("write");
        let options = SpawnOptions {
            cwd: dir.path().to_path_buf(),
            cols: 40,
            rows: 8,
            command: Some(format!(
                "cat '{}' '{}'",
                first_path.display(),
                second_path.display()
            )),
            graphics_bytes: 256 * 1024,
            ..Default::default()
        };
        let session = spawn(options).expect("spawn");
        session.set_cell_size(8.0, 16.0);
        let mut snap = Snapshot::default();
        let bounded = wait_until(
            || {
                let _ = session.take_graphics_updates();
                session.snapshot_into(&mut snap);
                snap.graphics_images.contains(&2)
            },
            std::time::Duration::from_secs(10),
        );
        assert!(bounded, "second image missing: {:?}", snap.graphics_images);
        assert!(
            !snap.graphics_images.contains(&1),
            "the oldest image should be evicted: {:?}",
            snap.graphics_images
        );
        // The cap counts decoded pixel bytes.
        let total: usize = snap.graphics_images.len() * 200 * 200 * 4;
        assert!(total <= 256 * 1024);
    }

    #[test]
    fn keyboard_protocol_stack_and_modify_other_keys_are_parsed() {
        let session = echo_session(8);
        let mut snap = Snapshot::default();
        await_echo(&session, &mut snap);
        session.write(b"\x1b[>1u");
        assert!(wait_until(
            || session.modes().kitty.bits() == 1,
            std::time::Duration::from_secs(5)
        ));
        let modes = session.modes();
        assert!(modes.kitty_keyboard);
        assert_eq!(
            crate::input::encode_key(
                crate::input::KeyStroke::plain(crate::input::TerminalKey::Char('a'))
                    .with_event(crate::input::KeyEvent::Release),
                &modes
            ),
            None,
            "release needs the event-types flag"
        );

        session.write(b"\x1b[>3u");
        assert!(wait_until(
            || session.modes().kitty.bits() == 3,
            std::time::Duration::from_secs(5)
        ));
        let modes = session.modes();
        let release = crate::input::KeyStroke::plain(crate::input::TerminalKey::Char('a'))
            .with_event(crate::input::KeyEvent::Release);
        assert_eq!(
            crate::input::encode_key(release, &modes).expect("encoded"),
            b"\x1b[97;1:3u".to_vec()
        );

        session.write(b"\x1b[<u");
        assert!(wait_until(
            || session.modes().kitty.bits() == 1,
            std::time::Duration::from_secs(5)
        ));
        session.write(b"\x1b[<u");
        assert!(wait_until(
            || !session.modes().kitty.is_active(),
            std::time::Duration::from_secs(5)
        ));

        session.write(b"\x1b[>4;2m");
        assert!(wait_until(
            || session.modes().modify_other_keys == 2,
            std::time::Duration::from_secs(5)
        ));
        let modes = session.modes();
        let ctrl_enter = crate::input::KeyStroke {
            key: crate::input::TerminalKey::Enter,
            ctrl: true,
            alt: false,
            shift: false,
            event: crate::input::KeyEvent::Press,
        };
        assert_eq!(
            crate::input::encode_key(ctrl_enter, &modes).expect("encoded"),
            b"\x1b[27;5;13~".to_vec()
        );
        session.write(b"\x1b[>4m");
        assert!(wait_until(
            || session.modes().modify_other_keys == 0,
            std::time::Duration::from_secs(5)
        ));
    }
}
