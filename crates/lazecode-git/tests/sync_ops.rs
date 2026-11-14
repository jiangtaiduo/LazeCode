use lazecode_git::{
    clone, delete_remote_branch, fetch_all, pending_commits, pending_count, pull, push, push_tag,
    remote_add, remote_list, remote_remove, remote_rename, remote_set_url, PullMode,
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

#[test]
fn remote_list_add_rename_seturl_remove_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "T"]);
    git(root, &["config", "user.email", "t@e"]);
    std::fs::write(root.join("f.txt"), "x\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "one"]);

    // add
    remote_add(root, "origin", "https://example.com/repo.git").unwrap();
    let remotes = remote_list(root).unwrap();
    assert_eq!(remotes.len(), 1);
    assert_eq!(remotes[0].name, "origin");
    assert_eq!(remotes[0].url, "https://example.com/repo.git");

    // set-url + rename
    remote_set_url(root, "origin", "https://example.com/other.git").unwrap();
    remote_rename(root, "origin", "upstream").unwrap();
    let remotes = remote_list(root).unwrap();
    assert_eq!(remotes[0].name, "upstream");
    assert_eq!(remotes[0].url, "https://example.com/other.git");

    // remove
    remote_remove(root, "upstream").unwrap();
    assert!(remote_list(root).unwrap().is_empty());
}

#[test]
fn push_fetch_pull_against_local_bare_remote() {
    // up-stream bare repo
    let upstream = tempfile::tempdir().unwrap();
    let upstream_path = upstream.path().join("origin.git");
    git(
        upstream.path(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            upstream_path.to_str().unwrap(),
        ],
    );

    // work repo 1 (clone doesn't matter here; init then push -u)
    let work = tempfile::tempdir().unwrap();
    let root = work.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "T"]);
    git(root, &["config", "user.email", "t@e"]);
    std::fs::write(root.join("a.txt"), "one\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "one"]);
    remote_add(root, "origin", upstream_path.to_str().unwrap()).unwrap();

    // Push with --set-upstream (this creates remote HEAD ref to main).
    push(root, "origin", "main", true, false).unwrap();

    // Pull on a second clone of the same remote
    let second = tempfile::tempdir().unwrap();
    let second_path = second.path().join("work");
    clone(upstream_path.to_str().unwrap(), &second_path).unwrap();
    assert_eq!(
        std::fs::read_to_string(second_path.join("a.txt")).unwrap(),
        "one\n"
    );

    // Commit again on the first repo; push; second pull fast-forwards.
    std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
    git(root, &["commit", "-qam", "two"]);
    push(root, "origin", "main", false, false).unwrap();
    pull(&second_path, PullMode::FastForwardOnly).unwrap();
    assert_eq!(
        std::fs::read_to_string(second_path.join("a.txt")).unwrap(),
        "one\ntwo\n"
    );

    // pending count on the second clone is 0 after pull.
    assert_eq!(pending_count(&second_path, "main").unwrap(), 0);
    // pending count on the first clone is 0 after push.
    assert_eq!(pending_count(root, "main").unwrap(), 0);

    // fetch --prune is a no-op against a healthy remote.
    fetch_all(&second_path).unwrap();
}

#[test]
fn fetch_prunes_stale_remote_tracking_refs() {
    let upstream = tempfile::tempdir().unwrap();
    let upstream_path = upstream.path().join("origin.git");
    git(
        upstream.path(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            upstream_path.to_str().unwrap(),
        ],
    );
    let work = tempfile::tempdir().unwrap();
    let root = work.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "T"]);
    git(root, &["config", "user.email", "t@e"]);
    std::fs::write(root.join("a.txt"), "one\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "one"]);
    remote_add(root, "origin", upstream_path.to_str().unwrap()).unwrap();
    push(root, "origin", "main", true, false).unwrap();

    let second = tempfile::tempdir().unwrap();
    let second_path = second.path().join("work");
    clone(upstream_path.to_str().unwrap(), &second_path).unwrap();
    let head = git(&second_path, &["rev-parse", "HEAD"]);
    git(
        &second_path,
        &["update-ref", "refs/remotes/origin/gone", &head],
    );
    fetch_all(&second_path).unwrap();
    let refs = git(&second_path, &["for-each-ref", "refs/remotes/origin"]);
    assert!(!refs.contains("origin/gone"), "{refs}");
    assert!(refs.contains("origin/main"), "{refs}");
}

#[test]
fn push_rejects_non_fast_forward_and_force_overwrites() {
    let upstream = tempfile::tempdir().unwrap();
    let upstream_path = upstream.path().join("origin.git");
    git(
        upstream.path(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            upstream_path.to_str().unwrap(),
        ],
    );
    let work = tempfile::tempdir().unwrap();
    let root = work.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "T"]);
    git(root, &["config", "user.email", "t@e"]);
    std::fs::write(root.join("a.txt"), "one\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "one"]);
    remote_add(root, "origin", upstream_path.to_str().unwrap()).unwrap();
    push(root, "origin", "main", true, false).unwrap();
    assert_eq!(git(root, &["config", "branch.main.remote"]), "origin");
    assert_eq!(
        git(root, &["config", "branch.main.merge"]),
        "refs/heads/main"
    );

    std::fs::write(root.join("a.txt"), "two\n").unwrap();
    git(root, &["commit", "-qam", "two"]);
    push(root, "origin", "main", false, false).unwrap();
    git(root, &["commit", "--amend", "-qm", "two rewritten"]);
    let error = push(root, "origin", "main", false, false).unwrap_err();
    assert!(error.to_string().contains("fast-forward"), "{error}");
    push(root, "origin", "main", false, true).unwrap();
    assert_eq!(
        git(&upstream_path, &["rev-parse", "main"]),
        git(root, &["rev-parse", "HEAD"])
    );
}

#[test]
fn remote_branch_delete_and_tag_push_against_bare_remote() {
    let upstream = tempfile::tempdir().unwrap();
    let upstream_path = upstream.path().join("origin.git");
    git(
        upstream.path(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            upstream_path.to_str().unwrap(),
        ],
    );
    let work = tempfile::tempdir().unwrap();
    let root = work.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "T"]);
    git(root, &["config", "user.email", "t@e"]);
    std::fs::write(root.join("a.txt"), "one\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "one"]);
    remote_add(root, "origin", upstream_path.to_str().unwrap()).unwrap();
    push(root, "origin", "main", true, false).unwrap();

    git(root, &["branch", "docs-only"]);
    push(root, "origin", "docs-only", false, false).unwrap();
    fetch_all(root).unwrap();
    delete_remote_branch(root, "origin/docs-only").unwrap();
    let heads = git(root, &["ls-remote", "--heads", "origin"]);
    assert!(!heads.contains("docs-only"), "{heads}");
    assert!(!git(root, &["for-each-ref", "refs/remotes/origin/docs-only"]).contains("docs-only"));

    git(root, &["tag", "v9.9.9"]);
    push_tag(root, "origin", "v9.9.9").unwrap();
    let tags = git(root, &["ls-remote", "--tags", "origin"]);
    assert!(tags.contains("refs/tags/v9.9.9"), "{tags}");
}

#[test]
fn pending_commits_are_limited_to_upstream_range() {
    let upstream = tempfile::tempdir().unwrap();
    let upstream_path = upstream.path().join("origin.git");
    git(
        upstream.path(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            upstream_path.to_str().unwrap(),
        ],
    );
    let work = tempfile::tempdir().unwrap();
    let root = work.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "T"]);
    git(root, &["config", "user.email", "t@e"]);
    std::fs::write(root.join("a.txt"), "one\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "one"]);
    remote_add(root, "origin", upstream_path.to_str().unwrap()).unwrap();
    push(root, "origin", "main", true, false).unwrap();

    std::fs::write(root.join("b.txt"), "two\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "two"]);
    let pending = pending_commits(root, Some("origin/main"), "main", 20).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].message, "two");
    push(root, "origin", "main", false, false).unwrap();
    assert!(pending_commits(root, Some("origin/main"), "main", 20)
        .unwrap()
        .is_empty());
}
