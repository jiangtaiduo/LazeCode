//! 全局键盘快捷键：所有应用级键位经 `keybindings::resolved` 查表分发
//! （编译期默认键 + `settings.toml` 覆盖），设置页录入态优先于正常分发。

use egui::{InputState, Key, Ui};
use lazecode_core::{AppModel, Command};

use crate::keybindings::{self, Action};
use crate::runtime::{DialogState, OverlaySnapshot, UiRuntime};

/// 命中 `action` 当前绑定的组合键（含用户覆盖）。
fn consumed(input: &mut InputState, model: &AppModel, action: Action) -> bool {
    keybindings::resolved(model, action)
        .is_some_and(|binding| input.consume_key(binding.modifiers, binding.key))
}

pub fn handle(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime) {
    let ctx = ui.ctx();

    if rt.keybind_capture.is_some() {
        keybindings::capture_tick(ctx, model, rt);
        let snapshot = OverlaySnapshot::capture(model, rt);
        rt.overlay_snapshot = snapshot;
        return;
    }

    let blocked = rt.blocks_input(model);
    let active_json_tab = model.active_project.and_then(|index| {
        let tab_id = model.projects[index].active_tab?;
        if rt.editor_focus != Some(tab_id) {
            return None;
        }
        let is_json = model.projects[index]
            .tab_by_id(tab_id)
            .is_some_and(|tab| match &tab.kind {
                lazecode_core::TabKind::File(file) => lazecode_core::is_json_path(&file.doc.path),
                _ => false,
            });
        is_json.then_some((index, tab_id))
    });
    let mut open_palette = false;
    let mut toggle_files = false;
    let mut toggle_changes = false;
    let mut toggle_terminal_tool = false;
    let mut toggle_consoles_tool = false;
    let mut toggle_compare = false;
    let mut toggle_panel = false;
    let mut open_settings = false;
    let mut save = false;
    let mut new_terminal = false;
    let mut git_log = false;
    let mut open_commit = false;
    let mut search = false;
    let mut find = false;
    let mut open_project = false;
    let mut close_tab = false;
    let mut close_overlay = false;
    let mut goto_line = false;
    let mut new_file = false;
    let mut toggle_tree_view = false;
    let mut unshelve_silent = false;
    let mut unshelve_dialog = false;
    let mut undo = false;
    let mut redo = false;
    let mut format_json = false;
    let mut minify_json = false;
    ctx.input_mut(|input| {
        // 命令面板在模态（设置/对话框/菜单）打开时仍可用，与原行为一致。
        if consumed(input, model, Action::OpenPalette) {
            open_palette = true;
        }
        // ^P：稳定版 Group by Directory 切换（源控抽屉可见时）。控制台聚焦时
        // 让给 TUI（omp 用 Ctrl+P 切换模型），不在应用层吞掉。
        if !blocked && rt.terminal_focus.is_none() && consumed(input, model, Action::ToggleTreeView)
        {
            toggle_tree_view = true;
        }
        // 顶栏工具键：文件 ⌘` / 变更 ⌘1 / Git Log ⌘2 / 终端 ⌘3 /
        // 控制台侧栏 ⌘4 / Compare（默认无键）。
        if !blocked && consumed(input, model, Action::Files) {
            toggle_files = true;
        }
        if !blocked && consumed(input, model, Action::Changes) {
            toggle_changes = true;
        }
        if !blocked && consumed(input, model, Action::GitLog) {
            git_log = true;
        }
        if !blocked && consumed(input, model, Action::Terminal) {
            toggle_terminal_tool = true;
        }
        if !blocked && consumed(input, model, Action::Consoles) {
            toggle_consoles_tool = true;
        }
        if !blocked && consumed(input, model, Action::Compare) {
            toggle_compare = true;
        }
        if !blocked && consumed(input, model, Action::TogglePanel) {
            toggle_panel = true;
        }
        if consumed(input, model, Action::OpenSettings) {
            open_settings = true;
        }
        if !blocked && consumed(input, model, Action::Save) {
            save = true;
        }
        if !blocked && consumed(input, model, Action::Redo) {
            redo = true;
        } else if !blocked && consumed(input, model, Action::Undo) {
            undo = true;
        }
        if !blocked && consumed(input, model, Action::OpenCommit) {
            open_commit = true;
        }
        if consumed(input, model, Action::CloseTab) {
            if blocked {
                close_overlay = true;
            } else {
                close_tab = true;
            }
        }
        if !blocked && consumed(input, model, Action::NewTerminal) {
            new_terminal = true;
        }
        if !blocked && consumed(input, model, Action::OpenProject) {
            open_project = true;
        }
        if !blocked && consumed(input, model, Action::Search) {
            search = true;
        }
        if !blocked && active_json_tab.is_some() && consumed(input, model, Action::FormatJson) {
            format_json = true;
        }
        if !blocked && active_json_tab.is_some() && consumed(input, model, Action::MinifyJson) {
            minify_json = true;
        }
        if !blocked && consumed(input, model, Action::UnshelveSilent) {
            unshelve_silent = true;
        }
        if !blocked && consumed(input, model, Action::UnshelveDialog) {
            unshelve_dialog = true;
        }
        if !blocked && consumed(input, model, Action::Find) {
            find = true;
        }
        if !blocked && consumed(input, model, Action::GotoLine) {
            goto_line = true;
        }
        if !blocked && consumed(input, model, Action::NewFile) {
            new_file = true;
        }
        if input.key_pressed(Key::Escape) && rt.escape_handled_frame != rt.frame_count {
            close_overlay = true;
        }
    });

    if open_palette {
        rt.open_palette();
        rt.settings_open = false;
        rt.about_open = false;
        rt.search.open = false;
        rt.find.open = false;
        model.ui.command_palette_open = true;
    }
    if toggle_files {
        keybindings::run_tool(model, Action::Files);
    }
    if toggle_changes {
        keybindings::run_tool(model, Action::Changes);
    }
    if toggle_terminal_tool {
        keybindings::run_tool(model, Action::Terminal);
    }
    if toggle_consoles_tool {
        keybindings::run_tool(model, Action::Consoles);
    }
    if toggle_compare {
        keybindings::run_tool(model, Action::Compare);
    }
    if toggle_panel {
        model.ui.panel_visible = !model.ui.panel_visible;
    }
    if open_settings {
        rt.settings_open = true;
        rt.about_open = false;
        model.ui.command_palette_open = false;
        rt.search.open = false;
        rt.find.open = false;
    }
    if save {
        if let Some(project) = model.active_project() {
            match project.active_tab {
                Some(id) => model.queue(Command::SaveTab(id)),
                None => model.queue(Command::SaveAll),
            }
        }
    }
    if close_tab {
        if let Some(index) = model.active_project {
            if let Some(id) = model.projects[index].active_tab {
                if let Some(tab) = model.projects[index].tab_by_id(id) {
                    if tab.is_dirty() {
                        rt.dialog = DialogState::CloseDirtyTab {
                            tab_id: id,
                            name: tab.title(),
                        };
                    } else {
                        model.close_tab(index, id);
                        rt.editor_focus = None;
                        rt.last_revealed_tab = None;
                    }
                }
            }
        }
    }
    if new_terminal {
        model.queue(Command::NewTerminal);
    }
    if open_project {
        model.queue(Command::OpenProjectDialog);
    }
    if open_commit {
        model.ui.sidebar_visible = true;
        model.ui.sidebar_view = lazecode_core::SidebarView::Git;
        rt.commit_tab = crate::runtime::CommitTab::Commit;
    }
    if git_log {
        keybindings::run_tool(model, Action::GitLog);
    }
    if undo {
        if let Some(project) = model.active_project_mut() {
            if let Some(id) = project.active_tab {
                if let Some(tab) = project.tab_mut_by_id(id) {
                    if let lazecode_core::TabKind::File(file_tab) = &mut tab.kind {
                        file_tab.doc.undo();
                    }
                }
            }
        }
    }
    if redo {
        if let Some(project) = model.active_project_mut() {
            if let Some(id) = project.active_tab {
                if let Some(tab) = project.tab_mut_by_id(id) {
                    if let lazecode_core::TabKind::File(file_tab) = &mut tab.kind {
                        file_tab.doc.redo();
                    }
                }
            }
        }
    }
    if format_json || minify_json {
        if let Some((project_index, tab_id)) = active_json_tab {
            crate::editor::format_json_tab(model, project_index, tab_id, minify_json);
            ctx.request_repaint();
        }
    }
    if search {
        rt.search.open = true;
        rt.search.opened_frame = rt.frame_count;
        rt.search.focus_requested = true;
        rt.search.searched = false;
        rt.settings_open = false;
        rt.find.open = false;
        model.ui.command_palette_open = false;
    }
    if find && rt.terminal_focus.is_none() && model.active_project().is_some() {
        rt.find.open = true;
        rt.find.opened_frame = rt.frame_count;
        rt.find.focus_requested = true;
    }
    if goto_line {
        rt.dialog = DialogState::GoToLine {
            value: String::new(),
            error: None,
        };
    }
    if toggle_tree_view && model.ui.sidebar_visible {
        match model.ui.sidebar_view {
            lazecode_core::SidebarView::Git => rt.commit_tree_view = !rt.commit_tree_view,
            // Compare 抽屉的同名快捷键（稳定版 ^P：Group By Directory）。
            lazecode_core::SidebarView::Compare => {
                if let Some(project) = model.active_project_mut() {
                    if let Some(session) = project.compare_session.as_mut() {
                        session.group_by_dir = !session.group_by_dir;
                    }
                }
            }
            _ => {}
        }
    }

    if new_file {
        if let Some(project) = model.active_project() {
            rt.dialog = DialogState::NewFile {
                dir: project.active_root().to_path_buf(),
                value: String::new(),
                error: None,
            };
        }
    }
    if close_overlay && !search && !rt.overlay_snapshot.closed_since(model, rt) {
        rt.close_overlay(model);
    }
    if (unshelve_silent || unshelve_dialog) && rt.commit_tab == crate::runtime::CommitTab::Shelf {
        if let Some(name) = rt.shelf_selected.clone() {
            if unshelve_silent {
                let paths = if rt.shelf_file_sel.is_empty() {
                    None
                } else {
                    Some(rt.shelf_file_sel.clone())
                };
                let default_list = model
                    .active_project()
                    .and_then(|p| {
                        lazecode_git::Changelists::open(p.active_root())
                            .lists()
                            .first()
                            .cloned()
                    })
                    .unwrap_or_else(|| "Default".to_string());
                model.queue(Command::ShelfRestore {
                    name,
                    paths,
                    target_list: Some(default_list),
                    delete_after: false,
                });
            } else {
                let root = model
                    .active_project()
                    .map(|p| p.active_root().to_path_buf())
                    .unwrap_or_default();
                let files = lazecode_git::shelf_files(&root, &name).unwrap_or_default();
                let entries: Vec<(String, bool)> = files
                    .into_iter()
                    .map(|f| {
                        let selected =
                            rt.shelf_file_sel.is_empty() || rt.shelf_file_sel.contains(&f);
                        (f, selected)
                    })
                    .collect();
                rt.dialog = crate::runtime::DialogState::Unshelve {
                    name,
                    files: entries,
                    target: None,
                    new_name: String::new(),
                    error: None,
                };
            }
        }
    }
    let snapshot = OverlaySnapshot::capture(model, rt);
    rt.overlay_snapshot = snapshot;
}
