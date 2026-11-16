//! Unified-diff parsing/application for tracked text files (add, delete,
//! modify, mode changes). Binary and rename patches are rejected.

use anyhow::{anyhow, bail, Result};

#[derive(Clone, Debug)]
pub struct FilePatch {
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub old_mode: Option<u32>,
    pub new_mode: Option<u32>,
    pub is_new: bool,
    pub is_delete: bool,
    pub hunks: Vec<Hunk>,
}

#[derive(Clone, Debug)]
pub struct Hunk {
    old_start: usize,
    old_count: usize,
    new_start: usize,
    new_count: usize,
    lines: Vec<PatchLine>,
}

#[derive(Clone, Debug)]
enum PatchLine {
    Context(Vec<u8>),
    Add(Vec<u8>),
    Remove(Vec<u8>),
}

impl FilePatch {
    pub fn target_path(&self, reverse: bool) -> Option<&str> {
        if reverse {
            self.old_path.as_deref()
        } else {
            self.new_path.as_deref()
        }
    }

    pub fn source_path(&self, reverse: bool) -> Option<&str> {
        if reverse {
            self.new_path.as_deref()
        } else {
            self.old_path.as_deref()
        }
    }

    /// `(path, create, delete)` after applying `reverse`.
    pub fn operation(&self, reverse: bool) -> Result<(&str, bool, bool)> {
        let (create, delete) = if reverse {
            (self.is_delete, self.is_new)
        } else {
            (self.is_new, self.is_delete)
        };
        let path = if create {
            self.old_path
                .as_deref()
                .or(self.new_path.as_deref())
                .ok_or_else(|| anyhow!("patch has no path"))?
        } else if delete {
            self.old_path
                .as_deref()
                .or(self.new_path.as_deref())
                .ok_or_else(|| anyhow!("patch has no path"))?
        } else {
            self.new_path
                .as_deref()
                .or(self.old_path.as_deref())
                .ok_or_else(|| anyhow!("patch has no path"))?
        };
        Ok((path, create, delete))
    }

    pub fn mode(&self, reverse: bool) -> Option<u32> {
        if reverse {
            self.old_mode.or(self.new_mode)
        } else {
            self.new_mode.or(self.old_mode)
        }
    }
}

pub fn parse(patch: &str) -> Result<Vec<FilePatch>> {
    let mut files = Vec::new();
    let mut current: Option<FilePatch> = None;
    let mut lines = patch.split_inclusive('\n').peekable();

    fn flush(files: &mut Vec<FilePatch>, current: &mut Option<FilePatch>) -> Result<()> {
        if let Some(file) = current.take() {
            if file.old_path.is_none() && file.new_path.is_none() && file.hunks.is_empty() {
                return Ok(());
            }
            files.push(file);
        }
        Ok(())
    }

    while let Some(line) = lines.next() {
        let raw = line
            .strip_suffix('\n')
            .unwrap_or(line)
            .strip_suffix('\r')
            .unwrap_or(line);
        if let Some(rest) = raw.strip_prefix("diff --git ") {
            flush(&mut files, &mut current)?;
            current = Some(FilePatch {
                old_path: None,
                new_path: None,
                old_mode: None,
                new_mode: None,
                is_new: false,
                is_delete: false,
                hunks: Vec::new(),
            });
            // The ---/+++ headers carry the authoritative paths; without a
            // b-side path fail loudly instead of guessing from the diff header.
            let _ = rest;
            continue;
        }
        if current.is_none() {
            current = Some(FilePatch {
                old_path: None,
                new_path: None,
                old_mode: None,
                new_mode: None,
                is_new: false,
                is_delete: false,
                hunks: Vec::new(),
            });
        }
        let file = current.as_mut().expect("current file");
        if let Some(rest) = raw.strip_prefix("--- ") {
            file.old_path = parse_header_path(rest)?;
            continue;
        }
        if let Some(rest) = raw.strip_prefix("+++ ") {
            file.new_path = parse_header_path(rest)?;
            continue;
        }
        if let Some(rest) = raw.strip_prefix("new file mode ") {
            file.is_new = true;
            file.new_mode = parse_mode(rest)?;
            continue;
        }
        if let Some(rest) = raw.strip_prefix("deleted file mode ") {
            file.is_delete = true;
            file.old_mode = parse_mode(rest)?;
            continue;
        }
        if let Some(rest) = raw.strip_prefix("old mode ") {
            file.old_mode = parse_mode(rest)?;
            continue;
        }
        if let Some(rest) = raw.strip_prefix("new mode ") {
            file.new_mode = parse_mode(rest)?;
            continue;
        }
        if raw.starts_with("rename from ") || raw.starts_with("rename to ") {
            bail!("rename patches are not supported");
        }
        if raw.starts_with("GIT binary patch") || raw.starts_with("Binary files ") {
            bail!("binary patches are not supported");
        }
        if let Some(rest) = raw.strip_prefix("@@ ") {
            let mut hunk = parse_hunk_header(rest)?;
            consume_hunk_body(&mut lines, &mut hunk)?;
            file.hunks.push(hunk);
            continue;
        }
        // `index`, `similarity index`, `dissimilarity index` and blank lines
        // between sections carry no apply semantics.
    }
    flush(&mut files, &mut current)?;
    if files.is_empty() {
        bail!("patch contains no file changes");
    }
    Ok(files)
}

fn parse_mode(text: &str) -> Result<Option<u32>> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    u32::from_str_radix(text, 8)
        .map(Some)
        .map_err(|_| anyhow!("invalid file mode '{text}'"))
}

fn parse_header_path(rest: &str) -> Result<Option<String>> {
    let rest = rest.trim_end_matches('\n').trim_end_matches('\r');
    if rest == "/dev/null" {
        return Ok(None);
    }
    let path = if rest.starts_with('"') {
        unquote_c(rest)?
    } else {
        rest.split('\t').next().unwrap_or(rest).to_string()
    };
    let path = path
        .strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(&path)
        .to_string();
    if path.is_empty() {
        bail!("patch has an empty path");
    }
    Ok(Some(path))
}

fn unquote_c(text: &str) -> Result<String> {
    let mut chars = text[1..].chars().peekable();
    let mut out = String::new();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => return Ok(out),
            '\\' => {
                let Some(escaped) = chars.next() else {
                    bail!("unterminated quoted path in patch");
                };
                match escaped {
                    '\\' => out.push('\\'),
                    '"' => out.push('"'),
                    't' => out.push('\t'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    '0'..='7' => {
                        let mut value = escaped.to_digit(8).unwrap_or(0);
                        for _ in 0..2 {
                            if let Some(next) = chars.peek().copied().filter(|c| c.is_digit(8)) {
                                chars.next();
                                value = value * 8 + next.to_digit(8).unwrap_or(0);
                            }
                        }
                        if let Some(ch) = char::from_u32(value) {
                            out.push(ch);
                        }
                    }
                    other => out.push(other),
                }
            }
            other => out.push(other),
        }
    }
    bail!("unterminated quoted path in patch")
}

fn parse_hunk_header(rest: &str) -> Result<Hunk> {
    let rest = rest.split("@@").next().unwrap_or(rest).trim();
    let mut parts = rest.split_whitespace();
    let old = parts
        .next()
        .and_then(|part| part.strip_prefix('-'))
        .ok_or_else(|| anyhow!("malformed hunk header"))?;
    let new = parts
        .next()
        .and_then(|part| part.strip_prefix('+'))
        .ok_or_else(|| anyhow!("malformed hunk header"))?;
    let (old_start, old_count) = parse_range(old)?;
    let (new_start, new_count) = parse_range(new)?;
    Ok(Hunk {
        old_start,
        old_count,
        new_start,
        new_count,
        lines: Vec::new(),
    })
}

fn parse_range(text: &str) -> Result<(usize, usize)> {
    let (start, count) = match text.split_once(',') {
        Some((start, count)) => (start, count),
        None => (text, "1"),
    };
    let start = start
        .parse::<usize>()
        .map_err(|_| anyhow!("invalid hunk range"))?;
    let count = count
        .parse::<usize>()
        .map_err(|_| anyhow!("invalid hunk range"))?;
    Ok((start, count))
}

fn consume_hunk_body<'a, I>(lines: &mut std::iter::Peekable<I>, hunk: &mut Hunk) -> Result<()>
where
    I: Iterator<Item = &'a str>,
{
    let mut old_seen = 0usize;
    let mut new_seen = 0usize;
    while old_seen < hunk.old_count || new_seen < hunk.new_count {
        let Some(line) = lines.next() else {
            bail!("patch ended inside a hunk");
        };
        if line.starts_with('\\') {
            strip_last_newline(hunk);
            continue;
        }
        let (prefix, data) = match line.as_bytes().first() {
            Some(prefix) => (*prefix, line.as_bytes()[1..].to_vec()),
            None => (b' ', Vec::new()),
        };
        match prefix {
            b' ' => {
                old_seen += 1;
                new_seen += 1;
                hunk.lines.push(PatchLine::Context(data));
            }
            b'-' => {
                old_seen += 1;
                hunk.lines.push(PatchLine::Remove(data));
            }
            b'+' => {
                new_seen += 1;
                hunk.lines.push(PatchLine::Add(data));
            }
            b'\\' => strip_last_newline(hunk),
            other => bail!("unexpected hunk line prefix '{}'", other as char),
        }
    }
    while matches!(lines.peek(), Some(line) if line.starts_with('\\')) {
        let _ = lines.next();
        strip_last_newline(hunk);
    }
    Ok(())
}

fn strip_last_newline(hunk: &mut Hunk) {
    if let Some(line) = hunk.lines.last_mut() {
        let data = match line {
            PatchLine::Context(data) | PatchLine::Add(data) | PatchLine::Remove(data) => data,
        };
        if data.ends_with(b"\r\n") {
            data.truncate(data.len() - 2);
        } else if data.ends_with(b"\n") {
            data.truncate(data.len() - 1);
        }
    }
}

/// Apply one file patch to `input` (canonical worktree/index bytes).
pub fn apply(input: &[u8], file: &FilePatch, reverse: bool) -> Result<Vec<u8>> {
    let lines: Vec<&[u8]> = input.split_inclusive(|byte| *byte == b'\n').collect();
    let mut output: Vec<Vec<u8>> = Vec::with_capacity(lines.len());
    let mut cursor = 0usize;
    for hunk in &file.hunks {
        let (old_seq, new_seq, start) = hunk_sequences(hunk, reverse);
        let expected = start.saturating_sub(1);
        let mut position = expected.max(cursor);
        if !matches_at(&lines, position, &old_seq) {
            position = (cursor..=lines.len())
                .find(|candidate| matches_at(&lines, *candidate, &old_seq))
                .ok_or_else(|| {
                    anyhow!(
                        "patch does not apply: context not found near line {}",
                        expected + 1
                    )
                })?;
        }
        output.extend(lines[cursor..position].iter().map(|line| line.to_vec()));
        output.extend(new_seq.iter().map(|line| line.to_vec()));
        cursor = position + old_seq.len();
    }
    output.extend(lines[cursor..].iter().map(|line| line.to_vec()));
    Ok(output.concat())
}

fn hunk_sequences(hunk: &Hunk, reverse: bool) -> (Vec<Vec<u8>>, Vec<Vec<u8>>, usize) {
    let mut old_seq = Vec::new();
    let mut new_seq = Vec::new();
    for line in &hunk.lines {
        match line {
            PatchLine::Context(data) => {
                old_seq.push(data.clone());
                new_seq.push(data.clone());
            }
            PatchLine::Add(data) => {
                if reverse {
                    old_seq.push(data.clone());
                } else {
                    new_seq.push(data.clone());
                }
            }
            PatchLine::Remove(data) => {
                if reverse {
                    new_seq.push(data.clone());
                } else {
                    old_seq.push(data.clone());
                }
            }
        }
    }
    let start = if reverse {
        hunk.new_start
    } else {
        hunk.old_start
    };
    (old_seq, new_seq, start)
}

fn matches_at(lines: &[&[u8]], at: usize, expected: &[Vec<u8>]) -> bool {
    if at > lines.len() || expected.len() > lines.len().saturating_sub(at) {
        return false;
    }
    expected
        .iter()
        .enumerate()
        .all(|(index, line)| lines[at + index] == line.as_slice())
}
