use lazecode_git::{
    blame, compare_commits, file_history, shelf_delete, shelf_list, shelf_patch, shelf_restore,
    shelf_restore_partial, shelf_save, stash_apply, stash_drop, stash_files, stash_list,
    stash_push, submodule_list,
};
use std::path::Path;
use std::process::Command;

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
    git(dir.path(), &["config", "user.name", "T"]);
    git(dir.path(), &["config", "user.email", "t@e"]);
    std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-q", "-m", "c1"]);
    dir
}

#[test]
fn stash_round_trip() {
    let dir = repo();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
    stash_push(root, Some("work in progress"), false).unwrap();
    let list = stash_list(root).unwrap();
    assert_eq!(list.len(), 1);
    assert!(list[0].subject.contains("work in progress"));
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "one\n"
    );
    let files = stash_files(root, "stash@{0}").unwrap();
    assert_eq!(files, vec!["a.txt".to_string()]);
    stash_apply(root, "stash@{0}").unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "one\ntwo\n"
    );
    stash_drop(root, "stash@{0}").unwrap();
    assert!(stash_list(root).unwrap().is_empty());
}

#[test]
fn shelf_save_restore_partial_and_delete() {
    let dir = repo();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
    std::fs::write(root.join("b.txt"), "beta\n").unwrap();
    git(root, &["add", "b.txt"]);
    shelf_save(root, "wip").unwrap();
    // changes rolled out of the worktree
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "one\n"
    );
    let list = shelf_list(root).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "wip");
    assert!(shelf_patch(root, "wip").unwrap().contains("a.txt"));
    assert!(shelf_patch(root, "wip").unwrap().contains("b.txt"));

    // full restore brings both changes back
    let disk_bytes = std::fs::read(root.join(".git/lazecode-shelves/wip.patch"))
        .unwrap()
        .len();
    eprintln!("DISK SHELF wip bytes={disk_bytes}");
    eprintln!(
        "STSH: {STSH}",
        STSH = String::from_utf8_lossy(
            &Command::new("git")
                .current_dir(root)
                .args(["status", "--short"])
                .output()
                .unwrap()
                .stdout
        )
        .trim_end()
        .replace('\n', " | ")
    );
    eprintln!(
        "LSF: {LSF}",
        LSF = String::from_utf8_lossy(
            &Command::new("git")
                .current_dir(root)
                .args(["ls-files", "-s"])
                .output()
                .unwrap()
                .stdout
        )
        .trim_end()
        .replace('\n', " | ")
    );
    shelf_restore(root, "wip", false).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "one\ntwo\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("b.txt")).unwrap(),
        "beta\n"
    );
    // Roll them back out again, then partial-restore the b.txt hunk only.
    shelf_save(root, "wip2").unwrap();
    // Shelving retracts all changes: b.txt (a new file on the shelf) is gone.
    assert!(!root.join("b.txt").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "one\n"
    );
    shelf_restore_partial(root, "wip2", &["b.txt".to_string()]).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("b.txt")).unwrap(),
        "beta\n"
    );
    // a.txt stays untouched after the partial restore.
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "one\n"
    );

    shelf_delete(root, "wip").unwrap();
    shelf_delete(root, "wip2").unwrap();
    assert!(shelf_list(root).unwrap().is_empty());
}

#[test]
fn blame_and_file_history() {
    let dir = repo();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "one\nline two\n").unwrap();
    git(root, &["commit", "-qam", "second"]);
    std::fs::write(root.join("a.txt"), "one\nline two\nthird line\n").unwrap();
    git(root, &["commit", "-qam", "third"]);

    let history = file_history(root, "a.txt", 10).unwrap();
    assert!(history.len() >= 2, "{history:?}");
    assert_eq!(history[0].subject, "third");

    let blame = blame(root, "a.txt").unwrap();
    assert_eq!(blame.len(), 3, "one row per line: {blame:?}");
    assert_eq!(blame[0].line, 1);
    assert_eq!(blame[1].line, 2);
    assert_eq!(blame[2].line, 3);
    // Both authors from git config.
    assert!(blame.iter().all(|entry| entry.author == "T"));
}

#[test]
fn submodule_list_and_compare() {
    let dir = repo();
    let root = dir.path();
    assert!(submodule_list(root).unwrap().is_empty());

    let head = git(root, &["rev-parse", "HEAD"]);
    std::fs::write(root.join("b.txt"), "new\n").unwrap();
    git(root, &["add", "b.txt"]);
    git(root, &["commit", "-q", "-m", "second"]);
    let new_head = git(root, &["rev-parse", "HEAD"]);
    let files = compare_commits(root, &head, &new_head).unwrap();
    assert_eq!(files, vec![("b.txt".to_string(), "A".to_string())]);
}

#[test]
fn compare_branches_both_directions() {
    let dir = repo();
    let root = dir.path();
    // base commit; feature adds its file; main adds its own.
    git(root, &["checkout", "-qb", "feature"]);
    std::fs::write(root.join("feat.txt"), "feature\n").unwrap();
    git(root, &["add", "feat.txt"]);
    git(root, &["commit", "-q", "-m", "feature"]);
    git(root, &["checkout", "-q", "main"]);
    std::fs::write(root.join("main.txt"), "main\n").unwrap();
    git(root, &["add", "main.txt"]);
    git(root, &["commit", "-q", "-m", "main"]);

    let (left, right) = lazecode_git::compare_branches(root, "main", "feature").unwrap();
    // main-only (base..main)
    assert!(left.commitset_contains("main.txt"), "{left:?}");
    assert!(right.files.iter().any(|(path, _)| path == "feat.txt"));
    // Both sides have exactly one unique commit.
    assert_eq!(left.commits.len(), 1);
    assert_eq!(right.commits.len(), 1);
}

trait Contains {
    fn commitset_contains(&self, path: &str) -> bool;
}

impl Contains for lazecode_git::CompareSide {
    fn commitset_contains(&self, path: &str) -> bool {
        self.files.iter().any(|(entry, _)| entry == path)
    }
}
