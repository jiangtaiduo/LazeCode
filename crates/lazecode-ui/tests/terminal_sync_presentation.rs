//! Regression: omp renders each frame inside a synchronized update
//! (`CSI ? 2026 h` … `CSI ? 2026 l`). LazeCode used to snapshot and paint the
//! grid on every PTY wake, so a read that landed mid-frame presented a
//! half-composited screen — the brief white-on-black flash seen after
//! switching back to a project while its console was repainting.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{Pos2, RawInput, Rect, Vec2, ViewportId, ViewportInfo};
use lazecode_core::{AppModel, Settings, UiState};
use lazecode_terminal::{SessionKind, Snapshot};
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

fn frame(ctx: &egui::Context, model: &mut AppModel, rt: &mut UiRuntime, t: f64) {
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
        ..Default::default()
    };
    let mut output = ctx.run_ui(raw, |ui| {
        lazecode_ui::app_ui::draw(ui, model, rt);
    });
    output.textures_delta.clear();
}

fn fixture(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "lazecode-ui-sync-frame-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create fixture dir");
    dir
}

fn wait_until(mut predicate: impl FnMut() -> bool, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

fn snapshot_text(snapshot: &Snapshot) -> String {
    let mut text = String::new();
    for row in 0..snapshot.rows {
        for col in 0..snapshot.cols {
            let ch = snapshot.cell(row, col).ch;
            text.push(if ch == '\0' { ' ' } else { ch });
        }
        text.push('\n');
    }
    text
}

#[test]
fn synchronized_update_is_not_presented_mid_frame() {
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let dir = fixture("frame");
    let release = dir.join("release");
    let script = dir.join("sync-frame.sh");
    std::fs::write(
        &script,
        "printf '\\033[?2026h\\033[2J\\033[HPARTIAL'\n\
         while [ ! -f \"$1\" ]; do sleep 0.02; done\n\
         printf 'DONE\\033[?2026l'\n",
    )
    .expect("write script");

    let mut model = model(&settings);
    let project =
        lazecode_core::ProjectState::open(dir.clone(), &settings, None).expect("open project");
    model.projects.push(project);
    model.active_project = Some(0);
    model.ui.terminal_visible = true;
    let pane = model.projects[0]
        .open_terminal(None, None, SessionKind::Shell, &settings, None)
        .expect("open console");
    let mut rt = UiRuntime::default();
    rt.set_terminal_focus(pane);

    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);
    // The first frame sizes the PTY; the shell prompt may not have arrived.
    frame(&ctx, &mut model, &mut rt, 1.0);
    std::thread::sleep(Duration::from_millis(150));
    // Second frame latches the shell prompt as the last complete frame.
    frame(&ctx, &mut model, &mut rt, 1.2);
    let before = rt
        .terminal_snapshots
        .get(&pane)
        .filter(|snapshot| snapshot.rows > 0)
        .cloned()
        .expect("prompt snapshot");
    assert!(!snapshot_text(&before).contains("PARTIAL"));

    // Simulate switching away and back: the last complete frame survives the
    // project-switch cache clear so a mid-repaint grid is never shown.
    rt.clear_terminal_cache();
    rt.set_terminal_focus(pane);

    let command = format!("sh {} {}\r", script.display(), release.display());
    model.projects[0]
        .term_by_id(pane)
        .expect("console")
        .session
        .write(command.as_bytes());

    assert!(
        wait_until(
            || model.projects[0]
                .term_by_id(pane)
                .expect("console")
                .session
                .sync_update_hold()
                .is_some(),
            Duration::from_secs(4)
        ),
        "the script never entered a synchronized update"
    );

    // Mid-update the grid already contains PARTIAL, but the draw must keep the
    // last complete frame instead of presenting it.
    frame(&ctx, &mut model, &mut rt, 1.3);
    let held = rt
        .terminal_snapshots
        .get(&pane)
        .expect("held snapshot")
        .clone();
    assert!(
        held.rows > 0,
        "the last complete frame was dropped on project switch"
    );
    assert!(
        !snapshot_text(&held).contains("PARTIAL"),
        "presented a mid-synchronized-update frame"
    );

    std::fs::write(&release, b"go").expect("release script");
    assert!(
        wait_until(
            || model.projects[0]
                .term_by_id(pane)
                .expect("console")
                .session
                .sync_update_hold()
                .is_none(),
            Duration::from_secs(4)
        ),
        "ESU was never observed"
    );
    std::thread::sleep(Duration::from_millis(50));
    frame(&ctx, &mut model, &mut rt, 1.5);
    let after = rt.terminal_snapshots.get(&pane).expect("final snapshot");
    assert!(
        snapshot_text(after).contains("DONE"),
        "the completed frame was not presented"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
