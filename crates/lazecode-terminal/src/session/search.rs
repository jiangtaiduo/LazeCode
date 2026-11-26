//! Full-history search across the hot grid and the cold spool.

use super::*;

/// Full-history search shared by the synchronous and background entry points.
pub(super) fn search_all(
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
    // Only the index and a cloned fd are copied under the lock. Disk reads,
    // decoding and regex matching run on this worker without blocking the
    // reader that appends rows and polls compaction.
    let snapshot = {
        let spool = spool.lock();
        spool
            .store
            .as_ref()
            .and_then(|store| store.search_snapshot().ok())
    };
    if let Some(snapshot) = snapshot {
        let remaining = 2_000usize.saturating_sub(matches.len());
        if remaining > 0 {
            snapshot.search(
                query,
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
pub(super) fn search_hot(
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
pub(super) fn search_literal_locked(
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
