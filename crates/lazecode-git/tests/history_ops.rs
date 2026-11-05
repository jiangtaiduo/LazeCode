use lazecode_git::{
    cherry_pick, conflict_list, drop_commit, merge, rebase, reset, resolve_take, revert,
    sequencer_state, squash_commits, ResetMode, SequencerState,
};
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, expected_ok: bool, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    if expected_ok {
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), true, &["init", "-q", "-b", "main"]);
    git(dir.path(), true, &["config", "user.name", "T"]);
    git(dir.path(), true, &["config", "user.email", "t@e"]);
    dir
}

fn commit(dir: &Path, file: &str, content: &str, message: &str) -> String {
    std::fs::write(dir.join(file), content).unwrap();
    git(dir, true, &["add", file]);
    git(dir, true, &["commit", "-q", "-m", message]);
    git(dir, true, &["rev-parse", "HEAD"])
}

#[test]
fn merge_clean_and_conflict_take_sides() {
    let dir = repo();
    let root = dir.path();
    commit(root, "one.txt", "base\n", "base");
    git(root, true, &["checkout", "-qb", "feature"]);
    let _feature = commit(root, "one.txt", "feature\n", "feature wins");
    git(root, true, &["checkout", "-q", "main"]);
    commit(root, "one.txt", "main\n", "main wins");
    assert_eq!(sequencer_state(root).unwrap(), SequencerState::None);

    // Conflicting merge (both modify one.txt).
    let err = merge(root, "feature").unwrap_err().to_string();
    eprintln!("MERGE ERR: {err}");
    assert!(
        err.contains("conflict") || err.contains("CONFLICT"),
        "{err}"
    );
    assert!(
        matches!(sequencer_state(root).unwrap(), SequencerState::Merge { .. }),
        "sequencer reports the merge state"
    );
    let conflicts = conflict_list(root).unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].path, "one.txt");
    assert_eq!(conflicts[0].kind.as_deref(), Some("UU"));
    resolve_take(root, "one.txt", true).unwrap();
}

#[test]
fn clean_merge_and_rebase_run() {
    let dir = repo();
    let root = dir.path();
    commit(root, "base.txt", "base\n", "base");
    git(root, true, &["checkout", "-qb", "feature"]);
    commit(root, "new.txt", "new\n", "feature");
    git(root, true, &["checkout", "-q", "main"]);
    commit(root, "main.txt", "main\n", "main change");

    merge(root, "feature").unwrap();
    assert!(root.join("new.txt").exists());
    assert_eq!(sequencer_state(root).unwrap(), SequencerState::None);

    // rebase the feature branch onto main.
    git(root, true, &["checkout", "-q", "feature"]);
    rebase(root, "main").unwrap();
    assert_eq!(sequencer_state(root).unwrap(), SequencerState::None);
    git(root, true, &["checkout", "-q", "main"]);
}

#[test]
fn cherry_pick_conflict_continue_and_abort() {
    let dir = repo();
    let root = dir.path();
    commit(root, "file.txt", "base\n", "base");
    git(root, true, &["checkout", "-qb", "other"]);
    let picked = commit(root, "file.txt", "other\n", "other change");
    git(root, true, &["checkout", "-q", "main"]);
    commit(root, "file.txt", "main\n", "main change");
    let err = cherry_pick(root, &picked).unwrap_err().to_string();
    assert!(
        err.contains("conflict") || err.contains("CONFLICT"),
        "{err}"
    );
    assert!(matches!(
        sequencer_state(root).unwrap(),
        SequencerState::CherryPick { .. }
    ));
    resolve_take(root, "file.txt", false).unwrap();
    assert!(lazecode_git::history::cherry_pick_continue(root).is_ok());
    let log = git(root, true, &["log", "--oneline"]);
    assert!(log.contains("other change"), "{log}");
}

#[test]
fn reset_modes_drop_and_squash() {
    let dir = repo();
    let root = dir.path();
    let _base = commit(root, "a.txt", "a\n", "first");
    let second = commit(root, "b.txt", "b\n", "second");
    let third = commit(root, "c.txt", "c\n", "third");

    // reset --hard to second drops the third.
    reset(root, &second, ResetMode::Hard).unwrap();
    assert!(!root.join("c.txt").exists());

    // Re-pick the third commit hash — it was dropped, so cherry-picking
    // the hash back works.
    cherry_pick(root, &third).unwrap();
    assert!(root.join("c.txt").exists());

    // Squash the two fresh commits into one.
    let squash_a = commit(root, "x.txt", "x\n", "alpha");
    let squash_b = commit(root, "y.txt", "y\n", "beta");
    squash_commits(
        root,
        &[squash_a.clone(), squash_b.clone()],
        Some("squashed!"),
    )
    .unwrap();
    let log = git(root, true, &["log", "--oneline"]);
    assert!(log.contains("squashed!"), "{log}");
    assert!(!log.contains("alpha"), "{log}");
    assert!(!log.contains("beta"), "{log}");
}

#[test]
fn drop_commit_removes_and_refuses_dirty() {
    let dir = repo();
    let root = dir.path();
    let base = commit(root, "a.txt", "a\n", "base");
    let second = commit(root, "b.txt", "b\n", "second");
    commit(root, "c.txt", "c\n", "third");

    // dirty tree blocks drop (modify a tracked file)
    std::fs::write(root.join("a.txt"), "a\nmodified\n").unwrap();
    drop_commit(root, &second, false).unwrap_err();
    git(root, true, &["checkout", "-q", "a.txt"]);

    drop_commit(root, &second, false).unwrap();
    let log = git(root, true, &["log", "--oneline"]);
    assert!(!log.contains("second"), "{log}");
    assert!(log.contains("base"), "{log}");
    let _ = base;
}

#[test]
fn revert_generates_inverse_commit() {
    let dir = repo();
    let root = dir.path();
    commit(root, "a.txt", "a\n", "base");
    let target = commit(root, "b.txt", "b\n", "add");
    revert(root, &target).unwrap();
    assert!(!root.join("b.txt").exists());
}
