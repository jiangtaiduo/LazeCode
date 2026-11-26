//! Context menus: the dispatcher plus one module per menu family.
mod agent;
mod branch;
mod changes;
mod compare;
mod log;
mod prelude;
mod project;
mod stash;
mod tabs;

use agent::*;
use branch::*;
use changes::*;
use compare::*;
use log::*;
use prelude::*;
use project::*;
use stash::*;
use tabs::*;

pub(super) fn draw_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
) {
    // Menus float above dense content, so their labels need brighter ink than
    // ordinary rows. Shadowing `theme` upgrades every menu renderer at once.
    let mut menu_theme = *theme;
    menu_theme.palette.text = menu_theme.palette.menu_item_text;
    let theme = &menu_theme;
    if let Some(menu) = rt.menu.take() {
        match menu.kind {
            MenuKind::Project => draw_project_menu(ui, model, rt, theme, screen, menu),
            MenuKind::Branch => draw_branch_menu(ui, model, rt, theme, screen, menu),
            MenuKind::Worktree => draw_worktree_menu(ui, model, rt, theme, screen, menu),
            MenuKind::LogBranchFilter { project } => {
                draw_log_branch_menu(ui, model, rt, theme, screen, menu, project)
            }
            MenuKind::LogDateFilter { project } => {
                draw_log_date_menu(ui, model, rt, theme, screen, menu, project)
            }
            MenuKind::LogCommitMenu { project } => {
                let payload = menu.payload.clone();
                draw_commit_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref(),
                )
            }
            MenuKind::BranchContext { project } => {
                let payload = menu.payload.clone();
                draw_branch_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref(),
                )
            }
            MenuKind::TagContext { project } => {
                let payload = menu.payload.clone();
                draw_tag_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref(),
                )
            }
            MenuKind::TreeContext { project, is_dir } => {
                let payload = menu.payload.clone();
                draw_tree_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                    is_dir,
                )
            }
            MenuKind::EditorTabContext { project, tab_id } => {
                let payload = menu.payload.clone();
                draw_editor_tab_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    tab_id,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::TerminalTabContext {
                project,
                tab_id,
                terminal_id,
            } => {
                let payload = menu.payload.clone();
                draw_terminal_tab_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    tab_id,
                    terminal_id,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::ChangelistHeaderContext { project } => {
                let payload = menu.payload.clone();
                draw_changelist_header_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::ChangeFileContext { project } => {
                let payload = menu.payload.clone();
                draw_change_file_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::CommitHistory { project } => {
                draw_commit_history_menu(ui, model, rt, theme, screen, menu, project);
            }
            MenuKind::CommitExtras => {
                draw_commit_extras_menu(ui, model, rt, theme, screen, menu);
            }
            MenuKind::StashEntryMenu { project } => {
                let payload = menu.payload.clone();
                draw_stash_entry_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref(),
                );
            }
            MenuKind::StashFileMenu { project } => {
                let payload = menu.payload.clone();
                draw_stash_file_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::CommitViewOptions => {
                draw_commit_view_options_menu(ui, model, rt, theme, screen, menu);
            }
            MenuKind::CompareViewOptions { project } => {
                draw_compare_view_options_menu(ui, model, rt, theme, screen, menu, project);
            }
            MenuKind::ShelfContext { project } => {
                let payload = menu.payload.clone();
                draw_shelf_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::CompareUserFilter { side } => {
                draw_compare_user_menu(ui, model, rt, theme, screen, menu, side);
            }
            MenuKind::CompareDateFilter { side } => {
                draw_compare_date_menu(ui, model, rt, theme, screen, menu, side);
            }
            MenuKind::CompareRefFilter { side } => {
                draw_compare_ref_menu(ui, model, rt, theme, screen, menu, side);
            }
            MenuKind::LogSortOptions { project } => {
                draw_log_sort_options_menu(ui, model, rt, theme, screen, menu, project);
            }
            MenuKind::LogPathFilter { project } => {
                let payload = menu.payload.clone();
                draw_log_path_filter_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::CommitDetailsFileContext { project } => {
                let payload = menu.payload.clone();
                draw_commit_details_file_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                );
            }
            MenuKind::EditorContext { project } => {
                let payload = menu.payload.clone();
                draw_editor_context_menu(
                    ui,
                    model,
                    rt,
                    theme,
                    screen,
                    menu,
                    project,
                    payload.as_deref().unwrap_or(""),
                );
            }
        }
    }
    if let Some(anchor) = rt.agent_menu.take() {
        draw_agent_menu(ui, model, rt, theme, screen, anchor);
    }
}
