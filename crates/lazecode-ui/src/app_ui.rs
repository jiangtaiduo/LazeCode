use egui::{pos2, vec2, Rect, Stroke, Ui};

use lazecode_core::{AppModel, PanelTab, SidebarView};

use crate::runtime::UiRuntime;
use crate::theme::Theme;

/// Stable window skeleton: a full-width 40px titlebar, a 50px project rail,
/// one exclusive full-height drawer, and the center workspace grid
/// (main / bottom panel / status).
pub const STRIP_W: f32 = 50.0;
pub const TITLEBAR_H: f32 = 40.0;
pub const STATUS_H: f32 = 26.0;
pub const TABBAR_H: f32 = 30.0;
pub const PANEL_HEAD_H: f32 = 34.0;
pub const SPLITTER: f32 = 4.0;

pub struct Layout {
    pub full: Rect,
    pub titlebar: Rect,
    pub workspace: Rect,
    pub strip: Rect,
    pub sidebar: Option<Rect>,
    pub stage: Rect,
    pub editor: Rect,
    pub terminal_dock: Option<Rect>,
    pub panel: Option<Rect>,
    pub status: Rect,
}

/// Drawer width for the active view: the Commit drawer persists its own
/// width (stable `ide_commit_drawer_width`), the file tree keeps the
/// classic sidebar width.
fn drawer_width(model: &AppModel) -> f32 {
    match model.ui.sidebar_view {
        SidebarView::Git => model.ui.commit_width,
        SidebarView::Compare => model.ui.compare_width,
        SidebarView::Files | SidebarView::Consoles => model.ui.sidebar_width,
    }
}

pub fn compute_layout(full: Rect, model: &AppModel) -> Layout {
    let titlebar = Rect::from_min_max(full.min, pos2(full.right(), full.top() + TITLEBAR_H));
    // The status bar spans the full window width; the project rail ends
    // above it so the bottom-left status text aligns with the window edge.
    let status = Rect::from_min_max(pos2(full.left(), full.bottom() - STATUS_H), full.max);
    let workspace = Rect::from_min_max(
        pos2(full.left(), titlebar.bottom()),
        pos2(full.right(), status.top()),
    );
    let strip = Rect::from_min_max(
        pos2(full.left(), titlebar.bottom()),
        pos2(full.left() + STRIP_W, status.top()),
    );
    let mut stage_left = strip.right();
    let has_project = !model.projects.is_empty();
    let active_single = model
        .active_project()
        .is_some_and(|project| project.is_single_file);
    // Single files has no project tree or Git drawer. The console drawer is
    // still allowed because ad-hoc files often need a shell.
    let sidebar_allowed =
        !active_single || model.ui.sidebar_view == lazecode_core::SidebarView::Consoles;
    let sidebar = if model.ui.sidebar_visible && has_project && sidebar_allowed {
        let max_width = (workspace.width() - STRIP_W - 360.0).max(180.0);
        let width = drawer_width(model).clamp(180.0, max_width);
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
    let is_right = model.ui.terminal_placement == lazecode_core::TerminalPlacement::Right;
    let (show_panel, show_terminal_bottom) = if is_right {
        (model.ui.panel_visible, false)
    } else if model.ui.terminal_visible {
        (false, true)
    } else {
        (model.ui.panel_visible, false)
    };

    let panel_height = if show_panel {
        if model.ui.panel_maximised {
            stage.height()
        } else {
            let max_height = (stage.height() - TABBAR_H - 120.0).max(120.0);
            model.ui.panel_height.clamp(120.0, max_height)
        }
    } else {
        0.0
    };
    let panel = if show_panel && has_project && !active_single {
        Some(Rect::from_min_max(
            pos2(stage.left(), status.top() - panel_height),
            pos2(stage.right(), status.top()),
        ))
    } else {
        None
    };

    let terminal_dock = if has_project {
        if is_right && model.ui.terminal_visible {
            if model.ui.terminal_maximised {
                Some(Rect::from_min_max(
                    stage.min,
                    pos2(stage.right(), status.top()),
                ))
            } else {
                let max_width = (stage.width() - 320.0).max(240.0);
                let dock_width = model.ui.terminal_width.clamp(240.0, max_width);
                let upper_limit = panel
                    .as_ref()
                    .map(|p| p.top() - SPLITTER)
                    .unwrap_or_else(|| status.top());
                Some(Rect::from_min_max(
                    pos2(stage.right() - dock_width, stage.top()),
                    pos2(stage.right(), upper_limit),
                ))
            }
        } else if show_terminal_bottom {
            let dock_height = if model.ui.terminal_maximised {
                stage.height()
            } else {
                let max_height = (stage.height() - TABBAR_H - 120.0).max(120.0);
                model.ui.terminal_height.clamp(120.0, max_height)
            };
            Some(Rect::from_min_max(
                pos2(stage.left(), status.top() - dock_height),
                pos2(stage.right(), status.top()),
            ))
        } else {
            None
        }
    } else {
        None
    };
    let editor = if (show_panel && model.ui.panel_maximised)
        || (model.ui.terminal_visible && model.ui.terminal_maximised)
    {
        Rect::from_min_max(stage.min, stage.min)
    } else if is_right {
        let right_bound = terminal_dock
            .as_ref()
            .map(|dock| dock.left() - SPLITTER)
            .unwrap_or_else(|| stage.right());
        if let Some(panel) = &panel {
            Rect::from_min_max(stage.min, pos2(right_bound, panel.top() - SPLITTER))
        } else {
            Rect::from_min_max(stage.min, pos2(right_bound, status.top()))
        }
    } else if let Some(dock) = &terminal_dock {
        Rect::from_min_max(stage.min, pos2(stage.right(), dock.top() - SPLITTER))
    } else if let Some(panel) = &panel {
        Rect::from_min_max(stage.min, pos2(stage.right(), panel.top() - SPLITTER))
    } else {
        Rect::from_min_max(stage.min, pos2(stage.right(), status.top()))
    };
    Layout {
        full,
        titlebar,
        workspace,
        strip,
        sidebar,
        stage,
        editor,
        terminal_dock,
        panel,
        status,
    }
}

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime) {
    rt.frame_count = rt.frame_count.wrapping_add(1);
    rt.filter_ime_enter(ui);
    // A focused console/editor owns the keyboard, but egui still runs Tab
    // focus navigation over clickable widgets and treats Enter/Space on the
    // focused widget as a click (FAKE_PRIMARY_CLICKED). Tab completion or
    // indentation followed by Enter therefore activated whichever widget got
    // focus — a project-rail tile switched projects mid-command. Keep egui
    // focus off while a text surface owns input; it receives Tab/Enter itself.
    if rt.typing_surface_owns_keys(model, ui.ctx()) {
        ui.ctx().memory_mut(|memory| {
            memory.move_focus(egui::FocusDirection::None);
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
    }
    egui_extras::install_image_loaders(ui.ctx());
    model.sync_worktree_view();
    // The core can drop panes without going through the UI close paths
    // (`settle_single_files_workspace`), so reconcile per-pane caches here.
    rt.prune_terminal_caches(model);
    model.sync_lsp();
    crate::lsp_ui::poll(ui.ctx(), model, rt);
    let mode = model.settings.theme;
    let skin = model.settings.skin();
    let theme = Theme::new(mode, skin);
    if rt.applied_theme != Some((mode, skin)) {
        theme.apply(ui.ctx());
        rt.applied_theme = Some((mode, skin));
    }
    let layout = compute_layout(ui.max_rect(), model);

    // 「Commit & Push」提交成功后的一-shot：打开 Push 对话框（对齐稳定
    // 版 Commit and Push...）。标志由 JobResult 消费点置位。
    if model.ui.open_push_dialog {
        model.ui.open_push_dialog = false;
        if let Some(project) = model.active_project() {
            let branch = project.branch();
            let remote = project
                .remotes
                .first()
                .map(|info| info.name.clone())
                .unwrap_or_else(|| "origin".to_string());
            rt.dialog = crate::overlays::push_dialog_state(project, remote, branch);
        }
    }

    // Focus follows the pointer: clicking outside the editor or terminal
    // releases that surface's keyboard focus before the click is handled.
    if ui.ctx().input(|input| input.pointer.any_pressed()) {
        if let Some(pos) = ui.ctx().pointer_interact_pos() {
            if !layout.editor.contains(pos) {
                rt.editor_focus = None;
                if let Some(state) = rt.lsp.completion.take() {
                    if let Some(id) = state.request {
                        model.lsp.cancel(id);
                    }
                }
                if let Some(state) = rt.lsp.symbols.take() {
                    if let Some(id) = state.request {
                        model.lsp.cancel(id);
                    }
                }
                if let Some(state) = rt.lsp.hover.take() {
                    if let Some(id) = state.request {
                        model.lsp.cancel(id);
                    }
                }
            }
            if !layout.panel.is_some_and(|panel| panel.contains(pos)) {
                rt.terminal_focus = None;
            }
        }
    }

    let p = theme.palette;
    let painter = ui.painter().clone();
    // Flat opaque chrome: theme changes the neutral family, never a
    // full-window gradient.
    painter.rect_filled(layout.full, 0.0, p.bg);
    painter.rect_filled(layout.titlebar, 0.0, p.titlebar);
    painter.rect_filled(layout.strip, 0.0, p.rail);
    // The activity rail's edge must not read as a window divider; the
    // sidebar/stage divider below stays at `border`.
    painter.vline(
        layout.strip.right() - 0.5,
        layout.strip.y_range(),
        Stroke::new(1.0, p.hairline),
    );
    if let Some(sidebar) = layout.sidebar {
        painter.rect_filled(sidebar, 0.0, p.sidebar);
        painter.vline(
            sidebar.right() - 0.5,
            sidebar.y_range(),
            Stroke::new(1.0, p.border),
        );
    }
    painter.rect_filled(layout.stage, 0.0, p.editor);
    painter.rect_filled(layout.status, 0.0, p.statusbar);
    painter.hline(
        layout.status.x_range(),
        layout.status.top() + 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    if let Some(panel) = layout.panel {
        painter.rect_filled(panel, 0.0, p.panel);
        painter.hline(
            panel.x_range(),
            panel.top() + 0.5,
            Stroke::new(1.0, p.border),
        );
    }

    // egui hands the wheel to every scroll surface containing the pointer
    // (first consumer wins; no z-order test), and panels draw before
    // overlays. While an overlay owns the pointer — modals anywhere, popover
    // menus inside their rect — stash the delta away from the panels here and
    // hand it back just before the overlay pass below.
    let menu_rects = rt.take_menu_hit_rects();
    let overlay_wheel = ui.input_mut(|input| {
        if rt.blocks_wheel(model, input.pointer.interact_pos(), &menu_rects) {
            core::mem::take(&mut input.smooth_scroll_delta)
        } else {
            egui::Vec2::ZERO
        }
    });
    crate::strip::draw(ui, model, rt, &theme, layout.strip);
    crate::titlebar::draw(ui, model, rt, &theme, layout.titlebar);
    if let Some(sidebar) = layout.sidebar {
        crate::sidebar::draw(ui, model, rt, &theme, sidebar);
        splitter_vertical(ui, model, rt, &theme, sidebar.y_range(), sidebar.right());
    }
    if layout.editor.height() >= 35.0 && layout.editor.width() >= 10.0 {
        crate::editor::draw(ui, model, rt, &theme, layout.editor);
    }
    if let Some(dock) = layout.terminal_dock {
        painter.rect_filled(dock, 0.0, p.panel);
        let is_right = model.ui.terminal_placement == lazecode_core::TerminalPlacement::Right;
        if is_right {
            painter.vline(
                dock.left() - 0.5,
                dock.y_range(),
                Stroke::new(1.0, p.border),
            );
            if !model.ui.terminal_maximised {
                splitter_terminal_vertical(
                    ui,
                    model,
                    rt,
                    &theme,
                    dock.y_range(),
                    dock.left() - SPLITTER * 0.5,
                );
            }
        } else {
            painter.hline(dock.x_range(), dock.top() + 0.5, Stroke::new(1.0, p.border));
            if !model.ui.terminal_maximised {
                splitter_terminal(
                    ui,
                    model,
                    rt,
                    &theme,
                    dock.x_range(),
                    dock.top() - SPLITTER * 0.5,
                );
            }
        }
        crate::panel::draw_terminal_dock(ui, model, rt, &theme, dock);
    }
    if let Some(panel) = layout.panel {
        if !model.ui.panel_maximised {
            splitter_horizontal(
                ui,
                model,
                rt,
                &theme,
                panel.x_range(),
                panel.top() - SPLITTER * 0.5,
            );
        }
        crate::panel::draw(ui, model, rt, &theme, panel);
    }
    crate::statusbar::draw(ui, model, rt, &theme, layout.status);
    // Overlay phase: settings, dialogs and popover menus scroll with the
    // wheel the panels were denied above.
    if overlay_wheel != egui::Vec2::ZERO {
        ui.input_mut(|input| input.smooth_scroll_delta = overlay_wheel);
    }
    crate::lsp_ui::draw_popups(ui, model, rt, &theme);

    // Full-screen hit-test blocker: above the shell chrome, below every
    // overlay drawn after it.
    if rt.blocks_input(model) {
        ui.interact(
            layout.full,
            ui.scope_id().with("lazecode-modal-input-blocker"),
            egui::Sense::click_and_drag(),
        );
    }

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
    crate::toast::draw(
        ui,
        model,
        rt,
        &theme,
        layout.full,
        layout.titlebar.bottom() + 10.0,
    );
    crate::shortcuts::handle(ui, model, rt);
    rt.prune_media_caches(ui.ctx(), model);
    rt.last_frame = std::time::Instant::now();
}

fn splitter_vertical(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    y_range: egui::Rangef,
    x: f32,
) {
    // Stable `.drawer-resizer-v`: 12px hit area with a 2px hairline that
    // turns accent on hover.
    let rect = Rect::from_min_max(pos2(x - 6.0, y_range.min), pos2(x + 6.0, y_range.max));
    let response = ui.interact(
        rect,
        ui.scope_id().with("sidebar-splitter"),
        egui::Sense::drag(),
    );
    let active = response.hovered() || response.dragged();
    if active {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        ui.painter().rect_filled(
            Rect::from_min_max(pos2(x - 1.0, y_range.min), pos2(x + 1.0, y_range.max)),
            egui::CornerRadius::same(1),
            theme.palette.accent,
        );
    }
    if response.dragged() {
        let width = drawer_width(model) + response.drag_delta().x;
        match model.ui.sidebar_view {
            SidebarView::Git => model.ui.commit_width = width.clamp(240.0, 640.0),
            SidebarView::Compare => model.ui.compare_width = width.clamp(240.0, 640.0),
            SidebarView::Files | SidebarView::Consoles => {
                model.ui.sidebar_width = width.clamp(180.0, 520.0)
            }
        }
        rt.sidebar_drag = true;
    } else if rt.sidebar_drag {
        rt.sidebar_drag = false;
        model.settings.sidebar_width = model.ui.sidebar_width;
        model.settings.commit_drawer_width = model.ui.commit_width;
        model.settings.compare_drawer_width = model.ui.compare_width;
        model.save_settings();
    }
}

fn splitter_terminal(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    x_range: egui::Rangef,
    y: f32,
) {
    let rect = Rect::from_min_max(
        pos2(x_range.min, y - 2.0),
        pos2(x_range.max, y + SPLITTER + 2.0),
    );
    let response = ui.interact(
        rect,
        ui.scope_id().with("terminal-splitter"),
        egui::Sense::drag(),
    );
    let active = response.hovered() || response.dragged();
    if active {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        ui.painter().rect_filled(
            Rect::from_min_max(pos2(x_range.min, y + 1.0), pos2(x_range.max, y + 3.0)),
            egui::CornerRadius::same(1),
            theme.palette.accent,
        );
    }
    if response.dragged() {
        model.ui.terminal_height =
            (model.ui.terminal_height - response.drag_delta().y).clamp(120.0, 900.0);
        rt.panel_drag = true;
    } else if rt.panel_drag {
        // 与面板共用拖拽标志；面板自身不在拖拽时才落盘终端高度。
        rt.panel_drag = false;
        model.save_settings();
    }
}

fn splitter_terminal_vertical(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    y_range: egui::Rangef,
    x: f32,
) {
    let rect = Rect::from_min_max(
        pos2(x - 2.0, y_range.min),
        pos2(x + SPLITTER + 2.0, y_range.max),
    );
    let response = ui.interact(
        rect,
        ui.scope_id().with("terminal-splitter-vert"),
        egui::Sense::drag(),
    );
    let active = response.hovered() || response.dragged();
    if active {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        ui.painter().rect_filled(
            Rect::from_min_max(pos2(x + 1.0, y_range.min), pos2(x + 3.0, y_range.max)),
            egui::CornerRadius::same(1),
            theme.palette.accent,
        );
    }
    if response.dragged() {
        model.ui.terminal_width =
            (model.ui.terminal_width - response.drag_delta().x).clamp(240.0, 1200.0);
        rt.panel_drag = true;
    } else if rt.panel_drag {
        rt.panel_drag = false;
        model.save_workspace();
    }
}

fn splitter_horizontal(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    x_range: egui::Rangef,
    y: f32,
) {
    let rect = Rect::from_min_max(
        pos2(x_range.min, y - 2.0),
        pos2(x_range.max, y + SPLITTER + 2.0),
    );
    let response = ui.interact(
        rect,
        ui.scope_id().with("panel-splitter"),
        egui::Sense::drag(),
    );
    let active = response.hovered() || response.dragged();
    if active {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        ui.painter().rect_filled(
            Rect::from_min_max(pos2(x_range.min, y + 1.0), pos2(x_range.max, y + 3.0)),
            egui::CornerRadius::same(1),
            theme.palette.accent,
        );
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
