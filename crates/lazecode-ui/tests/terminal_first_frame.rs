//! Regression: a console pane first shown while its program is repainting
//! inside a synchronized update (DEC 2026, as omp does under load) must not
//! stay blank until the program exits. A fresh pane has no previous frame to
//! hold, so the first snapshot has to be taken immediately even mid-update.

use std::time::{Duration, Instant};

use egui::{Rect, Vec2};
use lazecode_core::ThemeMode;
use lazecode_terminal::{Session, Snapshot, SpawnOptions};
use lazecode_ui::runtime::{TermResizeGate, TermSearchState, TermSelection};
use lazecode_ui::terminal::GraphicsCache;
use lazecode_ui::theme::Theme;

fn wait_until(mut predicate: impl FnMut() -> bool, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    predicate()
}

/// Full-screen repaint inside BSU/ESU, holding the update open for 400ms per
/// cycle with no gap before the next BSU — the shape omp has while streaming
/// output. rio-vt's 1s safety timeout keeps every BSU live, so the renderer
/// sees `sync_update_hold() == Some` on virtually every frame.
fn spawn_sync_painter() -> Session {
    lazecode_terminal::spawn(SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 80,
        rows: 24,
        command: Some(
            "while true; do printf '\\033[?2026h\\033[2J\\033[H'; \
             i=0; while [ $i -lt 40 ]; do printf 'row %02d\\r\\n' $i; i=$((i+1)); done; \
             sleep 0.4; printf '\\033[?2026l'; done"
                .to_string(),
        ),
        ..Default::default()
    })
    .expect("spawn")
}

#[test]
fn first_draw_while_tui_holds_sync_update_is_not_blank() {
    let session = spawn_sync_painter();
    assert!(
        wait_until(
            || session.sync_update_hold().is_some(),
            Duration::from_secs(4)
        ),
        "program never entered a synchronized update"
    );

    let ctx = egui::Context::default();
    let theme = Theme::for_mode(ThemeMode::Dark);
    let mut snapshot = Snapshot::default(); // fresh pane: no previous frame
    let mut selection: Option<TermSelection> = None;
    let mut search = TermSearchState::default();
    let mut resize = TermResizeGate::default();
    let mut preedit = String::new();
    let mut wheel = 0.0f32;
    let mut graphics = GraphicsCache::default();

    let rect = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 500.0));
    let mut drew_mid_update = false;
    let deadline = Instant::now() + Duration::from_secs(4);
    while Instant::now() < deadline {
        if session.sync_update_hold().is_none() {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
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
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            lazecode_ui::terminal::draw(
                ui,
                &session,
                &mut snapshot,
                &mut selection,
                &mut search,
                &mut resize,
                false,
                &mut preedit,
                &mut wheel,
                &mut graphics,
                true,
                &theme,
                rect,
                1,
                13.0,
            );
        });
        out.textures_delta.clear();
        if session.sync_update_hold().is_some() {
            drew_mid_update = true;
            if snapshot.rows > 0 {
                return; // first frame captured despite the open update
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    assert!(
        drew_mid_update,
        "test never drew while the program was inside a synchronized update"
    );
    panic!(
        "pane stayed blank (snapshot.rows == 0) while the program kept repainting \
         inside synchronized updates"
    );
}

fn snapshot_text(snap: &Snapshot) -> String {
    let mut text = String::new();
    for row in 0..snap.rows {
        for col in 0..snap.cols {
            let ch = snap.cell(row, col).ch;
            text.push(if ch == '\0' { ' ' } else { ch });
        }
        text.push('\n');
    }
    text
}

fn draw_once(
    ctx: &egui::Context,
    session: &Session,
    snapshot: &mut Snapshot,
    theme: &Theme,
    rect: Rect,
    refresh: bool,
) {
    let mut selection: Option<TermSelection> = None;
    let mut search = TermSearchState::default();
    let mut resize = TermResizeGate::default();
    let mut preedit = String::new();
    let mut wheel = 0.0f32;
    let mut graphics = GraphicsCache::default();
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
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        lazecode_ui::terminal::draw(
            ui,
            session,
            snapshot,
            &mut selection,
            &mut search,
            &mut resize,
            false,
            &mut preedit,
            &mut wheel,
            &mut graphics,
            refresh,
            theme,
            rect,
            1,
            13.0,
        );
    });
    out.textures_delta.clear();
}

/// A pane that already has a frame keeps it while the program is mid-update
/// (the hold that prevents a garbled one-frame flash); the next complete frame
/// lands right after ESU.
#[test]
fn existing_frame_is_held_until_the_sync_update_closes() {
    let session = lazecode_terminal::spawn(SpawnOptions {
        cwd: std::env::temp_dir(),
        cols: 80,
        rows: 24,
        command: Some(
            "n=0; while true; do n=$((n+1)); printf '\\033[?2026h\\033[2J\\033[HFRAME %03d\\r\\n' $n; \
             sleep 0.3; printf '\\033[?2026l'; sleep 0.05; done"
                .to_string(),
        ),
        ..Default::default()
    })
    .expect("spawn");

    let ctx = egui::Context::default();
    let theme = Theme::for_mode(ThemeMode::Dark);
    let rect = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 500.0));
    let mut snapshot = Snapshot::default();

    // First frame: captured immediately even mid-update (fresh pane).
    assert!(wait_until(
        || session.sync_update_hold().is_some(),
        Duration::from_secs(4)
    ));
    draw_once(&ctx, &session, &mut snapshot, &theme, rect, true);
    assert!(snapshot.rows > 0, "fresh pane must capture a frame");
    let held = snapshot_text(&snapshot);
    let held_frame = held
        .split_whitespace()
        .find(|w| w.starts_with("FRAME"))
        .and_then(|_| held.split("FRAME ").nth(1))
        .map(|rest| rest[..3].to_string())
        .expect("frame marker in snapshot");

    // Wait until the program has painted the NEXT frame inside a new update:
    // a raw snapshot (what the renderer refuses to paint mid-update) differs.
    let mut advanced = false;
    let deadline = Instant::now() + Duration::from_secs(4);
    while Instant::now() < deadline {
        if session.sync_update_hold().is_some() {
            let mut scratch = Snapshot::default();
            session.snapshot_into(&mut scratch);
            if snapshot_text(&scratch).contains("FRAME")
                && !snapshot_text(&scratch).contains(&format!("FRAME {held_frame}"))
            {
                advanced = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(advanced, "program never painted the next frame");

    // Mid-update the pane must keep the last complete frame, not the partial one.
    draw_once(&ctx, &session, &mut snapshot, &theme, rect, true);
    assert!(
        snapshot_text(&snapshot).contains(&format!("FRAME {held_frame}")),
        "pane must hold the last complete frame while the update is open"
    );

    // After ESU the pane picks up the new frame (the wake is retried).
    assert!(
        wait_until(
            || {
                draw_once(&ctx, &session, &mut snapshot, &theme, rect, true);
                session.sync_update_hold().is_none()
                    && !snapshot_text(&snapshot).contains(&format!("FRAME {held_frame}"))
            },
            Duration::from_secs(4)
        ),
        "pane never picked up the next complete frame"
    );
}
