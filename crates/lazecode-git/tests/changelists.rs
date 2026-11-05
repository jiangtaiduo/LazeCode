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
