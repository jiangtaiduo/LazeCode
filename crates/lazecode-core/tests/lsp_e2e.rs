//! Optional end-to-end check against a real language server. Run with:
//! `cargo test -p lazecode-core --test lsp_e2e -- --ignored --nocapture`
//!
//! This is not part of CI: it requires `rust-analyzer` and `cargo` on PATH and
//! a few seconds of indexing. It proves the framing, initialize handshake,
//! document sync and a provider request against the real binary.

use std::time::{Duration, Instant};

use lazecode_core::lsp;
use lazecode_core::{LspManager, ServerKey};

#[test]
#[ignore = "requires rust-analyzer and cargo on PATH"]
fn rust_analyzer_initializes_and_answers_hover() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(dir.path().join("src")).expect("src dir");
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"lsp_e2e\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("manifest");
    let file = dir.path().join("src/lib.rs");
    let text = "pub fn answer() -> i32 { 42 }\npub fn use_it() {\n    let value = answer();\n    let _ = value;\n}\npub fn broken() {\n    let bad: i32 = \"text\";\n    let _ = bad;\n}\n";
    std::fs::write(&file, text).expect("write source");

    let key = ServerKey::new(dir.path(), "rust");
    let mut manager = LspManager::new(true, None);
    assert!(
        manager.ensure_server(&key),
        "rust-analyzer must start; install it or skip this ignored test"
    );

    for _ in 0..400 {
        manager.poll();
        if manager.server_ready(&key) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(manager.server_ready(&key), "initialize must complete");

    manager.note_document(key.clone(), file.clone(), "rust", 1, text.to_string());
    for _ in 0..20 {
        manager.poll();
        std::thread::sleep(Duration::from_millis(50));
    }

    // Cargo metadata/indexing can take a few seconds; retry hover until the
    // server returns a real payload (null means "not analyzed yet").
    let deadline = Instant::now() + Duration::from_secs(40);
    let mut hover = None;
    while Instant::now() < deadline && hover.is_none() {
        let id = manager.hover(&file, 2, 17).expect("hover request");
        let mut response = None;
        for _ in 0..40 {
            manager.poll();
            if let Some(result) = manager.take_response(id) {
                response = Some(result);
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if let Some(value) = response.and_then(Result::ok) {
            hover = lsp::parse_hover(&value);
        }
        if hover.is_none() {
            std::thread::sleep(Duration::from_millis(300));
        }
    }
    let hover = hover.expect("rust-analyzer should answer hover after indexing");
    assert!(!hover.text.is_empty());
    assert!(
        hover.text.contains("fn answer") || hover.text.contains("i32"),
        "expected a function/type signature in hover: {}",
        hover.text
    );

    // Diagnostics: the `i32 = "text"` line must produce an error. Some
    // servers publish on their own cadence after didOpen.
    let mut diagnostics = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        manager.poll();
        diagnostics = manager.diagnostics(&file).to_vec();
        if diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == lsp::DiagnosticSeverity::Error)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == lsp::DiagnosticSeverity::Error),
        "expected a type error diagnostic, got {diagnostics:?}"
    );

    // Document symbols exercise a second provider path.
    let id = manager.document_symbols(&file).expect("symbol request");
    let mut symbols = None;
    for _ in 0..100 {
        manager.poll();
        if let Some(result) = manager.take_response(id) {
            symbols = Some(result);
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let symbols = symbols.expect("symbol response").expect("symbol result");
    let symbols = lsp::parse_document_symbols(&symbols);
    assert!(
        symbols.iter().any(|symbol| symbol.name == "answer"),
        "expected function symbols, got {symbols:?}"
    );

    manager.shutdown_all();
}
