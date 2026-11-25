//! Terminal runtime state: selections, busy probes, resize/reflow gates,
//! search and the console-activity thresholds.

#[derive(Clone, Copy, Debug)]
pub struct TermSelection {
    /// Absolute grid line (negative = scrollback).
    pub start: (i32, usize),
    pub end: (i32, usize),
}

impl TermSelection {
    pub fn normalised(&self) -> ((i32, usize), (i32, usize)) {
        if (self.start.0, self.start.1) <= (self.end.0, self.end.1) {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        }
    }
}

/// Periodic `ps -t` result for one terminal pane.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerminalBusyState {
    pub busy: bool,
    pub outputting: bool,
    pub commands: Vec<String>,
    pub last_output_ms: Option<u64>,
}

/// In-flight one-off probe for a terminal-close confirmation.
pub(super) struct PendingCloseProbe {
    pub(super) action: TerminalCloseAction,
    pub(super) pane_ids: Vec<u64>,
    pub(super) rx: std::sync::mpsc::Receiver<Vec<(u64, TerminalBusyState)>>,
}

/// In-flight one-off probe for the quit guard.
pub(super) struct PendingQuitProbe {
    pub(super) pane_ids: Vec<u64>,
    pub(super) rx: std::sync::mpsc::Receiver<Vec<(u64, TerminalBusyState)>>,
}

/// Action closed by the terminal confirmation modal.
#[derive(Clone, Debug)]
pub enum TerminalCloseAction {
    Pane(u64),
    Tab(u64),
    Others(u64),
    All,
    Project(usize),
}

/// Stable console-activity thresholds.
pub const OUTPUT_ACTIVE_MS: u64 = 2_000;
pub const INPUT_ECHO_GRACE_MS: u64 = 200;
pub const UI_REDRAW_GRACE_MS: u64 = 500;

/// Whether recent PTY output lights the project marquee: echo right after
/// input or a resize does not count, and a console already outputting stays
/// hot across a resize.
pub fn console_outputting(
    busy: bool,
    last_output_ms: Option<u64>,
    last_input_ms: Option<u64>,
    last_resize_ms: Option<u64>,
    was_outputting: bool,
) -> bool {
    if !busy {
        return false;
    }
    let Some(output_age) = last_output_ms.filter(|age| *age < OUTPUT_ACTIVE_MS) else {
        return false;
    };
    let after_input = last_input_ms.is_some_and(|input_age| {
        output_age <= input_age && input_age.saturating_sub(output_age) <= INPUT_ECHO_GRACE_MS
    });
    if after_input {
        return false;
    }
    let after_resize = last_resize_ms.is_some_and(|resize_age| {
        output_age <= resize_age && resize_age.saturating_sub(output_age) <= UI_REDRAW_GRACE_MS
    });
    !after_resize || was_outputting
}

/// Debounce window for terminal reflow while a pane is being resized.
const TERM_RESIZE_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(120);

/// Per-pane reflow gate. Resizing rewraps the whole scrollback and raises
/// SIGWINCH, so a drag that changes the size on every frame both hitches the
/// UI (proportional to the retained history) and storms full-screen TUIs with
/// repaints. The first layout applies immediately; later size changes apply
/// once the target has been stable for the debounce window.
#[derive(Clone, Copy, Debug)]
pub struct TermResizeGate {
    /// Size last handed to the session.
    applied: Option<(u16, u16)>,
    /// Newest target seen while a reflow is deferred.
    seen: Option<(u16, u16)>,
    seen_at: std::time::Instant,
}

impl Default for TermResizeGate {
    fn default() -> Self {
        Self {
            applied: None,
            seen: None,
            seen_at: std::time::Instant::now(),
        }
    }
}

impl TermResizeGate {
    /// Feed the pane's current target; true when the session should reflow now.
    pub fn should_apply(&mut self, target: (u16, u16)) -> bool {
        if self.seen != Some(target) {
            self.seen = Some(target);
            self.seen_at = std::time::Instant::now();
            if self.applied.is_none() {
                self.applied = Some(target);
                return true;
            }
            return false;
        }
        if self.applied == Some(target) {
            return false;
        }
        if self.seen_at.elapsed() >= TERM_RESIZE_DEBOUNCE {
            self.applied = Some(target);
            true
        } else {
            false
        }
    }

    /// Time left before the deferred target may be applied.
    pub fn remaining(&self) -> std::time::Duration {
        TERM_RESIZE_DEBOUNCE.saturating_sub(self.seen_at.elapsed())
    }

    /// Forget the applied size (a different session now owns this pane id).
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// One background scan for the terminal find bar.
struct PendingTermSearch {
    query: String,
    rx: std::sync::mpsc::Receiver<Vec<lazecode_terminal::TerminalMatch>>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// Find-in-terminal state. The scan walks the whole retained history, so it
/// runs on a worker after an input debounce instead of inline on the UI
/// thread (one full pass per keystroke cost ~70ms at 43k lines and seconds at
/// the cold byte cap).
#[derive(Default)]
pub struct TermSearchState {
    pub open: bool,
    pub query: String,
    pub matches: Vec<lazecode_terminal::TerminalMatch>,
    pub index: usize,
    pub focus_requested: bool,
    /// Earliest time the next scan may start (input debounce).
    pending_at: Option<std::time::Instant>,
    /// Scan currently running; superseding it cancels the worker.
    inflight: Option<PendingTermSearch>,
}

/// Input debounce for the find bar: one scan per typing burst.
const TERM_SEARCH_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(120);

impl TermSearchState {
    /// Open the find bar with an empty query.
    pub fn open_bar(&mut self) {
        self.open = true;
        self.focus_requested = true;
        self.query.clear();
        self.index = 0;
        self.matches.clear();
        self.pending_at = None;
        self.cancel_inflight();
    }

    /// Close the find bar: drop results and stop any running scan.
    pub fn close_bar(&mut self) {
        self.open = false;
        self.pending_at = None;
        self.cancel_inflight();
        self.matches.clear();
        self.index = 0;
    }

    /// Record a query edit: restart the debounce window.
    pub fn query_changed(&mut self) {
        self.index = 0;
        self.matches.clear();
        self.pending_at = Some(std::time::Instant::now() + TERM_SEARCH_DEBOUNCE);
        self.cancel_inflight();
    }

    /// Start the debounced scan and apply finished results. Returns whether
    /// `matches` changed.
    pub fn pump_search(
        &mut self,
        session: &lazecode_terminal::Session,
        ctx: &egui::Context,
    ) -> bool {
        let mut changed = false;
        if self.query.trim().is_empty() {
            self.pending_at = None;
            self.cancel_inflight();
            if !self.matches.is_empty() {
                self.matches.clear();
                changed = true;
            }
            return changed;
        }
        if let Some(at) = self.pending_at {
            let now = std::time::Instant::now();
            if now >= at {
                self.pending_at = None;
                self.cancel_inflight();
                let (rx, cancel) = session.search_async(self.query.clone());
                self.inflight = Some(PendingTermSearch {
                    query: self.query.clone(),
                    rx,
                    cancel,
                });
            } else {
                ctx.request_repaint_after(at - now + std::time::Duration::from_millis(1));
            }
        }
        if let Some(pending) = self.inflight.take() {
            match pending.rx.try_recv() {
                Ok(matches) => {
                    if pending.query == self.query {
                        self.matches = matches;
                        self.index = 0;
                        changed = true;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.inflight = Some(pending);
                    ctx.request_repaint_after(std::time::Duration::from_millis(16));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {}
            }
        }
        changed
    }

    fn cancel_inflight(&mut self) {
        if let Some(pending) = self.inflight.take() {
            pending
                .cancel
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

impl Drop for TermSearchState {
    fn drop(&mut self) {
        self.cancel_inflight();
    }
}
