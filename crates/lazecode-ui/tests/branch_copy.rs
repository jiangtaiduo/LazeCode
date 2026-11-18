//! Copy shortcut on the Git log branch tree: Cmd+C / Ctrl+C copies the
//! selected branch names in full (including `a/b` path segments), and a
//! focused text surface keeps its own copy behavior.

use std::collections::HashSet;
use std::path::PathBuf;

use egui::{Event, Vec2};
use lazecode_core::{AppModel, PanelTab, Settings, UiState};
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
) -> Vec<String> {
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(W, H));
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
    let mut output = ctx.run_ui(raw, |ui| {
        lazecode_ui::app_ui::draw(ui, model, rt);
    });
    output.textures_delta.clear();
    output
        .platform_output
        .commands
        .into_iter()
        .filter_map(|command| match command {
            egui::OutputCommand::CopyText(text) => Some(text),
            _ => None,
        })
        .collect()
}

fn project_model(settings: &Settings) -> AppModel {
    let mut model = model(settings);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project = lazecode_core::ProjectState::open(root, settings, None).expect("project");
    model.projects.push(project);
    model.active_project = Some(0);
    model.ui.panel_visible = true;
    model.ui.panel_tab = PanelTab::Git;
    model
}

#[test]
fn copy_shortcut_copies_selected_branch_names_in_full() {
    let settings = Settings {
        lsp_enabled: false,
        ..Settings::default()
    };
    let mut model = project_model(&settings);
    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);
    let mut rt = UiRuntime::default();

    // 单分支：保留 `origin/feature/a/b` 的完整路径。
    model.projects[0].ref_selection = vec!["origin/feature/a/b".to_string()];
    let copied = frame(&ctx, &mut model, &mut rt, vec![Event::Copy]);
    assert_eq!(copied, vec!["origin/feature/a/b".to_string()]);

    // 多选：按行拼接。
    model.projects[0].ref_selection =
        vec!["feature/a/b".to_string(), "origin/main".to_string()];
    let copied = frame(&ctx, &mut model, &mut rt, vec![Event::Copy]);
    assert_eq!(copied, vec!["feature/a/b\norigin/main".to_string()]);

    // 文本面（编辑器）持有焦点时让位，不抢复制。
    rt.editor_focus = Some(1);
    let copied = frame(&ctx, &mut model, &mut rt, vec![Event::Copy]);
    assert!(copied.is_empty(), "editor focus must keep its own copy: {copied:?}");
}
