use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

/// Debounced batch of filesystem changes for a project.
#[derive(Clone, Debug, Default)]
pub struct WatchBatch {
    pub changed: Vec<PathBuf>,
    /// A change under `.git` (index, HEAD, refs, worktree metadata) that must
    /// refresh the source-control snapshot even when no working-tree file
    /// changed, e.g. a commit made by another tool.
    pub git_changed: bool,
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
        if batch.changed.is_empty() && !batch.git_changed {
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
                if (!batch.changed.is_empty() || batch.git_changed) && tx.send(batch).is_err() {
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
    // Git metadata events must be detected before dropping metadata/other
    // events: ref updates and index rewrites commonly arrive as metadata or
    // coalesced events. They refresh the Git snapshot without reloading the
    // whole file tree.
    if event.paths.iter().any(|path| {
        path.components()
            .any(|component| component.as_os_str() == ".git")
    }) {
        batch.git_changed = true;
    }
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
            .any(|component| component.as_os_str() == ".git")
        {
            batch.git_changed = true;
            continue;
        }
        if path
            .components()
            .any(|component| crate::is_default_ignored(component.as_os_str()))
        {
            continue;
        }
        batch.changed.push(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_metadata_sets_git_flag_without_tree_path() {
        let mut batch = WatchBatch::default();
        push_event(
            &mut batch,
            Ok(notify::Event::new(notify::EventKind::Any)
                .add_path(PathBuf::from("/repo/.git/index"))),
        );
        assert!(batch.git_changed);
        assert!(batch.changed.is_empty());

        let mut metadata = WatchBatch::default();
        push_event(
            &mut metadata,
            Ok(notify::Event::new(notify::EventKind::Modify(
                notify::event::ModifyKind::Metadata(notify::event::MetadataKind::Any),
            ))
            .add_path(PathBuf::from("/repo/.git/refs/heads/main"))),
        );
        assert!(metadata.git_changed);
        assert!(metadata.changed.is_empty());

        push_event(
            &mut batch,
            Ok(notify::Event::new(notify::EventKind::Any)
                .add_path(PathBuf::from("/repo/src/main.rs"))),
        );
        assert!(batch.git_changed);
        assert_eq!(batch.changed, [PathBuf::from("/repo/src/main.rs")]);
    }
}
