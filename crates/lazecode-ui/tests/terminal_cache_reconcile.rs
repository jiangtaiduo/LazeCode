//! Regression: per-pane UI caches must not outlive their model panes. The
//! core can close panes on its own — `settle_single_files_workspace` drops an
//! emptied Single files project outright — without running the UI's close
//! path, so the shell reconciles the caches against the model once per frame.

use lazecode_core::{AppModel, ProjectState, Settings, UiState};
use lazecode_terminal::{SessionKind, Snapshot};
use lazecode_ui::UiRuntime;

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
        favorite_branches: Default::default(),
        git_cmd_log: Vec::new(),
    }
}

fn project_model(settings: &Settings, name: &str) -> (AppModel, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "lazecode-ui-cache-prune-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create project dir");
    let mut model = model(settings);
    let project = ProjectState::open(dir.clone(), settings, None).expect("open project");
    model.projects.push(project);
    model.active_project = Some(0);
    model.ui.terminal_visible = true;
    (model, dir)
}

#[test]
fn model_closed_panes_are_pruned_from_ui_caches() {
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let (mut model, dir) = project_model(&settings, "closed");
    let pane = model.projects[0]
        .open_terminal(None, None, SessionKind::Shell, &settings, None)
        .expect("open console");
    let mut rt = UiRuntime::default();
    rt.set_terminal_focus(pane);
    // Populate the caches the draw path would own for this pane.
    rt.terminal_snapshots.insert(pane, Snapshot::default());
    rt.terminal_selections.insert(pane, None);
    rt.terminal_preedit.insert(pane, "x".to_string());
    rt.terminal_wheel.insert(pane, 3.0);

    // The core closes the pane behind the UI's back, as
    // `settle_single_files_workspace` does when the last single-file tab
    // closes.
    model.projects[0].close_terminal(pane);
    assert!(
        model.projects[0].term_by_id(pane).is_none(),
        "pane really left the model"
    );

    rt.prune_terminal_caches(&model);

    assert!(
        rt.terminal_snapshots.is_empty(),
        "snapshot of a model-closed pane survived"
    );
    assert!(rt.terminal_selections.is_empty());
    assert!(rt.terminal_preedit.is_empty());
    assert!(rt.terminal_wheel.is_empty());
    assert_eq!(
        rt.terminal_focus, None,
        "focus still points at a model-closed pane"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn live_panes_keep_their_caches() {
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let (mut model, dir) = project_model(&settings, "live");
    let pane = model.projects[0]
        .open_terminal(None, None, SessionKind::Shell, &settings, None)
        .expect("open console");
    let mut rt = UiRuntime::default();
    rt.set_terminal_focus(pane);
    rt.terminal_snapshots.insert(pane, Snapshot::default());
    rt.terminal_preedit.insert(pane, "x".to_string());

    rt.prune_terminal_caches(&model);

    assert!(
        rt.terminal_snapshots.contains_key(&pane),
        "a live pane's last complete frame was pruned"
    );
    assert_eq!(
        rt.terminal_preedit.get(&pane).map(String::as_str),
        Some("x")
    );
    assert_eq!(rt.terminal_focus, Some(pane));

    if let Some(term) = model.projects[0].term_by_id(pane) {
        term.session.kill();
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The draw path runs the reconciliation every frame, so a cache seeded for a
/// pane that never existed (e.g. left behind by an earlier project) is gone
/// after one frame.
#[test]
fn draw_prunes_stale_caches() {
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let (mut model, dir) = project_model(&settings, "draw");
    let mut rt = UiRuntime::default();
    rt.terminal_snapshots.insert(9_999_999, Snapshot::default());

    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::new(1440.0, 900.0));
    let mut viewports = egui::ViewportIdMap::default();
    viewports.insert(
        egui::ViewportId::ROOT,
        egui::ViewportInfo {
            native_pixels_per_point: Some(2.0),
            inner_rect: Some(rect),
            outer_rect: Some(rect),
            focused: Some(true),
            ..Default::default()
        },
    );
    let raw = egui::RawInput {
        screen_rect: Some(rect),
        viewports,
        time: Some(1.0),
        ..Default::default()
    };
    let mut output = ctx.run_ui(raw, |ui| {
        lazecode_ui::app_ui::draw(ui, &mut model, &mut rt);
    });
    output.textures_delta.clear();

    // `ProjectState::open` starts one console, so the map is not empty after
    // the frame; the prune must only have dropped the pane the model never
    // owned.
    let live: Vec<u64> = model.projects[0]
        .worktrees
        .iter()
        .flat_map(|worktree| worktree.terminals.iter())
        .flat_map(|tab| tab.panes.iter().map(|pane| pane.id))
        .collect();
    assert!(
        !rt.terminal_snapshots.contains_key(&9_999_999),
        "stale per-pane cache survived a drawn frame"
    );
    assert!(
        rt.terminal_snapshots.keys().all(|id| live.contains(id)),
        "drawn frame holds caches for panes the model no longer owns: {:?} vs {live:?}",
        rt.terminal_snapshots.keys().collect::<Vec<_>>()
    );

    let _ = std::fs::remove_dir_all(&dir);
}
