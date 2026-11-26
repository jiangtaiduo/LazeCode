//! Regression: the wheel must scroll the popup that owns the pointer, not
//! the scroll surface underneath it. egui delivers the wheel to every scroll
//! area containing the pointer (first consumer wins, panels draw before
//! overlays), so without shell interception a filter menu over the commit
//! list scrolled the commit list instead of the menu.

use egui::{pos2, vec2, Event, MouseWheelSource, MouseWheelUnit, Pos2, Rect, TouchPhase, Vec2};
use lazecode_core::{AppModel, PanelTab, Settings, UiState};
use lazecode_git::{BranchInfo, CommitInfo};
use lazecode_ui::{app_ui, runtime::MenuKind, runtime::MenuState, UiRuntime};

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
        favorite_branches: Default::default(),
        git_cmd_log: Vec::new(),
    }
}

fn project_model(settings: &Settings) -> AppModel {
    let mut model = model(settings);
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project = lazecode_core::ProjectState::open(root, settings, None).expect("project");
    model.projects.push(project);
    model.active_project = Some(0);
    model.ui.panel_visible = true;
    model.ui.panel_tab = PanelTab::Git;
    // Enough rows that the branch filter menu overflows its 320px viewport
    // and actually has something to scroll.
    model.projects[0].branches = (0..24)
        .map(|i| BranchInfo {
            name: format!("feature/branch-{i:02}"),
            full_name: format!("refs/heads/feature/branch-{i:02}"),
            remote: false,
            current: i == 0,
            target: "abc123".to_string(),
            tracking: None,
            ahead: 0,
            behind: 0,
        })
        .collect();
    // CI checks out with depth 1; a real log can be a single row there. Both
    // tests need a scrollable commit list regardless of the local history.
    model.projects[0].log = (0..40)
        .map(|i| CommitInfo {
            id: format!("{i:040x}"),
            short: format!("{i:07x}"),
            message: format!("commit {i:02}"),
            body: String::new(),
            author: "Tester".to_string(),
            email: "tester@example.com".to_string(),
            time: 1_700_000_000 - i * 60,
            parents: if i + 1 < 40 {
                vec![format!("{:040x}", i + 1)]
            } else {
                vec![]
            },
            in_head: Some(true),
            refs: Vec::new(),
            lane: 0,
            lane_count: 1,
            edges: Vec::new(),
        })
        .collect();
    model
}

fn frame(
    ctx: &egui::Context,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    events: Vec<Event>,
) -> egui::Id {
    let rect = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(W, H));
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
    let raw = egui::RawInput {
        screen_rect: Some(rect),
        viewports,
        time: Some(1.0),
        events,
        ..Default::default()
    };
    let mut root_scope = egui::Id::NULL;
    let mut output = ctx.run_ui(raw, |ui| {
        root_scope = ui.scope_id();
        lazecode_ui::app_ui::draw(ui, model, rt);
    });
    output.textures_delta.clear();
    root_scope
}

/// Scroll state id of `region(ui, rect, salt, …)` + `ScrollArea::id_salt(...)`.
/// Both mix an `IdSalt` value (hash-of-hash), not the raw string.
fn region_scroll_id(root_scope: egui::Id, region: &'static str, area: &'static str) -> egui::Id {
    root_scope
        .with(egui::IdSalt::new(region))
        .with(egui::IdSalt::new(area))
}

fn wheel(delta: Vec2) -> Vec<Event> {
    // Keep |delta| under egui's 8pt smoothing threshold so the points land
    // in `smooth_wheel_delta` on the event frame instead of trickling in via
    // `after_events` (which needs advancing `time` to make progress).
    debug_assert!(delta.length() < 8.0);
    vec![Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta,
        phase: TouchPhase::Move,
        source: MouseWheelSource::Wheel,
        modifiers: egui::Modifiers::default(),
    }]
}

fn scroll_offset(ctx: &egui::Context, id: egui::Id) -> f32 {
    egui::scroll_area::State::load(ctx, id)
        .expect("scroll area state present")
        .clamped_offset()
        .y
}

/// The reported bug: wheel over the open log-branch filter menu must scroll
/// the menu, never the commit list underneath it.
#[test]
fn wheel_over_open_menu_scrolls_menu_not_commit_list() {
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let mut model = project_model(&settings);
    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);
    let mut rt = UiRuntime::default();

    // Pointer must sit inside BOTH the menu (x 496…756 for this anchor) and
    // the list, or the bug (list eats the wheel first) cannot reproduce.
    let layout = app_ui::compute_layout(
        Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(W, H)),
        &model,
    );
    let panel = layout.panel.expect("panel visible");
    let list_point = Pos2::new(626.0, panel.top() + 160.0);
    // Popover (260x332) must cover `list_point`: place its top ~230pt above.
    rt.menu = Some(MenuState {
        kind: MenuKind::LogBranchFilter { project: 0 },
        anchor: Rect::from_min_size(pos2(500.0, list_point.y - 230.0 - 26.0), vec2(90.0, 22.0)),
        payload: None,
        opened_frame: 0,
    });

    // Frame 1: menu opens.
    let root_scope = frame(&ctx, &mut model, &mut rt, vec![]);
    let menu_id = region_scroll_id(root_scope, "log-branch-menu", "log-branch-scroll");
    let list_id = region_scroll_id(root_scope, "log-list", "git-log");

    // Frame 2: pointer rests where menu and commit list overlap.
    frame(
        &ctx,
        &mut model,
        &mut rt,
        vec![Event::PointerMoved(list_point)],
    );
    let menu_before = scroll_offset(&ctx, menu_id);
    let list_before = scroll_offset(&ctx, list_id);

    // Frame 3: wheel while over the menu.
    frame(&ctx, &mut model, &mut rt, wheel(vec2(0.0, -6.0)));

    let menu_after = scroll_offset(&ctx, menu_id);
    let list_after = scroll_offset(&ctx, list_id);

    assert!(
        (menu_after - menu_before).abs() > 1.0,
        "menu must scroll with the wheel over it (before {menu_before}, after {menu_after})"
    );
    assert_eq!(
        list_before, list_after,
        "commit list under the menu must not scroll (before {list_before}, after {list_after})"
    );
}

/// No over-blocking: without any popup, the wheel still scrolls the commit
/// list at the pointer.
#[test]
fn wheel_without_menu_still_scrolls_commit_list() {
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let mut model = project_model(&settings);
    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);
    let mut rt = UiRuntime::default();

    frame(&ctx, &mut model, &mut rt, vec![]);
    let root_scope = frame(&ctx, &mut model, &mut rt, vec![]);
    let list_id = region_scroll_id(root_scope, "log-list", "git-log");

    // A point inside the log list: middle of the bottom panel, below the
    // tab strip and toolbar.
    let layout = app_ui::compute_layout(
        Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(W, H)),
        &model,
    );
    let panel = layout.panel.expect("panel visible");
    let point = Pos2::new(panel.center().x, panel.top() + 160.0);

    frame(&ctx, &mut model, &mut rt, vec![Event::PointerMoved(point)]);
    let before = scroll_offset(&ctx, list_id);

    frame(&ctx, &mut model, &mut rt, wheel(vec2(0.0, -6.0)));
    let after = scroll_offset(&ctx, list_id);

    assert!(
        (after - before).abs() > 1.0,
        "commit list must scroll with the wheel over it (before {before}, after {after})"
    );
}
