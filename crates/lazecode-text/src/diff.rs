use similar::{ChangeTag, DiffOp, TextDiff as SimilarDiff};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffLineKind {
    Context,
    Add,
    Delete,
}

#[derive(Clone, Debug)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct DiffHunk {
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Debug, Default)]
pub struct TextDiff {
    pub hunks: Vec<DiffHunk>,
    pub additions: u32,
    pub deletions: u32,
}

/// Line diff with 3 lines of context, shaped for the editor's diff view.
pub fn diff_text(old: &str, new: &str) -> TextDiff {
    diff_text_with_context(old, new, 3)
}

pub fn diff_text_with_context(old: &str, new: &str, context: usize) -> TextDiff {
    let old_lines: Vec<&str> = split_lines(old);
    let new_lines: Vec<&str> = split_lines(new);
    let diff = SimilarDiff::from_lines(old, new);
    let mut result = TextDiff::default();

    for group in diff.grouped_ops(context) {
        if group.is_empty() {
            continue;
        }
        let mut lines = Vec::new();
        let mut old_start: Option<u32> = None;
        let mut new_start: Option<u32> = None;
        let mut old_count = 0u32;
        let mut new_count = 0u32;

        let mut push =
            |kind: DiffLineKind, old_no: Option<u32>, new_no: Option<u32>, text: String| {
                match kind {
                    DiffLineKind::Add => result.additions += 1,
                    DiffLineKind::Delete => result.deletions += 1,
                    DiffLineKind::Context => {}
                }
                if old_start.is_none() && old_no.is_some() {
                    old_start = old_no;
                }
                if new_start.is_none() && new_no.is_some() {
                    new_start = new_no;
                }
                if old_no.is_some() {
                    old_count += 1;
                }
                if new_no.is_some() {
                    new_count += 1;
                }
                lines.push(DiffLine {
                    kind,
                    old_lineno: old_no,
                    new_lineno: new_no,
                    text,
                });
            };

        for op in group {
            match op {
                DiffOp::Equal {
                    old_index,
                    new_index,
                    len,
                } => {
                    for offset in 0..len {
                        push(
                            DiffLineKind::Context,
                            Some((old_index + offset + 1) as u32),
                            Some((new_index + offset + 1) as u32),
                            old_lines
                                .get(old_index + offset)
                                .copied()
                                .unwrap_or_default()
                                .to_string(),
                        );
                    }
                }
                DiffOp::Delete {
                    old_index,
                    old_len,
                    new_index,
                } => {
                    for offset in 0..old_len {
                        push(
                            DiffLineKind::Delete,
                            Some((old_index + offset + 1) as u32),
                            None,
                            old_lines
                                .get(old_index + offset)
                                .copied()
                                .unwrap_or_default()
                                .to_string(),
                        );
                    }
                    let _ = new_index;
                }
                DiffOp::Insert {
                    old_index,
                    new_index,
                    new_len,
                } => {
                    for offset in 0..new_len {
                        push(
                            DiffLineKind::Add,
                            None,
                            Some((new_index + offset + 1) as u32),
                            new_lines
                                .get(new_index + offset)
                                .copied()
                                .unwrap_or_default()
                                .to_string(),
                        );
                    }
                    let _ = old_index;
                }
                DiffOp::Replace {
                    old_index,
                    old_len,
                    new_index,
                    new_len,
                } => {
                    for offset in 0..old_len {
                        push(
                            DiffLineKind::Delete,
                            Some((old_index + offset + 1) as u32),
                            None,
                            old_lines
                                .get(old_index + offset)
                                .copied()
                                .unwrap_or_default()
                                .to_string(),
                        );
                    }
                    for offset in 0..new_len {
                        push(
                            DiffLineKind::Add,
                            None,
                            Some((new_index + offset + 1) as u32),
                            new_lines
                                .get(new_index + offset)
                                .copied()
                                .unwrap_or_default()
                                .to_string(),
                        );
                    }
                }
            }
        }

        if lines.is_empty() {
            continue;
        }
        let old_start = old_start.unwrap_or(0);
        let new_start = new_start.unwrap_or(0);
        result.hunks.push(DiffHunk {
            header: format!("@@ -{old_start},{old_count} +{new_start},{new_count} @@"),
            lines,
        });
    }
    result
}

fn split_lines(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines
}

/// Render a truthful unified diff for display in a text area.
pub fn unified_patch(path: &str, diff: &TextDiff) -> String {
    let mut out = format!("--- a/{path}\n+++ b/{path}\n");
    for hunk in &diff.hunks {
        out.push_str(&hunk.header);
        out.push('\n');
        for line in &hunk.lines {
            let sign = match line.kind {
                DiffLineKind::Context => ' ',
                DiffLineKind::Add => '+',
                DiffLineKind::Delete => '-',
            };
            out.push(sign);
            out.push_str(&line.text);
            out.push('\n');
        }
    }
    out
}

/// Count adds/deletes only, for the source-control list.
pub fn change_stats(old: &str, new: &str) -> (u32, u32) {
    let diff = SimilarDiff::from_lines(old, new);
    let mut additions = 0;
    let mut deletions = 0;
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => additions += 1,
            ChangeTag::Delete => deletions += 1,
            ChangeTag::Equal => {}
        }
    }
    (additions, deletions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_add_delete_hunks() {
        let old = "a\nb\nc\nd\ne\n";
        let new = "a\nB\nc\nd\ne\nf\n";
        let diff = diff_text(old, new);
        assert_eq!(diff.additions, 2);
        assert_eq!(diff.deletions, 1);
        assert_eq!(diff.hunks.len(), 1);
        assert!(diff.hunks[0]
            .lines
            .iter()
            .any(|l| l.kind == DiffLineKind::Add));
    }

    #[test]
    fn identical_text_has_no_hunks() {
        let diff = diff_text("same\n", "same\n");
        assert!(diff.hunks.is_empty());
        assert_eq!(diff.additions, 0);
        assert_eq!(diff.deletions, 0);
    }
}
