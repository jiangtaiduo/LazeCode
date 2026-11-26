//! Regression: list surfaces answer Up/Down/Home/End while the pointer is over
//! them (or, for modal panels, while their own query field owns the keyboard),
//! and stay quiet when a typing surface owns the keys.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use egui::{Event, Key, Modifiers, Pos2, RawInput, Rect, Vec2, ViewportId, ViewportInfo};
use lazecode_core::{AppModel, Settings, SidebarView, UiState};
use lazecode_fs::SearchHit;
use lazecode_git::{Change, ChangeKind, RepoSnapshot};
use lazecode_ui::runtime::{CommitTab, MenuKind, MenuState};
use lazecode_ui::UiRuntime;

const W: f32 = 1440.0;
const H: f32 = 900.0;
const SCALE: f32 = 2.0;

fn model(settings: &Settings) -> AppModel {
    AppModel {
        settings: settings.clone(),
        projects: Vec::new(),
        active_project: None,
        ui: UiState::default(),
        toasts: Vec::new(),
        recent: Vec::new(),
        recent_files: Vec::new(),
        commands: Vec::new(),
        notifier: None,
        lsp: lazecode_core::LspManager::new(false, None),
        should_quit: false,
        version: "test",
        favorite_branches: HashSet::new(),
        git_cmd_log: Vec::new(),
    }
}

fn key(key: Key) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}

fn pointer(x: f32, y: f32) -> Event {
    Event::PointerMoved(Pos2::new(x, y))
}

fn frame(
    ctx: &egui::Context,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    events: Vec<Event>,
    t: f64,
) {
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(W, H));
    let mut viewports = egui::ViewportIdMap::default();
    viewports.insert(
        ViewportId::ROOT,
        ViewportInfo {
            native_pixels_per_point: Some(SCALE),
            inner_rect: Some(rect),
            outer_rect: Some(rect),
            focused: Some(true),
            ..Default::default()
        },
    );
    let raw = RawInput {
        screen_rect: Some(rect),
        viewports,
        time: Some(t),
        events,
        ..Default::default()
    };
    let mut output = ctx.run_ui(raw, |ui| {
        lazecode_ui::app_ui::draw(ui, model, rt);
    });
    output.textures_delta.clear();
}

fn project_fixture(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lazecode-ui-list-nav-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create project dir");
    for (path, body) in files {
        std::fs::write(dir.join(path), body).expect("write fixture file");
    }
    dir
}

fn open_project(model: &mut AppModel, settings: &Settings, dir: &Path) -> usize {
    let project =
        lazecode_core::ProjectState::open(dir.to_path_buf(), settings, None).expect("open project");
    let index = model.projects.len();
    model.projects.push(project);
    index
}

fn hit(path: &str, line: usize) -> SearchHit {
    SearchHit {
        path: PathBuf::from(path),
        line,
        start: 0,
        end: 6,
        line_text: "needle in a haystack".to_string(),
    }
}
fn click(pos: Pos2, modifiers: Modifiers) -> Vec<Event> {
    let mut events = Vec::new();
    if modifiers.shift || modifiers.command || modifiers.ctrl || modifiers.alt {
        events.push(Event::ModifiersChanged(modifiers));
    }
    events.extend([
        Event::PointerMoved(pos),
        Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers,
        },
        Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers,
        },
    ]);
    events
}

fn shortcut(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn tree_menu(rel: &str) -> MenuState {
    MenuState {
        kind: MenuKind::TreeContext {
            project: 0,
            is_dir: true,
        },
        anchor: Rect::from_min_size(Pos2::new(100.0, 100.0), Vec2::splat(4.0)),
        payload: Some(rel.to_string()),
        opened_frame: 0,
    }
}

fn git_change(path: &str) -> Change {
    Change {
        path: path.to_string(),
        staged: Some(ChangeKind::Modified),
        unstaged: None,
        untracked: false,
        conflicted: false,
        additions: 1,
        deletions: 0,
        binary: false,
    }
}

#[test]
fn search_results_step_with_arrow_keys_from_the_query_field() {
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let mut model = model(&settings);
    let dir = project_fixture("search", &[("a.rs", "fn main() {}\n")]);
    let active = open_project(&mut model, &settings, &dir);
    model.active_project = Some(active);

    let mut rt = UiRuntime::default();
    rt.search.open = true;
    rt.search.query = "needle".to_string();
    rt.search.focus_requested = true;

    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);
    // First frame records the query signature and starts the debounce; the
    // background search never runs in this harness, so seed the results by
    // hand afterwards.
    frame(&ctx, &mut model, &mut rt, Vec::new(), 1.0);
    rt.search.dirty_since = None;
    rt.search.results = vec![hit("a.rs", 1), hit("b.rs", 2), hit("c.rs", 3)];
    rt.search.selected = 0;
    frame(&ctx, &mut model, &mut rt, Vec::new(), 1.1);
    assert_eq!(rt.search.selected, 0, "no keys, no move");

    // The query TextEdit owns the keyboard; ↑/↓ must still walk the matches
    // (its caret keys are single-line left/right).
    frame(&ctx, &mut model, &mut rt, vec![key(Key::ArrowDown)], 1.2);
    assert_eq!(rt.search.selected, 1, "↓ did not move the match selection");
    frame(&ctx, &mut model, &mut rt, vec![key(Key::ArrowDown)], 1.3);
    assert_eq!(rt.search.selected, 2);
    frame(&ctx, &mut model, &mut rt, vec![key(Key::ArrowDown)], 1.4);
    assert_eq!(
        rt.search.selected, 2,
        "selection must clamp at the last row"
    );
    frame(&ctx, &mut model, &mut rt, vec![key(Key::ArrowUp)], 1.5);
    assert_eq!(rt.search.selected, 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tree_selection_steps_only_while_the_pointer_is_over_the_tree() {
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let mut model = model(&settings);
    let dir = project_fixture(
        "tree",
        &[
            ("a.rs", "fn a() {}\n"),
            ("b.rs", "fn b() {}\n"),
            ("c.rs", "fn c() {}\n"),
        ],
    );
    let active = open_project(&mut model, &settings, &dir);
    model.active_project = Some(active);
    model.ui.sidebar_view = lazecode_core::SidebarView::Files;

    let mut rt = UiRuntime::default();
    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);
    // The first frame mirrors the active checkout into the view state, which
    // clears any selection carried in; seed it afterwards, like a click would.
    frame(&ctx, &mut model, &mut rt, Vec::new(), 0.9);
    model.ui.tree_selected = Some(PathBuf::from("a.rs"));
    model.ui.tree_selection.insert(PathBuf::from("a.rs"));
    // Pointer parked over the tree.
    frame(
        &ctx,
        &mut model,
        &mut rt,
        vec![pointer(90.0, 320.0), key(Key::ArrowDown)],
        1.0,
    );
    assert_eq!(
        model.ui.tree_selected.as_deref(),
        Some(Path::new("b.rs")),
        "↓ over the tree did not advance the selection"
    );
    frame(
        &ctx,
        &mut model,
        &mut rt,
        vec![pointer(90.0, 320.0), key(Key::ArrowDown)],
        1.1,
    );
    assert_eq!(model.ui.tree_selected.as_deref(), Some(Path::new("c.rs")));
    frame(
        &ctx,
        &mut model,
        &mut rt,
        vec![pointer(90.0, 320.0), key(Key::ArrowDown)],
        1.2,
    );
    assert_eq!(
        model.ui.tree_selected.as_deref(),
        Some(Path::new("c.rs")),
        "↑/↓ must clamp at the ends"
    );
    frame(
        &ctx,
        &mut model,
        &mut rt,
        vec![pointer(90.0, 320.0), key(Key::Home)],
        1.3,
    );
    assert_eq!(
        model.ui.tree_selected.as_deref(),
        Some(Path::new("")),
        "Home selects the project root row"
    );

    // Pointer leaves the tree: arrows belong to whatever pane is under it.
    frame(
        &ctx,
        &mut model,
        &mut rt,
        vec![pointer(700.0, 320.0), key(Key::ArrowDown)],
        1.4,
    );
    assert_eq!(
        model.ui.tree_selected.as_deref(),
        Some(Path::new("")),
        "↓ over the editor must not move the tree selection"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tree_arrows_yield_to_a_focused_editor() {
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let mut model = model(&settings);
    let dir = project_fixture(
        "tree-editor",
        &[("a.rs", "fn a() {}\n"), ("b.rs", "fn b() {}\n")],
    );
    let active = open_project(&mut model, &settings, &dir);
    model.active_project = Some(active);
    model.ui.sidebar_view = lazecode_core::SidebarView::Files;

    let mut rt = UiRuntime::default();
    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);
    // Warm-up frame: mirrors the checkout into the view state before the test
    // seeds the selection.
    frame(&ctx, &mut model, &mut rt, Vec::new(), 0.9);
    model.ui.tree_selected = Some(PathBuf::from("a.rs"));
    model.ui.tree_selection.insert(PathBuf::from("a.rs"));
    let tab = model.projects[active]
        .open_file(dir.join("a.rs"), &settings)
        .expect("open source file");
    rt.set_editor_focus(tab);
    frame(&ctx, &mut model, &mut rt, vec![pointer(90.0, 320.0)], 1.0);
    frame(
        &ctx,
        &mut model,
        &mut rt,
        vec![pointer(90.0, 320.0), key(Key::ArrowDown)],
        1.1,
    );
    assert_eq!(
        model.ui.tree_selected.as_deref(),
        Some(Path::new("a.rs")),
        "a focused editor owns the arrow keys even with the pointer over the tree"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
#[test]
fn search_shortcut_yields_to_modals_and_menus() {
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let mut model = model(&settings);
    let dir = project_fixture("search-shortcut", &[("a.rs", "fn a() {}\n")]);
    let active = open_project(&mut model, &settings, &dir);
    model.active_project = Some(active);
    let mut rt = UiRuntime::default();
    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);
    let open_search = shortcut(Key::F, Modifiers::COMMAND | Modifiers::SHIFT);

    frame(&ctx, &mut model, &mut rt, vec![open_search.clone()], 1.0);
    assert!(rt.search.open, "⌘⇧F must open search on a free frame");

    rt.search.open = false;
    rt.settings_open = true;
    frame(&ctx, &mut model, &mut rt, vec![open_search.clone()], 1.1);
    assert!(!rt.search.open, "settings must swallow ⌘⇧F");
    assert!(rt.settings_open);

    rt.settings_open = false;
    rt.menu = Some(tree_menu(""));
    frame(&ctx, &mut model, &mut rt, vec![open_search], 1.2);
    assert!(!rt.search.open, "an open menu must swallow ⌘⇧F");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn search_keys_require_query_focus_and_no_other_overlay() {
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let mut model = model(&settings);
    let dir = project_fixture("search-focus", &[("a.rs", "fn a() {}\n")]);
    let active = open_project(&mut model, &settings, &dir);
    model.active_project = Some(active);
    let mut rt = UiRuntime::default();
    rt.search.open = true;
    rt.search.query = "needle".to_string();
    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);
    frame(&ctx, &mut model, &mut rt, Vec::new(), 1.0);
    rt.search.dirty_since = None;
    rt.search.results = vec![hit("a.rs", 1), hit("b.rs", 2), hit("c.rs", 3)];
    rt.search.selected = 0;

    ctx.memory_mut(|mem| mem.request_focus(egui::Id::unique("outside-search")));
    frame(&ctx, &mut model, &mut rt, vec![key(Key::ArrowDown)], 1.1);
    assert_eq!(rt.search.selected, 0, "a foreign focus owns the arrow keys");

    rt.search.focus_requested = true;
    rt.menu = Some(tree_menu(""));
    frame(&ctx, &mut model, &mut rt, vec![key(Key::ArrowDown)], 1.2);
    assert_eq!(
        rt.search.selected, 0,
        "a second overlay owns the arrow keys"
    );

    rt.menu = None;
    rt.search.focus_requested = true;
    frame(&ctx, &mut model, &mut rt, vec![key(Key::ArrowDown)], 1.3);
    assert_eq!(
        rt.search.selected, 1,
        "the query field may step the matches"
    );

    frame(&ctx, &mut model, &mut rt, vec![key(Key::End)], 1.4);
    assert_eq!(rt.search.selected, 1, "Home/End stay with the text caret");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn search_enter_runs_only_from_the_focused_query_field() {
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let mut model = model(&settings);
    let dir = project_fixture("search-enter", &[("a.rs", "fn a() {}\n")]);
    let active = open_project(&mut model, &settings, &dir);
    model.active_project = Some(active);
    let mut rt = UiRuntime::default();
    rt.search.open = true;
    rt.search.query = "needle".to_string();
    rt.search.request_id = 5;
    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);
    frame(&ctx, &mut model, &mut rt, Vec::new(), 1.0);
    rt.search.dirty_since = None;
    let baseline = rt.search.request_id;

    ctx.memory_mut(|mem| mem.request_focus(egui::Id::unique("outside-search")));
    frame(&ctx, &mut model, &mut rt, vec![key(Key::Enter)], 1.1);
    assert_eq!(
        rt.search.request_id, baseline,
        "Enter outside the query must not search"
    );

    rt.search.focus_requested = true;
    rt.settings_open = true;
    frame(&ctx, &mut model, &mut rt, vec![key(Key::Enter)], 1.2);
    assert_eq!(
        rt.search.request_id, baseline,
        "a second overlay owns Enter too"
    );

    rt.settings_open = false;
    rt.search.focus_requested = true;
    frame(&ctx, &mut model, &mut rt, vec![key(Key::Enter)], 1.3);
    assert_eq!(
        rt.search.request_id,
        baseline + 1,
        "Enter in the query field runs the search"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn escape_closes_one_overlay_per_frame() {
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let mut model = model(&settings);
    let dir = project_fixture("escape", &[("a.rs", "fn a() {}\n")]);
    let active = open_project(&mut model, &settings, &dir);
    model.active_project = Some(active);
    let mut rt = UiRuntime::default();
    rt.search.open = true;
    rt.settings_open = true;
    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);

    frame(&ctx, &mut model, &mut rt, vec![key(Key::Escape)], 1.0);
    assert!(!rt.search.open, "search handles its own Escape");
    assert!(
        rt.settings_open,
        "the shared Escape handler must not close a second layer in the same frame"
    );

    frame(&ctx, &mut model, &mut rt, vec![key(Key::Escape)], 1.1);
    assert!(!rt.settings_open, "the next Escape closes the next layer");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn escape_does_not_close_search_and_menu_together() {
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let mut model = model(&settings);
    let dir = project_fixture("escape-menu", &[("a.rs", "fn a() {}\n")]);
    let active = open_project(&mut model, &settings, &dir);
    model.active_project = Some(active);
    let mut rt = UiRuntime::default();
    rt.search.open = true;
    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);
    // Warm-up initializes the overlay snapshot before the menu is opened.
    frame(&ctx, &mut model, &mut rt, Vec::new(), 0.9);
    rt.menu = Some(tree_menu(""));
    // The menu must be visible for one frame so the overlay snapshot includes
    // it; a menu opened mid-frame would own Escape before search sees it.
    frame(&ctx, &mut model, &mut rt, Vec::new(), 0.95);

    frame(&ctx, &mut model, &mut rt, vec![key(Key::Escape)], 1.0);
    assert!(rt.menu.is_none(), "the menu handles its own Escape");
    assert!(
        rt.search.open,
        "search must not close on the same key press"
    );

    frame(&ctx, &mut model, &mut rt, vec![key(Key::Escape)], 1.1);
    assert!(!rt.search.open, "the next Escape closes search");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn changes_navigation_uses_file_rows_and_skips_headers() {
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let mut model = model(&settings);
    let dir = project_fixture(
        "changes",
        &[("a.rs", "a\n"), ("b.rs", "b\n"), ("c.rs", "c\n")],
    );
    let active = open_project(&mut model, &settings, &dir);
    model.active_project = Some(active);
    model.ui.sidebar_view = SidebarView::Git;
    model.projects[active].active_worktree_mut().git = Some(RepoSnapshot {
        root: dir.clone(),
        branch: Some("main".to_string()),
        detached: false,
        unborn: false,
        ahead: 0,
        behind: 0,
        changes: vec![git_change("a.rs"), git_change("b.rs"), git_change("c.rs")],
        ignored_files: Vec::new(),
        ignored_dirs: Vec::new(),
        head: None,
    });
    let mut rt = UiRuntime::default();
    rt.commit_tab = CommitTab::Commit;
    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);
    frame(&ctx, &mut model, &mut rt, Vec::new(), 0.9);
    model.projects[active].change_anchor = Some("a.rs".to_string());
    model.projects[active]
        .change_selection
        .insert("a.rs".to_string());

    frame(
        &ctx,
        &mut model,
        &mut rt,
        vec![pointer(90.0, 150.0), key(Key::ArrowDown)],
        1.0,
    );
    assert_eq!(
        model.projects[active].change_anchor.as_deref(),
        Some("b.rs"),
        "↓ must step file rows even though a group header precedes them"
    );
    frame(
        &ctx,
        &mut model,
        &mut rt,
        vec![pointer(90.0, 150.0), key(Key::End)],
        1.1,
    );
    assert_eq!(
        model.projects[active].change_anchor.as_deref(),
        Some("c.rs")
    );
    frame(
        &ctx,
        &mut model,
        &mut rt,
        vec![pointer(90.0, 150.0), key(Key::Home)],
        1.2,
    );
    assert_eq!(
        model.projects[active].change_anchor.as_deref(),
        Some("a.rs")
    );

    frame(
        &ctx,
        &mut model,
        &mut rt,
        click(Pos2::new(100.0, 180.0), Modifiers::SHIFT),
        1.3,
    );
    let selection = &model.projects[active].change_selection;
    assert_eq!(
        selection.len(),
        3,
        "Shift range must cover all three files (anchor={:?}, selection={:?})",
        model.projects[active].change_anchor,
        selection
    );
    assert!(selection.contains("a.rs") && selection.contains("b.rs") && selection.contains("c.rs"));

    rt.settings_open = true;
    frame(
        &ctx,
        &mut model,
        &mut rt,
        vec![pointer(90.0, 150.0), key(Key::ArrowDown)],
        1.4,
    );
    assert_eq!(
        model.projects[active].change_anchor.as_deref(),
        Some("a.rs"),
        "a modal must freeze the changes selection"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tree_keyboard_end_and_reanchors_after_a_file_disappears() {
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let mut model = model(&settings);
    let dir = project_fixture(
        "tree-end",
        &[("a.rs", "a\n"), ("b.rs", "b\n"), ("c.rs", "c\n")],
    );
    let active = open_project(&mut model, &settings, &dir);
    model.active_project = Some(active);
    model.ui.sidebar_view = SidebarView::Files;
    let mut rt = UiRuntime::default();
    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);
    frame(&ctx, &mut model, &mut rt, Vec::new(), 0.9);
    model.ui.tree_selected = Some(PathBuf::from("a.rs"));
    model.ui.tree_selection.insert(PathBuf::from("a.rs"));

    frame(
        &ctx,
        &mut model,
        &mut rt,
        vec![pointer(90.0, 320.0), key(Key::End)],
        1.0,
    );
    assert_eq!(model.ui.tree_selected.as_deref(), Some(Path::new("c.rs")));

    std::fs::remove_file(dir.join("c.rs")).expect("remove selected file");
    model.projects[active].reload_tree(&settings);
    frame(
        &ctx,
        &mut model,
        &mut rt,
        vec![pointer(90.0, 320.0), key(Key::ArrowDown)],
        1.1,
    );
    assert_ne!(
        model.ui.tree_selected.as_deref(),
        Some(Path::new("c.rs")),
        "a vanished selection must not stay on a removed row"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
