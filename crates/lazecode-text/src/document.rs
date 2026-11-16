use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ropey::Rope;

use crate::syntax::{comment_tokens, SyntaxHighlighter};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cursor {
    pub line: usize,
    /// Character offset inside the line (tabs count as one char).
    pub col: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Cursor,
    pub head: Cursor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    LineStart,
    LineEnd,
    WordLeft,
    WordRight,
    DocStart,
    DocEnd,
    PageUp(usize),
    PageDown(usize),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MatchOptions {
    pub case_sensitive: bool,
    pub regex: bool,
    pub whole_word: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MergeKind {
    None,
    Insert,
    DeleteBack,
    DeleteForward,
}

#[derive(Clone, Debug)]
struct Edit {
    start: usize,
    removed: String,
    inserted: String,
}

#[derive(Clone, Debug)]
struct EditGroup {
    edits: Vec<Edit>,
    before: Cursor,
    after: Cursor,
}

/// A rope-backed editable document.
pub struct Document {
    pub path: PathBuf,
    pub rope: Rope,
    pub encoding: Option<&'static encoding_rs::Encoding>,
    pub eol: lazecode_fs::Eol,
    pub had_bom: bool,
    pub lossy: bool,
    pub read_only: bool,
    pub dirty: bool,
    pub cursor: Cursor,
    pub selection: Option<Selection>,
    pub scroll_line: f32,
    pub scroll_x: f32,
    pub tab_width: usize,
    pub use_spaces: bool,
    pub revision: u64,
    pub highlighter: SyntaxHighlighter,
    undo: Vec<EditGroup>,
    redo: Vec<EditGroup>,
    merge_kind: MergeKind,
    last_edit_at: Option<Instant>,
}

impl Document {
    pub fn empty(path: PathBuf, tab_width: usize, use_spaces: bool) -> Self {
        Self {
            highlighter: SyntaxHighlighter::new(&path),
            path,
            rope: Rope::new(),
            encoding: None,
            eol: lazecode_fs::Eol::Lf,
            had_bom: false,
            lossy: false,
            read_only: false,
            dirty: false,
            cursor: Cursor::default(),
            selection: None,
            scroll_line: 0.0,
            scroll_x: 0.0,
            tab_width: tab_width.max(1),
            use_spaces,
            revision: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            merge_kind: MergeKind::None,
            last_edit_at: None,
        }
    }

    pub fn open(path: &Path, tab_width: usize, use_spaces: bool) -> anyhow::Result<Self> {
        let file = lazecode_fs::file::read_text_file(path)?;
        let mut doc = Self::empty(path.to_path_buf(), tab_width, use_spaces);
        doc.rope = Rope::from_str(&file.text);
        doc.encoding = file.encoding;
        doc.eol = file.eol;
        doc.had_bom = file.had_bom;
        doc.lossy = file.lossy;
        doc.read_only = file_is_read_only(path);
        Ok(doc)
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn title(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_string())
    }

    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn line(&self, index: usize) -> String {
        if index >= self.rope.len_lines() {
            return String::new();
        }
        self.rope.line(index).to_string()
    }

    pub fn line_content(&self, index: usize) -> String {
        let mut line = self.line(index);
        while line.ends_with('\n') || line.ends_with('\r') {
            line.pop();
        }
        line
    }

    pub fn line_content_len(&self, index: usize) -> usize {
        if index >= self.rope.len_lines() {
            return 0;
        }
        let slice = self.rope.line(index);
        let mut len = slice.len_chars();
        while len > 0 {
            let ch = slice.char(len - 1);
            if ch == '\n' || ch == '\r' {
                len -= 1;
            } else {
                break;
            }
        }
        len
    }

    /// Human label for the status bar (UTF-8 / UTF-16LE / … plus BOM).
    pub fn encoding_label(&self) -> String {
        let name = self
            .encoding
            .map(|encoding| encoding.name().to_string())
            .unwrap_or_else(|| "UTF-8".to_string());
        if self.had_bom {
            format!("{name} BOM")
        } else {
            name
        }
    }

    pub fn char_index(&self, cursor: Cursor) -> usize {
        let line = cursor.line.min(self.rope.len_lines().saturating_sub(1));
        let base = self.rope.line_to_char(line);
        base + cursor.col.min(self.line_content_len(line))
    }

    pub fn cursor_at(&self, char_index: usize) -> Cursor {
        let char_index = char_index.min(self.rope.len_chars());
        let line = self.rope.char_to_line(char_index);
        let base = self.rope.line_to_char(line);
        Cursor {
            line,
            col: (char_index - base).min(self.line_content_len(line)),
        }
    }

    /// LSP position for a cursor: zero-based line plus a UTF-16 code-unit
    /// column. `Cursor::col` counts Unicode scalars, so astral characters
    /// (emoji, some CJK extensions) occupy two LSP columns but one editor
    /// column.
    pub fn lsp_position(&self, cursor: Cursor) -> (u32, u32) {
        let line = cursor.line.min(self.rope.len_lines().saturating_sub(1));
        let content = self.line_content(line);
        (
            line as u32,
            crate::lsp::utf16_col_for_char(&content, cursor.col),
        )
    }

    /// Cursor for an LSP position. Out-of-range lines clamp to the last line
    /// and UTF-16 columns clamp to the line's end.
    pub fn lsp_cursor(&self, line: u32, character: u32) -> Cursor {
        let line = (line as usize).min(self.rope.len_lines().saturating_sub(1));
        let content = self.line_content(line);
        Cursor {
            line,
            col: crate::lsp::char_col_for_utf16(&content, character),
        }
    }

    /// Apply a batch of LSP edits as one undo group. Positions refer to the
    /// document *before* any of the edits; they must not overlap. Returns
    /// `false` (and leaves the document untouched) for an invalid batch.
    pub fn apply_text_edits(&mut self, edits: &[crate::LspTextEdit]) -> bool {
        if edits.is_empty() {
            return false;
        }
        let mut specs: Vec<(usize, usize, &str)> = Vec::with_capacity(edits.len());
        for edit in edits {
            if (edit.start_line, edit.start_character) > (edit.end_line, edit.end_character) {
                return false;
            }
            let lines = self.rope.len_lines();
            if edit.start_line as usize >= lines || edit.end_line as usize >= lines {
                return false;
            }
            let start = self.char_index(self.lsp_cursor(edit.start_line, edit.start_character));
            let end = self.char_index(self.lsp_cursor(edit.end_line, edit.end_character));
            if end < start {
                return false;
            }
            specs.push((start, end, edit.new_text.as_str()));
        }
        specs.sort_by_key(|(start, _, _)| *start);
        for pair in specs.windows(2) {
            if pair[0].1 > pair[1].0 {
                return false;
            }
        }

        // Map the old caret against the ascending edit list before applying
        // back-to-front, which invalidates raw offsets.
        let old_index = self.char_index(self.cursor);
        let mut mapped = old_index as isize;
        for (start, end, text) in &specs {
            let removed = (end - start) as isize;
            let inserted = text.chars().count() as isize;
            if *end <= old_index {
                mapped += inserted - removed;
            } else if *start < old_index {
                mapped = *start as isize + inserted;
            }
        }

        let before = self.cursor;
        let undo_len = self.undo.len();
        for (start, end, text) in specs.iter().rev() {
            self.apply_edit(*start, *end, text, MergeKind::None);
        }
        self.finish_multi_edit(undo_len, before);
        let mapped = mapped.max(0) as usize;
        self.cursor = self.cursor_at(mapped.min(self.rope.len_chars()));
        self.selection = None;
        true
    }

    pub fn selection_char_range(&self) -> Option<(usize, usize)> {
        let selection = self.selection?;
        let a = self.char_index(selection.anchor);
        let b = self.char_index(selection.head);
        if a == b {
            None
        } else {
            Some((a.min(b), a.max(b)))
        }
    }

    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection_char_range()?;
        Some(self.rope.slice(start..end).to_string())
    }

    /// Delete the current selection as one undoable edit.
    pub fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection_char_range() else {
            return false;
        };
        self.apply_edit(start, end, "", MergeKind::None);
        true
    }

    pub fn set_cursor(&mut self, cursor: Cursor, extend: bool) {
        let line = cursor.line.min(self.rope.len_lines().saturating_sub(1));
        let col = cursor.col.min(self.line_content_len(line));
        let next = Cursor { line, col };
        if extend {
            let anchor = self.selection.map(|s| s.anchor).unwrap_or(self.cursor);
            self.selection = Some(Selection { anchor, head: next });
        } else {
            self.selection = None;
        }
        self.cursor = next;
    }

    pub fn select_all(&mut self) {
        let last = self.rope.len_lines().saturating_sub(1);
        self.selection = Some(Selection {
            anchor: Cursor { line: 0, col: 0 },
            head: Cursor {
                line: last,
                col: self.line_content_len(last),
            },
        });
        self.cursor = self.selection.unwrap().head;
    }

    pub fn move_cursor(&mut self, motion: Motion, extend: bool) {
        let current = self.cursor;
        let mut next = current;
        match motion {
            Motion::Left => {
                if current.col > 0 {
                    next.col -= 1;
                } else if current.line > 0 {
                    next.line = current.line - 1;
                    next.col = self.line_content_len(next.line);
                }
            }
            Motion::Right => {
                if current.col < self.line_content_len(current.line) {
                    next.col += 1;
                } else if current.line + 1 < self.rope.len_lines() {
                    next.line += 1;
                    next.col = 0;
                }
            }
            Motion::Up => {
                next.line = current.line.saturating_sub(1);
                next.col = current.col.min(self.line_content_len(next.line));
            }
            Motion::Down => {
                if current.line + 1 < self.rope.len_lines() {
                    next.line = current.line + 1;
                    next.col = current.col.min(self.line_content_len(next.line));
                }
            }
            Motion::Home | Motion::LineStart => next.col = 0,
            Motion::End | Motion::LineEnd => next.col = self.line_content_len(current.line),
            Motion::WordLeft => {
                let index = self.prev_word_start(self.char_index(current));
                next = self.cursor_at(index);
            }
            Motion::WordRight => {
                let index = self.next_word_end(self.char_index(current));
                next = self.cursor_at(index);
            }
            Motion::DocStart => next = Cursor::default(),
            Motion::DocEnd => {
                let last = self.rope.len_lines().saturating_sub(1);
                next = Cursor {
                    line: last,
                    col: self.line_content_len(last),
                };
            }
            Motion::PageUp(lines) => {
                next.line = current.line.saturating_sub(lines);
                next.col = current.col.min(self.line_content_len(next.line));
            }
            Motion::PageDown(lines) => {
                next.line = (current.line + lines).min(self.rope.len_lines().saturating_sub(1));
                next.col = current.col.min(self.line_content_len(next.line));
            }
        }
        self.set_cursor(next, extend);
    }

    fn prev_word_start(&self, index: usize) -> usize {
        if index == 0 {
            return 0;
        }
        let mut i = index;
        while i > 0 && !is_word_char(self.rope.char(i - 1)) {
            i -= 1;
        }
        while i > 0 && is_word_char(self.rope.char(i - 1)) {
            i -= 1;
        }
        i
    }

    fn next_word_end(&self, index: usize) -> usize {
        let len = self.rope.len_chars();
        let mut i = index;
        while i < len && is_word_char(self.rope.char(i)) {
            i += 1;
        }
        while i < len && !is_word_char(self.rope.char(i)) {
            i += 1;
        }
        i
    }

    pub fn insert_text(&mut self, text: &str) {
        let text = normalise_newlines(text);
        if text.is_empty() {
            return;
        }
        let merge = if text.chars().count() == 1 {
            MergeKind::Insert
        } else {
            MergeKind::None
        };
        let (start, end) = self
            .selection_char_range()
            .unwrap_or((self.char_index(self.cursor), self.char_index(self.cursor)));
        self.apply_edit(start, end, &text, merge);
    }

    pub fn insert_char(&mut self, ch: char) {
        if ch == '\t' {
            self.insert_tab();
        } else if ch == '\n' || ch == '\r' {
            self.newline();
        } else if !ch.is_control() {
            self.insert_text(&ch.to_string());
        }
    }

    pub fn insert_tab(&mut self) {
        let col = self.cursor.col;
        let stops = self.tab_width.max(1);
        let width = stops - (col % stops);
        let text = if self.use_spaces {
            " ".repeat(width)
        } else {
            "\t".to_string()
        };
        self.insert_text(&text);
    }

    pub fn newline(&mut self) {
        let current_line = self.line_content(self.cursor.line);
        let leading: String = current_line
            .chars()
            .take_while(|ch| *ch == ' ' || *ch == '\t')
            .collect();
        let before_cursor: String = current_line.chars().take(self.cursor.col).collect();
        let opens_block = before_cursor
            .trim_end()
            .chars()
            .last()
            .is_some_and(|ch| matches!(ch, '{' | '[' | '(' | ':'));
        let mut indent = leading.clone();
        if opens_block && self.cursor.col > leading.chars().count() {
            if self.use_spaces {
                indent.push_str(&" ".repeat(self.tab_width));
            } else {
                indent.push('\t');
            }
        }
        let text = format!("\n{indent}");
        let (start, end) = self
            .selection_char_range()
            .unwrap_or((self.char_index(self.cursor), self.char_index(self.cursor)));
        self.apply_edit(start, end, &text, MergeKind::None);
    }

    pub fn backspace(&mut self) {
        if let Some((start, end)) = self.selection_char_range() {
            self.apply_edit(start, end, "", MergeKind::None);
            return;
        }
        let index = self.char_index(self.cursor);
        if index == 0 {
            return;
        }
        // Delete a whole grapheme-ish cluster: combining marks stay attached.
        let mut start = index - 1;
        while start > 0 {
            let ch = self.rope.char(start);
            if is_combining(ch) {
                start -= 1;
            } else {
                break;
            }
        }
        self.apply_edit(start, index, "", MergeKind::DeleteBack);
    }

    pub fn delete_forward(&mut self) {
        if let Some((start, end)) = self.selection_char_range() {
            self.apply_edit(start, end, "", MergeKind::None);
            return;
        }
        let index = self.char_index(self.cursor);
        if index >= self.rope.len_chars() {
            return;
        }
        let mut end = index + 1;
        while end < self.rope.len_chars() && is_combining(self.rope.char(end)) {
            end += 1;
        }
        self.apply_edit(index, end, "", MergeKind::DeleteForward);
    }

    /// Insert one indentation level at the start of each selected line (or the
    /// current line). A single undo group covers the whole operation.
    pub fn indent_selection(&mut self) {
        self.shift_selection(true);
    }

    pub fn outdent_selection(&mut self) {
        self.shift_selection(false);
    }

    fn shift_selection(&mut self, indent: bool) {
        let (first, last) = match self.selection_char_range() {
            Some((start, end)) => (self.rope.char_to_line(start), self.rope.char_to_line(end)),
            None => (self.cursor.line, self.cursor.line),
        };
        let width = if self.use_spaces { self.tab_width } else { 1 };
        let indent_text = if self.use_spaces {
            " ".repeat(self.tab_width)
        } else {
            "\t".to_string()
        };
        let undo_len = self.undo.len();
        let before = self.cursor;
        // Apply bottom-up so earlier line numbers stay valid.
        for line in (first..=last).rev() {
            let base = self.rope.line_to_char(line);
            if indent {
                self.apply_edit(base, base, &indent_text, MergeKind::None);
            } else {
                let content = self.line_content(line);
                let mut remove = 0usize;
                for ch in content.chars().take(width) {
                    if ch == ' ' || ch == '\t' {
                        remove += 1;
                    } else {
                        break;
                    }
                }
                if remove > 0 {
                    self.apply_edit(base, base + remove, "", MergeKind::None);
                }
            }
        }
        self.finish_multi_edit(undo_len, before);
    }

    fn finish_multi_edit(&mut self, undo_len: usize, before: Cursor) {
        if self.undo.len() <= undo_len {
            return;
        }
        let groups = self.undo.split_off(undo_len);
        let mut edits: Vec<Edit> = groups.into_iter().flat_map(|group| group.edits).collect();
        edits.sort_by_key(|edit| std::cmp::Reverse(edit.start));
        let after = self.cursor;
        self.undo.push(EditGroup {
            edits,
            before,
            after,
        });
    }

    pub fn toggle_comment(&mut self) {
        let (prefix, suffix) = comment_tokens(&self.path);
        let (first, last) = match self.selection_char_range() {
            Some((start, end)) => (self.rope.char_to_line(start), self.rope.char_to_line(end)),
            None => (self.cursor.line, self.cursor.line),
        };
        let all_commented = (first..=last).all(|line| {
            let content = self.line_content(line);
            let trimmed = content.trim_start();
            trimmed.starts_with(prefix) && (suffix.is_empty() || trimmed.ends_with(suffix))
        });
        let undo_len = self.undo.len();
        let before = self.cursor;
        for line in (first..=last).rev() {
            let content = self.line_content(line);
            let indent_chars =
                content.chars().count() - content.trim_start_matches([' ', '\t']).chars().count();
            let base = self.rope.line_to_char(line);
            if all_commented {
                let trimmed = content.trim_start_matches([' ', '\t']);
                let inner = trimmed.strip_prefix(prefix).unwrap_or(trimmed);
                let inner = if suffix.is_empty() {
                    inner
                } else {
                    inner.strip_suffix(suffix).unwrap_or(inner)
                };
                let start = base + indent_chars;
                let end = base + content.chars().count();
                self.apply_edit(start, end, inner.trim(), MergeKind::None);
            } else if !content.trim().is_empty() {
                let start = base + indent_chars;
                let end = base + content.chars().count();
                if suffix.is_empty() {
                    self.apply_edit(start, start, &format!("{prefix} "), MergeKind::None);
                } else {
                    self.apply_edit(end, end, &format!(" {suffix}"), MergeKind::None);
                    self.apply_edit(start, start, &format!("{prefix} "), MergeKind::None);
                }
            }
        }
        self.finish_multi_edit(undo_len, before);
    }

    fn apply_edit(&mut self, start: usize, end: usize, text: &str, merge: MergeKind) {
        let start = start.min(self.rope.len_chars());
        let end = end.min(self.rope.len_chars()).max(start);
        let removed = self.rope.slice(start..end).to_string();
        if removed.is_empty() && text.is_empty() {
            return;
        }
        let before = self.cursor;
        if end > start {
            self.rope.remove(start..end);
        }
        if !text.is_empty() {
            self.rope.insert(start, text);
        }
        let after = self.cursor_at(start + text.chars().count());
        self.cursor = after;
        self.selection = None;
        self.dirty = true;
        self.revision = self.revision.wrapping_add(1);
        let line = self.rope.char_to_line(start.min(self.rope.len_chars()));
        self.highlighter.invalidate_from(line);
        self.redo.clear();

        let now = Instant::now();
        let merged = self.try_merge_edit(start, &removed, text, merge, now);
        if !merged {
            self.undo.push(EditGroup {
                edits: vec![Edit {
                    start,
                    removed,
                    inserted: text.to_string(),
                }],
                before,
                after,
            });
            if self.undo.len() > 5_000 {
                self.undo.remove(0);
            }
        }
        self.merge_kind = merge;
        self.last_edit_at = Some(now);
    }

    fn try_merge_edit(
        &mut self,
        start: usize,
        removed: &str,
        inserted: &str,
        merge: MergeKind,
        now: Instant,
    ) -> bool {
        if merge == MergeKind::None {
            return false;
        }
        let recent = self
            .last_edit_at
            .is_some_and(|at| now.duration_since(at) < Duration::from_millis(700));
        if !recent || self.merge_kind != merge {
            return false;
        }
        let Some(group) = self.undo.last_mut() else {
            return false;
        };
        let Some(edit) = group.edits.last_mut() else {
            return false;
        };
        let inserted_len = edit.inserted.chars().count();
        match merge {
            MergeKind::Insert => {
                if start == edit.start + inserted_len {
                    edit.inserted.push_str(inserted);
                    group.after = self.cursor;
                    return true;
                }
            }
            MergeKind::DeleteBack => {
                if !removed.is_empty() && start + removed.chars().count() == edit.start {
                    edit.start = start;
                    let mut next = removed.to_string();
                    next.push_str(&edit.removed);
                    edit.removed = next;
                    group.after = self.cursor;
                    return true;
                }
            }
            MergeKind::DeleteForward => {
                if !removed.is_empty() && start == edit.start {
                    edit.removed.push_str(removed);
                    group.after = self.cursor;
                    return true;
                }
            }
            MergeKind::None => {}
        }
        false
    }

    pub fn undo(&mut self) -> bool {
        let Some(group) = self.undo.pop() else {
            return false;
        };
        for edit in group.edits.iter().rev() {
            let inserted_len = edit.inserted.chars().count();
            if inserted_len > 0 {
                self.rope.remove(edit.start..edit.start + inserted_len);
            }
            if !edit.removed.is_empty() {
                self.rope.insert(edit.start, &edit.removed);
            }
        }
        self.cursor = group.before;
        self.selection = None;
        self.highlighter
            .invalidate_from(group.before.line.saturating_sub(1));
        self.revision = self.revision.wrapping_add(1);
        self.dirty = true;
        self.merge_kind = MergeKind::None;
        self.redo.push(group);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(group) = self.redo.pop() else {
            return false;
        };
        for edit in &group.edits {
            let inserted_len = edit.inserted.chars().count();
            if inserted_len > 0 {
                self.rope.insert(edit.start, &edit.inserted);
            }
            let removed_len = edit.removed.chars().count();
            if removed_len > 0 {
                self.rope
                    .remove(edit.start + inserted_len..edit.start + inserted_len + removed_len);
            }
        }
        self.cursor = group.after;
        self.selection = None;
        self.highlighter
            .invalidate_from(group.after.line.saturating_sub(1));
        self.revision = self.revision.wrapping_add(1);
        self.dirty = true;
        self.merge_kind = MergeKind::None;
        self.undo.push(group);
        true
    }

    pub fn ensure_highlight(&mut self, first: usize, last: usize) {
        self.highlighter.ensure(&self.rope, first, last);
    }

    /// Re-read the file from disk after an external change. Undo history is
    /// discarded because the old rope no longer matches the file.
    pub fn reload(&mut self) -> anyhow::Result<()> {
        let file = lazecode_fs::file::read_text_file(&self.path)?;
        // The watcher reports our own atomic save; identical bytes keep the rope
        // and undo history intact.
        if self.rope.chars().eq(file.text.chars()) {
            self.encoding = file.encoding;
            self.eol = file.eol;
            self.had_bom = file.had_bom;
            self.lossy = file.lossy;
            self.read_only = file_is_read_only(&self.path);
            return Ok(());
        }
        self.rope = Rope::from_str(&file.text);
        self.encoding = file.encoding;
        self.eol = file.eol;
        self.had_bom = file.had_bom;
        self.lossy = file.lossy;
        self.read_only = file_is_read_only(&self.path);
        let line = self
            .cursor
            .line
            .min(self.rope.len_lines().saturating_sub(1));
        let col = self.cursor.col.min(self.line_content_len(line));
        self.cursor = Cursor { line, col };
        self.selection = None;
        self.scroll_line = self
            .scroll_line
            .min(self.rope.len_lines().saturating_sub(1) as f32);
        self.undo.clear();
        self.redo.clear();
        self.dirty = false;
        self.revision = self.revision.wrapping_add(1);
        self.highlighter.reset();
        Ok(())
    }

    pub fn save(&mut self) -> anyhow::Result<()> {
        if self.read_only {
            anyhow::bail!("document is read-only");
        }
        if self.lossy {
            anyhow::bail!("file was decoded with replacement characters; saving would corrupt it");
        }
        let text = self.text();
        lazecode_fs::file::write_text_file(
            &self.path,
            &text,
            self.encoding,
            self.eol,
            self.had_bom,
        )?;
        self.dirty = false;
        Ok(())
    }

    /// Character ranges (start, end) of every match in the document.
    pub fn find_all(&self, query: &str, options: MatchOptions) -> Vec<(Cursor, Cursor)> {
        if query.is_empty() {
            return Vec::new();
        }
        let pattern = if options.regex {
            query.to_string()
        } else {
            regex::escape(query)
        };
        let pattern = if options.whole_word && !options.regex {
            format!(r"\b{pattern}\b")
        } else {
            pattern
        };
        let Ok(regex) = regex::RegexBuilder::new(&pattern)
            .case_insensitive(!options.case_sensitive)
            .size_limit(4 * 1024 * 1024)
            .build()
        else {
            return Vec::new();
        };
        let mut matches = Vec::new();
        for line in 0..self.rope.len_lines() {
            let text = self.line_content(line);
            for found in regex.find_iter(&text) {
                let start = text[..found.start()].chars().count();
                let end = start + text[found.start()..found.end()].chars().count();
                matches.push((Cursor { line, col: start }, Cursor { line, col: end }));
                if matches.len() >= 10_000 {
                    return matches;
                }
            }
        }
        matches
    }

    pub fn replace_match(&mut self, start: Cursor, end: Cursor, replacement: &str) {
        let s = self.char_index(start);
        let e = self.char_index(end);
        self.apply_edit(s, e, replacement, MergeKind::None);
        self.cursor = self.cursor_at(s + replacement.chars().count());
        self.selection = None;
    }

    pub fn replace_all(&mut self, query: &str, replacement: &str, options: MatchOptions) -> usize {
        let matches = self.find_all(query, options);
        let undo_len = self.undo.len();
        let before = self.cursor;
        for (start, end) in matches.iter().rev() {
            let s = self.char_index(*start);
            let e = self.char_index(*end);
            self.apply_edit(s, e, replacement, MergeKind::None);
        }
        self.finish_multi_edit(undo_len, before);
        matches.len()
    }

    pub fn cursor_line_text(&self) -> String {
        self.line_content(self.cursor.line)
    }

    /// Byte-free visual column: tabs expand to the configured width.
    pub fn visual_col(line: &str, char_col: usize, tab_width: usize) -> usize {
        let mut visual = 0usize;
        for ch in line.chars().take(char_col) {
            if ch == '\t' {
                let width = tab_width.max(1);
                visual += width - (visual % width);
            } else {
                visual += unicode_width::UnicodeWidthChar::width(ch)
                    .unwrap_or(1)
                    .max(1);
            }
        }
        visual
    }
}

fn file_is_read_only(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|metadata| metadata.permissions().readonly())
        .unwrap_or(false)
}

fn normalise_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn is_combining(ch: char) -> bool {
    matches!(ch as u32, 0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Document {
        let mut doc = Document::empty(PathBuf::from("test.rs"), 4, true);
        doc.rope = Rope::from_str(text);
        doc
    }

    #[test]
    fn inserts_and_undoes_typing_in_one_group() {
        let mut doc = doc("abc\n");
        doc.set_cursor(Cursor { line: 0, col: 3 }, false);
        doc.insert_text("d");
        doc.insert_text("e");
        assert_eq!(doc.line_content(0), "abcde");
        assert!(doc.undo());
        assert_eq!(doc.line_content(0), "abc");
        assert!(doc.redo());
        assert_eq!(doc.line_content(0), "abcde");
    }

    #[test]
    fn backspace_joins_lines() {
        let mut doc = doc("a\nb\n");
        doc.set_cursor(Cursor { line: 1, col: 0 }, false);
        doc.backspace();
        assert_eq!(doc.line_content(0), "ab");
    }

    #[test]
    fn selection_move_and_replace() {
        let mut doc = doc("hello\n");
        doc.set_cursor(Cursor { line: 0, col: 0 }, false);
        doc.move_cursor(Motion::Right, true);
        doc.move_cursor(Motion::Right, true);
        assert_eq!(doc.selected_text().as_deref(), Some("he"));
        doc.insert_text("HE");
        assert_eq!(doc.line_content(0), "HEllo");
    }

    #[test]
    fn find_all_returns_ranges() {
        let doc = doc("foo bar foo\n");
        let matches = doc.find_all("foo", MatchOptions::default());
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[1].0, Cursor { line: 0, col: 8 });
    }

    #[test]
    fn lsp_positions_use_utf16_columns() {
        let doc = doc("a😀b\nsecond");
        assert_eq!(doc.lsp_position(Cursor { line: 0, col: 2 }), (0, 3));
        assert_eq!(doc.lsp_cursor(0, 3), Cursor { line: 0, col: 2 });
        assert_eq!(doc.lsp_cursor(0, 4), Cursor { line: 0, col: 3 });
        // Clamps out-of-range lines/columns instead of panicking.
        assert_eq!(doc.lsp_cursor(99, 99), Cursor { line: 1, col: 6 });
    }

    #[test]
    fn applies_lsp_edits_as_one_undo_group() {
        let mut doc = doc("one\ntwo\nthree\n");
        let edits = [
            crate::LspTextEdit::new(0, 0, 0, 3, "ONE"),
            crate::LspTextEdit::new(2, 0, 2, 5, "THREE"),
        ];
        assert!(doc.apply_text_edits(&edits));
        assert_eq!(doc.text(), "ONE\ntwo\nTHREE\n");
        assert!(doc.undo());
        assert_eq!(doc.text(), "one\ntwo\nthree\n");
    }

    #[test]
    fn rejects_overlapping_lsp_edits_without_mutating() {
        let mut doc = doc("abcdef");
        let edits = [
            crate::LspTextEdit::new(0, 0, 0, 3, "x"),
            crate::LspTextEdit::new(0, 2, 0, 5, "y"),
        ];
        assert!(!doc.apply_text_edits(&edits));
        assert_eq!(doc.text(), "abcdef");
        assert!(!doc.dirty);
    }

    #[test]
    fn reload_replaces_buffer_and_clears_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reload.txt");
        std::fs::write(&path, "old\n").unwrap();
        let mut doc = Document::open(&path, 4, true).unwrap();
        doc.set_cursor(Cursor { line: 0, col: 1 }, false);
        doc.insert_text("X");
        std::fs::write(&path, "new one\nnew two\n").unwrap();
        doc.reload().unwrap();
        assert_eq!(doc.line_content(0), "new one");
        assert_eq!(doc.line_count(), 3);
        assert!(!doc.dirty);
        assert!(!doc.undo());
    }

    #[test]
    fn save_writes_through_encoding_and_eol() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("save.txt");
        std::fs::write(&path, "a\r\nb\r\n").unwrap();
        let mut doc = Document::open(&path, 4, true).unwrap();
        doc.set_cursor(Cursor { line: 0, col: 1 }, false);
        doc.insert_text("X");
        assert!(doc.dirty);
        doc.save().unwrap();
        assert!(!doc.dirty);
        assert_eq!(std::fs::read(&path).unwrap(), b"aX\r\nb\r\n");
    }

    #[test]
    fn auto_indent_after_brace() {
        let mut doc = doc("fn main() {\n");
        doc.set_cursor(Cursor { line: 0, col: 11 }, false);
        doc.newline();
        assert_eq!(doc.line_count(), 3);
        assert_eq!(doc.line_content(1), "    ");
    }
}

#[cfg(test)]
mod edit_tests {
    use super::*;

    fn doc(text: &str) -> Document {
        let mut doc = Document::empty(PathBuf::from("test.rs"), 4, true);
        doc.rope = Rope::from_str(text);
        doc
    }

    #[test]
    fn cut_deletes_entire_selection_and_undoes_once() {
        let mut doc = doc("hello world\n");
        doc.selection = Some(Selection {
            anchor: Cursor { line: 0, col: 6 },
            head: Cursor { line: 0, col: 11 },
        });
        doc.cursor = Cursor { line: 0, col: 11 };
        assert!(doc.delete_selection());
        assert_eq!(doc.line_content(0), "hello ");
        assert!(doc.undo());
        assert_eq!(doc.line_content(0), "hello world");
    }

    #[test]
    fn multi_line_indent_is_one_undo_group() {
        let mut doc = doc("a\nb\nc\n");
        doc.selection = Some(Selection {
            anchor: Cursor { line: 0, col: 0 },
            head: Cursor { line: 2, col: 1 },
        });
        doc.cursor = Cursor { line: 2, col: 1 };
        doc.indent_selection();
        assert_eq!(doc.line_content(0), "    a");
        assert_eq!(doc.line_content(1), "    b");
        assert_eq!(doc.line_content(2), "    c");
        assert!(doc.undo());
        assert_eq!(doc.line_content(0), "a");
        assert_eq!(doc.line_content(1), "b");
        assert_eq!(doc.line_content(2), "c");
    }

    #[test]
    fn replace_all_is_one_undo_group() {
        let mut doc = doc("foo foo foo\n");
        assert_eq!(doc.replace_all("foo", "bar", MatchOptions::default()), 3);
        assert_eq!(doc.line_content(0), "bar bar bar");
        assert!(doc.undo());
        assert_eq!(doc.line_content(0), "foo foo foo");
    }
}

#[cfg(test)]
mod comment_tests {
    use super::*;

    fn doc_at(path: &str, text: &str) -> Document {
        let mut doc = Document::empty(PathBuf::from(path), 4, true);
        doc.rope = Rope::from_str(text);
        doc
    }

    #[test]
    fn toggles_block_comments_around_suffix() {
        let mut doc = doc_at("page.html", "hello\n");
        doc.toggle_comment();
        assert_eq!(doc.line_content(0), "<!-- hello -->");
        doc.toggle_comment();
        assert_eq!(doc.line_content(0), "hello");
    }

    #[test]
    fn treats_dotfiles_as_hash_comments() {
        let mut doc = doc_at(".gitignore", "target/\n");
        doc.toggle_comment();
        assert_eq!(doc.line_content(0), "# target/");
        doc.toggle_comment();
        assert_eq!(doc.line_content(0), "target/");
    }
}

#[cfg(test)]
mod reload_tests {
    use super::*;

    #[test]
    fn reload_with_identical_content_keeps_undo_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("same.txt");
        std::fs::write(&path, "alpha\n").unwrap();
        let mut doc = Document::open(&path, 4, true).unwrap();
        doc.set_cursor(Cursor { line: 0, col: 5 }, false);
        doc.insert_text(" beta");
        doc.save().unwrap();
        // The watcher sees the atomic save and calls reload; identical content
        // must not discard history.
        doc.reload().unwrap();
        assert!(doc.undo());
        assert_eq!(doc.line_content(0), "alpha");
    }
}
