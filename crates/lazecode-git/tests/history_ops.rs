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
    let snap = lazecode_git::snapshot(root).unwrap();
    let conflicted = snap
        .changes
        .iter()
        .find(|change| change.path == "one.txt")
        .expect("conflicted change row");
    assert!(conflicted.conflicted);
    assert_eq!(conflicted.letter(), 'C');
    assert!(!conflicted.untracked);
    resolve_take(root, "one.txt", true).unwrap();
}

#[test]
fn resolve_take_deletion_side_removes_the_file() {
    let dir = repo();
    let root = dir.path();
    commit(root, "gone.txt", "base\n", "base");
    git(root, true, &["checkout", "-qb", "feature"]);
    git(root, true, &["rm", "-q", "gone.txt"]);
    git(root, true, &["commit", "-q", "-m", "delete"]);
    git(root, true, &["checkout", "-q", "main"]);
    commit(root, "gone.txt", "main edit\n", "modify");
    merge(root, "feature").unwrap_err();
    assert!(root.join("gone.txt").exists());
    // Taking "theirs" means accepting the deletion, which must be `git rm`,
    // not writing an empty file and staging it.
    resolve_take(root, "gone.txt", false).unwrap();
    assert!(!root.join("gone.txt").exists());
    assert!(git(root, true, &["ls-files", "gone.txt"]).is_empty());
    assert!(conflict_list(root).unwrap().is_empty());
    assert!(matches!(
        sequencer_state(root).unwrap(),
        SequencerState::Merge { .. }
    ));
}

#[test]
fn drop_rejects_merge_published_and_foreign_commits() {
    let dir = repo();
    let root = dir.path();
    commit(root, "a.txt", "a\n", "base");
    git(root, true, &["checkout", "-qb", "side"]);
    commit(root, "side.txt", "s\n", "side");
    git(root, true, &["checkout", "-q", "main"]);
    commit(root, "b.txt", "b\n", "main");
    merge(root, "side").unwrap();
    let merge_id = git(root, true, &["rev-parse", "HEAD"]);
    drop_commit(root, &merge_id, false).unwrap_err();

    // Foreign commit not on HEAD's ancestry.
    git(root, true, &["checkout", "-q", "--orphan", "orphan"]);
    git(root, true, &["rm", "-rf", "."]);
    let foreign = commit(root, "foreign.txt", "f\n", "foreign");
    git(root, true, &["checkout", "-q", "main"]);
    let error = drop_commit(root, &foreign, false).unwrap_err().to_string();
    assert!(error.contains("not an ancestor"), "{error}");

    // Published HEAD (a normal commit; the merge commit above only tests the
    // merge guard).
    commit(root, "c.txt", "c\n", "after merge");
    let bare = tempfile::tempdir().unwrap();
    git(bare.path(), true, &["init", "-q", "--bare"]);
    git(
        root,
        true,
        &["remote", "add", "origin", bare.path().to_str().unwrap()],
    );
    git(root, true, &["push", "-q", "-u", "origin", "main"]);
    let head = git(root, true, &["rev-parse", "HEAD"]);
    let error = drop_commit(root, &head, false).unwrap_err().to_string();
    assert!(
        error.contains("published") || error.contains("remote branch"),
        "{error}"
    );
}

#[test]
fn squash_rejects_non_contiguous_selection() {
    let dir = repo();
    let root = dir.path();
    let _root_commit = commit(root, "a.txt", "a\n", "first");
    let second = commit(root, "b.txt", "b\n", "second");
    let third = commit(root, "c.txt", "c\n", "third");
    let fourth = commit(root, "d.txt", "d\n", "fourth");

    // second+third are older than the unselected fourth: folding them would
    // also rewrite `fourth`.
    let error = squash_commits(root, &[second.clone(), third.clone()], Some("folded"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("contiguous"), "{error}");

    // The newest two are a valid contiguous suffix.
    squash_commits(root, &[third, fourth], Some("folded")).unwrap();
    let log = git(root, true, &["log", "--oneline"]);
    assert!(log.contains("folded"), "{log}");
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

#[test]
fn three_way_merge_file_content_and_resolve() {
    let dir = repo();
    let root = dir.path();
    commit(root, "file.txt", "base content\n", "base");

    git(root, true, &["checkout", "-qb", "theirs-branch"]);
    commit(root, "file.txt", "theirs change\n", "theirs commit");

    git(root, true, &["checkout", "-q", "main"]);
    commit(root, "file.txt", "ours change\n", "ours commit");

    // trigger merge conflict
    git(root, false, &["merge", "--no-ff", "theirs-branch"]);

    // read 3-way content
    let m = lazecode_git::merge_file_content(root, "file.txt").unwrap();
    assert!(m.ours.contains("ours change"));
    assert!(m.theirs.contains("theirs change"));
    assert!(m.worktree.contains("<<<<<<<"));

    // mark resolved with clean text
    lazecode_git::mark_conflict_resolved(root, "file.txt", Some("resolved content\n")).unwrap();
    let clean = std::fs::read_to_string(root.join("file.txt")).unwrap();
    assert_eq!(clean, "resolved content\n");
    let conflicts = lazecode_git::conflict_list(root).unwrap();
    assert!(
        conflicts.is_empty(),
        "conflicts should be empty after resolution"
    );
}

#[test]
fn undo_amend_checkout_rev_and_patch() {
    let dir = repo();
    let root = dir.path();
    let base = commit(root, "a.txt", "line1\n", "initial commit");
    let second = commit(root, "b.txt", "line2\n", "second commit");

    // 1. Commit patch
    let patch = lazecode_git::commit_patch(root, &second).unwrap();
    assert!(patch.contains("second commit"), "{patch}");
    assert!(patch.contains("+line2"), "{patch}");

    // 2. Amend commit message
    lazecode_git::amend_commit_message(root, "amended second commit").unwrap();
    let log = git(root, true, &["log", "-1", "--format=%s"]);
    assert_eq!(log.trim(), "amended second commit");

    // 3. Checkout detached revision
    lazecode_git::checkout_revision(root, &base).unwrap();
    let head_rev = git(root, true, &["rev-parse", "HEAD"]);
    assert_eq!(head_rev.trim(), base);

    // Go back to main
    git(root, true, &["checkout", "-q", "main"]);

    // 4. Undo HEAD commit
    lazecode_git::undo_commit_head(root).unwrap();
    let log = git(root, true, &["log", "-1", "--format=%s"]);
    assert_eq!(log.trim(), "initial commit");
    // Change b.txt is still present in worktree / index
    assert!(root.join("b.txt").exists());
}

#[test]
fn discard_changes_and_git_init() {
    let temp = tempfile::tempdir().unwrap();
    let new_repo = temp.path().join("subrepo");

    // git_init on a clean path
    lazecode_git::git_init(&new_repo).unwrap();
    assert!(new_repo.join(".git").exists());

    let dir = repo();
    let root = dir.path();
    commit(root, "tracked.txt", "orig\n", "init");

    // Modify tracked file
    std::fs::write(root.join("tracked.txt"), "modified\n").unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("tracked.txt")).unwrap(),
        "modified\n"
    );

    // Discard tracked changes
    lazecode_git::discard_file_changes(root, "tracked.txt").unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("tracked.txt")).unwrap(),
        "orig\n"
    );

    // Create untracked file
    std::fs::write(root.join("untracked.txt"), "untracked\n").unwrap();
    assert!(root.join("untracked.txt").exists());

    // Discard untracked changes
    lazecode_git::discard_file_changes(root, "untracked.txt").unwrap();
    assert!(!root.join("untracked.txt").exists());
}

#[test]
fn interactive_rebase_applies_edited_todo() {
    let dir = repo();
    let root = dir.path();
    commit(root, "base.txt", "base\n", "base");
    git(root, true, &["checkout", "-qb", "feature"]);
    let first = commit(root, "one.txt", "one\n", "one");
    let second = commit(root, "two.txt", "two\n", "two");
    let _third = commit(root, "three.txt", "three\n", "three");

    // squash second into first, drop third：执行后 two/three 文件消失，
    // one.txt 保留，且 feature 的提交数缩为 1（+ base 之外）。
    let steps = vec![
        lazecode_git::RebaseStep {
            action: "pick".into(),
            commit: first.clone(),
        },
        lazecode_git::RebaseStep {
            action: "squash".into(),
            commit: second.clone(),
        },
        lazecode_git::RebaseStep {
            action: "drop".into(),
            commit: _third.clone(),
        },
    ];
    lazecode_git::rebase_interactive(root, "main", &steps).unwrap();
    assert!(!root.join("three.txt").exists());
    assert!(root.join("one.txt").exists());
    assert!(root.join("two.txt").exists());
    let count = git(root, true, &["rev-list", "--count", "main..feature"]);
    assert_eq!(count, "1");
}

#[test]
fn interactive_rebase_rejects_bad_action_and_hash() {
    let dir = repo();
    let root = dir.path();
    commit(root, "base.txt", "base\n", "base");
    let base = git(root, true, &["rev-parse", "HEAD"]);
    let steps = vec![lazecode_git::RebaseStep {
        action: "explode".into(),
        commit: base.clone(),
    }];
    assert!(lazecode_git::rebase_interactive(root, "main", &steps).is_err());
    let steps = vec![lazecode_git::RebaseStep {
        action: "pick".into(),
        commit: "../not-a-hash".into(),
    }];
    assert!(lazecode_git::rebase_interactive(root, "main", &steps).is_err());
}

#[test]
fn discard_refuses_paths_outside_the_worktree() {
    let dir = repo();
    let root = dir.path();
    commit(root, "a.txt", "base\n", "base");
    let outside = root.parent().unwrap().join("lazecode-outside.txt");
    std::fs::write(&outside, "keep me\n").unwrap();
    let result = lazecode_git::discard_file_changes(root, "../lazecode-outside.txt");
    assert!(result.is_err(), "path traversal must be rejected");
    assert_eq!(
        std::fs::read_to_string(&outside).unwrap(),
        "keep me\n",
        "outside file must be untouched"
    );
    std::fs::remove_file(&outside).ok();
}

#[test]
fn reset_soft_mixed_hard_and_detached_head_are_native() {
    // Soft: HEAD moves, index and worktree keep the old tree.
    let soft = repo();
    let root = soft.path();
    let base = commit(root, "a.txt", "a\n", "base");
    commit(root, "b.txt", "b\n", "second");
    reset(root, &base, ResetMode::Soft).unwrap();
    assert_eq!(git(root, true, &["rev-parse", "HEAD"]), base);
    assert!(root.join("b.txt").exists());
    assert!(git(root, true, &["diff", "--cached", "--name-only"]).contains("b.txt"));

    // Mixed: HEAD and index move, worktree keeps the old file as untracked.
    let mixed = repo();
    let root = mixed.path();
    let base = commit(root, "a.txt", "a\n", "base");
    commit(root, "b.txt", "b\n", "second");
    reset(root, &base, ResetMode::Mixed).unwrap();
    assert_eq!(git(root, true, &["rev-parse", "HEAD"]), base);
    assert!(git(root, true, &["diff", "--cached", "--name-only"]).is_empty());
    assert!(git(root, true, &["status", "--porcelain"]).contains("?? b.txt"));

    // Hard: worktree follows, tracked files disappear, untracked files stay.
    let hard = repo();
    let root = hard.path();
    let base = commit(root, "a.txt", "a\n", "base");
    commit(root, "b.txt", "b\n", "second");
    std::fs::write(root.join("untracked.txt"), "keep\n").unwrap();
    reset(root, &base, ResetMode::Hard).unwrap();
    assert!(!root.join("b.txt").exists());
    assert!(root.join("untracked.txt").exists());
    assert_eq!(lazecode_git::snapshot(root).unwrap().changes.len(), 1);

    // Detached HEAD: moving it must not move the branch.
    git(root, true, &["checkout", "-q", "--detach", "main"]);
    commit(root, "c.txt", "c\n", "detached");
    reset(root, "HEAD~1", ResetMode::Soft).unwrap();
    assert_eq!(git(root, true, &["rev-parse", "HEAD"]), base);
    assert_eq!(git(root, true, &["rev-parse", "main"]), base);
}
