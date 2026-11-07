use std::path::PathBuf;

use lazecode_core::{AppModel, PanelTab, ProjectState, Settings, SidebarView, TabKind, UiState};
use std::collections::HashSet;

fn model() -> AppModel {
    AppModel {
        settings: Settings::default(),
        projects: Vec::new(),
        active_project: None,
        ui: UiState::default(),
        toasts: Vec::new(),
        recent: Vec::new(),
        commands: Vec::new(),
        notifier: None,
        should_quit: false,
        version: "test",
        favorite_branches: HashSet::new(),
    }
}

#[test]
fn project_lifecycle_opens_files_and_terminals() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    let settings = Settings::default();
    let mut project = ProjectState::open(dir.path().to_path_buf(), &settings, None).unwrap();
    assert_eq!(project.tree.file_count, 2);
    assert_eq!(project.active_worktree().terminals.len(), 1);
    let terminal_id = project.active_worktree().active_terminal.unwrap();

    let tab = project
        .open_file(dir.path().join("src/main.rs"), &settings)
        .unwrap();
    assert_eq!(project.tabs.len(), 1);
    assert!(matches!(
        project.tab_by_id(tab).unwrap().kind,
        TabKind::File(_)
    ));
    assert!(project.expanded.contains(&PathBuf::from("src")));
    assert!(project.expanded.contains(&PathBuf::from("src/main.rs")));

    project.close_tab(tab);
    assert!(project.tabs.is_empty());
    project.close_terminal(terminal_id);
    assert!(project.active_worktree().terminals.is_empty());
}

#[test]
fn app_model_adds_projects_and_switches() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    std::fs::write(dir_a.path().join("a.txt"), "a\n").unwrap();
    std::fs::write(dir_b.path().join("b.txt"), "b\n").unwrap();
    let mut model = model();
    let a = ProjectState::open(dir_a.path().to_path_buf(), &model.settings, None).unwrap();
    let index_a = model.add_loaded_project(a);
    let b = ProjectState::open(dir_b.path().to_path_buf(), &model.settings, None).unwrap();
    let index_b = model.add_loaded_project(b);
    assert_eq!(model.active_project, Some(index_b));
    assert_eq!(model.recent.len(), 2);
    model.switch_project(index_a);
    assert_eq!(model.active_project, Some(index_a));
    assert_eq!(
        model.active_project().unwrap().root,
        dir_a.path().canonicalize().unwrap()
    );
    model.close_project(index_a);
    assert_eq!(model.projects.len(), 1);
    assert_eq!(model.active_project, Some(0));
    model.ui.sidebar_view = SidebarView::Git;
    model.ui.panel_tab = PanelTab::Git;
    assert_eq!(model.ui.sidebar_view, SidebarView::Git);
    assert_eq!(model.ui.panel_tab, PanelTab::Git);
}

#[test]
fn bad_shell_setting_does_not_drop_the_project() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
    let mut settings = Settings::default();
    settings.terminal.shell = Some("/definitely/not/a/shell".to_string());
    let project = ProjectState::open(dir.path().to_path_buf(), &settings, None).unwrap();
    assert!(project.active_worktree().terminals.is_empty());
    assert_eq!(project.tree.file_count, 1);
}

#[test]
fn single_file_session_open_and_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("standalone.rs");
    std::fs::write(&file_path, "fn standalone() {}\n").unwrap();

    let mut model = model();
    let project = ProjectState::open(file_path.clone(), &model.settings, None).unwrap();
    assert!(project.is_single_file);
    assert_eq!(project.tabs.len(), 1);

    let index = model.add_loaded_project(project);
    assert_eq!(model.active_project, Some(index));
    // Single-file project does not pollute recent projects
    assert!(model.recent.is_empty());

    // Saving workspace does not persist single-file project
    model.save_workspace();
    // Verify project has 1 open tab
    assert_eq!(model.projects[index].tabs.len(), 1);
}

#[test]
fn tabs_and_terminals_batch_closing_operations() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    std::fs::write(root.join("1.txt"), "1\n").unwrap();
    std::fs::write(root.join("2.txt"), "2\n").unwrap();
    std::fs::write(root.join("3.txt"), "3\n").unwrap();

    let settings = Settings::default();
    let mut project = ProjectState::open(root.clone(), &settings, None).unwrap();
    let t1 = project.open_file(root.join("1.txt"), &settings).unwrap();
    let t2 = project.open_file(root.join("2.txt"), &settings).unwrap();
    let t3 = project.open_file(root.join("3.txt"), &settings).unwrap();
    assert_eq!(project.tabs.len(), 3);

    // close_tabs_to_right
    project.close_tabs_to_right(t2);
    assert_eq!(project.tabs.len(), 2);
    assert!(project.tabs.iter().any(|t| t.id == t1));
    assert!(project.tabs.iter().any(|t| t.id == t2));
    assert!(!project.tabs.iter().any(|t| t.id == t3));

    // close_other_tabs
    project.close_other_tabs(t2);
    assert_eq!(project.tabs.len(), 1);
    assert_eq!(project.tabs[0].id, t2);

    // Terminals: open additional terminals and test batch close
    let _ = project.open_terminal(
        None,
        None,
        lazecode_terminal::SessionKind::Shell,
        &settings,
        None,
    );
    let _ = project.open_terminal(
        None,
        None,
        lazecode_terminal::SessionKind::Shell,
        &settings,
        None,
    );
    let wt = project.active_worktree();
    assert_eq!(wt.terminals.len(), 3);
    let first_id = wt.terminals[0].id;

    project.close_other_terminals(first_id);
    assert_eq!(project.active_worktree().terminals.len(), 1);
    assert_eq!(project.active_worktree().terminals[0].id, first_id);

    project.close_all_terminals();
    assert!(project.active_worktree().terminals.is_empty());
}

#[test]
fn commit_history_is_deduped_and_capped() {
    let mut settings = lazecode_core::Settings::default();
    for index in 0..14 {
        settings.remember_commit_message(&format!("message {index}"));
    }
    assert_eq!(settings.commit_history.len(), 10);
    assert_eq!(settings.commit_history[0], "message 13");
    // 重复提交信息移到最前，而不是产生重复项。
    settings.remember_commit_message("message 7");
    assert_eq!(settings.commit_history[0], "message 7");
    assert_eq!(
        settings
            .commit_history
            .iter()
            .filter(|message| **message == "message 7")
            .count(),
        1
    );
    // 空白信息不入史。
    settings.remember_commit_message("   ");
    assert_eq!(settings.commit_history.len(), 10);
}

#[test]
fn diff_tabs_reuse_one_window_like_stable() {
    // 稳定版语义：diff 是全局单例标签——换文件/换提交只换内容，
    // 不新开标签；任两文件互比（Files）独立成标签。
    let settings = Settings::default();
    let dir = tempfile::tempdir().unwrap();
    let mut project = ProjectState::open(dir.path().to_path_buf(), &settings, None).unwrap();

    let t1 = project.open_diff_source(lazecode_core::DiffSource::Worktree {
        path: "a.rs".into(),
    });
    assert_eq!(project.tabs.len(), 1);
    let t2 = project.open_diff_source(lazecode_core::DiffSource::Worktree {
        path: "b.rs".into(),
    });
    assert_eq!(t2, t1, "换文件复用同一 diff 标签");
    assert_eq!(project.tabs.len(), 1);
    match &project.tabs[0].kind {
        TabKind::Diff(diff) => {
            assert_eq!(diff.path, "b.rs");
            assert!(diff.loading, "复用时重置为加载态");
        }
        _ => panic!("expected diff tab"),
    }

    let t3 = project.open_diff_source(lazecode_core::DiffSource::Commit {
        commit: "abc1234def".into(),
        path: "a.rs".into(),
    });
    assert_eq!(t3, t1, "换提交仍复用同一 diff 标签");
    assert_eq!(project.tabs.len(), 1);
    assert_eq!(project.tabs[0].title(), "Diff (abc1234)");

    let t4 = project.open_diff_source(lazecode_core::DiffSource::Files {
        old_path: "/tmp/x.txt".into(),
        new_path: "/tmp/y.txt".into(),
    });
    assert_ne!(t4, t1, "Files 互比独立成标签");
    assert_eq!(project.tabs.len(), 2);

    // 完全同源只激活，不重置加载态。
    project.set_diff_for_source(
        &lazecode_core::DiffSource::Commit {
            commit: "abc1234def".into(),
            path: "a.rs".into(),
        },
        Ok(lazecode_git::FileDiff {
            path: "a.rs".into(),
            kind: lazecode_git::ChangeKind::Modified,
            old_text: String::new(),
            new_text: String::new(),
            diff: Default::default(),
            binary: false,
        }),
    );
    let t5 = project.open_diff_source(lazecode_core::DiffSource::Commit {
        commit: "abc1234def".into(),
        path: "a.rs".into(),
    });
    assert_eq!(t5, t1);
    match &project.tabs.iter().find(|tab| tab.id == t1).unwrap().kind {
        TabKind::Diff(diff) => {
            assert!(diff.diff.is_some(), "同源激活不应重置已加载内容");
            assert!(!diff.loading);
        }
        _ => panic!("expected diff tab"),
    }
}
