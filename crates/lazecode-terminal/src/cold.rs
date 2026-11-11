//! Cold (disk-backed) scrollback spool.
//!
//! `rio-vt` keeps the last ``hot`` history lines in its in-memory grid. Lines
//! that would roll out of that window are serialised here, keyed by the
//! terminal's stable absolute row id (`grid.lines_evicted()` base), so the
//! scrollback can grow past the RAM budget without unbounded memory use.
//!
//! The spool is an append-only file with an in-memory index; the oldest lines
//! are evicted once either budget is exceeded and the file is compacted when
//! the dead fraction grows, so disk usage stays bounded too.

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::session::{CellData, ColorSpec};

/// Compact once the dead bytes in the file exceed this and outnumber the live
/// rows.
const COMPACT_MIN_BYTES: u64 = 1 << 20;

static NEXT_FILE: AtomicU64 = AtomicU64::new(1);

/// One spooled row on disk.
#[derive(Clone, Copy, Debug)]
struct Entry {
    /// Stable absolute terminal line id this row was captured from.
    id: u64,
    offset: u64,
    len: u32,
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
}

impl ColdStore {
    /// Create a spool in the system temp directory.
    ///
    /// `line_budget`/`byte_budget` bound the retained rows; both are clamped
    /// to sane minimums so a misconfigured zero cannot make the spool useless.
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

    /// Append a run of encoded rows whose ids start at `first_id`.
    ///
    /// Ids must increase: the index is ordered by id so a visible row can be
    /// resolved with a binary search. A stale or replayed batch is dropped
    /// instead of corrupting that order.
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
        let mut offsets = Vec::with_capacity(rows.len());
        let mut offset = self.file_len;
        for row in rows {
            if row.len() as u64 > self.byte_budget as u64 {
                continue;
            }
            offsets.push((offset, row.len() as u32));
            offset += row.len() as u64;
            buffer.extend_from_slice(row);
        }
        if buffer.is_empty() {
            return;
        }
        if self
            .file
            .seek(SeekFrom::Start(self.file_len))
            .and_then(|_| self.file.write_all(&buffer))
            .is_err()
        {
            log::warn!("cold scrollback spool: write failed, dropping rows");
            return;
        }
        if self.entries.is_empty() {
            self.first_id = first_id;
        }
        for (index, (offset, len)) in offsets.into_iter().enumerate() {
            self.entries.push_back(Entry {
                id: first_id + index as u64,
                offset,
                len,
            });
            self.live_bytes += len as usize;
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
            if let Err(err) = self.compact() {
                log::warn!("cold scrollback spool compaction failed: {err}");
                self.dead_bytes = 0;
            }
        }
    }

    /// Rewrite the live rows into a fresh file so evicted payload is released.
    fn compact(&mut self) -> std::io::Result<()> {
        let tmp_path = self.path.with_extension("compact");
        let mut tmp = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)?;
        let mut buffer = vec![0u8; 64 * 1024];
        let mut written = 0u64;
        for entry in self.entries.iter_mut() {
            self.file.seek(SeekFrom::Start(entry.offset))?;
            let mut remaining = entry.len as usize;
            entry.offset = written;
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
        drop(tmp);
        self.file = OpenOptions::new().read(true).write(true).open(&tmp_path)?;
        std::fs::remove_file(&self.path)?;
        std::fs::rename(&tmp_path, &self.path)?;
        self.file_len = written;
        self.dead_bytes = 0;
        Ok(())
    }

    /// Decode the row at `index` (0 = oldest retained).
    pub fn get(&mut self, index: usize) -> Option<Vec<CellData>> {
        let entry = *self.entries.get(index)?;
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
        let first = self.entries.front()?.id;
        if id < first {
            return None;
        }
        let index = (id - first) as usize;
        (index < self.entries.len()).then_some(index)
    }

    /// Case-insensitive scan over every retained row.
    ///
    /// `emit(id, column, len)` receives the row's stable absolute line id plus
    /// a column-span hit; scanning stops after `limit` hits. The id (not the
    /// index) is what the terminal maps back into viewport coordinates.
    pub fn search(
        &mut self,
        needle_lower: &str,
        limit: usize,
        mut emit: impl FnMut(u64, usize, usize),
    ) {
        if needle_lower.is_empty() || limit == 0 {
            return;
        }
        let mut hits = 0usize;
        let mut row_buffer = vec![0u8; 64 * 1024];
        for entry in self.entries.iter() {
            if hits >= limit {
                break;
            }
            if row_buffer.len() < entry.len as usize {
                row_buffer.resize(entry.len as usize, 0);
            }
            if self.file.seek(SeekFrom::Start(entry.offset)).is_err()
                || self
                    .file
                    .read_exact(&mut row_buffer[..entry.len as usize])
                    .is_err()
            {
                continue;
            }
            let Some(cells) = decode_row(&row_buffer[..entry.len as usize]) else {
                continue;
            };
            let (text, columns) = row_text(&cells);
            let mut spans = Vec::new();
            find_matches(&text, &columns, needle_lower, limit - hits, &mut spans);
            for (col, len) in spans {
                emit(entry.id, col, len);
                hits += 1;
                if hits >= limit {
                    return;
                }
            }
        }
    }

    /// Drop every retained row and truncate the file.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.live_bytes = 0;
        self.dead_bytes = 0;
        self.file_len = 0;
        let _ = self.file.set_len(0);
        let _ = self.file.seek(SeekFrom::Start(0));
    }
}

impl Drop for ColdStore {
    fn drop(&mut self) {
        // Session close removes the spooled history from disk.
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(self.path.with_extension("compact"));
    }
}

/// Text + column map for one decoded row.
///
/// Continuation cells of wide glyphs are skipped, matching the rendering and
/// selection code paths.
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
        store.search("marker", 10, |id, col, len| {
            hits.push((id, col, len));
        });
        assert_eq!(hits, vec![(100, 6, 6), (102, 0, 6)]);
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
}
