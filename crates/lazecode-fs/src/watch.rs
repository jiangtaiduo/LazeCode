use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

/// Debounced batch of filesystem changes for a project.
#[derive(Clone, Debug, Default)]
pub struct WatchBatch {
    pub changed: Vec<PathBuf>,
}

/// Owns a recursive watcher for one project. Drop to stop watching.
pub struct ProjectWatcher {
    _watcher: notify::RecommendedWatcher,
    rx: Receiver<WatchBatch>,
    root: PathBuf,
}

impl ProjectWatcher {
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Non-blocking poll, returns all changes accumulated in the last poll.
    pub fn poll(&self) -> Option<WatchBatch> {
        let mut batch = WatchBatch::default();
        while let Ok(next) = self.rx.try_recv() {
            batch.changed.extend(next.changed);
        }
        if batch.changed.is_empty() {
            None
        } else {
            batch.changed.sort();
            batch.changed.dedup();
            Some(batch)
        }
    }
}

/// Watch `root` recursively on a background thread and coalesce events into
/// roughly 200 ms batches. Errors are reported through the returned option.
pub fn watch_project(root: &Path) -> anyhow::Result<ProjectWatcher> {
    use notify::{RecursiveMode, Watcher};

    let (raw_tx, raw_rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::RecommendedWatcher::new(
        move |event| {
            let _ = raw_tx.send(event);
        },
        notify::Config::default().with_poll_interval(Duration::from_millis(500)),
    )?;
    watcher.watch(root, RecursiveMode::Recursive)?;

    let (tx, rx) = mpsc::channel::<WatchBatch>();
    std::thread::Builder::new()
        .name("lazecode-fs-watch".into())
        .spawn(move || {
            while let Ok(first) = raw_rx.recv() {
                let mut batch = WatchBatch::default();
                push_event(&mut batch, first);
                let deadline = std::time::Instant::now() + Duration::from_millis(200);
                loop {
                    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                    if remaining.is_zero() {
                        break;
                    }
                    match raw_rx.recv_timeout(remaining) {
                        Ok(event) => push_event(&mut batch, event),
                        Err(mpsc::RecvTimeoutError::Timeout) => break,
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
                batch.changed.sort();
                batch.changed.dedup();
                if !batch.changed.is_empty() && tx.send(batch).is_err() {
                    return;
                }
            }
        })?;

    Ok(ProjectWatcher {
        _watcher: watcher,
        rx,
        root: root.to_path_buf(),
    })
}

fn push_event(batch: &mut WatchBatch, event: notify::Result<notify::Event>) {
    let Ok(event) = event else { return };
    use notify::EventKind;
    if matches!(
        event.kind,
        EventKind::Access(_)
            | EventKind::Other
            | EventKind::Modify(notify::event::ModifyKind::Metadata(_))
    ) {
        return;
    }
    let root = event
        .paths
        .first()
        .and_then(|p| p.parent())
        .unwrap_or(Path::new(""));
    let _ = root;
    for path in event.paths {
        if path
            .components()
            .any(|component| crate::is_default_ignored(component.as_os_str()))
        {
            continue;
        }
        batch.changed.push(path);
    }
}
