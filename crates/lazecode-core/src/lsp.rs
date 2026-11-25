//! Minimal Language Server Protocol client.
//!
//! One server per `(workspace root, language)`: JSON-RPC over stdio, document
//! sync, diagnostics and the provider requests the editor uses. The UI thread
//! never blocks (reads/writes run on per-server threads, every queue bounded,
//! every request with a deadline); a missing server degrades to a plain editor.
//!
//! Protocol types, language detection and payload parsing live in the
//! `protocol`/`detect`/`parse` submodules and are re-exported here.

mod detect;
mod parse;
mod protocol;

pub use detect::*;
pub use parse::*;
pub use protocol::*;

use detect::candidates;

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command as ProcessCommand, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};
use serde_json::{json, Value};

use lazecode_terminal::Notifier;

/// How long a document may be idle before the full-text `didChange` is sent.
/// LSP allows version jumps, so coalescing keystrokes is safe and keeps the
/// server from re-parsing the whole buffer on every key.
const CHANGE_DEBOUNCE: Duration = Duration::from_millis(200);
/// Feature requests that never answer are abandoned so the UI can recover.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// A server that fails to start is not retried for this long.
const SPAWN_BACKOFF: Duration = Duration::from_secs(30);
/// Hard cap on completed responses waiting for the UI to consume them.
const MAX_QUEUED_RESPONSES: usize = 256;
/// Ubuntu/stable LSP frame limit; protects against a broken length header.
const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;
#[derive(Clone, Debug)]
enum LspEvent {
    Message { key: ServerKey, value: Value },
    ReaderClosed { key: ServerKey },
    WriterClosed { key: ServerKey, error: String },
}

#[derive(Clone, Debug)]
enum ServerState {
    Starting,
    Ready,
    Failed(String),
}

struct ServerProcess {
    child: Child,
    writer: Sender<Vec<u8>>,
    state: ServerState,
    init_id: i64,
    /// Documents that asked for this server, including ones waiting for
    /// `initialize` to complete. Prevents `retain_documents` from stopping a
    /// server that is still starting.
    wanted: HashSet<PathBuf>,
}

struct DocumentState {
    key: ServerKey,
    language: String,
    /// LSP document version; starts at 1 after `didOpen`.
    version: i64,
    /// Latest editor revision observed.
    revision: u64,
    /// Full text waiting for the debounce window.
    pending_text: Option<String>,
    last_change: Instant,
    opened: bool,
}

struct PendingRequest {
    key: ServerKey,
    method: &'static str,
    sent: Instant,
}

impl Drop for LspManager {
    fn drop(&mut self) {
        // Last-resort cleanup for normal process teardown; explicit quit paths
        // call `shutdown_all` earlier so the UI does not exit with live servers.
        self.shutdown_all();
    }
}

/// Language-server registry and protocol client. Owned by [`crate::AppModel`]
/// and driven from the UI thread through [`LspManager::poll`].
pub struct LspManager {
    enabled: bool,
    servers: HashMap<ServerKey, ServerProcess>,
    documents: HashMap<PathBuf, DocumentState>,
    diagnostics: HashMap<PathBuf, Vec<Diagnostic>>,
    pending: HashMap<i64, PendingRequest>,
    responses: HashMap<i64, Result<Value, String>>,
    response_order: VecDeque<i64>,
    events_tx: Sender<LspEvent>,
    events_rx: Receiver<LspEvent>,
    next_id: i64,
    failed: HashMap<ServerKey, (Instant, String)>,
    notifier: Option<Arc<dyn Notifier>>,
    /// Last user-visible-ish failure (log-only; the editor stays usable).
    pub last_error: Option<String>,
    /// Bumped whenever diagnostics change so the UI can cheaply notice.
    pub diagnostics_generation: u64,
    /// Signature of the document set/revisions that was last reconciled, so
    /// `sync_lsp` can skip per-frame key building and full-text syncs.
    reconcile_signature: Option<u64>,
}

impl LspManager {
    pub fn new(enabled: bool, notifier: Option<Arc<dyn Notifier>>) -> Self {
        let (events_tx, events_rx) = bounded(4096);
        Self {
            enabled,
            servers: HashMap::new(),
            documents: HashMap::new(),
            diagnostics: HashMap::new(),
            pending: HashMap::new(),
            responses: HashMap::new(),
            response_order: VecDeque::new(),
            events_tx,
            events_rx,
            next_id: 1,
            failed: HashMap::new(),
            notifier,
            last_error: None,
            diagnostics_generation: 0,
            reconcile_signature: None,
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Toggle the optional integration. Disabling stops every server; enabling
    /// lets the next `sync_documents` call re-open the active buffers.
    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        if !enabled {
            self.shutdown_all();
        }
    }

    /// Kill every server. Called on quit/disable so child processes do not
    /// outlive the app (LSP servers are not tied to our process group by OS
    /// defaults, so `kill` alone is not enough).
    pub fn shutdown_all(&mut self) {
        for (_, server) in self.servers.drain() {
            terminate(server.child);
        }
        self.pending.clear();
        self.responses.clear();
        self.response_order.clear();
        self.documents.clear();
        self.diagnostics.clear();
        self.failed.clear();
        // Re-enabling must re-open every buffer even if nothing else changed.
        self.reconcile_signature = None;
    }

    /// Record the current document-set signature; true when it differs from
    /// the last reconciled one (i.e. reconciliation is due).
    pub fn note_reconcile_signature(&mut self, signature: u64) -> bool {
        if self.reconcile_signature == Some(signature) {
            return false;
        }
        self.reconcile_signature = Some(signature);
        true
    }

    pub fn server_count(&self) -> usize {
        self.servers.len()
    }

    pub fn document_revision(&self, path: &Path) -> Option<u64> {
        self.documents.get(path).map(|doc| doc.revision)
    }

    /// True when the manager has no usable text for this buffer (new file,
    /// crashed/reopened server, or a different workspace server).
    pub fn document_needs_text(&self, key: &ServerKey, path: &Path, revision: u64) -> bool {
        match self.documents.get(path) {
            None => true,
            Some(doc) => {
                doc.key != *key
                    || doc.revision != revision
                    || (!doc.opened && doc.pending_text.is_none())
            }
        }
    }

    /// Keep a document attached to an existing server without re-sending its
    /// text. This preserves `wanted` membership across server restarts.
    pub fn touch_document(&mut self, key: &ServerKey, path: &Path) {
        if !self.enabled {
            return;
        }
        let matches = self.documents.get(path).is_some_and(|doc| doc.key == *key);
        if !matches {
            return;
        }
        if self
            .servers
            .get(key)
            .is_some_and(|server| server.wanted.contains(path))
        {
            return;
        }
        if !self.servers.contains_key(key) {
            self.ensure_server(key);
        }
        if let Some(server) = self.servers.get_mut(key) {
            server.wanted.insert(path.to_path_buf());
        }
    }

    pub fn has_pending_work(&self) -> bool {
        if !self.pending.is_empty() {
            return true;
        }
        self.documents.iter().any(|(_, doc)| {
            self.server_ready(&doc.key) && (doc.pending_text.is_some() || !doc.opened)
        })
    }

    /// Register/replace a buffer's current text. The manager debounces the
    /// `didChange` and waits for `initialize` before the first `didOpen`.
    pub fn note_document(
        &mut self,
        key: ServerKey,
        path: PathBuf,
        language: &str,
        revision: u64,
        text: String,
    ) {
        if !self.enabled {
            return;
        }
        if self.documents.get(&path).is_some_and(|doc| doc.key != key) {
            self.close_document(&path);
        }
        let now = Instant::now();
        let needs_server = !self.servers.contains_key(&key);
        if needs_server {
            self.ensure_server(&key);
        }
        if let Some(server) = self.servers.get_mut(&key) {
            server.wanted.insert(path.clone());
        }
        let doc = self.documents.entry(path).or_insert_with(|| DocumentState {
            key: key.clone(),
            language: language.to_string(),
            version: 0,
            revision: u64::MAX,
            pending_text: None,
            last_change: now,
            opened: false,
        });
        if doc.language != language {
            doc.language = language.to_string();
        }
        if doc.revision != revision || (!doc.opened && doc.pending_text.is_none()) {
            doc.revision = revision;
            doc.pending_text = Some(text);
            doc.last_change = now;
        }
    }

    /// Send any debounced change immediately; provider requests must see the
    /// same buffer the user is looking at.
    pub fn flush_document(&mut self, path: &Path) {
        let Some(key) = self.documents.get(path).map(|doc| doc.key.clone()) else {
            return;
        };
        if !self.server_ready(&key) {
            return;
        }
        let Some(doc) = self.documents.get_mut(path) else {
            return;
        };
        let mut outgoing: Option<(&'static str, Value)> = None;
        if !doc.opened {
            if let Some(text) = doc.pending_text.take() {
                doc.version = 1;
                doc.opened = true;
                outgoing = Some((
                    "textDocument/didOpen",
                    json!({
                        "textDocument": {
                            "uri": path_to_uri(path),
                            "languageId": doc.language,
                            "version": doc.version,
                            "text": text,
                        }
                    }),
                ));
            }
        } else if let Some(text) = doc.pending_text.take() {
            doc.version += 1;
            outgoing = Some((
                "textDocument/didChange",
                json!({
                    "textDocument": {"uri": path_to_uri(path), "version": doc.version},
                    "contentChanges": [{"text": text}],
                }),
            ));
        }
        if let Some((method, params)) = outgoing {
            self.send_notification(&key, method, params);
        }
    }

    /// Close buffers that no longer exist in the model and stop servers that
    /// nobody needs any more. Called once per frame by `AppModel::sync_lsp`.
    pub fn retain_documents(&mut self, active: &HashSet<PathBuf>) {
        let stale: Vec<PathBuf> = self
            .documents
            .keys()
            .filter(|path| !active.contains(*path))
            .cloned()
            .collect();
        for path in stale {
            self.close_document(&path);
        }
        let unused: Vec<ServerKey> = self
            .servers
            .iter()
            .filter(|(_, server)| server.wanted.is_empty())
            .map(|(key, _)| key.clone())
            .collect();
        for key in unused {
            self.stop_server(&key);
        }
        let live_roots: HashSet<PathBuf> = self
            .documents
            .values()
            .map(|document| document.key.root.clone())
            .collect();
        self.failed.retain(|key, _| live_roots.contains(&key.root));
    }

    fn close_document(&mut self, path: &Path) {
        let Some(doc) = self.documents.remove(path) else {
            return;
        };
        self.diagnostics.remove(path);
        self.diagnostics_generation = self.diagnostics_generation.wrapping_add(1);
        if let Some(server) = self.servers.get_mut(&doc.key) {
            server.wanted.remove(path);
        }
        if doc.opened {
            self.send_notification(
                &doc.key,
                "textDocument/didClose",
                json!({"textDocument": {"uri": path_to_uri(path)}}),
            );
        }
    }

    /// Drain protocol events, flush debounced edits, detect process exits and
    /// expire requests. Cheap when nothing happened.
    pub fn poll(&mut self) {
        let now = Instant::now();
        while let Ok(event) = self.events_rx.try_recv() {
            match event {
                LspEvent::Message { key, value } => self.handle_message(&key, value, now),
                LspEvent::ReaderClosed { key } => {
                    log::debug!("LSP[{}] stdout closed", key.language);
                }
                LspEvent::WriterClosed { key, error } => {
                    log::warn!("LSP[{}] writer stopped: {error}", key.language);
                    self.last_error = Some(error);
                }
            }
        }

        // Reap exited children. `try_wait` does not block.
        let keys: Vec<ServerKey> = self.servers.keys().cloned().collect();
        for key in keys {
            let exited = self
                .servers
                .get_mut(&key)
                .and_then(|server| server.child.try_wait().ok().flatten())
                .map(|status| status.code());
            if let Some(code) = exited {
                log::warn!("LSP[{}] exited ({code:?})", key.language);
                self.servers.remove(&key);
                let message = format!(
                    "language server for {} exited ({})",
                    key.language,
                    code.map(|code| code.to_string())
                        .unwrap_or_else(|| "signal".to_string())
                );
                self.last_error = Some(message.clone());
                self.failed.insert(key.clone(), (now, message.clone()));
                self.fail_key_requests(&key, &message);
                for doc in self.documents.values_mut() {
                    if doc.key == key {
                        doc.opened = false;
                        doc.version = 0;
                        doc.pending_text = None;
                    }
                }
            }
        }

        // Expire unanswered requests.
        let timed_out: Vec<(i64, &'static str, ServerKey)> = self
            .pending
            .iter()
            .filter(|(_, request)| {
                request.method != "initialize" && now.duration_since(request.sent) > REQUEST_TIMEOUT
            })
            .map(|(id, request)| (*id, request.method, request.key.clone()))
            .collect();
        for (id, method, key) in timed_out {
            self.pending.remove(&id);
            self.push_response(id, Err(format!("{method} timed out")));
            let _ = self.send_json_to(
                &key,
                json!({"jsonrpc": "2.0", "method": "$/cancelRequest", "params": {"id": id}}),
            );
        }

        // Flush debounced document changes once the server is ready.
        let ready_keys: HashSet<ServerKey> = self
            .servers
            .iter()
            .filter(|(_, server)| matches!(server.state, ServerState::Ready))
            .map(|(key, _)| key.clone())
            .collect();
        let mut actions: Vec<(ServerKey, PathBuf, String, i64, String, bool)> = Vec::new();
        for (path, doc) in self.documents.iter_mut() {
            if !ready_keys.contains(&doc.key) {
                continue;
            }
            if !doc.opened {
                if let Some(text) = doc.pending_text.take() {
                    doc.version = 1;
                    doc.opened = true;
                    actions.push((
                        doc.key.clone(),
                        path.clone(),
                        doc.language.clone(),
                        doc.version,
                        text,
                        true,
                    ));
                }
            } else if doc.pending_text.is_some()
                && now.duration_since(doc.last_change) >= CHANGE_DEBOUNCE
            {
                let text = doc.pending_text.take().unwrap_or_default();
                doc.version += 1;
                actions.push((
                    doc.key.clone(),
                    path.clone(),
                    doc.language.clone(),
                    doc.version,
                    text,
                    false,
                ));
            }
        }
        for (key, path, language, version, text, open) in actions {
            if open {
                self.send_notification(
                    &key,
                    "textDocument/didOpen",
                    json!({
                        "textDocument": {
                            "uri": path_to_uri(&path),
                            "languageId": language,
                            "version": version,
                            "text": text,
                        }
                    }),
                );
            } else {
                self.send_notification(
                    &key,
                    "textDocument/didChange",
                    json!({
                        "textDocument": {"uri": path_to_uri(&path), "version": version},
                        "contentChanges": [{"text": text}],
                    }),
                );
            }
        }
    }

    // -- lifecycle ---------------------------------------------------------

    /// Ensure a server exists for `key`. Returns whether a server is starting
    /// or ready. Missing binaries/backoff failures return `false` without
    /// surfacing a dialog.
    pub fn ensure_server(&mut self, key: &ServerKey) -> bool {
        if !self.enabled || key.language.is_empty() || !key.root.is_dir() {
            return false;
        }
        let state = self.servers.get(key).map(|server| server.state.clone());
        match state {
            Some(ServerState::Starting) | Some(ServerState::Ready) => return true,
            Some(ServerState::Failed(error)) => {
                log::debug!("LSP[{}] failed earlier: {error}", key.language);
                self.stop_server(key);
                self.failed.insert(key.clone(), (Instant::now(), error));
                return false;
            }
            None => {}
        }
        if let Some((at, _)) = self.failed.get(key) {
            if at.elapsed() < SPAWN_BACKOFF {
                return false;
            }
            self.failed.remove(key);
        }
        self.spawn_server(key)
    }

    fn spawn_server(&mut self, key: &ServerKey) -> bool {
        let path_env = augmented_path();
        let mut last_error = String::new();
        for candidate in candidates(&key.language) {
            match self.spawn_command(key, candidate.command, candidate.args, &path_env) {
                Ok(()) => return true,
                Err(error) => last_error = error,
            }
        }
        let message = if last_error.is_empty() {
            format!("no language server configured for {}", key.language)
        } else {
            last_error
        };
        log::info!(
            "LSP[{}] unavailable in {}: {message}",
            key.language,
            key.root.display()
        );
        self.failed
            .insert(key.clone(), (Instant::now(), message.clone()));
        self.last_error = Some(message);
        false
    }

    fn spawn_command(
        &mut self,
        key: &ServerKey,
        command: &str,
        args: &[&str],
        path_env: &std::ffi::OsStr,
    ) -> Result<(), String> {
        let mut process = ProcessCommand::new(command);
        process
            .args(args)
            .current_dir(&key.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("PATH", path_env);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            process.process_group(0);
        }
        let mut child = process
            .spawn()
            .map_err(|error| format!("{command}: {error}"))?;
        let Some(stdin) = child.stdin.take() else {
            terminate(child);
            return Err(format!("{command}: no stdin pipe"));
        };
        let Some(stdout) = child.stdout.take() else {
            terminate(child);
            return Err(format!("{command}: no stdout pipe"));
        };
        let Some(stderr) = child.stderr.take() else {
            terminate(child);
            return Err(format!("{command}: no stderr pipe"));
        };

        let (writer_tx, writer_rx) = bounded::<Vec<u8>>(256);
        spawn_writer(key.clone(), stdin, writer_rx, self.events_tx.clone());
        spawn_reader(
            key.clone(),
            stdout,
            self.events_tx.clone(),
            self.notifier.clone(),
        );
        spawn_stderr(key.clone(), stderr);

        let init_id = self.alloc_id();
        let pid = child.id();
        self.servers.insert(
            key.clone(),
            ServerProcess {
                child,
                writer: writer_tx,
                state: ServerState::Starting,
                init_id,
                wanted: HashSet::new(),
            },
        );
        self.failed.remove(key);
        self.pending.insert(
            init_id,
            PendingRequest {
                key: key.clone(),
                method: "initialize",
                sent: Instant::now(),
            },
        );
        let initialize = json!({
            "jsonrpc": "2.0",
            "id": init_id,
            "method": "initialize",
            "params": initialize_params(&key.root),
        });
        if let Err(error) = self.send_json_to(key, initialize) {
            self.pending.remove(&init_id);
            self.push_response(init_id, Err(error.clone()));
            self.failed
                .insert(key.clone(), (Instant::now(), error.clone()));
            self.last_error = Some(error);
            self.stop_server(key);
            return Err(format!("{command}: initialize could not be sent"));
        }
        log::info!("LSP[{}] started {command} (pid {pid})", key.language);
        Ok(())
    }

    fn stop_server(&mut self, key: &ServerKey) {
        let Some(server) = self.servers.remove(key) else {
            return;
        };
        terminate(server.child);
        self.fail_key_requests(key, "language server stopped");
        for doc in self.documents.values_mut() {
            if doc.key == *key {
                doc.opened = false;
                doc.version = 0;
                doc.pending_text = None;
            }
        }
    }

    fn fail_key_requests(&mut self, key: &ServerKey, message: &str) {
        let ids: Vec<i64> = self
            .pending
            .iter()
            .filter(|(_, request)| request.key == *key)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.pending.remove(&id);
            self.push_response(id, Err(message.to_string()));
        }
    }

    pub fn server_ready(&self, key: &ServerKey) -> bool {
        self.servers
            .get(key)
            .is_some_and(|server| matches!(server.state, ServerState::Ready))
    }

    pub fn server_ready_for_path(&self, path: &Path) -> bool {
        self.documents
            .get(path)
            .is_some_and(|doc| self.server_ready(&doc.key))
    }

    // -- requests ----------------------------------------------------------

    pub fn hover(&mut self, path: &Path, line: u32, character: u32) -> Option<i64> {
        self.flush_document(path);
        let key = self.documents.get(path)?.key.clone();
        if !self.server_ready(&key) {
            return None;
        }
        self.request(
            &key,
            "textDocument/hover",
            json!({
                "textDocument": {"uri": path_to_uri(path)},
                "position": {"line": line, "character": character},
            }),
        )
    }

    pub fn definition(&mut self, path: &Path, line: u32, character: u32) -> Option<i64> {
        self.flush_document(path);
        let key = self.documents.get(path)?.key.clone();
        if !self.server_ready(&key) {
            return None;
        }
        self.request(
            &key,
            "textDocument/definition",
            json!({
                "textDocument": {"uri": path_to_uri(path)},
                "position": {"line": line, "character": character},
            }),
        )
    }

    pub fn completion(&mut self, path: &Path, line: u32, character: u32) -> Option<i64> {
        self.flush_document(path);
        let key = self.documents.get(path)?.key.clone();
        if !self.server_ready(&key) {
            return None;
        }
        self.request(
            &key,
            "textDocument/completion",
            json!({
                "textDocument": {"uri": path_to_uri(path)},
                "position": {"line": line, "character": character},
                "context": {"triggerKind": 1},
            }),
        )
    }

    pub fn formatting(&mut self, path: &Path, tab_size: u32, insert_spaces: bool) -> Option<i64> {
        self.flush_document(path);
        let key = self.documents.get(path)?.key.clone();
        if !self.server_ready(&key) {
            return None;
        }
        self.request(
            &key,
            "textDocument/formatting",
            json!({
                "textDocument": {"uri": path_to_uri(path)},
                "options": {"tabSize": tab_size, "insertSpaces": insert_spaces},
            }),
        )
    }

    pub fn rename(
        &mut self,
        path: &Path,
        line: u32,
        character: u32,
        new_name: &str,
    ) -> Option<i64> {
        self.flush_document(path);
        let key = self.documents.get(path)?.key.clone();
        if !self.server_ready(&key) {
            return None;
        }
        self.request(
            &key,
            "textDocument/rename",
            json!({
                "textDocument": {"uri": path_to_uri(path)},
                "position": {"line": line, "character": character},
                "newName": new_name,
            }),
        )
    }

    pub fn document_symbols(&mut self, path: &Path) -> Option<i64> {
        self.flush_document(path);
        let key = self.documents.get(path)?.key.clone();
        if !self.server_ready(&key) {
            return None;
        }
        self.request(
            &key,
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": path_to_uri(path)}}),
        )
    }

    fn request(&mut self, key: &ServerKey, method: &'static str, params: Value) -> Option<i64> {
        let id = self.alloc_id();
        self.pending.insert(
            id,
            PendingRequest {
                key: key.clone(),
                method,
                sent: Instant::now(),
            },
        );
        let message = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        if let Err(error) = self.send_json_to(key, message) {
            self.pending.remove(&id);
            self.push_response(id, Err(error));
        }
        Some(id)
    }

    /// Take a completed response. `None` means "not answered yet"; the caller
    /// keeps polling. Unknown ids are dropped.
    pub fn take_response(&mut self, id: i64) -> Option<Result<Value, String>> {
        let response = self.responses.remove(&id);
        if response.is_some() {
            if let Some(position) = self.response_order.iter().position(|queued| *queued == id) {
                self.response_order.remove(position);
            }
        }
        response
    }

    pub fn cancel(&mut self, id: i64) {
        if let Some(request) = self.pending.remove(&id) {
            let _ = self.send_json_to(
                &request.key,
                json!({"jsonrpc": "2.0", "method": "$/cancelRequest", "params": {"id": id}}),
            );
        }
    }

    pub fn diagnostics(&self, path: &Path) -> &[Diagnostic] {
        self.diagnostics
            .get(path)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn diagnostic_counts(&self, path: &Path) -> (usize, usize) {
        let mut errors = 0;
        let mut warnings = 0;
        for diagnostic in self.diagnostics(path) {
            match diagnostic.severity {
                DiagnosticSeverity::Error => errors += 1,
                DiagnosticSeverity::Warning => warnings += 1,
                _ => {}
            }
        }
        (errors, warnings)
    }

    // -- protocol plumbing -------------------------------------------------

    fn alloc_id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        if self.next_id <= 0 {
            self.next_id = 1;
        }
        id
    }

    fn push_response(&mut self, id: i64, response: Result<Value, String>) {
        self.responses.insert(id, response);
        self.response_order.push_back(id);
        while self.response_order.len() > MAX_QUEUED_RESPONSES {
            if let Some(oldest) = self.response_order.pop_front() {
                self.responses.remove(&oldest);
            }
        }
    }

    fn send_json_to(&self, key: &ServerKey, value: Value) -> Result<(), String> {
        let Some(server) = self.servers.get(key) else {
            return Err("language server is not running".to_string());
        };
        let body = serde_json::to_vec(&value)
            .map_err(|error| format!("cannot encode LSP message: {error}"))?;
        let mut frame = Vec::with_capacity(body.len() + 32);
        frame.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
        frame.extend_from_slice(&body);
        match server.writer.try_send(frame) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err("language server write queue is full".to_string()),
            Err(TrySendError::Disconnected(_)) => Err("language server writer stopped".to_string()),
        }
    }

    fn send_notification(&self, key: &ServerKey, method: &str, params: Value) {
        let message = json!({"jsonrpc": "2.0", "method": method, "params": params});
        if let Err(error) = self.send_json_to(key, message) {
            log::debug!("LSP[{}] {method} not sent: {error}", key.language);
        }
    }

    fn handle_message(&mut self, key: &ServerKey, value: Value, now: Instant) {
        let id = value.get("id").cloned();
        let method = value
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_string);
        match (id, method) {
            (Some(id), Some(method)) => {
                // Server -> client request. Answer minimally so servers that
                // wait for configuration/progress do not stall.
                let result = match method.as_str() {
                    "workspace/configuration" => {
                        let count = value
                            .pointer("/params/items")
                            .and_then(Value::as_array)
                            .map(Vec::len)
                            .unwrap_or(0);
                        Value::Array(vec![Value::Null; count])
                    }
                    "workspace/applyEdit" => json!({"applied": false}),
                    "workspace/workspaceFolders" => json!([{
                        "uri": path_to_uri(&key.root),
                        "name": key
                            .root
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("workspace"),
                    }]),
                    "window/workDoneProgress/create"
                    | "client/registerCapability"
                    | "client/unregisterCapability" => Value::Null,
                    _ => Value::Null,
                };
                let response = json!({"jsonrpc": "2.0", "id": id, "result": result});
                if let Err(error) = self.send_json_to(key, response) {
                    log::debug!("LSP[{}] server request reply failed: {error}", key.language);
                }
            }
            (Some(id), None) => {
                let Some(id) = id.as_i64() else {
                    return;
                };
                // Decide the initialize outcome while holding the server
                // borrow, then mutate manager-level state after it is dropped.
                let mut init_state: Option<Result<(), String>> = None;
                if let Some(server) = self.servers.get_mut(key) {
                    if id == server.init_id {
                        let error = value
                            .get("error")
                            .and_then(|error| error.get("message"))
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        match error {
                            Some(error) => {
                                server.state = ServerState::Failed(error.clone());
                                init_state = Some(Err(error));
                            }
                            None => {
                                server.state = ServerState::Ready;
                                init_state = Some(Ok(()));
                            }
                        }
                    }
                }
                match init_state {
                    Some(Ok(())) => {
                        let initialized = json!({
                            "jsonrpc": "2.0",
                            "method": "initialized",
                            "params": {},
                        });
                        let _ = self.send_json_to(key, initialized);
                    }
                    Some(Err(error)) => {
                        self.failed.insert(key.clone(), (now, error.clone()));
                        self.last_error = Some(error);
                    }
                    None => {}
                }
                if self.pending.remove(&id).is_some() {
                    let response = match value.get("error") {
                        Some(error) => Err(error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("language server request failed")
                            .to_string()),
                        None => Ok(value.get("result").cloned().unwrap_or(Value::Null)),
                    };
                    self.push_response(id, response);
                }
            }
            (None, Some(method)) => match method.as_str() {
                "textDocument/publishDiagnostics" => {
                    let Some(uri) = value.pointer("/params/uri").and_then(Value::as_str) else {
                        return;
                    };
                    let Some(path) = uri_to_path(uri) else {
                        return;
                    };
                    if !self.documents.contains_key(&path) {
                        return;
                    }
                    let diagnostics = value
                        .pointer("/params/diagnostics")
                        .map(parse_diagnostics)
                        .unwrap_or_default();
                    if diagnostics.is_empty() {
                        self.diagnostics.remove(&path);
                    } else {
                        self.diagnostics.insert(path, diagnostics);
                    }
                    self.diagnostics_generation = self.diagnostics_generation.wrapping_add(1);
                }
                "window/showMessage" => {
                    let message = value
                        .pointer("/params/message")
                        .and_then(Value::as_str)
                        .unwrap_or("language server message")
                        .to_string();
                    log::warn!("LSP[{}] {message}", key.language);
                    self.last_error = Some(message);
                }
                "window/logMessage" => {
                    if let Some(message) = value.pointer("/params/message").and_then(Value::as_str)
                    {
                        log::debug!("LSP[{}] {message}", key.language);
                    }
                }
                _ => {}
            },
            (None, None) => {}
        }
    }
}

fn initialize_params(root: &Path) -> Value {
    let name = root_name(root);
    json!({
        "processId": std::process::id(),
        "clientInfo": {"name": "LazeCode", "version": env!("CARGO_PKG_VERSION")},
        "rootUri": path_to_uri(root),
        "rootPath": root.to_string_lossy(),
        "workspaceFolders": [{"uri": path_to_uri(root), "name": name}],
        "capabilities": {
            "workspace": {
                "workspaceFolders": true,
                "configuration": true,
                "applyEdit": true,
                "workspaceEdit": {"documentChanges": true}
            },
            "textDocument": {
                "synchronization": {"dynamicRegistration": false, "didSave": false},
                "hover": {"contentFormat": ["markdown", "plaintext"]},
                "definition": {"linkSupport": true},
                "completion": {
                    "completionItem": {
                        "snippetSupport": false,
                        "documentationFormat": ["markdown", "plaintext"]
                    }
                },
                "formatting": {},
                "rename": {"prepareSupport": false},
                "documentSymbol": {"hierarchicalDocumentSymbolSupport": true},
                "publishDiagnostics": {"relatedInformation": true, "versionSupport": true}
            }
        }
    })
}

fn root_name(root: &Path) -> String {
    root.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace")
        .to_string()
}

/// Finder-launched apps inherit a minimal `PATH`; cargo/homebrew servers live
/// outside it. Append the usual developer locations without dropping the
/// inherited entries.
fn augmented_path() -> std::ffi::OsString {
    let mut paths: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    let mut extras = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/sbin"),
        PathBuf::from("/sbin"),
    ];
    if let Some(home) = dirs::home_dir() {
        extras.push(home.join(".cargo/bin"));
        extras.push(home.join(".local/bin"));
    }
    for extra in extras {
        if !paths.contains(&extra) {
            paths.push(extra);
        }
    }
    std::env::join_paths(paths).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Threads
// ---------------------------------------------------------------------------

fn notify(notifier: &Option<Arc<dyn Notifier>>) {
    if let Some(notifier) = notifier {
        notifier.notify();
    }
}

fn spawn_writer(
    key: ServerKey,
    mut stdin: std::process::ChildStdin,
    receiver: Receiver<Vec<u8>>,
    events: Sender<LspEvent>,
) {
    let _ = std::thread::Builder::new()
        .name(format!("lsp-writer-{}", key.language))
        .spawn(move || {
            while let Ok(frame) = receiver.recv() {
                if stdin.write_all(&frame).and_then(|_| stdin.flush()).is_err() {
                    let _ = events.send(LspEvent::WriterClosed {
                        key,
                        error: "language server stdin closed".to_string(),
                    });
                    return;
                }
            }
        });
}

fn spawn_reader(
    key: ServerKey,
    stdout: std::process::ChildStdout,
    events: Sender<LspEvent>,
    notifier: Option<Arc<dyn Notifier>>,
) {
    let _ = std::thread::Builder::new()
        .name(format!("lsp-reader-{}", key.language))
        .spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_frame(&mut reader) {
                    Ok(Some(body)) => match serde_json::from_slice::<Value>(&body) {
                        Ok(value) => {
                            if events
                                .send(LspEvent::Message {
                                    key: key.clone(),
                                    value,
                                })
                                .is_err()
                            {
                                return;
                            }
                            notify(&notifier);
                        }
                        Err(error) => {
                            log::debug!(
                                "LSP[{}] dropped malformed JSON frame: {error}",
                                key.language
                            );
                        }
                    },
                    Ok(None) => break,
                    Err(error) => {
                        log::debug!("LSP[{}] read error: {error}", key.language);
                        break;
                    }
                }
            }
            let _ = events.send(LspEvent::ReaderClosed { key });
            notify(&notifier);
        });
}

fn spawn_stderr(key: ServerKey, stderr: std::process::ChildStderr) {
    let _ = std::thread::Builder::new()
        .name(format!("lsp-stderr-{}", key.language))
        .spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                log::debug!("LSP[{}] {line}", key.language);
            }
        });
}

fn read_frame(reader: &mut impl BufRead) -> std::io::Result<Option<Vec<u8>>> {
    let mut content_length: Option<usize> = None;
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = value.trim().parse::<usize>().ok();
        }
    }
    let Some(length) = content_length else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "LSP frame is missing Content-Length",
        ));
    };
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("LSP frame length {length} is out of range"),
        ));
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

fn terminate(mut child: Child) {
    #[cfg(unix)]
    {
        // Negative pid targets the process group created by `process_group(0)`,
        // so servers that spawn analyzers/daemons do not outlive the app.
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGTERM);
        }
    }
    let _ = child.kill();
    // `kill` leaves a zombie until reaped; the UI thread must not wait, so a
    // short-lived thread owns the Child and calls `wait`.
    let _ = std::thread::Builder::new()
        .name("lsp-reaper".to_string())
        .spawn(move || {
            let _ = child.wait();
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_table_matches_stable_scope() {
        assert_eq!(language_id_for_path(Path::new("a.py")), Some("python"));
        assert_eq!(language_id_for_path(Path::new("a.hpp")), Some("cpp"));
        assert_eq!(language_id_for_path(Path::new("Gemfile")), Some("ruby"));
        assert_eq!(language_id_for_path(Path::new("a.ts")), None);
        assert_eq!(language_id_for_path(Path::new("a.json")), None);
    }

    #[test]
    fn uri_round_trips_spaces_and_unicode() {
        let path = PathBuf::from("/tmp/中文 folder/main.rs");
        let uri = path_to_uri(&path);
        assert!(uri.starts_with("file:///tmp/"));
        assert!(!uri.contains(' '));
        assert_eq!(uri_to_path(&uri).as_deref(), Some(path.as_path()));
        assert_eq!(
            uri_to_path("file:///tmp/a%23b.rs"),
            Some(PathBuf::from("/tmp/a#b.rs"))
        );
    }

    #[test]
    fn parses_hover_markup_and_marked_strings() {
        let hover = json!({
            "contents": {"kind": "markdown", "value": "**docs**"},
            "range": {"start": {"line": 1, "character": 2}, "end": {"line": 1, "character": 5}},
        });
        let parsed = parse_hover(&hover).expect("hover");
        assert_eq!(parsed.text, "**docs**");
        assert_eq!(parsed.range.unwrap().start.line, 1);

        let marked = json!({"contents": [{"language": "rust", "value": "let x = 1;"}, "plain"]});
        assert_eq!(parse_hover(&marked).unwrap().text, "let x = 1;\n\nplain");
    }

    #[test]
    fn parses_definition_array_and_location_links() {
        let value = json!([
            {"uri": "file:///tmp/a.rs", "range": {"start": {"line": 3, "character": 1}, "end": {"line": 3, "character": 4}}},
            {"targetUri": "file:///tmp/b.rs", "targetSelectionRange": {"start": {"line": 8, "character": 0}, "end": {"line": 8, "character": 2}}}
        ]);
        let locations = parse_locations(&value);
        assert_eq!(locations.len(), 2);
        assert_eq!(locations[0].path, PathBuf::from("/tmp/a.rs"));
        assert_eq!(locations[1].path, PathBuf::from("/tmp/b.rs"));
        assert_eq!(locations[1].range.start.line, 8);
    }

    #[test]
    fn parses_completion_list_and_plain_arrays() {
        let list = json!({"isIncomplete": false, "items": [
            {"label": "print", "detail": "builtin", "insertText": "print($0)"},
        ]});
        let items = parse_completions(&list);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].insert_text, "print($0)");
        assert_eq!(items[0].detail.as_deref(), Some("builtin"));
    }

    #[test]
    fn parses_workspace_edits_both_shapes() {
        let value = json!({
            "changes": {
                "file:///tmp/a.rs": [
                    {"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}}, "newText": "A"}
                ]
            },
            "documentChanges": [
                {"textDocument": {"uri": "file:///tmp/b.rs", "version": 1}, "edits": [
                    {"range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 1}}, "newText": "B"}
                ]}
            ]
        });
        let edit = parse_workspace_edit(&value);
        assert_eq!(edit.files.len(), 2);
        assert!(edit.files.iter().all(|(_, edits)| edits.len() == 1));
    }

    #[test]
    fn parses_hierarchical_document_symbols_with_depth() {
        let value = json!([{
            "name": "root",
            "kind": 12,
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 9, "character": 0}},
            "selectionRange": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 4}},
            "children": [{
                "name": "child",
                "kind": 6,
                "range": {"start": {"line": 2, "character": 0}, "end": {"line": 2, "character": 4}},
                "selectionRange": {"start": {"line": 2, "character": 0}, "end": {"line": 2, "character": 4}}
            }]
        }]);
        let symbols = parse_document_symbols(&value);
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[1].depth, 1);
        assert_eq!(symbols[1].selection.start.line, 2);
    }

    #[test]
    fn reads_content_length_frames() {
        let raw = b"Content-Length: 2\r\n\r\n{}Content-Length: 7\r\n\r\n{\"a\":1}";
        let mut reader = BufReader::new(&raw[..]);
        assert_eq!(read_frame(&mut reader).unwrap().unwrap(), b"{}");
        assert_eq!(read_frame(&mut reader).unwrap().unwrap(), b"{\"a\":1}");
        assert!(read_frame(&mut reader).unwrap().is_none());
    }

    #[test]
    fn rejects_missing_or_oversized_content_length() {
        let mut reader = BufReader::new(&b"\r\n{}"[..]);
        assert!(read_frame(&mut reader).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn lifecycle_spawns_initializes_and_stops_a_server() {
        use std::os::unix::fs::PermissionsExt;

        let key = ServerKey::new(std::env::temp_dir(), "mock");
        let dir = std::env::temp_dir().join(format!("lazecode-lsp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let script_path = dir.join("mock-lsp");
        let body = r#"{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}"#;
        let script = format!(
            "#!/bin/sh\nIFS= read -r _header || exit 1\nIFS= read -r _blank || exit 1\nprintf 'Content-Length: %s\\r\\n\\r\\n%s' {} '{}'\nsleep 30\n",
            body.len(),
            body
        );
        std::fs::write(&script_path, script).expect("write mock server");
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod");

        let mut manager = LspManager::new(true, None);
        manager
            .spawn_command(&key, script_path.to_str().unwrap(), &[], &augmented_path())
            .expect("mock server starts");
        assert_eq!(manager.server_count(), 1);
        for _ in 0..100 {
            manager.poll();
            if manager.server_ready(&key) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(manager.server_ready(&key), "initialize must complete");
        manager.shutdown_all();
        assert_eq!(manager.server_count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn diagnostics_are_scoped_to_open_documents() {
        let mut manager = LspManager::new(true, None);
        let path = PathBuf::from("/tmp/lazecode-diagnostics.rs");
        manager.documents.insert(
            path.clone(),
            DocumentState {
                key: ServerKey::new("/tmp", "rust"),
                language: "rust".to_string(),
                version: 1,
                revision: 1,
                pending_text: None,
                last_change: Instant::now(),
                opened: true,
            },
        );
        let key = manager.documents.get(&path).unwrap().key.clone();
        manager.handle_message(
            &key,
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": {
                    "uri": path_to_uri(&path),
                    "diagnostics": [{
                        "range": {
                            "start": {"line": 2, "character": 1},
                            "end": {"line": 2, "character": 4}
                        },
                        "severity": 1,
                        "message": "mismatched types",
                        "source": "rustc"
                    }]
                }
            }),
            Instant::now(),
        );
        let diagnostics = manager.diagnostics(&path);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, DiagnosticSeverity::Error);
        assert_eq!(diagnostics[0].range.start.line, 2);

        // Unknown files must not accumulate diagnostics memory.
        manager.handle_message(
            &key,
            json!({
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": {
                    "uri": path_to_uri(Path::new("/tmp/not-open.rs")),
                    "diagnostics": [{
                        "range": {
                            "start": {"line": 0, "character": 0},
                            "end": {"line": 0, "character": 1}
                        },
                        "message": "x"
                    }]
                }
            }),
            Instant::now(),
        );
        assert!(manager
            .diagnostics(Path::new("/tmp/not-open.rs"))
            .is_empty());
    }
}
