//! PTY session: parser driver, scrollback spooling, snapshots and console
//! busy probing.

mod busy;
mod render;
mod search;

#[cfg(test)]
use busy::*;
pub use busy::{probe_busy, probe_busy_many};
use render::*;
use search::*;

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
mod tests;

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
