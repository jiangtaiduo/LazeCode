use std::str::FromStr;
use std::sync::OnceLock;

use ropey::Rope;
use syntect::highlighting::{
    Color, HighlightState, Highlighter, RangedHighlightIterator, ScopeSelectors, StyleModifier,
    Theme, ThemeItem, ThemeSettings,
};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

/// Coarse token classes. The UI maps these to the design palette, so syntax
/// colours stay consistent between themes without shipping tmTheme files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Text,
    Keyword,
    Type,
    String,
    Number,
    Function,
    Comment,
    Punctuation,
    Literal,
    Variable,
    Property,
    Attribute,
}

#[derive(Clone, Copy, Debug)]
pub struct HighlightSpan {
    /// Character offsets within the line (newline excluded).
    pub start: usize,
    pub end: usize,
    pub kind: TokenKind,
}

struct CachedLine {
    /// Parser + highlighter state *after* this line.
    parse: ParseState,
    highlight: HighlightState,
    spans: Vec<HighlightSpan>,
}

/// Per-document incremental highlighter.
///
/// The cache keeps the parse state at every line boundary, so scrolling to a
/// distant line only re-parses from the last cached line, never from line 0.
pub struct SyntaxHighlighter {
    syntax: Option<&'static SyntaxReference>,
    cache: Vec<Option<CachedLine>>,
    enabled: bool,
}

impl SyntaxHighlighter {
    pub fn new(path: &std::path::Path) -> Self {
        let syntax_set = syntax_set();
        let syntax = syntax_set
            .find_syntax_for_file(path)
            .ok()
            .flatten()
            .or_else(|| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .and_then(|ext| syntax_set.find_syntax_by_extension(ext))
            });
        Self {
            syntax,
            cache: Vec::new(),
            enabled: syntax.is_some(),
        }
    }

    pub fn language_name(&self) -> Option<&str> {
        self.syntax.map(|s| s.name.as_str())
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Drop every cached line at or after `line`.
    pub fn invalidate_from(&mut self, line: usize) {
        if line < self.cache.len() {
            self.cache.truncate(line);
        }
    }

    /// Reset the cache when the document changes shape underneath (reload).
    pub fn reset(&mut self) {
        self.cache.clear();
    }

    /// Make sure highlighting is available for lines `first..=last`.
    pub fn ensure(&mut self, rope: &Rope, first: usize, last: usize) {
        let Some(syntax) = self.syntax else {
            return;
        };
        if !self.enabled {
            return;
        }
        let line_count = rope.len_lines();
        if line_count == 0 {
            return;
        }
        let last = last.min(line_count - 1);
        let first = first.min(last);
        if self.cache.len() < line_count {
            self.cache.resize_with(line_count, || None);
        }
        if self.cache.len() > line_count {
            self.cache.truncate(line_count);
        }
        // Find the last line before `first` whose post-state is cached.
        let mut start_line = first;
        while start_line > 0 && self.cache[start_line - 1].is_none() {
            start_line -= 1;
        }
        let (mut parse, mut highlight) = if start_line == 0 {
            let highlighter = Highlighter::new(theme());
            (
                ParseState::new(syntax),
                HighlightState::new(&highlighter, ScopeStack::new()),
            )
        } else {
            let cached = self.cache[start_line - 1].as_ref().unwrap();
            (cached.parse.clone(), cached.highlight.clone())
        };
        let highlighter = Highlighter::new(theme());
        for line_idx in start_line..=last {
            let line = rope.line(line_idx).to_string();
            // Keep the terminator so multi-line constructs (comments, strings)
            // close correctly; it is excluded from the emitted spans.
            let spans = match parse.parse_line(&line, syntax_set()) {
                Ok(ops) => {
                    let mut iter =
                        RangedHighlightIterator::new(&mut highlight, &ops, &line, &highlighter);
                    let mut spans = Vec::new();
                    let content_chars = line.chars().count()
                        - usize::from(line.ends_with('\n'))
                        - usize::from(line.ends_with("\r\n"));
                    let mut char_pos = 0usize;
                    for (style, text, _range) in iter.by_ref() {
                        let count = text.chars().count();
                        if count == 0 {
                            continue;
                        }
                        let start = char_pos;
                        let end = (char_pos + count).min(content_chars);
                        if start < end {
                            spans.push(HighlightSpan {
                                start,
                                end,
                                kind: kind_for_color(style.foreground),
                            });
                        }
                        char_pos += count;
                    }
                    spans
                }
                Err(_) => Vec::new(),
            };
            self.cache[line_idx] = Some(CachedLine {
                parse: parse.clone(),
                highlight: highlight.clone(),
                spans,
            });
        }
    }

    pub fn spans(&self, line: usize) -> &[HighlightSpan] {
        self.cache
            .get(line)
            .and_then(|entry| entry.as_ref())
            .map(|entry| entry.spans.as_slice())
            .unwrap_or(&[])
    }
}

fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(build_theme)
}

const C_TEXT: Color = Color {
    r: 1,
    g: 1,
    b: 1,
    a: 255,
};
const C_KEYWORD: Color = Color {
    r: 2,
    g: 2,
    b: 2,
    a: 255,
};
const C_TYPE: Color = Color {
    r: 3,
    g: 3,
    b: 3,
    a: 255,
};
const C_STRING: Color = Color {
    r: 4,
    g: 4,
    b: 4,
    a: 255,
};
const C_NUMBER: Color = Color {
    r: 5,
    g: 5,
    b: 5,
    a: 255,
};
const C_FUNCTION: Color = Color {
    r: 6,
    g: 6,
    b: 6,
    a: 255,
};
const C_COMMENT: Color = Color {
    r: 7,
    g: 7,
    b: 7,
    a: 255,
};
const C_PUNCT: Color = Color {
    r: 8,
    g: 8,
    b: 8,
    a: 255,
};
const C_LITERAL: Color = Color {
    r: 9,
    g: 9,
    b: 9,
    a: 255,
};
const C_VARIABLE: Color = Color {
    r: 10,
    g: 10,
    b: 10,
    a: 255,
};
const C_PROPERTY: Color = Color {
    r: 11,
    g: 11,
    b: 11,
    a: 255,
};
const C_ATTRIBUTE: Color = Color {
    r: 12,
    g: 12,
    b: 12,
    a: 255,
};

fn build_theme() -> Theme {
    let mut scopes = Vec::new();
    let mut push = |selector: &str, color: Color| {
        if let Ok(scope) = ScopeSelectors::from_str(selector) {
            scopes.push(ThemeItem {
                scope,
                style: StyleModifier {
                    foreground: Some(color),
                    background: None,
                    font_style: None,
                },
            });
        }
    };
    // Most specific first; syntect scores specificity, but keeping a stable
    // order also makes the mapping deterministic.
    push("comment", C_COMMENT);
    push("string", C_STRING);
    push("constant.character", C_STRING);
    push("constant.numeric", C_NUMBER);
    push("constant.language", C_LITERAL);
    push("constant", C_LITERAL);
    push("keyword.operator", C_PUNCT);
    push("keyword", C_KEYWORD);
    push("storage.type", C_KEYWORD);
    push("storage", C_KEYWORD);
    push("entity.name.type", C_TYPE);
    push("entity.name.class", C_TYPE);
    push("entity.name.struct", C_TYPE);
    push("entity.name.enum", C_TYPE);
    push("entity.name.trait", C_TYPE);
    push("entity.name.interface", C_TYPE);
    push("support.type", C_TYPE);
    push("support.class", C_TYPE);
    push("entity.name.function", C_FUNCTION);
    push("support.function", C_FUNCTION);
    push("variable.function", C_FUNCTION);
    push("entity.name.tag", C_ATTRIBUTE);
    push("entity.other.attribute-name", C_ATTRIBUTE);
    push("variable.other.property", C_PROPERTY);
    push("variable.other.member", C_PROPERTY);
    push("support.variable.property", C_PROPERTY);
    push("meta.object-literal.key", C_PROPERTY);
    push("variable", C_VARIABLE);
    push("punctuation", C_PUNCT);
    Theme {
        name: Some("LazeCode Tokens".into()),
        author: None,
        settings: ThemeSettings {
            foreground: Some(C_TEXT),
            background: Some(Color {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            }),
            ..Default::default()
        },
        scopes,
    }
}

fn kind_for_color(color: Color) -> TokenKind {
    match (color.r, color.g, color.b) {
        (2, 2, 2) => TokenKind::Keyword,
        (3, 3, 3) => TokenKind::Type,
        (4, 4, 4) => TokenKind::String,
        (5, 5, 5) => TokenKind::Number,
        (6, 6, 6) => TokenKind::Function,
        (7, 7, 7) => TokenKind::Comment,
        (8, 8, 8) => TokenKind::Punctuation,
        (9, 9, 9) => TokenKind::Literal,
        (10, 10, 10) => TokenKind::Variable,
        (11, 11, 11) => TokenKind::Property,
        (12, 12, 12) => TokenKind::Attribute,
        _ => TokenKind::Text,
    }
}

/// Comment prefix/suffix for the common languages handled by the editor.
/// The suffix is empty for line comments and `-->` for HTML-family blocks.
pub fn comment_tokens(path: &std::path::Path) -> (&'static str, &'static str) {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if matches!(
        file_name,
        ".gitignore"
            | ".gitattributes"
            | ".editorconfig"
            | "Makefile"
            | "makefile"
            | "Dockerfile"
            | "CMakeLists.txt"
    ) {
        return ("#", "");
    }
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
    {
        "py" | "sh" | "bash" | "zsh" | "rb" | "toml" | "yaml" | "yml" | "conf" | "gitignore" => {
            ("#", "")
        }
        "sql" | "lua" => ("--", ""),
        "html" | "htm" | "xml" | "md" | "markdown" | "vue" | "svelte" => ("<!--", "-->"),
        "bat" | "cmd" => ("REM", ""),
        _ => ("//", ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_rust_keyword_and_string() {
        let rope = Rope::from_str("fn main() { let s = \"hi\"; }\n");
        let mut highlighter = SyntaxHighlighter::new(std::path::Path::new("main.rs"));
        highlighter.ensure(&rope, 0, 0);
        let spans = highlighter.spans(0);
        assert!(spans.iter().any(|s| s.kind == TokenKind::Keyword));
        assert!(spans.iter().any(|s| s.kind == TokenKind::String));
    }

    #[test]
    fn spans_stay_within_line_content() {
        let rope = Rope::from_str("fn a() {}\nfn b() {}\n");
        let mut highlighter = SyntaxHighlighter::new(std::path::Path::new("main.rs"));
        highlighter.ensure(&rope, 0, 1);
        for line in 0..2 {
            for span in highlighter.spans(line) {
                assert!(span.end <= 9, "span {:?}", span);
            }
        }
    }
}
