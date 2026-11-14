use std::path::Path;
use std::process::Command;

use lazecode_git::{
    checkout_branch, create_branch_from, create_tag, delete_branch, delete_tag,
    force_delete_branch, rename_branch,
};

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
    std::fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);
    dir
}

#[test]
fn branch_rename_updates_head() {
    let dir = repo();
    let root = dir.path();
    assert_eq!(git(root, &["branch", "--show-current"]), "main");
    rename_branch(root, "main", "trunk").unwrap();
    assert_eq!(git(root, &["branch", "--show-current"]), "trunk");
    // Old ref gone.
    let refs = git(root, &["for-each-ref", "refs/heads/"]);
    assert!(!refs.contains("main"));
    assert!(refs.contains("trunk"));
    // Duplicate rename must fail.
    rename_branch(root, "trunk", "trunk").unwrap_err();
    // Rename onto a fresh name succeeds.
    rename_branch(root, "trunk", "main").unwrap();
    assert_eq!(git(root, &["branch", "--show-current"]), "main");
}

#[test]
fn tag_create_list_delete_round_trip() {
    let dir = repo();
    let root = dir.path();
    create_tag(root, "v0.1.0", "", None).unwrap();
    let tags = git(root, &["tag", "--list"]);
    assert!(tags.contains("v0.1.0"));
    // Tag points at HEAD.
    let head = git(root, &["rev-parse", "HEAD"]);
    let tag_target = git(root, &["rev-parse", "v0.1.0"]);
    assert_eq!(head, tag_target);
    // Duplicate name errors (must-exist-or-not semantics differ for update).
    create_tag(root, "v0.1.0", "", None).unwrap_err();
    delete_tag(root, "v0.1.0").unwrap();
    let tags = git(root, &["tag", "--list"]);
    assert!(!tags.contains("v0.1.0"));
    // Tagging another commit by hash works.
    let head = git(root, &["rev-parse", "HEAD"]);
    create_tag(root, "release-1", &head, None).unwrap();
    assert!(git(root, &["rev-parse", "release-1"]) == head);
}

#[test]
fn delete_branch_rejects_current() {
    let dir = repo();
    let root = dir.path();
    git(root, &["branch", "side"]);
    git(root, &["checkout", "-q", "side"]);
    let err = delete_branch(root, "side").unwrap_err().to_string();
    assert!(err.contains("cannot delete the current branch"), "{err}");
    git(root, &["checkout", "-q", "main"]);
    delete_branch(root, "side").unwrap();
    let refs = git(root, &["for-each-ref", "refs/heads/"]);
    assert!(!refs.contains("side"));
}

#[test]
fn create_branch_from_start_point_can_checkout() {
    let dir = repo();
    let root = dir.path();
    let base = git(root, &["rev-parse", "HEAD"]);
    std::fs::write(root.join("b.txt"), "second\n").unwrap();
    git(root, &["add", "b.txt"]);
    git(root, &["commit", "-q", "-m", "second"]);

    create_branch_from(root, "from-base", Some(&base), true).unwrap();
    assert_eq!(git(root, &["branch", "--show-current"]), "from-base");
    assert_eq!(git(root, &["rev-parse", "HEAD"]), base);
    assert!(!root.join("b.txt").exists());
}

#[test]
fn delete_unmerged_branch_requires_force() {
    let dir = repo();
    let root = dir.path();
    git(root, &["checkout", "-q", "-b", "side"]);
    std::fs::write(root.join("side.txt"), "side\n").unwrap();
    git(root, &["add", "side.txt"]);
    git(root, &["commit", "-q", "-m", "side"]);
    git(root, &["checkout", "-q", "main"]);

    let err = delete_branch(root, "side").unwrap_err().to_string();
    assert!(err.contains("not fully merged"), "{err}");
    assert!(!git(root, &["for-each-ref", "refs/heads/"]).is_empty());
    force_delete_branch(root, "side").unwrap();
    assert!(!git(root, &["for-each-ref", "refs/heads/side"]).contains("side"));
}

#[test]
fn checkout_remote_branch_creates_local_tracking_branch() {
    let dir = repo();
    let root = dir.path();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "-q", "--bare"]);
    git(
        root,
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(root, &["push", "-q", "origin", "main"]);
    git(root, &["branch", "docs-only"]);
    git(root, &["push", "-q", "origin", "docs-only"]);
    git(root, &["branch", "-D", "docs-only"]);
    git(root, &["fetch", "-q", "origin"]);

    checkout_branch(root, "origin/docs-only").unwrap();
    assert_eq!(git(root, &["branch", "--show-current"]), "docs-only");
    assert_eq!(
        git(root, &["rev-parse", "docs-only@{upstream}"]),
        git(root, &["rev-parse", "origin/docs-only"])
    );
}
