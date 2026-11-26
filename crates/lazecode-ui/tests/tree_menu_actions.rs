//! Regression: the project-root row must not offer destructively wrong
//! Rename/Delete actions, and tree Expand All / Reload must keep the filtered
//! tree fully populated.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use egui::{Event, Modifiers, Pos2, RawInput, Rect, Vec2, ViewportId, ViewportInfo};
use lazecode_core::{AppModel, Settings, UiState};
use lazecode_ui::runtime::{DialogState, MenuKind, MenuState};
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
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(rect),
            viewports,
            time: Some(t),
            events,
            ..Default::default()
        },
        |ui| lazecode_ui::app_ui::draw(ui, model, rt),
    );
    output.textures_delta.clear();
}

fn click(y: f32) -> Vec<Event> {
    let pos = Pos2::new(150.0, y);
    vec![
        Event::PointerMoved(pos),
        Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
        Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        },
    ]
}

fn open_tree_menu(rt: &mut UiRuntime, rel: &str) {
    rt.menu = Some(MenuState {
        kind: MenuKind::TreeContext {
            project: 0,
            is_dir: true,
        },
        anchor: Rect::from_min_size(Pos2::new(100.0, 100.0), Vec2::splat(4.0)),
        payload: Some(rel.to_string()),
        opened_frame: 0,
    });
}

fn fixture(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lazecode-ui-tree-menu-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src/sub")).expect("create fixture dirs");
    std::fs::write(dir.join("src/top.rs"), "fn top() {}\n").expect("write top.rs");
    std::fs::write(dir.join("src/sub/deep.rs"), "fn deep() {}\n").expect("write deep.rs");
    std::fs::write(dir.join("ignored.log"), "noise\n").expect("write ignored.log");
    dir
}

fn node<'a>(
    nodes: &'a [lazecode_fs::tree::TreeNode],
    rel: &str,
) -> Option<&'a lazecode_fs::tree::TreeNode> {
    for entry in nodes {
        if entry.path == Path::new(rel) {
            return Some(entry);
        }
        if let Some(found) = node(&entry.children, rel) {
            return Some(found);
        }
    }
    None
}

#[test]
fn project_root_menu_cannot_rename_or_delete_the_checkout() {
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let mut model = model(&settings);
    let dir = fixture("root-actions");
    let project =
        lazecode_core::ProjectState::open(dir.clone(), &settings, None).expect("open project");
    model.projects.push(project);
    model.active_project = Some(0);
    let mut rt = UiRuntime::default();
    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);
    frame(&ctx, &mut model, &mut rt, Vec::new(), 0.9);

    // Menu rows (top = 100, 24px each): Rename at y≈245, Delete at y≈341.
    open_tree_menu(&mut rt, "");
    frame(&ctx, &mut model, &mut rt, click(245.0), 1.0);
    assert!(
        matches!(rt.dialog, DialogState::None),
        "the project root row must not open a rename dialog"
    );

    open_tree_menu(&mut rt, "");
    frame(&ctx, &mut model, &mut rt, click(341.0), 1.1);
    assert!(
        matches!(rt.dialog, DialogState::None),
        "the project root row must not offer trash/delete"
    );

    // The very same row on a normal folder still renames, so the guard is
    // specific to the root and does not silently disable the whole menu.
    open_tree_menu(&mut rt, "src");
    frame(&ctx, &mut model, &mut rt, click(245.0), 1.2);
    assert!(
        matches!(rt.dialog, DialogState::RenamePath { .. }),
        "a regular folder must remain renameable"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn expand_all_loads_deferred_children_and_reload_keeps_custom_ignore() {
    let settings = Settings {
        lsp_enabled: false,
        ignored_patterns: vec!["*.log".to_string()],
        ..Settings::default()
    };
    let mut model = model(&settings);
    let dir = fixture("expand-reload");
    let project =
        lazecode_core::ProjectState::open(dir.clone(), &settings, None).expect("open project");
    model.projects.push(project);
    model.active_project = Some(0);
    let mut rt = UiRuntime::default();
    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);
    frame(&ctx, &mut model, &mut rt, Vec::new(), 0.9);

    // Simulate the bounded scan: the nested folder is known but unloaded.
    model.projects[0].expanded.clear();
    if let Some(folder) = lazecode_fs::tree::find_mut(
        &mut model.projects[0].active_worktree_mut().tree,
        Path::new("src/sub"),
    ) {
        folder.children_loaded = false;
        folder.children.clear();
    }

    // Expand All row at y≈367; hover it once before clicking.
    open_tree_menu(&mut rt, "");
    frame(
        &ctx,
        &mut model,
        &mut rt,
        vec![Event::PointerMoved(Pos2::new(150.0, 367.0))],
        1.0,
    );
    frame(&ctx, &mut model, &mut rt, click(367.0), 1.1);
    let tree = &model.projects[0].active_tree();
    let sub = node(&tree.nodes, "src/sub").expect("deferred folder is still in the tree");
    assert!(
        sub.children_loaded,
        "Expand All must load children deferred by the scan budget"
    );
    assert!(
        node(&tree.nodes, "ignored.log").is_none()
            && !tree.nodes.iter().any(|entry| entry.name == "ignored.log"),
        "the fixture tree already honours the custom ignore"
    );

    std::fs::write(dir.join("src/sub/new.rs"), "fn fresh() {}\n").expect("write new.rs");
    // Reload row at y≈421.
    open_tree_menu(&mut rt, "");
    frame(&ctx, &mut model, &mut rt, click(421.0), 1.1);
    let tree = &model.projects[0].active_tree();
    let sub = node(&tree.nodes, "src/sub").expect("expanded folder survives reload");
    assert!(
        node(&sub.children, "src/sub/new.rs").is_some(),
        "Reload must pick up newly written files"
    );
    assert!(
        node(&tree.nodes, "ignored.log").is_none()
            && !tree.nodes.iter().any(|entry| entry.name == "ignored.log"),
        "Reload must honour the project's custom ignore patterns"
    );
    assert!(
        model.projects[0].expanded.contains(Path::new("src/sub")),
        "Reload must keep expanded folders"
    );
    assert!(
        node(&tree.nodes, "src/sub").is_some_and(|entry| entry.children_loaded),
        "Reload must keep expanded folders populated"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
