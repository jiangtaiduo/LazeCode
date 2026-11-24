use lazecode_git::apply_patch;
use std::process::Command;

fn git(dir: &std::path::Path, args: &[&str]) -> String {
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
    std::fs::write(dir.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);
    dir
}

#[test]
fn apply_patch_round_trip_worktree_and_index() {
    let dir = repo();
    let root = dir.path();
    let patch = "--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n";

    // Worktree apply.
    apply_patch(root, patch, false, false).unwrap();
    let worktree = std::fs::read_to_string(root.join("a.txt")).unwrap();
    assert!(worktree.contains("TWO"));

    // Reverse apply rolls it back.
    apply_patch(root, patch, false, true).unwrap();
    let rolled = std::fs::read_to_string(root.join("a.txt")).unwrap();
    assert!(rolled.contains("two") && !rolled.contains("TWO"));

    // Index-only (cached) apply leaves the worktree alone.
    apply_patch(root, patch, true, false).unwrap();
    assert!(std::fs::read_to_string(root.join("a.txt"))
        .unwrap()
        .contains("two"));
    let staged = git(root, &["diff", "--cached"]);
    assert!(staged.contains("TWO"), "{staged}");
}

#[test]
fn apply_patch_rejects_broken_and_empty() {
    let dir = repo();
    apply_patch(dir.path(), "", false, false).unwrap_err();
    let broken = "--- a/a.txt\n+++ b/a.txt\n@@ -999,1 +1,1 @@\n x\n+y\n";
    let err = apply_patch(dir.path(), broken, false, false)
        .unwrap_err()
        .to_string();
    assert!(err.contains("does not apply"), "{err}");
}

#[test]
fn generated_patch_reverts_a_no_newline_file() {
    let dir = repo();
    let path = dir.path().join("testbb.txt");
    std::fs::write(&path, b"123123").unwrap();
    let patch = lazecode_text::unified_patch("testbb.txt", &lazecode_text::diff_text("", "123123"));
    apply_patch(dir.path(), &patch, false, true).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"");
}
