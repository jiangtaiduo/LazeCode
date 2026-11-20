//! Stash tab interactions: each stash entry expands/collapses on its own and
//! right-clicking entries, files and folders opens the reference menus.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use egui::{Pos2, Vec2};
use lazecode_core::{AppModel, ProjectState, Settings, SidebarView, UiState};
use lazecode_ui::runtime::{CommitTab, MenuKind};
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
    clicks: &[(Pos2, egui::PointerButton, bool)],
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
    let events: Vec<egui::Event> = clicks
        .iter()
        .map(|(pos, button, pressed)| egui::Event::PointerButton {
            pos: *pos,
            button: *button,
            pressed: *pressed,
            modifiers: egui::Modifiers::default(),
        })
        .collect();
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
    output.textures_delta.clear();
}

fn stash_fixture(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lazecode-ui-stash-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture dir");
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .current_dir(&dir)
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "T"]);
    git(&["config", "user.email", "t@e"]);
    std::fs::write(dir.join("a.txt"), "one\n").expect("write");
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "c1"]);
    std::fs::write(dir.join("a.txt"), "one\ntwo\n").expect("write");
    lazecode_git::stash_push(&dir, Some("first stash"), false, false).expect("first stash");
    std::fs::write(dir.join("a.txt"), "one\nthree\n").expect("write");
    lazecode_git::stash_push(&dir, Some("second stash"), false, false).expect("second stash");
    dir
}

fn stash_runtime(root: &Path, settings: &Settings) -> (AppModel, UiRuntime) {
    let mut model = model(settings);
    let project = ProjectState::open(root.to_path_buf(), settings, None).expect("project");
    model.projects.push(project);
    model.active_project = Some(0);
    model.ui.sidebar_visible = true;
    model.ui.sidebar_view = SidebarView::Git;
    let mut rt = UiRuntime::default();
    rt.commit_tab = CommitTab::Stash;
    (model, rt)
}

/// Press+release pairs across a rectangle until `done` holds.
fn click_until(
    ctx: &egui::Context,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    button: egui::PointerButton,
    mut done: impl FnMut(&AppModel, &UiRuntime) -> bool,
) -> bool {
    for y in (88..190).step_by(3) {
        for x in (16..260).step_by(6) {
            let pos = Pos2::new(x as f32, y as f32);
            frame(ctx, model, rt, &[(pos, button, true)]);
            frame(ctx, model, rt, &[(pos, button, false)]);
            frame(ctx, model, rt, &[]);
            if done(model, rt) {
                return true;
            }
        }
    }
    false
}

#[test]
fn every_stash_entry_collapses_independently() {
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let root = stash_fixture("collapse");
    let (mut model, mut rt) = stash_runtime(&root, &settings);
    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);

    frame(&ctx, &mut model, &mut rt, &[]);
    assert_eq!(rt.stash_entries.len(), 2, "both stashes listed");
    assert_eq!(rt.stash_selected.as_deref(), Some("stash@{0}"));
    assert!(rt.stash_expanded.contains("stash@{0}"));
    assert!(!rt.stash_expanded.contains("stash@{1}"));

    // Clicking the first entry collapses only that entry's tree.
    let collapsed = click_until(
        &ctx,
        &mut model,
        &mut rt,
        egui::PointerButton::Primary,
        |_, rt| !rt.stash_expanded.contains("stash@{0}"),
    );
    assert!(collapsed, "first entry must toggle closed");
    assert_eq!(rt.stash_selected.as_deref(), Some("stash@{0}"));
    assert_eq!(rt.commit_tab, CommitTab::Stash, "tab stays selected");
}

#[test]
fn stash_entries_files_and_dirs_share_the_reference_menus() {
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let root = stash_fixture("menus");
    let (mut model, mut rt) = stash_runtime(&root, &settings);
    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);
    frame(&ctx, &mut model, &mut rt, &[]);

    // Entry right-click → the stash entry menu carrying that entry.
    let entry_menu = click_until(
        &ctx,
        &mut model,
        &mut rt,
        egui::PointerButton::Secondary,
        |_, rt| {
            rt.menu.as_ref().is_some_and(|menu| {
                menu.kind == MenuKind::StashEntryMenu { project: 0 }
                    && menu.payload.as_deref() == Some("stash@{0}")
            })
        },
    );
    assert!(entry_menu, "entry context menu opens on the stash row");
    rt.menu = None;
    frame(&ctx, &mut model, &mut rt, &[]);

    // File right-click → the six-item file menu, payload marks a file.
    let file_menu = click_until(
        &ctx,
        &mut model,
        &mut rt,
        egui::PointerButton::Secondary,
        |_, rt| {
            rt.menu.as_ref().is_some_and(|menu| {
                menu.kind == MenuKind::StashFileMenu { project: 0 }
                    && menu
                        .payload
                        .as_deref()
                        .is_some_and(|payload| payload.ends_with("\u{1}f\u{1}a.txt"))
            })
        },
    );
    assert!(file_menu, "file context menu opens on a stash file row");
}
