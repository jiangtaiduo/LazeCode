use crossbeam_channel::{Receiver, TrySendError};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Debounced batch of filesystem changes for a project.
#[derive(Clone, Debug, Default)]
pub struct WatchBatch {
    pub changed: Vec<PathBuf>,
    /// A change under the project's top-level `.git` that must refresh the
    /// source-control snapshot (nested `.git` dirs do not count).
    pub git_changed: bool,
    /// Events under ignored dirs never enter `changed`, but a structural
    /// change there invalidates the ignored-file cache.
    pub ignored_touched: bool,
    /// Create/remove/rename (or an unknown event) that can change tree shape.
    /// Content-only edits leave this false so the tree is not rescanned.
    pub structure_changed: bool,
}

/// Owns a recursive watcher for one project. Drop to stop watching.
pub struct ProjectWatcher {
    _watcher: notify::RecommendedWatcher,
    rx: Arc<Receiver<WatchBatch>>,
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
            batch.structure_changed |= next.structure_changed;
        }
        if batch.changed.is_empty()
            && !batch.git_changed
            && !batch.ignored_touched
            && !batch.structure_changed
        {
            None
        } else {
            batch.changed.sort();
            batch.changed.dedup();
            Some(batch)
        }
    }
}

fn merge_batches(mut first: WatchBatch, second: WatchBatch) -> WatchBatch {
    first.changed.extend(second.changed);
    first.git_changed |= second.git_changed;
    first.ignored_touched |= second.ignored_touched;
    first.structure_changed |= second.structure_changed;
    first.changed.sort();
    first.changed.dedup();
    first
}

/// Watch `root` recursively on a background thread and coalesce events into
/// roughly 200 ms batches. `ignored_patterns` is the project's effective ignore
/// list; matching paths never enter `changed`. Queues are bounded; bursts are
/// merged instead of growing without limit.
pub fn watch_project(root: &Path, ignored_patterns: &[String]) -> anyhow::Result<ProjectWatcher> {
    use notify::{RecursiveMode, Watcher};

    const RAW_CAP: usize = 1024;
    const BATCH_CAP: usize = 256;

    let (raw_tx, raw_rx) = crossbeam_channel::bounded::<notify::Result<notify::Event>>(RAW_CAP);
    let overflow = Arc::new(AtomicBool::new(false));
    let overflow_writer = overflow.clone();
    let mut watcher = notify::RecommendedWatcher::new(
        move |event| {
            if raw_tx.try_send(event).is_err() {
                // Events are coalesced by the batching thread; record one
                // structural refresh instead of losing the burst silently.
                overflow_writer.store(true, Ordering::Release);
            }
        },
        notify::Config::default().with_poll_interval(Duration::from_millis(500)),
    )?;
    watcher.watch(root, RecursiveMode::Recursive)?;

    let (tx, rx) = crossbeam_channel::bounded::<WatchBatch>(BATCH_CAP);
    let batch_root = root.to_path_buf();
    let ignored_patterns = ignored_patterns.to_vec();
    let batch_overflow = overflow.clone();
    let batch_rx = rx.clone();
    std::thread::Builder::new()
        .name("lazecode-fs-watch".into())
        .spawn(move || {
            while let Ok(first) = raw_rx.recv() {
                let mut batch = WatchBatch::default();
                if batch_overflow.swap(false, Ordering::Acquire) {
                    batch.structure_changed = true;
                    batch.ignored_touched = true;
                }
                push_event(&mut batch, first, &batch_root, &ignored_patterns);
                let deadline = std::time::Instant::now() + Duration::from_millis(200);
                loop {
                    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                    if remaining.is_zero() {
                        break;
                    }
                    match raw_rx.recv_timeout(remaining) {
                        Ok(event) => push_event(&mut batch, event, &batch_root, &ignored_patterns),
                        Err(crossbeam_channel::RecvTimeoutError::Timeout) => break,
                        Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
                    }
                }
                batch.changed.sort();
                batch.changed.dedup();
                if batch.changed.is_empty()
                    && !batch.git_changed
                    && !batch.ignored_touched
                    && !batch.structure_changed
                {
                    continue;
                }
                // If the UI has stopped draining, merge the newest batch into
                // the oldest queued one so the queue stays bounded and no
                // structural change is dropped.
                loop {
                    match tx.try_send(batch) {
                        Ok(()) => break,
                        Err(TrySendError::Full(returned)) => {
                            batch = match batch_rx.try_recv() {
                                Ok(old) => merge_batches(old, returned),
                                Err(_) => returned,
                            };
                        }
                        Err(TrySendError::Disconnected(_)) => return,
                    }
                }
            }
        })?;

    Ok(ProjectWatcher {
        _watcher: watcher,
        rx: Arc::new(rx),
        root: root.to_path_buf(),
    })
}

fn push_event(
    batch: &mut WatchBatch,
    event: notify::Result<notify::Event>,
    root: &Path,
    ignored_patterns: &[String],
) {
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

    // Atomic saves are a temp-file create + rename. They replace file content
    // but not the tree, so do not force a full tree rescan for them.
    let editor_temp = event.paths.iter().any(|path| is_editor_temp(path));
    let structural = !editor_temp
        && matches!(
            event.kind,
            EventKind::Any
                | EventKind::Create(_)
                | EventKind::Remove(_)
                | EventKind::Modify(notify::event::ModifyKind::Name(_))
        );
    for path in event.paths {
        if is_top_level_git(root, &path) {
            batch.git_changed = true;
            continue;
        }
        if is_editor_temp(&path) {
            continue;
        }
        if is_watch_ignored(root, &path, ignored_patterns) {
            if structural {
                batch.ignored_touched = true;
            }
            continue;
        }
        if structural {
            batch.structure_changed = true;
        }
        batch.changed.push(path);
    }
}

/// Ignored build output and the project's configured ignore list. An empty
/// list falls back to the built-in defaults, matching the tree scanner.
fn is_watch_ignored(root: &Path, path: &Path, ignored_patterns: &[String]) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let configured = (!ignored_patterns.is_empty()).then_some(ignored_patterns);
    relative.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        matches!(&*name, "target" | "dist") || crate::is_ignored_pattern(&name, true, configured)
    })
}

/// `TextFile::save` writes `.<name>.laze-<pid>-<n>-<nanos>.tmp` next to the
/// target. These paths are implementation detail, never project files.
fn is_editor_temp(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.starts_with('.') && name.contains(".laze-") && name.ends_with(".tmp")
        })
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
    use notify::event::{CreateKind, DataChange, ModifyKind, RenameMode};

    fn push(
        batch: &mut WatchBatch,
        kind: notify::EventKind,
        paths: &[&str],
        root: &Path,
        ignored_patterns: &[String],
    ) {
        let mut event = notify::Event::new(kind);
        for path in paths {
            event = event.add_path(PathBuf::from(path));
        }
        push_event(batch, Ok(event), root, ignored_patterns);
    }

    #[test]
    fn git_metadata_sets_git_flag_without_tree_path() {
        let root = PathBuf::from("/repo");
        let mut batch = WatchBatch::default();
        push(
            &mut batch,
            notify::EventKind::Any,
            &["/repo/.git/index"],
            &root,
            &[],
        );
        assert!(batch.git_changed);
        assert!(batch.changed.is_empty());

        let mut metadata = WatchBatch::default();
        push(
            &mut metadata,
            notify::EventKind::Modify(ModifyKind::Metadata(notify::event::MetadataKind::Any)),
            &["/repo/.git/refs/heads/main"],
            &root,
            &[],
        );
        assert!(metadata.git_changed);
        assert!(metadata.changed.is_empty());

        push(
            &mut batch,
            notify::EventKind::Any,
            &["/repo/src/main.rs"],
            &root,
            &[],
        );
        assert!(batch.git_changed);
        assert_eq!(batch.changed, [PathBuf::from("/repo/src/main.rs")]);
        assert!(batch.structure_changed, "unknown events are conservative");
    }

    #[test]
    fn nested_git_repo_does_not_flag_the_outer_repo() {
        let root = PathBuf::from("/repo");
        let mut batch = WatchBatch::default();
        push(
            &mut batch,
            notify::EventKind::Any,
            &["/repo/vendor/lib/.git/HEAD"],
            &root,
            &[],
        );
        assert!(!batch.git_changed, "a vendored nested .git is not ours");
        assert!(batch.changed.is_empty());
    }

    #[test]
    fn ignored_directory_events_only_invalidate_the_ignored_cache() {
        let root = PathBuf::from("/repo");
        let mut batch = WatchBatch::default();
        push(
            &mut batch,
            notify::EventKind::Any,
            &["/repo/node_modules/pkg/index.js"],
            &root,
            &[],
        );
        assert!(batch.ignored_touched);
        assert!(batch.changed.is_empty());
        assert!(!batch.git_changed);
    }

    #[test]
    fn configured_and_build_ignores_stay_out_of_changed() {
        let root = PathBuf::from("/repo");
        let patterns = vec!["build".to_string(), "*.log".to_string()];
        let mut batch = WatchBatch::default();
        for path in [
            "/repo/target/debug/app",
            "/repo/dist/out",
            "/repo/build/cache",
            "/repo/logs/app.log",
        ] {
            push(
                &mut batch,
                notify::EventKind::Create(CreateKind::File),
                &[path],
                &root,
                &patterns,
            );
        }
        assert!(batch.changed.is_empty());
        assert!(batch.ignored_touched);
        assert!(!batch.structure_changed);

        push(
            &mut batch,
            notify::EventKind::Modify(ModifyKind::Data(DataChange::Any)),
            &["/repo/src/lib.rs"],
            &root,
            &patterns,
        );
        assert_eq!(batch.changed, [PathBuf::from("/repo/src/lib.rs")]);
        assert!(
            !batch.structure_changed,
            "content edits do not change shape"
        );

        // The checkout itself may be named like an ignored directory; only
        // path components below the root participate in matching.
        let mut checkout_named_target = WatchBatch::default();
        push(
            &mut checkout_named_target,
            notify::EventKind::Create(CreateKind::File),
            &["/repo/target/src/keep.rs"],
            &PathBuf::from("/repo/target"),
            &patterns,
        );
        assert_eq!(
            checkout_named_target.changed,
            [PathBuf::from("/repo/target/src/keep.rs")]
        );
        assert!(checkout_named_target.structure_changed);
    }

    #[test]
    fn atomic_save_temp_is_not_a_tree_change() {
        let root = PathBuf::from("/repo");
        let mut batch = WatchBatch::default();
        push(
            &mut batch,
            notify::EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
            &["/repo/src/.lib.rs.laze-42-0-123.tmp", "/repo/src/lib.rs"],
            &root,
            &[],
        );
        assert_eq!(batch.changed, [PathBuf::from("/repo/src/lib.rs")]);
        assert!(!batch.structure_changed);
    }

    #[test]
    fn batch_merge_preserves_flags_and_deduplicates_paths() {
        let first = WatchBatch {
            changed: vec![PathBuf::from("/repo/a"), PathBuf::from("/repo/b")],
            git_changed: false,
            ignored_touched: true,
            structure_changed: false,
        };
        let second = WatchBatch {
            changed: vec![PathBuf::from("/repo/b"), PathBuf::from("/repo/c")],
            git_changed: true,
            ignored_touched: false,
            structure_changed: true,
        };
        let merged = merge_batches(first, second);
        assert_eq!(
            merged.changed,
            [
                PathBuf::from("/repo/a"),
                PathBuf::from("/repo/b"),
                PathBuf::from("/repo/c")
            ]
        );
        assert!(merged.git_changed && merged.ignored_touched && merged.structure_changed);
    }
}
