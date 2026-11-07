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
    let palette_open = model.ui.command_palette_open;
    let mut open_palette = false;
    let mut toggle_sidebar = false;
    let mut toggle_panel = false;
    let mut open_settings = false;
    let mut save = false;
    let mut new_terminal = false;
    let mut git_log = false;
    let mut search = false;
    let mut find = false;
    let mut open_project = false;
    let mut close_tab = false;
    let mut switch_project: Option<usize> = None;
    let mut close_overlay = false;
    let mut goto_line = false;
    let mut new_file = false;

    ctx.input_mut(|input| {
        if input.consume_key(command_only(), Key::K) || input.consume_key(command_only(), Key::P) {
            open_palette = true;
        }
        if input.consume_key(command_only(), Key::B) {
            toggle_sidebar = true;
        }
        if input.consume_key(command_only(), Key::J) {
            toggle_panel = true;
        }
        if input.consume_key(command_only(), Key::Comma) {
            open_settings = true;
        }
        if input.consume_key(command_only(), Key::S) {
            save = true;
        }
        if input.consume_key(command_only(), Key::W) {
            close_tab = true;
        }
        if input.consume_key(command_only(), Key::T) {
            new_terminal = true;
        }
        if input.consume_key(command_only(), Key::O) {
            open_project = true;
        }
        if input.consume_key(command_shift(), Key::G) {
            git_log = true;
        }
        if input.consume_key(command_shift(), Key::F) {
            search = true;
        }
        if input.consume_key(command_only(), Key::F) {
            find = true;
        }
        if input.consume_key(command_only(), Key::L) {
            goto_line = true;
        }
        if input.consume_key(command_only(), Key::N) {
            new_file = true;
        }
        if input.consume_key(command_only(), Key::Num1) {
            switch_project = Some(0);
        }
        if input.consume_key(command_only(), Key::Num2) {
            switch_project = Some(1);
        }
        if input.consume_key(command_only(), Key::Num3) {
            switch_project = Some(2);
        }
        if input.consume_key(command_only(), Key::Num4) {
            switch_project = Some(3);
        }
        if input.consume_key(command_only(), Key::Num5) {
            switch_project = Some(4);
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
        if let Some(project) = model.active_project_mut() {
            if let Some(id) = project.active_tab {
                if let Some(tab) = project.tab_by_id(id) {
                    if tab.is_dirty() {
                        rt.dialog = DialogState::CloseDirtyTab {
                            tab_id: id,
                            name: tab.title(),
                        };
                    } else {
                        project.close_tab(id);
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
    if git_log {
        model.ui.panel_visible = true;
        model.ui.panel_tab = PanelTab::Git;
        model.queue(Command::RefreshLog);
    }
    if search {
        rt.search.open = true;
        rt.search.focus_requested = true;
        rt.search.searched = false;
        rt.settings_open = false;
        rt.find.open = false;
        model.ui.command_palette_open = false;
    }
    if find && rt.terminal_focus.is_none() && model.active_project().is_some() {
        rt.find.open = true;
        rt.find.focus_requested = true;
    }
    if goto_line {
        rt.dialog = DialogState::GoToLine {
            value: String::new(),
            error: None,
        };
    }
    if new_file {
        if let Some(project) = model.active_project() {
            rt.dialog = DialogState::NewFile {
                dir: project.root.clone(),
                value: String::new(),
                error: None,
            };
        }
    }
    if let Some(index) = switch_project {
        model.switch_project(index);
        rt.clear_terminal_cache();
    }
    if close_overlay {
        if model.ui.command_palette_open {
            model.ui.command_palette_open = false;
            rt.palette.focus_requested = false;
        } else if rt.settings_open {
            rt.settings_open = false;
        } else if rt.search.open {
            rt.search.open = false;
        } else if rt.find.open {
            rt.find.open = false;
        } else if !matches!(rt.dialog, DialogState::None) {
            rt.dialog = DialogState::None;
        } else if rt.menu.is_some() {
            rt.menu = None;
        } else if palette_open {
            model.ui.command_palette_open = false;
        }
    }
}
