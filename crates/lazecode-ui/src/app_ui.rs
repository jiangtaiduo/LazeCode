use egui::{pos2, vec2, Rect, Ui};

use lazecode_core::{AppModel, PanelTab, SidebarView};

use crate::runtime::UiRuntime;
use crate::theme::{Theme, BODY, SMALL};

pub const RAIL_W: f32 = 58.0;
pub const TITLEBAR_H: f32 = 40.0;
pub const ACTIVITY_W: f32 = 44.0;
pub const STATUS_H: f32 = 24.0;
pub const TABBAR_H: f32 = 35.0;
pub const PANEL_HEAD_H: f32 = 33.0;
pub const SPLITTER: f32 = 4.0;

pub struct Layout {
    pub full: Rect,
    pub rail: Rect,
    pub main: Rect,
    pub titlebar: Rect,
    pub workspace: Rect,
    pub activity: Rect,
    pub sidebar: Option<Rect>,
    pub stage: Rect,
    pub editor: Rect,
    pub panel: Option<Rect>,
    pub status: Rect,
}

pub fn compute_layout(full: Rect, model: &AppModel) -> Layout {
    let rail = Rect::from_min_max(full.min, pos2(full.left() + RAIL_W, full.bottom()));
    let main = Rect::from_min_max(pos2(full.left() + RAIL_W, full.top()), full.max);
    let titlebar = Rect::from_min_max(main.min, pos2(main.right(), main.top() + TITLEBAR_H));
    let status = Rect::from_min_max(pos2(main.left(), main.bottom() - STATUS_H), main.max);
    let workspace = Rect::from_min_max(
        pos2(main.left(), titlebar.bottom()),
        pos2(main.right(), status.top()),
    );
    let activity = Rect::from_min_max(
        workspace.min,
        pos2(workspace.left() + ACTIVITY_W, workspace.bottom()),
    );
    let mut stage_left = activity.right();
    let has_project = !model.projects.is_empty();
    let sidebar = if model.ui.sidebar_visible && has_project {
        let max_width = (workspace.width() - ACTIVITY_W - 360.0).max(180.0);
        let width = model.ui.sidebar_width.clamp(180.0, max_width);
        let rect = Rect::from_min_max(
            pos2(stage_left, workspace.top()),
            pos2(stage_left + width, workspace.bottom()),
        );
        stage_left = rect.right();
        Some(rect)
    } else {
        None
    };
    let stage = Rect::from_min_max(pos2(stage_left, workspace.top()), workspace.max);
    let panel_height = if model.ui.panel_visible {
        let max_height = (stage.height() - TABBAR_H - 120.0).max(120.0);
        if model.ui.panel_maximised {
            max_height
        } else {
            model.ui.panel_height.clamp(120.0, max_height)
        }
    } else {
        0.0
    };
    let panel = if model.ui.panel_visible && has_project {
        Some(Rect::from_min_max(
            pos2(stage.left(), stage.bottom() - panel_height),
            stage.max,
        ))
    } else {
        None
    };
    let editor = if let Some(panel) = panel {
        Rect::from_min_max(stage.min, pos2(stage.right(), panel.top() - SPLITTER))
    } else {
        stage
    };
    Layout {
        full,
        rail,
        main,
        titlebar,
        workspace,
        activity,
        sidebar,
        stage,
        editor,
        panel,
        status,
    }
}

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime) {
    rt.frame_count = rt.frame_count.wrapping_add(1);
    let theme = Theme::for_mode(model.settings.theme);
    if rt.applied_theme != Some(model.settings.theme) {
        theme.apply(ui.ctx());
        rt.applied_theme = Some(model.settings.theme);
    }
    let layout = compute_layout(ui.max_rect(), model);

    // Focus follows the pointer: clicking outside the editor or terminal
    // releases that surface's keyboard focus before the click is handled.
    if ui.ctx().input(|input| input.pointer.any_pressed()) {
        if let Some(pos) = ui.ctx().pointer_interact_pos() {
            if !layout.editor.contains(pos) {
                rt.editor_focus = None;
            }
            if !layout.panel.is_some_and(|panel| panel.contains(pos)) {
                rt.terminal_focus = None;
            }
        }
    }

    let p = theme.palette;
    let painter = ui.painter();
    painter.rect_filled(layout.full, 0.0, p.bg);
    painter.rect_filled(layout.rail, 0.0, p.rail);
    painter.rect_filled(layout.titlebar, 0.0, p.titlebar);
    painter.rect_filled(layout.activity, 0.0, p.panel);
    if let Some(sidebar) = layout.sidebar {
        painter.rect_filled(sidebar, 0.0, p.sidebar);
        painter.vline(
            sidebar.right() - 0.5,
            sidebar.y_range(),
            egui::Stroke::new(1.0, p.border),
        );
    }
    painter.rect_filled(layout.stage, 0.0, p.editor);
    if let Some(panel) = layout.panel {
        painter.rect_filled(panel, 0.0, p.panel);
        painter.hline(
            panel.x_range(),
            panel.top() + 0.5,
            egui::Stroke::new(1.0, p.border),
        );
    }
    painter.rect_filled(layout.status, 0.0, p.statusbar);
    painter.hline(
        layout.status.x_range(),
        layout.status.top() + 0.5,
        egui::Stroke::new(1.0, p.border),
    );
    painter.vline(
        layout.titlebar.right() - 0.5,
        layout.titlebar.y_range(),
        egui::Stroke::new(1.0, p.border_soft),
    );

    crate::rail::draw(ui, model, rt, &theme, layout.rail);
    crate::titlebar::draw(ui, model, rt, &theme, layout.titlebar);
    crate::activity::draw(ui, model, rt, &theme, layout.activity);
    if let Some(sidebar) = layout.sidebar {
        crate::sidebar::draw(ui, model, rt, &theme, sidebar);
        splitter_vertical(ui, model, rt, sidebar.right());
    }
    crate::editor::draw(ui, model, rt, &theme, layout.editor);
    if let Some(panel) = layout.panel {
        splitter_horizontal(ui, model, rt, panel.top() - SPLITTER * 0.5);
        crate::panel::draw(ui, model, rt, &theme, panel);
    }
    crate::statusbar::draw(ui, model, rt, &theme, layout.status);

    if rt.settings_open {
        crate::settings::draw(ui, model, rt, &theme);
    }
    if rt.search.open {
        crate::search::draw(ui, model, rt, &theme);
    }
    if model.ui.command_palette_open {
        crate::palette::draw(ui, model, rt, &theme);
    }
    crate::overlays::draw(ui, model, rt, &theme);
    crate::toast::draw(ui, model, rt, &theme, layout.status);
    crate::shortcuts::handle(ui, model, rt);
    rt.last_frame = std::time::Instant::now();
}

fn splitter_vertical(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, x: f32) {
    let rect = Rect::from_min_max(
        pos2(x - 2.0, ui.max_rect().top()),
        pos2(x + 2.0, ui.max_rect().bottom()),
    );
    let response = ui.interact(
        rect,
        ui.scope_id().with("sidebar-splitter"),
        egui::Sense::drag(),
    );
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    if response.dragged() {
        model.ui.sidebar_width =
            (model.ui.sidebar_width + response.drag_delta().x).clamp(180.0, 520.0);
        rt.sidebar_drag = true;
    } else if rt.sidebar_drag {
        rt.sidebar_drag = false;
        model.settings.sidebar_width = model.ui.sidebar_width;
        model.save_settings();
    }
}

fn splitter_horizontal(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, y: f32) {
    let rect = Rect::from_min_max(
        pos2(ui.max_rect().left(), y),
        pos2(ui.max_rect().right(), y + SPLITTER),
    );
    let response = ui.interact(
        rect,
        ui.scope_id().with("panel-splitter"),
        egui::Sense::drag(),
    );
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if response.dragged() {
        model.ui.panel_height =
            (model.ui.panel_height - response.drag_delta().y).clamp(120.0, 900.0);
        model.ui.panel_maximised = false;
        rt.panel_drag = true;
    } else if rt.panel_drag {
        rt.panel_drag = false;
        model.settings.panel_height = model.ui.panel_height;
        model.save_settings();
    }
}

/// Scope helper: create a clipped child UI covering exactly `rect`.
pub fn region<R>(ui: &mut Ui, rect: Rect, id: &'static str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .id_salt(id)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    add(&mut child)
}

/// `true` when the active project changed this frame.
pub fn panel_tab(model: &AppModel) -> PanelTab {
    model.ui.panel_tab
}

pub fn sidebar_view(model: &AppModel) -> SidebarView {
    model.ui.sidebar_view
}

pub fn placeholder_size() -> egui::Vec2 {
    vec2(0.0, 0.0)
}

pub const _TEXT: f32 = BODY;
pub const _SMALL: f32 = SMALL;
