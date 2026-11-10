//! End-to-end worktree lifecycle: create via the git crate, attach to the
//! model, spawn a console rooted at the worktree, then remove the worktree
//! (which must also kill its consoles).

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use lazecode_core::{AppModel, ProjectState, Settings, UiState, WorktreeState};
use lazecode_terminal::Notifier;
use std::collections::HashSet;

struct Noop;

impl Notifier for Noop {
    fn notify(&self) {}
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(dir)
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

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["config", "user.name", "Tester"]);
    git(dir.path(), &["config", "user.email", "tester@example.com"]);
    std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);
    dir
}

fn model(project: ProjectState) -> (AppModel, usize) {
    let mut model = AppModel {
        settings: Settings::default(),
        projects: Vec::new(),
        active_project: None,
        ui: UiState::default(),
        toasts: Vec::new(),
        recent: Vec::new(),
        commands: Vec::new(),
        notifier: None,
        should_quit: false,
        version: "test",
        favorite_branches: HashSet::new(),
        git_cmd_log: Vec::new(),
    };
    let index = model.add_loaded_project(project);
    (model, index)
}

#[test]
fn worktree_create_console_remove_round_trip() {
    let dir = repo();
    let repo_root = dir.path().canonicalize().unwrap();
    let worktree_dir = tempfile::tempdir().unwrap();
    let destination = worktree_dir.path().join("feature-wt");
    let destination = destination.canonicalize().unwrap_or(destination);

    let created = lazecode_git::add_worktree_detached_at_head(&repo_root, &destination).unwrap();
    assert!(
        created.join("main.rs").is_file(),
        "checkout must materialise the branch files"
    );
    assert!(lazecode_git::is_linked_worktree(&created).unwrap());

    let settings = Settings::default();
    let project = ProjectState::open(repo_root.clone(), &settings, Some(Arc::new(Noop))).unwrap();
    let (mut model, project_index) = model(project);
    let repo = &mut model.projects[project_index];
    repo.attach_worktree(WorktreeState::load(
        "feature-wt".to_string(),
        created.clone(),
        false,
    ));
    let worktree_index = repo.worktree_index("feature-wt").unwrap();
    repo.active_worktree = worktree_index;

    // Console spawned in the worktree, isolated from the main checkout.
    let id = repo
        .open_terminal_in(
            repo.active_worktree().path.clone(),
            None,
            Some("feature-console".into()),
            lazecode_terminal::SessionKind::Shell,
            &settings,
            None,
        )
        .unwrap();
    assert_eq!(repo.active_worktree().terminals.len(), 1);
    assert_eq!(repo.active_worktree().terminals[0].id, id);
    assert_eq!(repo.worktrees[0].terminals.len(), 1);
    let main_terminal_id = repo.worktrees[0].terminals[0].id;
    assert_ne!(main_terminal_id, id, "terminal ids must not collide");
    let session_cwd = repo.active_worktree().terminals[0].session.cwd.clone();
    assert_eq!(session_cwd, created);

    // Git state per worktree is independent: the linked worktree is detached
    // while main keeps its branch.
    let main_branch = repo.worktrees[0].branch();
    let wt_branch = repo.active_worktree().branch();
    assert_eq!(main_branch, "main");
    assert!(
        wt_branch.contains("(detached)"),
        "expected detached HEAD, got {wt_branch}"
    );

    // teardown: close the console, detach, remove the worktree.
    repo.close_terminal(id);
    repo.active_worktree = 0;
    repo.detach_worktree("feature-wt");
    assert_eq!(repo.worktrees.len(), 1);
    assert!(repo.worktrees[0].is_main);

    lazecode_git::remove_worktree(&repo_root, &created, false).unwrap();
    assert!(!created.exists());
    let listed = lazecode_git::list_worktrees(&repo_root).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "main");

    // The detached model entry cannot be revived by the same name.
    assert!(repo.worktree_index("feature-wt").is_none());
}

#[test]
fn ensure_worktree_attaches_only_git_registered_checkouts() {
    let dir = repo();
    let repo_root = dir.path().canonicalize().unwrap();
    let other = tempfile::tempdir().unwrap();
    let destination = other.path().join("wt");

    git(
        dir.path(),
        &["worktree", "add", destination.to_str().unwrap()],
    );
    let settings = Settings::default();
    let project = ProjectState::open(repo_root.clone(), &settings, None).unwrap();
    let (mut model, project_index) = model(project);
    let entry = &mut model.projects[project_index];

    // Not registered yet: only main.
    assert_eq!(entry.worktrees.len(), 1);

    // Unknown name errors.
    let err = entry.ensure_worktree("does-not-exist").unwrap_err();
    assert!(err.contains("does not exist"));

    // Registered git worktree is attached lazily by name.
    let linked = lazecode_git::list_worktrees(&repo_root)
        .unwrap()
        .into_iter()
        .find(|worktree| worktree.name == "wt")
        .expect("git registered worktree should be listed");
    entry.attach_worktree(WorktreeState::load(linked.name, linked.path, false));
    assert!(entry.worktree_index("wt").is_some());
    entry.ensure_worktree("wt").unwrap();
    assert_eq!(entry.active_worktree, entry.worktree_index("wt").unwrap());
}
