use lazecode_git::Changelists;

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();
    dir
}

#[test]
fn create_assign_remove_round_trip() {
    let dir = repo();
    let mut lists = Changelists::open(dir.path());
    assert_eq!(lists.lists(), ["Default"]);

    lists.create_list("Feature");
    lists.create_list("Feature"); // duplicate ignored
    assert_eq!(lists.lists(), ["Default", "Feature"]);

    lists.assign("src/a.rs", "Feature");
    assert_eq!(lists.list_of("src/a.rs"), "Feature");
    assert!(lists.files_of("Feature").contains(&"src/a.rs".to_string()));
    assert!(lists.files_of("Default").is_empty());

    // Removing the list returns the files and re-homes them to Default.
    let moved = lists.remove_list("Feature");
    assert_eq!(moved, vec!["src/a.rs".to_string()]);
    assert!(lists.list_of("src/a.rs") != "Feature");
    assert!(lists.files_of("Feature").is_empty());
}

#[test]
fn default_list_is_undeletable() {
    let dir = repo();
    let mut lists = Changelists::open(dir.path());
    lists.remove_list("Default");
    assert!(lists.lists().contains(&"Default".to_string()));
}

#[test]
fn persists_across_instances() {
    let dir = repo();
    {
        let mut lists = Changelists::open(dir.path());
        lists.create_list("Persistent");
        lists.assign("x.txt", "Persistent");
    }
    let lists = Changelists::open(dir.path());
    assert_eq!(lists.list_of("x.txt"), "Persistent");
    assert_eq!(lists.lists(), ["Default", "Persistent"]);
}

#[test]
fn assign_many_and_create_checked() {
    let dir = repo();
    let mut lists = Changelists::open(dir.path());
    // create_list_checked：重名/空名报错，成功创建返回 Ok。
    assert!(lists.create_list_checked("Work").is_ok());
    assert!(lists.create_list_checked("Work").is_err());
    assert!(lists.create_list_checked("  ").is_err());

    // assign_many 一次落盘多条归属。
    lists.assign_many(&["a.txt".to_string(), "b.txt".to_string()], "Work");
    assert_eq!(lists.list_of("a.txt"), "Work");
    assert_eq!(lists.list_of("b.txt"), "Work");

    // 新实例读回（单一 flush 语义）。
    let reloaded = Changelists::open(dir.path());
    assert_eq!(reloaded.files_of("Work").len(), 2);
}

#[test]
fn resolves_linked_worktree_git_dir() {
    // 关联 worktree 的 `.git` 是文件而非目录：changelist 必须写入真实
    // git-dir（回归覆盖：曾经直接拼 root/.git 导致静默失效）。
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("main");
    std::fs::create_dir_all(&root).unwrap();
    let run = |args: &[&str], cwd: &std::path::Path| {
        let output = std::process::Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(&["init", "-q", "-b", "main"], &root);
    run(&["config", "user.name", "T"], &root);
    run(&["config", "user.email", "t@e"], &root);
    std::fs::write(root.join("a.txt"), "one\n").unwrap();
    run(&["add", "."], &root);
    run(&["commit", "-q", "-m", "c1"], &root);
    run(&["worktree", "add", "-b", "side", "../side"], &root);

    let side = dir.path().join("side");
    let mut lists = Changelists::open(&side);
    lists.assign("a.txt", "Default");
    // 落点在主仓库的 .git/worktrees/<name>/ 下，而不是 side/.git（它是文件）。
    assert!(!side.join(".git").is_dir());
    let found = std::fs::read_dir(root.join(".git/worktrees"))
        .unwrap()
        .flatten()
        .any(|entry| entry.path().join("lazecode-changelists.json").exists());
    assert!(found, "changelists.json must land in the real git-dir");
}
