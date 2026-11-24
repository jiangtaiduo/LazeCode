use std::collections::VecDeque;
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
use rio_vt::ansi::graphics::{Graphics, UpdateQueues};
use rio_vt::ansi::kitty_virtual::{
    compute_run_geometry, resolve_virtual_placement, IncompletePlacement, PLACEHOLDER,
};
use rio_vt::ansi::CursorShape as RioCursorShape;
use rio_vt::ansi::KeyboardModes as RioKeyboardModes;
use rio_vt::config::colors::{AnsiColor, NamedColor};
use rio_vt::crosswords::grid::row::Row;
use rio_vt::crosswords::grid::{Dimensions, ExtrasTable, Scroll};
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

/// Fixed id for the single-window embed; sessions are addressed by app ids.
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
            graphics_bytes: 64 * 1024 * 1024,
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
    /// Blank cell: transparent bg, default fg (`ColorSpec` defaults to
    /// `DefaultFg`, which would paint the cell with the text colour).
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

/// One visible Unicode-placeholder run of a Kitty virtual image.
#[derive(Clone, Debug)]
pub struct VirtualImageRun {
    pub image_id: u32,
    /// Screen position/size in cells, including sub-cell offsets.
    pub col: f32,
    pub row: f32,
    pub columns: f32,
    pub rows: f32,
    /// Normalised texture rectangle `[u0, v0, u1, v1]`.
    pub source_uv: [f32; 4],
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

/// Sanity caps for terminal geometry, so a layout glitch cannot allocate a
/// gigantic grid or snapshot buffer.
const MAX_COLS: u16 = 1_024;
const MAX_ROWS: u16 = 512;

/// Rebuild the snapshot buffer once its capacity is this many times the live
/// cell count, so a one-off huge geometry does not keep gigabytes resident.
const SNAPSHOT_SHRINK_FACTOR: usize = 4;
const SNAPSHOT_MIN_CAPACITY: usize = 16_384;

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
    /// Visible rows of Unicode virtual placements (omp image chips).
    pub virtual_graphics: Vec<VirtualImageRun>,
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

/// Hot/cold split: `hot_cap` lines stay in RAM, older rows go to `store`;
/// `slack` is headroom so no row is evicted before it is spooled.
struct SpoolState {
    store: Option<ColdStore>,
    spooled_through: u64,
    hot_cap: usize,
    slack: usize,
    /// Last `lines_evicted` observed. A decrease means the VT grid was reset
    /// (`RIS`) and the absolute row-id space restarted at zero.
    last_evicted: u64,
}

/// Textures awaiting upload/drop, bounded by the graphics budget (only the
/// active pane drains it) and by an entry cap: per-entry overhead is not part
/// of the pixel-byte budget, so a flood of tiny images would otherwise grow
/// the queue without bound.
#[derive(Default)]
struct GraphicsQueue {
    images: VecDeque<KittyImage>,
    bytes: usize,
    generation: u64,
}

/// Upper bound for `GraphicsQueue::images` regardless of the payload bytes.
const GRAPHICS_QUEUE_MAX_ENTRIES: usize = 512;

/// Drop oldest pending images until both budgets hold.
fn trim_graphics_queue(images: &mut VecDeque<KittyImage>, bytes: &mut usize, byte_cap: usize) {
    while *bytes > byte_cap || images.len() > GRAPHICS_QUEUE_MAX_ENTRIES {
        let Some(dropped) = images.pop_front() else {
            break;
        };
        *bytes = bytes.saturating_sub(dropped.rgba.len());
    }
}

pub(crate) struct Shared {
    title: Mutex<Option<String>>,
    bell: AtomicU32,
    running: AtomicBool,
    exit_code: AtomicI32,
    last_output_ms: AtomicU64,
    /// Last user write to the PTY (keystrokes/paste/IME). Busy polling uses
    /// this to distinguish shell echo from real command output.
    last_input_ms: AtomicU64,
    /// Last PTY grid resize. SIGWINCH makes full-screen TUIs repaint; that
    /// repaint must not start the project marquee.
    last_resize_ms: AtomicU64,
    last_notify_ms: AtomicU64,
    pending_notify: AtomicBool,
    wake: AtomicU64,
    size: Mutex<(u16, u16)>,
    /// Lines scrolled above the live screen (0 = live view). Owned here so the
    /// reader thread can pin it while history keeps growing.
    view_offset: AtomicUsize,
    graphics: Mutex<GraphicsQueue>,
    /// Soft target for decoded Kitty image bytes; visible placements are
    /// protected up to the hard cap (4x).
    graphics_soft_limit: usize,
    /// Byte cap for the pending-image queue; only the active pane drains it, so
    /// background consoles need their own limit.
    graphics_queue_bytes: usize,
    notifier: Option<Arc<dyn Notifier>>,
}

impl Shared {
    /// Coalesce reader wakes to one per 16ms frame budget. When a batch lands
    /// inside the window it only sets `pending_notify`; the reader loop's
    /// short poll (and its own top-of-loop check) delivers the trailing wake,
    /// so no thread is spawned per window.
    fn notify_output(self: &Arc<Self>) {
        self.wake.fetch_add(1, Ordering::Relaxed);
        let now = now_ms();
        let last = self.last_notify_ms.load(Ordering::Relaxed);
        if now.saturating_sub(last) < 16 {
            self.pending_notify.store(true, Ordering::Relaxed);
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

    /// Deliver a coalesced batch once the frame window has elapsed.
    fn flush_pending_output(&self) {
        if !self.pending_notify.load(Ordering::Relaxed) {
            return;
        }
        let now = now_ms();
        if now.saturating_sub(self.last_notify_ms.load(Ordering::Relaxed)) < 16 {
            return;
        }
        if self.pending_notify.swap(false, Ordering::Relaxed) {
            self.last_notify_ms.store(now.max(1), Ordering::Relaxed);
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
        let queue = &mut *self.shared.graphics.lock();
        let queue_cap = self.shared.graphics_queue_bytes;
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
            let bytes = rgba.len();
            queue.images.push_back(KittyImage {
                id,
                width,
                height,
                rgba: Arc::from(rgba.into_boxed_slice()),
                generation,
            });
            queue.bytes += bytes;
            // Only the active pane's draw drains this queue; drop the oldest
            // payloads when a background console outruns either budget.
            trim_graphics_queue(&mut queue.images, &mut queue.bytes, queue_cap);
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

#[allow(clippy::too_many_arguments)]
fn virtual_runs_into(
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

/// Fill `cells` with the renderable characters of one grid line and `text`
/// with their non-blank subsequence. Both buffers are reused so the anchor
/// scan below stays allocation-free.
fn row_text(term: &Crosswords<Proxy>, line: i32, cells: &mut Vec<CellData>, text: &mut Vec<char>) {
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
fn is_wrap_continuation(term: &Crosswords<Proxy>, line: i32, hot: usize) -> bool {
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
fn viewport_anchor(
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
fn anchored_row(
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

/// A VT reset (`RIS`) reinitialises the grid, so `lines_evicted` restarts at
/// zero while the spool's watermark still points past the old epoch. Without
/// this, the new stream would not spool a single row until its id caught up
/// and cold reads would keep showing pre-reset lines.
fn reset_epoch_if_rewound(spool: &mut SpoolState, evicted: u64) -> bool {
    if evicted >= spool.last_evicted {
        spool.last_evicted = evicted;
        return false;
    }
    if let Some(store) = spool.store.as_mut() {
        store.clear();
    }
    spool.spooled_through = evicted;
    spool.last_evicted = evicted;
    true
}

/// Max bytes fed to the parser between spool observations; one byte scrolls at
/// most one line, so the ring's slack is never exceeded.
const PARSE_SLICE: usize = 1024;

/// Move history that fell out of the hot window into the disk spool and shrink
/// the in-memory ring back to its budget.
fn spool_scrollback(
    term: &mut Crosswords<Proxy>,
    spool: &mut SpoolState,
    view_offset: &AtomicUsize,
    total_before: u64,
) {
    let evicted = term.grid.lines_evicted();
    reset_epoch_if_rewound(spool, evicted);
    let Some(store) = spool.store.as_mut() else {
        return;
    };
    let hot = term.history_size();
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
    // `evicted + history` (the first screen row's id) is unchanged by
    // shrinking, so this counts only lines pushed into history; a scrolled-back
    // viewport follows that growth to stay pinned.
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
    shell_pid: Option<u32>,
    tty_name: Option<PathBuf>,
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

/// Processes other than the shell that are attached to a terminal's tty.
/// Empty `commands` with `busy == false` is the normal idle prompt.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BusyInfo {
    pub busy: bool,
    pub commands: Vec<String>,
}

/// The immutable slice of a session needed to probe it from a worker thread.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BusyTarget {
    pub tty: Option<PathBuf>,
    pub shell_pid: Option<u32>,
    /// Configured shell path, used to distinguish the idle shell from a
    /// command that `zsh -lc ...` exec'd in place.
    pub shell: String,
}

/// A login shell shows as `-zsh` in `ps comm`; only dash forms count as idle,
/// otherwise `exec fish` over the shell pid would look idle.
fn is_shell_name(name: &str, shell: &str) -> bool {
    let basename = |value: &str| {
        std::path::Path::new(value)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(value)
            .to_string()
    };
    let configured = basename(shell);
    let raw_base = basename(name);
    // macOS `ps` reports processes whose executable it cannot read with a
    // parenthesised name, e.g. `(bash)`; strip both forms before comparing.
    let base = raw_base
        .trim_start_matches('-')
        .trim_start_matches('(')
        .trim_end_matches(')');
    if raw_base == configured || base == configured {
        return true;
    }
    name.starts_with('-')
        && matches!(
            base,
            "zsh" | "bash" | "sh" | "dash" | "ksh" | "fish" | "tcsh" | "csh" | "nu" | "pwsh"
        )
}

/// One tty-attached process: the kernel's short command (login shells keep
/// their `-` prefix there, which is how an idle login shell is recognised)
/// and the display name (full executable path).
struct TtyProcess {
    comm: String,
    name: String,
}

fn busy_from_processes(entries: &[TtyProcess], _shell_pid: Option<u32>, shell: &str) -> BusyInfo {
    let mut commands = Vec::new();
    for process in entries {
        let name = process.name.trim();
        if name.is_empty() {
            continue;
        }
        // Match by name, not pid: macOS may re-exec or rename the shell, and
        // the recorded spawn pid no longer identifies it.
        if is_shell_name(process.comm.trim(), shell) || is_shell_name(name, shell) {
            continue;
        }
        let short = name.rsplit('/').next().unwrap_or(name).to_string();
        if !commands.iter().any(|command| command == &short) {
            commands.push(short);
        }
        if commands.len() >= 3 {
            break;
        }
    }
    BusyInfo {
        busy: !commands.is_empty(),
        commands,
    }
}

fn busy_from_entries(entries: &[(u32, String)], shell_pid: Option<u32>, shell: &str) -> BusyInfo {
    let processes: Vec<TtyProcess> = entries
        .iter()
        .map(|(_, name)| TtyProcess {
            comm: name.clone(),
            name: name.clone(),
        })
        .collect();
    busy_from_processes(&processes, shell_pid, shell)
}

#[cfg(test)]
fn parse_busy_output(stdout: &str, shell_pid: Option<u32>, shell: &str) -> BusyInfo {
    let entries: Vec<(u32, String)> = stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.trim().splitn(2, char::is_whitespace);
            let pid = parts.next()?.trim().parse::<u32>().ok()?;
            let name = parts.next()?.trim().to_string();
            (!name.is_empty()).then_some((pid, name))
        })
        .collect();
    busy_from_entries(&entries, shell_pid, shell)
}

/// One row of a single process-table snapshot.
struct ProcessRow {
    /// Controlling tty device id, 0 when the process has no terminal.
    tdev: u32,
    /// Kernel `p_comm`: short name, `-` prefixed for login shells.
    comm: String,
    /// Full executable path (or the short command name as a fallback).
    name: String,
}

/// Snapshot every process once, without spawning `ps`.
///
/// `ps -axo pid=,tty=,comm= -ww` costs 20-40ms of kernel time on a busy Mac,
/// and the activity dot polls every two seconds for the lifetime of the app.
/// `proc_listallpids` + `proc_pidinfo` + `proc_pidpath` answer the same three
/// questions (pid, controlling tty, executable) with per-process syscalls and
/// no fork/exec.
#[cfg(target_os = "macos")]
fn process_table() -> Vec<ProcessRow> {
    use std::ffi::CStr;

    const PIDPATH_MAX: usize = 4096;
    let mut pids: Vec<libc::c_int> = vec![0; 4096];
    loop {
        let bytes = (pids.len() * std::mem::size_of::<libc::c_int>()) as libc::c_int;
        let count = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
        if count <= 0 {
            return Vec::new();
        }
        let count = count as usize;
        if count < pids.len() {
            pids.truncate(count);
            break;
        }
        pids.resize(pids.len() * 2, 0);
    }

    let mut rows = Vec::with_capacity(pids.len());
    let mut path = [0 as libc::c_char; PIDPATH_MAX];
    for pid in pids {
        if pid <= 0 {
            continue;
        }
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        let got = unsafe {
            libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size)
        };
        if got != size {
            continue;
        }
        let len = unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), PIDPATH_MAX as u32) };
        let comm: String = {
            let raw: Vec<u8> = info
                .pbi_comm
                .iter()
                .take_while(|c| **c != 0)
                .map(|c| *c as u8)
                .collect();
            String::from_utf8_lossy(&raw).into_owned()
        };
        let name = if len > 0 {
            unsafe { CStr::from_ptr(path.as_ptr()) }
                .to_string_lossy()
                .into_owned()
        } else {
            // The kernel hides some paths (other users, protected binaries);
            // the 16-byte `comm` still identifies those.
            comm.clone()
        };
        rows.push(ProcessRow {
            tdev: info.e_tdev,
            comm,
            name,
        });
    }
    rows
}

/// Match one target's tty against the snapshot: anything attached to that
/// terminal besides the idle shell is a running command.
#[cfg(target_os = "macos")]
fn busy_from_table(table: &[ProcessRow], target: &BusyTarget) -> BusyInfo {
    use std::os::unix::fs::MetadataExt;
    let Some(tty) = target.tty.as_deref() else {
        return BusyInfo::default();
    };
    // The device number of `/dev/ttysNNN` is exactly what `e_tdev` reports.
    let Ok(dev) = std::fs::metadata(tty).map(|meta| meta.rdev() as u32) else {
        return BusyInfo::default();
    };
    let entries: Vec<TtyProcess> = table
        .iter()
        .filter(|row| row.tdev == dev)
        .map(|row| TtyProcess {
            comm: row.comm.clone(),
            name: row.name.clone(),
        })
        .collect();
    busy_from_processes(&entries, target.shell_pid, &target.shell)
}

/// Probe all targets with one process-table snapshot: anything on the tty
/// besides the idle shell is a command (`zsh -lc omp` exec'd omp over the
/// shell pid).
pub fn probe_busy_many(targets: &[BusyTarget]) -> Vec<BusyInfo> {
    if targets.is_empty() {
        return Vec::new();
    }
    #[cfg(target_os = "macos")]
    {
        let table = process_table();
        if !table.is_empty() {
            return targets
                .iter()
                .map(|target| busy_from_table(&table, target))
                .collect();
        }
        // libproc unavailable (sandboxed build): fall back to `ps` below.
    }
    #[cfg(unix)]
    {
        let output = match std::process::Command::new("ps")
            .args(["-axo", "pid=,tty=,comm=", "-ww"])
            .output()
        {
            Ok(output) if output.status.success() => output,
            Ok(output) => {
                log::warn!(
                    "batch busy probe failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
                return vec![BusyInfo::default(); targets.len()];
            }
            Err(err) => {
                log::warn!("batch busy probe failed: {err}");
                return vec![BusyInfo::default(); targets.len()];
            }
        };
        let mut by_tty: std::collections::HashMap<String, Vec<(u32, String)>> =
            std::collections::HashMap::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let mut parts = line.split_whitespace();
            let Some(pid) = parts.next().and_then(|pid| pid.parse::<u32>().ok()) else {
                continue;
            };
            let Some(tty) = parts.next() else {
                continue;
            };
            let name = parts.collect::<Vec<_>>().join(" ");
            if !name.is_empty() {
                by_tty
                    .entry(tty.trim_start_matches("/dev/").to_string())
                    .or_default()
                    .push((pid, name));
            }
        }
        targets
            .iter()
            .map(|target| {
                let Some(tty) = target.tty.as_deref() else {
                    return BusyInfo::default();
                };
                let tty = tty.to_string_lossy();
                let key = tty.strip_prefix("/dev/").unwrap_or(&tty);
                let entries = by_tty.get(key).map(Vec::as_slice).unwrap_or(&[]);
                busy_from_entries(entries, target.shell_pid, &target.shell)
            })
            .collect()
    }
    #[cfg(not(unix))]
    {
        vec![BusyInfo::default(); targets.len()]
    }
}

/// Probe one tty. Kept as the single-target convenience wrapper.
pub fn probe_busy(target: &BusyTarget) -> BusyInfo {
    probe_busy_many(std::slice::from_ref(target))
        .into_iter()
        .next()
        .unwrap_or_default()
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

/// Drop unused/inactive kitty images until the soft target is met. Live
/// placements and images from the last two seconds are protected; the parser's
/// hard cap still bounds total memory at 4x.
fn trim_graphics_to_soft(graphics: &mut Graphics, soft_limit: usize) {
    if graphics.total_bytes <= soft_limit {
        return;
    }
    let now = rio_vt::time::Instant::now();
    let mut used: std::collections::HashSet<u64> = std::collections::HashSet::new();
    for placement in graphics.kitty_placements.values() {
        used.insert(placement.image_id as u64);
    }
    for (image_id, _) in graphics.kitty_virtual_placements.keys() {
        used.insert(*image_id as u64);
    }
    for (image_id, stored) in &graphics.kitty_images {
        if now.duration_since(stored.transmission_time) < std::time::Duration::from_secs(2) {
            used.insert(*image_id as u64);
        }
    }
    let used_bytes: usize = used
        .iter()
        .filter_map(|id| graphics.kitty_images.get(&(*id as u32)))
        .map(|stored| stored.data.pixels.len())
        .sum();
    let unused_bytes = graphics.total_bytes.saturating_sub(used_bytes);
    let need = (graphics.total_bytes - soft_limit).min(unused_bytes);
    if need > 0 {
        // Lower the limit to the soft target so the unused-image tier is
        // trimmed; visible/recent images are in `used`.
        let hard_limit = graphics.total_limit;
        // Pass zero required bytes and lower the limit by the excess to free,
        // so one large unused image does not trigger a second eviction.
        graphics.total_limit = graphics.total_bytes.saturating_sub(need);
        graphics.evict_images(0, &used);
        graphics.total_limit = hard_limit;
    }
}

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

/// Full-history search shared by the synchronous and background entry points.
fn search_all(
    term: &FairMutex<Crosswords<Proxy>>,
    spool: &Mutex<SpoolState>,
    query: &str,
    cancel: &AtomicBool,
) -> Vec<TerminalMatch> {
    if query.is_empty() {
        return Vec::new();
    }
    let (mut matches, hot_base) = search_hot(term, query, cancel);
    if cancel.load(Ordering::Relaxed) {
        return matches;
    }
    let needle = query.to_lowercase();
    let mut spool = spool.lock();
    if let Some(store) = spool.store.as_mut() {
        let remaining = 2_000usize.saturating_sub(matches.len());
        if remaining > 0 {
            store.search(
                &needle,
                remaining,
                |id, col, len| {
                    let line = id as i64 - hot_base;
                    matches.push(TerminalMatch {
                        line: line.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
                        col,
                        len,
                    });
                },
                Some(cancel),
            );
        }
    }
    matches.sort_by_key(|hit| (hit.line, hit.col));
    matches
}

/// Hot-grid scan; also returns the id of the first screen row so cold hits
/// can be placed in the same absolute space.
fn search_hot(
    term: &FairMutex<Crosswords<Proxy>>,
    query: &str,
    cancel: &AtomicBool,
) -> (Vec<TerminalMatch>, i64) {
    let term = term.lock();
    let hot_base = term.grid.lines_evicted() as i64 + term.history_size() as i64;
    let cols = term.grid.columns();
    let rows = term.grid.screen_lines();
    let history = term.history_size() as i32;
    let mut regex = match RegexSearch::new(query) {
        Ok(regex) => regex,
        // Invalid regex: fall back to plain literal scan.
        Err(_) => {
            let needle = query.to_lowercase();
            return (search_literal_locked(&term, &needle, cancel), hot_base);
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
        if cancel.load(Ordering::Relaxed) {
            break;
        }
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
fn search_literal_locked(
    term: &Crosswords<Proxy>,
    needle: &str,
    cancel: &AtomicBool,
) -> Vec<TerminalMatch> {
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
        if cancel.load(Ordering::Relaxed) {
            break;
        }
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

impl Session {
    pub fn is_running(&self) -> bool {
        self.shared.running.load(Ordering::Relaxed)
    }

    /// Snapshot the immutable probe inputs for a background busy poll.
    pub fn busy_target(&self) -> BusyTarget {
        BusyTarget {
            tty: self.tty_name.clone(),
            shell_pid: self.shell_pid,
            shell: self.shell.clone(),
        }
    }

    /// Run a one-off probe on the caller's thread (used by close confirmation
    /// when the periodic snapshot may be stale).
    pub fn busy_info(&self) -> BusyInfo {
        probe_busy(&self.busy_target())
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

    pub fn last_input_ms(&self) -> Option<u64> {
        let value = self.shared.last_input_ms.load(Ordering::Relaxed);
        if value == 0 {
            None
        } else {
            Some(now_ms().saturating_sub(value))
        }
    }

    pub fn last_resize_ms(&self) -> Option<u64> {
        let value = self.shared.last_resize_ms.load(Ordering::Relaxed);
        if value == 0 {
            None
        } else {
            Some(now_ms().saturating_sub(value))
        }
    }

    /// How long presentation must still be held for a synchronized update
    /// (DEC private mode 2026). `None` means the grid is safe to snapshot;
    /// `Some` is the time until the program closes the frame or rio-vt's
    /// safety timeout fires. Snapshotting mid-update exposes a partially
    /// painted screen (omp repaints whole screens inside BSU/ESU after a
    /// resize), which reads as a one-frame flash of garbled text.
    pub fn sync_update_hold(&self) -> Option<std::time::Duration> {
        let term = self.term.lock();
        if !term.synchronized_output() {
            return None;
        }
        let now = rio_vt::time::Instant::now();
        if term.sync_update_expired(now) {
            return None;
        }
        term.sync_update_deadline()
            .map(|deadline| deadline.saturating_duration_since(now))
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
        // Remember user input before it reaches the PTY. The shell/TUI echo
        // that follows is an interaction redraw, not command output.
        self.shared
            .last_input_ms
            .store(now_ms().max(1), Ordering::Relaxed);
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
        let cols = cols.clamp(10, MAX_COLS);
        let rows = rows.clamp(2, MAX_ROWS);
        let mut geom = self.geom.lock();
        if *geom == (cols, rows, cell_w, cell_h) {
            return;
        }
        let size_changed = geom.0 != cols || geom.1 != rows;
        *geom = (cols, rows, cell_w, cell_h);
        drop(geom);
        {
            let mut term = self.term.lock();
            // Reflow rewraps history and rewrites how many grid lines sit
            // above the live screen, so the mirrored `view_offset` would land
            // somewhere else entirely — widening shrinks history until the
            // snapshot clamp pins the view to the oldest line and the console
            // appears to jump to the top. Hand the offset to the grid for the
            // resize: rio-vt stabilizes the viewport around its internal
            // display offset line by line, then we read the stabilized
            // position back.
            let view = self.shared.view_offset.load(Ordering::Relaxed);
            let hot_before = term.history_size();
            let pinned = view.min(hot_before);
            let current = term.grid.display_offset();
            if pinned != current {
                term.grid
                    .scroll_display(Scroll::Delta(pinned as i32 - current as i32));
            }
            // Content signature of a rewrap-stable row at or below the
            // viewport top, so the rewrapped grid can be re-anchored on the
            // content the user was reading.
            let mut cells = Vec::with_capacity(term.grid.columns());
            let mut text = Vec::with_capacity(term.grid.columns());
            let anchor = if pinned > 0 {
                viewport_anchor(&term, pinned, hot_before, &mut cells, &mut text)
            } else {
                None
            };
            term.resize(CrosswordsSize::new_with_dimensions(
                cols as usize,
                rows as usize,
                (cols as u32).saturating_mul(cell_w),
                (rows as u32).saturating_mul(cell_h),
                cell_w,
                cell_h,
            ));
            let hot_after = term.history_size();
            let mut stabilized = term.grid.display_offset();
            if let Some((_, signature, span)) = anchor.as_ref() {
                // The reflow keeps the live screen anchored, so the anchor row
                // moves by at most the net history change — hundreds of rows
                // when widening merges a long scrollback. Scan that whole
                // range around rio's position.
                let radius = hot_before.abs_diff(hot_after).clamp(64, 4096);
                if let Some(found) = anchored_row(
                    &term, stabilized, hot_after, radius, signature, &mut cells, &mut text,
                ) {
                    // The viewport top sits `span` characters above the
                    // anchor row. Walk them up row by row: rows fully inside
                    // the span hold only span characters, so the first row
                    // that exhausts them is the exact viewport top no matter
                    // how the reflow merged or split the span.
                    let mut top = found;
                    let mut remaining = span.len();
                    while remaining > 0 && top < hot_after {
                        top += 1;
                        row_text(&term, -(top as i32), &mut cells, &mut text);
                        remaining = remaining.saturating_sub(text.len());
                    }
                    stabilized = top.clamp(1, hot_after);
                }
            }
            if stabilized != pinned {
                self.store_view_offset(view - pinned + stabilized);
            }
            // Re-render everything once the grid is reflowed.
            *self.dirty_rows.lock() = Vec::new();
            term.mark_fully_damaged();
        }
        if size_changed {
            // Mark before ioctl: a TUI may repaint synchronously in response.
            self.shared
                .last_resize_ms
                .store(now_ms().max(1), Ordering::Relaxed);
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

    /// Fill `snap` from the grid, spool and viewport so history is continuous.
    pub fn snapshot_into(&self, snap: &mut Snapshot) {
        let mut term = self.term.lock();
        trim_graphics_to_soft(&mut term.graphics, self.shared.graphics_soft_limit);
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
        let needed = cols * rows;
        let keep = needed
            .saturating_mul(SNAPSHOT_SHRINK_FACTOR)
            .max(SNAPSHOT_MIN_CAPACITY);
        if snap.cells.capacity() > keep {
            // `Vec::resize` never gives memory back; a transient huge geometry
            // would otherwise pin gigabytes for the rest of the session.
            snap.cells = Vec::with_capacity(needed);
        }
        snap.cells.resize(needed, CellData::default());

        let styles = grid.styles();
        let extras = &grid.extras_table;
        snap.virtual_graphics.clear();
        let (cell_w, cell_h) = {
            let geom = self.geom.lock();
            (geom.2.max(1) as f32, geom.3.max(1) as f32)
        };
        for row in 0..rows {
            let abs = row as i64 - offset as i64;
            if abs >= -(hot as i64) {
                virtual_runs_into(
                    &grid[Line(abs as i32)],
                    styles,
                    extras,
                    &term.graphics,
                    row,
                    cell_w,
                    cell_h,
                    &mut snap.virtual_graphics,
                );
            }
        }
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

    /// Case-insensitive search over hot grid plus cold spool; hits map to
    /// absolute line indices (negative = history).
    pub fn search(&self, query: &str) -> Vec<TerminalMatch> {
        let cancel = AtomicBool::new(false);
        search_all(&self.term, &self.spool, query, &cancel)
    }

    /// Search on a worker thread. The receiver yields the hits (or nothing
    /// when cancelled); the returned flag stops the scan early, and dropping
    /// the receiver makes the final send fail.
    ///
    /// A scan walks the whole retained history, so running it inline on the
    /// UI thread cost one full pass per keystroke (~70ms at 43k lines,秒级 at
    /// the cold byte cap).
    pub fn search_async(
        &self,
        query: String,
    ) -> (
        std::sync::mpsc::Receiver<Vec<TerminalMatch>>,
        Arc<AtomicBool>,
    ) {
        let (tx, rx) = std::sync::mpsc::channel();
        let term = self.term.clone();
        let spool = self.spool.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let thread_cancel = cancel.clone();
        let spawned = std::thread::Builder::new()
            .name("lazecode-term-search".to_string())
            .spawn(move || {
                let matches = search_all(&term, &spool, &query, &thread_cancel);
                if !thread_cancel.load(Ordering::Relaxed) {
                    let _ = tx.send(matches);
                }
            });
        if spawned.is_err() {
            // `tx` lived inside the failed closure and is dropped with it, so
            // the caller's receiver disconnects and the pending state ends.
            log::warn!("cannot spawn terminal search worker");
        }
        (rx, cancel)
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
        let mut queue = self.shared.graphics.lock();
        let images = queue.images.drain(..).collect();
        // The byte counter must track the pending payloads only; leaving the
        // taken images counted would make the cap evict live images after
        // the next drain.
        queue.bytes = 0;
        images
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
        let mut groups = Vec::new();
        if was_running {
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
            // Polite first: a shell writes its history and jobs on SIGHUP, and
            // a SIGKILL right behind it leaves no chance to run that handler.
            for pgid in &groups {
                unsafe {
                    libc::kill(-*pgid, libc::SIGHUP);
                }
            }
            let _ = self.killer.lock().kill();
        }
        // Closing the master forces a terminal hangup for processes that
        // ignored the signals above; the reader has its own cancellation flag.
        let _ = self.master.lock().take();
        if was_running && !groups.is_empty() {
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(100);
            while std::time::Instant::now() < deadline
                && groups.iter().any(|pgid| group_is_alive(*pgid))
            {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            for pgid in groups {
                unsafe {
                    libc::kill(-pgid, libc::SIGKILL);
                }
            }
        }
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

/// Whether any process in `pgid`'s group still exists (`ESRCH` = gone).
fn group_is_alive(pgid: i32) -> bool {
    let result = unsafe { libc::kill(-pgid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Resolve the login shell for a console.
///
/// The candidate (settings or `$SHELL`) must be a known login shell by *file
/// name*; an absolute path is only accepted when it exists, is executable and
/// does not live in a world-writable temp directory (so a stale setting cannot
/// point at `/tmp/…/zsh`). Anything else resolves through the standard bin
/// directories and finally `/bin/zsh`. The settings file is user-authored, so
/// this is a hygiene guard, not a security boundary.
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
        let temp = std::env::temp_dir();
        let temp = temp.canonicalize().unwrap_or(temp);
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if canonical.starts_with(&temp) {
            log::warn!("refusing shell '{candidate}' inside the temp directory");
        } else if is_executable(&canonical) {
            return canonical.to_string_lossy().into_owned();
        }
    }
    for dir in [
        "/bin",
        "/usr/bin",
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/opt/local/bin",
    ] {
        let path = std::path::Path::new(dir).join(name);
        if path.is_file() {
            return path.to_string_lossy().into_owned();
        }
    }
    "/bin/zsh".to_string()
}

fn is_executable(path: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
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
    // omp picks its image protocol from environment whitelists; advertise
    // LazeCode so it uses kitty placements / virtual placeholders.
    command.env("PI_FORCE_IMAGE_PROTOCOL", "kitty");
    command.env("PI_KITTY_PLACEHOLDERS", "1");
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
    let shell_pid = child.process_id();
    let tty_name = pair.master.tty_name();
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
        // NOTE: no O_NONBLOCK on the reader: it shares the open file
        // description with the writer, which would turn large writes into
        // `EAGAIN` partials. Readiness comes from `poll` below.
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

    let graphics_soft = options.graphics_bytes.clamp(256 * 1024, 512 * 1024 * 1024);
    let graphics_hard = graphics_soft
        .saturating_mul(4)
        .clamp(graphics_soft, 512 * 1024 * 1024);
    let shared = Arc::new(Shared {
        title: Mutex::new(None),
        bell: AtomicU32::new(0),
        running: AtomicBool::new(true),
        exit_code: AtomicI32::new(i32::MIN),
        last_output_ms: AtomicU64::new(0),
        last_input_ms: AtomicU64::new(0),
        last_resize_ms: AtomicU64::new(0),
        last_notify_ms: AtomicU64::new(0),
        pending_notify: AtomicBool::new(false),
        wake: AtomicU64::new(0),
        size: Mutex::new((cols, rows)),
        view_offset: AtomicUsize::new(0),
        graphics: Mutex::new(GraphicsQueue::default()),
        graphics_soft_limit: graphics_soft,
        graphics_queue_bytes: graphics_soft,
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
        last_evicted: 0,
    }));
    let mut term = Crosswords::new(
        CrosswordsSize::new(cols as usize, rows as usize),
        RioCursorShape::Block,
        proxy,
        *WINDOW_ID,
        ROUTE_ID,
        hot_cap + slack,
    );
    // The parser hard cap protects visible images from the oldest-first
    // eviction until the hard ceiling; snapshot-time trimming brings unused
    // images back under the user's soft target.
    term.graphics.total_limit = graphics_hard;
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
                // Trailing wake for a batch that landed inside the 16ms
                // frame window. This runs on the reader instead of a timer
                // thread per window.
                reader_shared.flush_pending_output();
                // Finish a spool rewrite here: the copy runs on its own
                // thread, and the swap must not happen while the VT lock is
                // held (UI snapshots share that lock). Polling on every loop
                // iteration also picks results up once output pauses.
                {
                    let mut spool = reader_spool.lock();
                    if let Some(store) = spool.store.as_mut() {
                        store.poll_compaction();
                    }
                }
                #[cfg(unix)]
                {
                    let mut pollfd = libc::pollfd {
                        fd: reader_fd,
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    // A pending coalesced batch must be delivered at the next
                    // frame boundary, so poll no longer than the window.
                    let timeout = if reader_shared.pending_notify.load(Ordering::Relaxed) {
                        16
                    } else {
                        100
                    };
                    let ready = unsafe { libc::poll(&mut pollfd, 1, timeout) };
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
        shell_pid,
        tty_name,
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
    fn synchronized_update_holds_presentation_until_esu() {
        let session = spawn(SpawnOptions {
            cwd: std::env::temp_dir(),
            cols: 80,
            rows: 12,
            command: Some(
                "printf '\\033[?2026h\\033[2J\\033[HPARTIAL'; sleep 0.4; \
                 printf 'DONE\\033[?2026l'"
                    .to_string(),
            ),
            ..Default::default()
        })
        .expect("spawn");

        assert!(
            wait_until(
                || session.sync_update_hold().is_some(),
                std::time::Duration::from_secs(4)
            ),
            "BSU was never observed"
        );
        // The grid already holds the partial frame; a renderer that snapshots
        // here would paint it. The hold is what keeps the last full frame.
        let mut snap = Snapshot::default();
        session.snapshot_into(&mut snap);
        assert!(snapshot_text(&snap).contains("PARTIAL"));
        assert!(
            wait_until(
                || session.sync_update_hold().is_none(),
                std::time::Duration::from_secs(4)
            ),
            "ESU was never observed"
        );
        session.snapshot_into(&mut snap);
        assert!(snapshot_text(&snap).contains("DONE"));
    }

    #[test]
    fn absurd_geometry_is_clamped_and_snapshot_capacity_reclaimed() {
        let session = spawn(SpawnOptions {
            cwd: std::env::temp_dir(),
            cols: 80,
            rows: 12,
            command: Some("printf 'hi\\n'".to_string()),
            ..Default::default()
        })
        .expect("spawn");
        // 47,599 x 1,600 cells x 32-byte CellData is the 2.38 GB single
        // allocation observed in a long-running 0.4.5 process.
        session.resize(47_599, 1_600);
        let mut snap = Snapshot {
            // Simulate a stale oversized buffer from an earlier geometry.
            cells: Vec::with_capacity(1_000_000),
            ..Snapshot::default()
        };
        session.snapshot_into(&mut snap);
        assert!(snap.cols <= MAX_COLS as usize, "cols={}", snap.cols);
        assert!(snap.rows <= MAX_ROWS as usize, "rows={}", snap.rows);
        let needed = snap.cols * snap.rows;
        assert_eq!(snap.cells.len(), needed);
        assert!(
            snap.cells.capacity() <= needed * SNAPSHOT_SHRINK_FACTOR,
            "capacity {} for {needed} cells",
            snap.cells.capacity()
        );
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
    fn resize_keeps_scrolled_viewport_on_content() {
        // Each echo is 46-47 columns: it soft-wraps at 30 columns and fits on
        // one line at 60, so resizing rewrites how many grid lines the same
        // content occupies. A scroll offset that survives the resize as a raw
        // line count lands on different content — widening even shrinks
        // history until the offset clamps to the oldest line (the reported
        // "console jumps to the top" on window resize).
        let options = SpawnOptions {
            cwd: std::env::temp_dir(),
            cols: 30,
            rows: 5,
            command: Some(
                "for i in $(seq 1 40); do echo row-$i-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa; done; echo END-MARKER"
                    .to_string(),
            ),
            ..Default::default()
        };
        let session = spawn(options).expect("spawn");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            if !session.search("END-MARKER").is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        // Drain the shell prompt that trails the marker so no reader-thread
        // output races the resizes below.
        std::thread::sleep(std::time::Duration::from_millis(200));

        let mut snap = Snapshot::default();
        let viewport = |session: &Session, snap: &mut Snapshot| {
            session.snapshot_into(snap);
            let mut text = String::new();
            for row in 0..snap.rows {
                for col in 0..snap.cols {
                    text.push(snap.cell(row, col).ch);
                }
                text.push('\n');
            }
            text
        };

        let hits = session.search("row-20-");
        assert!(!hits.is_empty(), "row-20 should be in scrollback");
        session.scroll_to_line(hits[0].line);
        assert!(
            viewport(&session, &mut snap).contains("row-20-"),
            "scrolled viewport should show row-20"
        );

        // Widen: the wrapped pairs unwrap and history shrinks. The line the
        // user was reading must stay at the top of the viewport.
        session.resize(60, 5);
        let text = viewport(&session, &mut snap);
        assert!(
            text.lines().next().unwrap_or_default().contains("row-20-"),
            "widening must keep the scrolled viewport on row-20: {text}"
        );

        // Narrow again: the lines re-wrap and history grows back.
        session.resize(30, 5);
        let text = viewport(&session, &mut snap);
        assert!(
            text.lines().next().unwrap_or_default().contains("row-20-"),
            "narrowing must keep the scrolled viewport on row-20: {text}"
        );
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
    fn resize_keeps_anchor_when_viewport_top_is_a_wrap_continuation() {
        // Dragging the window wider merges soft-wrapped rows. When the
        // viewport top is the *continuation* row of a wrapped line, its text
        // moves into the middle of the merged row after the reflow — a
        // row-start signature can never match it again, and the merged rows
        // above the viewport shift it far past the old ±24 scan window. The
        // viewport must still land on that logical line after one widen.
        let options = SpawnOptions {
            cwd: std::env::temp_dir(),
            cols: 30,
            rows: 6,
            command: Some(
                "for i in $(seq 1 300); do echo row-$i-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa; done; echo END-MARKER"
                    .to_string(),
            ),
            ..Default::default()
        };
        let session = spawn(options).expect("spawn");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            if !session.search("END-MARKER").is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        std::thread::sleep(std::time::Duration::from_millis(200));

        let mut snap = Snapshot::default();
        let viewport = |session: &Session, snap: &mut Snapshot| {
            session.snapshot_into(snap);
            let mut text = String::new();
            for row in 0..snap.rows {
                for col in 0..snap.cols {
                    text.push(snap.cell(row, col).ch);
                }
                text.push('\n');
            }
            text
        };

        // Put the continuation row of `row-220` at the very top of the
        // viewport: its first row wraps at 30 columns, so the next grid row
        // starts with the wrapped `aaaa…` fragment.
        let hits = session.search("row-220-");
        assert!(!hits.is_empty(), "row-220 should be in scrollback");
        session.scroll_to_line(hits[0].line + 1);
        let text = viewport(&session, &mut snap);
        assert!(
            text.lines()
                .next()
                .unwrap_or_default()
                .trim_start()
                .starts_with("aaaa"),
            "viewport top should be the wrap continuation of row-220: {text}"
        );

        // One widening step: every continuation merges into its first row and
        // history halves — hundreds of rows above the viewport shift.
        session.resize(60, 6);
        let text = viewport(&session, &mut snap);
        assert!(
            text.lines().next().unwrap_or_default().contains("row-220"),
            "widening must keep the viewport on the merged row-220: {text}"
        );
    }

    #[test]
    fn resize_drag_sequence_keeps_scrolled_anchor() {
        // A drag fires a burst of small resizes. Each widening step merges
        // rows above the viewport; when one step shifts more than the scan
        // window, the old code kept rio's clamped position and the drift
        // compounded until the view pinned at the oldest line. Every step of
        // the sequence must keep the anchor line at the viewport top.
        let options = SpawnOptions {
            cwd: std::env::temp_dir(),
            cols: 30,
            rows: 6,
            command: Some(
                "for i in $(seq 1 300); do echo row-$i-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa; done; echo END-MARKER"
                    .to_string(),
            ),
            ..Default::default()
        };
        let session = spawn(options).expect("spawn");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            if !session.search("END-MARKER").is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        std::thread::sleep(std::time::Duration::from_millis(200));

        let mut snap = Snapshot::default();
        let viewport = |session: &Session, snap: &mut Snapshot| {
            session.snapshot_into(snap);
            let mut text = String::new();
            for row in 0..snap.rows {
                for col in 0..snap.cols {
                    text.push(snap.cell(row, col).ch);
                }
                text.push('\n');
            }
            text
        };

        let hits = session.search("row-260-");
        assert!(!hits.is_empty(), "row-260 should be in scrollback");
        session.scroll_to_line(hits[0].line);
        let text = viewport(&session, &mut snap);
        assert!(
            text.lines().next().unwrap_or_default().contains("row-260"),
            "scrolled viewport should show row-260 at the top: {text}"
        );

        for cols in [36u16, 42, 48, 54, 60] {
            session.resize(cols, 6);
            let text = viewport(&session, &mut snap);
            assert!(
                text.lines().next().unwrap_or_default().contains("row-260"),
                "widening to {cols} must keep row-260 at the viewport top: {text}"
            );
        }
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
    fn busy_parser_ignores_shell_and_dedupes_commands() {
        let info = parse_busy_output(
            "  100 /bin/zsh\n 101 /usr/local/bin/vite\n 102 vite\n 103 node\n",
            Some(100),
            "/bin/zsh",
        );
        assert!(info.busy);
        assert_eq!(info.commands, ["vite", "node"]);
        assert!(!parse_busy_output("  100 /bin/zsh\n", Some(100), "/bin/zsh").busy);
        let exec = parse_busy_output("  100 omp\n", Some(100), "/bin/zsh");
        assert!(exec.busy);
        assert_eq!(exec.commands, ["omp"]);
    }

    #[test]
    fn busy_parser_distinguishes_login_shell_from_exec_d_shell() {
        // macOS reports the idle login shell with a dash (argv[0]); an exec'd
        // command over the same pid has no dash. `exec fish` previously read
        // as idle because any shell name matched.
        assert!(
            !parse_busy_output("  100 -zsh\n", Some(100), "/bin/zsh").busy,
            "dash-form login shell is idle"
        );
        assert!(
            !parse_busy_output("  100 -/bin/zsh\n", Some(100), "/bin/zsh").busy,
            "dash-path login shell is idle"
        );
        let exec_fish = parse_busy_output("  100 fish\n", Some(100), "/bin/zsh");
        assert!(exec_fish.busy, "exec'd fish over the shell pid is busy");
        assert_eq!(exec_fish.commands, ["fish"]);
        // A plain configured-shell name stays idle (non-login spawn forms).
        assert!(!parse_busy_output("  100 /bin/zsh\n", Some(100), "/bin/zsh").busy);
        // macOS 15 `ps` can report the shell as `(bash)`, sometimes under a
        // pid other than the one recorded at spawn.
        let paren = parse_busy_output("  100 (bash)\n", Some(100), "/bin/bash");
        assert!(!paren.busy, "parenthesised shell is idle: {paren:?}");
        let reexec = parse_busy_output("  200 (bash)\n", Some(100), "/bin/bash");
        assert!(!reexec.busy, "shell matches by name, not pid: {reexec:?}");
        // Documented residual: `exec zsh` over a zsh session is
        // indistinguishable from the idle prompt by `ps` alone.
        assert!(!parse_busy_output("  100 zsh\n", Some(100), "/bin/zsh").busy);
    }

    #[test]
    fn busy_probe_batches_distinct_ttys() {
        let mut busy_options = SpawnOptions {
            cwd: std::env::temp_dir(),
            cols: 60,
            rows: 6,
            command: Some("sleep 30".to_string()),
            ..Default::default()
        };
        busy_options.scrollback = 32;
        let busy = spawn(busy_options).unwrap();
        let mut idle_options = SpawnOptions {
            cwd: std::env::temp_dir(),
            cols: 60,
            rows: 6,
            ..Default::default()
        };
        idle_options.scrollback = 32;
        let idle = spawn(idle_options).unwrap();
        let targets = [busy.busy_target(), idle.busy_target()];
        // Wait for the *exec'd command*, not merely "busy": a login shell can
        // spawn transient children while sourcing dotfiles, and any of those
        // legitimately counts as a command for one probe round.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let infos = loop {
            let probed = probe_busy_many(&targets);
            let saw_sleep = probed
                .first()
                .is_some_and(|info| info.commands.iter().any(|command| command == "sleep"));
            if saw_sleep {
                break probed;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "sleep was never reported on the busy tty: {probed:?}"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert_eq!(infos.len(), 2);
        assert!(infos[0].busy, "exec'd sleep must be busy: {infos:?}");
        // The idle pane's shell may still be sourcing dotfiles while the busy
        // one is checked; wait for it to settle.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let idle_info = probe_busy_many(&targets).swap_remove(1);
            if !idle_info.busy {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "idle shell never settled: {idle_info:?}"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        busy.kill();
        idle.kill();
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

    fn test_image(bytes: usize, age_secs: u64) -> rio_vt::ansi::graphics::StoredImage {
        rio_vt::ansi::graphics::StoredImage {
            data: rio_graphics::GraphicData {
                id: rio_graphics::GraphicId(1),
                width: 1,
                height: 1,
                color_type: rio_graphics::ColorType::Rgba,
                pixels: vec![0; bytes],
                is_opaque: true,
                resize: None,
                display_width: None,
                display_height: None,
                transmit_time: rio_vt::time::Instant::now(),
            },
            transmission_time: rio_vt::time::Instant::now()
                - std::time::Duration::from_secs(age_secs),
        }
    }

    fn test_virtual_placement(image_id: u32) -> rio_vt::ansi::graphics::VirtualPlacement {
        rio_vt::ansi::graphics::VirtualPlacement {
            image_id,
            placement_id: 1,
            columns: 1,
            rows: 1,
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        }
    }

    #[test]
    fn graphics_trim_keeps_visible_and_recent_images() {
        let mut graphics = Graphics::default();
        graphics.kitty_images.insert(1, test_image(60_000, 10));
        graphics
            .kitty_virtual_placements
            .insert((1, 1), test_virtual_placement(1));
        graphics.kitty_images.insert(2, test_image(60_000, 10));
        graphics.total_bytes = 120_000;

        trim_graphics_to_soft(&mut graphics, 100_000);
        assert!(graphics.kitty_images.contains_key(&1));
        assert!(!graphics.kitty_images.contains_key(&2));
        assert!(graphics.total_bytes <= 100_000);

        // A just-transmitted image with no placement yet must survive a
        // snapshot between transmission and placement.
        graphics.kitty_images.insert(2, test_image(60_000, 0));
        graphics.total_bytes = 120_000;
        trim_graphics_to_soft(&mut graphics, 100_000);
        assert!(graphics.kitty_images.contains_key(&1));
        assert!(graphics.kitty_images.contains_key(&2));
    }

    #[test]
    fn graphics_trim_does_not_overshoot_into_visible_images() {
        let mut graphics = Graphics::default();
        graphics.kitty_images.insert(1, test_image(100_000, 10));
        graphics
            .kitty_virtual_placements
            .insert((1, 1), test_virtual_placement(1));
        graphics.kitty_images.insert(2, test_image(30_000, 10));
        graphics.kitty_images.insert(3, test_image(30_000, 10));
        graphics.total_bytes = 160_000;

        trim_graphics_to_soft(&mut graphics, 110_000);
        assert!(
            graphics.kitty_images.contains_key(&1),
            "the visible image must survive trimming"
        );
        assert!(graphics.total_bytes <= 110_000);
    }

    #[test]
    fn graphics_trim_leaves_visible_images_over_the_soft_target() {
        let mut graphics = Graphics::default();
        graphics.kitty_images.insert(1, test_image(120_000, 10));
        graphics
            .kitty_virtual_placements
            .insert((1, 1), test_virtual_placement(1));
        graphics.total_bytes = 120_000;

        trim_graphics_to_soft(&mut graphics, 100_000);
        assert!(graphics.kitty_images.contains_key(&1));
        assert_eq!(graphics.total_bytes, 120_000);
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
    fn kitty_png_payload_decodes() {
        let session = echo_session(4);
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 2, 2);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("png header");
            writer
                .write_image_data(&[
                    255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
                ])
                .expect("png data");
        }
        let mut sequence = format!("\x1b_Ga=t,f=100,q=2,i=9;{}\x1b\\", base64(&png)).into_bytes();
        sequence.extend_from_slice(b"\x1b_Ga=p,U=1,i=9,p=1,c=2,r=1,q=2\x1b\\");
        sequence.extend_from_slice(b"\x1b[38;2;0;0;9m\x1b[58;2;0;0;1m");
        sequence.extend_from_slice(
            rio_vt::ansi::kitty_virtual::encode_placeholder(0, 0, None).as_bytes(),
        );
        sequence.extend_from_slice(
            rio_vt::ansi::kitty_virtual::encode_placeholder(0, 1, None).as_bytes(),
        );
        sequence.extend_from_slice(b"\x1b[0m");
        session.write(&sequence);
        let mut updates = Vec::new();
        let mut snap = Snapshot::default();
        let received = wait_until(
            || {
                updates.extend(session.take_graphics_updates());
                session.snapshot_into(&mut snap);
                updates.iter().any(|image| image.id == 9) && !snap.virtual_graphics.is_empty()
            },
            std::time::Duration::from_secs(6),
        );
        assert!(
            received,
            "png image / virtual run missing: images={:?} runs={} text={:?}",
            snap.graphics_images,
            snap.virtual_graphics.len(),
            snapshot_text(&snap)
        );
    }

    #[test]
    fn kitty_virtual_placeholders_resolve_to_runs() {
        let session = echo_session(4);
        session.set_cell_size(8.0, 16.0);
        let pixels = vec![0xffu8; 16];
        let mut seq = kitty_apc("t", 7, 2, 2, &pixels);
        seq.extend_from_slice(b"\x1b_Ga=p,U=1,i=7,p=1,c=2,r=1,q=2\x1b\\");
        seq.extend_from_slice(b"\x1b[38;2;0;0;7m\x1b[58;2;0;0;1m");
        seq.extend_from_slice(
            rio_vt::ansi::kitty_virtual::encode_placeholder(0, 0, None).as_bytes(),
        );
        seq.extend_from_slice(
            rio_vt::ansi::kitty_virtual::encode_placeholder(0, 1, None).as_bytes(),
        );
        seq.extend_from_slice(b"\x1b[0m");
        session.write(&seq);
        let mut snap = Snapshot::default();
        let found = wait_until(
            || {
                session.snapshot_into(&mut snap);
                !snap.virtual_graphics.is_empty()
            },
            std::time::Duration::from_secs(6),
        );
        assert!(
            found,
            "no virtual runs: images={:?} text={:?}",
            snap.graphics_images,
            snapshot_text(&snap)
        );
        let run = &snap.virtual_graphics[0];
        assert_eq!(run.image_id, 7);
        assert!(run.columns > 0.0 && run.rows > 0.0);
        assert_eq!(run.source_uv, [0.0, 0.0, 1.0, 1.0]);
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
        // `f=100` is the PNG format code; without the `graphics` feature this
        // payload cannot be decoded, so drop the request without wedging the
        // parser.
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
        // Visible placements are protected above the soft target; both
        // images stay until the hard cap (4x the soft target) is reached.
        assert!(
            snap.graphics_images.contains(&1),
            "visible image should not be evicted: {:?}",
            snap.graphics_images
        );
        let total: usize = snap.graphics_images.len() * 200 * 200 * 4;
        assert!(total <= 4 * 256 * 1024);
    }

    #[test]
    fn pending_graphics_queue_is_bounded_when_never_drained() {
        // A background console never drains `take_graphics_updates`, so the
        // pending queue caps itself at the session's graphics budget.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut files = Vec::new();
        for index in 1..=10u32 {
            let path = dir.path().join(format!("img{index}.apc"));
            std::fs::write(
                &path,
                kitty_apc("T", index, 200, 200, &vec![index as u8; 200 * 200 * 4]),
            )
            .expect("write");
            files.push(path);
        }
        let mut command = String::from("cat");
        for path in &files {
            command.push_str(&format!(" '{}'", path.display()));
        }
        let options = SpawnOptions {
            cwd: dir.path().to_path_buf(),
            cols: 40,
            rows: 8,
            command: Some(command),
            graphics_bytes: 256 * 1024,
            ..Default::default()
        };
        let session = spawn(options).expect("spawn");
        session.set_cell_size(8.0, 16.0);
        let mut snap = Snapshot::default();
        let parsed = wait_until(
            || {
                session.snapshot_into(&mut snap);
                snap.graphics_images.contains(&10)
            },
            std::time::Duration::from_secs(10),
        );
        assert!(
            parsed,
            "tenth image never parsed: {:?}",
            snap.graphics_images
        );
        // The pending queue holds at most one budget of the newest payloads.
        let taken = session.take_graphics_updates();
        let total: usize = taken.iter().map(|image| image.rgba.len()).sum();
        assert!(
            total <= 256 * 1024,
            "pending queue exceeded the budget: {} bytes in {} images",
            total,
            taken.len()
        );
        assert!(
            !taken.is_empty(),
            "the newest image must still be delivered"
        );
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

#[test]
fn busy_probe_detects_exec_and_batch_mapping() {
    let mut options = SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 60,
        rows: 6,
        command: Some("sleep 30".to_string()),
        ..Default::default()
    };
    options.scrollback = 32;
    let session = spawn(options).unwrap();
    let target = session.busy_target();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let info = loop {
        let info = probe_busy_many(std::slice::from_ref(&target))
            .into_iter()
            .next()
            .unwrap_or_default();
        // Wait for the exec'd command itself: a login shell may spawn
        // transient children while sourcing dotfiles, and those legitimately
        // report as commands for one probe round.
        if info.commands.iter().any(|command| command == "sleep") {
            break info;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "sleep was never reported busy: {info:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    assert!(info.busy, "exec'd command must be reported busy: {info:?}");
    assert!(info.commands.iter().any(|command| command == "sleep"));
    session.kill();
}

#[cfg(target_os = "macos")]
#[test]
fn process_table_lists_this_process() {
    let table = process_table();
    let exe = std::env::current_exe().expect("own executable path");
    let name = exe.file_name().expect("executable file name");
    let name = name.to_string_lossy();
    assert!(
        table
            .iter()
            .any(|row| row.name.rsplit('/').next() == Some(name.as_ref())),
        "own executable {name} missing from the process table"
    );
}

#[test]
fn graphics_queue_caps_entries_not_only_bytes() {
    let small = |id: u32| KittyImage {
        id,
        width: 1,
        height: 1,
        rgba: Arc::from([0u8, 0, 0, 0].as_slice()),
        generation: id as u64,
    };
    let mut images: VecDeque<KittyImage> = (0..GRAPHICS_QUEUE_MAX_ENTRIES as u32 + 10)
        .map(small)
        .collect();
    let mut bytes = images.len() * 4;
    // 4-byte payloads stay far under any byte cap; the entry cap must still
    // bound the queue, otherwise per-entry overhead grows without limit.
    trim_graphics_queue(&mut images, &mut bytes, 1 << 30);
    assert_eq!(images.len(), GRAPHICS_QUEUE_MAX_ENTRIES);
    assert_eq!(bytes, GRAPHICS_QUEUE_MAX_ENTRIES * 4);
    assert_eq!(images.front().map(|image| image.id), Some(10));
    // The byte cap still evicts oldest-first.
    trim_graphics_queue(&mut images, &mut bytes, 16);
    assert_eq!(bytes, 16);
    assert_eq!(images.len(), 4);
}

#[test]
fn temp_directory_shells_are_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let fake = dir.path().join("zsh");
    std::fs::write(&fake, b"#!/bin/sh\n").expect("write");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    let resolved = resolve_shell(Some(fake.to_string_lossy().into_owned()));
    assert_ne!(std::path::Path::new(&resolved), fake.as_path());
    assert!(resolved.ends_with("zsh"), "{resolved}");
    assert_eq!(resolve_shell(Some("/bin/zsh".to_string())), "/bin/zsh");
    assert_eq!(
        resolve_shell(Some("/tmp/not-a-shell".to_string())),
        "/bin/zsh"
    );
}

#[test]
fn vt_reset_restarts_the_cold_epoch() {
    let mut store = crate::cold::ColdStore::create(10, 1 << 20).expect("create");
    let row = vec![CellData::default(); 4];
    store.append_rows(0, &[crate::cold::encode_row(&row)]);
    let mut spool = SpoolState {
        store: Some(store),
        spooled_through: 5_000,
        hot_cap: 10,
        slack: 4,
        last_evicted: 5_000,
    };
    // `RIS` rewinds `lines_evicted`: the spool must start a new epoch instead
    // of waiting for ids to catch up past the old watermark.
    assert!(reset_epoch_if_rewound(&mut spool, 3));
    assert_eq!(spool.spooled_through, 3);
    assert_eq!(spool.last_evicted, 3);
    assert_eq!(
        spool.store.as_ref().map(|store| store.len()),
        Some(0),
        "pre-reset cold rows must be dropped"
    );
    // Monotonic progress keeps the watermark and the store.
    assert!(!reset_epoch_if_rewound(&mut spool, 9));
    assert_eq!(spool.spooled_through, 3);
    assert_eq!(spool.last_evicted, 9);
}
