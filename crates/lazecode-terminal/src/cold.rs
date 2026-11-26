//! Disk-backed cold scrollback spool keyed by absolute row id: grows past the
//! RAM budget without unbounded memory or disk use.

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use rio_vt::ansi::CursorShape as RioCursorShape;
use rio_vt::crosswords::grid::Dimensions;
use rio_vt::crosswords::pos::{Column, Line, Pos};
use rio_vt::crosswords::search::RegexSearch;
use rio_vt::crosswords::square::{CellFlags, Square};
use rio_vt::crosswords::{Crosswords, CrosswordsSize};
use rio_vt::event::{VoidListener, WindowId};

use crate::session::{CellData, ColorSpec, ATTR_LEADING_SPACER, ATTR_SPACER};

/// Compact once the dead bytes in the file exceed this and outnumber the live
/// rows.
const COMPACT_MIN_BYTES: u64 = 1 << 20;
/// Rows appended while a background rewrite runs may be re-appended to the new
/// file before the swap; beyond this the rewrite falls back to an in-place
/// copy. 2k rows is a few ms of extra spool-lock work at full-width rows.
const COMPACT_TAIL_MAX_ROWS: usize = 2_048;
/// Absolute disk ceiling before the rewrite is forced in place: a session that
/// never pauses must not grow its spool without bound.
const COMPACT_FORCE_FACTOR: u64 = 2;

static NEXT_FILE: AtomicU64 = AtomicU64::new(1);

/// One spooled row on disk.
#[derive(Clone, Copy, Debug)]
struct Entry {
    /// Stable absolute terminal line id this row was captured from.
    id: u64,
    offset: u64,
    len: u32,
}

/// Result of copying the live rows into a fresh file.
struct CompactCopy {
    tmp: PathBuf,
    written: u64,
    /// `(row id, offset in the new file)`, ordered by id.
    rows: Vec<(u64, u64)>,
    /// Generation the snapshot was taken in; a later reset invalidates it.
    generation: u64,
}

/// A rewrite copying live rows off the reader's critical path.
struct CompactJob {
    handle: std::thread::JoinHandle<std::io::Result<CompactCopy>>,
    generation: u64,
    cancel: Arc<AtomicBool>,
}

/// Immutable index + file-handle snapshot for a search running without the
/// spool lock. The cloned handle keeps the exact inode the offsets refer to
/// even if compaction swaps the live file mid-scan.
pub(crate) struct ColdSearchSnapshot {
    file: File,
    entries: Vec<Entry>,
}

/// A bounded, disk-backed row store.
pub struct ColdStore {
    path: PathBuf,
    file: File,
    entries: VecDeque<Entry>,
    /// Bytes of the rows still indexed (the payload budget).
    live_bytes: usize,
    /// Length of the backing file, including evicted rows.
    file_len: u64,
    /// Bytes in the file no longer referenced by `entries`.
    dead_bytes: u64,
    line_budget: usize,
    byte_budget: usize,
    first_id: u64,
    /// Bumped on every reset/swap; stale compaction results are discarded.
    generation: u64,
    /// Rewrite in flight; finished by `poll_compaction`.
    compact_job: Option<CompactJob>,
}

fn compact_tmp(path: &Path, generation: u64) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "lazecode-cold".to_string());
    path.with_file_name(format!("{name}.compact-{generation}"))
}

/// POSIX rename replaces the destination atomically. Platforms whose rename
/// refuses an existing file keep a backup so a failed swap restores the old
/// spool instead of losing it.
#[cfg(unix)]
fn replace_spool_file(tmp: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::rename(tmp, dest)
}

#[cfg(not(unix))]
fn replace_spool_file(tmp: &Path, dest: &Path) -> std::io::Result<()> {
    let backup = dest.with_extension("replace-backup");
    match std::fs::rename(dest, &backup) {
        Ok(()) => {
            if let Err(err) = std::fs::rename(tmp, dest) {
                let _ = std::fs::rename(&backup, dest);
                return Err(err);
            }
            let _ = std::fs::remove_file(&backup);
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => std::fs::rename(tmp, dest),
        Err(err) => Err(err),
    }
}

impl ColdStore {
    /// Create a spool in temp; budgets bound retained rows and are clamped to
    /// sane minimums.
    pub fn create(line_budget: usize, byte_budget: usize) -> std::io::Result<Self> {
        let line_budget = line_budget.max(1);
        let byte_budget = byte_budget.max(4096);
        let seq = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "lazecode-cold-{}-{seq}-{nanos}.scrollback",
            std::process::id()
        ));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)?;
        Ok(Self {
            path,
            file,
            entries: VecDeque::new(),
            live_bytes: 0,
            file_len: 0,
            dead_bytes: 0,
            line_budget,
            byte_budget,
            first_id: 0,
            generation: 1,
            compact_job: None,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Stable id of the oldest retained row.
    pub fn first_id(&self) -> Option<u64> {
        self.entries.front().map(|entry| entry.id)
    }

    /// Stable id of the newest retained row.
    pub fn last_id(&self) -> Option<u64> {
        self.entries.back().map(|entry| entry.id)
    }

    /// Bytes of retained row payload.
    pub fn bytes(&self) -> usize {
        self.live_bytes
    }

    /// Length of the backing file (may include evicted rows).
    pub fn file_bytes(&self) -> u64 {
        self.file_len
    }

    /// Append encoded rows starting at `first_id`. Ids must increase (the index
    /// is binary-searched); oversized rows become zero-length placeholders.
    pub fn append_rows(&mut self, first_id: u64, rows: &[Vec<u8>]) {
        if rows.is_empty() {
            return;
        }
        if let Some(last) = self.entries.back() {
            if first_id <= last.id {
                return;
            }
        }
        let mut buffer = Vec::new();
        let mut appended: Vec<Entry> = Vec::with_capacity(rows.len());
        let mut offset = self.file_len;
        let mut live_added = 0usize;
        for (index, row) in rows.iter().enumerate() {
            let id = first_id + index as u64;
            if row.len() as u64 > self.byte_budget as u64 {
                appended.push(Entry {
                    id,
                    offset: 0,
                    len: 0,
                });
                continue;
            }
            appended.push(Entry {
                id,
                offset,
                len: row.len() as u32,
            });
            offset += row.len() as u64;
            live_added += row.len();
            buffer.extend_from_slice(row);
        }
        if !buffer.is_empty()
            && self
                .file
                .seek(SeekFrom::Start(self.file_len))
                .and_then(|_| self.file.write_all(&buffer))
                .is_err()
        {
            log::warn!("cold scrollback spool: write failed, dropping rows");
            return;
        }
        self.entries.extend(appended);
        self.live_bytes += live_added;
        if let Some(front) = self.entries.front() {
            self.first_id = front.id;
        }
        self.file_len = offset;
        self.evict();
    }

    fn evict(&mut self) {
        while self.entries.len() > self.line_budget || self.live_bytes > self.byte_budget {
            let Some(entry) = self.entries.pop_front() else {
                break;
            };
            self.live_bytes = self.live_bytes.saturating_sub(entry.len as usize);
            self.dead_bytes += entry.len as u64;
        }
        if let Some(front) = self.entries.front() {
            self.first_id = front.id;
        }
        if self.dead_bytes > COMPACT_MIN_BYTES && self.dead_bytes > self.live_bytes as u64 {
            self.schedule_compaction();
        }
    }

    /// Start a rewrite on a worker, or force it in place once the file grew
    /// far past the payload budget (continuous output never pauses long enough
    /// for the async swap, and disk use must stay bounded).
    fn schedule_compaction(&mut self) {
        if self.compact_job.is_some() {
            return;
        }
        if self.dead_bytes > COMPACT_FORCE_FACTOR * self.byte_budget as u64 {
            // A write rate that outruns the copier also makes the copy itself
            // the bottleneck. Cold history is evictable by design, so shed the
            // oldest half before the in-place rewrite: it holds the spool lock
            // for half as long and still bounds the file.
            let live_target = (self.byte_budget / 2).max(4096) as u64;
            let line_target = (self.line_budget / 2).max(1);
            while self.entries.len() > line_target || self.live_bytes as u64 > live_target {
                let Some(entry) = self.entries.pop_front() else {
                    break;
                };
                self.live_bytes = self.live_bytes.saturating_sub(entry.len as usize);
                self.dead_bytes += entry.len as u64;
            }
            if let Some(front) = self.entries.front() {
                self.first_id = front.id;
            }
            if let Err(err) = self.compact_sync() {
                log::warn!("cold scrollback spool compaction failed: {err}");
                // The store stays consistent across a failed compact, so
                // retrying is safe; halving avoids firing on every eviction.
                self.dead_bytes /= 2;
            }
            return;
        }
        let entries: Vec<Entry> = self.entries.iter().copied().collect();
        if entries.is_empty() {
            return;
        }
        // The worker reads through its own handle: the store keeps serving
        // rows and appending to the old file while the copy runs. The temp
        // name carries the generation so a reset can never adopt its result.
        let source = self.path.clone();
        let tmp = compact_tmp(&self.path, self.generation);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let generation = self.generation;
        let spawned = std::thread::Builder::new()
            .name("lazecode-cold-compact".to_string())
            .spawn(move || copy_live_rows(&source, &tmp, &entries, &worker_cancel, generation));
        match spawned {
            Ok(handle) => {
                self.compact_job = Some(CompactJob {
                    handle,
                    generation,
                    cancel,
                })
            }
            Err(err) => log::warn!("cannot spawn cold spool compaction: {err}"),
        }
    }

    /// Finish a background rewrite if its worker completed. Callers hold the
    /// spool lock but not the VT lock, so the swap never blocks screen
    /// snapshots.
    pub(crate) fn poll_compaction(&mut self) {
        let Some(job) = self.compact_job.take() else {
            return;
        };
        if !job.handle.is_finished() {
            self.compact_job = Some(job);
            return;
        }
        let stale_tmp = compact_tmp(&self.path, job.generation);
        if job.generation != self.generation {
            // A reset happened while the worker copied. Its rows describe a
            // store that no longer exists; drop the file instead of adopting
            // it through a reused temp name.
            match job.handle.join() {
                Ok(Ok(copy)) => {
                    let _ = std::fs::remove_file(&copy.tmp);
                }
                Ok(Err(_)) | Err(_) => {
                    let _ = std::fs::remove_file(&stale_tmp);
                }
            }
            return;
        }
        let mut copy = match job.handle.join() {
            Ok(Ok(copy)) if copy.generation == self.generation => copy,
            Ok(Ok(copy)) => {
                let _ = std::fs::remove_file(&copy.tmp);
                return;
            }
            Ok(Err(err)) => {
                log::warn!("cold scrollback spool compaction failed: {err}");
                let _ = std::fs::remove_file(&stale_tmp);
                self.dead_bytes /= 2;
                return;
            }
            Err(_) => {
                log::warn!("cold scrollback spool compaction panicked");
                let _ = std::fs::remove_file(&stale_tmp);
                self.dead_bytes /= 2;
                return;
            }
        };
        // Match every live row against the copied snapshot. Rows appended (or
        // re-spooled) while the worker ran live only in the old file; ids are
        // ordered but not contiguous, so this is a binary search, not
        // arithmetic. A bounded tail is re-appended to keep the rewrite.
        let mut offsets: Vec<u64> = Vec::with_capacity(self.entries.len());
        let mut tail: Vec<(usize, Entry)> = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            match copy.rows.binary_search_by_key(&entry.id, |(id, _)| *id) {
                Ok(found) => offsets.push(copy.rows[found].1),
                Err(_) => {
                    offsets.push(u64::MAX);
                    tail.push((index, *entry));
                }
            }
        }
        if tail.len() > COMPACT_TAIL_MAX_ROWS {
            // Output outran the worker. Falling back to the in-place copy
            // keeps disk use bounded; it holds the spool lock (not the VT
            // lock), so per-frame screen probe stays cheap.
            let _ = std::fs::remove_file(&copy.tmp);
            if let Err(err) = self.compact_sync() {
                log::warn!("cold scrollback spool compaction failed: {err}");
                self.dead_bytes /= 2;
            }
            return;
        }
        if let Err(err) = self.append_tail(&mut copy, &mut offsets, &tail) {
            log::warn!("cold scrollback spool tail copy failed: {err}");
            let _ = std::fs::remove_file(&copy.tmp);
            self.dead_bytes /= 2;
            return;
        }
        if let Err(err) = self.install(&copy, offsets) {
            log::warn!("cold scrollback spool swap failed: {err}");
            let _ = std::fs::remove_file(&copy.tmp);
            self.dead_bytes /= 2;
            return;
        }
        // The rewrite copied every row that was live at snapshot time; rows
        // evicted since then are stale in the new file. One more pass (cheap:
        // only live rows) converges instead of waiting for the next megabyte
        // of dead bytes.
        if self.dead_bytes > (self.live_bytes as u64) / 2 {
            self.schedule_compaction();
        }
    }

    /// Re-append rows newer than the snapshot to the rewritten file.
    fn append_tail(
        &mut self,
        copy: &mut CompactCopy,
        offsets: &mut [u64],
        tail: &[(usize, Entry)],
    ) -> std::io::Result<()> {
        if tail.is_empty() {
            return Ok(());
        }
        let mut dest = OpenOptions::new().append(true).open(&copy.tmp)?;
        let mut buffer = vec![0u8; 64 * 1024];
        for (index, entry) in tail {
            self.file.seek(SeekFrom::Start(entry.offset))?;
            let mut remaining = entry.len as usize;
            offsets[*index] = copy.written;
            while remaining > 0 {
                let take = remaining.min(buffer.len());
                self.file.read_exact(&mut buffer[..take])?;
                dest.write_all(&buffer[..take])?;
                copy.written += take as u64;
                remaining -= take;
            }
        }
        dest.flush()?;
        dest.sync_all()?;
        Ok(())
    }

    /// Swap the rewritten file in and re-point every row offset. The new
    /// handle is opened before the rename, so a failure leaves the old file
    /// and its offsets untouched.
    fn install(&mut self, copy: &CompactCopy, offsets: Vec<u64>) -> std::io::Result<()> {
        if copy.generation != self.generation {
            return Err(std::io::Error::other("stale compaction result"));
        }
        if offsets.len() != self.entries.len() {
            return Err(std::io::Error::other("entry set changed during compaction"));
        }
        let file = OpenOptions::new().read(true).write(true).open(&copy.tmp)?;
        replace_spool_file(&copy.tmp, &self.path)?;
        drop(std::mem::replace(&mut self.file, file));
        for (entry, offset) in self.entries.iter_mut().zip(offsets) {
            entry.offset = offset;
        }
        self.file_len = copy.written;
        self.generation = self.generation.wrapping_add(1);
        // The copy also holds rows that were live at snapshot time but have
        // been evicted (or re-spooled) since, plus the re-appended tail, so
        // the file is not garbage-free yet.
        self.dead_bytes = copy.written.saturating_sub(self.live_bytes as u64);
        Ok(())
    }

    /// Copy live rows into a fresh file; offsets switch only after success.
    /// Runs on the reader only as the over-budget fallback.
    fn compact_sync(&mut self) -> std::io::Result<()> {
        let tmp_path = compact_tmp(&self.path, self.generation);
        let mut tmp = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)?;
        let mut buffer = vec![0u8; 64 * 1024];
        let mut written = 0u64;
        let mut new_offsets = Vec::with_capacity(self.entries.len());
        for entry in self.entries.iter() {
            self.file.seek(SeekFrom::Start(entry.offset))?;
            let mut remaining = entry.len as usize;
            new_offsets.push(written);
            while remaining > 0 {
                let take = remaining.min(buffer.len());
                self.file.read_exact(&mut buffer[..take])?;
                tmp.write_all(&buffer[..take])?;
                written += take as u64;
                remaining -= take;
            }
        }
        tmp.flush()?;
        tmp.sync_all()?;
        replace_spool_file(&tmp_path, &self.path)?;
        self.file = tmp;
        self.file_len = written;
        self.dead_bytes = 0;
        self.generation = self.generation.wrapping_add(1);
        for (entry, offset) in self.entries.iter_mut().zip(new_offsets) {
            entry.offset = offset;
        }
        Ok(())
    }

    /// Decode the row at `index` (0 = oldest retained).
    pub fn get(&mut self, index: usize) -> Option<Vec<CellData>> {
        let entry = *self.entries.get(index)?;
        if entry.len == 0 {
            // Oversized-row placeholder: the row existed but was never spooled.
            return None;
        }
        let mut buffer = vec![0u8; entry.len as usize];
        self.file.seek(SeekFrom::Start(entry.offset)).ok()?;
        self.file.read_exact(&mut buffer).ok()?;
        decode_row(&buffer)
    }

    /// Decode the row with stable id `id`.
    pub fn get_by_id(&mut self, id: u64) -> Option<Vec<CellData>> {
        let index = self.index_of(id)?;
        self.get(index)
    }

    fn index_of(&self, id: u64) -> Option<usize> {
        // Ids are ordered but not contiguous (reflow jumps, placeholders), so
        // binary-search instead of subtracting `first`.
        self.entries
            .binary_search_by_key(&id, |entry| entry.id)
            .ok()
    }

    /// Smart-case regex scan over retained rows. The public signature is
    /// unchanged; the index/handle snapshot is taken here and the scan itself
    /// runs on the caller's worker without holding the spool lock.
    pub fn search(
        &mut self,
        needle: &str,
        limit: usize,
        emit: impl FnMut(u64, usize, usize),
        cancel: Option<&AtomicBool>,
    ) {
        if let Ok(snapshot) = self.search_snapshot() {
            snapshot.search(needle, limit, emit, cancel);
        }
    }

    /// Copy only what the scan needs: the fd (so offsets keep pointing at the
    /// right inode across compaction) and the row index. Cheap enough to take
    /// under the spool lock on every keystroke.
    pub(crate) fn search_snapshot(&self) -> std::io::Result<ColdSearchSnapshot> {
        Ok(ColdSearchSnapshot {
            file: self.file.try_clone()?,
            entries: self.entries.iter().copied().collect(),
        })
    }

    /// Stop an in-flight rewrite and remove its generation-tagged temp file.
    /// The worker only owns its own handles, so joining under the spool lock
    /// cannot deadlock with the reader.
    fn cancel_compaction(&mut self) {
        let Some(job) = self.compact_job.take() else {
            return;
        };
        job.cancel.store(true, Ordering::Relaxed);
        let tmp = compact_tmp(&self.path, job.generation);
        match job.handle.join() {
            Ok(Ok(copy)) => {
                let _ = std::fs::remove_file(&copy.tmp);
            }
            Ok(Err(_)) | Err(_) => {
                let _ = std::fs::remove_file(&tmp);
            }
        }
    }

    /// Drop every retained row and truncate the file. Any rewrite from an
    /// older generation is cancelled and its result invalidated first.
    pub fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.cancel_compaction();
        self.entries.clear();
        self.live_bytes = 0;
        self.dead_bytes = 0;
        self.file_len = 0;
        let _ = self.file.set_len(0);
        let _ = self.file.seek(SeekFrom::Start(0));
    }
}

impl ColdSearchSnapshot {
    /// Same query semantics as the hot grid: rio's `RegexSearch` applies
    /// smart-case (lowercase queries fold case, uppercase queries do not)
    /// and accepts the same regex syntax. Invalid patterns fall back to the
    /// literal scan used by the hot grid's invalid-regex path.
    pub(crate) fn search(
        &self,
        query: &str,
        limit: usize,
        mut emit: impl FnMut(u64, usize, usize),
        cancel: Option<&AtomicBool>,
    ) {
        if query.is_empty() || limit == 0 {
            return;
        }
        let mut regex = RegexSearch::new(query).ok();
        let mut scratch = regex.as_ref().map(|_| scratch_terminal(1));
        let folded = query.to_lowercase();
        let mut hits = 0usize;
        let mut row_buffer = vec![0u8; 64 * 1024];
        for entry in &self.entries {
            if hits >= limit {
                break;
            }
            if cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
                return;
            }
            if entry.len == 0 {
                continue;
            }
            if row_buffer.len() < entry.len as usize {
                row_buffer.resize(entry.len as usize, 0);
            }
            let len = entry.len as usize;
            match self.file.read_at(&mut row_buffer[..len], entry.offset) {
                Ok(read) if read == len => {}
                _ => continue,
            }
            let Some(cells) = decode_row(&row_buffer[..entry.len as usize]) else {
                continue;
            };
            if let Some(regex) = regex.as_mut() {
                for (col, len) in regex_spans(
                    &cells,
                    scratch.as_mut().expect("scratch"),
                    regex,
                    limit - hits,
                ) {
                    emit(entry.id, col, len);
                    hits += 1;
                    if hits >= limit {
                        return;
                    }
                }
            } else {
                let (text, columns) = row_text(&cells);
                let mut spans = Vec::new();
                find_matches(&text, &columns, &folded, limit - hits, &mut spans);
                for (col, len) in spans {
                    emit(entry.id, col, len);
                    hits += 1;
                    if hits >= limit {
                        return;
                    }
                }
            }
        }
    }
}

fn scratch_terminal(cols: usize) -> Crosswords<VoidListener> {
    Crosswords::new(
        CrosswordsSize::new(cols.max(1), 1),
        RioCursorShape::Block,
        VoidListener,
        WindowId::from(0),
        0,
        0,
    )
}

/// Run the same DFA the hot grid uses over one row's decoded cells. Scratch
/// cells are written at their original columns (without wide/spacer flags,
/// which `row_text` already removed), so reported columns and lengths stay in
/// the terminal's grid coordinate space.
fn regex_spans(
    cells: &[CellData],
    scratch: &mut Crosswords<VoidListener>,
    regex: &mut RegexSearch,
    limit: usize,
) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    if cells.is_empty() || limit == 0 {
        return out;
    }
    let cols = cells.len();
    if scratch.grid.columns() < cols {
        scratch.resize(CrosswordsSize::new(cols, 1));
    }
    {
        let row = &mut scratch.grid[Line(0)];
        for col in 0..row.len() {
            row[Column(col)] = Square::from_char(' ');
        }
    }
    let mut writes = Vec::with_capacity(cols);
    for (col, cell) in cells.iter().enumerate() {
        if cell.attrs & (ATTR_SPACER | ATTR_LEADING_SPACER) != 0 {
            continue;
        }
        let (base, zerowidth) = match cell.text.as_deref() {
            Some(text) if !text.is_empty() => {
                let mut chars = text.chars();
                let base = chars.next().unwrap_or(' ');
                (
                    if base == '\0' { ' ' } else { base },
                    chars.collect::<Vec<char>>(),
                )
            }
            _ => (
                if cell.ch == '\0' { ' ' } else { cell.ch },
                Vec::<char>::new(),
            ),
        };
        let mut square = Square::from_char(base);
        if !zerowidth.is_empty() {
            let id = scratch
                .grid
                .extras_table
                .alloc(rio_vt::crosswords::square::Extras {
                    zerowidth,
                    hyperlink: None,
                });
            if id != 0 {
                square.set_extras_id(Some(id));
                square.insert_cell_flag(CellFlags::GRAPHEME);
                scratch.grid[Line(0)].has_extras = true;
            }
        }
        writes.push((col, square));
    }
    {
        let row = &mut scratch.grid[Line(0)];
        for (col, square) in writes {
            row[Column(col)] = square;
        }
    }
    let start = Pos {
        row: Line(0),
        col: Column(0),
    };
    let end = Pos {
        row: Line(0),
        col: Column(cols - 1),
    };
    let mut origin = start;
    while out.len() < limit {
        let Some(hit) = scratch.regex_search_right(regex, origin, end) else {
            break;
        };
        let from = hit.start().col.0;
        let to = hit.end().col.0;
        if from >= cols {
            break;
        }
        out.push((from.min(cols - 1), to.saturating_sub(from) + 1));
        let next = to.saturating_add(1);
        if next >= cols {
            break;
        }
        origin = Pos {
            row: Line(0),
            col: Column(next),
        };
    }
    out
}

/// Worker side of an asynchronous rewrite: copy the live rows through an
/// independent read handle so the store keeps serving rows and appending to
/// the old file meanwhile. `cancel` lets a reset stop the copy promptly.
fn copy_live_rows(
    source: &Path,
    tmp: &Path,
    entries: &[Entry],
    cancel: &AtomicBool,
    generation: u64,
) -> std::io::Result<CompactCopy> {
    let mut src = File::open(source)?;
    let mut dest = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(tmp)?;
    let mut buffer = vec![0u8; 64 * 1024];
    let mut written = 0u64;
    let mut rows = Vec::with_capacity(entries.len());
    for entry in entries {
        if cancel.load(Ordering::Relaxed) {
            let _ = std::fs::remove_file(tmp);
            return Err(std::io::Error::other("compaction cancelled"));
        }
        src.seek(SeekFrom::Start(entry.offset))?;
        let mut remaining = entry.len as usize;
        rows.push((entry.id, written));
        while remaining > 0 {
            let take = remaining.min(buffer.len());
            src.read_exact(&mut buffer[..take])?;
            dest.write_all(&buffer[..take])?;
            written += take as u64;
            remaining -= take;
        }
    }
    dest.flush()?;
    dest.sync_all()?;
    Ok(CompactCopy {
        tmp: tmp.to_path_buf(),
        written,
        rows,
        generation,
    })
}

impl Drop for ColdStore {
    fn drop(&mut self) {
        // A worker outliving the store must not recreate its temp file here.
        self.cancel_compaction();
        // Session close removes the spooled history from disk.
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(compact_tmp(&self.path, self.generation));
    }
}

/// Text plus column map for one decoded row; wide-glyph continuation cells are
/// skipped.
pub fn row_text(cells: &[CellData]) -> (String, Vec<usize>) {
    let mut text = String::with_capacity(cells.len());
    let mut columns = Vec::with_capacity(cells.len());
    for (col, cell) in cells.iter().enumerate() {
        if cell.attrs & crate::session::ATTR_SPACER != 0
            || cell.attrs & crate::session::ATTR_LEADING_SPACER != 0
        {
            continue;
        }
        columns.push(col);
        if let Some(cluster) = &cell.text {
            text.push_str(cluster);
        } else if cell.ch == '\0' {
            // Untouched cells are blank; emit a space so char positions keep
            // matching grid columns.
            text.push(' ');
        } else {
            text.push(cell.ch);
        }
    }
    (text, columns)
}

/// Case-insensitive span search shared by the hot grid and the cold spool.
pub fn find_matches(
    text: &str,
    columns: &[usize],
    needle_lower: &str,
    limit: usize,
    out: &mut Vec<(usize, usize)>,
) {
    if needle_lower.is_empty() || limit == 0 {
        return;
    }
    let hay_l = text.to_lowercase();
    let needle_chars = needle_lower.chars().count();
    if hay_l.len() == text.len() {
        for (byte_pos, _) in hay_l.match_indices(needle_lower) {
            let char_pos = hay_l[..byte_pos].chars().count();
            out.push((columns.get(char_pos).copied().unwrap_or(0), needle_chars));
            if out.len() >= limit {
                return;
            }
        }
    } else {
        // Lowercasing changed the byte length (e.g. `İ`); match per char so
        // byte offsets never point into the middle of a sequence.
        for (byte_pos, _) in text.char_indices() {
            if text[byte_pos..].to_lowercase().starts_with(needle_lower) {
                out.push((
                    columns
                        .get(text[..byte_pos].chars().count())
                        .copied()
                        .unwrap_or(0),
                    needle_chars,
                ));
                if out.len() >= limit {
                    return;
                }
            }
        }
    }
}

/// Serialise a row. Trailing default cells are dropped (`logical_len`) and
/// restored as blanks on decode, which keeps mostly-empty rows cheap.
pub fn encode_row(cells: &[CellData]) -> Vec<u8> {
    let cols = cells.len().min(u16::MAX as usize);
    let logical = cells[..cols]
        .iter()
        .rposition(|cell| !is_default(cell))
        .map(|index| index + 1)
        .unwrap_or(0);
    let mut out = Vec::with_capacity(4 + logical * 16);
    out.extend_from_slice(&(cols as u16).to_le_bytes());
    out.extend_from_slice(&(logical as u16).to_le_bytes());
    for cell in &cells[..logical] {
        out.extend_from_slice(&(cell.ch as u32).to_le_bytes());
        out.extend_from_slice(&cell.attrs.to_le_bytes());
        encode_color(&mut out, cell.fg);
        encode_color(&mut out, cell.bg);
        match &cell.text {
            Some(text) if !text.is_empty() => {
                let bytes = text.as_bytes();
                let len = bytes.len().min(u16::MAX as usize);
                out.extend_from_slice(&(len as u16).to_le_bytes());
                out.extend_from_slice(&bytes[..len]);
            }
            _ => out.extend_from_slice(&0u16.to_le_bytes()),
        }
    }
    out
}

/// Inverse of [`encode_row`]; the row width is part of the payload.
pub fn decode_row(bytes: &[u8]) -> Option<Vec<CellData>> {
    if bytes.len() < 4 {
        return None;
    }
    let cols = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
    let logical = u16::from_le_bytes([bytes[2], bytes[3]]) as usize;
    let mut cells = vec![CellData::default(); cols];
    let mut reader = &bytes[4..];
    for slot in cells.iter_mut().take(logical.min(cols)) {
        let ch = u32::from_le_bytes(take::<4>(&mut reader)?);
        let attrs = u16::from_le_bytes(take::<2>(&mut reader)?);
        let fg = decode_color(&mut reader)?;
        let bg = decode_color(&mut reader)?;
        let text_len = u16::from_le_bytes(take::<2>(&mut reader)?) as usize;
        let text = if text_len == 0 {
            None
        } else {
            let raw = take_slice(&mut reader, text_len)?;
            // A truncated multibyte sequence would make the whole row
            // useless; treat it as absent instead of failing the row.
            std::str::from_utf8(raw)
                .ok()
                .map(std::sync::Arc::<str>::from)
        };
        *slot = CellData {
            ch: char::from_u32(ch).unwrap_or('\0'),
            text,
            fg,
            bg,
            attrs,
        };
    }
    Some(cells)
}

fn is_default(cell: &CellData) -> bool {
    cell.ch == '\0'
        && cell.text.is_none()
        && cell.fg == ColorSpec::DefaultFg
        && cell.bg == ColorSpec::DefaultBg
        && cell.attrs == 0
}

fn encode_color(out: &mut Vec<u8>, spec: ColorSpec) {
    match spec {
        ColorSpec::DefaultFg => out.push(0),
        ColorSpec::DefaultBg => out.push(1),
        ColorSpec::Cursor => out.push(2),
        ColorSpec::Indexed(index) => {
            out.push(3);
            out.push(index);
        }
        ColorSpec::Rgb(r, g, b) => {
            out.push(4);
            out.extend_from_slice(&[r, g, b]);
        }
    }
}

fn decode_color(reader: &mut &[u8]) -> Option<ColorSpec> {
    let tag = take_slice(reader, 1)?[0];
    Some(match tag {
        0 => ColorSpec::DefaultFg,
        1 => ColorSpec::DefaultBg,
        2 => ColorSpec::Cursor,
        3 => ColorSpec::Indexed(take_slice(reader, 1)?[0]),
        4 => {
            let rgb = take_slice(reader, 3)?;
            ColorSpec::Rgb(rgb[0], rgb[1], rgb[2])
        }
        _ => return None,
    })
}

fn take<const N: usize>(reader: &mut &[u8]) -> Option<[u8; N]> {
    if reader.len() < N {
        return None;
    }
    let (head, tail) = reader.split_at(N);
    *reader = tail;
    let mut out = [0u8; N];
    out.copy_from_slice(head);
    Some(out)
}

fn take_slice<'a>(reader: &mut &'a [u8], len: usize) -> Option<&'a [u8]> {
    if reader.len() < len {
        return None;
    }
    let (head, tail) = reader.split_at(len);
    *reader = tail;
    Some(head)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{ATTR_BOLD, ATTR_WIDE};

    fn cell(ch: char) -> CellData {
        CellData {
            ch,
            ..Default::default()
        }
    }

    fn row(chars: &str) -> Vec<CellData> {
        chars
            .chars()
            .map(|ch| {
                if ch == ' ' {
                    CellData::default()
                } else {
                    cell(ch)
                }
            })
            .collect()
    }

    #[test]
    fn row_round_trip_preserves_colors_attrs_and_graphemes() {
        let mut cells = row("ab ");
        cells[0].fg = ColorSpec::Indexed(196);
        cells[1].bg = ColorSpec::Rgb(1, 2, 3);
        cells[1].attrs = ATTR_BOLD | ATTR_WIDE;
        cells[2].text = Some(std::sync::Arc::<str>::from("e\u{301}"));
        let encoded = encode_row(&cells);
        let decoded = decode_row(&encoded).expect("decode");
        assert_eq!(decoded.len(), cells.len());
        assert_eq!(decoded[0].ch, 'a');
        assert_eq!(decoded[0].fg, ColorSpec::Indexed(196));
        assert_eq!(decoded[1].bg, ColorSpec::Rgb(1, 2, 3));
        assert_eq!(decoded[1].attrs, ATTR_BOLD | ATTR_WIDE);
        assert_eq!(decoded[2].display_text(), "e\u{301}");
    }

    #[test]
    fn trailing_blank_cells_are_not_written() {
        let cells = row("hi        ");
        let encoded = encode_row(&cells);
        let decoded = decode_row(&encoded).expect("decode");
        assert_eq!(decoded.len(), 10);
        assert_eq!(decoded[0].ch, 'h');
        assert_eq!(decoded[2].ch, '\0');
        // 4 header bytes + 2 cells (4 ch + 2 attrs + a color tag each + 2
        // text-length bytes = 10 bytes) instead of all ten cells.
        assert_eq!(encoded.len(), 4 + 2 * 10);
    }

    #[test]
    fn spools_rows_in_id_order_and_reads_them_back() {
        let mut store = ColdStore::create(100, 1 << 20).expect("create");
        let rows = vec![
            encode_row(&row("one")),
            encode_row(&row("two")),
            encode_row(&row("three")),
        ];
        store.append_rows(7, &rows);
        assert_eq!(store.len(), 3);
        assert_eq!(store.first_id(), Some(7));
        assert_eq!(store.last_id(), Some(9));
        let text = |cells: Vec<CellData>| row_text(&cells).0.trim_end().to_string();
        assert_eq!(text(store.get(0).expect("row 0")), "one");
        assert_eq!(text(store.get(2).expect("row 2")), "three");
        assert_eq!(text(store.get_by_id(8).expect("row 8")), "two");
        assert!(store.get_by_id(6).is_none());
        assert!(store.get(3).is_none());
    }

    #[test]
    fn ids_with_gaps_resolve_exactly_instead_of_shifting() {
        // Reflow leaves gaps in the id range; binary-search or later rows
        // resolve to their pre-gap neighbours.
        let mut store = ColdStore::create(100, 1 << 20).expect("create");
        let before = vec![
            encode_row(&row("before-gap")),
            encode_row(&row("also-before")),
        ];
        store.append_rows(0, &before);
        let after = vec![encode_row(&row("after-gap"))];
        store.append_rows(50, &after);
        let text = |cells: Vec<CellData>| row_text(&cells).0.trim_end().to_string();
        assert_eq!(text(store.get_by_id(0).expect("id 0")), "before-gap");
        assert_eq!(text(store.get_by_id(1).expect("id 1")), "also-before");
        assert_eq!(text(store.get_by_id(50).expect("id 50")), "after-gap");
        // In-hole ids resolve to nothing, never to a shifted row.
        assert!(store.get_by_id(2).is_none());
        assert!(store.get_by_id(25).is_none());
        assert!(store.get_by_id(49).is_none());
    }

    #[test]
    fn oversized_rows_keep_later_ids_aligned() {
        let mut store = ColdStore::create(100, 4096).expect("create");
        let rows = vec![
            encode_row(&row("kept")),
            vec![0u8; 8192], // larger than the byte budget: not spooled
            encode_row(&row("kept-after-oversize")),
        ];
        store.append_rows(10, &rows);
        let text = |cells: Vec<CellData>| row_text(&cells).0.trim_end().to_string();
        assert_eq!(text(store.get_by_id(10).expect("id 10")), "kept");
        assert!(
            store.get_by_id(11).is_none(),
            "oversized row is a placeholder"
        );
        assert_eq!(
            text(store.get_by_id(12).expect("id 12")),
            "kept-after-oversize",
            "the id after a skipped row must not shift"
        );
    }

    #[test]
    fn evicts_oldest_rows_past_the_line_budget() {
        let mut store = ColdStore::create(3, 1 << 20).expect("create");
        let rows: Vec<Vec<u8>> = (0..6)
            .map(|index| encode_row(&row(&format!("row{index}"))))
            .collect();
        store.append_rows(0, &rows);
        assert_eq!(store.len(), 3);
        assert_eq!(store.first_id(), Some(3));
        assert_eq!(store.last_id(), Some(5));
        assert!(store.get_by_id(0).is_none());
    }

    #[test]
    fn evicts_oldest_rows_past_the_byte_budget_and_compacts() {
        // The byte budget (clamped to 4 KiB) holds only a few rows; the rest
        // are evicted and the file is compacted once dead bytes dominate.
        let mut store = ColdStore::create(100_000, 4096).expect("create");
        let mut next_id = 0u64;
        for batch in 0..20 {
            let rows: Vec<Vec<u8>> = (0..10)
                .map(|index| encode_row(&row(&format!("row-{batch:02}-{index:02}"))))
                .collect();
            store.append_rows(next_id, &rows);
            next_id += rows.len() as u64;
        }
        assert_eq!(next_id, 200);
        assert!(store.len() < 200, "byte budget must bound the spool");
        assert!(store.bytes() <= 4096);
        assert_eq!(store.last_id(), Some(199));
        let expected_first = 200 - store.len() as u64;
        assert_eq!(store.first_id(), Some(expected_first));
        // Compaction rewrote the file around the live rows.
        assert!(store.file_bytes() <= 4096 + COMPACT_MIN_BYTES);
    }

    #[test]
    fn search_reports_row_index_and_column() {
        let mut store = ColdStore::create(100, 1 << 20).expect("create");
        let rows = vec![
            encode_row(&row("alpha marker")),
            encode_row(&row("nothing here")),
            encode_row(&row("MARKER again")),
        ];
        store.append_rows(100, &rows);
        let mut hits = Vec::new();
        store.search(
            "marker",
            10,
            |id, col, len| {
                hits.push((id, col, len));
            },
            None,
        );
        assert_eq!(hits, vec![(100, 6, 6), (102, 0, 6)]);
    }

    #[test]
    fn cold_search_uses_the_hot_grids_smart_case_regex() {
        let mut store = ColdStore::create(100, 1 << 20).expect("create");
        let rows = vec![
            encode_row(&row("row-17-marker")),
            encode_row(&row("row-18-MARKER")),
            encode_row(&row("row-19-marker")),
            encode_row(&row("[unclosed marker")),
        ];
        store.append_rows(10, &rows);
        let mut hits = |query: &str| {
            let mut hits = Vec::new();
            store.search(query, 20, |id, col, len| hits.push((id, col, len)), None);
            hits
        };
        // Regex classes work across cold rows, and a lowercase query folds
        // case exactly like rio's hot-grid DFA.
        assert_eq!(
            hits("row-1[7-9]-marker"),
            vec![(10, 0, 13), (11, 0, 13), (12, 0, 13)]
        );
        // Any uppercase letter opts out of folding for the whole query.
        assert!(hits("row-17-MARKER").is_empty());
        assert_eq!(hits("row-18-MARKER"), vec![(11, 0, 13)]);
        // An invalid regex falls back to the hot path's literal semantics.
        assert_eq!(hits("[unclosed"), vec![(13, 0, 9)]);
    }

    #[test]
    fn snapshot_search_runs_without_the_spool_lock() {
        let mut store = ColdStore::create(100, 1 << 20).expect("create");
        store.append_rows(0, &[encode_row(&row("marker row"))]);
        let snapshot = store.search_snapshot().expect("snapshot");
        let locked_store = parking_lot::Mutex::new(store);
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut hits = Vec::new();
            snapshot.search("marker", 10, |id, col, len| hits.push((id, col, len)), None);
            let _ = tx.send(hits);
        });
        // If any part of the scan needed the store lock this would deadlock
        // until the 2s timeout instead of returning hits.
        let guard = locked_store.lock();
        let hits = rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("cold scan blocked on the spool lock");
        drop(guard);
        worker.join().expect("search worker");
        assert_eq!(hits, vec![(0, 0, 6)]);
    }

    #[test]
    fn clear_cancels_and_invalidates_an_in_flight_rewrite() {
        let mut store = ColdStore::create(10, 1 << 20).expect("create");
        store.append_rows(0, &[encode_row(&row("old row"))]);
        let generation = store.generation;
        let tmp = compact_tmp(store.path(), generation);
        std::fs::write(&tmp, b"stale").expect("stale temp");
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let worker_tmp = tmp.clone();
        let handle = std::thread::spawn(move || {
            while !worker_cancel.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Ok::<_, std::io::Error>(CompactCopy {
                tmp: worker_tmp,
                written: 5,
                rows: vec![(0, 0)],
                generation,
            })
        });
        store.compact_job = Some(CompactJob {
            handle,
            generation,
            cancel,
        });
        store.clear();
        assert!(store.compact_job.is_none());
        assert_eq!(store.len(), 0);
        assert_eq!(store.file_bytes(), 0);
        assert!(!tmp.exists(), "the cancelled worker's temp must be removed");
        store.poll_compaction();
        assert_eq!(store.len(), 0, "a cleared store must not regain rows");
        assert!(store.path().exists());
    }

    #[test]
    fn stale_compaction_result_is_not_installed() {
        let mut store = ColdStore::create(10, 1 << 20).expect("create");
        store.append_rows(0, &[encode_row(&row("kept row"))]);
        let generation = store.generation;
        let tmp = compact_tmp(store.path(), generation);
        // A stale worker's temp holds junk that would destroy the row if the
        // result were adopted after a reset.
        std::fs::write(&tmp, b"junk").expect("stale temp");
        let worker_tmp = tmp.clone();
        let handle = std::thread::spawn(move || {
            Ok::<_, std::io::Error>(CompactCopy {
                tmp: worker_tmp,
                written: 4,
                rows: vec![(0, 0)],
                generation,
            })
        });
        store.compact_job = Some(CompactJob {
            handle,
            generation,
            cancel: Arc::new(AtomicBool::new(false)),
        });
        store.generation = generation + 1;
        for _ in 0..1000 {
            store.poll_compaction();
            if !tmp.exists() {
                break;
            }
            std::thread::yield_now();
        }
        assert!(!tmp.exists());
        let cells = store.get(0).expect("live row survived");
        assert_eq!(row_text(&cells).0.trim_end(), "kept row");
    }

    #[test]
    fn failed_install_keeps_the_live_spool() {
        let mut store = ColdStore::create(10, 1 << 20).expect("create");
        store.append_rows(0, &[encode_row(&row("kept row"))]);
        let path = store.path().to_path_buf();
        let copy = CompactCopy {
            tmp: path.with_file_name("lazecode-cold-missing.compact-9"),
            written: 0,
            rows: Vec::new(),
            generation: store.generation,
        };
        assert!(store.install(&copy, vec![0]).is_err());
        assert!(path.exists(), "a failed swap must not delete the old spool");
        let cells = store.get(0).expect("old index still valid");
        assert_eq!(row_text(&cells).0.trim_end(), "kept row");
    }

    #[test]
    fn clear_and_drop_remove_the_file() {
        let mut store = ColdStore::create(10, 1 << 20).expect("create");
        store.append_rows(0, &[encode_row(&row("hello"))]);
        let path = store.path().to_path_buf();
        assert!(path.exists());
        store.clear();
        assert_eq!(store.len(), 0);
        assert_eq!(store.file_bytes(), 0);
        drop(store);
        assert!(!path.exists());
    }

    #[test]
    fn async_compaction_rewrites_the_file_and_keeps_every_live_row() {
        // 8 live rows, 1MiB payload budget, ~23MB written: dead bytes cross
        // the rewrite threshold well below the forced-in-place ceiling, so
        // the background path is the one exercised.
        let mut store = ColdStore::create(8, 1 << 20).expect("create");
        let payload = "x".repeat(180);
        let rows: Vec<Vec<u8>> = (0..12_000u64)
            .map(|id| encode_row(&row(&format!("{id:06}{payload}"))))
            .collect();
        for (chunk_index, chunk) in rows.chunks(64).enumerate() {
            store.append_rows(chunk_index as u64 * 64, chunk);
        }
        let before = store.file_bytes();
        assert!(
            before > COMPACT_MIN_BYTES,
            "dead bytes should have accumulated, file={before}"
        );
        let live = store.len();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            store.poll_compaction();
            if store.file_bytes() <= store.bytes() as u64 + 4096 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "compaction never finished: file={} live={}",
                store.file_bytes(),
                store.bytes()
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            store.file_bytes() < before,
            "rewrite must shrink the file: {} -> {}",
            before,
            store.file_bytes()
        );
        // Every retained row must still decode to its own id.
        let first = store.first_id().expect("live rows");
        for id in first..first + live as u64 {
            let cells = store
                .get_by_id(id)
                .unwrap_or_else(|| panic!("row {id} lost"));
            let text: String = cells
                .iter()
                .filter(|cell| cell.ch != '\0')
                .map(|cell| cell.ch)
                .collect();
            assert!(
                text.starts_with(&format!("{id:06}")),
                "row {id} bound to the wrong payload: {text:?}"
            );
        }
    }
}
