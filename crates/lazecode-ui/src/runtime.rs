//! UI runtime state: per-frame view state, menu/dialog/search/terminal state
//! and the `UiRuntime` implementation.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

use lazecode_core::{AppModel, ProjectState};
use lazecode_terminal::{probe_busy_many, BusyTarget, Snapshot};

use crate::update_ui::UpdateState;

mod branch;
mod dialog;
mod log;
mod menu;
mod search;
mod terminal;

pub use branch::*;
pub use dialog::*;
pub use log::*;
pub use menu::*;
pub use search::*;
pub use terminal::*;
use terminal::{PendingCloseProbe, PendingQuitProbe};

/// Overlay state remembered after the previous frame's shortcut pass. Escape
/// is often consumed while an overlay is drawn (search, menus, dialogs);
/// comparing this snapshot keeps the shared handler from closing two layers
/// on one key press.
#[derive(Clone, Copy, Default)]
pub(crate) struct OverlaySnapshot {
    initialized: bool,
    palette: bool,
    update: bool,
    dialog: bool,
    menu: bool,
    agent_menu: bool,
    settings: bool,
    about: bool,
    search: bool,
    find: bool,
}

impl OverlaySnapshot {
    pub(crate) fn capture(model: &AppModel, rt: &UiRuntime) -> Self {
        Self {
            initialized: true,
            palette: model.ui.command_palette_open,
            update: rt.update_open,
            dialog: !matches!(rt.dialog, DialogState::None),
            menu: rt.menu.is_some() || rt.tree_submenu.is_some() || rt.branch_submenu.is_some(),
            agent_menu: rt.agent_menu.is_some(),
            settings: rt.settings_open,
            about: rt.about_open,
            search: rt.search.open,
            find: rt.find.open,
        }
    }

    /// True when an overlay that was open at the last shortcut pass has since
    /// closed itself (typically by handling Escape while being drawn).
    pub(crate) fn closed_since(&self, model: &AppModel, rt: &UiRuntime) -> bool {
        let previous = if self.initialized {
            *self
        } else {
            return false;
        };
        (previous.palette && !model.ui.command_palette_open)
            || (previous.update && !rt.update_open)
            || (previous.dialog && matches!(rt.dialog, DialogState::None))
            || (previous.menu
                && rt.menu.is_none()
                && rt.tree_submenu.is_none()
                && rt.branch_submenu.is_none())
            || (previous.agent_menu && rt.agent_menu.is_none())
            || (previous.settings && !rt.settings_open)
            || (previous.about && !rt.about_open)
            || (previous.search && !rt.search.open)
            || (previous.find && !rt.find.open)
    }

    /// Menus, dialogs, the palette and the updater dismiss themselves on
    /// Escape while being drawn. Search must leave the key to them instead of
    /// closing itself in the same frame.
    pub(crate) fn had_esc_overlay(&self) -> bool {
        self.initialized
            && (self.palette || self.update || self.dialog || self.menu || self.agent_menu)
    }
}

pub struct UiRuntime {
    pub(crate) lsp: LspUiState,
    pub(crate) palette: PaletteState,
    pub search: SearchState,
    pub(crate) find: FindState,
    pub menu: Option<MenuState>,
    pub dialog: DialogState,
    /// One-off close/quit `ps` probes running off-thread, drained by the
    /// `poll_*_probe` helpers.
    pending_close_probe: Option<PendingCloseProbe>,
    pending_quit_probe: Option<PendingQuitProbe>,
    pub terminal_snapshots: HashMap<u64, Snapshot>,
    pub terminal_selections: HashMap<u64, Option<TermSelection>>,
    pub terminal_focus: Option<u64>,
    pub(crate) terminal_search: HashMap<u64, TermSearchState>,
    pub(crate) terminal_scroll_fraction: HashMap<u64, f32>,
    pub terminal_preedit: HashMap<u64, String>,
    /// Fractional trackpad wheel remainder per terminal session.
    pub terminal_wheel: HashMap<u64, f32>,
    /// Reflow debounce per pane (a drag must not rewrap the scrollback per frame).
    pub(crate) terminal_resize: HashMap<u64, TermResizeGate>,
    /// Last snapshot wake counter per terminal; skips rebuilding unchanged grids.
    pub(crate) terminal_wake: HashMap<u64, u64>,
    /// egui textures for each pane's kitty graphics images.
    pub(crate) terminal_graphics: HashMap<u64, crate::terminal::GraphicsCache>,
    /// Per-pane process activity from the 2s background probe.
    pub terminal_busy: HashMap<u64, TerminalBusyState>,
    /// Filtered Git-log view + lane layout, reused until its inputs change.
    pub(crate) git_log_cache: Option<GitLogCache>,
    /// Set by the app shell: wakes the event loop when an off-thread close/
    /// quit probe finishes, so the confirmation dialog lands next frame
    /// instead of waiting for the next poll deadline.
    pub wake_notifier: Option<std::sync::Arc<dyn lazecode_terminal::Notifier>>,
    pub editor_focus: Option<u64>,
    pub(crate) editor_drag: bool,
    /// Vertical scroll offset of the project rail (0 = newest/top).
    pub(crate) project_rail_scroll: f32,
    /// Lazily uploaded titlebar copy of the packaged application icon.
    pub(crate) app_icon: Option<egui::TextureHandle>,
    /// Titlebar control rectangles in egui/window coordinates. The macOS
    /// native `zoom:` guard uses these to keep double-clicks on buttons from
    /// reaching AppKit's titlebar zoom.
    pub titlebar_controls: Vec<egui::Rect>,
    pub(crate) sidebar_drag: bool,
    pub(crate) panel_drag: bool,
    /// 分支比较页：pane 高度分隔条拖拽中（松手时落盘 settings）。
    /// Diff 页边缘气球：文案 + 出现时刻（1.8s 后消失）。
    pub(crate) diff_balloon: Option<(String, std::time::Instant)>,
    /// Diff 页导航后的滚动定位请求（tab id, 组序号）。
    pub diff_scroll: Option<(u64, usize)>,
    /// Diff 视图模式下拉打开中的 tab。
    pub(crate) diff_mode_open: Option<u64>,
    /// 分支比较页：每侧的 DAG 图开关与详情根行折叠（会话内瞬时态）。
    pub(crate) cb_show_graph: [bool; 2],
    pub(crate) cb_root_collapsed: [bool; 2],
    pub(crate) compare_split_drag: bool,
    /// 分支比较页：详情列宽度分隔条拖拽中。
    pub(crate) compare_details_drag: bool,
    pub settings_open: bool,
    /// Settings changed the UI/editor/terminal font: reinstall before the
    /// next frame so egui does not keep galleys from the old atlas.
    pub fonts_dirty: bool,
    pub about_open: bool,
    /// Software update window (settings → Updates / auto-check notice).
    pub update_open: bool,
    /// Update endpoint results, in-flight job and staging status.
    pub update: UpdateState,
    pub(crate) applied_theme: Option<(lazecode_core::ThemeMode, lazecode_core::ThemeSkin)>,
    pub(crate) last_frame: Instant,
    pub(crate) md_cache: egui_commonmark::CommonMarkCache,
    /// Per-tab rendered-Markdown scroll offset, in points.
    pub(crate) md_scroll: HashMap<u64, f32>,
    /// Bounded decoded-image cache for image tabs (not `egui_extras`' global
    /// loader cache, which never evicts).
    pub(crate) image_cache: crate::image_cache::ImageCache,
    /// Open image paths, used to drop textures as soon as a tab closes.
    pub(crate) media_project_roots: Vec<PathBuf>,
    pub(crate) media_markdown_paths: HashSet<PathBuf>,
    pub(crate) media_gif_paths: HashSet<PathBuf>,
    pub(crate) agent_menu: Option<egui::Rect>,
    pub(crate) agent_menu_frame: u64,
    /// Per-frame rects of open popovers, pushed by the overlay draw code and
    /// taken by the shell at the start of the next frame: egui routes the
    /// wheel by rect containment only (no z-order test), so panels need to
    /// know where an overlay drawn later in the frame will land.
    pub(crate) menu_hit_rects: Vec<egui::Rect>,
    pub(crate) frame_count: u64,
    pub(crate) branch_create_error: Option<String>,
    pub(crate) last_revealed_tab: Option<u64>,
    pub(crate) last_revealed_cmp_file: Option<String>,
    pub(crate) ime_preedit: String,
    /// True while a Chinese/Japanese/Korean IME owns an uncommitted
    /// composition. Enter must select the candidate, not confirm the dialog
    /// or insert a newline.
    pub(crate) ime_composing: bool,
    /// Whether the previous frame had a surface asking egui for IME input.
    /// A composition cannot outlive that focus: winit clears its marked text
    /// on `set_ime_allowed(false)` (and on input-source changes) without
    /// emitting `Preedit("")`/`Commit`, and egui-winit drops `Ime::Disabled`,
    /// so the filter itself must treat the next frame as composition-free.
    pub ime_focus_seen: bool,
    // --- 提交抽屉（稳定版 CommitDrawer）状态 ---
    /// Commit / Shelf 页签。
    pub commit_tab: CommitTab,
    /// 稳定版 ^P：Group by Directory（树视图）。
    pub(crate) commit_tree_view: bool,
    /// 稳定版：是否展示被 .gitignore 忽略的文件。
    pub(crate) commit_show_ignored: bool,
    /// Shelf 页签数据（页签激活时同步加载：读本地补丁文件，无子进程）。
    pub shelf_entries: Vec<lazecode_git::ShelfEntry>,
    pub shelf_deleted: Vec<lazecode_git::ShelfEntry>,
    /// 已加载的架藏数据所属仓库（切换项目后重新加载）。
    pub shelf_loaded: Option<std::path::PathBuf>,
    /// 架藏名 → 文件列表（加载时从补丁解析）。
    pub shelf_files_map: HashMap<String, Vec<String>>,
    pub shelf_selected: Option<String>,
    /// 选中的架藏文件（仓库相对路径，单架藏内多选）。
    pub shelf_file_sel: Vec<String>,
    // --- Stash 页签状态 ---
    /// 贮藏列表（抽屉可见时按仓库加载；决定页签是否显示）。
    pub stash_entries: Vec<lazecode_git::StashEntry>,
    /// 已加载贮藏数据所属仓库（切换项目或 git 变更后重新加载）。
    pub stash_loaded: Option<std::path::PathBuf>,
    /// 当前选中的贮藏，如 `stash@{0}`。
    pub stash_selected: Option<String>,
    /// 展开的贮藏条目（`stash@{n}`）。每个条目独立展开/折叠；首帧只展开
    /// 第一条，避免多条贮藏各自铺开整棵文件树。
    pub stash_expanded: std::collections::HashSet<String>,
    /// 折叠的树根/目录键（`<stash>:tracked:…` / `<stash>:untracked:…`；
    /// 默认展开）。
    pub stash_collapsed: std::collections::HashSet<String>,
    /// 选中的贮藏文件（仓库相对路径）。
    pub stash_file_sel: Option<String>,
    pub(crate) new_ignore_pattern: String,
    pub(crate) tree_submenu: Option<(String, egui::Rect)>,
    pub(crate) branch_submenu: Option<BranchSubmenuState>,
    pub(crate) branch_sections_collapsed: std::collections::HashSet<String>,
    /// 控制台侧栏两级分组的折叠键（`p:<项目根>` / `w:<worktree 路径>`；
    /// 默认为空，即全部展开）。
    pub(crate) consoles_collapsed: std::collections::HashSet<String>,
    /// Overlay state from the previous shortcut pass (Escape guard).
    pub(crate) overlay_snapshot: OverlaySnapshot,
    /// Frame in which an overlay already consumed Escape while being drawn.
    pub(crate) escape_handled_frame: u64,
    /// Compare filter cache, one entry per side.
    pub(crate) compare_filter_cache: [Option<crate::compare::CompareFilterCache>; 2],
    /// 设置页快捷键录入态：`Some(动作)` = 等待用户按下新组合键。
    pub keybind_capture: Option<crate::keybindings::Action>,
}
impl Default for UiRuntime {
    fn default() -> Self {
        Self {
            lsp: LspUiState::default(),
            palette: PaletteState::default(),
            search: SearchState::default(),
            find: FindState::default(),
            menu: None,
            dialog: DialogState::None,
            pending_close_probe: None,
            pending_quit_probe: None,
            overlay_snapshot: OverlaySnapshot::default(),
            escape_handled_frame: 0,
            compare_filter_cache: std::array::from_fn(|_| None),
            terminal_snapshots: HashMap::new(),
            terminal_selections: HashMap::new(),
            terminal_focus: None,
            terminal_search: HashMap::new(),
            terminal_scroll_fraction: HashMap::new(),
            terminal_preedit: HashMap::new(),
            terminal_wheel: HashMap::new(),
            terminal_resize: HashMap::new(),
            terminal_wake: HashMap::new(),
            terminal_graphics: HashMap::new(),
            terminal_busy: HashMap::new(),
            git_log_cache: None,
            wake_notifier: None,
            new_ignore_pattern: String::new(),
            editor_focus: None,
            editor_drag: false,
            project_rail_scroll: 0.0,
            app_icon: None,
            titlebar_controls: Vec::new(),
            sidebar_drag: false,
            panel_drag: false,
            diff_balloon: None,
            diff_scroll: None,
            diff_mode_open: None,
            cb_show_graph: [true, true],
            cb_root_collapsed: [false, false],
            compare_split_drag: false,
            compare_details_drag: false,
            settings_open: false,
            fonts_dirty: false,
            about_open: false,
            update_open: false,
            update: UpdateState::default(),
            applied_theme: None,
            last_frame: Instant::now(),
            md_cache: egui_commonmark::CommonMarkCache::default(),
            md_scroll: HashMap::new(),
            image_cache: crate::image_cache::ImageCache::default(),
            media_project_roots: Vec::new(),
            media_markdown_paths: HashSet::new(),
            media_gif_paths: HashSet::new(),
            agent_menu: None,
            agent_menu_frame: 0,
            menu_hit_rects: Vec::new(),
            frame_count: 0,
            branch_create_error: None,
            last_revealed_tab: None,
            last_revealed_cmp_file: None,
            ime_preedit: String::new(),
            ime_composing: false,
            ime_focus_seen: false,
            commit_tab: CommitTab::Commit,
            commit_tree_view: false,
            commit_show_ignored: false,
            shelf_entries: Vec::new(),
            shelf_deleted: Vec::new(),
            shelf_loaded: None,
            shelf_files_map: HashMap::new(),
            shelf_selected: None,
            shelf_file_sel: Vec::new(),
            stash_entries: Vec::new(),
            stash_loaded: None,
            stash_selected: None,
            stash_expanded: std::collections::HashSet::new(),
            stash_collapsed: std::collections::HashSet::new(),
            stash_file_sel: None,
            tree_submenu: None,
            branch_submenu: None,
            branch_sections_collapsed: std::collections::HashSet::new(),
            consoles_collapsed: std::collections::HashSet::new(),
            keybind_capture: None,
        }
    }
}

impl UiRuntime {
    /// Chinese/Japanese/Korean IMEs can report the Enter that leaves a
    /// composition as a plain key event without an `ImeEvent::Commit` (macOS
    /// Shuangpin does). Every text surface treats Enter as "confirm", so when
    /// a composition is active: commit the preedit text into the focused
    /// field, drop the key, and let the next Enter confirm the dialog.
    ///
    /// If the IME does send its own `Commit` in the same frame, keep that text
    /// untouched. Either way the Enter that ends composition is suppressed;
    /// the next Enter confirms normally.
    pub fn filter_ime_enter(&mut self, ui: &egui::Ui) {
        // Nothing asked for IME input last frame, so any composition state is
        // stale: winit cleared its marked text without telling egui (see
        // `ime_focus_seen`). Dropping it here is what keeps a later Enter from
        // committing a preedit that no longer exists on screen.
        if !self.ime_focus_seen {
            self.ime_composing = false;
            self.ime_preedit.clear();
        }
        let mut composing = self.ime_composing;
        let mut preedit = self.ime_preedit.clone();
        let mut ime_activity = false;
        let mut committed = false;
        let pointer_pressed = ui.input(|input| input.pointer.any_pressed());
        ui.input(|input| {
            for event in &input.events {
                match event {
                    egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) => {
                        preedit = text.clone();
                        composing = !text.is_empty();
                        ime_activity = true;
                    }
                    egui::Event::Ime(egui::ImeEvent::Commit(_)) => {
                        preedit.clear();
                        composing = false;
                        ime_activity = true;
                        committed = true;
                    }
                    egui::Event::Key {
                        key: egui::Key::Escape,
                        pressed: true,
                        ..
                    } => {
                        preedit.clear();
                        composing = false;
                    }
                    _ => {}
                }
            }
        });
        if pointer_pressed {
            preedit.clear();
            composing = false;
        }
        let enter_pressed = ui.input(|input| input.key_pressed(egui::Key::Enter));
        if enter_pressed && composing {
            // macOS/egui-winit reports the Enter that leaves a composition as
            // a plain key event, without an ImeEvent::Commit. Synthesize the
            // commit so the preedit text lands in the field, then drop the
            // key so it cannot also press the dialog's confirm button.
            if !committed && !preedit.is_empty() {
                // Chinese IME preedits separate syllables with spaces (for
                // example `test c c d`). A direct Enter commit is the user's
                // literal text, so those separators must not leak into file
                // names or editor content.
                let text = preedit.replace(' ', "");
                ui.input_mut(|input| {
                    input
                        .events
                        .push(egui::Event::Ime(egui::ImeEvent::Commit(text)));
                });
            }
            preedit.clear();
            composing = false;
            ime_activity = true;
        }
        if composing || ime_activity {
            ui.input_mut(|input| {
                input.events.retain(|event| {
                    !matches!(
                        event,
                        egui::Event::Key {
                            key: egui::Key::Enter,
                            pressed: true,
                            ..
                        }
                    )
                });
            });
        }
        self.ime_preedit = preedit;
        self.ime_composing = composing;
    }

    /// Decode the packaged application icon once and reuse it in the
    /// titlebar and the empty-workspace page.
    pub fn app_icon_texture(&mut self, ctx: &egui::Context) -> egui::TextureId {
        self.app_icon
            .get_or_insert_with(|| {
                let bytes = include_bytes!("../../../packaging/macos/icon-1024.png");
                let decoded = image::load_from_memory(bytes)
                    .expect("embedded LazeCode application icon must be a valid PNG")
                    .to_rgba8();
                let resized = image::imageops::resize(
                    &decoded,
                    64,
                    64,
                    image::imageops::FilterType::Lanczos3,
                );
                let pixels = egui::ColorImage::from_rgba_unmultiplied([64, 64], resized.as_raw());
                ctx.load_texture("lazecode-app-icon", pixels, egui::TextureOptions::LINEAR)
            })
            .id()
    }

    /// True while a full-screen surface or popover owns input. Used by the
    /// shell to place a full-screen hit-test blocker and by the editor and
    /// terminal to keep keystrokes out of the document behind it.
    pub fn blocks_input(&self, model: &AppModel) -> bool {
        self.settings_open
            || self.about_open
            || self.search.open
            || model.ui.command_palette_open
            || self.menu.is_some()
            || self.agent_menu.is_some()
            || self.update_open
            || !matches!(self.dialog, DialogState::None)
    }

    /// True when any overlay *other than the search panel* owns input. Search
    /// itself must not suppress its own query field's keys.
    pub(crate) fn other_overlay_open(&self, model: &AppModel) -> bool {
        self.settings_open
            || self.about_open
            || model.ui.command_palette_open
            || self.menu.is_some()
            || self.tree_submenu.is_some()
            || self.branch_submenu.is_some()
            || self.agent_menu.is_some()
            || self.update_open
            || self.find.open
            || !matches!(self.dialog, DialogState::None)
    }

    /// Hand this frame's popover rects to the shell and start a fresh buffer.
    pub fn take_menu_hit_rects(&mut self) -> Vec<egui::Rect> {
        std::mem::take(&mut self.menu_hit_rects)
    }

    /// True when scroll-wheel input this frame belongs to an overlay instead
    /// of the panels underneath: modals own the whole window, popover menus
    /// own their (previous frame's) rects. egui delivers the wheel to every
    /// scroll surface containing the pointer — first consumer wins, and
    /// panels always draw before overlays — so the shell must intercept the
    /// delta before the panels run.
    pub fn blocks_wheel(
        &self,
        model: &AppModel,
        pos: Option<egui::Pos2>,
        menu_rects: &[egui::Rect],
    ) -> bool {
        self.settings_open
            || self.about_open
            || self.search.open
            || model.ui.command_palette_open
            || self.update_open
            || !matches!(self.dialog, DialogState::None)
            || pos.is_some_and(|pos| menu_rects.iter().any(|rect| rect.contains(pos)))
    }

    /// Close the topmost overlay. Used by Escape and by the native Cmd+W
    /// menu accelerator while a modal (settings, dialog, update, menu) owns
    /// input; it must never close the editor tab underneath.
    pub fn close_overlay(&mut self, model: &mut AppModel) {
        if model.ui.command_palette_open {
            model.ui.command_palette_open = false;
            self.palette.focus_requested = false;
        } else if self.update_open {
            // Checking may be dismissed; its worker keeps running and a found
            // release reappears as the title-bar badge. Downloading and
            // installing must stay visible until they finish.
            if !matches!(
                self.update.view(&model.settings),
                crate::update_ui::UpdateView::Downloading { .. }
                    | crate::update_ui::UpdateView::Installing
            ) {
                self.update_open = false;
            }
        } else if !matches!(self.dialog, DialogState::None) {
            self.dialog = DialogState::None;
        } else if self.menu.is_some()
            || self.tree_submenu.is_some()
            || self.branch_submenu.is_some()
        {
            self.menu = None;
            self.tree_submenu = None;
            self.branch_submenu = None;
        } else if self.agent_menu.is_some() {
            self.agent_menu = None;
        } else if self.settings_open {
            self.settings_open = false;
        } else if self.about_open {
            self.about_open = false;
        } else if self.search.open {
            self.search.open = false;
        } else if self.find.open {
            self.find.open = false;
        }
    }

    pub fn open_menu(&mut self, kind: MenuKind, anchor: egui::Rect, payload: Option<String>) {
        self.tree_submenu = None;
        self.branch_submenu = None;
        self.menu = Some(MenuState {
            kind,
            anchor,
            payload,
            opened_frame: self.frame_count,
        });
    }

    pub fn close_menu(&mut self) {
        self.menu = None;
        self.tree_submenu = None;
        self.branch_submenu = None;
    }
    pub fn toggle_menu(&mut self, kind: MenuKind, anchor: egui::Rect, payload: Option<String>) {
        if matches!(&self.menu, Some(m) if m.kind == kind) {
            self.menu = None;
        } else {
            self.open_menu(kind, anchor, payload);
        }
    }

    pub fn open_agent_menu(&mut self, anchor: egui::Rect) {
        self.agent_menu = Some(anchor);
        self.agent_menu_frame = self.frame_count;
    }

    pub fn toggle_agent_menu(&mut self, anchor: egui::Rect) {
        if self.agent_menu.is_some() {
            self.agent_menu = None;
        } else {
            self.open_agent_menu(anchor);
        }
    }

    pub fn set_editor_focus(&mut self, id: u64) {
        self.editor_focus = Some(id);
        self.terminal_focus = None;
    }

    pub fn set_terminal_focus(&mut self, id: u64) {
        self.terminal_focus = Some(id);
        self.editor_focus = None;
    }

    /// True when a text surface (console or editor) owns the keyboard: no
    /// overlay/text field is taking keys, and the focused surface is the
    /// active pane/tab of the active project.
    pub fn typing_surface_owns_keys(&self, model: &AppModel, ctx: &egui::Context) -> bool {
        if self.blocks_input(model) || ctx.text_edit_focused() {
            return false;
        }
        let Some(project) = model.active_project() else {
            return false;
        };
        if let Some(pane_id) = self.terminal_focus {
            if !model.ui.terminal_visible || self.terminal_search.values().any(|search| search.open)
            {
                return false;
            }
            let worktree = project.active_worktree();
            return worktree
                .active_tab_id()
                .and_then(|tab_id| worktree.tab_by_id(tab_id))
                .is_some_and(|tab| tab.active_pane == pane_id);
        }
        if let Some(tab_id) = self.editor_focus {
            return project.active_tab == Some(tab_id)
                && project
                    .tab_by_id(tab_id)
                    .is_some_and(|tab| matches!(tab.kind, lazecode_core::TabKind::File(_)));
        }
        false
    }

    pub fn open_palette(&mut self) {
        self.palette.query.clear();
        self.palette.selected = 0;
        self.palette.focus_requested = true;
        self.palette.opened_frame = self.frame_count;
        self.menu = None;
        self.agent_menu = None;
        self.settings_open = false;
        self.about_open = false;
        self.search.open = false;
        self.find.open = false;
        self.dialog = DialogState::None;
    }

    pub fn close_overlays(&mut self) {
        self.menu = None;
        self.agent_menu = None;
        self.dialog = DialogState::None;
        self.search.open = false;
        self.settings_open = false;
        self.about_open = false;
    }

    /// Drop decoded images for closed image tabs and flush `egui_extras`
    /// caches for Markdown previews when their project/tab set changes.
    pub fn prune_media_caches(&mut self, ctx: &egui::Context, model: &AppModel) {
        use lazecode_core::TabKind;

        let mut open_images = HashSet::new();
        let mut gifs = HashSet::new();
        let mut markdown = HashSet::new();
        let mut roots = Vec::with_capacity(model.projects.len());
        for project in &model.projects {
            roots.push(project.root.clone());
            for worktree in project.worktrees.iter().skip(1) {
                roots.push(worktree.path.clone());
            }
            for tab in &project.tabs {
                match &tab.kind {
                    TabKind::Image(image) => {
                        open_images.insert(image.path.clone());
                        if image
                            .path
                            .extension()
                            .and_then(|ext| ext.to_str())
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("gif"))
                        {
                            gifs.insert(image.path.clone());
                        }
                    }
                    TabKind::File(file)
                        if file
                            .doc
                            .path
                            .extension()
                            .and_then(|ext| ext.to_str())
                            .is_some_and(|ext| {
                                ext.eq_ignore_ascii_case("md")
                                    || ext.eq_ignore_ascii_case("markdown")
                            }) =>
                    {
                        markdown.insert(file.doc.path.clone());
                    }
                    _ => {}
                }
            }
        }

        self.image_cache.retain_paths(&open_images);

        if roots != self.media_project_roots
            || markdown != self.media_markdown_paths
            || gifs != self.media_gif_paths
        {
            // Flush the markdown/image caches on project or Markdown-tab
            // changes; per-frame flushing would reload visible images.
            ctx.forget_all_images();
            self.md_cache = egui_commonmark::CommonMarkCache::default();
            self.media_project_roots = roots;
            self.media_markdown_paths = markdown;
            self.media_gif_paths = gifs;
        }
    }

    /// Aggregate live process activity for one project.
    pub fn project_busy(&self, project: &ProjectState) -> TerminalBusyState {
        let mut out = TerminalBusyState::default();
        for pane in project
            .worktrees
            .iter()
            .flat_map(|worktree| worktree.terminals.iter())
            .flat_map(|tab| tab.panes.iter())
        {
            let Some(state) = self.terminal_busy.get(&pane.id) else {
                continue;
            };
            if !state.busy {
                continue;
            }
            out.busy = true;
            out.outputting |= state.outputting;
            for command in &state.commands {
                if !out.commands.iter().any(|existing| existing == command) {
                    out.commands.push(command.clone());
                }
            }
        }
        out
    }

    pub fn busy_count(&self) -> usize {
        self.terminal_busy
            .values()
            .filter(|state| state.busy)
            .count()
    }

    pub fn busy_commands_for(&self, pane_ids: &[u64]) -> Vec<String> {
        let mut commands = Vec::new();
        for id in pane_ids {
            let Some(state) = self.terminal_busy.get(id) else {
                continue;
            };
            if !state.busy {
                continue;
            }
            for command in &state.commands {
                if !commands.iter().any(|existing| existing == command) {
                    commands.push(command.clone());
                }
            }
        }
        commands
    }

    /// Probe panes off-thread, bypassing the 2s cache (used on close/quit).
    fn spawn_busy_probe(
        &self,
        model: &AppModel,
        pane_ids: &[u64],
    ) -> std::sync::mpsc::Receiver<Vec<(u64, TerminalBusyState)>> {
        struct ProbeInput {
            id: u64,
            target: BusyTarget,
            output_ms: Option<u64>,
            input_ms: Option<u64>,
            resize_ms: Option<u64>,
            was_outputting: bool,
        }
        let mut rows: Vec<ProbeInput> = Vec::new();
        for project in &model.projects {
            for pane in project
                .worktrees
                .iter()
                .flat_map(|worktree| worktree.terminals.iter())
                .flat_map(|tab| tab.panes.iter())
            {
                if !pane_ids.contains(&pane.id) {
                    continue;
                }
                let was_outputting = self
                    .terminal_busy
                    .get(&pane.id)
                    .is_some_and(|state| state.outputting);
                rows.push(ProbeInput {
                    id: pane.id,
                    target: pane.session.busy_target(),
                    output_ms: pane.session.last_output_ms(),
                    input_ms: pane.session.last_input_ms(),
                    resize_ms: pane.session.last_resize_ms(),
                    was_outputting,
                });
            }
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let wake = self.wake_notifier.clone();
        std::thread::Builder::new()
            .name("lazecode-close-probe".to_string())
            .spawn(move || {
                let targets: Vec<BusyTarget> = rows.iter().map(|row| row.target.clone()).collect();
                let infos = probe_busy_many(&targets);
                let results: Vec<(u64, TerminalBusyState)> = rows
                    .into_iter()
                    .zip(infos)
                    .map(|(row, info)| {
                        (
                            row.id,
                            TerminalBusyState {
                                busy: info.busy,
                                outputting: console_outputting(
                                    info.busy,
                                    row.output_ms,
                                    row.input_ms,
                                    row.resize_ms,
                                    row.was_outputting,
                                ),
                                commands: info.commands,
                                last_output_ms: row.output_ms,
                            },
                        )
                    })
                    .collect();
                let _ = tx.send(results);
                if let Some(notifier) = &wake {
                    notifier.notify();
                }
            })
            .ok();
        rx
    }

    fn apply_busy_results(&mut self, results: Vec<(u64, TerminalBusyState)>) {
        for (id, state) in results {
            self.terminal_busy.insert(id, state);
        }
    }

    /// Ask before killing panes when a busy command is running. The probe
    /// runs off-thread; `poll_pending_close_probe` applies the outcome.
    pub fn request_terminal_close(&mut self, model: &mut AppModel, action: TerminalCloseAction) {
        let pane_ids = self.terminal_close_panes(model, &action);
        if pane_ids.is_empty() {
            self.apply_terminal_close(model, action);
            return;
        }
        let rx = self.spawn_busy_probe(model, &pane_ids);
        self.pending_close_probe = Some(PendingCloseProbe {
            action,
            pane_ids,
            rx,
        });
    }

    /// Drain a finished close probe (if any) and apply its decision: close
    /// the panes when idle, otherwise show the confirmation dialog. Returns
    /// true when the UI should redraw.
    pub fn poll_pending_close_probe(&mut self, model: &mut AppModel) -> bool {
        let Some(pending) = self.pending_close_probe.as_mut() else {
            return false;
        };
        let results = match pending.rx.try_recv() {
            Ok(results) => results,
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Vec::new(),
        };
        let pane_ids = pending.pane_ids.clone();
        let action = pending.action.clone();
        self.pending_close_probe = None;
        self.apply_busy_results(results);
        let commands = self.busy_commands_for(&pane_ids);
        if commands.is_empty() {
            self.apply_terminal_close(model, action);
        } else {
            self.dialog = DialogState::ConfirmTerminalClose { action, commands };
        }
        true
    }

    pub fn apply_terminal_close(&mut self, model: &mut AppModel, action: TerminalCloseAction) {
        match action {
            TerminalCloseAction::Pane(pane_id) => {
                let Some((project_index, _, _)) =
                    model
                        .projects
                        .iter()
                        .enumerate()
                        .find_map(|(index, project)| {
                            project
                                .pane_owner(pane_id)
                                .map(|owner| (index, owner.0, owner.1))
                        })
                else {
                    return;
                };
                let project = &mut model.projects[project_index];
                project.close_terminal(pane_id);
                self.forget_terminal(pane_id);
                if model.active_project == Some(project_index) {
                    match project.active_worktree().active_terminal {
                        Some(next) => self.set_terminal_focus(next),
                        None => self.terminal_focus = None,
                    }
                }
            }
            TerminalCloseAction::Tab(tab_id) => {
                let Some(project_index) = model.projects.iter().position(|project| {
                    project
                        .worktrees
                        .iter()
                        .any(|worktree| worktree.tab_by_id(tab_id).is_some())
                }) else {
                    return;
                };
                let panes: Vec<u64> = model.projects[project_index]
                    .worktrees
                    .iter()
                    .find_map(|worktree| worktree.tab_by_id(tab_id))
                    .map(|tab| tab.panes.iter().map(|pane| pane.id).collect())
                    .unwrap_or_default();
                let project = &mut model.projects[project_index];
                project.close_terminal_tab(tab_id);
                for id in panes {
                    self.forget_terminal(id);
                }
                if model.active_project == Some(project_index) {
                    match project.active_worktree().active_terminal {
                        Some(next) => self.set_terminal_focus(next),
                        None => self.terminal_focus = None,
                    }
                }
            }
            TerminalCloseAction::Others(keep_pane) => {
                let Some(project_index) = model
                    .projects
                    .iter()
                    .position(|project| project.term_by_id(keep_pane).is_some())
                else {
                    return;
                };
                let project = &mut model.projects[project_index];
                let keep_tab = project
                    .active_worktree()
                    .tab_of_pane(keep_pane)
                    .map(|tab| tab.id);
                let doomed: Vec<u64> = project
                    .active_worktree()
                    .terminals
                    .iter()
                    .filter(|tab| Some(tab.id) != keep_tab)
                    .flat_map(|tab| tab.panes.iter().map(|pane| pane.id))
                    .collect();
                project.close_other_terminals(keep_pane);
                for id in doomed {
                    self.forget_terminal(id);
                }
                self.set_terminal_focus(keep_pane);
            }
            TerminalCloseAction::All => {
                let Some(project_index) = model.active_project else {
                    return;
                };
                let project = &mut model.projects[project_index];
                let doomed: Vec<u64> = project
                    .active_worktree()
                    .terminals
                    .iter()
                    .flat_map(|tab| tab.panes.iter().map(|pane| pane.id))
                    .collect();
                project.close_all_terminals();
                for id in doomed {
                    self.forget_terminal(id);
                }
                self.terminal_focus = None;
            }
            TerminalCloseAction::Project(index) => {
                if index >= model.projects.len() {
                    return;
                }
                let active = model.active_project == Some(index);
                // Snapshots and probes are keyed by pane id; forget the closed
                // project's panes instead of clearing every other project's
                // last complete frame.
                let doomed: Vec<u64> = model.projects[index]
                    .worktrees
                    .iter()
                    .flat_map(|worktree| worktree.terminals.iter())
                    .flat_map(|tab| tab.panes.iter().map(|pane| pane.id))
                    .collect();
                model.close_project(index);
                for id in doomed {
                    self.forget_terminal(id);
                }
                model.save_workspace();
                self.clear_terminal_cache();
                if active {
                    self.terminal_focus = None;
                }
            }
        }
    }

    fn terminal_close_panes(&self, model: &AppModel, action: &TerminalCloseAction) -> Vec<u64> {
        match action {
            TerminalCloseAction::Pane(id) => vec![*id],
            TerminalCloseAction::Tab(tab_id) => model
                .projects
                .iter()
                .flat_map(|project| project.worktrees.iter())
                .find_map(|worktree| worktree.tab_by_id(*tab_id))
                .map(|tab| tab.panes.iter().map(|pane| pane.id).collect())
                .unwrap_or_default(),
            TerminalCloseAction::Others(keep) => model
                .projects
                .iter()
                .find(|project| project.term_by_id(*keep).is_some())
                .and_then(|project| {
                    project
                        .active_worktree()
                        .tab_of_pane(*keep)
                        .map(|tab| tab.id)
                })
                .map(|keep_tab| {
                    model
                        .projects
                        .iter()
                        .find(|project| project.term_by_id(*keep).is_some())
                        .into_iter()
                        .flat_map(|project| project.active_worktree().terminals.iter())
                        .filter(|tab| tab.id != keep_tab)
                        .flat_map(|tab| tab.panes.iter().map(|pane| pane.id))
                        .collect()
                })
                .unwrap_or_default(),
            TerminalCloseAction::All => model
                .active_project()
                .map(|project| {
                    project
                        .active_worktree()
                        .terminals
                        .iter()
                        .flat_map(|tab| tab.panes.iter().map(|pane| pane.id))
                        .collect()
                })
                .unwrap_or_default(),
            TerminalCloseAction::Project(index) => model
                .projects
                .get(*index)
                .map(|project| {
                    project
                        .worktrees
                        .iter()
                        .flat_map(|worktree| worktree.terminals.iter())
                        .flat_map(|tab| tab.panes.iter().map(|pane| pane.id))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// Begin the async quit guard: probe every console off-thread, then
    /// `poll_quit_probe` either shows the Quit dialog (unsaved tabs, running
    /// commands) or sets `should_quit`.
    pub fn request_quit_probe(&mut self, model: &AppModel) {
        let pane_ids: Vec<u64> = model
            .projects
            .iter()
            .flat_map(|project| project.worktrees.iter())
            .flat_map(|worktree| worktree.terminals.iter())
            .flat_map(|tab| tab.panes.iter().map(|pane| pane.id))
            .collect();
        let rx = self.spawn_busy_probe(model, &pane_ids);
        self.pending_quit_probe = Some(PendingQuitProbe { pane_ids, rx });
    }

    /// Drain a finished quit probe and apply its decision. Returns true when
    /// the UI should redraw.
    pub fn poll_quit_probe(&mut self, model: &mut AppModel) -> bool {
        let Some(pending) = self.pending_quit_probe.as_mut() else {
            return false;
        };
        let results = match pending.rx.try_recv() {
            Ok(results) => results,
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Vec::new(),
        };
        let pane_ids = pending.pane_ids.clone();
        self.pending_quit_probe = None;
        self.apply_busy_results(results);
        let commands = self.busy_commands_for(&pane_ids);
        let unsaved = model
            .projects
            .iter()
            .any(|project| project.tabs.iter().any(|tab| tab.is_dirty()));
        if unsaved || !commands.is_empty() {
            self.dialog = DialogState::Quit {
                commands,
                restart_update: false,
            };
        } else {
            model.should_quit = true;
        }
        true
    }

    pub fn clear_terminal_cache(&mut self) {
        // Drop a stale focus id from another project or its console looks
        // dead until clicked; the panel latches the new active session.
        // Snapshot cells are per pane and are the last complete frames: keep
        // them so the first draw after a switch can hold a coherent screen
        // while a TUI finishes its resize repaint. `forget_terminal` frees
        // them when the pane closes.
        self.terminal_selections.clear();
        self.terminal_search.clear();
        self.terminal_preedit.clear();
        self.terminal_wheel.clear();
        self.terminal_graphics.clear();
        self.terminal_resize.clear();
        // Keep `terminal_busy`: switching projects must not make a running
        // green ring disappear until the next 2s probe. Entries are keyed by
        // globally unique pane ids and `forget_terminal` removes closed ones.
        self.terminal_focus = None;
    }

    /// Drop cached rendering state for one closed pane (its PTY is gone).
    pub fn forget_terminal(&mut self, id: u64) {
        self.terminal_snapshots.remove(&id);
        self.terminal_selections.remove(&id);
        self.terminal_search.remove(&id);
        self.terminal_preedit.remove(&id);
        self.terminal_wheel.remove(&id);
        self.terminal_wake.remove(&id);
        self.terminal_graphics.remove(&id);
        self.terminal_busy.remove(&id);
        self.terminal_scroll_fraction.remove(&id);
        self.terminal_resize.remove(&id);
        if self.terminal_focus == Some(id) {
            self.terminal_focus = None;
        }
    }

    /// Drop per-pane state for panes the model no longer owns. The core can
    /// close panes on its own (`settle_single_files_workspace` drops an
    /// emptied Single files project outright), so call-site cleanup is not
    /// exhaustive; one retain per frame is cheap and idempotent, and keeps
    /// kitty texture caches from outliving their panes.
    pub fn prune_terminal_caches(&mut self, model: &AppModel) {
        let mut live: Vec<u64> = Vec::new();
        for project in &model.projects {
            for worktree in &project.worktrees {
                for tab in &worktree.terminals {
                    live.extend(tab.panes.iter().map(|pane| pane.id));
                }
            }
        }
        let alive = |id: &u64| live.contains(id);
        self.terminal_snapshots.retain(|id, _| alive(id));
        self.terminal_selections.retain(|id, _| alive(id));
        self.terminal_search.retain(|id, _| alive(id));
        self.terminal_preedit.retain(|id, _| alive(id));
        self.terminal_wheel.retain(|id, _| alive(id));
        self.terminal_wake.retain(|id, _| alive(id));
        self.terminal_graphics.retain(|id, _| alive(id));
        self.terminal_busy.retain(|id, _| alive(id));
        self.terminal_scroll_fraction.retain(|id, _| alive(id));
        self.terminal_resize.retain(|id, _| alive(id));
        if self.terminal_focus.is_some_and(|id| !alive(&id)) {
            self.terminal_focus = None;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_output_lights_marquee() {
        assert!(console_outputting(true, Some(100), None, None, false));
    }

    #[test]
    fn input_echo_does_not_light_marquee() {
        assert!(!console_outputting(true, Some(100), Some(180), None, false));
        assert!(console_outputting(true, Some(100), Some(350), None, false));
    }

    #[test]
    fn resize_repaint_does_not_light_marquee() {
        assert!(!console_outputting(true, Some(100), None, Some(300), false));
        assert!(console_outputting(true, Some(100), None, Some(700), false));
    }

    #[test]
    fn resize_keeps_an_already_hot_marquee() {
        assert!(console_outputting(true, Some(100), None, Some(300), true));
    }

    #[test]
    fn stale_output_or_idle_console_stays_dark() {
        assert!(!console_outputting(true, Some(2_500), None, None, false));
        assert!(!console_outputting(false, Some(100), None, None, false));
    }

    fn enter_event() -> egui::Event {
        egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn preedit_event(text: &str) -> egui::Event {
        egui::Event::Ime(egui::ImeEvent::Preedit {
            text: text.to_string(),
            active_range_chars: None,
        })
    }

    fn filter_frame(rt: &mut UiRuntime, events: Vec<egui::Event>) -> (bool, Option<String>) {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::Vec2::new(800.0, 600.0),
            )),
            events,
            ..Default::default()
        };
        let mut enter = false;
        let mut commit = None;
        let mut output = ctx.run_ui(input, |ui| {
            rt.filter_ime_enter(ui);
            enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
            commit = ui.input(|input| {
                input.events.iter().find_map(|event| match event {
                    egui::Event::Ime(egui::ImeEvent::Commit(text)) => Some(text.clone()),
                    _ => None,
                })
            });
        });
        output.textures_delta.clear();
        (enter, commit)
    }

    #[test]
    fn ime_preedit_enter_commits_instead_of_confirming() {
        let mut rt = UiRuntime::default();
        let (enter, commit) = filter_frame(&mut rt, vec![preedit_event("ce"), enter_event()]);
        assert!(!enter, "Enter must not confirm the dialog");
        assert_eq!(
            commit.as_deref(),
            Some("ce"),
            "Enter must commit the preedit text"
        );
        let (enter, _) = filter_frame(&mut rt, vec![enter_event()]);
        assert!(enter, "the next Enter confirms normally");
    }

    #[test]
    fn ime_direct_enter_strips_preedit_separators() {
        let mut rt = UiRuntime::default();
        let (enter, commit) =
            filter_frame(&mut rt, vec![preedit_event("test c c d"), enter_event()]);
        assert!(!enter, "Enter must not confirm the dialog");
        assert_eq!(
            commit.as_deref(),
            Some("testccd"),
            "IME syllable separators must not reach the text field"
        );
    }

    #[test]
    fn ime_commit_enter_is_not_a_confirm() {
        let mut rt = UiRuntime::default();
        let (enter, commit) = filter_frame(
            &mut rt,
            vec![
                preedit_event("ce"),
                egui::Event::Ime(egui::ImeEvent::Commit("测".into())),
                enter_event(),
            ],
        );
        assert!(
            !enter,
            "the Enter that commits a candidate must not confirm"
        );
        assert_eq!(
            commit.as_deref(),
            Some("测"),
            "the IME's own commit must not be replaced"
        );
    }

    #[test]
    fn plain_enter_still_confirms() {
        let mut rt = UiRuntime::default();
        let (enter, _) = filter_frame(&mut rt, vec![enter_event()]);
        assert!(enter);
    }

    #[test]
    fn losing_ime_focus_drops_a_stale_composition() {
        // Frame N: a text field holds IME focus and the IME reports a preedit.
        let mut rt = UiRuntime {
            ime_focus_seen: true,
            ..UiRuntime::default()
        };
        let (enter, _) = filter_frame(&mut rt, vec![preedit_event("ce")]);
        assert!(!enter);
        assert!(rt.ime_composing);

        // Frame N+1: the focused surface is gone. winit cleared its marked
        // text without an event egui can see, so the Enter must be a real
        // confirm and must not commit the stale preedit.
        rt.ime_focus_seen = false;
        let (enter, commit) = filter_frame(&mut rt, vec![enter_event()]);
        assert!(enter, "a stale preedit must not swallow the next Enter");
        assert_eq!(commit, None, "a stale preedit must not be committed");
        assert!(!rt.ime_composing);
        assert!(rt.ime_preedit.is_empty());
    }
}
