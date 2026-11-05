use lazecode_git::{
    clone, fetch_all, pending_count, pull, push, remote_add, remote_list, remote_remove,
    remote_rename, remote_set_url, PullMode,
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
        &["init", "-q", "--bare", upstream_path.to_str().unwrap()],
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
