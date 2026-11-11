//! LSP text primitives shared by the editor and the language-server client.
//!
//! The language server protocol addresses text with zero-based line numbers
//! and UTF-16 code-unit columns. The editor stores Unicode scalar columns, so
//! every request/response has to cross this boundary exactly once. Keeping the
//! conversion here (instead of in the UI) makes it testable without egui and
//! keeps both directions symmetric.

/// One LSP text edit. Lines are zero-based; columns are UTF-16 code units.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LspTextEdit {
    pub start_line: u32,
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
    pub new_text: String,
}

impl LspTextEdit {
    pub fn new(
        start_line: u32,
        start_character: u32,
        end_line: u32,
        end_character: u32,
        new_text: impl Into<String>,
    ) -> Self {
        Self {
            start_line,
            start_character,
            end_line,
            end_character,
            new_text: new_text.into(),
        }
    }
}

/// Byte offset at which every line starts. A trailing newline produces a final
/// empty line, matching `ropey`'s `len_lines` view.
pub fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = Vec::with_capacity(64);
    starts.push(0);
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(index + 1);
        }
    }
    starts
}

/// Unicode scalar column for a UTF-16 code-unit column inside one line.
///
/// A column that points inside a surrogate pair clamps to the character that
/// starts the pair; servers must not send such positions, but clamping keeps a
/// malformed edit from panicking or splitting a character.
pub fn char_col_for_utf16(line: &str, utf16: u32) -> usize {
    let mut units = 0u32;
    for (index, ch) in line.chars().enumerate() {
        if units >= utf16 {
            return index;
        }
        let next = units + ch.len_utf16() as u32;
        if next > utf16 {
            return index;
        }
        units = next;
    }
    line.chars().count()
}

/// UTF-16 code-unit column for a Unicode scalar column inside one line.
pub fn utf16_col_for_char(line: &str, char_col: usize) -> u32 {
    line.chars()
        .take(char_col)
        .map(|ch| ch.len_utf16() as u32)
        .sum()
}

fn char_offset_for_position(
    text: &str,
    starts: &[usize],
    line: u32,
    character: u32,
) -> Result<usize, String> {
    let line = line as usize;
    if line >= starts.len() {
        return Err(format!("text edit line {line} is out of range"));
    }
    let start = starts[line];
    let end = starts.get(line + 1).copied().unwrap_or(text.len());
    let content = &text[start..end];
    let char_col = char_col_for_utf16(content, character);
    Ok(start
        + content
            .chars()
            .take(char_col)
            .map(char::len_utf8)
            .sum::<usize>())
}

/// Apply LSP edits to a plain string, used for files that are not open in an
/// editor buffer (workspace renames). Edits are validated and applied in
/// reverse document order so earlier offsets stay valid.
pub fn apply_text_edits(text: &str, edits: &[LspTextEdit]) -> Result<String, String> {
    if edits.is_empty() {
        return Ok(text.to_string());
    }
    let starts = line_starts(text);
    let mut ranges: Vec<(usize, usize, &str)> = Vec::with_capacity(edits.len());
    for edit in edits {
        if (edit.start_line, edit.start_character) > (edit.end_line, edit.end_character) {
            return Err("text edit has an inverted range".to_string());
        }
        let start = char_offset_for_position(text, &starts, edit.start_line, edit.start_character)?;
        let end = char_offset_for_position(text, &starts, edit.end_line, edit.end_character)?;
        if end < start {
            return Err("text edit range is inverted after conversion".to_string());
        }
        ranges.push((start, end, edit.new_text.as_str()));
    }
    ranges.sort_by_key(|(start, end, _)| (*start, *end));
    for pair in ranges.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err("text edits overlap".to_string());
        }
    }
    let mut output = text.to_string();
    for (start, end, new_text) in ranges.into_iter().rev() {
        output.replace_range(start..end, new_text);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_columns_round_trip_with_surrogate_pairs() {
        let line = "a😀b";
        assert_eq!(utf16_col_for_char(line, 0), 0);
        assert_eq!(utf16_col_for_char(line, 1), 1);
        assert_eq!(utf16_col_for_char(line, 2), 3);
        assert_eq!(utf16_col_for_char(line, 3), 4);
        assert_eq!(char_col_for_utf16(line, 0), 0);
        assert_eq!(char_col_for_utf16(line, 1), 1);
        assert_eq!(char_col_for_utf16(line, 3), 2);
        assert_eq!(char_col_for_utf16(line, 4), 3);
        // Mid-pair positions clamp to the start of the character.
        assert_eq!(char_col_for_utf16(line, 2), 1);
    }

    #[test]
    fn applies_multiple_edits_in_reverse_order() {
        let text = "one\ntwo\nthree\n";
        let edits = [
            LspTextEdit::new(0, 0, 0, 3, "ONE"),
            LspTextEdit::new(2, 0, 2, 5, "THREE"),
        ];
        assert_eq!(apply_text_edits(text, &edits).unwrap(), "ONE\ntwo\nTHREE\n");
    }

    #[test]
    fn applies_insertions_at_end_of_file() {
        let text = "a\nb";
        let edits = [LspTextEdit::new(1, 1, 1, 1, "\nc")];
        assert_eq!(apply_text_edits(text, &edits).unwrap(), "a\nb\nc");
    }

    #[test]
    fn rejects_overlapping_edits() {
        let text = "abcdef";
        let edits = [
            LspTextEdit::new(0, 0, 0, 3, "x"),
            LspTextEdit::new(0, 2, 0, 5, "y"),
        ];
        assert!(apply_text_edits(text, &edits).is_err());
    }

    #[test]
    fn rejects_out_of_range_lines() {
        let edits = [LspTextEdit::new(9, 0, 9, 0, "x")];
        assert!(apply_text_edits("a\nb", &edits).is_err());
    }

    #[test]
    fn empty_edit_list_is_a_no_op() {
        assert_eq!(apply_text_edits("text", &[]).unwrap(), "text");
    }
}
