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

fn modify_patch(old: &str, new: &str) -> String {
    format!(
        "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n one\n-{old}\n+{new}\n"
    )
}

#[test]
fn applies_modify_add_delete_and_reverse() {
    let dir = init();
    std::fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();
    std::fs::write(dir.path().join("del.txt"), "bye\n").unwrap();
    lazecode_git::stage(dir.path(), &["a.txt".into(), "del.txt".into()]).unwrap();
    lazecode_git::commit(dir.path(), "base", false).unwrap();

    let patch = format!(
        "{}\
         diff --git a/new.txt b/new.txt\n\
         new file mode 100644\n\
         --- /dev/null\n\
         +++ b/new.txt\n\
         @@ -0,0 +1,2 @@\n\
         +hello\n\
         +world\n\
         diff --git a/del.txt b/del.txt\n\
         deleted file mode 100644\n\
         --- a/del.txt\n\
         +++ /dev/null\n\
         @@ -1 +0,0 @@\n\
         -bye\n",
        modify_patch("two", "TWO")
    );
    lazecode_git::apply_patch(dir.path(), &patch, false, false).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "one\nTWO\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("new.txt")).unwrap(),
        "hello\nworld\n"
    );
    assert!(!dir.path().join("del.txt").exists());

    lazecode_git::apply_patch(dir.path(), &patch, false, true).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "one\ntwo\n"
    );
    assert!(!dir.path().join("new.txt").exists());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("del.txt")).unwrap(),
        "bye\n"
    );
}

#[test]
fn cached_patch_updates_index_without_touching_worktree() {
    let dir = init();
    std::fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();
    lazecode_git::stage(dir.path(), &["a.txt".into()]).unwrap();
    lazecode_git::commit(dir.path(), "base", false).unwrap();
    lazecode_git::apply_patch(dir.path(), &modify_patch("two", "TWO"), true, false).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "one\ntwo\n"
    );
    let snap = lazecode_git::snapshot(dir.path()).unwrap();
    assert_eq!(snap.changes.len(), 1);
    assert!(snap.changes[0].staged.is_some());
    lazecode_git::commit(dir.path(), "cached", false).unwrap();
    assert_eq!(
        git(dir.path(), &["cat-file", "-p", "HEAD:a.txt"]),
        "one\nTWO"
    );
}

#[test]
fn no_newline_markers_round_trip_and_bad_context_is_atomic() {
    let dir = init();
    std::fs::write(dir.path().join("a.txt"), "old").unwrap();
    let patch = "--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n";
    lazecode_git::apply_patch(dir.path(), patch, false, false).unwrap();
    assert_eq!(std::fs::read(dir.path().join("a.txt")).unwrap(), b"new");

    let error = lazecode_git::apply_patch(dir.path(), &modify_patch("nope", "x"), false, false)
        .unwrap_err();
    assert!(error.to_string().contains("does not apply"), "{error}");
    assert_eq!(std::fs::read(dir.path().join("a.txt")).unwrap(), b"new");
}

#[test]
fn mode_only_patch_reverses() {
    use std::os::unix::fs::PermissionsExt;
    let dir = init();
    std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
    let patch = "--- a/a.txt\n+++ b/a.txt\nold mode 100644\nnew mode 100755\n";
    lazecode_git::apply_patch(dir.path(), patch, false, false).unwrap();
    let mode = std::fs::metadata(dir.path().join("a.txt"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o755);
    lazecode_git::apply_patch(dir.path(), patch, false, true).unwrap();
    let mode = std::fs::metadata(dir.path().join("a.txt"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o644);
}
