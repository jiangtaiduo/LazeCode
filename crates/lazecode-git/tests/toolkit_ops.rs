use lazecode_git::{
    blame, compare_commits, diff_patch_paths, file_history, shelf_delete, shelf_files, shelf_list,
    shelf_patch, shelf_rename, shelf_restore, shelf_restore_partial, shelf_save, shelf_save_paths,
    stash_apply, stash_drop, stash_files, stash_list, stash_pop, stash_push, submodule_list,
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
    stash_push(root, Some("work in progress"), false, false).unwrap();
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
    assert_eq!(
        files,
        vec![lazecode_git::CompareFile {
            path: "b.txt".to_string(),
            status: 'A',
            additions: 1,
            deletions: 0,
        }]
    );
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

    let result = lazecode_git::compare_branches(root, "main", "feature", None, None, 1000).unwrap();
    // merge-base anchored.
    assert!(result.merge_base.is_some());
    // main-only side lists main.txt with numstat; feature side lists feat.txt.
    let main_file = result
        .left
        .files
        .iter()
        .find(|file| file.path == "main.txt")
        .expect("left side carries main.txt");
    assert_eq!(main_file.status, 'A');
    assert_eq!(main_file.additions, 1);
    let feat_file = result
        .right
        .files
        .iter()
        .find(|file| file.path == "feat.txt")
        .expect("right side carries feat.txt");
    assert_eq!(feat_file.status, 'A');
    // Both sides have exactly one unique commit, and not truncated.
    assert_eq!(result.left.commits.len(), 1);
    assert_eq!(result.right.commits.len(), 1);
    assert!(!result.left.truncated);
    assert!(!result.right.truncated);
    // path 过滤：限定 feat.txt 后右侧文件清单只剩它，左侧为空。
    let filtered =
        lazecode_git::compare_branches(root, "main", "feature", None, Some("feat.txt"), 1000)
            .unwrap();
    assert!(filtered
        .right
        .files
        .iter()
        .all(|file| file.path == "feat.txt"));
    // 极小 limit：提交列表截断并被裁到 limit。
    let tiny = lazecode_git::compare_branches(root, "main", "feature", None, None, 1).unwrap();
    assert_eq!(tiny.left.commits.len(), 1);
}

#[test]
fn commit_files_reports_numstat_and_status() {
    let dir = repo();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "c1"]);
    std::fs::write(root.join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
    std::fs::write(root.join("new.txt"), "hello\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "c2"]);
    let head = git(root, &["rev-parse", "HEAD"]);
    let files = lazecode_git::commit_files(root, &head).unwrap();
    let modified = files
        .iter()
        .find(|file| file.path == "a.txt")
        .expect("a.txt in commit");
    assert_eq!(modified.status, 'M');
    assert_eq!(modified.additions, 2);
    let added = files
        .iter()
        .find(|file| file.path == "new.txt")
        .expect("new.txt in commit");
    assert_eq!(added.status, 'A');
}

#[test]
fn shelf_save_paths_scopes_to_selected_files_including_untracked() {
    let dir = repo();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
    std::fs::write(root.join("c.txt"), "untracked\n").unwrap();
    // Only shelve a.txt (tracked) and c.txt (untracked) — the stable
    // version's "shelve the checked files" semantics.
    shelf_save_paths(root, "sel", &["a.txt".to_string(), "c.txt".to_string()]).unwrap();
    // Both retracted from the worktree.
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "one\n"
    );
    assert!(!root.join("c.txt").exists());
    // The patch carries both sections.
    let patch = shelf_patch(root, "sel").unwrap();
    assert!(patch.contains("a.txt"), "patch misses tracked path");
    assert!(patch.contains("c.txt"), "patch misses untracked path");
    assert_eq!(shelf_files(root, "sel").unwrap(), vec!["a.txt", "c.txt"]);
    // Partial restore of the untracked section recreates the file.
    shelf_restore_partial(root, "sel", &["c.txt".to_string()]).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("c.txt")).unwrap(),
        "untracked\n"
    );
    shelf_delete(root, "sel").unwrap();
}

#[test]
fn diff_patch_paths_includes_untracked_files() {
    let dir = repo();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
    std::fs::write(root.join("c.txt"), "fresh\n").unwrap();
    let patch = diff_patch_paths(root, &["a.txt".to_string(), "c.txt".to_string()]).unwrap();
    assert!(patch.contains("diff --git"));
    assert!(patch.contains("a.txt"));
    assert!(patch.contains("c.txt"));
    // Empty selection yields an empty patch.
    assert!(diff_patch_paths(root, &[]).unwrap().is_empty());
}

#[test]
fn stash_push_can_include_untracked_files() {
    let dir = repo();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
    std::fs::write(root.join("new.txt"), "brand new\n").unwrap();
    stash_push(root, Some("with untracked"), false, true).unwrap();
    assert!(
        !root.join("new.txt").exists(),
        "untracked file must be stashed away"
    );
    let files = stash_files(root, "stash@{0}").unwrap();
    assert!(files.contains(&"new.txt".to_string()));
    stash_apply(root, "stash@{0}").unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("new.txt")).unwrap(),
        "brand new\n"
    );
    stash_drop(root, "stash@{0}").unwrap();
}

#[test]
fn shelf_names_cannot_escape_the_shelf_directory() {
    let dir = repo();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
    shelf_save(root, "safe").unwrap();
    // Rename/read/save with traversal segments must be rejected before any
    // file outside `.git/lazecode-shelves` is touched.
    assert!(shelf_rename(root, "safe", "../pwn").is_err());
    assert!(shelf_patch(root, "../anything").is_err());
    assert!(!root.join(".git/pwn.patch").exists());
    assert!(shelf_rename(root, "safe", "renamed").is_ok());
    assert!(shelf_patch(root, "renamed").unwrap().contains("a.txt"));
}

#[test]
fn lfs_status_and_track_use_native_attributes() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new("git")
        .current_dir(dir.path())
        .args(["init", "-q", "-b", "main"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let status = lazecode_git::lfs_status(dir.path());
    assert!(status.installed);
    assert!(status.patterns.is_empty());
    assert_eq!(
        lazecode_git::lfs_track(dir.path(), "*.bin").unwrap(),
        "Tracking \"*.bin\""
    );
    let attributes = std::fs::read_to_string(dir.path().join(".gitattributes")).unwrap();
    assert_eq!(attributes, "*.bin filter=lfs diff=lfs merge=lfs -text\n");
    let status = lazecode_git::lfs_status(dir.path());
    assert_eq!(status.patterns, vec!["*.bin".to_string()]);
    assert_eq!(
        lazecode_git::lfs_track(dir.path(), "*.bin").unwrap(),
        "Already tracking \"*.bin\""
    );
    assert!(lazecode_git::lfs_track(dir.path(), "a b").is_err());
}

#[test]
fn native_stash_is_git_compatible_both_ways() {
    let dir = repo();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
    stash_push(root, Some("ours"), false, false).unwrap();
    assert!(git(root, &["stash", "list", "--format=%gs"]).contains("ours"));
    git(root, &["stash", "apply", "-q", "stash@{0}"]);
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "one\ntwo\n"
    );
    git(root, &["checkout", "--", "a.txt"]);

    std::fs::write(root.join("a.txt"), "one\nthree\n").unwrap();
    git(root, &["stash", "push", "-q", "-m", "theirs"]);
    let list = stash_list(root).unwrap();
    assert!(list[0].subject.contains("theirs"), "{list:?}");
    stash_apply(root, "stash@{0}").unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "one\nthree\n"
    );
    stash_drop(root, "stash@{0}").unwrap();
    assert!(!stash_list(root)
        .unwrap()
        .iter()
        .any(|s| s.subject.contains("theirs")));
}

#[test]
fn stash_drop_middle_reindexes_and_apply_works() {
    let dir = repo();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
    stash_push(root, Some("first"), false, false).unwrap();
    std::fs::write(root.join("a.txt"), "one\nthree\n").unwrap();
    stash_push(root, Some("second"), false, false).unwrap();
    let list = stash_list(root).unwrap();
    assert_eq!(list.len(), 2);
    assert!(list[0].subject.contains("second"));
    assert!(list[1].subject.contains("first"));

    stash_drop(root, "stash@{1}").unwrap();
    let list = stash_list(root).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "stash@{0}");
    assert!(list[0].subject.contains("second"));
    stash_apply(root, "stash@{0}").unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "one\nthree\n"
    );
}

#[test]
fn stash_pop_applies_then_drops() {
    let dir = repo();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "one\npop\n").unwrap();
    stash_push(root, Some("pop me"), false, false).unwrap();
    stash_pop(root, "stash@{0}").unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "one\npop\n"
    );
    assert!(stash_list(root).unwrap().is_empty());
}

#[test]
fn stash_keep_index_keeps_staged_and_applies_unstaged() {
    let dir = repo();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
    git(root, &["add", "a.txt"]);
    std::fs::write(root.join("a.txt"), "one\ntwo\nthree\n").unwrap();
    stash_push(root, Some("keep"), true, false).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "one\ntwo\n"
    );
    let status = git(root, &["status", "--porcelain"]);
    assert!(status.contains("a.txt"), "{status}");
    stash_apply(root, "stash@{0}").unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "one\ntwo\nthree\n"
    );
}

#[test]
fn stash_apply_preserves_mode_only_changes() {
    use std::os::unix::fs::PermissionsExt;
    let dir = repo();
    let root = dir.path();
    std::fs::set_permissions(root.join("a.txt"), std::fs::Permissions::from_mode(0o755)).unwrap();
    stash_push(root, Some("mode"), false, false).unwrap();
    assert_eq!(
        std::fs::metadata(root.join("a.txt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o644
    );
    stash_apply(root, "stash@{0}").unwrap();
    assert_eq!(
        std::fs::metadata(root.join("a.txt"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
}

#[test]
fn native_diff_patch_is_git_apply_compatible() {
    let dir = repo();
    let root = dir.path();
    std::fs::write(root.join("del.txt"), "bye\n").unwrap();
    git(root, &["add", "del.txt"]);
    git(root, &["commit", "-qm", "delete base"]);

    std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
    std::fs::write(root.join("new.txt"), "brand new\n").unwrap();
    std::fs::remove_file(root.join("del.txt")).unwrap();
    let patch = diff_patch_paths(
        root,
        &[
            "a.txt".to_string(),
            "new.txt".to_string(),
            "del.txt".to_string(),
        ],
    )
    .unwrap();
    assert!(patch.contains("diff --git a/new.txt b/new.txt"), "{patch}");
    let mut child = Command::new("git")
        .current_dir(root)
        .args(["apply", "--check", "--reverse", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(patch.as_bytes())
        .unwrap();
    let checked = child.wait_with_output().unwrap();
    assert!(
        checked.status.success(),
        "git apply --check -R failed: {}",
        String::from_utf8_lossy(&checked.stderr)
    );

    lazecode_git::apply_patch(root, &patch, false, true).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "one\n"
    );
    assert!(!root.join("new.txt").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("del.txt")).unwrap(),
        "bye\n"
    );
}

#[test]
fn native_diff_patch_preserves_missing_final_newline() {
    let dir = repo();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "old").unwrap();
    git(root, &["commit", "-qam", "no newline"]);
    std::fs::write(root.join("a.txt"), "new").unwrap();
    let patch = diff_patch_paths(root, &["a.txt".to_string()]).unwrap();
    assert!(patch.contains("\\ No newline at end of file"), "{patch}");
    let mut child = Command::new("git")
        .current_dir(root)
        .args(["apply", "--check", "--reverse", "-"])
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(patch.as_bytes())
        .unwrap();
    let checked = child.wait_with_output().unwrap();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    lazecode_git::apply_patch(root, &patch, false, true).unwrap();
    assert_eq!(std::fs::read(root.join("a.txt")).unwrap(), b"old");
}

#[test]
fn stash_apply_three_way_merges_non_overlapping_edits() {
    let dir = repo();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "one\ntwo\nthree\n").unwrap();
    git(root, &["commit", "-qam", "base"]);
    std::fs::write(root.join("a.txt"), "one\ntwo\nTHREE\n").unwrap();
    git(root, &["stash", "push", "-q", "-m", "theirs"]);
    std::fs::write(root.join("a.txt"), "ONE\ntwo\nthree\n").unwrap();
    stash_apply(root, "stash@{0}").unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "ONE\ntwo\nTHREE\n"
    );
}

#[test]
fn stash_apply_writes_conflict_markers_on_overlap() {
    let dir = repo();
    let root = dir.path();
    std::fs::write(root.join("a.txt"), "base\n").unwrap();
    git(root, &["commit", "-qam", "base"]);
    std::fs::write(root.join("a.txt"), "theirs\n").unwrap();
    git(root, &["stash", "push", "-q", "-m", "theirs"]);
    std::fs::write(root.join("a.txt"), "ours\n").unwrap();
    let error = stash_apply(root, "stash@{0}").unwrap_err().to_string();
    assert!(error.contains("conflicts"), "{error}");
    let content = std::fs::read_to_string(root.join("a.txt")).unwrap();
    assert!(content.contains("<<<<<<<"), "{content}");
    assert!(content.contains("======="), "{content}");
    assert!(content.contains(">>>>>>>"), "{content}");
    assert!(content.contains("ours"), "{content}");
    assert!(content.contains("theirs"), "{content}");
    // A conflicted pop must keep the stash entry.
    assert_eq!(stash_list(root).unwrap().len(), 1);
}

#[test]
fn stash_apply_streams_large_lfs_objects() {
    let dir = repo();
    let root = dir.path();
    std::fs::write(root.join(".gitattributes"), "*.bin filter=lfs -text\n").unwrap();
    let old = vec![1u8; 17 * 1024 * 1024];
    std::fs::write(root.join("big.bin"), &old).unwrap();
    git(root, &["add", ".gitattributes", "big.bin"]);
    git(root, &["commit", "-qm", "lfs base"]);

    let new = vec![2u8; 17 * 1024 * 1024];
    std::fs::write(root.join("big.bin"), &new).unwrap();
    stash_push(root, Some("lfs change"), false, false).unwrap();
    assert_eq!(
        std::fs::read(root.join("big.bin")).unwrap(),
        old,
        "stash push must restore the base LFS object"
    );
    stash_apply(root, "stash@{0}").unwrap();
    assert_eq!(std::fs::read(root.join("big.bin")).unwrap(), new);
}
