//! Regression: closing the last tab of a Single files workspace used to
//! panic mid-frame — `editor::draw` kept indexing the project vec after the
//! close button had emptied it (`settle_single_files_workspace` drops the
//! emptied ad-hoc checkout between the tab strip and the editor body).

use std::collections::HashSet;
use std::path::PathBuf;

use egui::{Pos2, Vec2};
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

fn frame(
    ctx: &egui::Context,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    click: Option<(Pos2, bool)>,
) {
    let rect = egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(W, H));
    let mut viewports = egui::ViewportIdMap::default();
    viewports.insert(
        egui::ViewportId::ROOT,
        egui::ViewportInfo {
            native_pixels_per_point: Some(SCALE),
            inner_rect: Some(rect),
            outer_rect: Some(rect),
            focused: Some(true),
            ..Default::default()
        },
    );
    let mut events = Vec::new();
    if let Some((pos, pressed)) = click {
        events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        });
    }
    let raw = egui::RawInput {
        screen_rect: Some(rect),
        viewports,
        time: Some(1.0),
        events,
        ..Default::default()
    };
    let mut output = ctx.run_ui(raw, |ui| {
        lazecode_ui::app_ui::draw(ui, model, rt);
    });
    // The real renderer applies these on the GPU; the test only needs the
    // draw-side model effects, so drop them the documented way.
    output.textures_delta.clear();
}

/// Click across the tab strip until the ✕ hitbox lands. Press and release on
/// the same spot reproduce the real mid-frame close click.
fn click_until(
    ctx: &egui::Context,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    mut done: impl FnMut(&AppModel) -> bool,
) -> bool {
    for y in (20..100).step_by(4) {
        for x in (60..500).step_by(6) {
            let pos = Pos2::new(x as f32, y as f32);
            frame(ctx, model, rt, Some((pos, true)));
            frame(ctx, model, rt, Some((pos, false)));
            if done(model) {
                return true;
            }
        }
    }
    false
}

fn single_file_fixture(name: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("lazecode-ui-close-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create fixture dir");
    let file = dir.join("single.txt");
    std::fs::write(&file, "hello\n").expect("write fixture file");
    (dir, file)
}

#[test]
fn closing_last_single_file_tab_falls_back_to_home_without_panicking() {
    let (dir, file) = single_file_fixture("home");
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let mut model = model(&settings);
    model
        .open_files(vec![file.clone()])
        .expect("open single file");
    assert_eq!(model.projects.len(), 1);
    assert!(model.projects[0].is_single_file);

    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);
    let mut rt = UiRuntime::default();
    frame(&ctx, &mut model, &mut rt, None);

    let settled = click_until(&ctx, &mut model, &mut rt, |model| model.projects.is_empty());
    assert!(settled, "sweep never closed the last single-file tab");

    // The emptied ad-hoc checkout is dropped and the home screen shows.
    assert!(model.active_project.is_none());
    // The frame after the settle must render the welcome screen without panic.
    frame(&ctx, &mut model, &mut rt, None);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn closing_last_single_file_tab_switches_to_the_other_project() {
    let (dir, file) = single_file_fixture("project");
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let mut model = model(&settings);
    let project_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut project =
        lazecode_core::ProjectState::open(project_dir.clone(), &settings, None).expect("project");
    let _ = project.open_file(project_dir.join("Cargo.toml"), &settings);
    model.projects.push(project);
    model.active_project = Some(0);
    model
        .open_files(vec![file.clone()])
        .expect("open single file");

    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);
    let mut rt = UiRuntime::default();
    frame(&ctx, &mut model, &mut rt, None);

    let settled = click_until(&ctx, &mut model, &mut rt, |model| {
        !model
            .projects
            .iter()
            .any(|project| project.is_single_file && !project.tabs.is_empty())
    });
    assert!(settled, "sweep never closed the single-file tabs");

    // The ad-hoc checkout is gone and the remaining project took over.
    assert_eq!(model.projects.len(), 1);
    assert!(!model.projects[0].is_single_file);
    assert_eq!(model.active_project, Some(0));
    // Drawing after the settle must not panic on a stale project index.
    frame(&ctx, &mut model, &mut rt, None);

    let _ = std::fs::remove_dir_all(&dir);
}
