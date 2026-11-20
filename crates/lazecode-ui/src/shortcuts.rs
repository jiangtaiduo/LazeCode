use egui::{Key, Modifiers, Ui};
use lazecode_core::{AppModel, Command, PanelTab};

use crate::runtime::{DialogState, UiRuntime};

fn command_only() -> Modifiers {
    Modifiers {
        command: true,
        ..Default::default()
    }
}

fn command_shift() -> Modifiers {
    Modifiers {
        command: true,
        shift: true,
        ..Default::default()
    }
}

pub fn handle(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime) {
    let ctx = ui.ctx();
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
    let mut toggle_sidebar = false;
    let mut toggle_panel = false;
    let mut toggle_terminal = false;
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
        if input.consume_key(command_shift(), Key::A) {
            open_palette = true;
        }
        // ^P：稳定版 Group by Directory 切换（源控抽屉可见时）。控制台聚焦时
        // 让给 TUI（omp 用 Ctrl+P 切换模型），不在应用层吞掉。
        if !blocked
            && rt.terminal_focus.is_none()
            && input.consume_key(egui::Modifiers::CTRL, Key::P)
        {
            toggle_tree_view = true;
        }
        if !blocked && input.consume_key(command_only(), Key::Num1) {
            toggle_sidebar = true;
        }
        if !blocked && input.consume_key(command_only(), Key::Num4) {
            toggle_panel = true;
        }
        if !blocked
            && (input.consume_key(command_only(), Key::Backtick)
                || input.consume_key(Modifiers::ALT, Key::F12))
        {
            toggle_terminal = true;
        }
        if input.consume_key(command_only(), Key::Comma) {
            open_settings = true;
        }
        if !blocked && input.consume_key(command_only(), Key::S) {
            save = true;
        }
        if !blocked && input.consume_key(command_shift(), Key::Z) {
            redo = true;
        } else if !blocked && input.consume_key(command_only(), Key::Z) {
            undo = true;
        }
        if !blocked && input.consume_key(command_only(), Key::K) {
            open_commit = true;
        }
        if input.consume_key(command_only(), Key::W) {
            if blocked {
                close_overlay = true;
            } else {
                close_tab = true;
            }
        }
        if !blocked && input.consume_key(command_shift(), Key::T) {
            new_terminal = true;
        }
        if !blocked && input.consume_key(command_only(), Key::O) {
            open_project = true;
        }
        if !blocked && input.consume_key(command_only(), Key::Num9) {
            git_log = true;
        }
        if input.consume_key(command_shift(), Key::F) {
            search = true;
        }
        let cmd_alt = Modifiers {
            alt: true,
            command: true,
            mac_cmd: true,
            ..Default::default()
        };
        let alt_shift = Modifiers {
            alt: true,
            shift: true,
            ..Default::default()
        };
        if !blocked && active_json_tab.is_some() && input.consume_key(alt_shift, Key::F) {
            format_json = true;
        }
        if !blocked && active_json_tab.is_some() && input.consume_key(alt_shift, Key::M) {
            minify_json = true;
        }
        if !blocked && input.consume_key(cmd_alt, Key::U) {
            unshelve_silent = true;
        }
        if !blocked && input.consume_key(command_shift(), Key::U) {
            unshelve_dialog = true;
        }
        if !blocked && input.consume_key(command_only(), Key::F) {
            find = true;
        }
        if !blocked && input.consume_key(command_only(), Key::L) {
            goto_line = true;
        }
        if !blocked && input.consume_key(command_only(), Key::N) {
            new_file = true;
        }
        if input.key_pressed(Key::Escape) {
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
    if toggle_sidebar {
        model.ui.sidebar_visible = !model.ui.sidebar_visible;
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
        if model.ui.panel_visible && model.ui.panel_tab == PanelTab::Git {
            model.ui.panel_visible = false;
        } else {
            model.ui.panel_visible = true;
            model.ui.panel_tab = PanelTab::Git;
            if model.ui.terminal_placement == lazecode_core::TerminalPlacement::Bottom {
                model.ui.terminal_visible = false;
            }
            model.queue(Command::RefreshLog);
        }
    }
    if toggle_terminal {
        if model.ui.terminal_placement == lazecode_core::TerminalPlacement::Bottom {
            if model.ui.terminal_visible {
                model.ui.terminal_visible = false;
            } else {
                model.ui.terminal_visible = true;
                model.ui.panel_visible = false;
            }
        } else {
            model.ui.terminal_visible = !model.ui.terminal_visible;
        }
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
    if close_overlay {
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
}
