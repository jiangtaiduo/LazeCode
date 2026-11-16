use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

/// Debounced batch of filesystem changes for a project.
#[derive(Clone, Debug, Default)]
pub struct WatchBatch {
    pub changed: Vec<PathBuf>,
    /// A change under the project's top-level `.git` that must refresh the
    /// source-control snapshot (nested `.git` dirs do not count).
    pub git_changed: bool,
    /// Events under default-ignored dirs never enter `changed`, but they
    /// invalidate the ignored-file cache.
    pub ignored_touched: bool,
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
            batch.git_changed |= next.git_changed;
            batch.ignored_touched |= next.ignored_touched;
        }
        if batch.changed.is_empty() && !batch.git_changed && !batch.ignored_touched {
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
    let batch_root = root.to_path_buf();
    std::thread::Builder::new()
        .name("lazecode-fs-watch".into())
        .spawn(move || {
            while let Ok(first) = raw_rx.recv() {
                let mut batch = WatchBatch::default();
                push_event(&mut batch, first, &batch_root);
                let deadline = std::time::Instant::now() + Duration::from_millis(200);
                loop {
                    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                    if remaining.is_zero() {
                        break;
                    }
                    match raw_rx.recv_timeout(remaining) {
                        Ok(event) => push_event(&mut batch, event, &batch_root),
                        Err(mpsc::RecvTimeoutError::Timeout) => break,
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
                batch.changed.sort();
                batch.changed.dedup();
                if (!batch.changed.is_empty() || batch.git_changed || batch.ignored_touched)
                    && tx.send(batch).is_err()
                {
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

fn push_event(batch: &mut WatchBatch, event: notify::Result<notify::Event>, root: &Path) {
    let Ok(event) = event else { return };
    use notify::EventKind;
    // Git metadata events can arrive as metadata or coalesced events, so they
    // are detected before dropping. Only the watched root's own `.git` counts.
    if event.paths.iter().any(|path| is_top_level_git(root, path)) {
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
    for path in event.paths {
        if is_top_level_git(root, &path) {
            batch.git_changed = true;
            continue;
        }
        if path
            .components()
            .any(|component| crate::is_default_ignored(component.as_os_str()))
        {
            batch.ignored_touched = true;
            continue;
        }
        batch.changed.push(path);
    }
}

/// Whether `path` points inside the watched project's own `.git`. Paths
/// outside `root` keep the old any-component rule (conservative: a stray
/// refresh is cheaper than a missed one).
fn is_top_level_git(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root)
        .map(|rel| {
            rel.components()
                .next()
                .is_some_and(|component| component.as_os_str() == ".git")
        })
        .unwrap_or_else(|_| {
            path.components()
                .any(|component| component.as_os_str() == ".git")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_metadata_sets_git_flag_without_tree_path() {
        let root = PathBuf::from("/repo");
        let mut batch = WatchBatch::default();
        push_event(
            &mut batch,
            Ok(notify::Event::new(notify::EventKind::Any)
                .add_path(PathBuf::from("/repo/.git/index"))),
            &root,
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
            &root,
        );
        assert!(metadata.git_changed);
        assert!(metadata.changed.is_empty());

        push_event(
            &mut batch,
            Ok(notify::Event::new(notify::EventKind::Any)
                .add_path(PathBuf::from("/repo/src/main.rs"))),
            &root,
        );
        assert!(batch.git_changed);
        assert_eq!(batch.changed, [PathBuf::from("/repo/src/main.rs")]);
    }

    #[test]
    fn nested_git_repo_does_not_flag_the_outer_repo() {
        let root = PathBuf::from("/repo");
        let mut batch = WatchBatch::default();
        push_event(
            &mut batch,
            Ok(notify::Event::new(notify::EventKind::Any)
                .add_path(PathBuf::from("/repo/vendor/lib/.git/HEAD"))),
            &root,
        );
        assert!(!batch.git_changed, "a vendored nested .git is not ours");
        assert!(batch.changed.is_empty());
    }

    #[test]
    fn ignored_directory_events_only_invalidate_the_ignored_cache() {
        let root = PathBuf::from("/repo");
        let mut batch = WatchBatch::default();
        push_event(
            &mut batch,
            Ok(notify::Event::new(notify::EventKind::Any)
                .add_path(PathBuf::from("/repo/node_modules/pkg/index.js"))),
            &root,
        );
        assert!(batch.ignored_touched);
        assert!(batch.changed.is_empty());
        assert!(!batch.git_changed);
    }
}
