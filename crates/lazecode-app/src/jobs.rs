use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};
use lazecode_core::{
    AppModel, Command, DiffSource, DiffViewMode, ProjectState, Settings, TabKind, ToastKind,
    WorktreeState,
};
use lazecode_fs::SearchHit;
use lazecode_git::{BranchInfo, CommitDetails, CommitInfo, FileDiff, LogOptions, RepoSnapshot};
use lazecode_terminal::Notifier;
use lazecode_ui::UiRuntime;

/// Mixed file/folder picker for "Open": NSOpenPanel is the only chooser that
/// returns either a directory or several files.
#[cfg(target_os = "macos")]
fn pick_open_paths() -> Vec<PathBuf> {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2_foundation::NSString;

    unsafe {
        let panel: *mut AnyObject = msg_send![objc2::class!(NSOpenPanel), openPanel];
        if panel.is_null() {
            return Vec::new();
        }
        let _: () = msg_send![panel, setCanChooseFiles: true];
        let _: () = msg_send![panel, setCanChooseDirectories: true];
        let _: () = msg_send![panel, setAllowsMultipleSelection: true];
        let title = NSString::from_str("Open");
        let _: () = msg_send![panel, setTitle: &*title];
        // NSModalResponseOK == 1.
        let response: isize = msg_send![panel, runModal];
        if response != 1 {
            return Vec::new();
        }
        let urls: *mut AnyObject = msg_send![panel, URLs];
        if urls.is_null() {
            return Vec::new();
        }
        let count: usize = msg_send![urls, count];
        let mut paths = Vec::with_capacity(count);
        for index in 0..count {
            let url: *mut AnyObject = msg_send![urls, objectAtIndex: index];
            if url.is_null() {
                continue;
            }
            let path: *mut AnyObject = msg_send![url, path];
            if path.is_null() {
                continue;
            }
            let utf8: *const std::ffi::c_char = msg_send![path, UTF8String];
            if !utf8.is_null() {
                paths.push(PathBuf::from(
                    std::ffi::CStr::from_ptr(utf8)
                        .to_string_lossy()
                        .into_owned(),
                ));
            }
        }
        paths
    }
}

#[cfg(not(target_os = "macos"))]
fn pick_open_paths() -> Vec<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Open")
        .pick_files()
        .unwrap_or_default()
}

mod apply;
mod execute;
mod refresh;

/// Commit count loaded per log page. Small enough for a snappy first frame,
/// big enough that a repo rarely needs more than a couple of pages.
const LOG_PAGE_SIZE: usize = 300;

/// Build `gix` options for one page of a filtered log; `offset` re-walks from
/// the same tips so layout stays prefix-stable.
fn log_options_from_query(
    query: &lazecode_core::LogQuery,
    job_root: &std::path::Path,
    limit: usize,
    offset: usize,
) -> LogOptions {
    let head_ancestors = if query.all_refs || !query.branch.trim().is_empty() {
        // Only meaningful when history carries non-HEAD commits; cheap walk.
        lazecode_git::head_ancestors(job_root)
    } else {
        None
    };
    let since = match query.date {
        lazecode_core::DatePreset::All => (query.since > 0).then_some(query.since),
        lazecode_core::DatePreset::Today => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            Some(now - (now % 86400))
        }
        lazecode_core::DatePreset::Week => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            Some(now.saturating_sub(7 * 86400))
        }
        lazecode_core::DatePreset::Month => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            Some(now.saturating_sub(30 * 86400))
        }
    };
    let has_branch = !query.branch.trim().is_empty();
    LogOptions {
        limit: limit + offset,
        first_parent: query.first_parent,
        no_merges: query.no_merges,
        topo_order: query.topo_order,
        author: (!query.author.trim().is_empty()).then(|| query.author.clone()),
        since,
        path: (!query.path.trim().is_empty()).then(|| query.path.clone()),
        all_refs: query.all_refs && !has_branch,
        branch: has_branch.then(|| query.branch.trim().to_string()),
        head_ancestors,
        range_walk: false,
    }
}

/// Stable key so unrelated filter edits don't clobber the in-flight dedup.
fn log_query_fingerprint(query: &lazecode_core::LogQuery) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}",
        query.branch,
        query.author,
        query.path,
        query.date as u8,
        query.since,
        query.all_refs,
        query.first_parent,
        query.no_merges,
        query.topo_order,
    )
}

/// Compact payload fingerprint for job keys. Mutating commands with different
/// payloads must not be silently deduplicated by `spawn`.
fn payload_hash<T: std::hash::Hash>(value: &T) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(value, &mut hasher);
    std::hash::Hasher::finish(&hasher)
}

pub enum JobResult {
    ProjectLoaded {
        path: PathBuf,
        result: Result<Box<ProjectState>, String>,
    },
    TreeLoaded {
        root: PathBuf,
        tree: lazecode_fs::tree::FileTree,
    },
    GitRefreshed {
        root: PathBuf,
        result: Result<RepoSnapshot, String>,
    },
    CmpSessionLoaded {
        root: PathBuf,
        from: String,
        to: String,
        local: bool,
        result: Result<Vec<lazecode_git::CompareFile>, String>,
    },
    LogLoaded {
        root: PathBuf,
        branch: Option<String>,
        query: lazecode_core::LogQuery,
        /// Page offset this job loaded (0 = fresh full query).
        offset: usize,
        result: Result<(Vec<CommitInfo>, Vec<Option<String>>), String>,
    },
    BranchesLoaded {
        root: PathBuf,
        result: Vec<BranchInfo>,
        tags: Vec<String>,
    },
    CommitDetails {
        root: PathBuf,
        id: String,
        result: Result<Box<CommitDetails>, String>,
    },
    DiffLoaded {
        root: PathBuf,
        source: DiffSource,
        result: Result<FileDiff, String>,
    },
    SearchDone {
        root: PathBuf,
        request_id: u64,
        query: String,
        result: Result<Vec<SearchHit>, String>,
    },
    OpDone {
        root: PathBuf,
        message: String,
        ok: bool,
        refresh: bool,
        reload_files: bool,
        clear_commit_message: bool,
    },
    /// A safe `git branch -d` was rejected because the branch is not merged;
    /// the UI should show the explicit force-delete confirmation.
    BranchDeleteNeedsForce { root: PathBuf, name: String },
    /// 文件更改已回滚：除常规刷新外，还要关闭已不存在文件的编辑器标签
    /// （对齐稳定版：新增/未跟踪文件回滚后关闭标签页）。
    Discarded {
        root: PathBuf,
        paths: Vec<String>,
        message: String,
    },
    /// 分支比较整单回填（按 root+refs+paths 匹配 UiState 缓存；root 防
    /// 跨项目同名分支错配）。
    CompareBranchesLoaded {
        root: PathBuf,
        left: String,
        right: String,
        left_path: String,
        right_path: String,
        result: Result<lazecode_git::CompareBranches, String>,
    },
    /// 某一侧「Load More」的提交列表回填。
    CompareBranchesCommits {
        root: PathBuf,
        side: u8,
        exclude: String,
        include: String,
        result: Result<(Vec<lazecode_git::CommitInfo>, bool), String>,
    },
    /// 详情列文件清单回填（root+key 匹配）。
    CompareFilesLoaded {
        root: PathBuf,
        key: String,
        result: Result<Vec<lazecode_git::CompareFile>, String>,
    },
    Saved {
        root: PathBuf,
        path: PathBuf,
        revision: u64,
        result: Result<(), String>,
    },
    WorktreeAdded {
        project_root: PathBuf,
        name: String,
        result: Result<PathBuf, String>,
    },
    WorktreeRemoved {
        project_root: PathBuf,
        name: String,
        result: Result<(), String>,
    },
}

pub struct JobCompletion {
    pub key: String,
    pub result: JobResult,
}

pub struct JobQueue {
    pub rx: Receiver<JobCompletion>,
    tx: Sender<JobCompletion>,
    notifier: Arc<dyn Notifier>,
    in_flight: HashSet<String>,
    pending_git: HashSet<PathBuf>,
    /// Worktrees whose reload was requested mid-scan; re-queued when the running
    /// scan completes so watcher bursts coalesce.
    pending_trees: HashSet<PathBuf>,
    /// Log/branch refreshes requested while the same query was in flight.
    /// Dropping these would leave the panel showing pre-op data.
    pending_log: HashSet<(PathBuf, usize)>,
    pending_branches: HashSet<PathBuf>,
    /// Diff/compare refreshes requested while the same key was in flight.
    /// Without these, an operation that lands mid-load can leave the old
    /// pre-operation content painted forever.
    pending_diffs: HashSet<String>,
    pending_compare_local: HashSet<String>,
    /// Cancel flags for the latest find-in-files per worktree, so rapid typing
    /// cannot leave a full-tree scan per keystroke running.
    search_cancels: HashMap<PathBuf, (u64, Arc<AtomicBool>)>,
}

impl JobQueue {
    pub fn new(notifier: Arc<dyn Notifier>) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        Self {
            rx,
            tx,
            notifier,
            in_flight: HashSet::new(),
            pending_git: HashSet::new(),
            pending_trees: HashSet::new(),
            pending_log: HashSet::new(),
            pending_branches: HashSet::new(),
            pending_diffs: HashSet::new(),
            pending_compare_local: HashSet::new(),
            search_cancels: HashMap::new(),
        }
    }

    fn spawn(
        &mut self,
        name: &'static str,
        key: String,
        job: impl FnOnce() -> JobResult + Send + 'static,
    ) -> bool {
        if !self.in_flight.insert(key.clone()) {
            log::debug!("dropping duplicate {name} job for {key}");
            return false;
        }
        let tx = self.tx.clone();
        let notifier = self.notifier.clone();
        let job_key = key.clone();
        let thread = std::thread::Builder::new()
            .name(format!("lazecode-{name}"))
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
                    .unwrap_or_else(|_| JobResult::OpDone {
                        root: PathBuf::new(),
                        message: "background task panicked".to_string(),
                        ok: false,
                        refresh: false,
                        reload_files: false,
                        clear_commit_message: false,
                    });
                let _ = tx.send(JobCompletion {
                    key: job_key,
                    result,
                });
                notifier.notify();
            });
        if let Err(err) = thread {
            self.in_flight.remove(&key);
            log::error!("cannot spawn {name} job: {err}");
            false
        } else {
            true
        }
    }
}

struct SaveData {
    text: String,
    encoding: Option<&'static encoding_rs::Encoding>,
    eol: lazecode_fs::Eol,
    bom: bool,
}

/// Move only the selected local changes to `target` using a temporary shelf.
/// Refuses to proceed if other local changes exist, so the target checkout
/// cannot silently carry them across; restores the selected changes on the
/// original branch if checkout fails.
fn cherry_pick_selected_changes(
    root: &std::path::Path,
    paths: &[String],
    target: &str,
) -> anyhow::Result<String> {
    let target = target.trim();
    if target.is_empty() {
        anyhow::bail!("target branch is empty");
    }
    if paths.is_empty() {
        anyhow::bail!("no changes selected");
    }
    let snapshot = lazecode_git::snapshot(root)?;
    let Some(current) = snapshot.branch.clone() else {
        anyhow::bail!("cannot cherry-pick while HEAD is detached");
    };
    if current == target {
        anyhow::bail!("already on branch '{target}'");
    }
    let selected: std::collections::BTreeSet<&str> = paths.iter().map(String::as_str).collect();
    let changed: std::collections::BTreeSet<&str> = snapshot
        .changes
        .iter()
        .map(|change| change.path.as_str())
        .collect();
    if changed.is_empty() {
        anyhow::bail!("no local changes to cherry-pick");
    }
    if let Some(path) = changed.iter().find(|path| !selected.contains(**path)) {
        anyhow::bail!(
            "unselected local change would be carried to '{target}': {path}; \
             commit, stash, or include it first"
        );
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let temp = format!("lazecode-cherry-pick-{stamp}");
    let shelf_exists = |name: &str| {
        lazecode_git::shelf_list(root)
            .map(|shelves| shelves.iter().any(|shelf| shelf.name == name))
            .unwrap_or(false)
    };
    if shelf_exists(&temp) {
        anyhow::bail!("temporary shelf '{temp}' already exists; nothing was moved");
    }
    // Shelve first: this reverts exactly the selected paths, leaving a clean
    // worktree for the target checkout.
    if let Err(err) = lazecode_git::shelf_save_paths(root, &temp, paths) {
        if !shelf_exists(&temp) {
            return Err(anyhow::anyhow!(
                "could not prepare selected changes: {err}; nothing was moved"
            ));
        }
        // `shelf_save_paths` writes the shelf before retracting the paths. If
        // a retraction failed midway, the shelf is the only complete copy of
        // the selection; try to put it back instead of claiming nothing moved.
        return Err(match shelf_restore_scoped(root, &temp, None, None, true) {
            Ok(()) => anyhow::anyhow!(
                "could not prepare selected changes: {err}; changes restored on '{current}'"
            ),
            Err(restore_err) => anyhow::anyhow!(
                "could not prepare selected changes: {err}; also failed to restore them: \
                 {restore_err} (shelf '{temp}' was kept)"
            ),
        });
    }
    if let Err(err) = lazecode_git::checkout_branch(root, target) {
        let restore = shelf_restore_scoped(root, &temp, None, None, true);
        return Err(match restore {
            Ok(()) => {
                anyhow::anyhow!(
                    "cannot check out '{target}': {err}; changes restored on '{current}'"
                )
            }
            Err(restore_err) => anyhow::anyhow!(
                "cannot check out '{target}': {err}; also failed to restore changes on \
                 '{current}': {restore_err} (shelf '{temp}' was kept)"
            ),
        });
    }
    match shelf_restore_scoped(root, &temp, None, None, true) {
        Ok(()) => Ok(format!(
            "Moved selected changes from '{current}' onto '{target}' (uncommitted)"
        )),
        Err(err) => Err(anyhow::anyhow!(
            "changes were moved to '{target}' but applying them did not complete cleanly: \
             {err}; inspect the worktree and shelf '{temp}'"
        )),
    }
}

/// Restore a shelf (all or `paths` only) into `target_list` (created if
/// absent); `delete_after` removes the shelf entry on success.
fn shelf_restore_scoped(
    root: &std::path::Path,
    name: &str,
    paths: Option<&[String]>,
    target_list: Option<&str>,
    delete_after: bool,
) -> anyhow::Result<()> {
    match paths {
        Some(selected) => lazecode_git::shelf_restore_partial(root, name, selected)?,
        None => lazecode_git::shelf_restore(root, name, false)?,
    }
    if let Some(list) = target_list {
        let mut lists = lazecode_git::Changelists::open(root);
        if !lists.lists().iter().any(|existing| existing == list) {
            lists.create_list(list)?;
        }
        let assigned = match paths {
            Some(selected) => selected.to_vec(),
            None => lazecode_git::shelf_files(root, name)?,
        };
        lists.assign_many(&assigned, list)?;
    }
    if delete_after {
        lazecode_git::shelf_delete(root, name)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazecode_core::UiState;

    struct NoopNotifier;

    impl Notifier for NoopNotifier {
        fn notify(&self) {}
    }

    fn empty_model() -> AppModel {
        AppModel {
            settings: Settings::default(),
            projects: Vec::new(),
            active_project: None,
            ui: UiState::default(),
            toasts: Vec::new(),
            recent: Vec::new(),
            recent_files: Vec::new(),
            commands: Vec::new(),
            notifier: None,
            lsp: lazecode_core::LspManager::new(true, None),
            should_quit: false,
            version: "test",
            favorite_branches: HashSet::new(),
            git_cmd_log: Vec::new(),
        }
    }

    fn run_and_apply(queue: &mut JobQueue, completion: JobCompletion) {
        let mut model = empty_model();
        let mut runtime = UiRuntime::default();
        queue.apply(&mut model, &mut runtime, completion);
    }

    #[test]
    fn completed_save_and_op_jobs_release_their_keys() {
        let mut queue = JobQueue::new(Arc::new(NoopNotifier));

        queue.spawn("test-save", "save:/tmp/a".into(), || JobResult::Saved {
            root: PathBuf::new(),
            path: PathBuf::from("/tmp/a"),
            revision: 0,
            result: Ok(()),
        });
        let completion = queue
            .rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        run_and_apply(&mut queue, completion);
        assert!(!queue.in_flight.contains("save:/tmp/a"));

        queue.spawn("test-op", "commit:/repo".into(), || JobResult::OpDone {
            root: PathBuf::new(),
            message: "ok".into(),
            ok: true,
            refresh: false,
            reload_files: false,
            clear_commit_message: false,
        });
        let completion = queue
            .rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        run_and_apply(&mut queue, completion);
        assert!(!queue.in_flight.contains("commit:/repo"));
    }

    #[test]
    fn panicking_job_releases_its_key() {
        let mut queue = JobQueue::new(Arc::new(NoopNotifier));
        queue.spawn("test-panic", "git:/repo".into(), || panic!("boom"));
        let completion = queue
            .rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        run_and_apply(&mut queue, completion);
        assert!(!queue.in_flight.contains("git:/repo"));
    }
    #[test]
    fn refresh_log_spawns_and_completes_job() {
        let mut queue = JobQueue::new(Arc::new(NoopNotifier));
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        queue.refresh_log(root.clone(), lazecode_core::LogQuery::default(), 0);
        let completion = queue
            .rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("refresh_log should produce completion");
        let key = completion.key.clone();
        let mut model = empty_model();
        let mut project = ProjectState::open(root, &Settings::default(), None).unwrap();
        project.log_paging = true;
        model.projects.push(project);
        model.active_project = Some(0);
        let mut runtime = UiRuntime::default();
        queue.apply(&mut model, &mut runtime, completion);
        assert!(!queue.in_flight.contains(&key));
        assert!(!model.projects[0].log.is_empty());
        assert!(!model.projects[0].log_paging);
    }
    fn temp_repo(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "lazecode-app-{name}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let remote = root.with_extension("remote.git");
        std::process::Command::new("git")
            .args([
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                remote.to_str().unwrap(),
            ])
            .status()
            .unwrap();
        (root, remote)
    }

    fn git_ok(root: &std::path::Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn commit(repo: &std::path::Path, file: &str, text: &str, message: &str) {
        std::fs::write(repo.join(file), text).unwrap();
        git_ok(repo, &["add", "."]);
        git_ok(repo, &["commit", "-q", "-m", message]);
    }

    fn open_command_model(root: &std::path::Path) -> (AppModel, UiRuntime, JobQueue) {
        let settings = Settings::default();
        let notifier: Arc<dyn Notifier> = Arc::new(NoopNotifier);
        let mut model = empty_model();
        model.settings = settings.clone();
        let project = ProjectState::open(root.to_path_buf(), &settings, Some(notifier.clone()))
            .expect("open project");
        model.projects.push(project);
        model.active_project = Some(0);
        let queue = JobQueue::new(notifier);
        (model, UiRuntime::default(), queue)
    }

    fn run_command(
        queue: &mut JobQueue,
        model: &mut AppModel,
        runtime: &mut UiRuntime,
        command: Command,
    ) {
        queue.execute(model, runtime, command);
        while !queue.in_flight.is_empty() {
            let completion = queue
                .rx
                .recv_timeout(std::time::Duration::from_secs(20))
                .expect("job completion");
            queue.apply(model, runtime, completion);
        }
    }

    #[test]
    fn fetch_all_command_updates_remote_tracking() {
        let (root, remote) = temp_repo("fetch");
        git_ok(&root, &["init", "-q", "-b", "main"]);
        git_ok(&root, &["config", "user.name", "T"]);
        git_ok(&root, &["config", "user.email", "t@e"]);
        commit(&root, "a.txt", "one\n", "one");
        git_ok(
            &root,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git_ok(&root, &["push", "-q", "-u", "origin", "main"]);

        let other = root.with_extension("other");
        git_ok(
            &root,
            &[
                "clone",
                "-q",
                remote.to_str().unwrap(),
                other.to_str().unwrap(),
            ],
        );
        git_ok(&other, &["config", "user.name", "Other"]);
        git_ok(&other, &["config", "user.email", "other@e"]);
        commit(&other, "b.txt", "two\n", "remote two");
        let remote_head = git_ok(&other, &["rev-parse", "HEAD"]);
        git_ok(&other, &["push", "-q", "origin", "main"]);

        let (mut model, mut runtime, mut queue) = open_command_model(&root);
        run_command(&mut queue, &mut model, &mut runtime, Command::FetchAll);
        assert_eq!(
            git_ok(&root, &["rev-parse", "refs/remotes/origin/main"]),
            remote_head
        );
        let toast = model
            .toasts
            .last()
            .map(|toast| toast.message.clone())
            .unwrap_or_default();
        assert!(!toast.contains("M4"), "{toast}");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&remote);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn remove_dirty_worktree_prompts_force_then_removes() {
        let (root, remote) = temp_repo("worktree-dirty");
        git_ok(&root, &["init", "-q", "-b", "main"]);
        git_ok(&root, &["config", "user.name", "T"]);
        git_ok(&root, &["config", "user.email", "t@e"]);
        commit(&root, "a.txt", "one\n", "one");
        git_ok(&root, &["branch", "side"]);
        let worktree_path = root.with_extension("wt-dirty");
        lazecode_git::add_worktree(&root, &worktree_path, "side").unwrap();
        std::fs::write(worktree_path.join("a.txt"), "uncommitted\n").unwrap();

        let (mut model, mut runtime, mut queue) = open_command_model(&root);
        let worktree_name = lazecode_git::list_worktrees(&root)
            .unwrap()
            .into_iter()
            .find(|wt| wt.name != "main")
            .map(|wt| wt.name)
            .unwrap();
        let ignored_patterns = model.settings.ignored_patterns.clone();
        model.projects[0]
            .ensure_worktree(&worktree_name, &ignored_patterns)
            .unwrap();
        assert_eq!(model.projects[0].worktrees.len(), 2);

        run_command(
            &mut queue,
            &mut model,
            &mut runtime,
            Command::RemoveWorktree {
                name: worktree_name.clone(),
                force: false,
            },
        );
        assert!(worktree_path.exists());
        assert!(matches!(
            runtime.dialog,
            lazecode_ui::runtime::DialogState::RemoveWorktree {
                dirty: true,
                changes,
                ..
            } if changes > 0
        ));

        run_command(
            &mut queue,
            &mut model,
            &mut runtime,
            Command::RemoveWorktree {
                name: worktree_name,
                force: true,
            },
        );
        assert!(!worktree_path.exists());
        assert_eq!(model.projects[0].worktrees.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&remote);
    }

    #[test]
    fn create_worktrees_with_distinct_branches_and_reject_reuse() {
        let (root, remote) = temp_repo("worktree-create");
        git_ok(&root, &["init", "-q", "-b", "main"]);
        git_ok(&root, &["config", "user.name", "T"]);
        git_ok(&root, &["config", "user.email", "t@e"]);
        commit(&root, "a.txt", "one\n", "one");

        let (mut model, mut runtime, mut queue) = open_command_model(&root);
        for name in ["wt-a", "wt-b"] {
            run_command(
                &mut queue,
                &mut model,
                &mut runtime,
                Command::CreateWorktree {
                    name: name.into(),
                    branch: format!("feature-{name}"),
                    location: root.join(".worktree").join(name),
                },
            );
            assert!(
                root.join(".worktree").join(name).is_dir(),
                "{name} checkout missing"
            );
        }
        assert_eq!(model.projects[0].worktrees.len(), 3);
        assert_eq!(model.projects[0].branch(), "feature-wt-b");
        assert_eq!(
            model.projects[0].active_root(),
            root.join(".worktree")
                .join("wt-b")
                .canonicalize()
                .unwrap()
                .as_path()
        );

        run_command(
            &mut queue,
            &mut model,
            &mut runtime,
            Command::CreateWorktree {
                name: "wt-reuse".into(),
                branch: "feature-wt-a".into(),
                location: root.join(".worktree").join("wt-reuse"),
            },
        );
        assert_eq!(model.projects[0].worktrees.len(), 3);
        assert_eq!(lazecode_git::list_worktrees(&root).unwrap().len(), 3);
        assert!(!root.join(".worktree").join("wt-reuse").exists());
        assert!(
            model
                .toasts
                .last()
                .is_some_and(|toast| matches!(toast.kind, ToastKind::Error)),
            "reusing a checked-out branch must report an error"
        );

        run_command(
            &mut queue,
            &mut model,
            &mut runtime,
            Command::RemoveWorktree {
                name: "main".into(),
                force: false,
            },
        );
        assert_eq!(model.projects[0].worktrees.len(), 3);
        assert!(root.join("a.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&remote);
    }

    #[test]
    fn switch_worktree_updates_branch_and_changes() {
        let (root, remote) = temp_repo("worktree-switch");
        git_ok(&root, &["init", "-q", "-b", "main"]);
        git_ok(&root, &["config", "user.name", "T"]);
        git_ok(&root, &["config", "user.email", "t@e"]);
        commit(&root, "a.txt", "one\n", "one");
        git_ok(&root, &["branch", "side"]);
        let linked = root.with_extension("side-worktree");
        lazecode_git::add_worktree(&root, &linked, "side").unwrap();
        std::fs::write(linked.join("side-only.txt"), "side change\n").unwrap();
        let side_name = lazecode_git::list_worktrees(&root)
            .unwrap()
            .into_iter()
            .find(|worktree| worktree.name != "main")
            .map(|worktree| worktree.name)
            .unwrap();

        let (mut model, mut runtime, mut queue) = open_command_model(&root);
        let settings = model.settings.clone();
        let main_tab = model.projects[0]
            .open_file(root.join("a.txt"), &settings)
            .unwrap();
        run_command(
            &mut queue,
            &mut model,
            &mut runtime,
            Command::SwitchWorktree(side_name.clone()),
        );
        assert_eq!(model.projects[0].branch(), "side");
        assert_eq!(
            model.projects[0].active_root(),
            linked.canonicalize().unwrap().as_path()
        );
        assert!(model.projects[0]
            .changes()
            .iter()
            .any(|change| change.path == "side-only.txt"));
        assert_eq!(model.projects[0].active_worktree_tabs().count(), 0);
        assert_ne!(model.projects[0].active_tab, Some(main_tab));

        // A file created while the worktree is attached but not focused must
        // appear after switching back.
        std::fs::write(linked.join("side-later.txt"), "later\n").unwrap();
        run_command(
            &mut queue,
            &mut model,
            &mut runtime,
            Command::SwitchWorktree("main".into()),
        );
        assert_eq!(model.projects[0].active_tab, Some(main_tab));
        run_command(
            &mut queue,
            &mut model,
            &mut runtime,
            Command::SwitchWorktree(side_name),
        );
        assert!(model.projects[0]
            .active_tree()
            .nodes
            .iter()
            .any(|node| node.name == "side-later.txt"));

        run_command(
            &mut queue,
            &mut model,
            &mut runtime,
            Command::SwitchWorktree("main".into()),
        );
        assert_eq!(model.projects[0].branch(), "main");
        assert!(!model.projects[0]
            .changes()
            .iter()
            .any(|change| change.path == "side-only.txt"));

        let _ = std::fs::remove_dir_all(&linked);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&remote);
    }

    #[test]
    fn delete_branch_command_requires_force_and_then_succeeds() {
        let (root, remote) = temp_repo("delete");
        git_ok(&root, &["init", "-q", "-b", "main"]);
        git_ok(&root, &["config", "user.name", "T"]);
        git_ok(&root, &["config", "user.email", "t@e"]);
        commit(&root, "a.txt", "one\n", "one");
        git_ok(&root, &["checkout", "-q", "-b", "side"]);
        commit(&root, "side.txt", "side\n", "side");
        git_ok(&root, &["checkout", "-q", "main"]);

        let (mut model, mut runtime, mut queue) = open_command_model(&root);
        run_command(
            &mut queue,
            &mut model,
            &mut runtime,
            Command::DeleteBranch {
                name: "side".into(),
                force: false,
            },
        );
        assert!(
            git_ok(&root, &["show-ref", "--verify", "refs/heads/side"]).contains("refs/heads/side")
        );
        assert!(matches!(
            runtime.dialog,
            lazecode_ui::runtime::DialogState::DeleteBranch {
                ref name,
                force: true,
                remote: false,
                ..
            } if name == "side"
        ));
        run_command(
            &mut queue,
            &mut model,
            &mut runtime,
            Command::DeleteBranch {
                name: "side".into(),
                force: true,
            },
        );
        let refs = git_ok(&root, &["for-each-ref", "refs/heads/"]);
        assert!(!refs.contains("refs/heads/side"));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&remote);
    }
    #[test]
    fn branch_tag_and_remote_commands_work_end_to_end() {
        let (root, remote) = temp_repo("branches");
        git_ok(&root, &["init", "-q", "-b", "main"]);
        git_ok(&root, &["config", "user.name", "T"]);
        git_ok(&root, &["config", "user.email", "t@e"]);
        commit(&root, "a.txt", "one\n", "one");
        let first = git_ok(&root, &["rev-parse", "HEAD"]);
        git_ok(
            &root,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git_ok(&root, &["push", "-q", "-u", "origin", "main"]);
        commit(&root, "b.txt", "two\n", "two");
        git_ok(&root, &["branch", "docs-only"]);
        git_ok(&root, &["push", "-q", "origin", "docs-only"]);
        git_ok(&root, &["branch", "-D", "docs-only"]);
        git_ok(&root, &["tag", "v1", &first]);
        git_ok(&root, &["fetch", "-q", "origin"]);

        let (mut model, mut runtime, mut queue) = open_command_model(&root);
        run_command(
            &mut queue,
            &mut model,
            &mut runtime,
            Command::CheckoutBranch("origin/docs-only".into()),
        );
        assert_eq!(git_ok(&root, &["branch", "--show-current"]), "docs-only");
        assert_eq!(
            git_ok(&root, &["rev-parse", "docs-only@{upstream}"]),
            git_ok(&root, &["rev-parse", "origin/docs-only"])
        );

        run_command(
            &mut queue,
            &mut model,
            &mut runtime,
            Command::CheckoutBranch("main".into()),
        );
        run_command(
            &mut queue,
            &mut model,
            &mut runtime,
            Command::CheckoutRevision("v1".into()),
        );
        assert_eq!(git_ok(&root, &["rev-parse", "HEAD"]), first);
        assert!(git_ok(&root, &["branch", "--show-current"]).is_empty());

        git_ok(&root, &["checkout", "-q", "main"]);
        run_command(
            &mut queue,
            &mut model,
            &mut runtime,
            Command::CreateBranch {
                name: "from-first".into(),
                start_point: Some(first.clone()),
                checkout: true,
            },
        );
        assert_eq!(git_ok(&root, &["branch", "--show-current"]), "from-first");
        assert_eq!(git_ok(&root, &["rev-parse", "HEAD"]), first);

        run_command(
            &mut queue,
            &mut model,
            &mut runtime,
            Command::DeleteRemoteBranch("origin/docs-only".into()),
        );
        let heads = git_ok(&root, &["ls-remote", "--heads", "origin"]);
        assert!(!heads.contains("docs-only"), "{heads}");

        run_command(
            &mut queue,
            &mut model,
            &mut runtime,
            Command::PushTag {
                remote: "origin".into(),
                tag: "v1".into(),
            },
        );
        let tags = git_ok(&root, &["ls-remote", "--tags", "origin"]);
        assert!(tags.contains("refs/tags/v1"), "{tags}");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&remote);
    }
    #[test]
    fn push_tags_and_remote_edit_commands_work() {
        let (root, remote) = temp_repo("sync");
        git_ok(&root, &["init", "-q", "-b", "main"]);
        git_ok(&root, &["config", "user.name", "T"]);
        git_ok(&root, &["config", "user.email", "t@e"]);
        commit(&root, "a.txt", "one\n", "one");
        git_ok(
            &root,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git_ok(&root, &["tag", "v1"]);

        let (mut model, mut runtime, mut queue) = open_command_model(&root);
        run_command(
            &mut queue,
            &mut model,
            &mut runtime,
            Command::Push {
                remote: "origin".into(),
                branch: "main".into(),
                set_upstream: true,
                force: false,
                push_tags: true,
            },
        );
        assert!(git_ok(&root, &["ls-remote", "--heads", "origin"]).contains("refs/heads/main"));
        assert!(git_ok(&root, &["ls-remote", "--tags", "origin"]).contains("refs/tags/v1"));

        run_command(
            &mut queue,
            &mut model,
            &mut runtime,
            Command::RemoteEdit {
                from: "origin".into(),
                name: "upstream".into(),
                url: remote.to_str().unwrap().to_string(),
            },
        );
        assert_eq!(
            git_ok(&root, &["remote", "get-url", "upstream"]),
            remote.to_str().unwrap()
        );
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&remote);
    }

    #[test]
    fn cherry_pick_selected_changes_moves_only_selected_changes() {
        let (root, remote) = temp_repo("cherry-changes");
        git_ok(&root, &["init", "-q", "-b", "main"]);
        git_ok(&root, &["config", "user.name", "T"]);
        git_ok(&root, &["config", "user.email", "t@e"]);
        commit(&root, "a.txt", "base\n", "base");
        git_ok(&root, &["branch", "target"]);

        // An unselected change must block the move instead of leaking onto
        // the target branch.
        std::fs::write(root.join("a.txt"), "selected\n").unwrap();
        std::fs::write(root.join("b.txt"), "other\n").unwrap();
        let err = cherry_pick_selected_changes(&root, &["a.txt".to_string()], "target")
            .unwrap_err()
            .to_string();
        assert!(err.contains("unselected"), "{err}");
        assert_eq!(git_ok(&root, &["branch", "--show-current"]), "main");
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "selected\n"
        );

        std::fs::remove_file(root.join("b.txt")).unwrap();
        let message =
            cherry_pick_selected_changes(&root, &["a.txt".to_string()], "target").unwrap();
        assert!(message.contains("target"), "{message}");
        assert_eq!(git_ok(&root, &["branch", "--show-current"]), "target");
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "selected\n"
        );
        assert!(lazecode_git::shelf_list(&root).unwrap().is_empty());
        git_ok(&root, &["checkout", "-f", "main"]);
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "base\n"
        );
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&remote);
    }

    #[test]
    fn cherry_pick_moves_untracked_files_and_restores_on_checkout_failure() {
        let (root, remote) = temp_repo("cherry-untracked");
        git_ok(&root, &["init", "-q", "-b", "main"]);
        git_ok(&root, &["config", "user.name", "T"]);
        git_ok(&root, &["config", "user.email", "t@e"]);
        commit(&root, "a.txt", "base\n", "base");
        git_ok(&root, &["branch", "target"]);

        std::fs::write(root.join("a.txt"), "selected\n").unwrap();
        std::fs::write(root.join("new.txt"), "untracked\n").unwrap();
        let paths = ["a.txt".to_string(), "new.txt".to_string()];
        let message = cherry_pick_selected_changes(&root, &paths, "target").unwrap();
        assert!(message.contains("target"), "{message}");
        assert_eq!(git_ok(&root, &["branch", "--show-current"]), "target");
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "selected\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("new.txt")).unwrap(),
            "untracked\n"
        );
        assert!(lazecode_git::shelf_list(&root).unwrap().is_empty());

        // A failed checkout must put the selection back on the original
        // branch and remove the temporary shelf.
        git_ok(&root, &["checkout", "-f", "main"]);
        std::fs::remove_file(root.join("new.txt")).unwrap();
        std::fs::write(root.join("a.txt"), "rollback\n").unwrap();
        let err = cherry_pick_selected_changes(&root, &["a.txt".to_string()], "missing-branch")
            .unwrap_err()
            .to_string();
        assert!(err.contains("cannot check out"), "{err}");
        assert_eq!(git_ok(&root, &["branch", "--show-current"]), "main");
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "rollback\n"
        );
        assert!(lazecode_git::shelf_list(&root).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&remote);
    }

    #[test]
    fn stash_clear_command_drops_every_entry_and_reports_success() {
        let (root, remote) = temp_repo("stash-clear");
        git_ok(&root, &["init", "-q", "-b", "main"]);
        git_ok(&root, &["config", "user.name", "T"]);
        git_ok(&root, &["config", "user.email", "t@e"]);
        commit(&root, "a.txt", "base\n", "base");
        std::fs::write(root.join("a.txt"), "one\n").unwrap();
        git_ok(&root, &["stash", "push", "-q", "-m", "one"]);
        std::fs::write(root.join("a.txt"), "two\n").unwrap();
        git_ok(&root, &["stash", "push", "-q", "-m", "two"]);
        assert_eq!(lazecode_git::stash_list(&root).unwrap().len(), 2);

        let (mut model, mut runtime, mut queue) = open_command_model(&root);
        run_command(&mut queue, &mut model, &mut runtime, Command::StashClear);
        assert!(lazecode_git::stash_list(&root).unwrap().is_empty());
        assert!(model
            .toasts
            .last()
            .is_some_and(|toast| toast.message.contains('2')));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&remote);
    }
}
