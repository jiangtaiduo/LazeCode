use std::io::{Read, Write};
#[cfg(unix)]
use std::os::fd::FromRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape, NamedColor, Processor, Rgb};
use crossbeam_channel::Sender;
use parking_lot::Mutex;
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize};

use crate::next_id;

pub trait Notifier: Send + Sync {
    fn notify(&self);
}

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
    pub shape: CursorShape,
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
    wake: AtomicU64,
    size: Mutex<(u16, u16)>,
    notifier: Option<Arc<dyn Notifier>>,
}

impl Shared {
    fn notify(&self) {
        self.wake.fetch_add(1, Ordering::Relaxed);
        if let Some(notifier) = &self.notifier {
            notifier.notify();
        }
    }
}

struct Listener {
    shared: Arc<Shared>,
    writer: Sender<Vec<u8>>,
}

impl Listener {
    fn reply(&self, bytes: Vec<u8>) {
        // The writer thread is bounded; a terminal query reply is tiny and
        // must never block the reader thread if the UI is stalled.
        let _ = self.writer.try_send(bytes);
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            Event::Title(title) => {
                *self.shared.title.lock() = Some(title);
                self.shared.notify();
            }
            Event::ResetTitle => {
                *self.shared.title.lock() = None;
                self.shared.notify();
            }
            Event::Bell => {
                self.shared.bell.fetch_add(1, Ordering::Relaxed);
                self.shared.notify();
            }
            Event::Wakeup => self.shared.notify(),
            Event::PtyWrite(text) => self.reply(text.into_bytes()),
            Event::TextAreaSizeRequest(format) => {
                let (cols, rows) = *self.shared.size.lock();
                let response = format(WindowSize {
                    num_lines: rows,
                    num_cols: cols,
                    cell_width: 0,
                    cell_height: 0,
                });
                self.reply(response.into_bytes());
            }
            Event::ColorRequest(index, format) => {
                let response = format(palette_color(index));
                self.reply(response.into_bytes());
            }
            Event::Exit | Event::ChildExit(_) => {
                self.shared.running.store(false, Ordering::Relaxed);
                self.shared.notify();
            }
            // OSC 52 writes/reads stay disabled: the product does not allow a
            // program to read or write the system clipboard silently.
            Event::ClipboardStore(..)
            | Event::ClipboardLoad(..)
            | Event::CursorBlinkingChange
            | Event::MouseCursorDirty => {}
        }
    }
}

fn palette_color(index: usize) -> Rgb {
    // alacritty/vte use 256=foreground, 257=background, 258=cursor and
    // 259..=268 for the dim variants. Treat these as palette entries, not as
    // the 232..=255 grayscale ramp (which would overflow u8 arithmetic).
    if let 256..=258 = index {
        return match index {
            256 => Rgb {
                r: 0xc5,
                g: 0xc8,
                b: 0xc6,
            },
            257 => Rgb {
                r: 0x1d,
                g: 0x1f,
                b: 0x21,
            },
            _ => Rgb {
                r: 0x8a,
                g: 0xbe,
                b: 0xb8,
            },
        };
    }
    if let 259..=268 = index {
        const BASE_DIM: [(u8, u8, u8); 10] = [
            (0x1d, 0x1f, 0x21),
            (0xcc, 0x66, 0x66),
            (0x98, 0xc3, 0x79),
            (0xf0, 0xc6, 0x74),
            (0x81, 0xa2, 0xbe),
            (0xb2, 0x94, 0xbb),
            (0x8a, 0xbe, 0xb8),
            (0xc5, 0xc8, 0xc6),
            (0x66, 0x66, 0x66),
            (0xd5, 0x4e, 0x53),
        ];
        let (r, g, b) = BASE_DIM[index - 259];
        return Rgb { r, g, b };
    }
    const BASE: [(u8, u8, u8); 16] = [
        (0x1d, 0x1f, 0x21),
        (0xcc, 0x66, 0x66),
        (0x98, 0xc3, 0x79),
        (0xf0, 0xc6, 0x74),
        (0x81, 0xa2, 0xbe),
        (0xb2, 0x94, 0xbb),
        (0x8a, 0xbe, 0xb8),
        (0xc5, 0xc8, 0xc6),
        (0x66, 0x66, 0x66),
        (0xd5, 0x4e, 0x53),
        (0xb9, 0xca, 0x76),
        (0xe7, 0xc5, 0x47),
        (0x7a, 0xa6, 0xda),
        (0xc6, 0x78, 0xdd),
        (0x70, 0xc0, 0xb0),
        (0xea, 0xea, 0xea),
    ];
    if index < 16 {
        let (r, g, b) = BASE[index];
        Rgb { r, g, b }
    } else if index < 232 {
        let i = (index - 16) as u8;
        let steps = [0u8, 0x5f, 0x87, 0xaf, 0xd7, 0xff];
        let r = steps[(i / 36) as usize];
        let g = steps[((i % 36) / 6) as usize];
        let b = steps[(i % 6) as usize];
        Rgb { r, g, b }
    } else {
        let v = (8u16 + (index.saturating_sub(232) as u16) * 10).min(255) as u8;
        Rgb { r: v, g: v, b: v }
    }
}

fn color_spec(color: AnsiColor) -> ColorSpec {
    match color {
        AnsiColor::Spec(rgb) => ColorSpec::Rgb(rgb.r, rgb.g, rgb.b),
        AnsiColor::Indexed(index) => ColorSpec::Indexed(index),
        AnsiColor::Named(named) => {
            let index = match named {
                NamedColor::Black | NamedColor::DimBlack => 0,
                NamedColor::Red | NamedColor::DimRed => 1,
                NamedColor::Green | NamedColor::DimGreen => 2,
                NamedColor::Yellow | NamedColor::DimYellow => 3,
                NamedColor::Blue | NamedColor::DimBlue => 4,
                NamedColor::Magenta | NamedColor::DimMagenta => 5,
                NamedColor::Cyan | NamedColor::DimCyan => 6,
                NamedColor::White | NamedColor::DimWhite => 7,
                NamedColor::BrightBlack => 8,
                NamedColor::BrightRed => 9,
                NamedColor::BrightGreen => 10,
                NamedColor::BrightYellow => 11,
                NamedColor::BrightBlue => 12,
                NamedColor::BrightMagenta => 13,
                NamedColor::BrightCyan => 14,
                NamedColor::BrightWhite => 15,
                NamedColor::Background => return ColorSpec::DefaultBg,
                NamedColor::Cursor => return ColorSpec::Cursor,
                NamedColor::Foreground
                | NamedColor::BrightForeground
                | NamedColor::DimForeground => return ColorSpec::DefaultFg,
            };
            ColorSpec::Indexed(index)
        }
    }
}

#[derive(Clone, Copy)]
struct Dims {
    cols: usize,
    rows: usize,
}

impl Dimensions for Dims {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

pub struct Session {
    pub id: u64,
    pub kind: SessionKind,
    pub command_label: String,
    pub shell: String,
    pub cwd: PathBuf,
    created_ms: u64,
    term: Arc<Mutex<Term<Listener>>>,
    shared: Arc<Shared>,
    writer: Sender<Vec<u8>>,
    reader_cancel: Arc<AtomicBool>,
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    pgid: Option<i32>,
    size: Mutex<(u16, u16)>,
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
        let term = self.term.lock();
        modes_from(&term)
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
            term.resize(Dims {
                cols: cols as usize,
                rows: rows as usize,
            });
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
        let grid = term.grid();
        let cols = grid.columns();
        let rows = grid.screen_lines();
        let display_offset = grid.display_offset();
        snap.cols = cols;
        snap.rows = rows;
        snap.history_size = grid.history_size();
        snap.display_offset = display_offset;
        snap.bell = self.shared.bell.load(Ordering::Relaxed);
        snap.title = self.shared.title.lock().clone();
        snap.modes = modes_from(&term);
        snap.cells.resize(cols * rows, CellData::default());
        snap.cells.fill(CellData {
            ch: '\0',
            text: None,
            fg: ColorSpec::DefaultFg,
            bg: ColorSpec::DefaultBg,
            attrs: 0,
        });

        for row in 0..rows {
            let line = Line(row as i32 - display_offset as i32);
            let grid_row = &grid[line];
            for col in 0..cols {
                let cell = &grid_row[Column(col)];
                let mut attrs = 0u16;
                let flags = cell.flags;
                use alacritty_terminal::term::cell::Flags;
                if flags.contains(Flags::BOLD) || flags.contains(Flags::DIM_BOLD) {
                    attrs |= ATTR_BOLD;
                }
                if flags.contains(Flags::ITALIC) || flags.contains(Flags::BOLD_ITALIC) {
                    attrs |= ATTR_ITALIC;
                }
                if flags.intersects(Flags::ALL_UNDERLINES) {
                    attrs |= ATTR_UNDERLINE;
                }
                if flags.contains(Flags::STRIKEOUT) {
                    attrs |= ATTR_STRIKEOUT;
                }
                if flags.contains(Flags::INVERSE) {
                    attrs |= ATTR_INVERSE;
                }
                if flags.contains(Flags::DIM) || flags.contains(Flags::DIM_BOLD) {
                    attrs |= ATTR_DIM;
                }
                if flags.contains(Flags::HIDDEN) {
                    attrs |= ATTR_HIDDEN;
                }
                if flags.contains(Flags::WIDE_CHAR) {
                    attrs |= ATTR_WIDE;
                }
                if flags.contains(Flags::WIDE_CHAR_SPACER) {
                    attrs |= ATTR_SPACER;
                }
                if flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
                    attrs |= ATTR_LEADING_SPACER;
                }
                let text = cell.zerowidth().map(|zw| {
                    let mut buffer = String::with_capacity(4 * (zw.len() + 1));
                    buffer.push(cell.c);
                    for ch in zw {
                        buffer.push(*ch);
                    }
                    Arc::<str>::from(buffer)
                });
                snap.cells[row * cols + col] = CellData {
                    ch: cell.c,
                    text,
                    fg: color_spec(cell.fg),
                    bg: color_spec(cell.bg),
                    attrs,
                };
            }
        }

        let cursor_point = grid.cursor.point;
        let cursor_row = cursor_point.line.0 + display_offset as i32;
        let cursor_visible = term.mode().contains(TermMode::SHOW_CURSOR)
            && cursor_row >= 0
            && (cursor_row as usize) < rows;
        let cursor_style = term.cursor_style();
        snap.cursor = if cursor_visible {
            Some(CursorView {
                row: cursor_row as usize,
                col: cursor_point.column.0.min(cols.saturating_sub(1)),
                shape: cursor_style.shape,
                blinking: cursor_style.blinking,
            })
        } else {
            None
        };
    }

    pub fn scroll_delta(&self, delta: i32) {
        if delta == 0 {
            return;
        }
        let mut term = self.term.lock();
        term.scroll_display(Scroll::Delta(delta));
        drop(term);
        self.shared.notify();
    }

    pub fn scroll_page(&self, up: bool) {
        let mut term = self.term.lock();
        term.scroll_display(if up { Scroll::PageUp } else { Scroll::PageDown });
        drop(term);
        self.shared.notify();
    }

    pub fn scroll_top(&self) {
        let mut term = self.term.lock();
        term.scroll_display(Scroll::Top);
        drop(term);
        self.shared.notify();
    }

    pub fn scroll_bottom(&self) {
        let mut term = self.term.lock();
        term.scroll_display(Scroll::Bottom);
        drop(term);
        self.shared.notify();
    }

    pub fn scroll_to_line(&self, absolute_line: i32) {
        let mut term = self.term.lock();
        let history = term.grid().history_size() as i32;
        let target = (-absolute_line).clamp(0, history);
        let current = term.grid().display_offset() as i32;
        if target != current {
            term.scroll_display(Scroll::Delta(target - current));
        }
    }

    /// Plain case-insensitive search across scrollback + screen.
    pub fn search(&self, query: &str) -> Vec<TerminalMatch> {
        if query.is_empty() {
            return Vec::new();
        }
        let needle = query.to_lowercase();
        let term = self.term.lock();
        let grid = term.grid();
        let cols = grid.columns();
        let rows = grid.screen_lines();
        let history = grid.history_size() as i32;
        let mut matches = Vec::new();
        for line_index in -history..rows as i32 {
            let line = Line(line_index);
            let grid_row = &grid[line];
            let mut text = String::with_capacity(cols);
            let mut columns = Vec::with_capacity(cols);
            for col in 0..cols {
                let cell = &grid_row[Column(col)];
                if cell
                    .flags
                    .contains(alacritty_terminal::term::cell::Flags::WIDE_CHAR_SPACER)
                {
                    continue;
                }
                columns.push(col);
                text.push(cell.c);
                if let Some(zw) = cell.zerowidth() {
                    for ch in zw {
                        text.push(*ch);
                    }
                }
            }
            let haystack = text.to_lowercase();
            if haystack.len() != text.len() {
                // Lowercasing changed byte lengths; fall back to per-char scan.
                for (start_col, _) in text.char_indices() {
                    // Do not slice by query.len(): lowercasing can change byte
                    // lengths (e.g. İ), and the slice would land mid-character.
                    let tail = &text[start_col..];
                    if tail.to_lowercase().starts_with(&needle) {
                        matches.push(TerminalMatch {
                            line: line_index,
                            col: columns
                                .get(text[..start_col].chars().count())
                                .copied()
                                .unwrap_or(0),
                            len: query.chars().count(),
                        });
                        if matches.len() >= 2_000 {
                            return matches;
                        }
                    }
                }
            } else {
                for (byte_pos, _) in haystack.match_indices(&needle) {
                    let char_pos = haystack[..byte_pos].chars().count();
                    let end = needle.chars().count();
                    matches.push(TerminalMatch {
                        line: line_index,
                        col: columns.get(char_pos).copied().unwrap_or(0),
                        len: end,
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

fn modes_from(term: &Term<Listener>) -> Modes {
    let mode = term.mode();
    Modes {
        alt_screen: mode.contains(TermMode::ALT_SCREEN),
        app_cursor: mode.contains(TermMode::APP_CURSOR),
        bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
        mouse_click: mode.contains(TermMode::MOUSE_REPORT_CLICK),
        mouse_drag: mode.contains(TermMode::MOUSE_DRAG),
        mouse_motion: mode.contains(TermMode::MOUSE_MOTION),
        sgr_mouse: mode.contains(TermMode::SGR_MOUSE),
        show_cursor: mode.contains(TermMode::SHOW_CURSOR),
        kitty_keyboard: mode.contains(TermMode::KITTY_KEYBOARD_PROTOCOL),
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
        wake: AtomicU64::new(0),
        size: Mutex::new((cols, rows)),
        notifier: options.notifier.clone(),
    });

    let reader_cancel = Arc::new(AtomicBool::new(false));
    let listener = Listener {
        shared: shared.clone(),
        writer: writer_tx.clone(),
    };
    let config = Config {
        scrolling_history: options.scrollback.clamp(1_000, 200_000),
        // Programs may request keyboard protocol upgrades; keeping it enabled
        // lets modern TUIs (agents, editors) receive the escape forms they expect.
        kitty_keyboard: true,
        ..Default::default()
    };
    let term = Term::new(
        config,
        &Dims {
            cols: cols as usize,
            rows: rows as usize,
        },
        listener,
    );
    let term = Arc::new(Mutex::new(term));

    // Reader thread: owns the parser and drives the Term. On Unix the reader
    // fd is non-blocking and polled with a timeout so teardown can cancel it
    // even when a daemonized child keeps the slave side open.
    let reader_term = term.clone();
    let reader_shared = shared.clone();
    let reader_cancel_thread = reader_cancel.clone();
    std::thread::Builder::new()
        .name("lazecode-term-read".to_string())
        .spawn(move || {
            let mut processor: Processor = Processor::new();
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
                        }
                        reader_shared.notify();
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
    fn palette_color_handles_named_and_grayscale_indices() {
        for index in [0, 15, 16, 231, 232, 255, 256, 257, 258, 268, 999] {
            let _ = palette_color(index);
        }
    }

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
    fn search_handles_lowercase_length_changes_without_panicking() {
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
