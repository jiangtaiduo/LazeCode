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
