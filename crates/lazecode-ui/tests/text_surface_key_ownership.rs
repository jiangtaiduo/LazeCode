//! Regression: a focused console owns the keyboard, so egui's Tab focus
//! navigation must not park focus on a clickable widget. egui activates a
//! focused `Sense::click` widget with Enter/Space (FAKE_PRIMARY_CLICKED), so
//! Tab-completing a shell command and then pressing Enter also clicked the
//! focused project-rail tile and switched projects mid-command.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use egui::{Event, Key, Modifiers, Pos2, RawInput, Rect, Vec2, ViewportId, ViewportInfo};
use lazecode_core::{AppModel, Settings, UiState};
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

fn project_fixture(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lazecode-ui-console-keys-{}-{name}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create project dir");
    dir
}

fn open_project(model: &mut AppModel, settings: &Settings, dir: &Path) -> usize {
    let project =
        lazecode_core::ProjectState::open(dir.to_path_buf(), settings, None).expect("open project");
    let index = model.projects.len();
    model.projects.push(project);
    index
}

#[test]
fn tab_completion_then_enter_keeps_the_console_project() {
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let mut model = model(&settings);
    let first = project_fixture("first");
    let second = project_fixture("second");
    open_project(&mut model, &settings, &first);
    let active = open_project(&mut model, &settings, &second);
    model.active_project = Some(active);
    model.ui.terminal_visible = true;

    let pane = model.projects[active]
        .open_terminal(
            None,
            None,
            lazecode_terminal::SessionKind::Shell,
            &settings,
            None,
        )
        .expect("open console");
    let mut rt = UiRuntime::default();
    rt.set_terminal_focus(pane);

    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);
    frame(&ctx, &mut model, &mut rt, Vec::new(), 1.0);

    // Tab is shell completion. It must not hand egui's keyboard focus to the
    // first clickable widget (the project rail draws first).
    frame(&ctx, &mut model, &mut rt, vec![key(Key::Tab)], 1.1);
    assert!(
        ctx.memory(|memory| memory.focused()).is_none(),
        "Tab in a focused console left egui focus on a clickable widget"
    );
    // Enter is the command's newline, not a click on whatever Tab focused.
    frame(&ctx, &mut model, &mut rt, vec![key(Key::Enter)], 1.2);

    assert_eq!(
        model.active_project,
        Some(active),
        "Enter in a focused console switched projects"
    );

    model.projects[active]
        .term_by_id(pane)
        .expect("console still alive")
        .session
        .kill();
    let _ = std::fs::remove_dir_all(&first);
    let _ = std::fs::remove_dir_all(&second);
}

#[test]
fn tab_indent_then_enter_keeps_the_editor_project() {
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let mut model = model(&settings);
    let first = project_fixture("first-editor");
    let second = project_fixture("second-editor");
    std::fs::write(second.join("main.rs"), "fn main() {}\n").expect("write source");
    open_project(&mut model, &settings, &first);
    let active = open_project(&mut model, &settings, &second);
    model.active_project = Some(active);

    let tab = model.projects[active]
        .open_file(second.join("main.rs"), &settings)
        .expect("open source file");
    let mut rt = UiRuntime::default();
    rt.set_editor_focus(tab);

    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);
    frame(&ctx, &mut model, &mut rt, Vec::new(), 1.0);

    // Tab indents code. It must not hand egui's keyboard focus to the first
    // clickable widget and let the next Enter click it.
    frame(&ctx, &mut model, &mut rt, vec![key(Key::Tab)], 1.1);
    assert!(
        ctx.memory(|memory| memory.focused()).is_none(),
        "Tab in a focused editor left egui focus on a clickable widget"
    );
    frame(&ctx, &mut model, &mut rt, vec![key(Key::Enter)], 1.2);

    assert_eq!(
        model.active_project,
        Some(active),
        "Enter in a focused editor switched projects"
    );

    let _ = std::fs::remove_dir_all(&first);
    let _ = std::fs::remove_dir_all(&second);
}
