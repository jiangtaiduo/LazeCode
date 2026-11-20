//! UI glue for the optional LSP client: provider popups plus once-per-frame
//! polling of completed responses.

use std::time::{Duration, Instant};

use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, StrokeKind, Vec2};
use lazecode_core::lsp::{self, Diagnostic, DiagnosticSeverity, DocumentSymbol};
use lazecode_core::{AppModel, Command, FileTab, TabKind};
use lazecode_text::{Cursor, Document};

use crate::runtime::{LspCompletionState, LspHoverState, LspSymbolState, UiRuntime};
use crate::theme::Theme;

const COMPLETION_DELAY: Duration = Duration::from_millis(140);
const HOVER_DELAY: Duration = Duration::from_millis(420);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(6);

/// Find an open text buffer by tab id (any project/worktree).
pub fn file_tab(model: &AppModel, tab_id: u64) -> Option<&FileTab> {
    for project in &model.projects {
        let Some(tab) = project.tab_by_id(tab_id) else {
            continue;
        };
        if let TabKind::File(file) = &tab.kind {
            return Some(file);
        }
    }
    None
}

pub fn file_tab_mut(model: &mut AppModel, tab_id: u64) -> Option<&mut FileTab> {
    for project in &mut model.projects {
        let Some(tab) = project.tab_mut_by_id(tab_id) else {
            continue;
        };
        if let TabKind::File(file) = &mut tab.kind {
            return Some(file);
        }
    }
    None
}

/// Poll triggers and completed responses. Called once per frame, after
/// `AppModel::sync_lsp`.
pub fn poll(ctx: &egui::Context, model: &mut AppModel, rt: &mut UiRuntime) {
    let zh = model.settings.language.is_zh();

    if let Some((tab_id, at)) = rt.lsp.completion_due {
        if at.elapsed() >= COMPLETION_DELAY {
            rt.lsp.completion_due = None;
            if rt.lsp.completion.is_none() {
                start_completion(model, rt, tab_id);
            }
        } else {
            ctx.request_repaint_after(COMPLETION_DELAY - at.elapsed());
        }
    }
    if let Some((tab_id, cursor)) = rt.lsp.goto_request.take() {
        let info = file_tab(model, tab_id)
            .map(|file| (file.doc.path.clone(), file.doc.lsp_position(cursor)));
        if let Some((path, (line, character))) = info {
            if let Some(id) = model.lsp.definition(&path, line, character) {
                rt.lsp.goto_pending = Some(id);
            }
        }
    }
    if let Some(tab_id) = rt.lsp.format_request.take() {
        start_format(model, rt, tab_id, zh);
    }
    if let Some(tab_id) = rt.lsp.symbols_request.take() {
        start_symbols(model, rt, tab_id, zh);
    }

    poll_hover(model, rt, ctx);
    poll_completion(model, rt, ctx);
    poll_symbols(model, rt, ctx, zh);
    poll_goto(model, rt, zh);
    poll_format(model, rt, ctx, zh);
    poll_rename(model, rt, ctx, zh);

    if has_pending(rt) || model.lsp.has_pending_work() {
        ctx.request_repaint_after(Duration::from_millis(120));
    }
}

fn has_pending(rt: &UiRuntime) -> bool {
    rt.lsp.goto_pending.is_some()
        || rt.lsp.format_pending.is_some()
        || rt.lsp.rename_pending.is_some()
        || rt
            .lsp
            .completion
            .as_ref()
            .is_some_and(|state| state.request.is_some())
        || rt
            .lsp
            .hover
            .as_ref()
            .is_some_and(|state| state.request.is_some())
        || rt
            .lsp
            .symbols
            .as_ref()
            .is_some_and(|state| state.request.is_some())
}

fn start_completion(model: &mut AppModel, rt: &mut UiRuntime, tab_id: u64) {
    let info = file_tab(model, tab_id).map(|file| {
        (
            file.doc.path.clone(),
            file.doc.lsp_position(file.doc.cursor),
            file.doc.cursor,
            file.doc.revision,
            word_start(&file.doc, file.doc.cursor),
        )
    });
    let Some((path, (line, character), cursor, revision, prefix_start)) = info else {
        return;
    };
    let Some(id) = model.lsp.completion(&path, line, character) else {
        return;
    };
    rt.lsp.completion = Some(LspCompletionState {
        tab_id,
        request: Some(id),
        requested_at: Some(Instant::now()),
        items: Vec::new(),
        selected: 0,
        prefix_start,
        request_cursor: cursor,
        revision,
        anchor: egui::Pos2::ZERO,
    });
}

fn start_format(model: &mut AppModel, rt: &mut UiRuntime, tab_id: u64, zh: bool) {
    let Some(file) = file_tab(model, tab_id) else {
        return;
    };
    let path = file.doc.path.clone();
    let settings = model.settings.editor.clone();
    let Some(id) = model
        .lsp
        .formatting(&path, settings.tab_width as u32, settings.use_spaces)
    else {
        toast(
            model,
            zh,
            "没有可用的语言服务器",
            "No language server is available",
        );
        return;
    };
    rt.lsp.format_pending = Some((tab_id, id));
}

fn start_symbols(model: &mut AppModel, rt: &mut UiRuntime, tab_id: u64, zh: bool) {
    let Some(file) = file_tab(model, tab_id) else {
        return;
    };
    let path = file.doc.path.clone();
    let Some(id) = model.lsp.document_symbols(&path) else {
        toast(
            model,
            zh,
            "没有可用的语言服务器",
            "No language server is available",
        );
        return;
    };
    rt.lsp.symbols = Some(LspSymbolState {
        tab_id,
        request: Some(id),
        requested_at: Some(Instant::now()),
        items: Vec::new(),
        selected: 0,
    });
}

fn poll_hover(model: &mut AppModel, rt: &mut UiRuntime, ctx: &egui::Context) {
    let mut clear = false;
    if let Some(state) = rt.lsp.hover.as_mut() {
        if let Some(id) = state.request {
            match model.lsp.take_response(id) {
                Some(Ok(value)) => {
                    state.result = lsp::parse_hover(&value);
                    state.request = None;
                    ctx.request_repaint();
                }
                Some(Err(_)) => clear = true,
                None => {
                    if state
                        .requested_at
                        .is_some_and(|at| at.elapsed() > REQUEST_TIMEOUT)
                    {
                        model.lsp.cancel(id);
                        clear = true;
                    }
                }
            }
        }
    }
    if clear {
        rt.lsp.hover = None;
    }
}

fn poll_completion(model: &mut AppModel, rt: &mut UiRuntime, ctx: &egui::Context) {
    if poll_completion_state(model, rt, ctx) {
        rt.lsp.completion = None;
    }
}

/// Returns `true` when the popup should be closed.
fn poll_completion_state(model: &mut AppModel, rt: &mut UiRuntime, ctx: &egui::Context) -> bool {
    let Some(state) = rt.lsp.completion.as_mut() else {
        return false;
    };
    let current = file_tab(model, state.tab_id).map(|file| (file.doc.cursor, file.doc.revision));
    let Some((cursor, revision)) = current else {
        if let Some(id) = state.request.take() {
            model.lsp.cancel(id);
        }
        return true;
    };
    let stale = cursor != state.request_cursor || revision != state.revision;
    if let Some(id) = state.request {
        if stale {
            model.lsp.cancel(id);
            return true;
        }
        match model.lsp.take_response(id) {
            Some(Ok(value)) => {
                state.request = None;
                let items = lsp::parse_completions(&value);
                if items.is_empty() {
                    return true;
                }
                state.items = items;
                state.selected = 0;
                ctx.request_repaint();
            }
            Some(Err(_)) => return true,
            None => {
                if state
                    .requested_at
                    .is_some_and(|at| at.elapsed() > REQUEST_TIMEOUT)
                {
                    model.lsp.cancel(id);
                    return true;
                }
            }
        }
    } else if stale {
        return true;
    }
    false
}

fn poll_symbols(model: &mut AppModel, rt: &mut UiRuntime, ctx: &egui::Context, zh: bool) {
    let mut clear = false;
    if let Some(state) = rt.lsp.symbols.as_mut() {
        if let Some(id) = state.request {
            match model.lsp.take_response(id) {
                Some(Ok(value)) => {
                    state.request = None;
                    state.items = lsp::parse_document_symbols(&value);
                    if state.items.is_empty() {
                        clear = true;
                        toast(model, zh, "没有找到符号", "No symbols found");
                    } else {
                        state.selected = 0;
                        ctx.request_repaint();
                    }
                }
                Some(Err(_)) => {
                    clear = true;
                    toast(model, zh, "符号查询失败", "Symbol lookup failed");
                }
                None => {
                    if state
                        .requested_at
                        .is_some_and(|at| at.elapsed() > REQUEST_TIMEOUT)
                    {
                        model.lsp.cancel(id);
                        clear = true;
                    }
                }
            }
        }
    }
    if clear {
        rt.lsp.symbols = None;
    }
}

fn poll_goto(model: &mut AppModel, rt: &mut UiRuntime, zh: bool) {
    let Some(id) = rt.lsp.goto_pending else {
        return;
    };
    match model.lsp.take_response(id) {
        Some(Ok(value)) => {
            rt.lsp.goto_pending = None;
            let locations = lsp::parse_locations(&value);
            if let Some(location) = locations.into_iter().next() {
                model.queue(Command::OpenFileAt {
                    path: location.path,
                    line: location.range.start.line,
                    character: location.range.start.character,
                });
            } else {
                toast(model, zh, "没有找到定义", "No definition found");
            }
        }
        Some(Err(_)) => {
            rt.lsp.goto_pending = None;
            toast(model, zh, "跳转定义失败", "Go to definition failed");
        }
        None => {}
    }
}

fn poll_format(model: &mut AppModel, rt: &mut UiRuntime, ctx: &egui::Context, zh: bool) {
    let Some((tab_id, id)) = rt.lsp.format_pending else {
        return;
    };
    match model.lsp.take_response(id) {
        Some(Ok(value)) => {
            rt.lsp.format_pending = None;
            let edits = lsp::parse_text_edits(&value);
            if edits.is_empty() {
                return;
            }
            let Some(file) = file_tab_mut(model, tab_id) else {
                return;
            };
            if file.doc.apply_text_edits(&edits) {
                ctx.request_repaint();
            } else {
                toast(model, zh, "格式化失败", "Formatting failed");
            }
        }
        Some(Err(_)) => {
            rt.lsp.format_pending = None;
            toast(model, zh, "格式化失败", "Formatting failed");
        }
        None => {}
    }
}

fn poll_rename(model: &mut AppModel, rt: &mut UiRuntime, ctx: &egui::Context, zh: bool) {
    let Some(id) = rt.lsp.rename_pending else {
        return;
    };
    match model.lsp.take_response(id) {
        Some(Ok(value)) => {
            rt.lsp.rename_pending = None;
            let edit = lsp::parse_workspace_edit(&value);
            if edit.is_empty() {
                toast(model, zh, "没有可重命名的符号", "Nothing to rename");
            } else {
                model.queue(Command::ApplyWorkspaceEdit(edit));
                ctx.request_repaint();
            }
        }
        Some(Err(_)) => {
            rt.lsp.rename_pending = None;
            toast(model, zh, "重命名失败", "Rename failed");
        }
        None => {}
    }
}

fn toast(model: &mut AppModel, zh: bool, en: &str, cn: &str) {
    model.toast(if zh { cn } else { en }, lazecode_core::ToastKind::Error);
}

fn word_start(doc: &Document, cursor: Cursor) -> Cursor {
    let line = doc.line_content(cursor.line);
    let chars: Vec<char> = line.chars().collect();
    let mut col = cursor.col.min(chars.len());
    while col > 0 && is_ident(chars[col - 1]) {
        col -= 1;
    }
    Cursor {
        line: cursor.line,
        col,
    }
}

fn is_ident(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// Whether a freshly typed character should ask for completion. Mirrors the
/// stable `triggerCharacters` list.
pub fn is_completion_trigger(text: &str) -> bool {
    text.chars()
        .last()
        .is_some_and(|ch| matches!(ch, '.' | ':' | '(' | '"' | '/' | '@' | '<' | '>'))
}

/// Drive hover acquisition. `cursor` is the character under the pointer, if
/// any; the caller already excludes drags and clicks.
pub fn update_hover(
    ctx: &egui::Context,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    tab_id: u64,
    cursor: Option<Cursor>,
) {
    if rt.lsp.completion.is_some() || rt.lsp.symbols.is_some() {
        clear_hover(model, rt);
        return;
    }
    let Some(cursor) = cursor else {
        clear_hover(model, rt);
        return;
    };
    let same = rt.lsp.hover.as_ref().is_some_and(|state| {
        state.tab_id == tab_id && state.line == cursor.line && state.col == cursor.col
    });
    if !same {
        clear_hover(model, rt);
        rt.lsp.hover = Some(LspHoverState {
            tab_id,
            line: cursor.line,
            col: cursor.col,
            pointer: ctx.pointer_hover_pos().unwrap_or_default(),
            since: Some(Instant::now()),
            request: None,
            requested_at: None,
            result: None,
        });
    }
    let settled = rt
        .lsp
        .hover
        .as_ref()
        .and_then(|state| state.since)
        .is_some_and(|since| since.elapsed() >= HOVER_DELAY);
    if !settled {
        if let Some(since) = rt.lsp.hover.as_ref().and_then(|state| state.since) {
            ctx.request_repaint_after(HOVER_DELAY.saturating_sub(since.elapsed()));
        }
        return;
    }
    if rt
        .lsp
        .hover
        .as_ref()
        .is_some_and(|state| state.result.is_some() || state.request.is_some())
    {
        return;
    }
    // Avoid hammering the manager every frame when the language has no
    // server (or the server is still starting). Once per second is enough.
    if rt
        .lsp
        .hover
        .as_ref()
        .and_then(|state| state.requested_at)
        .is_some_and(|at| at.elapsed() < Duration::from_secs(1))
    {
        return;
    }
    let info =
        file_tab(model, tab_id).map(|file| (file.doc.path.clone(), file.doc.lsp_position(cursor)));
    let Some((path, (line, character))) = info else {
        return;
    };
    let id = model.lsp.hover(&path, line, character);
    if let Some(state) = rt.lsp.hover.as_mut() {
        state.requested_at = Some(Instant::now());
        if let Some(id) = id {
            state.request = Some(id);
            state.pointer = ctx.pointer_hover_pos().unwrap_or(state.pointer);
        }
    }
}

fn clear_hover(model: &mut AppModel, rt: &mut UiRuntime) {
    if let Some(state) = rt.lsp.hover.take() {
        if let Some(id) = state.request {
            model.lsp.cancel(id);
        }
    }
}

/// Draw the hover tooltip when a response is ready.
pub fn draw_hover(ui: &egui::Ui, rt: &UiRuntime, theme: &Theme) {
    let Some(state) = &rt.lsp.hover else {
        return;
    };
    let Some(hover) = &state.result else {
        return;
    };
    let p = theme.palette;
    egui::Tooltip::always_open(
        ui.ctx().clone(),
        ui.layer_id(),
        egui::Id::unique(("lsp-hover", state.tab_id)),
        egui::PopupAnchor::Pointer,
    )
    .show(|ui| {
        ui.set_max_width(560.0);
        ui.label(egui::RichText::new(&hover.text).size(12.0).color(p.text));
    });
}

/// Draw the completion list below the caret. Purely visual; accepting is
/// handled in the editor input path.
pub fn draw_completion(ui: &egui::Ui, rt: &mut UiRuntime, theme: &Theme, line_h: f32) {
    let Some(state) = &rt.lsp.completion else {
        return;
    };
    if state.items.is_empty() {
        return;
    }
    let p = theme.palette;
    let screen = ui.max_rect();
    let caret = state.anchor;
    let row_h = 20.0;
    let visible = state.items.len().min(10);
    let width = 360.0_f32.min(screen.width() - 24.0);
    let height = row_h * visible as f32 + 8.0;
    let mut min = Pos2::new(caret.x, caret.y + line_h + 2.0);
    if min.x + width > screen.right() - 8.0 {
        min.x = (screen.right() - width - 8.0).max(screen.left() + 8.0);
    }
    if min.y + height > screen.bottom() - 8.0 {
        min.y = (caret.y - height - 2.0).max(screen.top() + 8.0);
    }
    let rect = Rect::from_min_size(min, Vec2::new(width, height));
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(6), p.elev);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(6),
        Stroke::new(1.0, p.border_strong),
        StrokeKind::Inside,
    );
    let painter = painter.with_clip_rect(rect.shrink(2.0));
    let start = state.selected.saturating_sub(visible - 1);
    for (row, item) in state.items.iter().skip(start).take(visible).enumerate() {
        let index = start + row;
        let y = rect.top() + 4.0 + row as f32 * row_h;
        let row_rect = Rect::from_min_size(
            Pos2::new(rect.left() + 2.0, y),
            Vec2::new(rect.width() - 4.0, row_h),
        );
        if index == state.selected {
            painter.rect_filled(row_rect, egui::CornerRadius::same(4), p.active);
        }
        painter.text(
            Pos2::new(row_rect.left() + 8.0, row_rect.center().y),
            Align2::LEFT_CENTER,
            &item.label,
            FontId::proportional(12.0),
            p.text,
        );
        if let Some(detail) = &item.detail {
            painter.text(
                Pos2::new(row_rect.right() - 8.0, row_rect.center().y),
                Align2::RIGHT_CENTER,
                detail,
                FontId::proportional(10.5),
                p.text3,
            );
        }
    }
    // Register for the shell's wheel interception (next frame); `state`
    // borrowed `rt` above, so this must come after the loop.
    rt.menu_hit_rects.push(rect);
}

/// Draw the document-symbol picker.
pub fn draw_symbols(ui: &egui::Ui, model: &AppModel, rt: &mut UiRuntime, theme: &Theme) {
    let Some(state) = &rt.lsp.symbols else {
        return;
    };
    if state.items.is_empty() {
        return;
    }
    let p = theme.palette;
    let screen = ui.max_rect();
    let row_h = 22.0;
    let visible = state.items.len().min(14);
    let width = 520.0_f32.min(screen.width() - 60.0);
    let height = row_h * visible as f32 + 34.0;
    let rect = Rect::from_center_size(
        Pos2::new(screen.center().x, screen.top() + 80.0 + height / 2.0),
        Vec2::new(width, height),
    );
    let painter = ui.painter();
    painter.rect_filled(rect, egui::CornerRadius::same(8), p.elev);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(8),
        Stroke::new(1.0, p.border_strong),
        StrokeKind::Inside,
    );
    painter.text(
        Pos2::new(rect.left() + 14.0, rect.top() + 18.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Go to Symbol", "转到符号"),
        FontId::proportional(12.0),
        p.text2,
    );
    let list = Rect::from_min_max(
        Pos2::new(rect.left() + 6.0, rect.top() + 32.0),
        Pos2::new(rect.right() - 6.0, rect.bottom() - 6.0),
    );
    let painter = painter.with_clip_rect(list);
    let start = state.selected.saturating_sub(visible - 1);
    for (row, symbol) in state.items.iter().skip(start).take(visible).enumerate() {
        let index = start + row;
        let y = list.top() + row as f32 * row_h;
        let row_rect =
            Rect::from_min_size(Pos2::new(list.left(), y), Vec2::new(list.width(), row_h));
        if index == state.selected {
            painter.rect_filled(row_rect, egui::CornerRadius::same(4), p.active);
        }
        painter.text(
            Pos2::new(
                row_rect.left() + 10.0 + symbol.depth as f32 * 14.0,
                row_rect.center().y,
            ),
            Align2::LEFT_CENTER,
            symbol_label(symbol),
            FontId::proportional(12.0),
            p.text,
        );
    }
    // Register for the shell's wheel interception (next frame); `state`
    // borrowed `rt` above, so this must come after the loop.
    rt.menu_hit_rects.push(rect);
}

fn symbol_label(symbol: &DocumentSymbol) -> String {
    let kind = match symbol.kind {
        2 | 3 | 5 | 8 | 9 => "◇",
        6 => "ƒ",
        12 => "▸",
        13 => "▸",
        _ => "·",
    };
    format!("{kind} {}", symbol.name)
}

/// Insert the selected completion item, replacing the typed prefix.
pub fn accept_completion(doc: &mut Document, state: &LspCompletionState) -> bool {
    let Some(item) = state.items.get(state.selected) else {
        return false;
    };
    if state.prefix_start.line != doc.cursor.line || state.prefix_start.col > doc.cursor.col {
        return false;
    }
    doc.selection = Some(lazecode_text::Selection {
        anchor: state.prefix_start,
        head: doc.cursor,
    });
    doc.insert_text(&item.insert_text);
    true
}

/// Jump to the selected document symbol.
pub fn accept_symbol(doc: &mut Document, state: &LspSymbolState) -> bool {
    let Some(symbol) = state.items.get(state.selected) else {
        return false;
    };
    let cursor = doc.lsp_cursor(
        symbol.selection.start.line,
        symbol.selection.start.character,
    );
    doc.set_cursor(cursor, false);
    doc.scroll_line = cursor.line.saturating_sub(6) as f32;
    true
}

/// Count problems for the status bar.
pub fn diagnostic_counts(diagnostics: &[Diagnostic]) -> (usize, usize) {
    let mut errors = 0;
    let mut warnings = 0;
    for diagnostic in diagnostics {
        match diagnostic.severity {
            DiagnosticSeverity::Error => errors += 1,
            DiagnosticSeverity::Warning => warnings += 1,
            _ => {}
        }
    }
    (errors, warnings)
}

/// Colour for a diagnostic underline.
pub fn diagnostic_color(theme: &Theme, severity: DiagnosticSeverity) -> Color32 {
    let p = theme.palette;
    match severity {
        DiagnosticSeverity::Error => p.rose,
        DiagnosticSeverity::Warning => p.amber,
        DiagnosticSeverity::Information => p.blue,
        DiagnosticSeverity::Hint => p.text3,
    }
}

/// Hover text for a diagnostic under the pointer.
pub fn diagnostic_at<'a>(
    doc: &Document,
    diagnostics: &'a [Diagnostic],
    cursor: Cursor,
) -> Option<&'a Diagnostic> {
    let (line, character) = doc.lsp_position(cursor);
    diagnostics.iter().find(|diagnostic| {
        let start = diagnostic.range.start;
        let end = diagnostic.range.end;
        if line < start.line || line > end.line {
            return false;
        }
        let after_start = line > start.line || character >= start.character;
        let before_end = line < end.line || character <= end.character;
        after_start && before_end
    })
}

/// Foreground overlays for LSP providers. Called after the panels so the
/// completion list and symbol picker are never covered by the terminal or
/// status bar.
pub fn draw_popups(ui: &egui::Ui, model: &AppModel, rt: &mut UiRuntime, theme: &Theme) {
    let line_h = (model.settings.editor.font_size * 1.54).round().max(18.0);
    draw_completion(ui, rt, theme, line_h);
    draw_symbols(ui, model, rt, theme);
    draw_hover(ui, rt, theme);
}
