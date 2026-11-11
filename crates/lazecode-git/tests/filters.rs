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

#[test]
fn eol_attributes_keep_status_clean_and_store_lf() {
    let dir = init();
    std::fs::write(dir.path().join(".gitattributes"), "*.txt text eol=crlf\n").unwrap();
    std::fs::write(dir.path().join("a.txt"), "one\r\ntwo\r\n").unwrap();
    lazecode_git::stage(dir.path(), &[".gitattributes".into(), "a.txt".into()]).unwrap();
    lazecode_git::commit(dir.path(), "crlf", false).unwrap();

    let blob = git(dir.path(), &["cat-file", "-p", "HEAD:a.txt"]);
    assert_eq!(blob, "one\ntwo");
    let snap = lazecode_git::snapshot(dir.path()).unwrap();
    assert!(
        snap.changes.is_empty(),
        "CRLF worktree must be clean: {:?}",
        snap.changes
    );
}

#[test]
fn checked_commit_applies_clean_filters() {
    let dir = init();
    std::fs::write(dir.path().join(".gitattributes"), "*.txt text eol=crlf\n").unwrap();
    std::fs::write(dir.path().join("a.txt"), "one\r\n").unwrap();
    lazecode_git::stage(dir.path(), &[".gitattributes".into(), "a.txt".into()]).unwrap();
    lazecode_git::commit(dir.path(), "base", false).unwrap();

    std::fs::write(dir.path().join("a.txt"), "one\r\ntwo\r\n").unwrap();
    lazecode_git::commit_checked(dir.path(), &["a.txt".into()], &[], "update", false).unwrap();
    let blob = git(dir.path(), &["cat-file", "-p", "HEAD:a.txt"]);
    assert_eq!(blob, "one\ntwo");
}

#[test]
fn lfs_filter_stores_pointer_and_smudges_object() {
    let dir = init();
    std::fs::write(
        dir.path().join(".gitattributes"),
        "*.bin filter=lfs diff=lfs merge=lfs -text\n",
    )
    .unwrap();
    let content = b"\x00binary\xffpayload".to_vec();
    std::fs::write(dir.path().join("data.bin"), &content).unwrap();
    lazecode_git::stage(dir.path(), &[".gitattributes".into(), "data.bin".into()]).unwrap();
    lazecode_git::commit(dir.path(), "lfs", false).unwrap();

    let pointer = git(dir.path(), &["cat-file", "-p", "HEAD:data.bin"]);
    assert!(pointer.contains("version https://git-lfs.github.com/spec/v1"));
    assert!(pointer.contains(&format!("size {}", content.len())));
    assert!(
        dir.path().join(".git/lfs/objects").read_dir().is_ok(),
        "LFS object store must exist"
    );

    let pointer_bytes = pointer.into_bytes();
    let restored = lazecode_git::filter::smudge(
        dir.path(),
        "data.bin",
        "0000000000000000000000000000000000000000",
        pointer_bytes,
    )
    .unwrap();
    assert_eq!(restored, content);
}

#[test]
fn user_filter_commands_run_clean_and_smudge() {
    let dir = init();
    git(dir.path(), &["config", "filter.up.clean", "tr a-z A-Z"]);
    git(dir.path(), &["config", "filter.up.smudge", "tr A-Z a-z"]);
    std::fs::write(dir.path().join(".gitattributes"), "*.up filter=up\n").unwrap();
    std::fs::write(dir.path().join("a.up"), "hello\n").unwrap();
    lazecode_git::stage(dir.path(), &[".gitattributes".into(), "a.up".into()]).unwrap();
    lazecode_git::commit(dir.path(), "filtered", false).unwrap();

    let blob = git(dir.path(), &["cat-file", "-p", "HEAD:a.up"]);
    assert_eq!(blob, "HELLO");
    let restored = lazecode_git::filter::smudge(
        dir.path(),
        "a.up",
        "0000000000000000000000000000000000000000",
        blob.into_bytes(),
    )
    .unwrap();
    assert_eq!(String::from_utf8(restored).unwrap(), "hello");
}

#[test]
fn required_filter_failure_blocks_staging() {
    let dir = init();
    git(dir.path(), &["config", "filter.bad.clean", "false"]);
    git(dir.path(), &["config", "filter.bad.required", "true"]);
    std::fs::write(dir.path().join(".gitattributes"), "*.bad filter=bad\n").unwrap();
    std::fs::write(dir.path().join("a.bad"), "payload\n").unwrap();
    let error = lazecode_git::stage(dir.path(), &["a.bad".into()]).unwrap_err();
    assert!(error.to_string().contains("filter command failed"));
}

#[test]
fn ident_attribute_collapses_and_expands() {
    let dir = init();
    std::fs::write(dir.path().join(".gitattributes"), "*.id ident\n").unwrap();
    std::fs::write(dir.path().join("a.id"), "$Id$\n").unwrap();
    lazecode_git::stage(dir.path(), &[".gitattributes".into(), "a.id".into()]).unwrap();
    lazecode_git::commit(dir.path(), "ident", false).unwrap();

    let oid = git(dir.path(), &["rev-parse", "HEAD:a.id"]);
    let restored =
        lazecode_git::filter::smudge(dir.path(), "a.id", &oid, b"$Id$\n".to_vec()).unwrap();
    assert_eq!(
        String::from_utf8(restored).unwrap(),
        format!("$Id: {oid} $\n")
    );
    let collapsed =
        lazecode_git::filter::clean(dir.path(), "a.id", format!("$Id: {oid} $\n").into_bytes())
            .unwrap();
    assert_eq!(String::from_utf8(collapsed).unwrap(), "$Id$\n");
}

#[test]
fn failed_checkout_rolls_back_worktree_and_index() {
    let dir = init();
    std::fs::write(dir.path().join("keep.txt"), "keep\n").unwrap();
    lazecode_git::stage(dir.path(), &["keep.txt".into()]).unwrap();
    lazecode_git::commit(dir.path(), "base", false).unwrap();

    git(dir.path(), &["checkout", "-q", "-b", "broken"]);
    std::fs::write(dir.path().join(".gitattributes"), "*.bin filter=broken\n").unwrap();
    std::fs::write(dir.path().join("a.txt"), "target\n").unwrap();
    std::fs::write(dir.path().join("data.bin"), "payload\n").unwrap();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "broken target"]);
    git(dir.path(), &["checkout", "-q", "main"]);
    git(dir.path(), &["config", "filter.broken.clean", "cat"]);
    git(dir.path(), &["config", "filter.broken.smudge", "false"]);
    git(dir.path(), &["config", "filter.broken.required", "true"]);

    let error = lazecode_git::checkout_branch(dir.path(), "broken").unwrap_err();
    assert!(
        error.to_string().contains("filter command failed"),
        "{error}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("keep.txt")).unwrap(),
        "keep\n"
    );
    assert!(!dir.path().join(".gitattributes").exists());
    assert!(!dir.path().join("a.txt").exists());
    assert!(!dir.path().join("data.bin").exists());
    let snap = lazecode_git::snapshot(dir.path()).unwrap();
    assert_eq!(snap.branch.as_deref(), Some("main"));
    assert!(snap.changes.is_empty(), "{:?}", snap.changes);
}

#[test]
fn lfs_streams_files_above_the_stage_limit() {
    let dir = init();
    std::fs::write(
        dir.path().join(".gitattributes"),
        "*.bin filter=lfs -text\n",
    )
    .unwrap();
    let content = vec![7u8; 17 * 1024 * 1024];
    std::fs::write(dir.path().join("big.bin"), &content).unwrap();
    lazecode_git::stage(dir.path(), &[".gitattributes".into(), "big.bin".into()]).unwrap();
    lazecode_git::commit(dir.path(), "big", false).unwrap();
    let pointer = git(dir.path(), &["cat-file", "-p", "HEAD:big.bin"]);
    assert!(pointer.contains(&format!("size {}", content.len())));
    assert!(lazecode_git::snapshot(dir.path())
        .unwrap()
        .changes
        .is_empty());

    git(dir.path(), &["checkout", "-q", "-b", "empty"]);
    git(dir.path(), &["rm", "-q", "-f", "big.bin"]);
    git(dir.path(), &["commit", "-q", "-m", "empty"]);
    lazecode_git::checkout_branch(dir.path(), "main").unwrap();
    assert_eq!(
        std::fs::metadata(dir.path().join("big.bin")).unwrap().len(),
        content.len() as u64
    );
}

#[test]
fn text_auto_normalizes_text_but_not_binary() {
    let dir = init();
    std::fs::write(
        dir.path().join(".gitattributes"),
        "*.txt text=auto\n*.bin text=auto\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("a.txt"), "one\r\ntwo\r\n").unwrap();
    std::fs::write(dir.path().join("a.bin"), b"one\r\n\x00two\r\n").unwrap();
    lazecode_git::stage(
        dir.path(),
        &[".gitattributes".into(), "a.txt".into(), "a.bin".into()],
    )
    .unwrap();
    lazecode_git::commit(dir.path(), "auto", false).unwrap();
    assert_eq!(
        git(dir.path(), &["cat-file", "-p", "HEAD:a.txt"]),
        "one\ntwo"
    );
    assert_eq!(
        git(dir.path(), &["cat-file", "-p", "HEAD:a.bin"]),
        "one\r\n\x00two"
    );
}
