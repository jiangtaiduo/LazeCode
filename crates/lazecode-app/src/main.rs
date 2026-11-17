mod clipboard;
mod jobs;

use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use crossbeam_channel::{Receiver, Sender};
use egui_wgpu::winit::Painter;
use egui_wgpu::{RendererOptions, WgpuConfiguration};
use lazecode_core::{AppModel, Settings, TabKind};
use lazecode_terminal::{probe_busy_many, BusyTarget, Notifier};
use lazecode_ui::UiRuntime;
use parking_lot::Mutex;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::platform::macos::{OptionAsAlt, WindowAttributesExtMacOS};
use winit::window::{Window, WindowId};

use block2::RcBlock;
#[cfg(target_os = "macos")]
use muda::accelerator::{Accelerator, Code, Modifiers as MenuModifiers, CMD_OR_CTRL};
#[cfg(target_os = "macos")]
use muda::{Menu, MenuItem, PredefinedMenuItem, Submenu};
#[cfg(target_os = "macos")]
use objc2::runtime::{AnyClass, AnyObject, Sel};
#[cfg(target_os = "macos")]
use objc2::{class, msg_send};
use objc2_app_kit::{NSEvent, NSEventMask};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSPoint, NSRect, NSString};
use std::ptr::NonNull;
#[cfg(target_os = "macos")]
use std::sync::OnceLock;

use jobs::JobQueue;

/// Cheap fingerprint of the `.git` files that determine status: top-level
/// metadata files, `refs/`, and the sequencer dirs. `objects/`, `logs/` and
/// `hooks/` are excluded because they never change a status result.
/// `None` = missing `.git` or an unreadable linked-worktree pointer.
fn git_fingerprint(root: &std::path::Path) -> Option<Vec<(std::path::PathBuf, u64, u64)>> {
    const BUDGET: usize = 800;
    let dot_git = root.join(".git");
    let meta = std::fs::symlink_metadata(&dot_git).ok()?;
    let (git_dir, common_dir) = if meta.is_dir() {
        (dot_git.clone(), dot_git)
    } else if meta.is_file() {
        // Linked worktree: `.git` is `gitdir: /repo/.git/worktrees/<name>`.
        let text = std::fs::read_to_string(&dot_git).ok()?;
        let raw = text
            .lines()
            .find_map(|line| line.trim().strip_prefix("gitdir:").map(str::trim))?;
        let pointer = std::path::PathBuf::from(raw);
        let git_dir = if pointer.is_absolute() {
            pointer
        } else {
            root.join(pointer)
        };
        let git_dir = std::fs::canonicalize(git_dir).ok()?;
        // Status also depends on shared refs in the common git dir.
        let common_dir = std::fs::read_to_string(git_dir.join("commondir"))
            .ok()
            .map(|text| {
                let path = std::path::PathBuf::from(text.trim());
                if path.is_absolute() {
                    path
                } else {
                    git_dir.join(path)
                }
            })
            .and_then(|path| std::fs::canonicalize(path).ok())
            .unwrap_or_else(|| git_dir.clone());
        (git_dir, common_dir)
    } else {
        return None;
    };
    let mut entries: Vec<(std::path::PathBuf, u64, u64)> = Vec::new();
    fn push_file(
        path: &std::path::Path,
        rel: &std::path::Path,
        entries: &mut Vec<(std::path::PathBuf, u64, u64)>,
    ) {
        let Ok(meta) = std::fs::symlink_metadata(path) else {
            return;
        };
        if meta.is_dir() {
            return;
        }
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        entries.push((rel.to_path_buf(), mtime, meta.len()));
    }
    fn walk_dir(
        dir: &std::path::Path,
        rel: &std::path::Path,
        entries: &mut Vec<(std::path::PathBuf, u64, u64)>,
        budget: &mut usize,
    ) {
        if *budget == 0 {
            return;
        }
        let Ok(read_dir) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in read_dir.flatten() {
            if *budget == 0 {
                return;
            }
            let child_rel = rel.join(entry.file_name());
            let child = entry.path();
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                walk_dir(&child, &child_rel, entries, budget);
            } else {
                *budget -= 1;
                push_file(&child, &child_rel, entries);
            }
        }
    }
    fn collect_dir(
        dir: &std::path::Path,
        prefix: &std::path::Path,
        entries: &mut Vec<(std::path::PathBuf, u64, u64)>,
        budget: &mut usize,
    ) {
        // Top-level files only; directories (objects/, logs/, hooks/, ...) are
        // walked below only when they carry state the status depends on.
        if let Ok(read_dir) = std::fs::read_dir(dir) {
            for entry in read_dir.flatten() {
                if *budget == 0 {
                    break;
                }
                let Ok(meta) = entry.metadata() else {
                    continue;
                };
                if meta.is_dir() {
                    continue;
                }
                *budget -= 1;
                push_file(&entry.path(), &prefix.join(entry.file_name()), entries);
            }
        }
        for tracked in ["refs", "rebase-merge", "rebase-apply", "sequencer"] {
            walk_dir(&dir.join(tracked), &prefix.join(tracked), entries, budget);
        }
    }

    let mut budget = BUDGET;
    let linked = common_dir != git_dir;
    collect_dir(
        &git_dir,
        if linked {
            std::path::Path::new("worktree")
        } else {
            std::path::Path::new("")
        },
        &mut entries,
        &mut budget,
    );
    if linked {
        collect_dir(
            &common_dir,
            std::path::Path::new("common"),
            &mut entries,
            &mut budget,
        );
    }
    entries.sort();
    Some(entries)
}

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

#[derive(Clone)]
struct BusyProbeTarget {
    session_id: u64,
    target: BusyTarget,
    last_output_ms: Option<u64>,
    last_input_ms: Option<u64>,
    last_resize_ms: Option<u64>,
    was_outputting: bool,
}

struct Gfx {
    window: Arc<Window>,
    painter: Painter,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
}

#[cfg(target_os = "macos")]
type ZoomImp = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject);

/// AppKit calls `zoom:` before the content view can reject it; discard it when
/// the double-click lands on a titlebar control.
#[cfg(target_os = "macos")]
static ORIGINAL_ZOOM: OnceLock<ZoomImp> = OnceLock::new();
#[cfg(target_os = "macos")]
static ZOOM_GUARD_INSTALLED: OnceLock<()> = OnceLock::new();
#[cfg(target_os = "macos")]
static TITLEBAR_CONTROLS: std::sync::Mutex<Vec<(f32, f32, f32, f32)>> =
    std::sync::Mutex::new(Vec::new());

#[cfg(target_os = "macos")]
unsafe extern "C-unwind" fn titlebar_zoom_guard(
    window: *mut AnyObject,
    command: Sel,
    sender: *mut AnyObject,
) {
    if suppress_control_zoom(window) {
        return;
    }
    if let Some(original) = ORIGINAL_ZOOM.get() {
        unsafe { original(window, command, sender) };
    }
}

#[cfg(target_os = "macos")]
unsafe fn suppress_control_zoom(window: *mut AnyObject) -> bool {
    if window.is_null() {
        return false;
    }
    let Some(app_class) = AnyClass::get(c"NSApplication") else {
        return false;
    };
    let app: *mut AnyObject = unsafe { msg_send![app_class, sharedApplication] };
    if app.is_null() {
        return false;
    }
    let event: *mut AnyObject = unsafe { msg_send![app, currentEvent] };
    if event.is_null() {
        return false;
    }
    let event_type: isize = unsafe { msg_send![event, type] };
    // 1/2/6 = leftMouseDown/leftMouseUp/leftMouseDragged. Native zoom is
    // driven by mouse gestures; menu/keyboard Zoom must keep working.
    if !matches!(event_type, 1 | 2 | 6) {
        return false;
    }
    let point: NSPoint = unsafe { msg_send![event, locationInWindow] };
    let frame: NSRect = unsafe { msg_send![window, frame] };
    // AppKit window coordinates have their origin at the bottom-left; the
    // egui titlebar rects are top-left based.
    let top_y = (frame.size.height - point.y) as f32;
    // Full-size content view with a custom titlebar: suppress AppKit's
    // whole-titlebar double-click zoom and let the egui titlebar handle it.
    if top_y <= lazecode_ui::app_ui::TITLEBAR_H * 2.0 {
        return true;
    }
    let x = point.x as f32;
    let controls = match TITLEBAR_CONTROLS.lock() {
        Ok(controls) => controls,
        Err(poisoned) => poisoned.into_inner(),
    };
    controls.iter().any(|(left, top, right, bottom)| {
        x >= *left && x <= *right && top_y >= *top && top_y <= *bottom
    })
}

#[cfg(target_os = "macos")]
fn install_titlebar_zoom_guard(window: &Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    if ZOOM_GUARD_INSTALLED.set(()).is_err() {
        return;
    }
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    let ns_view = handle.ns_view.as_ptr() as *mut AnyObject;
    let ns_window: *mut AnyObject = unsafe { msg_send![ns_view, window] };
    if ns_window.is_null() {
        return;
    }
    let class: *const AnyClass = unsafe { msg_send![ns_window, class] };
    let Some(class) = (unsafe { class.as_ref() }) else {
        return;
    };
    let selector = Sel::register(c"zoom:");
    let Some(method) = class.instance_method(selector) else {
        return;
    };
    let original: ZoomImp = unsafe { std::mem::transmute(method.implementation()) };
    if ORIGINAL_ZOOM.set(original).is_err() {
        return;
    }
    let guard: objc2::runtime::Imp = unsafe {
        std::mem::transmute::<*const (), objc2::runtime::Imp>(titlebar_zoom_guard as *const ())
    };
    unsafe { method.set_implementation(guard) };
}

#[cfg(target_os = "macos")]
static DOUBLE_CLICK_MONITOR_INSTALLED: OnceLock<()> = OnceLock::new();

/// Native double-click action for the custom titlebar; AppKit's own gesture
/// never reaches a full-size content view.
#[cfg(target_os = "macos")]
unsafe fn perform_standard_titlebar_double_click(ns_window: *mut AnyObject) {
    let defaults: *mut AnyObject =
        unsafe { msg_send![class!(NSUserDefaults), standardUserDefaults] };
    let domain_name = NSString::from_str("NSGlobalDomain");
    let domain: *mut AnyObject =
        unsafe { msg_send![defaults, persistentDomainForName: &*domain_name] };
    let key = NSString::from_str("AppleActionOnDoubleClick");
    let mut action = "zoom";
    if !domain.is_null() {
        let value: *mut AnyObject = unsafe { msg_send![domain, objectForKey: &*key] };
        let is_string: bool = unsafe { msg_send![value, isKindOfClass: class!(NSString)] };
        if !value.is_null() && is_string {
            let utf8: *const std::ffi::c_char = unsafe { msg_send![value, UTF8String] };
            if !utf8.is_null() {
                let text = unsafe { std::ffi::CStr::from_ptr(utf8) }
                    .to_string_lossy()
                    .to_ascii_lowercase();
                match text.as_str() {
                    "minimize" | "miniaturize" => action = "minimize",
                    "none" | "no action" => action = "none",
                    _ => {}
                }
            }
        }
    }
    if action == "zoom" && !domain.is_null() {
        let legacy_key = NSString::from_str("AppleMiniaturizeOnDoubleClick");
        let legacy: bool = unsafe { msg_send![defaults, boolForKey: &*legacy_key] };
        if legacy {
            action = "minimize";
        }
    }
    match action {
        "minimize" => {
            let _: () =
                unsafe { msg_send![ns_window, miniaturize: std::ptr::null_mut::<AnyObject>()] };
        }
        "none" => {}
        _ => {
            if let Some(original) = ORIGINAL_ZOOM.get() {
                unsafe { original(ns_window, Sel::register(c"zoom:"), std::ptr::null_mut()) };
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn titlebar_point_is_control(x: f32, y: f32) -> bool {
    let controls = match TITLEBAR_CONTROLS.lock() {
        Ok(controls) => controls,
        Err(poisoned) => poisoned.into_inner(),
    };
    controls
        .iter()
        .any(|(left, top, right, bottom)| x >= *left && x <= *right && y >= *top && y <= *bottom)
}

#[cfg(target_os = "macos")]
fn install_titlebar_double_click_monitor(window: &Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    if DOUBLE_CLICK_MONITOR_INSTALLED.set(()).is_err() {
        return;
    }
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    let ns_view = handle.ns_view.as_ptr() as *mut AnyObject;
    let ns_window: *mut AnyObject = unsafe { msg_send![ns_view, window] };
    if ns_window.is_null() {
        return;
    }
    let handler = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        let event_ref = unsafe { event.as_ref() };
        let event_ptr = event.as_ptr();
        let event_window: *mut AnyObject = unsafe { msg_send![event_ref, window] };
        if event_window.is_null() || event_window != ns_window {
            return event_ptr;
        }
        if event_ref.clickCount() != 2 {
            return event_ptr;
        }
        let point = event_ref.locationInWindow();
        let frame: NSRect = unsafe { msg_send![ns_window, frame] };
        let top_y = frame.size.height - point.y;
        if point.x < 78.0
            || top_y < 0.0
            || top_y > lazecode_ui::app_ui::TITLEBAR_H as f64
            || titlebar_point_is_control(point.x as f32, top_y as f32)
        {
            return event_ptr;
        }
        unsafe { perform_standard_titlebar_double_click(ns_window) };
        std::ptr::null_mut()
    });
    let mask = NSEventMask::LeftMouseDown;
    let monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &handler) };
    if monitor.is_some() {
        // AppKit retains the monitor and its block for the app lifetime.
        std::mem::forget(handler);
    }
}

/// Coalesces paint requests into one bounded frame budget; background and
/// occluded windows get fewer (or no) frames.
struct FramePacer {
    next: Instant,
    focused: bool,
    occluded: bool,
}

impl FramePacer {
    const ACTIVE_INTERVAL: Duration = Duration::from_micros(16_667);
    const BACKGROUND_INTERVAL: Duration = Duration::from_millis(100);

    fn new() -> Self {
        Self {
            next: Instant::now(),
            focused: true,
            occluded: false,
        }
    }

    fn interval(&self) -> Duration {
        if self.focused {
            Self::ACTIVE_INTERVAL
        } else {
            Self::BACKGROUND_INTERVAL
        }
    }

    fn due(&self, now: Instant) -> bool {
        !self.occluded && now >= self.next
    }

    fn note_frame(&mut self, now: Instant) {
        self.next = now + self.interval();
    }

    fn set_focused(&mut self, focused: bool, now: Instant) {
        self.focused = focused;
        // Re-evaluate immediately on focus gain so the first frame is not
        // delayed by a background-length interval.
        self.next = self.next.min(now);
    }

    fn set_occluded(&mut self, occluded: bool, now: Instant) {
        self.occluded = occluded;
        if !occluded {
            self.next = self.next.min(now);
        }
    }
}

const DROP_BATCH_DELAY: Duration = Duration::from_millis(50);

/// What a batch of dropped paths should do. Files and folders are intentionally
/// never mixed: a folder means "open project", a file means "single-file mode".
#[derive(Debug, PartialEq, Eq)]
enum DroppedPaths {
    Files(Vec<std::path::PathBuf>),
    Projects(Vec<std::path::PathBuf>),
    Mixed,
    Empty,
}

fn classify_dropped_paths(paths: Vec<std::path::PathBuf>) -> DroppedPaths {
    let mut files = Vec::new();
    let mut projects = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for path in paths {
        if !seen.insert(path.clone()) {
            continue;
        }
        if path.is_dir() {
            projects.push(path);
        } else if path.is_file() {
            files.push(path);
        }
    }
    if !files.is_empty() && !projects.is_empty() {
        DroppedPaths::Mixed
    } else if !projects.is_empty() {
        DroppedPaths::Projects(projects)
    } else if !files.is_empty() {
        DroppedPaths::Files(files)
    } else {
        DroppedPaths::Empty
    }
}

struct LazeApp {
    model: AppModel,
    runtime: UiRuntime,
    gfx: Option<Gfx>,
    jobs: JobQueue,
    /// Native drop events arrive one path at a time; collect them briefly so
    /// mixed file/folder batches can be rejected before anything opens.
    pending_drops: Vec<std::path::PathBuf>,
    drop_deadline: Option<Instant>,
    last_watch_poll: std::time::Instant,
    last_vcs_poll: std::time::Instant,
    last_full_vcs_poll: std::time::Instant,
    git_fingerprint: Option<Vec<(std::path::PathBuf, u64, u64)>>,
    last_busy_poll: std::time::Instant,
    busy_tx: Sender<Vec<TerminalBusyRow>>,
    busy_rx: Receiver<Vec<TerminalBusyRow>>,
    busy_in_flight: bool,
    started: std::time::Instant,
    modifiers: ModifiersState,
    last_workspace_save: std::time::Instant,
    last_autosave: std::time::Instant,
    repaint_after: std::time::Duration,
    pacer: FramePacer,
    wants_frame: bool,
    last_shift_press: Option<Instant>,
    #[cfg(target_os = "macos")]
    menu: Option<Menu>,
    #[cfg(target_os = "macos")]
    menu_locale_zh: bool,
}

impl LazeApp {
    fn new(model: AppModel, jobs: JobQueue) -> Self {
        let (busy_tx, busy_rx) = crossbeam_channel::unbounded();
        let mut runtime = UiRuntime::default();
        // Close/quit probes notify this when they finish so the confirmation
        // dialog is not delayed until the next poll deadline.
        runtime.wake_notifier = model.notifier.clone();
        Self {
            model,
            runtime,
            gfx: None,
            jobs,
            pending_drops: Vec::new(),
            drop_deadline: None,
            last_watch_poll: std::time::Instant::now(),
            // The first VCS poll runs on the first idle turn; native file
            // watchers are best effort and can miss .git ref/index rewrites.
            last_vcs_poll: std::time::Instant::now()
                .checked_sub(Duration::from_secs(3))
                .unwrap_or_else(std::time::Instant::now),
            last_full_vcs_poll: std::time::Instant::now()
                .checked_sub(Duration::from_secs(31))
                .unwrap_or_else(std::time::Instant::now),
            git_fingerprint: None,
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
            pacer: FramePacer::new(),
            wants_frame: false,
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
        #[cfg(target_os = "macos")]
        install_titlebar_zoom_guard(window.as_ref());
        #[cfg(target_os = "macos")]
        install_titlebar_double_click_monitor(window.as_ref());

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
    fn publish_titlebar_controls(&self) {
        let mut controls = match TITLEBAR_CONTROLS.lock() {
            Ok(controls) => controls,
            Err(poisoned) => poisoned.into_inner(),
        };
        controls.clear();
        controls.extend(
            self.runtime
                .titlebar_controls
                .iter()
                .map(|rect| (rect.min.x, rect.min.y, rect.max.x, rect.max.y)),
        );
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
                    | "check-updates"
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
                    "check-updates" => {
                        self.runtime.update_open = true;
                        self.runtime.settings_open = false;
                        self.runtime.about_open = false;
                        self.model.ui.command_palette_open = false;
                        let version = self.model.version;
                        if self
                            .runtime
                            .update
                            .begin_check(&mut self.model.settings, version, false)
                        {
                            self.model.save_settings();
                        }
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
                            let remote = project
                                .remotes
                                .first()
                                .map(|r| r.name.clone())
                                .unwrap_or_else(|| "origin".into());
                            self.runtime.dialog = lazecode_ui::overlays::push_dialog_state(
                                project,
                                remote,
                                project.branch(),
                            );
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
                            editing: None,
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
                        if let Some(index) = self.model.active_project {
                            if let Some(id) = self.model.projects[index].active_tab {
                                if let Some(tab) = self.model.projects[index].tab_by_id(id) {
                                    if tab.is_dirty() {
                                        self.runtime.dialog =
                                            lazecode_ui::runtime::DialogState::CloseDirtyTab {
                                                tab_id: id,
                                                name: tab.title(),
                                            };
                                    } else {
                                        self.model.close_tab(index, id);
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
        let mut changed: Vec<(
            std::path::PathBuf,
            Vec<std::path::PathBuf>,
            bool,
            bool,
            bool,
        )> = Vec::new();
        for project in &mut self.model.projects {
            // The main worktree watcher lives on the project itself; linked
            // worktrees carry their own.
            if let Some(watcher) = project.watcher.as_ref() {
                if let Some(batch) = watcher.poll() {
                    if batch.ignored_touched || batch.structure_changed {
                        lazecode_git::invalidate_ignored_cache(&project.root);
                    }
                    changed.push((
                        project.root.clone(),
                        batch.changed,
                        batch.git_changed,
                        batch.structure_changed,
                        batch.ignored_touched,
                    ));
                }
            }
            for worktree in project.worktrees.iter().skip(1) {
                if let Some(watcher) = worktree.watcher.as_ref() {
                    if let Some(batch) = watcher.poll() {
                        if batch.ignored_touched || batch.structure_changed {
                            lazecode_git::invalidate_ignored_cache(&worktree.path);
                        }
                        changed.push((
                            worktree.path.clone(),
                            batch.changed,
                            batch.git_changed,
                            batch.structure_changed,
                            batch.ignored_touched,
                        ));
                    }
                }
            }
        }
        let mut had_changes = !changed.is_empty();
        let mut warnings = Vec::new();
        for (root, paths, git_changed, structure_changed, _ignored_touched) in changed {
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
            let had_paths = !paths.is_empty();
            if structure_changed {
                // Tree scans run as a background job; custom ignore patterns
                // must match the ones the tree was loaded with.
                let worktree_path = self.model.projects[position].worktrees[wt_index]
                    .path
                    .clone();
                self.jobs.refresh_tree(
                    worktree_path,
                    Arc::new(self.model.settings.ignored_patterns.clone()),
                );
            }
            let canonical_paths: std::collections::HashSet<std::path::PathBuf> = paths
                .into_iter()
                .map(|path| path.canonicalize().unwrap_or(path))
                .collect();
            let project = &mut self.model.projects[position];
            for tab in &mut project.tabs {
                let lazecode_core::TabKind::File(file) = &mut tab.kind else {
                    continue;
                };
                if !canonical_paths.contains(&file.doc.path) {
                    continue;
                }
                if file.doc.dirty {
                    // Our own atomic save also fires a watcher event; do not
                    // report it, but stamp the metadata so the fallback poll
                    // does not repeat it.
                    if !self.jobs.is_saving(&file.doc.path) {
                        warnings.push(format!(
                            "{} changed on disk; save or reload it",
                            file.doc.title()
                        ));
                    }
                    file.stamp_disk();
                } else if let Err(err) = file.doc.reload() {
                    warnings.push(format!("Cannot reload {}: {err}", file.doc.title()));
                } else {
                    file.stamp_disk();
                }
            }
            if git_changed || had_paths {
                self.jobs.refresh_git(root, self.model.notifier.clone());
            }
        }
        // Watcher events are best effort (FSEvents can coalesce or miss an
        // atomic replace). A cheap metadata poll guarantees that an open file
        // edited by another tool is reloaded even when no event arrived.
        for project in &mut self.model.projects {
            for tab in &mut project.tabs {
                let lazecode_core::TabKind::File(file) = &mut tab.kind else {
                    continue;
                };
                if !file.disk_changed() {
                    continue;
                }
                file.stamp_disk();
                had_changes = true;
                if file.doc.dirty {
                    if !self.jobs.is_saving(&file.doc.path) {
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
        for warning in warnings {
            self.model.toast(warning, lazecode_core::ToastKind::Warning);
        }
        had_changes
    }

    /// PyCharm-style VCS fallback: watchers are best effort, so poll the
    /// active checkout. A cheap `.git` fingerprint gates the expensive full
    /// `snapshot()`; a slow unconditional backstop covers missed cases.
    fn poll_vcs(&mut self) -> bool {
        if self.last_vcs_poll.elapsed() < std::time::Duration::from_secs(2) {
            return false;
        }
        let Some(project) = self.model.active_project() else {
            return false;
        };
        if project.is_single_file {
            return false;
        }
        let root = project.active_root().to_path_buf();
        self.last_vcs_poll = std::time::Instant::now();
        let slow_due = self.last_full_vcs_poll.elapsed() >= std::time::Duration::from_secs(30);
        let fingerprint = git_fingerprint(&root);
        match fingerprint {
            // No cheap signature (linked worktree whose .git is a pointer
            // file): rely on the slow backstop alone.
            None => {
                if slow_due {
                    self.last_full_vcs_poll = std::time::Instant::now();
                    self.jobs.refresh_git(root, self.model.notifier.clone());
                    return true;
                }
                false
            }
            Some(signature) => {
                let changed = self.git_fingerprint.as_ref() != Some(&signature);
                self.git_fingerprint = Some(signature);
                if changed || slow_due {
                    if slow_due {
                        self.last_full_vcs_poll = std::time::Instant::now();
                    }
                    self.jobs.refresh_git(root, self.model.notifier.clone());
                    true
                } else {
                    false
                }
            }
        }
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

        let mut targets: Vec<BusyProbeTarget> = Vec::new();
        for project in &self.model.projects {
            for pane in project
                .worktrees
                .iter()
                .flat_map(|worktree| worktree.terminals.iter())
                .flat_map(|tab| tab.panes.iter())
            {
                let was_outputting = self
                    .runtime
                    .terminal_busy
                    .get(&pane.id)
                    .is_some_and(|state| state.outputting);
                targets.push(BusyProbeTarget {
                    session_id: pane.id,
                    target: pane.session.busy_target(),
                    last_output_ms: pane.session.last_output_ms(),
                    last_input_ms: pane.session.last_input_ms(),
                    last_resize_ms: pane.session.last_resize_ms(),
                    was_outputting,
                });
            }
        }
        if targets.is_empty() {
            self.runtime.terminal_busy.clear();
            return;
        }

        self.busy_in_flight = true;
        let tx = self.busy_tx.clone();
        std::thread::spawn(move || {
            // One `ps` snapshot for all panes; the completion send must run
            // even on panic or the in-flight gate never clears.
            let rows = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let probes: Vec<BusyTarget> =
                    targets.iter().map(|target| target.target.clone()).collect();
                let infos = probe_busy_many(&probes);
                let rows: Vec<TerminalBusyRow> = targets
                    .into_iter()
                    .zip(infos)
                    .map(|(target, info)| TerminalBusyRow {
                        session_id: target.session_id,
                        busy: info.busy,
                        outputting: lazecode_ui::runtime::console_outputting(
                            info.busy,
                            target.last_output_ms,
                            target.last_input_ms,
                            target.last_resize_ms,
                            target.was_outputting,
                        ),
                        commands: info.commands,
                        last_output_ms: target.last_output_ms,
                    })
                    .collect();
                rows
            }))
            .unwrap_or_default();
            let _ = tx.send(rows);
        });
    }

    fn finish_drop_batch(&mut self, now: Instant) -> bool {
        if self.pending_drops.is_empty() {
            self.drop_deadline = None;
            return false;
        }
        if self.drop_deadline.is_some_and(|deadline| now < deadline) {
            return false;
        }
        self.drop_deadline = None;
        let paths = std::mem::take(&mut self.pending_drops);
        self.open_dropped_paths(paths);
        true
    }

    fn open_dropped_paths(&mut self, paths: Vec<std::path::PathBuf>) {
        let zh = self.model.settings.language.is_zh();
        match classify_dropped_paths(paths) {
            DroppedPaths::Mixed => {
                self.model.toast(
                    if zh {
                        "不能同时拖入文件和目录，请分次打开"
                    } else {
                        "Drop files and folders separately"
                    },
                    lazecode_core::ToastKind::Warning,
                );
            }
            DroppedPaths::Projects(projects) => {
                let mut opened = false;
                for project in projects {
                    match self.model.open_project(project) {
                        Ok(_) => opened = true,
                        Err(err) => self.model.toast(err, lazecode_core::ToastKind::Error),
                    }
                }
                if opened {
                    self.runtime.clear_terminal_cache();
                }
            }
            DroppedPaths::Files(files) => match self.model.open_files(files) {
                Ok(_) => self.runtime.clear_terminal_cache(),
                Err(err) => self.model.toast(err, lazecode_core::ToastKind::Error),
            },
            DroppedPaths::Empty => {
                self.model.toast(
                    if zh {
                        "只能打开文件或目录"
                    } else {
                        "Only files and folders can be opened"
                    },
                    lazecode_core::ToastKind::Warning,
                );
            }
        }
    }

    fn request_quit(&mut self, _event_loop: &ActiveEventLoop) {
        // The probe runs off-thread; `poll_quit_probe` resolves it, and exit
        // happens where `model.should_quit` is consumed.
        self.runtime.request_quit_probe(&self.model);
        if let Some(gfx) = &self.gfx {
            gfx.window.request_redraw();
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

    fn render(&mut self) -> bool {
        let now = Instant::now();
        if !self.pacer.due(now) {
            return false;
        }
        self.pacer.note_frame(now);
        let Some(gfx) = self.gfx.as_mut() else {
            return false;
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
            .map(|project| {
                if project.is_single_file && project.tabs.is_empty() {
                    "LazeCode".to_string()
                } else {
                    format!("LazeCode — {}", project.name)
                }
            })
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
        true
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
            if self.pacer.due(Instant::now()) {
                gfx.window.request_redraw();
            } else {
                self.wants_frame = true;
            }
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
            if self.pacer.due(Instant::now()) {
                gfx.window.request_redraw();
            } else {
                self.wants_frame = true;
            }
        }
        match event {
            WindowEvent::Focused(focused) => {
                let now = Instant::now();
                self.pacer.set_focused(focused, now);
                if focused {
                    window.request_redraw();
                }
            }
            WindowEvent::Occluded(occluded) => {
                let now = Instant::now();
                self.pacer.set_occluded(occluded, now);
                if !occluded {
                    window.request_redraw();
                }
            }
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
                self.pending_drops.push(path);
                self.drop_deadline
                    .get_or_insert_with(|| Instant::now() + DROP_BATCH_DELAY);
            }
            WindowEvent::RedrawRequested => {
                self.poll_jobs();
                self.poll_watchers();
                self.poll_vcs();
                let had_busy = self.poll_terminal_busy();
                self.start_terminal_busy_poll();
                if had_busy {
                    window.request_redraw();
                }
                self.model.tick();
                self.autosave();
                let rendered = self.render();
                #[cfg(target_os = "macos")]
                if rendered {
                    self.publish_titlebar_controls();
                }
                let had_commands = self.process_commands();
                self.poll_jobs();
                if self.model.should_quit {
                    self.model.save_workspace();
                    self.model.save_settings();
                    self.model.shutdown_lsp();
                    event_loop.exit();
                }
                if had_commands || self.repaint_after.is_zero() {
                    if self.pacer.due(Instant::now()) {
                        window.request_redraw();
                    } else {
                        self.wants_frame = true;
                    }
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
        let had_drops = self.finish_drop_batch(Instant::now());
        let had_jobs = self.poll_jobs();
        let had_watcher_changes = self.poll_watchers();
        let had_vcs_poll = self.poll_vcs();
        let had_busy = self.poll_terminal_busy();
        let had_close_probe = self.runtime.poll_pending_close_probe(&mut self.model);
        let had_quit_probe = self.runtime.poll_quit_probe(&mut self.model);
        self.start_terminal_busy_poll();
        if self.last_workspace_save.elapsed() > std::time::Duration::from_secs(30) {
            self.last_workspace_save = std::time::Instant::now();
            self.model.save_workspace();
        }

        let now = std::time::Instant::now();
        let frame_pending = self.wants_frame
            || had_drops
            || had_jobs
            || had_watcher_changes
            || had_vcs_poll
            || had_busy
            || had_close_probe
            || had_quit_probe
            || self.repaint_after.is_zero();
        let mut deadline = now + self.repaint_after.min(std::time::Duration::from_secs(1));
        if frame_pending && !self.pacer.occluded {
            deadline = deadline.min(self.pacer.next);
        } else if frame_pending {
            // Do not turn an occluded window's pending frame into a zero-delay
            // wait loop: just poll its state at a low cadence until visible.
            deadline = deadline.max(now + std::time::Duration::from_millis(100));
        }
        if deadline <= now {
            deadline = now + std::time::Duration::from_millis(1);
        }
        deadline = deadline.min(self.last_watch_poll + std::time::Duration::from_millis(600));
        deadline = deadline.min(self.last_vcs_poll + std::time::Duration::from_secs(2));
        deadline = deadline.min(self.last_busy_poll + std::time::Duration::from_secs(2));
        if let Some(at) = self.drop_deadline {
            deadline = deadline.min(at);
        }
        deadline = deadline.min(self.last_workspace_save + std::time::Duration::from_secs(30));
        if self.model.settings.editor.auto_save {
            deadline = deadline.min(self.last_autosave + std::time::Duration::from_secs(2));
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));

        if frame_pending && self.pacer.due(now) {
            self.wants_frame = false;
            if let Some(gfx) = &self.gfx {
                gfx.window.request_redraw();
            }
        } else if frame_pending {
            // The budget deadline above will wake us for the next frame.
            self.wants_frame = true;
        }
    }
}

/// Remove spool files of dead processes; each embeds its owner pid, and files
/// of live processes (other LazeCode windows) are kept.
fn cleanup_stale_cold_spools() {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(rest) = name.strip_prefix("lazecode-cold-") else {
            continue;
        };
        let Some(pid_text) = rest.split('-').next() else {
            continue;
        };
        let Ok(pid) = pid_text.parse::<i32>() else {
            continue;
        };
        if process_alive(pid) {
            continue;
        }
        let _ = std::fs::remove_file(entry.path());
    }
}

/// Signal 0 probes existence without side effects.
fn process_alive(pid: i32) -> bool {
    // SAFETY: `kill` with signal 0 performs a permission/existence check only.
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    cleanup_stale_cold_spools();
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
                "check-updates",
                pick(zh, "Check for Updates…", "检查更新…"),
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
            // Clipboard stays with egui-winit's Copy/Paste: predefined AppKit
            // items consume the key before the terminal/editor sees it.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_dot_git(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "lazecode-fingerprint-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".git/refs/heads")).unwrap();
        std::fs::create_dir_all(root.join(".git/objects/ab")).unwrap();
        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(root.join(".git/index"), "index").unwrap();
        std::fs::write(root.join(".git/refs/heads/main"), "aaa\n").unwrap();
        std::fs::write(root.join(".git/objects/ab/cd"), "blob").unwrap();
        root
    }

    #[test]
    fn fingerprint_tracks_refs_and_index_but_not_objects() {
        let root = fake_dot_git("track");
        let before = git_fingerprint(&root).expect("dir .git has a signature");
        assert_eq!(before.len(), 3, "HEAD + index + refs/heads/main");

        // A ref update is detected.
        std::fs::write(root.join(".git/refs/heads/main"), "bbb\n").unwrap();
        let after = git_fingerprint(&root).unwrap();
        assert_ne!(before, after);

        // Object-store churn is invisible to the signature.
        std::fs::write(root.join(".git/objects/ab/cd"), "blob2").unwrap();
        std::fs::write(root.join(".git/objects/ab/ef"), "blob3").unwrap();
        let objects_only = git_fingerprint(&root).unwrap();
        assert_eq!(after, objects_only);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn fingerprint_follows_linked_worktree_pointer_and_common_refs() {
        let root = fake_dot_git("linked");
        let worktree_git = root.join(".git/worktrees/feature");
        std::fs::create_dir_all(&worktree_git).unwrap();
        std::fs::write(worktree_git.join("commondir"), "../..\n").unwrap();
        std::fs::write(worktree_git.join("HEAD"), "ref: refs/heads/feature\n").unwrap();
        std::fs::write(worktree_git.join("index"), "index").unwrap();

        let linked = root.join("feature");
        std::fs::create_dir_all(&linked).unwrap();
        std::fs::write(
            linked.join(".git"),
            format!("gitdir: {}\n", worktree_git.display()),
        )
        .unwrap();

        let before = git_fingerprint(&linked).expect("linked worktree has a signature");
        assert!(before.iter().any(|(rel, _, _)| rel == "worktree/HEAD"));
        assert!(before
            .iter()
            .any(|(rel, _, _)| rel == "common/refs/heads/main"));

        let fp = root.join(".git/refs/heads/main");
        std::fs::write(&fp, "bbb\n").unwrap();
        let after = git_fingerprint(&linked).unwrap();
        assert_ne!(
            before, after,
            "shared refs must invalidate the linked checkout"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dropped_paths_open_files_and_projects_by_batch_type() {
        struct NoopNotifier;
        impl Notifier for NoopNotifier {
            fn notify(&self) {}
        }

        let root = std::env::temp_dir().join(format!("lazecode-drop-open-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("one")).unwrap();
        std::fs::create_dir_all(root.join("two")).unwrap();
        let file_a = root.join("a.txt");
        std::fs::write(&file_a, "a\n").unwrap();
        let file_b = root.join("b.txt");
        std::fs::write(&file_b, "b\n").unwrap();
        let project_one = root.join("one").canonicalize().unwrap();
        let project_two = root.join("two").canonicalize().unwrap();

        let model = AppModel {
            settings: Settings::default(),
            projects: Vec::new(),
            active_project: None,
            ui: Default::default(),
            toasts: Vec::new(),
            recent: Vec::new(),
            recent_files: Vec::new(),
            commands: Vec::new(),
            notifier: None,
            lsp: lazecode_core::LspManager::new(false, None),
            should_quit: false,
            version: "test",
            favorite_branches: std::collections::HashSet::new(),
            git_cmd_log: Vec::new(),
        };
        let mut app = LazeApp::new(model, JobQueue::new(Arc::new(NoopNotifier)));

        app.open_dropped_paths(vec![file_a.clone(), file_b.clone()]);
        let files = app.model.active_project().unwrap();
        assert!(files.is_single_file);
        assert_eq!(files.tabs.len(), 2);

        app.open_dropped_paths(vec![project_one.clone()]);
        assert!(!app.model.active_project().unwrap().is_single_file);
        assert_eq!(app.model.active_project().unwrap().root, project_one);

        app.open_dropped_paths(vec![project_two.clone()]);
        assert_eq!(app.model.active_project().unwrap().root, project_two);
        let project_count = app.model.projects.len();

        // Dropping an already-open project focuses it instead of duplicating.
        app.open_dropped_paths(vec![project_one.clone()]);
        assert_eq!(app.model.active_project().unwrap().root, project_one);
        assert_eq!(app.model.projects.len(), project_count);

        // Mixed batches are rejected before anything opens or switches.
        app.open_dropped_paths(vec![file_a, project_two]);
        assert_eq!(app.model.active_project().unwrap().root, project_one);
        assert_eq!(app.model.projects.len(), project_count);

        drop(app);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dropped_paths_classify_files_projects_and_mixed_batches() {
        let root =
            std::env::temp_dir().join(format!("lazecode-drop-classify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("project")).unwrap();
        let file = root.join("a.txt");
        std::fs::write(&file, "a\n").unwrap();
        let file_two = root.join("b.txt");
        std::fs::write(&file_two, "b\n").unwrap();
        let project = root.join("project");

        assert_eq!(
            classify_dropped_paths(vec![file.clone(), file.clone()]),
            DroppedPaths::Files(vec![file.clone()])
        );
        assert_eq!(
            classify_dropped_paths(vec![project.clone(), project.clone()]),
            DroppedPaths::Projects(vec![project.clone()])
        );
        assert_eq!(
            classify_dropped_paths(vec![file.clone(), project.clone()]),
            DroppedPaths::Mixed
        );
        assert_eq!(
            classify_dropped_paths(vec![root.join("missing")]),
            DroppedPaths::Empty
        );
        assert_eq!(
            classify_dropped_paths(vec![file_two.clone(), project.clone()]),
            DroppedPaths::Mixed
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn fingerprint_none_for_bad_pointer_or_missing_git() {
        let root = fake_dot_git("pointer");
        std::fs::remove_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git"), "gitdir: /elsewhere/.git\n").unwrap();
        assert_eq!(git_fingerprint(&root), None);

        let plain =
            std::env::temp_dir().join(format!("lazecode-fingerprint-{}-plain", std::process::id()));
        let _ = std::fs::remove_dir_all(&plain);
        std::fs::create_dir_all(&plain).unwrap();
        assert_eq!(git_fingerprint(&plain), None);
        let _ = std::fs::remove_dir_all(&plain);
    }
}
