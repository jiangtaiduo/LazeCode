//! JSON-RPC payload parsers for diagnostics, hovers, completions, edits and
//! document symbols.

use serde_json::Value;

use super::*;

pub(super) fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// JSON-RPC parsing
// ---------------------------------------------------------------------------

fn parse_position(value: &Value) -> Option<LspPosition> {
    Some(LspPosition {
        line: value.get("line")?.as_u64()? as u32,
        character: value.get("character")?.as_u64()? as u32,
    })
}

pub fn parse_range(value: &Value) -> Option<LspRange> {
    Some(LspRange {
        start: parse_position(value.get("start")?)?,
        end: parse_position(value.get("end")?)?,
    })
}

pub fn parse_diagnostics(value: &Value) -> Vec<Diagnostic> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            Some(Diagnostic {
                range: parse_range(item.get("range")?)?,
                severity: DiagnosticSeverity::from_lsp(
                    item.get("severity").and_then(Value::as_u64),
                ),
                message: item.get("message")?.as_str()?.to_string(),
                source: item
                    .get("source")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect()
}

fn hover_contents_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().filter_map(hover_contents_text).collect();
            (!parts.is_empty()).then(|| parts.join("\n\n"))
        }
        Value::Object(map) => map
            .get("value")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                map.get("language")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            }),
        _ => None,
    }
}

pub fn parse_hover(value: &Value) -> Option<Hover> {
    let contents = value.get("contents")?;
    let text = hover_contents_text(contents)?;
    if text.trim().is_empty() {
        return None;
    }
    Some(Hover {
        text,
        range: value.get("range").and_then(parse_range),
    })
}

pub fn parse_locations(value: &Value) -> Vec<Location> {
    let mut out = Vec::new();
    collect_locations(value, &mut out);
    out
}

fn collect_locations(value: &Value, out: &mut Vec<Location>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_locations(item, out);
            }
        }
        Value::Object(_) => {
            let uri = value
                .get("uri")
                .or_else(|| value.get("targetUri"))
                .and_then(Value::as_str);
            let Some(path) = uri.and_then(uri_to_path) else {
                return;
            };
            let range = value
                .get("range")
                .or_else(|| value.get("targetSelectionRange"))
                .or_else(|| value.get("targetRange"))
                .and_then(parse_range);
            if let Some(range) = range {
                out.push(Location { path, range });
            }
        }
        _ => {}
    }
}

pub fn parse_completions(value: &Value) -> Vec<CompletionItem> {
    let items = value
        .as_array()
        .or_else(|| value.get("items").and_then(Value::as_array));
    let Some(items) = items else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let label = item.get("label")?.as_str()?.to_string();
            let documentation = item.get("documentation").and_then(hover_contents_text);
            let insert_text = item
                .get("insertText")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    item.pointer("/textEdit/newText")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_else(|| label.clone());
            Some(CompletionItem {
                label,
                detail: item
                    .get("detail")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                documentation,
                insert_text,
                kind: item.get("kind").and_then(Value::as_u64),
            })
        })
        .collect()
}

pub fn parse_text_edits(value: &Value) -> Vec<lazecode_text::LspTextEdit> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let range = parse_range(item.get("range")?)?;
            let new_text = item.get("newText")?.as_str()?.to_string();
            Some(lazecode_text::LspTextEdit::new(
                range.start.line,
                range.start.character,
                range.end.line,
                range.end.character,
                new_text,
            ))
        })
        .collect()
}

pub fn parse_workspace_edit(value: &Value) -> WorkspaceEdit {
    let mut files: Vec<(PathBuf, Vec<lazecode_text::LspTextEdit>)> = Vec::new();
    let mut push = |path: PathBuf, edits: Vec<lazecode_text::LspTextEdit>| {
        if edits.is_empty() {
            return;
        }
        if let Some((_, existing)) = files.iter_mut().find(|(existing, _)| *existing == path) {
            existing.extend(edits);
        } else {
            files.push((path, edits));
        }
    };

    if let Some(changes) = value.get("changes").and_then(Value::as_object) {
        for (uri, edits) in changes {
            if let Some(path) = uri_to_path(uri) {
                push(path, parse_text_edits(edits));
            }
        }
    }
    if let Some(document_changes) = value.get("documentChanges").and_then(Value::as_array) {
        for change in document_changes {
            let Some(uri) = change.pointer("/textDocument/uri").and_then(Value::as_str) else {
                continue;
            };
            let Some(path) = uri_to_path(uri) else {
                continue;
            };
            let edits = change
                .get("edits")
                .map(parse_text_edits)
                .unwrap_or_default();
            push(path, edits);
        }
    }
    WorkspaceEdit { files }
}

pub fn parse_document_symbols(value: &Value) -> Vec<DocumentSymbol> {
    let mut out = Vec::new();
    if let Some(items) = value.as_array() {
        for item in items {
            collect_symbol(item, 0, &mut out);
        }
    }
    out
}

fn collect_symbol(value: &Value, depth: usize, out: &mut Vec<DocumentSymbol>) {
    let Some(name) = value.get("name").and_then(Value::as_str) else {
        return;
    };
    let selection = value
        .get("selectionRange")
        .and_then(parse_range)
        .or_else(|| value.get("range").and_then(parse_range))
        .or_else(|| value.pointer("/location/range").and_then(parse_range));
    if let Some(selection) = selection {
        out.push(DocumentSymbol {
            name: name.to_string(),
            detail: value
                .get("detail")
                .and_then(Value::as_str)
                .map(str::to_string),
            kind: value.get("kind").and_then(Value::as_u64).unwrap_or(0),
            selection,
            depth,
        });
    }
    if let Some(children) = value.get("children").and_then(Value::as_array) {
        for child in children {
            collect_symbol(child, depth + 1, out);
        }
    }
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------
