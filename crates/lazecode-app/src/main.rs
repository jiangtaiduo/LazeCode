mod clipboard;
mod jobs;

use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use crossbeam_channel::{Receiver, Sender};
use egui_wgpu::winit::Painter;
use egui_wgpu::{RendererOptions, WgpuConfiguration};
use lazecode_core::{AppModel, Settings, TabKind};
use lazecode_terminal::{probe_busy, BusyTarget, Notifier};
use lazecode_ui::UiRuntime;
use parking_lot::Mutex;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::platform::macos::{OptionAsAlt, WindowAttributesExtMacOS};
use winit::window::{Window, WindowId};

#[cfg(target_os = "macos")]
use muda::accelerator::{Accelerator, Code, Modifiers as MenuModifiers, CMD_OR_CTRL};
#[cfg(target_os = "macos")]
use muda::{Menu, MenuItem, PredefinedMenuItem, Submenu};

use jobs::JobQueue;

#[derive(Clone, Copy, Debug)]
pub enum AppEvent {
    Wake,
}

struct ProxyNotifier(Mutex<EventLoopProxy<AppEvent>>);

impl Notifier for ProxyNotifier {
    fn notify(&self) {
        let _ = self.0.lock().send_event(AppEvent::Wake);
    }
}

struct TerminalBusyRow {
    session_id: u64,
    busy: bool,
    outputting: bool,
    commands: Vec<String>,
    last_output_ms: Option<u64>,
}

struct Gfx {
    window: Arc<Window>,
    painter: Painter,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
}

struct LazeApp {
    model: AppModel,
    runtime: UiRuntime,
    gfx: Option<Gfx>,
    jobs: JobQueue,
    last_watch_poll: std::time::Instant,
    last_busy_poll: std::time::Instant,
    busy_tx: Sender<Vec<TerminalBusyRow>>,
    busy_rx: Receiver<Vec<TerminalBusyRow>>,
    busy_in_flight: bool,
    started: std::time::Instant,
    modifiers: ModifiersState,
    last_workspace_save: std::time::Instant,
    last_autosave: std::time::Instant,
    repaint_after: std::time::Duration,
    last_shift_press: Option<std::time::Instant>,
    #[cfg(target_os = "macos")]
    menu: Option<Menu>,
    #[cfg(target_os = "macos")]
    menu_locale_zh: bool,
}

impl LazeApp {
    fn new(model: AppModel, jobs: JobQueue) -> Self {
        let (busy_tx, busy_rx) = crossbeam_channel::unbounded();
        Self {
            model,
            runtime: UiRuntime::default(),
            gfx: None,
            jobs,
            last_watch_poll: std::time::Instant::now(),
            last_busy_poll: std::time::Instant::now()
                .checked_sub(Duration::from_secs(3))
                .unwrap_or_else(std::time::Instant::now),
            busy_tx,
            busy_rx,
            busy_in_flight: false,
            started: std::time::Instant::now(),
            modifiers: ModifiersState::empty(),
            last_workspace_save: std::time::Instant::now(),
            last_autosave: std::time::Instant::now(),
            repaint_after: std::time::Duration::MAX,
            last_shift_press: None,
            #[cfg(target_os = "macos")]
            menu: None,
            #[cfg(target_os = "macos")]
            menu_locale_zh: false,
        }
    }

    fn init(&mut self, event_loop: &ActiveEventLoop) -> anyhow::Result<()> {
        if self.gfx.is_some() {
            return Ok(());
        }
        let attributes = Window::default_attributes()
            .with_title("LazeCode")
            .with_inner_size(winit::dpi::LogicalSize::new(1440.0, 920.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(980.0, 620.0))
            .with_titlebar_transparent(true)
            .with_fullsize_content_view(true)
            .with_title_hidden(true)
            .with_option_as_alt(OptionAsAlt::OnlyLeft);
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .context("create window")?,
        );
        window.set_visible(true);

        let egui_ctx = egui::Context::default();
        lazecode_ui::install_fonts(&egui_ctx, &self.model.settings);
        egui_ctx.style_mut_of(egui::Theme::Dark, |style| {
            style.animation_time = 0.08;
        });

        let mut painter = pollster::block_on(Painter::new(
            egui_ctx.clone(),
            WgpuConfiguration::default(),
            false,
            RendererOptions::default(),
        ));
        pollster::block_on(painter.set_window(egui::ViewportId::ROOT, Some(window.clone())))
            .context("initialise wgpu surface")?;
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );

        self.gfx = Some(Gfx {
            window,
            painter,
            egui_ctx,
            egui_state,
        });
        #[cfg(target_os = "macos")]
        {
            self.install_menu();
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn install_menu(&mut self) {
        let zh = self.model.settings.language.is_zh();
        if let Some(menu) = build_menu(zh) {
            menu.init_for_nsapp();
            self.menu = Some(menu);
            self.menu_locale_zh = zh;
        }
    }

    #[cfg(target_os = "macos")]
    fn poll_menu(&mut self, event_loop: &ActiveEventLoop) {
        if self.menu_locale_zh != self.model.settings.language.is_zh() {
            self.install_menu();
        }
        while let Ok(event) = muda::MenuEvent::receiver().try_recv() {
            let action = event.id().0.as_str();
            log::info!("menu action: {action}");
            let blocked = self.runtime.blocks_input(&self.model);
            if blocked && action == "close-tab" {
                self.runtime.close_overlay(&mut self.model);
            }
            // Native menu accelerators bypass egui's shortcut handler. While
            // a modal owns input, only global surfaces and Cmd+W (close the
            // modal) may run; never toggle the shell or close a hidden tab.
            let allowed = matches!(
                action,
                "settings"
                    | "about"
                    | "quit"
                    | "new-window"
                    | "palette"
                    | "find"
                    | "save"
                    | "save-all"
            );
            if !blocked || allowed {
                match action {
                    "about" => {
                        self.runtime.settings_open = true;
                        self.model.ui.settings_section = 3;
                        self.model.ui.command_palette_open = false;
                        self.runtime.search.open = false;
                    }
                    "settings" => {
                        self.runtime.settings_open = true;
                        self.runtime.about_open = false;
                        self.model.ui.command_palette_open = false;
                        self.runtime.search.open = false;
                    }
                    "quit" => self.request_quit(event_loop),
                    "open-project" => self.model.queue(lazecode_core::Command::OpenProjectDialog),
                    "new-window" => {
                        let exe = std::env::current_exe();
                        let spawned =
                            exe.and_then(|exe| std::process::Command::new(exe).spawn().map(|_| ()));
                        if let Err(error) = spawned {
                            self.model.toast(
                                format!("Cannot open a new window: {error}"),
                                lazecode_core::ToastKind::Error,
                            );
                        }
                    }
                    "new-file" => {
                        if let Some(project) = self.model.active_project() {
                            self.runtime.dialog = lazecode_ui::runtime::DialogState::NewFile {
                                dir: project.root.clone(),
                                value: String::new(),
                                error: None,
                            };
                        }
                    }
                    "clone-repo" => {
                        self.runtime.dialog = lazecode_ui::runtime::DialogState::CloneRepo {
                            url: String::new(),
                            dest: String::new(),
                            error: None,
                        };
                    }
                    "close-project" => {
                        if let Some(active) = self.model.active_project {
                            self.model
                                .queue(lazecode_core::Command::CloseProject(active));
                        }
                    }
                    "new-console" => self.model.queue(lazecode_core::Command::NewTerminal),
                    "undo" => {
                        if let Some(project) = self.model.active_project_mut() {
                            if let Some(id) = project.active_tab {
                                if let Some(tab) = project.tab_mut_by_id(id) {
                                    if let lazecode_core::TabKind::File(file_tab) = &mut tab.kind {
                                        file_tab.doc.undo();
                                    }
                                }
                            }
                        }
                    }
                    "redo" => {
                        if let Some(project) = self.model.active_project_mut() {
                            if let Some(id) = project.active_tab {
                                if let Some(tab) = project.tab_mut_by_id(id) {
                                    if let lazecode_core::TabKind::File(file_tab) = &mut tab.kind {
                                        file_tab.doc.redo();
                                    }
                                }
                            }
                        }
                    }
                    "save-all" => self.model.queue(lazecode_core::Command::SaveAll),
                    "goto-line" => {
                        self.model.ui.goto_line_open = true;
                        self.model.ui.goto_line_input.clear();
                    }
                    "git-commit" => {
                        self.model.ui.sidebar_visible = true;
                        self.model.ui.sidebar_view = lazecode_core::SidebarView::Git;
                    }
                    "git-push" => {
                        if let Some(project) = self.model.active_project() {
                            self.runtime.dialog = lazecode_ui::runtime::DialogState::Push {
                                remote: project
                                    .remotes
                                    .first()
                                    .map(|r| r.name.clone())
                                    .unwrap_or_else(|| "origin".into()),
                                branch: project.branch(),
                                set_upstream: false,
                                status: None,
                            };
                        }
                    }
                    "git-pull" => {
                        self.runtime.dialog = lazecode_ui::runtime::DialogState::Pull {
                            mode: lazecode_git::PullMode::Merge,
                            status: None,
                        };
                    }
                    "git-fetch" => {
                        self.model.queue(lazecode_core::Command::FetchAll);
                    }
                    "git-remotes" => {
                        let remotes = self
                            .model
                            .active_project()
                            .map(|p| p.remotes.clone())
                            .unwrap_or_default();
                        self.runtime.dialog = lazecode_ui::runtime::DialogState::Remotes {
                            remotes,
                            new_name: String::new(),
                            new_url: String::new(),
                            error: None,
                        };
                    }
                    "git-stashes" => {
                        self.runtime.dialog = lazecode_ui::runtime::DialogState::StashManager {
                            stashes: Vec::new(),
                            selected: 0,
                            error: None,
                            loaded: false,
                        };
                    }
                    "git-shelves" => {
                        self.runtime.dialog = lazecode_ui::runtime::DialogState::ShelfManager {
                            shelves: Vec::new(),
                            selected: 0,
                            error: None,
                            loaded: false,
                        };
                    }
                    "git-tools" => {
                        self.runtime.dialog = lazecode_ui::runtime::DialogState::GitTools {
                            tab: lazecode_ui::runtime::GitToolsTab::Submodules,
                            submodules: Vec::new(),
                            lfs_status: None,
                            loaded: (false, false),
                        };
                    }
                    "git-refresh" => {
                        self.model.queue(lazecode_core::Command::ReloadGit);
                        self.model.queue(lazecode_core::Command::ReloadTree);
                    }
                    "save" => {
                        if let Some(project) = self.model.active_project() {
                            let command = match project.active_tab {
                                Some(id) => lazecode_core::Command::SaveTab(id),
                                None => lazecode_core::Command::SaveAll,
                            };
                            self.model.queue(command);
                        }
                    }
                    "close-tab" => {
                        if let Some(project) = self.model.active_project_mut() {
                            if let Some(id) = project.active_tab {
                                if let Some(tab) = project.tab_by_id(id) {
                                    if tab.is_dirty() {
                                        self.runtime.dialog =
                                            lazecode_ui::runtime::DialogState::CloseDirtyTab {
                                                tab_id: id,
                                                name: tab.title(),
                                            };
                                    } else {
                                        project.close_tab(id);
                                        self.runtime.editor_focus = None;
                                    }
                                }
                            }
                        }
                    }
                    "toggle-explorer" => {
                        self.model.ui.sidebar_visible = !self.model.ui.sidebar_visible;
                    }
                    "toggle-console" => {
                        self.model.ui.panel_visible = !self.model.ui.panel_visible;
                    }
                    "git-log" => {
                        self.model.ui.panel_visible = true;
                        self.model.ui.panel_tab = lazecode_core::PanelTab::Git;
                        self.model.queue(lazecode_core::Command::RefreshLog);
                    }
                    "palette" => {
                        self.runtime.open_palette();
                        self.runtime.settings_open = false;
                        self.runtime.search.open = false;
                        self.model.ui.command_palette_open = true;
                    }
                    "find" => {
                        self.runtime.search.open = true;
                        self.runtime.search.focus_requested = true;
                        self.runtime.search.searched = false;
                        self.runtime.settings_open = false;
                        self.model.ui.command_palette_open = false;
                    }
                    _ => {}
                }
            }
            if let Some(gfx) = &self.gfx {
                gfx.window.request_redraw();
            }
        }
    }

    fn poll_jobs(&mut self) -> bool {
        let mut changed = false;
        while let Ok(completion) = self.jobs.rx.try_recv() {
            self.jobs
                .apply(&mut self.model, &mut self.runtime, completion);
            changed = true;
        }
        self.jobs.ensure_selected_details(&mut self.model);
        changed
    }

    fn poll_watchers(&mut self) -> bool {
        if self.last_watch_poll.elapsed() < std::time::Duration::from_millis(600) {
            return false;
        }
        self.last_watch_poll = std::time::Instant::now();
        let mut changed: Vec<(std::path::PathBuf, Vec<std::path::PathBuf>)> = Vec::new();
        for project in &mut self.model.projects {
            // The main worktree watcher lives on the project itself; linked
            // worktrees carry their own.
            if let Some(watcher) = project.watcher.as_ref() {
                if let Some(batch) = watcher.poll() {
                    lazecode_git::invalidate_ignored_cache(&project.root);
                    changed.push((project.root.clone(), batch.changed));
                }
            }
            for worktree in project.worktrees.iter().skip(1) {
                if let Some(watcher) = worktree.watcher.as_ref() {
                    if let Some(batch) = watcher.poll() {
                        lazecode_git::invalidate_ignored_cache(&worktree.path);
                        changed.push((worktree.path.clone(), batch.changed));
                    }
                }
            }
        }
        let had_changes = !changed.is_empty();
        let mut warnings = Vec::new();
        for (root, paths) in changed {
            // Find the worktree (its path) whose watcher fired.
            let Some((position, wt_index)) =
                self.model
                    .projects
                    .iter()
                    .enumerate()
                    .find_map(|(project_index, project)| {
                        let path_match = project.root == root;
                        if path_match {
                            return Some((
                                project_index,
                                project
                                    .worktrees
                                    .iter()
                                    .position(|worktree| worktree.is_main)?,
                            )); // index 0
                        }
                        project
                            .worktrees
                            .iter()
                            .position(|worktree| worktree.path == root)
                            .map(|worktree_index| (project_index, worktree_index))
                    })
            else {
                continue;
            };
            let project = &mut self.model.projects[position];
            if wt_index == 0 {
                project.tree = lazecode_fs::tree::load_tree(&project.root);
            } else {
                project.worktrees[wt_index].tree =
                    lazecode_fs::tree::load_tree(&project.worktrees[wt_index].path);
            }
            for path in paths {
                let canonical = path.canonicalize().unwrap_or(path);
                let Some(tab) = project.tabs.iter_mut().find(|tab| {
                    matches!(&tab.kind, lazecode_core::TabKind::File(file) if file.doc.path == canonical)
                }) else {
                    continue;
                };
                if let lazecode_core::TabKind::File(file) = &mut tab.kind {
                    if file.doc.dirty {
                        // Our own atomic save also produces a watcher event.
                        // Do not report it as a foreign modification.
                        if !self.jobs.is_saving(&canonical) {
                            warnings.push(format!(
                                "{} changed on disk; save or reload it",
                                file.doc.title()
                            ));
                        }
                    } else if let Err(err) = file.doc.reload() {
                        warnings.push(format!("Cannot reload {}: {err}", file.doc.title()));
                    }
                }
            }
            self.jobs.refresh_git(root, self.model.notifier.clone());
        }
        for warning in warnings {
            self.model.toast(warning, lazecode_core::ToastKind::Warning);
        }
        had_changes
    }

    fn autosave(&mut self) {
        if !self.model.settings.editor.auto_save
            || self.last_autosave.elapsed() < std::time::Duration::from_secs(2)
        {
            return;
        }
        self.last_autosave = std::time::Instant::now();
        for project in &mut self.model.projects {
            for tab in &mut project.tabs {
                let TabKind::File(file) = &mut tab.kind else {
                    continue;
                };
                if file.doc.dirty {
                    if let Err(err) = file.doc.save() {
                        log::warn!("auto-save failed for {}: {err}", file.doc.path.display());
                    }
                }
            }
        }
    }

    fn has_unsaved(&self) -> bool {
        self.model
            .projects
            .iter()
            .any(|project| project.tabs.iter().any(|tab| tab.is_dirty()))
    }

    /// Finder files and screenshot/image clipboards have no usable text
    /// event. Intercept Cmd+V before egui-winit consumes it when the terminal
    /// owns focus, then hand absolute paths to the PTY as a bracketed paste.
    fn try_terminal_attachment_paste(&mut self) -> bool {
        if !self.model.ui.terminal_visible
            || self.runtime.editor_focus.is_some()
            || self.runtime.blocks_input(&self.model)
        {
            return false;
        }
        let Some(pane_id) = self.runtime.terminal_focus else {
            return false;
        };
        let Some(project) = self.model.active_project() else {
            return false;
        };
        let Some(pane) = project.active_worktree().pane(pane_id) else {
            return false;
        };
        let paths = clipboard::file_paths();
        let Some(text) = clipboard::attachment_text(&paths) else {
            return false;
        };
        pane.session.send_text(&text);
        true
    }

    fn poll_terminal_busy(&mut self) -> bool {
        let mut changed = false;
        while let Ok(rows) = self.busy_rx.try_recv() {
            self.busy_in_flight = false;
            self.runtime.terminal_busy = rows
                .into_iter()
                .map(|row| {
                    (
                        row.session_id,
                        lazecode_ui::runtime::TerminalBusyState {
                            busy: row.busy,
                            outputting: row.outputting,
                            commands: row.commands,
                            last_output_ms: row.last_output_ms,
                        },
                    )
                })
                .collect();
            changed = true;
        }
        changed
    }

    fn start_terminal_busy_poll(&mut self) {
        if self.busy_in_flight || self.last_busy_poll.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.last_busy_poll = std::time::Instant::now();

        let mut targets: Vec<(u64, BusyTarget, Option<u64>)> = Vec::new();
        for project in &self.model.projects {
            for pane in project
                .worktrees
                .iter()
                .flat_map(|worktree| worktree.terminals.iter())
                .flat_map(|tab| tab.panes.iter())
            {
                targets.push((
                    pane.id,
                    pane.session.busy_target(),
                    pane.session.last_output_ms(),
                ));
            }
        }
        if targets.is_empty() {
            self.runtime.terminal_busy.clear();
            return;
        }

        self.busy_in_flight = true;
        let tx = self.busy_tx.clone();
        std::thread::spawn(move || {
            let mut rows = Vec::with_capacity(targets.len());
            // Match the stable batch policy: at most eight `ps` probes at once,
            // then serial batches, so many panes don't stampede at once.
            for chunk in targets.chunks(8) {
                let batch: Vec<TerminalBusyRow> = std::thread::scope(|scope| {
                    let handles: Vec<_> = chunk
                        .iter()
                        .cloned()
                        .map(|(session_id, target, last_output_ms)| {
                            scope.spawn(move || {
                                let info = probe_busy(&target);
                                TerminalBusyRow {
                                    session_id,
                                    busy: info.busy,
                                    outputting: info.busy
                                        && last_output_ms.is_some_and(|age| age < 2000),
                                    commands: info.commands,
                                    last_output_ms,
                                }
                            })
                        })
                        .collect();
                    handles
                        .into_iter()
                        .filter_map(|handle| handle.join().ok())
                        .collect()
                });
                rows.extend(batch);
            }
            let _ = tx.send(rows);
        });
    }

    fn request_quit(&mut self, event_loop: &ActiveEventLoop) {
        let commands = self.runtime.probe_all_terminal_busy(&self.model);
        if self.has_unsaved() || !commands.is_empty() {
            self.runtime.dialog = lazecode_ui::runtime::DialogState::Quit { commands };
            if let Some(gfx) = &self.gfx {
                gfx.window.request_redraw();
            }
        } else {
            self.model.save_workspace();
            self.model.save_settings();
            self.model.shutdown_lsp();
            event_loop.exit();
        }
    }

    fn process_commands(&mut self) -> bool {
        let commands = self.model.drain_commands();
        let had_commands = !commands.is_empty();
        for command in commands {
            self.jobs
                .execute(&mut self.model, &mut self.runtime, command);
        }
        had_commands
    }

    fn render(&mut self) {
        let Some(gfx) = self.gfx.as_mut() else {
            return;
        };
        let window = gfx.window.clone();
        let raw_input = gfx.egui_state.take_egui_input(&window);
        let ctx = gfx.egui_ctx.clone();
        if self.runtime.fonts_dirty {
            lazecode_ui::install_fonts(&ctx, &self.model.settings);
            self.runtime.fonts_dirty = false;
        }
        let output = ctx.run_ui(raw_input, |ui| {
            lazecode_ui::draw(ui, &mut self.model, &mut self.runtime);
        });
        self.repaint_after = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|viewport| viewport.repaint_delay)
            .unwrap_or(std::time::Duration::MAX);
        gfx.egui_state
            .handle_platform_output(&window, output.platform_output.clone());
        if output.platform_output.cursor_icon == egui::CursorIcon::Text {
            window.set_cursor(winit::window::CursorIcon::Text);
        }
        let pixels = output.pixels_per_point;
        let primitives = ctx.tessellate(output.shapes, pixels);
        let mut textures = output.textures_delta;
        let clear = [
            (self.model.settings.theme == lazecode_core::ThemeMode::Dark) as u8 as f32 * 0.04
                + 0.04,
            0.05,
            0.07,
            1.0,
        ];
        let title = self
            .model
            .active_project()
            .map(|project| format!("LazeCode — {}", project.name))
            .unwrap_or_else(|| "LazeCode".to_string());
        window.set_title(&title);
        gfx.painter.paint_and_update_textures(
            egui::ViewportId::ROOT,
            pixels,
            clear,
            &primitives,
            &mut textures,
            vec![],
            &window,
        );
    }
}

impl ApplicationHandler<AppEvent> for LazeApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(err) = self.init(event_loop) {
            log::error!("cannot initialise window: {err:#}");
            event_loop.exit();
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: AppEvent) {
        if let Some(gfx) = &self.gfx {
            gfx.window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.gfx.is_none() {
            return;
        }
        if let WindowEvent::KeyboardInput { event, .. } = &event {
            let plain_command = self.modifiers.super_key()
                && !self.modifiers.control_key()
                && !self.modifiers.alt_key();
            let paste_key = event.logical_key == winit::keyboard::Key::Character("v".into())
                || event.physical_key == PhysicalKey::Code(KeyCode::KeyV);
            if event.state == ElementState::Pressed
                && !event.repeat
                && plain_command
                && paste_key
                && self.try_terminal_attachment_paste()
            {
                if let Some(gfx) = &self.gfx {
                    gfx.window.request_redraw();
                }
                return;
            }
        }
        let gfx = self.gfx.as_mut().expect("gfx checked above");
        let window = Arc::clone(&gfx.window);
        let response = gfx.egui_state.on_window_event(&gfx.window, &event);
        if response.repaint {
            gfx.window.request_redraw();
        }
        match event {
            WindowEvent::CloseRequested => {
                self.request_quit(event_loop);
            }
            WindowEvent::Resized(size) => {
                if let (Some(width), Some(height)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                {
                    gfx.painter
                        .on_window_resized(egui::ViewportId::ROOT, width, height);
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = gfx.window.inner_size();
                if let (Some(width), Some(height)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                {
                    gfx.painter
                        .on_window_resized(egui::ViewportId::ROOT, width, height);
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    if self.modifiers.super_key()
                        && event.logical_key == winit::keyboard::Key::Character("q".into())
                    {
                        self.request_quit(event_loop);
                    } else if event.logical_key
                        == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Shift)
                        && !event.repeat
                        && !self.modifiers.super_key()
                        && !self.modifiers.control_key()
                        && !self.modifiers.alt_key()
                    {
                        let now = std::time::Instant::now();
                        if let Some(last) = self.last_shift_press {
                            if now.duration_since(last) < std::time::Duration::from_millis(450) {
                                self.runtime.open_palette();
                                self.model.ui.command_palette_open = true;
                                self.last_shift_press = None;
                                gfx.window.request_redraw();
                            } else {
                                self.last_shift_press = Some(now);
                            }
                        } else {
                            self.last_shift_press = Some(now);
                        }
                    } else if event.logical_key
                        != winit::keyboard::Key::Named(winit::keyboard::NamedKey::Shift)
                    {
                        self.last_shift_press = None;
                    }
                }
            }
            WindowEvent::DroppedFile(path) => {
                if path.is_dir() {
                    let _ = self.model.open_project(path);
                } else if path.is_file() {
                    let settings = self.model.settings.clone();
                    if let Some(project) = self.model.active_project_mut() {
                        let _ = project.open_file(path, &settings);
                    } else {
                        let _ = self.model.open_project(path);
                    }
                }
                gfx.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                self.poll_jobs();
                self.poll_watchers();
                let had_busy = self.poll_terminal_busy();
                self.start_terminal_busy_poll();
                if had_busy {
                    window.request_redraw();
                }
                self.model.tick();
                self.autosave();
                self.render();
                let had_commands = self.process_commands();
                self.poll_jobs();
                if self.model.should_quit {
                    self.model.save_workspace();
                    self.model.save_settings();
                    self.model.shutdown_lsp();
                    event_loop.exit();
                }
                if had_commands || self.repaint_after.is_zero() {
                    window.request_redraw();
                }
            }
            _ => {}
        }
        if self.started.elapsed().as_millis() < 1500 {
            if let Some(gfx) = &self.gfx {
                gfx.window.request_redraw();
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(target_os = "macos")]
        self.poll_menu(event_loop);
        let had_jobs = self.poll_jobs();
        let had_watcher_changes = self.poll_watchers();
        let had_busy = self.poll_terminal_busy();
        self.start_terminal_busy_poll();
        if self.last_workspace_save.elapsed() > std::time::Duration::from_secs(30) {
            self.last_workspace_save = std::time::Instant::now();
            self.model.save_workspace();
        }

        let now = std::time::Instant::now();
        let mut deadline = now + self.repaint_after.min(std::time::Duration::from_secs(1));
        deadline = deadline.min(self.last_watch_poll + std::time::Duration::from_millis(600));
        deadline = deadline.min(self.last_busy_poll + std::time::Duration::from_secs(2));
        deadline = deadline.min(self.last_workspace_save + std::time::Duration::from_secs(30));
        if self.model.settings.editor.auto_save {
            deadline = deadline.min(self.last_autosave + std::time::Duration::from_secs(2));
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));

        if had_jobs || had_watcher_changes || had_busy || self.repaint_after.is_zero() {
            if let Some(gfx) = &self.gfx {
                gfx.window.request_redraw();
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let event_loop = EventLoop::<AppEvent>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();
    let notifier: Arc<dyn Notifier> = Arc::new(ProxyNotifier(Mutex::new(proxy.clone())));
    let (settings, warning) = Settings::load();
    let mut model = AppModel::new(settings, Some(notifier.clone()));
    if let Some(warning) = warning {
        model.toast(warning, lazecode_core::ToastKind::Warning);
    }
    let jobs = JobQueue::new(notifier.clone());
    let mut app = LazeApp::new(model, jobs);
    for arg in std::env::args().skip(1) {
        if !arg.starts_with('-') {
            let path = std::path::PathBuf::from(&arg);
            if path.exists() {
                let _ = app.model.open_project(path);
                break;
            }
        }
    }
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn pick<'a>(zh: bool, en: &'a str, cn: &'a str) -> &'a str {
    if zh {
        cn
    } else {
        en
    }
}

#[cfg(target_os = "macos")]
fn build_menu(zh: bool) -> Option<Menu> {
    let accel = |key: Code| Some(Accelerator::new(CMD_OR_CTRL, key));
    let accel_shift = |key: Code| Some(Accelerator::new(CMD_OR_CTRL | MenuModifiers::SHIFT, key));
    let menu = Menu::new();

    let app = Submenu::with_items(
        "LazeCode",
        true,
        &[
            &MenuItem::with_id(
                "about",
                pick(zh, "About LazeCode", "关于 LazeCode"),
                true,
                None,
            ),
            &MenuItem::with_id(
                "settings",
                pick(zh, "Settings…", "设置…"),
                true,
                accel(Code::Comma),
            ),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::services(Some(pick(zh, "Services", "服务"))),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(Some(pick(zh, "Hide LazeCode", "隐藏 LazeCode"))),
            &PredefinedMenuItem::hide_others(Some(pick(zh, "Hide Others", "隐藏其他"))),
            &PredefinedMenuItem::show_all(Some(pick(zh, "Show All", "显示全部"))),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(
                "quit",
                pick(zh, "Quit LazeCode", "退出 LazeCode"),
                true,
                accel(Code::KeyQ),
            ),
        ],
    )
    .ok()?;
    menu.append(&app).ok()?;

    let file = Submenu::with_items(
        pick(zh, "File", "文件"),
        true,
        &[
            &MenuItem::with_id(
                "new-window",
                pick(zh, "New Window", "新建窗口"),
                true,
                accel_shift(Code::KeyN),
            ),
            &MenuItem::with_id(
                "open-project",
                pick(zh, "Open Project…", "打开项目…"),
                true,
                accel(Code::KeyO),
            ),
            &MenuItem::with_id(
                "new-file",
                pick(zh, "New File…", "新建文件…"),
                true,
                accel(Code::KeyN),
            ),
            &MenuItem::with_id(
                "clone-repo",
                pick(zh, "Clone Repository…", "克隆仓库…"),
                true,
                None,
            ),
            &MenuItem::with_id(
                "close-project",
                pick(zh, "Close Project", "关闭项目"),
                true,
                None,
            ),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(
                "new-console",
                pick(zh, "New Console", "新建控制台"),
                true,
                accel_shift(Code::KeyT),
            ),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id("save", pick(zh, "Save", "保存"), true, accel(Code::KeyS)),
            &MenuItem::with_id(
                "save-all",
                pick(zh, "Save All", "保存全部"),
                true,
                accel_shift(Code::KeyS),
            ),
            &MenuItem::with_id(
                "close-tab",
                pick(zh, "Close Tab", "关闭标签页"),
                true,
                accel(Code::KeyW),
            ),
        ],
    )
    .ok()?;
    menu.append(&file).ok()?;

    let edit = Submenu::with_items(
        pick(zh, "Edit", "编辑"),
        true,
        &[
            &MenuItem::with_id("undo", pick(zh, "Undo", "撤销"), true, accel(Code::KeyZ)),
            &MenuItem::with_id(
                "redo",
                pick(zh, "Redo", "重做"),
                true,
                accel_shift(Code::KeyZ),
            ),
            // Clipboard is handled by egui-winit's `Event::Copy`/`Paste`.
            // Predefined AppKit clipboard items install Cmd+C/V equivalents
            // that consume the key event before the terminal/editor sees it,
            // so they must not live in the native menu.
        ],
    )
    .ok()?;
    menu.append(&edit).ok()?;

    let view = Submenu::with_items(
        pick(zh, "View", "视图"),
        true,
        &[
            &MenuItem::with_id(
                "toggle-explorer",
                pick(zh, "Explorer", "资源管理器"),
                true,
                accel(Code::Digit1),
            ),
            &MenuItem::with_id(
                "toggle-console",
                pick(zh, "Console", "控制台"),
                true,
                accel(Code::Digit4),
            ),
            &MenuItem::with_id(
                "git-log",
                pick(zh, "Git Log", "Git 日志"),
                true,
                accel(Code::Digit9),
            ),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(
                "settings",
                pick(zh, "Settings", "设置"),
                true,
                accel(Code::Comma),
            ),
        ],
    )
    .ok()?;
    menu.append(&view).ok()?;

    let navigate = Submenu::with_items(
        pick(zh, "Navigate", "导航"),
        true,
        &[
            &MenuItem::with_id(
                "palette",
                pick(zh, "Command Palette…", "命令面板…"),
                true,
                accel_shift(Code::KeyA),
            ),
            &MenuItem::with_id(
                "find",
                pick(zh, "Find in Files…", "在文件中查找…"),
                true,
                accel_shift(Code::KeyF),
            ),
            &MenuItem::with_id(
                "goto-line",
                pick(zh, "Go to Line…", "跳转到行…"),
                true,
                accel(Code::KeyL),
            ),
        ],
    )
    .ok()?;
    menu.append(&navigate).ok()?;

    let git = Submenu::with_items(
        "Git",
        true,
        &[
            &MenuItem::with_id(
                "git-commit",
                pick(zh, "Commit…", "提交…"),
                true,
                accel(Code::KeyK),
            ),
            &MenuItem::with_id(
                "git-push",
                pick(zh, "Push…", "推送…"),
                true,
                accel_shift(Code::KeyK),
            ),
            &MenuItem::with_id(
                "git-pull",
                pick(zh, "Pull…", "拉取…"),
                true,
                accel(Code::KeyT),
            ),
            &MenuItem::with_id("git-fetch", pick(zh, "Fetch", "获取远程更新"), true, None),
            &MenuItem::with_id(
                "git-remotes",
                pick(zh, "Manage Remotes…", "管理远程仓库…"),
                true,
                None,
            ),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id("git-stashes", pick(zh, "Stashes…", "贮藏管理…"), true, None),
            &MenuItem::with_id("git-shelves", pick(zh, "Shelves…", "架藏管理…"), true, None),
            &MenuItem::with_id(
                "git-tools",
                pick(zh, "Git Tools (Submodules/LFS)…", "Git 工具 (子模块/LFS)…"),
                true,
                None,
            ),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(
                "git-refresh",
                pick(zh, "Refresh", "刷新"),
                true,
                accel(Code::F5),
            ),
        ],
    )
    .ok()?;
    menu.append(&git).ok()?;
    let window = Submenu::with_items(
        pick(zh, "Window", "窗口"),
        true,
        &[
            &PredefinedMenuItem::minimize(Some(pick(zh, "Minimize", "最小化"))),
            &PredefinedMenuItem::zoom(Some(pick(zh, "Zoom", "缩放"))),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::fullscreen(Some(pick(zh, "Enter Full Screen", "进入全屏"))),
        ],
    )
    .ok()?;
    menu.append(&window).ok()?;
    Some(menu)
}
