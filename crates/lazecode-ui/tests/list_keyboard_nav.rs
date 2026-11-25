//! Regression: list surfaces answer Up/Down/Home/End while the pointer is over
//! them (or, for modal panels, while their own query field owns the keyboard),
//! and stay quiet when a typing surface owns the keys.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use egui::{Event, Key, Modifiers, Pos2, RawInput, Rect, Vec2, ViewportId, ViewportInfo};
use lazecode_core::{AppModel, Settings, UiState};
use lazecode_fs::SearchHit;
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
