use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn init() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["config", "user.name", "Tester"]);
    git(dir.path(), &["config", "user.email", "tester@example.com"]);
    dir
}

#[cfg(unix)]
fn install_hook(root: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let dir = root.join(".git/hooks");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn snapshot_stage_commit_and_log() {
    let dir = init();
    std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();

    let snap = lazecode_git::snapshot(dir.path()).unwrap();
    assert_eq!(snap.changes.len(), 2);
    assert!(snap.changes.iter().all(|c| c.untracked));
    assert_eq!(snap.branch.as_deref(), Some("main"));

    lazecode_git::stage(dir.path(), &["a.txt".into(), "src/main.rs".into()]).unwrap();
    let snap = lazecode_git::snapshot(dir.path()).unwrap();
    assert_eq!(snap.changes.len(), 2);
    assert!(snap.changes.iter().all(|c| c.staged.is_some()));
    assert!(snap.has_tracked_changes());

    lazecode_git::commit(dir.path(), "initial", false).unwrap();
    let snap = lazecode_git::snapshot(dir.path()).unwrap();
    assert!(
        snap.changes.is_empty(),
        "changes after commit: {:?}",
        snap.changes
    );
    assert_eq!(snap.head.as_ref().unwrap().message, "initial");

    let log = lazecode_git::log(dir.path(), Default::default()).unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].message, "initial");
    assert!(log[0]
        .refs
        .iter()
        .any(|r| r.kind == lazecode_git::RefKind::Head));

    let branches = lazecode_git::branches(dir.path()).unwrap();
    assert!(branches.iter().any(|b| b.name == "main" && b.current));
}

#[test]
fn modify_diff_unstage_and_checkout() {
    let dir = init();
    std::fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();
    lazecode_git::stage(dir.path(), &["a.txt".into()]).unwrap();
    lazecode_git::commit(dir.path(), "base", false).unwrap();

    std::fs::write(dir.path().join("a.txt"), "one\nTWO\nthree\n").unwrap();
    let snap = lazecode_git::snapshot(dir.path()).unwrap();
    assert_eq!(snap.changes.len(), 1);
    let change = &snap.changes[0];
    assert_eq!(change.letter(), 'M');
    assert_eq!(change.additions, 2);
    assert_eq!(change.deletions, 1);

    let diff = lazecode_git::file_diff(dir.path(), "a.txt").unwrap();
    assert!(!diff.binary);
    assert_eq!(diff.diff.additions, 2);
    assert_eq!(diff.diff.deletions, 1);

    lazecode_git::stage(dir.path(), &["a.txt".into()]).unwrap();
    lazecode_git::unstage(dir.path(), &["a.txt".into()]).unwrap();
    let snap = lazecode_git::snapshot(dir.path()).unwrap();
    assert!(snap.changes[0].staged.is_none());

    // Restore the worktree so the branch switch is allowed.
    std::fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();
    lazecode_git::checkout_branch(dir.path(), "main").unwrap();
    let content = std::fs::read_to_string(dir.path().join("a.txt")).unwrap();
    assert_eq!(content, "one\ntwo\n");

    lazecode_git::create_branch(dir.path(), "feature", true).unwrap();
    assert_eq!(git(dir.path(), &["branch", "--show-current"]), "feature");
    lazecode_git::checkout_branch(dir.path(), "main").unwrap();
    assert_eq!(git(dir.path(), &["branch", "--show-current"]), "main");
}

#[test]
fn log_marks_head_and_puts_the_checked_out_branch_first() {
    let dir = init();
    std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
    lazecode_git::stage(dir.path(), &["a.txt".into()]).unwrap();
    lazecode_git::commit(dir.path(), "base", false).unwrap();

    // `aaa` sorts before `main`; the checked-out branch must still lead the
    // refs after HEAD so the compact log label names the current branch.
    lazecode_git::create_branch(dir.path(), "aaa", false).unwrap();
    lazecode_git::checkout_branch(dir.path(), "aaa").unwrap();

    let log = lazecode_git::log(dir.path(), Default::default()).unwrap();
    let refs = &log[0].refs;
    assert_eq!(refs[0].kind, lazecode_git::RefKind::Head);
    assert_eq!(refs[0].label, "HEAD");
    assert_eq!(refs[1].kind, lazecode_git::RefKind::LocalBranch);
    assert_eq!(refs[1].label, "aaa");
    let main_at = refs
        .iter()
        .position(|r| r.kind == lazecode_git::RefKind::LocalBranch && r.label == "main")
        .expect("main points at the same commit");
    assert!(
        main_at > 1,
        "checked-out branch must precede main: {refs:?}"
    );
}

#[test]
fn stage_deletion_removes_index_entry() {
    let dir = init();
    std::fs::write(dir.path().join("gone.txt"), "bye\n").unwrap();
    lazecode_git::stage(dir.path(), &["gone.txt".into()]).unwrap();
    lazecode_git::commit(dir.path(), "add gone", false).unwrap();
    std::fs::remove_file(dir.path().join("gone.txt")).unwrap();
    let snap = lazecode_git::snapshot(dir.path()).unwrap();
    assert!(snap
        .changes
        .iter()
        .any(|change| change.path == "gone.txt" && change.letter() == 'D'));
    lazecode_git::stage(dir.path(), &["gone.txt".into()]).unwrap();
    let snap = lazecode_git::snapshot(dir.path()).unwrap();
    assert!(snap.changes.iter().any(|change| {
        change.path == "gone.txt" && change.staged == Some(lazecode_git::ChangeKind::Deleted)
    }));
    lazecode_git::commit(dir.path(), "remove gone", false).unwrap();
    assert!(lazecode_git::snapshot(dir.path())
        .unwrap()
        .changes
        .is_empty());
}

#[test]
fn commit_details_lists_files() {
    let dir = init();
    std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
    lazecode_git::stage(dir.path(), &["a.txt".into()]).unwrap();
    lazecode_git::commit(dir.path(), "base", false).unwrap();
    std::fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();
    lazecode_git::stage(dir.path(), &["a.txt".into()]).unwrap();
    lazecode_git::commit(dir.path(), "second", false).unwrap();
    let details = lazecode_git::commit_details(dir.path(), "HEAD").unwrap();
    assert_eq!(details.info.message, "second");
    assert_eq!(details.files.len(), 1);
    assert_eq!(details.files[0].path, "a.txt");
    assert_eq!(details.files[0].additions, 1);
}

#[test]
fn commit_details_finds_changes_past_the_tree_listing_prefix() {
    let dir = init();
    let root = dir.path();
    for i in 0..320 {
        std::fs::write(root.join(format!("f{i:03}.txt")), "one\n").unwrap();
    }
    git(root, &["add", "-A"]);
    lazecode_git::commit(root, "base", false).unwrap();

    std::fs::write(root.join("f319.txt"), "one\ntwo\n").unwrap();
    git(root, &["add", "-A"]);
    lazecode_git::commit(root, "late path", false).unwrap();

    let details = lazecode_git::commit_details(root, "HEAD").unwrap();
    assert_eq!(details.info.message, "late path");
    assert_eq!(details.files.len(), 1);
    assert_eq!(details.files[0].path, "f319.txt");
}

#[test]
fn amend_replaces_head() {
    let dir = init();
    std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
    lazecode_git::stage(dir.path(), &["a.txt".into()]).unwrap();
    lazecode_git::commit(dir.path(), "first", false).unwrap();
    lazecode_git::commit(dir.path(), "amended", true).unwrap();
    assert_eq!(git(dir.path(), &["rev-list", "--count", "HEAD"]), "1");
    assert_eq!(git(dir.path(), &["log", "-1", "--pretty=%s"]), "amended");
}

#[test]
fn commit_hooks_can_block_and_rewrite_the_message() {
    let dir = init();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "one\n").unwrap();
    lazecode_git::stage(root, &["a.txt".into()]).unwrap();

    install_hook(root, "pre-commit", "exit 1");
    let error = lazecode_git::commit(root, "blocked", false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("pre-commit"), "{error}");
    assert!(lazecode_git::snapshot(root).unwrap().head.is_none());

    std::fs::remove_file(root.join(".git/hooks/pre-commit")).unwrap();
    install_hook(root, "commit-msg", "printf 'rewritten by hook\\n' > \"$1\"");
    lazecode_git::commit(root, "original", false).unwrap();
    assert_eq!(
        git(root, &["log", "-1", "--pretty=%s"]),
        "rewritten by hook"
    );
}

#[cfg(unix)]
#[test]
fn checkout_runs_post_checkout_hook() {
    let dir = init();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "one\n").unwrap();
    lazecode_git::stage(root, &["a.txt".into()]).unwrap();
    lazecode_git::commit(root, "base", false).unwrap();
    lazecode_git::create_branch(root, "feature", false).unwrap();
    install_hook(
        root,
        "post-checkout",
        "printf '%s' \"$3\" > post-checkout-flag",
    );
    lazecode_git::checkout_branch(root, "feature").unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("post-checkout-flag")).unwrap(),
        "1"
    );
}

#[test]
fn commit_checked_keeps_unchecked_paths_dirty() {
    let dir = init();
    std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
    std::fs::write(dir.path().join("b.txt"), "b\n").unwrap();
    lazecode_git::stage(dir.path(), &["a.txt".into(), "b.txt".into()]).unwrap();
    lazecode_git::commit(dir.path(), "base", false).unwrap();

    std::fs::write(dir.path().join("a.txt"), "a\nA\n").unwrap();
    std::fs::write(dir.path().join("b.txt"), "b\nB\n").unwrap();
    // Simulate a previously staged index: both files staged, only a checked.
    lazecode_git::stage(dir.path(), &["a.txt".into(), "b.txt".into()]).unwrap();
    lazecode_git::commit_checked(
        dir.path(),
        &["a.txt".into()],
        &["b.txt".into()],
        "only a",
        false,
    )
    .unwrap();

    let details = lazecode_git::commit_details(dir.path(), "HEAD").unwrap();
    assert_eq!(details.info.message, "only a");
    assert_eq!(details.files.len(), 1);
    assert_eq!(details.files[0].path, "a.txt");

    let snap = lazecode_git::snapshot(dir.path()).unwrap();
    assert!(!snap.changes.iter().any(|change| change.path == "a.txt"));
    let b = snap
        .changes
        .iter()
        .find(|change| change.path == "b.txt")
        .expect("b remains dirty");
    // Stable partial commit leaves files outside the checked set staged.
    assert!(
        b.staged.is_some(),
        "unchecked staged file must keep its index state: {b:?}"
    );
    assert!(b.unstaged.is_none());
}

#[test]
fn commit_rejects_submodules_instead_of_dropping_gitlinks() {
    let dir = init();
    let sub = tempfile::tempdir().unwrap();
    git(sub.path(), &["init", "-q", "-b", "main"]);
    git(sub.path(), &["config", "user.name", "Tester"]);
    git(sub.path(), &["config", "user.email", "tester@example.com"]);
    std::fs::write(sub.path().join("sub.txt"), "sub\n").unwrap();
    git(sub.path(), &["add", "sub.txt"]);
    git(sub.path(), &["commit", "-q", "-m", "sub"]);
    let sub_path = sub.path().to_string_lossy().to_string();
    git(
        dir.path(),
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            &sub_path,
            "sub",
        ],
    );
    git(dir.path(), &["commit", "-q", "-m", "add submodule"]);

    // The gitlink must not appear as a phantom staged addition.
    let snap = lazecode_git::snapshot(dir.path()).unwrap();
    assert!(snap.changes.iter().all(|change| change.path != "sub"));

    std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
    lazecode_git::stage(dir.path(), &["a.txt".into()]).unwrap();
    let error = lazecode_git::commit(dir.path(), "should fail", false).unwrap_err();
    assert!(error.to_string().contains("submodule"), "{error}");
    assert_eq!(git(dir.path(), &["rev-list", "--count", "HEAD"]), "1");
}

#[test]
fn commit_rejects_unresolved_conflicts() {
    let dir = init();
    std::fs::write(dir.path().join("a.txt"), "base\n").unwrap();
    git(dir.path(), &["add", "a.txt"]);
    git(dir.path(), &["commit", "-q", "-m", "base"]);
    git(dir.path(), &["checkout", "-q", "-b", "other"]);
    std::fs::write(dir.path().join("a.txt"), "other\n").unwrap();
    git(dir.path(), &["commit", "-q", "-am", "other"]);
    git(dir.path(), &["checkout", "-q", "main"]);
    std::fs::write(dir.path().join("a.txt"), "main\n").unwrap();
    git(dir.path(), &["commit", "-q", "-am", "main"]);
    let output = Command::new("git")
        .current_dir(dir.path())
        .args(["merge", "other"])
        .output()
        .unwrap();
    assert!(!output.status.success());

    let error = lazecode_git::commit(dir.path(), "merge", false).unwrap_err();
    assert!(error.to_string().contains("MERGE_HEAD"), "{error}");
    let error = lazecode_git::stage(dir.path(), &["a.txt".into()]).unwrap_err();
    assert!(error.to_string().contains("conflict"), "{error}");
}

#[test]
fn checkout_does_not_follow_symlinked_directories_out_of_worktree() {
    let dir = init();
    std::fs::write(dir.path().join("base.txt"), "base\n").unwrap();
    git(dir.path(), &["add", "base.txt"]);
    git(dir.path(), &["commit", "-q", "-m", "base"]);

    let outside = tempfile::tempdir().unwrap();
    git(dir.path(), &["checkout", "-q", "-b", "symlink"]);
    std::os::unix::fs::symlink(outside.path(), dir.path().join("docs")).unwrap();
    git(dir.path(), &["add", "docs"]);
    git(dir.path(), &["commit", "-q", "-m", "symlink"]);

    git(dir.path(), &["checkout", "-q", "main"]);
    git(dir.path(), &["checkout", "-q", "-b", "target"]);
    std::fs::create_dir(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("docs/x.txt"), "payload\n").unwrap();
    git(dir.path(), &["add", "docs/x.txt"]);
    git(dir.path(), &["commit", "-q", "-m", "target"]);

    git(dir.path(), &["checkout", "-q", "symlink"]);
    let error = lazecode_git::checkout_branch(dir.path(), "target").unwrap_err();
    assert!(error.to_string().contains("symlink"), "{error}");
    assert!(!outside.path().join("x.txt").exists());
    assert!(!outside.path().join("docs").exists());
}

#[test]
fn rejects_path_traversal_in_stage_and_diff() {
    let dir = init();
    std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
    assert!(lazecode_git::stage(dir.path(), &["../outside.txt".into()]).is_err());
    assert!(lazecode_git::stage(dir.path(), &["/tmp/outside.txt".into()]).is_err());
    assert!(lazecode_git::file_diff(dir.path(), "../outside.txt").is_err());
}

#[test]
fn diff_sources_commit_and_ref_and_files() {
    let dir = init();
    let root = dir.path();
    std::fs::write(root.join("f.txt"), "v1\n").unwrap();
    git(root, &["add", "f.txt"]);
    git(root, &["commit", "-q", "-m", "c1"]);
    let c1 = git(root, &["rev-parse", "HEAD"]);

    std::fs::write(root.join("f.txt"), "v2\n").unwrap();
    git(root, &["add", "f.txt"]);
    git(root, &["commit", "-q", "-m", "c2"]);
    let c2 = git(root, &["rev-parse", "HEAD"]);

    // commit_file_diff on c2 against parent c1
    let d_commit = lazecode_git::commit_file_diff(root, &c2, "f.txt").unwrap();
    assert_eq!(d_commit.old_text, "v1\n");
    assert_eq!(d_commit.new_text, "v2\n");
    assert_eq!(d_commit.diff.additions, 1);
    assert_eq!(d_commit.diff.deletions, 1);

    // diff_commits c1..c2
    let d_commits = lazecode_git::diff_commits(root, &c1, &c2, "f.txt").unwrap();
    assert_eq!(d_commits.old_text, "v1\n");
    assert_eq!(d_commits.new_text, "v2\n");

    // diff_ref_worktree against c1 when worktree is v3
    std::fs::write(root.join("f.txt"), "v3\n").unwrap();
    let d_local = lazecode_git::diff_ref_worktree(root, &c1, "f.txt").unwrap();
    assert_eq!(d_local.old_text, "v1\n");
    assert_eq!(d_local.new_text, "v3\n");

    // diff_two_files
    let f1 = root.join("f1.txt");
    let f2 = root.join("f2.txt");
    std::fs::write(&f1, "hello\n").unwrap();
    std::fs::write(&f2, "world\n").unwrap();
    let d_files = lazecode_git::diff_two_files(&f1, &f2).unwrap();
    assert_eq!(d_files.old_text, "hello\n");
    assert_eq!(d_files.new_text, "world\n");
}

#[test]
fn diff_ref_worktree_empty_path_finds_a_real_difference() {
    let dir = init();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "a1\n").unwrap();
    std::fs::write(root.join("b.txt"), "b1\n").unwrap();
    git(root, &["add", "a.txt", "b.txt"]);
    git(root, &["commit", "-q", "-m", "base"]);
    let base = git(root, &["rev-parse", "HEAD"]);

    std::fs::write(root.join("a.txt"), "a2\n").unwrap();
    std::fs::write(root.join("b.txt"), "b2\n").unwrap();
    git(root, &["add", "a.txt", "b.txt"]);
    git(root, &["commit", "-q", "-m", "next"]);

    // Clean worktree, historical commit: the diff must compare the commit
    // against HEAD's files, not only against the (empty) dirty list.
    let clean = lazecode_git::diff_ref_worktree(root, &base, "").unwrap();
    assert_eq!(clean.path, "a.txt");
    assert_eq!(clean.old_text, "a1\n");
    assert_eq!(clean.new_text, "a2\n");
    assert!(!clean.diff.hunks.is_empty());

    // If the first candidate was reverted to the commit's content, the
    // next differing file must be selected instead of returning empty.
    std::fs::write(root.join("a.txt"), "a1\n").unwrap();
    let reverted = lazecode_git::diff_ref_worktree(root, &base, "").unwrap();
    assert_eq!(reverted.path, "b.txt");
    assert_eq!(reverted.old_text, "b1\n");
    assert_eq!(reverted.new_text, "b2\n");
    assert!(!reverted.diff.hunks.is_empty());
}

#[test]
fn compare_commit_to_worktree_lists_committed_and_uncommitted_changes() {
    let dir = init();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "a1\n").unwrap();
    std::fs::write(root.join("b.txt"), "b1\n").unwrap();
    git(root, &["add", "a.txt", "b.txt"]);
    git(root, &["commit", "-q", "-m", "base"]);
    let base = git(root, &["rev-parse", "HEAD"]);

    std::fs::write(root.join("a.txt"), "a2\n").unwrap();
    git(root, &["add", "a.txt"]);
    git(root, &["commit", "-q", "-m", "next"]);

    // Uncommitted edit plus an untracked file: both belong to the local side.
    std::fs::write(root.join("b.txt"), "b2\n").unwrap();
    std::fs::write(root.join("c.txt"), "c1\nc2\n").unwrap();

    let files = lazecode_git::compare_commit_to_worktree(root, &base).unwrap();
    let paths: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(paths, ["a.txt", "b.txt", "c.txt"]);
    assert_eq!(files[0].status, 'M');
    assert_eq!(files[0].additions, 1);
    assert_eq!(files[0].deletions, 1);
    assert_eq!(files[1].status, 'M');
    assert_eq!(files[1].additions, 1);
    assert_eq!(files[1].deletions, 1);
    assert_eq!(files[2].status, 'A');
    assert_eq!(files[2].additions, 2);
    assert_eq!(files[2].deletions, 0);
}

#[test]
fn index_file_diff_compares_staged_blob_with_worktree() {
    let dir = init();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "base\n").unwrap();
    git(root, &["add", "a.txt"]);
    git(root, &["commit", "-q", "-m", "base"]);
    std::fs::write(root.join("a.txt"), "staged\n").unwrap();
    git(root, &["add", "a.txt"]);
    std::fs::write(root.join("a.txt"), "worktree\n").unwrap();

    let staged = lazecode_git::index_file_diff(root, "a.txt").unwrap();
    assert_eq!(staged.old_text, "staged\n");
    assert_eq!(staged.new_text, "worktree\n");
    assert!(!staged.diff.hunks.is_empty());

    let head = lazecode_git::file_diff(root, "a.txt").unwrap();
    assert_eq!(head.old_text, "base\n");
    assert_eq!(head.new_text, "worktree\n");
}

#[test]
fn checkout_revision_path_accepts_symbolic_revisions() {
    let dir = init();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "v1\n").unwrap();
    git(root, &["add", "a.txt"]);
    git(root, &["commit", "-q", "-m", "one"]);
    let first = git(root, &["rev-parse", "HEAD"]);
    std::fs::write(root.join("a.txt"), "v2\n").unwrap();
    git(root, &["add", "a.txt"]);
    git(root, &["commit", "-q", "-m", "two"]);

    lazecode_git::checkout_revision_path(root, &first, "a.txt").unwrap();
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "v1\n");
    lazecode_git::checkout_revision_path(root, "HEAD", "a.txt").unwrap();
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "v2\n");
    lazecode_git::checkout_revision_path(root, "HEAD~1", "a.txt").unwrap();
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "v1\n");
}

#[test]
fn checkout_refuses_branch_checked_out_in_other_worktree() {
    let dir = init();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "base\n").unwrap();
    git(root, &["add", "a.txt"]);
    git(root, &["commit", "-q", "-m", "base"]);
    lazecode_git::create_branch(root, "side", false).unwrap();
    let wt_dest = dir.path().join("linked-wt");
    lazecode_git::add_worktree(root, &wt_dest, "side").unwrap();

    // Trying to check out "side" from the main worktree must fail
    let err = lazecode_git::checkout_branch(root, "side")
        .unwrap_err()
        .to_string();
    assert!(err.contains("already checked out in worktree"), "{err}");

    // Trying to delete "side" while checked out in linked worktree must also fail
    let del_err = lazecode_git::delete_branch(root, "side")
        .unwrap_err()
        .to_string();
    assert!(del_err.contains("checked out in worktree"), "{del_err}");
}

#[test]
fn discard_restores_tracked_and_drops_untracked_without_git_cli() {
    let dir = init();
    std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
    lazecode_git::stage(dir.path(), &["a.txt".into()]).unwrap();
    lazecode_git::commit(dir.path(), "base", false).unwrap();

    std::fs::write(dir.path().join("a.txt"), "changed\n").unwrap();
    lazecode_git::discard_file_changes(dir.path(), "a.txt").unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "one\n"
    );
    assert!(lazecode_git::snapshot(dir.path())
        .unwrap()
        .changes
        .is_empty());

    std::fs::create_dir(dir.path().join("tmp")).unwrap();
    std::fs::write(dir.path().join("tmp/untracked.txt"), "x\n").unwrap();
    lazecode_git::stage(dir.path(), &["tmp/untracked.txt".into()]).unwrap();
    lazecode_git::discard_file_changes(dir.path(), "tmp/untracked.txt").unwrap();
    assert!(!dir.path().join("tmp/untracked.txt").exists());
    assert!(lazecode_git::snapshot(dir.path())
        .unwrap()
        .changes
        .is_empty());
    lazecode_git::discard_file_changes(dir.path(), "tmp").unwrap();
    assert!(!dir.path().join("tmp").exists());
}

#[test]
fn git_init_uses_gix_and_produces_a_usable_repository() {
    let dir = tempfile::tempdir().unwrap();
    lazecode_git::git_init(dir.path()).unwrap();
    git(dir.path(), &["config", "user.name", "Tester"]);
    git(dir.path(), &["config", "user.email", "tester@example.com"]);
    std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
    lazecode_git::stage(dir.path(), &["a.txt".into()]).unwrap();
    lazecode_git::commit(dir.path(), "initial", false).unwrap();
    assert_eq!(
        lazecode_git::snapshot(dir.path())
            .unwrap()
            .head
            .unwrap()
            .message,
        "initial"
    );
}

#[test]
fn ahead_behind_uses_configured_upstream_not_only_origin() {
    let dir = init();
    std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
    lazecode_git::stage(dir.path(), &["a.txt".into()]).unwrap();
    lazecode_git::commit(dir.path(), "base", false).unwrap();
    let base = git(dir.path(), &["rev-parse", "HEAD"]);
    std::fs::write(dir.path().join("a.txt"), "two\n").unwrap();
    lazecode_git::stage(dir.path(), &["a.txt".into()]).unwrap();
    lazecode_git::commit(dir.path(), "next", false).unwrap();
    git(
        dir.path(),
        &["update-ref", "refs/remotes/upstream/main", &base],
    );
    git(dir.path(), &["config", "branch.main.remote", "upstream"]);
    git(
        dir.path(),
        &["config", "branch.main.merge", "refs/heads/main"],
    );
    let snapshot = lazecode_git::snapshot(dir.path()).unwrap();
    assert_eq!(snapshot.ahead, 1);
    assert_eq!(snapshot.behind, 0);
}

#[test]
fn ignored_entries_match_git_ignored_matching() {
    let dir = init();
    std::fs::write(dir.path().join(".gitignore"), "ignored.txt\ntarget/\n").unwrap();
    std::fs::write(dir.path().join("tracked.txt"), "ok\n").unwrap();
    std::fs::write(dir.path().join("ignored.txt"), "no\n").unwrap();
    std::fs::create_dir_all(dir.path().join("target/debug")).unwrap();
    std::fs::write(dir.path().join("target/debug/out"), "no\n").unwrap();
    lazecode_git::stage(dir.path(), &[".gitignore".into(), "tracked.txt".into()]).unwrap();
    lazecode_git::commit(dir.path(), "base", false).unwrap();
    let (all, dirs) = lazecode_git::ignored_entries(dir.path()).unwrap();
    assert_eq!(all, vec!["ignored.txt".to_string(), "target".to_string()]);
    assert_eq!(dirs, vec!["target".to_string()]);
}

#[test]
fn ahead_behind_cache_invalidates_when_upstream_moves() {
    let dir = init();
    std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
    lazecode_git::stage(dir.path(), &["a.txt".into()]).unwrap();
    lazecode_git::commit(dir.path(), "base", false).unwrap();
    let base = git(dir.path(), &["rev-parse", "HEAD"]);
    std::fs::write(dir.path().join("a.txt"), "two\n").unwrap();
    lazecode_git::stage(dir.path(), &["a.txt".into()]).unwrap();
    lazecode_git::commit(dir.path(), "next", false).unwrap();
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    git(
        dir.path(),
        &["update-ref", "refs/remotes/origin/main", &base],
    );
    let first = lazecode_git::snapshot(dir.path()).unwrap();
    assert_eq!(first.ahead, 1);
    git(
        dir.path(),
        &["update-ref", "refs/remotes/origin/main", &head],
    );
    let second = lazecode_git::snapshot(dir.path()).unwrap();
    assert_eq!(second.ahead, 0);
    assert_eq!(second.behind, 0);
}

#[test]
fn ignored_snapshot_cache_invalidates_explicitly() {
    let dir = init();
    std::fs::write(dir.path().join(".gitignore"), "*.tmp\n").unwrap();
    std::fs::write(dir.path().join("a.tmp"), "no\n").unwrap();
    let first = lazecode_git::snapshot(dir.path()).unwrap();
    assert_eq!(first.ignored_files, vec!["a.tmp".to_string()]);
    std::fs::write(dir.path().join("b.tmp"), "no\n").unwrap();
    let cached = lazecode_git::snapshot(dir.path()).unwrap();
    assert!(!cached.ignored_files.iter().any(|path| path == "b.tmp"));
    lazecode_git::invalidate_ignored_cache(dir.path());
    let fresh = lazecode_git::snapshot(dir.path()).unwrap();
    assert!(fresh.ignored_files.iter().any(|path| path == "b.tmp"));
}

#[test]
fn commit_checked_accepts_resolve_undo_index() {
    let dir = init();
    std::fs::write(dir.path().join("a.txt"), "base\n").unwrap();
    lazecode_git::stage(dir.path(), &["a.txt".into()]).unwrap();
    lazecode_git::commit(dir.path(), "base", false).unwrap();

    git(dir.path(), &["checkout", "-q", "-b", "topic"]);
    std::fs::write(dir.path().join("a.txt"), "topic\n").unwrap();
    git(dir.path(), &["commit", "-qam", "topic"]);

    git(dir.path(), &["checkout", "-q", "main"]);
    std::fs::write(dir.path().join("a.txt"), "main\n").unwrap();
    git(dir.path(), &["commit", "-qam", "main"]);

    let merge = Command::new("git")
        .current_dir(dir.path())
        .args(["merge", "topic"])
        .output()
        .unwrap();
    assert!(!merge.status.success(), "merge should conflict");

    std::fs::write(dir.path().join("a.txt"), "resolved\n").unwrap();
    git(dir.path(), &["add", "a.txt"]);
    let index = std::fs::read(dir.path().join(".git/index")).unwrap();
    assert!(
        index.windows(4).any(|window| window == b"REUC"),
        "the fixture must carry the resolve-undo extension"
    );
    // The merge has been resolved by hand; remove the remaining sequencer
    // marker so LazeCode's normal commit path (not merge continue) runs.
    let _ = std::fs::remove_file(dir.path().join(".git/MERGE_HEAD"));
    let _ = std::fs::remove_file(dir.path().join(".git/MERGE_MSG"));
    let _ = std::fs::remove_file(dir.path().join(".git/MERGE_MODE"));

    lazecode_git::commit_checked(dir.path(), &["a.txt".into()], &[], "resolved", false)
        .expect("commit must tolerate REUC instead of refusing the index");
    assert_eq!(git(dir.path(), &["status", "--porcelain"]), "");
}

#[test]
fn index_only_divergence_matching_disk_is_hidden() {
    let dir = init();
    std::fs::write(dir.path().join("a.txt"), "new\n").unwrap();
    std::fs::write(dir.path().join("b.txt"), "kept\n").unwrap();
    lazecode_git::stage(dir.path(), &["a.txt".into(), "b.txt".into()]).unwrap();
    lazecode_git::commit(dir.path(), "base", false).unwrap();

    // Stage stale content, then restore the disk file to HEAD: the staged and
    // unstaged halves cancel out, so JetBrains-style semantics list nothing.
    std::fs::write(dir.path().join("a.txt"), "old\n").unwrap();
    lazecode_git::stage(dir.path(), &["a.txt".into()]).unwrap();
    std::fs::write(dir.path().join("a.txt"), "new\n").unwrap();
    let snap = lazecode_git::snapshot(dir.path()).unwrap();
    assert!(
        snap.changes.is_empty(),
        "phantom staged entry listed: {:?}",
        snap.changes
            .iter()
            .map(|c| c.path.clone())
            .collect::<Vec<_>>()
    );
    let diff = lazecode_git::file_diff(dir.path(), "a.txt").unwrap();
    assert_eq!(diff.diff.additions, 0);

    // Index-only deletion with the disk file still at HEAD stays hidden too,
    // including the untracked walk.
    git(dir.path(), &["rm", "--cached", "-qf", "a.txt"]);
    let snap = lazecode_git::snapshot(dir.path()).unwrap();
    assert!(
        snap.changes.is_empty(),
        "phantom staged deletion listed: {:?}",
        snap.changes
            .iter()
            .map(|c| c.path.clone())
            .collect::<Vec<_>>()
    );

    // Real net changes still list, staged or not.
    std::fs::write(dir.path().join("b.txt"), "kept\nedited\n").unwrap();
    let snap = lazecode_git::snapshot(dir.path()).unwrap();
    assert_eq!(snap.changes.len(), 1);
    assert!(snap.changes[0].unstaged.is_some() && snap.changes[0].staged.is_none());
    lazecode_git::stage(dir.path(), &["b.txt".into()]).unwrap();
    let snap = lazecode_git::snapshot(dir.path()).unwrap();
    assert_eq!(snap.changes.len(), 1);
    assert!(snap.changes[0].staged.is_some() && snap.changes[0].unstaged.is_none());
}

#[test]
fn checked_commit_rolls_back_index_when_ref_update_fails() {
    let dir = init();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "one\n").unwrap();
    lazecode_git::stage(root, &["a.txt".into()]).unwrap();
    lazecode_git::commit(root, "base", false).unwrap();
    let base = git(root, &["rev-parse", "HEAD"]);

    std::fs::write(root.join("a.txt"), "two\n").unwrap();
    lazecode_git::stage(root, &["a.txt".into()]).unwrap();

    let lock = root.join(".git/refs/heads/main.lock");
    std::fs::write(&lock, "held by another writer\n").unwrap();
    let error = lazecode_git::commit_checked(root, &["a.txt".into()], &[], "checked", false)
        .unwrap_err()
        .to_string();
    std::fs::remove_file(&lock).unwrap();
    assert!(!error.is_empty(), "ref update must fail");
    assert_eq!(git(root, &["rev-parse", "HEAD"]), base);
    assert!(
        git(root, &["diff", "--cached", "a.txt"]).contains("+two"),
        "the staged change must survive a failed ref update"
    );
    assert!(
        git(root, &["diff", "a.txt"]).is_empty(),
        "worktree must still match the restored index"
    );
}

#[test]
fn stage_holds_the_index_lock_across_read_modify_write() {
    use std::thread;
    use std::time::{Duration, Instant};

    let dir = init();
    let root = dir.path();
    std::fs::write(root.join("base.txt"), "base\n").unwrap();
    lazecode_git::stage(root, &["base.txt".into()]).unwrap();
    lazecode_git::commit(root, "base", false).unwrap();

    // A slow clean filter gives deterministic visibility into the window
    // between reading the index and writing it back.
    git(root, &["config", "filter.slow.clean", "sleep 3; cat"]);
    std::fs::write(root.join(".gitattributes"), "*.slow filter=slow\n").unwrap();
    std::fs::write(root.join("a.slow"), "a\n").unwrap();
    std::fs::write(root.join("b.txt"), "b\n").unwrap();
    lazecode_git::stage(root, &[".gitattributes".into()]).unwrap();

    let root_path = root.to_string_lossy().into_owned();
    let first = thread::spawn(move || {
        lazecode_git::stage(Path::new(&root_path), &["a.slow".into()]).unwrap();
    });

    let lock = root.join(".git/index.lock");
    let start = Instant::now();
    while !lock.exists() && start.elapsed() < Duration::from_secs(2) {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(lock.exists(), "stage must hold index.lock while filtering");

    let second = lazecode_git::stage(root, &["b.txt".into()]);
    assert!(
        second.is_err(),
        "a concurrent writer must fail instead of losing one of the updates"
    );
    first.join().unwrap();

    let snap = lazecode_git::snapshot(root).unwrap();
    let staged: Vec<&str> = snap
        .changes
        .iter()
        .filter(|change| change.staged.is_some())
        .map(|change| change.path.as_str())
        .collect();
    assert!(
        staged.contains(&"a.slow"),
        "the first writer's index update must survive: {:?}",
        staged
    );
    assert!(
        !staged.contains(&"b.txt"),
        "the rejected writer must not mutate the index: {:?}",
        staged
    );
}
