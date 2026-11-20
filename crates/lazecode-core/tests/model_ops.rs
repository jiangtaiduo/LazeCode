use std::path::PathBuf;

use lazecode_core::{
    AppModel, PaneSplit, PanelTab, ProjectState, Settings, SidebarView, TabKind, UiState,
    MAX_PANES_PER_TAB,
};
use std::collections::HashSet;

fn model() -> AppModel {
    AppModel {
        settings: Settings::default(),
        projects: Vec::new(),
        active_project: None,
        ui: UiState::default(),
        toasts: Vec::new(),
        recent: Vec::new(),
        recent_files: Vec::new(),
        commands: Vec::new(),
        notifier: None,
        lsp: lazecode_core::LspManager::new(true, None),
        should_quit: false,
        version: "test",
        favorite_branches: HashSet::new(),
        git_cmd_log: Vec::new(),
    }
}

#[test]
fn opening_editor_hides_only_a_maximized_console() {
    let mut ui = UiState {
        terminal_visible: true,
        terminal_maximised: true,
        ..UiState::default()
    };
    ui.reveal_editor_area();
    assert!(!ui.terminal_visible, "maximized console must be hidden");
    assert!(
        ui.terminal_maximised,
        "re-opening the console keeps the user's full-height preference"
    );

    ui.terminal_visible = true;
    ui.terminal_maximised = false;
    ui.reveal_editor_area();
    assert!(
        ui.terminal_visible,
        "a normal docked console must not be closed"
    );
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
    assert!(
        project.worktrees[0].watcher.is_none(),
        "the main checkout watcher is owned by ProjectState"
    );
    assert_eq!(project.active_worktree().terminals.len(), 1);
    let terminal_id = project.active_worktree().active_terminal.unwrap();

    let tab = project
        .open_file(dir.path().join("src/main.rs"), &settings)
        .unwrap();
    assert_eq!(project.tabs.len(), 1);
    assert_eq!(project.active_worktree_tabs().count(), 1);
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
fn open_applies_ignore_patterns_to_the_main_worktree_tree() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("generated")).unwrap();
    std::fs::write(dir.path().join("generated/secret.txt"), "ignored\n").unwrap();
    std::fs::write(dir.path().join("keep.txt"), "keep\n").unwrap();
    let mut settings = Settings::default();
    settings.ignored_patterns.push("generated".to_string());

    let project = ProjectState::open(dir.path().to_path_buf(), &settings, None).unwrap();
    let names: Vec<&str> = project
        .active_tree()
        .nodes
        .iter()
        .map(|node| node.name.as_str())
        .collect();
    assert!(names.contains(&"keep.txt"));
    assert!(!names.contains(&"generated"));
}

#[test]
fn bad_shell_setting_does_not_drop_the_project() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
    let mut settings = Settings::default();
    settings.terminal.shell = Some("/definitely/not/a/shell".to_string());
    let project = ProjectState::open(dir.path().to_path_buf(), &settings, None).unwrap();
    assert_eq!(project.tree.file_count, 1);
    // An unknown shell falls back to /bin/zsh instead of dropping the project.
    assert_eq!(project.active_worktree().terminals.len(), 1);
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
    assert_eq!(model.projects[index].active_worktree_tabs().count(), 1);
    assert!(model.projects[index].active_tab.is_some());

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
    let first_id = wt.terminals[0].panes[0].id;

    project.close_other_terminals(first_id);
    assert_eq!(project.active_worktree().terminals.len(), 1);
    assert_eq!(project.active_worktree().terminals[0].panes[0].id, first_id);
    assert_eq!(project.active_worktree().active_terminal, Some(first_id));

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
    let active_root = project.active_root().to_path_buf();
    project.set_diff_for_source(
        &active_root,
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

#[test]
fn locate_active_file_expands_ancestors_and_sets_reveal() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("crates/lazecode-core/src")).unwrap();
    let file_path = dir.path().join("crates/lazecode-core/src/commands.rs");
    std::fs::write(&file_path, "// test\n").unwrap();

    let mut model = model();
    let mut project = ProjectState::open(dir.path().to_path_buf(), &model.settings, None).unwrap();
    project.expanded.clear();
    let tab_id = project
        .open_file(file_path.clone(), &model.settings)
        .unwrap();
    project.active_tab = Some(tab_id);
    project.expanded.clear(); // Clear to test locate_active_file re-expansion

    let idx = model.add_loaded_project(project);
    model.active_project = Some(idx);

    // Run manual locate
    model.locate_active_file(false);

    let project = &model.projects[idx];
    assert!(project.expanded.contains(&PathBuf::from("crates")));
    assert!(project
        .expanded
        .contains(&PathBuf::from("crates/lazecode-core")));
    assert!(project
        .expanded
        .contains(&PathBuf::from("crates/lazecode-core/src")));

    let expected_rel = PathBuf::from("crates/lazecode-core/src/commands.rs");
    assert_eq!(model.ui.tree_selected, Some(expected_rel.clone()));
    assert_eq!(model.ui.tree_reveal, Some((expected_rel.clone(), false)));
    assert!(model.ui.locate_flash.is_some());
}

#[test]
fn compare_session_cross_file_syncs_selected_and_uncollapses_parents() {
    let mut session = lazecode_core::CompareSession {
        from: "897cd53c".into(),
        to: "bf548fd0".into(),
        group_by_dir: true,
        root_expanded: true,
        ..Default::default()
    };
    session.collapsed.insert("crates".into());
    session.collapsed.insert("crates/lazecode-core".into());
    session.collapsed.insert("crates/lazecode-core/src".into());
    session.selected = Some("crates/lazecode-app/src/jobs.rs".into());

    // Simulate navigating to "crates/lazecode-core/src/commands.rs"
    let target = "crates/lazecode-core/src/commands.rs";
    session.selected = Some(target.into());
    let parts: Vec<&str> = target.split('/').collect();
    for i in 1..parts.len() {
        let parent = parts[..i].join("/");
        session.collapsed.remove(&parent);
    }

    assert_eq!(session.selected, Some(target.into()));
    assert!(!session.collapsed.contains("crates"));
    assert!(!session.collapsed.contains("crates/lazecode-core"));
    assert!(!session.collapsed.contains("crates/lazecode-core/src"));
}
#[test]
fn git_ignore_distinction_and_lookup() {
    use lazecode_core::is_snapshot_ignored;
    use std::path::Path;

    let snapshot = lazecode_git::RepoSnapshot {
        root: PathBuf::from("/test"),
        branch: Some("main".into()),
        detached: false,
        unborn: false,
        ahead: 0,
        behind: 0,
        changes: Vec::new(),
        ignored_files: vec![
            ".audit".into(),
            ".zcode".into(),
            "debug.log".into(),
            "dist".into(),
            "target".into(),
        ],
        ignored_dirs: vec![
            ".audit".into(),
            ".zcode".into(),
            "dist".into(),
            "target".into(),
        ],
        head: None,
    };

    // Root is not ignored
    assert!(!is_snapshot_ignored(&snapshot, Path::new("")));

    // Direct files ignored by git
    assert!(is_snapshot_ignored(&snapshot, Path::new("debug.log")));

    // Direct folders ignored by git
    assert!(is_snapshot_ignored(&snapshot, Path::new("target")));
    assert!(is_snapshot_ignored(&snapshot, Path::new("dist")));
    assert!(is_snapshot_ignored(&snapshot, Path::new(".audit")));
    assert!(is_snapshot_ignored(&snapshot, Path::new(".zcode")));

    // Descendants inside ignored folders are also ignored
    assert!(is_snapshot_ignored(&snapshot, Path::new("target/debug")));
    assert!(is_snapshot_ignored(
        &snapshot,
        Path::new("target/debug/app")
    ));
    assert!(is_snapshot_ignored(&snapshot, Path::new("dist/bundle.js")));
    assert!(is_snapshot_ignored(
        &snapshot,
        Path::new(".audit/report.json")
    ));

    // Normal files and folders are NOT git ignored
    assert!(!is_snapshot_ignored(&snapshot, Path::new("Cargo.toml")));
    assert!(!is_snapshot_ignored(&snapshot, Path::new("src")));
    assert!(!is_snapshot_ignored(&snapshot, Path::new("src/main.rs")));
    assert!(!is_snapshot_ignored(
        &snapshot,
        Path::new("crates/lazecode-ui")
    ));
}

/// A project with one shell tab holding `panes` panes.
fn split_project(panes: usize) -> (tempfile::TempDir, Settings, ProjectState, u64, u64) {
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings::default();
    let mut project = ProjectState::open(dir.path().to_path_buf(), &settings, None).unwrap();
    let worktree = project.active_worktree();
    assert_eq!(worktree.terminals.len(), 1, "project opens one shell tab");
    let tab_id = worktree.terminals[0].id;
    let first = worktree.terminals[0].panes[0].id;
    let mut active = first;
    for slot in 1..panes {
        let direction = if slot % 2 == 1 {
            PaneSplit::Row
        } else {
            PaneSplit::Column
        };
        active = project
            .split_terminal(active, direction, &settings, None)
            .unwrap();
    }
    (dir, settings, project, first, tab_id)
}

#[test]
fn terminal_split_creates_independent_panes_up_to_limit() {
    let (_dir, settings, mut project, first, tab_id) = split_project(1);
    let second = project
        .split_terminal(first, PaneSplit::Row, &settings, None)
        .unwrap();
    assert_eq!(
        project.active_worktree().terminals.len(),
        1,
        "splitting adds a pane, not a tab"
    );
    let third = project
        .split_terminal(second, PaneSplit::Column, &settings, None)
        .unwrap();
    let fourth = project
        .split_terminal(third, PaneSplit::Row, &settings, None)
        .unwrap();

    let tab = project.active_worktree().tab_by_id(tab_id).unwrap();
    assert_eq!(tab.panes.len(), MAX_PANES_PER_TAB);
    let ids: Vec<u64> = tab.panes.iter().map(|pane| pane.id).collect();
    assert_eq!(ids, vec![first, second, third, fourth]);
    let sessions: HashSet<u64> = tab.panes.iter().map(|pane| pane.session.id).collect();
    assert_eq!(
        sessions.len(),
        MAX_PANES_PER_TAB,
        "each pane owns its own PTY"
    );
    assert_eq!(tab.split, PaneSplit::Row, "the last split direction wins");
    assert_eq!(tab.active_pane, fourth);
    assert_eq!(project.active_worktree().active_terminal, Some(fourth));

    // A fifth split of the same tab is refused and changes nothing.
    let err = project
        .split_terminal(fourth, PaneSplit::Column, &settings, None)
        .unwrap_err();
    assert!(err.contains('4'), "unexpected error: {err}");
    let tab = project.active_worktree().tab_by_id(tab_id).unwrap();
    assert_eq!(tab.panes.len(), MAX_PANES_PER_TAB);
    assert_eq!(tab.split, PaneSplit::Row);
    assert_eq!(tab.active_pane, fourth);
}

#[test]
fn terminal_active_pane_drives_tab_label() {
    let (_dir, _settings, mut project, first, tab_id) = split_project(2);
    let worktree = project.active_worktree();
    let tab = worktree.tab_by_id(tab_id).unwrap();
    let second = tab.panes[1].id;

    // `active_terminal` is the active pane; the tab is derived from it.
    assert_eq!(worktree.active_terminal, Some(second));
    assert_eq!(worktree.active_tab_id(), Some(tab_id));

    project.term_by_id_mut(second).unwrap().title = "pane-two".into();
    assert_eq!(
        project.active_worktree().tab_by_id(tab_id).unwrap().title(),
        "pane-two"
    );

    project.active_worktree_mut().active_terminal = Some(first);
    project
        .active_worktree_mut()
        .tab_by_id_mut(tab_id)
        .unwrap()
        .active_pane = first;
    assert_eq!(
        project.active_worktree().tab_by_id(tab_id).unwrap().title(),
        project.term_by_id(first).unwrap().title
    );
    assert_eq!(project.active_worktree().active_tab_id(), Some(tab_id));
}

#[test]
fn terminal_close_pane_keeps_tab_until_last_pane() {
    let (_dir, _settings, mut project, first, tab_id) = split_project(3);
    let tab = project.active_worktree().tab_by_id(tab_id).unwrap();
    let second = tab.panes[1].id;
    let third = tab.panes[2].id;

    // Closing the active pane moves focus to the previous pane.
    project.close_terminal(third);
    let worktree = project.active_worktree();
    assert_eq!(worktree.terminals.len(), 1, "the tab survives");
    let tab = worktree.tab_by_id(tab_id).unwrap();
    assert_eq!(tab.panes.len(), 2);
    assert_eq!(tab.active_pane, second);
    assert_eq!(worktree.active_terminal, Some(second));
    assert!(project.term_by_id(third).is_none());

    // Closing a non-active pane leaves the active pane alone.
    project.close_terminal(first);
    let worktree = project.active_worktree();
    let tab = worktree.tab_by_id(tab_id).unwrap();
    assert_eq!(tab.panes.len(), 1);
    assert_eq!(tab.split, PaneSplit::None, "single pane loses the split");
    assert_eq!(tab.active_pane, second);
    assert_eq!(worktree.active_terminal, Some(second));

    // The last pane takes the tab with it.
    project.close_terminal(second);
    assert!(project.active_worktree().terminals.is_empty());
    assert_eq!(project.active_worktree().active_terminal, None);
}

#[test]
fn terminal_close_tab_tears_down_every_pane() {
    let (_dir, settings, mut project, first, tab_a) = split_project(2);
    let second = project.active_worktree().terminals[0].panes[1].id;
    let pane_b = project
        .open_terminal(
            None,
            None,
            lazecode_terminal::SessionKind::Shell,
            &settings,
            None,
        )
        .unwrap();

    project.active_worktree_mut().active_terminal = Some(second);
    project.close_terminal_tab(tab_a);
    let worktree = project.active_worktree();
    assert_eq!(worktree.terminals.len(), 1);
    assert!(worktree.tab_by_id(tab_a).is_none());
    assert!(project.term_by_id(first).is_none());
    assert!(project.term_by_id(second).is_none());
    // The surviving tab's pane becomes active.
    assert_eq!(worktree.active_terminal, Some(pane_b));
    let last_tab = worktree.terminals[0].id;
    assert_eq!(worktree.active_tab_id(), Some(last_tab));

    // Closing the last tab leaves the worktree with no consoles.
    project.close_terminal_tab(last_tab);
    assert!(project.active_worktree().terminals.is_empty());
    assert_eq!(project.active_worktree().active_terminal, None);
}

#[test]
fn terminal_restart_replaces_only_the_targeted_pane() {
    let (_dir, settings, mut project, first, tab_id) = split_project(2);
    let second = project.active_worktree().terminals[0].panes[1].id;
    let old_first = project.term_by_id(first).unwrap().session.id;
    let old_second = project.term_by_id(second).unwrap().session.id;
    project.term_by_id_mut(first).unwrap().title = "custom".into();

    project.restart_terminal(first, &settings, None).unwrap();
    let fresh = project.term_by_id(first).unwrap();
    assert_ne!(fresh.session.id, old_first, "targeted pane respawned");
    assert_eq!(fresh.title, "custom", "restart keeps the custom title");
    assert_eq!(
        project.term_by_id(second).unwrap().session.id,
        old_second,
        "sibling pane is untouched"
    );
    let worktree = project.active_worktree();
    assert_eq!(worktree.active_terminal, Some(first));
    assert_eq!(worktree.tab_by_id(tab_id).unwrap().active_pane, first);
}

#[test]
fn terminal_close_others_keeps_active_tab_and_panes() {
    let (_dir, settings, mut project, first, tab_a) = split_project(2);
    let second = project.active_worktree().terminals[0].panes[1].id;
    let third = project
        .open_terminal(
            None,
            None,
            lazecode_terminal::SessionKind::Shell,
            &settings,
            None,
        )
        .unwrap();
    project.active_worktree_mut().active_terminal = Some(first);

    project.close_other_terminals(first);
    let worktree = project.active_worktree();
    assert_eq!(worktree.terminals.len(), 1);
    assert_eq!(worktree.terminals[0].id, tab_a);
    assert_eq!(worktree.terminals[0].panes.len(), 2);
    assert_eq!(worktree.terminals[0].active_pane, first);
    assert_eq!(worktree.active_terminal, Some(first));
    assert_eq!(worktree.active_tab_id(), Some(tab_a));
    assert!(project.term_by_id(third).is_none());
    assert!(project.term_by_id(second).is_some());
}

#[test]
fn switching_worktrees_switches_branch_changes_and_tree() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "Tester"]);
    git(&["config", "user.email", "tester@example.com"]);
    std::fs::write(root.join("main.txt"), "main\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "main"]);
    git(&["branch", "side"]);
    let linked = root.with_extension("side-worktree");
    lazecode_git::add_worktree(root, &linked, "side").unwrap();

    std::fs::write(root.join("main-only.txt"), "main change\n").unwrap();
    std::fs::write(linked.join("side-only.txt"), "side change\n").unwrap();

    let settings = Settings::default();
    let mut project = ProjectState::open(root.to_path_buf(), &settings, None).unwrap();
    let side_name = lazecode_git::list_worktrees(root)
        .unwrap()
        .into_iter()
        .find(|worktree| worktree.name != "main")
        .map(|worktree| worktree.name)
        .unwrap();
    project
        .ensure_worktree(&side_name, &settings.ignored_patterns)
        .unwrap();
    project.refresh_git_for(&linked);
    project.reload_tree(&settings);

    assert_eq!(project.branch(), "side");
    assert!(project
        .changes()
        .iter()
        .any(|change| change.path == "side-only.txt"));
    assert!(!project
        .changes()
        .iter()
        .any(|change| change.path == "main-only.txt"));
    assert_eq!(
        project.active_root(),
        linked.canonicalize().unwrap().as_path()
    );
    assert!(project
        .active_tree()
        .nodes
        .iter()
        .any(|node| node.name == "side-only.txt"));

    project
        .ensure_worktree("main", &settings.ignored_patterns)
        .unwrap();
    project.refresh_git_for(root);
    assert_eq!(project.branch(), "main");
    assert!(project
        .changes()
        .iter()
        .any(|change| change.path == "main-only.txt"));
    assert!(!project
        .changes()
        .iter()
        .any(|change| change.path == "side-only.txt"));
    assert!(project.checked_paths.is_empty());
    assert!(project.change_selection.is_empty());

    let _ = std::fs::remove_dir_all(&linked);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn editor_tabs_and_tree_are_scoped_to_the_active_worktree() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .current_dir(&root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "Tester"]);
    git(&["config", "user.email", "tester@example.com"]);
    std::fs::write(root.join("main.txt"), "main\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "main"]);
    git(&["branch", "side"]);
    let linked = root.with_extension("side-worktree");
    lazecode_git::add_worktree(&root, &linked, "side").unwrap();
    let linked = linked.canonicalize().unwrap();

    let settings = Settings::default();
    let mut project = ProjectState::open(root.clone(), &settings, None).unwrap();
    let main_tab = project.open_file(root.join("main.txt"), &settings).unwrap();
    assert_eq!(project.active_tab, Some(main_tab));

    let side_name = lazecode_git::list_worktrees(&root)
        .unwrap()
        .into_iter()
        .find(|worktree| worktree.name != "main")
        .map(|worktree| worktree.name)
        .unwrap();
    project.switch_worktree(&side_name, &settings).unwrap();

    assert_eq!(project.active_root(), linked.as_path());
    assert_ne!(
        project.active_tab,
        Some(main_tab),
        "a file opened in main must not stay active in the linked worktree"
    );
    assert!(project.active_tab.is_none());
    assert_eq!(project.active_worktree_tabs().count(), 0);

    // A file created after the linked worktree was first attached must be
    // visible when switching back to it.
    std::fs::write(linked.join("side-fresh.txt"), "fresh\n").unwrap();
    project.switch_worktree("main", &settings).unwrap();
    project.switch_worktree(&side_name, &settings).unwrap();
    assert!(
        project
            .active_tree()
            .nodes
            .iter()
            .any(|node| node.name == "side-fresh.txt"),
        "switching worktrees must rescan the tree"
    );

    let side_tab = project
        .open_file(linked.join("side-fresh.txt"), &settings)
        .unwrap();
    assert_eq!(project.active_tab, Some(side_tab));

    project.switch_worktree("main", &settings).unwrap();
    assert_eq!(
        project.active_tab,
        Some(main_tab),
        "each worktree must remember its own active editor tab"
    );
    project.switch_worktree(&side_name, &settings).unwrap();
    assert_eq!(
        project.active_tab,
        Some(side_tab),
        "returning to a worktree must restore its own active editor tab"
    );

    let _ = std::fs::remove_dir_all(&linked);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn single_files_workspace_is_singleton_and_switches_back() {
    let files = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let a = files.path().join("a.txt");
    let b = files.path().join("b.txt");
    let c = files.path().join("c.txt");
    std::fs::write(&a, "a\n").unwrap();
    std::fs::write(&b, "b\n").unwrap();
    std::fs::write(&c, "c\n").unwrap();

    let mut model = model();
    model.open_project(a.clone()).unwrap();
    assert_eq!(model.projects.len(), 1);
    assert!(model.projects[0].is_single_file);
    assert!(model.projects[0].active_git().is_none());
    assert!(model.projects[0].root.ends_with("Single files"));
    assert_eq!(model.projects[0].tabs.len(), 1);
    assert_eq!(model.recent_files.first(), Some(&a.canonicalize().unwrap()));

    model.open_files(vec![b.clone()]).unwrap();
    assert_eq!(model.projects.len(), 1, "files must share one workspace");
    assert_eq!(model.projects[0].tabs.len(), 2);
    assert_eq!(model.recent_files.first(), Some(&b.canonicalize().unwrap()));

    model.open_project(project.path().to_path_buf()).unwrap();
    assert_eq!(model.projects.len(), 2);
    assert!(!model.projects[model.active_project.unwrap()].is_single_file);

    model.open_files(vec![c.clone()]).unwrap();
    assert_eq!(model.active_project, Some(0));
    assert_eq!(model.projects[0].tabs.len(), 3);
    assert!(!model.projects[1].is_single_file);
}

#[test]
fn last_single_tab_falls_back_to_an_open_project() {
    let real_dir = tempfile::tempdir().unwrap();
    std::fs::write(real_dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    let ad_hoc_dir = tempfile::tempdir().unwrap();
    let ad_hoc = ad_hoc_dir.path().join("standalone.rs");
    std::fs::write(&ad_hoc, "fn standalone() {}\n").unwrap();

    let mut model = model();
    assert!(!model.ui.terminal_visible);
    let real = ProjectState::open(real_dir.path().to_path_buf(), &model.settings, None).unwrap();
    let real_index = model.add_loaded_project(real);
    let single_index = model.open_files(vec![ad_hoc]).unwrap();
    assert_eq!(model.active_project, Some(single_index));
    let tab = model.projects[single_index].tabs[0].id;
    assert_eq!(
        model.projects[single_index].active_worktree_tabs().count(),
        1
    );
    let TabKind::File(file) = &model.projects[single_index].tabs[0].kind else {
        panic!("expected a file tab");
    };
    assert!(!file.external, "ad-hoc Single files tabs are not external");

    model.close_tab(single_index, tab);
    assert_eq!(model.projects.len(), 1);
    assert_eq!(model.active_project, Some(real_index));
    assert!(!model.projects[0].is_single_file);
}

#[test]
fn last_single_tab_without_other_projects_returns_home() {
    let ad_hoc_dir = tempfile::tempdir().unwrap();
    let ad_hoc = ad_hoc_dir.path().join("standalone.rs");
    std::fs::write(&ad_hoc, "fn standalone() {}\n").unwrap();

    let mut model = model();
    let single_index = model.open_files(vec![ad_hoc]).unwrap();
    let tab = model.projects[single_index].tabs[0].id;
    model.ui.terminal_visible = true;
    model.close_tab(single_index, tab);
    assert!(model.projects.is_empty());
    assert_eq!(model.active_project, None);
    assert!(!model.ui.terminal_visible);
}

#[test]
fn agent_console_cap_is_global_and_ignores_initial_shell() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
    let settings = Settings {
        terminal: lazecode_core::settings::TerminalSettings {
            max_agent_consoles: 10,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut model = model();
    model.settings = settings.clone();
    let project = ProjectState::open(dir.path().to_path_buf(), &settings, None).unwrap();
    let index = model.add_loaded_project(project);
    // The auto-created login shell is not an agent console and does not count.
    assert_eq!(model.live_agent_console_count(), 0);
    for i in 0..10 {
        model
            .open_agent_console(format!("agent-{i}"), "sleep 5".into(), &settings, None)
            .unwrap();
    }
    assert_eq!(model.live_agent_console_count(), 10);
    let err = model
        .open_agent_console("overflow".into(), "sleep 5".into(), &settings, None)
        .unwrap_err();
    assert!(err.contains("10"), "{err}");
    assert_eq!(model.projects[index].active_worktree().pane_count(), 11);
}

#[test]
fn external_file_edit_is_visible_to_disk_stamp_and_reload() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("external.txt");
    std::fs::write(&path, "before\n").unwrap();
    let settings = Settings::default();
    let mut project = ProjectState::open(dir.path().to_path_buf(), &settings, None).unwrap();
    let tab = project.open_file(path.clone(), &settings).unwrap();
    let TabKind::File(file) = &mut project.tab_mut_by_id(tab).unwrap().kind else {
        panic!("expected file tab");
    };
    assert!(!file.disk_changed());
    std::fs::write(&path, "after external edit\n").unwrap();
    assert!(file.disk_changed());
    assert_ne!(file.doc.text(), "after external edit\n");
    file.doc.reload().unwrap();
    file.stamp_disk();
    assert_eq!(file.doc.text(), "after external edit\n");
    assert!(!file.disk_changed());
}

#[test]
fn missing_repository_is_not_shown_as_a_git_error() {
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings::default();
    let mut project = ProjectState::open(dir.path().to_path_buf(), &settings, None).unwrap();
    project.git_error = Some(
        "not a Git repository: Could not find a git repository in \"/tmp/nowhere\"".to_string(),
    );
    project.refresh_git();
    assert!(
        project.git_error.is_none(),
        "a plain folder must use the initialize empty state, not a red Git error"
    );
    assert!(project.git.is_none());
}

#[test]
fn new_tab_diff_skips_the_singleton_reuse() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
    std::fs::write(dir.path().join("b.txt"), "b\n").unwrap();
    let settings = Settings::default();
    let mut project = ProjectState::open(dir.path().to_path_buf(), &settings, None).unwrap();
    let first = project.open_diff_source(lazecode_core::DiffSource::Worktree {
        path: "a.txt".to_string(),
    });
    // The default diff tab is a singleton: a second source replaces it.
    let reused = project.open_diff_source(lazecode_core::DiffSource::Worktree {
        path: "b.txt".to_string(),
    });
    assert_eq!(first, reused);
    // "Show Diff in a New Tab" must add a second diff tab instead.
    let extra = project.open_diff_source_new_tab(lazecode_core::DiffSource::Worktree {
        path: "b.txt".to_string(),
    });
    assert_ne!(first, extra);
    let diffs = project
        .tabs
        .iter()
        .filter(|tab| matches!(tab.kind, TabKind::Diff(_)))
        .count();
    assert_eq!(diffs, 2);
}

#[test]
fn repository_version_snapshot_opens_a_read_only_titled_tab() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("versioned.txt"), "worktree\n").unwrap();
    let settings = Settings::default();
    let mut project = ProjectState::open(dir.path().to_path_buf(), &settings, None).unwrap();
    let source = dir.path().join("versioned.txt");
    let id = project
        .open_revision_snapshot(
            &source,
            b"committed\n",
            "versioned.txt (HEAD)".to_string(),
            &settings,
        )
        .unwrap();
    let tab = project.tabs.iter().find(|tab| tab.id == id).expect("tab");
    assert_eq!(tab.title(), "versioned.txt (HEAD)");
    match &tab.kind {
        TabKind::File(file) => {
            assert!(file.doc.read_only);
            assert_eq!(file.doc.text(), "committed\n");
            // The temp snapshot must not alias the worktree file.
            assert_ne!(file.doc.path, source);
        }
        _ => panic!("expected a file tab"),
    }
}
