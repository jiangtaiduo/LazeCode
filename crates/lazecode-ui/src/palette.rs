use std::path::PathBuf;

use egui::{
    Align2, Color32, FontId, Key, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2,
};
use lazecode_core::{AppModel, Command, TabKind, ToastKind};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::{PaletteAction, PaletteItem, UiRuntime};
use crate::theme::Theme;
use crate::widgets::{kbd, text_clipped};

const WIDTH: f32 = 640.0;
const ROW_H: f32 = 38.0;

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme) {
    let p = theme.palette;
    let screen = ui.max_rect();
    ui.painter()
        .rect_filled(screen, 0.0, Color32::from_black_alpha(130));
    let panel = Rect::from_min_size(
        Pos2::new(
            screen.center().x - WIDTH * 0.5,
            screen.top() + screen.height() * 0.13,
        ),
        Vec2::new(WIDTH, 460.0),
    );
    ui.painter()
        .rect_filled(panel, egui::CornerRadius::same(10), p.elev);
    ui.painter().rect_stroke(
        panel,
        egui::CornerRadius::same(10),
        Stroke::new(1.0, p.border_strong),
        StrokeKind::Inside,
    );
    let input_rect = Rect::from_min_size(
        Pos2::new(panel.left() + 10.0, panel.top() + 10.0),
        Vec2::new(panel.width() - 20.0, 38.0),
    );
    ui.painter()
        .rect_filled(input_rect, egui::CornerRadius::same(6), p.panel2);
    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(input_rect.left() + 16.0, input_rect.center().y),
            Vec2::splat(14.0),
        ),
        Icon::Search,
        p.text3,
    );
    let field = Rect::from_min_max(
        Pos2::new(input_rect.left() + 30.0, input_rect.top() + 4.0),
        Pos2::new(input_rect.right() - 70.0, input_rect.bottom() - 4.0),
    );
    let response = ui.put(
        field,
        TextEdit::singleline(&mut rt.palette.query)
            .frame(egui::Frame::NONE)
            .hint_text(crate::tr!(
                model,
                "Search files, projects, commands",
                "搜索文件、项目与命令"
            ))
            .font(FontId::proportional(14.0)),
    );
    if rt.palette.focus_requested {
        response.request_focus();
        rt.palette.focus_requested = false;
    }
    kbd(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(input_rect.right() - 22.0, input_rect.center().y),
            Vec2::new(30.0, 18.0),
        ),
        "esc",
        theme,
    );

    let items = build_items(model, &rt.palette.query);
    if items.is_empty() {
        ui.painter().text(
            Pos2::new(panel.center().x, panel.top() + 96.0),
            Align2::CENTER_CENTER,
            crate::tr!(
                model,
                "No matching commands or files",
                "没有匹配的命令或文件"
            ),
            FontId::proportional(12.5),
            p.text3,
        );
        if ui.input(|input| input.key_pressed(Key::Escape)) {
            model.ui.command_palette_open = false;
        }
        return;
    }
    let max_visible = ((panel.height() - 72.0) / ROW_H).floor() as usize;
    rt.palette.selected = rt.palette.selected.min(items.len().saturating_sub(1));
    if ui.input(|input| input.key_pressed(Key::ArrowDown)) {
        rt.palette.selected = (rt.palette.selected + 1) % items.len();
    }
    if ui.input(|input| input.key_pressed(Key::ArrowUp)) {
        rt.palette.selected = (rt.palette.selected + items.len() - 1) % items.len();
    }
    let mut activate = false;
    let list_rect = Rect::from_min_size(
        Pos2::new(panel.left() + 6.0, input_rect.bottom() + 6.0),
        Vec2::new(panel.width() - 12.0, panel.height() - 72.0),
    );
    for (index, item) in items.iter().enumerate().take(max_visible) {
        let row = Rect::from_min_size(
            Pos2::new(list_rect.left(), list_rect.top() + index as f32 * ROW_H),
            Vec2::new(list_rect.width(), ROW_H - 2.0),
        );
        let response = ui.interact(
            row,
            ui.scope_id().with(("palette-item", index)),
            Sense::click(),
        );
        if response.hovered() {
            rt.palette.selected = index;
        }
        let selected = rt.palette.selected == index;
        if selected {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(5), p.active);
        }
        paint_icon(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(row.left() + 18.0, row.center().y),
                Vec2::splat(15.0),
            ),
            item.icon,
            if selected { p.accent } else { p.text3 },
        );
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(row.left() + 34.0, row.top() + 4.0),
                Pos2::new(row.right() - 90.0, row.top() + 21.0),
            ),
            &item.title,
            FontId::proportional(13.0),
            if selected { p.text } else { p.text2 },
            Align2::LEFT_CENTER,
        );
        if !item.subtitle.is_empty() {
            text_clipped(
                ui.painter(),
                Rect::from_min_max(
                    Pos2::new(row.left() + 34.0, row.top() + 19.0),
                    Pos2::new(row.right() - 90.0, row.bottom() - 1.0),
                ),
                &item.subtitle,
                FontId::proportional(11.0),
                p.text3,
                Align2::LEFT_CENTER,
            );
        }
        if let Some(shortcut) = &item.shortcut {
            kbd(
                ui.painter(),
                Rect::from_center_size(
                    Pos2::new(row.right() - 34.0, row.center().y),
                    Vec2::new(46.0, 17.0),
                ),
                shortcut,
                theme,
            );
        }
        if response.clicked() {
            rt.palette.selected = index;
            activate = true;
        }
    }
    let enter = ui.input(|input| input.key_pressed(Key::Enter));
    if enter {
        activate = true;
    }
    if activate {
        if let Some(item) = items.get(rt.palette.selected) {
            let action = item.action.clone();
            execute(action, model, rt);
        }
    }
    let is_escape = ui.input(|input| input.key_pressed(Key::Escape));
    let outside_click = rt.palette.opened_frame != rt.frame_count
        && ui.input(|input| {
            input.pointer.any_click()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|pos| !panel.contains(pos))
        });
    if is_escape || outside_click {
        model.ui.command_palette_open = false;
    }
    ui.painter().text(
        Pos2::new(panel.left() + 12.0, panel.bottom() - 12.0),
        Align2::LEFT_CENTER,
        if model.settings.language.is_zh() {
            format!("{} 个结果", items.len())
        } else {
            format!("{} results", items.len())
        },
        FontId::monospace(9.0),
        p.text3,
    );
    ui.painter().text(
        Pos2::new(panel.right() - 12.0, panel.bottom() - 12.0),
        Align2::RIGHT_CENTER,
        crate::tr!(model, "↑↓ navigate · ↵ open", "↑↓ 选择 · ↵ 打开"),
        FontId::monospace(9.0),
        p.text3,
    );
}

fn build_items(model: &AppModel, query: &str) -> Vec<PaletteItem> {
    let query = query.trim().to_lowercase();
    let mut items: Vec<PaletteItem> = Vec::new();
    let mut action =
        |icon, title: &str, subtitle: &str, shortcut: Option<&str>, action: PaletteAction| {
            let matches = query.is_empty()
                || title.to_lowercase().contains(&query)
                || subtitle.to_lowercase().contains(&query);
            if matches {
                items.push(PaletteItem {
                    icon,
                    title: title.to_string(),
                    subtitle: subtitle.to_string(),
                    shortcut: shortcut.map(|value| value.to_string()),
                    action,
                });
            }
        };

    action(
        Icon::FolderOpen,
        crate::tr!(model, "Open project…", "打开项目…"),
        crate::tr!(model, "Choose a folder", "选择文件夹"),
        None,
        PaletteAction::OpenProject,
    );
    action(
        Icon::Sidebar,
        crate::tr!(model, "Toggle explorer", "切换资源管理器"),
        crate::tr!(model, "Show or hide the sidebar", "显示或隐藏侧栏"),
        Some("⌘1"),
        PaletteAction::ToggleSidebar,
    );
    action(
        Icon::Panel,
        crate::tr!(model, "Toggle console", "切换控制台"),
        crate::tr!(model, "Show or hide the bottom panel", "显示或隐藏底部面板"),
        Some("⌘4"),
        PaletteAction::ToggleConsole,
    );
    action(
        Icon::GitLog,
        crate::tr!(model, "Open Git log", "打开 Git 日志"),
        crate::tr!(model, "Commit graph and details", "提交图与详情"),
        Some("⌘9"),
        PaletteAction::OpenGitLog,
    );
    action(
        Icon::GitLog,
        crate::tr!(model, "Push branch…", "推送分支…"),
        crate::tr!(
            model,
            "Publish current branch to a remote",
            "将当前分支推送到远程"
        ),
        None,
        PaletteAction::OpenPush,
    );
    action(
        Icon::GitLog,
        crate::tr!(model, "Pull…", "拉取…"),
        crate::tr!(
            model,
            "Merge/rebase/ff-only upstream integration",
            "合并/变基/仅快进集成上游"
        ),
        None,
        PaletteAction::OpenPull,
    );
    action(
        Icon::GitLog,
        crate::tr!(model, "Fetch all remotes", "拉取所有远程"),
        crate::tr!(model, "git fetch --all --prune", "git fetch --all --prune"),
        None,
        PaletteAction::FetchAll,
    );
    action(
        Icon::GitLog,
        crate::tr!(model, "Manage remotes…", "管理远程…"),
        crate::tr!(
            model,
            "Add, rename, re-point, delete",
            "添加、重命名、修改、删除"
        ),
        None,
        PaletteAction::OpenRemotes,
    );
    action(
        Icon::GitLog,
        crate::tr!(model, "Stash changes…", "贮藏更改…"),
        crate::tr!(
            model,
            "Set tracked changes aside; restore them later",
            "暂存已跟踪更改，稍后可恢复"
        ),
        None,
        PaletteAction::StashChanges,
    );
    action(
        Icon::GitLog,
        crate::tr!(model, "Stashes…", "贮藏管理…"),
        crate::tr!(
            model,
            "List, apply, pop or drop stashes",
            "查看、应用或删除贮藏"
        ),
        None,
        PaletteAction::StashManager,
    );
    action(
        Icon::GitLog,
        crate::tr!(model, "Shelves…", "架藏管理…"),
        crate::tr!(
            model,
            "List, restore or delete shelves",
            "查看、恢复或删除架藏"
        ),
        None,
        PaletteAction::ShelfManager,
    );
    action(
        Icon::GitLog,
        crate::tr!(model, "Git tools…", "Git 工具…"),
        crate::tr!(
            model,
            "Submodules and Git LFS status",
            "子模块与 Git LFS 状态"
        ),
        None,
        PaletteAction::GitTools,
    );
    action(
        Icon::GitLog,
        crate::tr!(model, "Compare branches…", "比较分支…"),
        crate::tr!(
            model,
            "Two-way commit and diff comparison",
            "分支双向提交与文件差异比较"
        ),
        None,
        PaletteAction::CompareBranches,
    );
    action(
        Icon::FolderOpen,
        crate::tr!(model, "Clone repository…", "克隆仓库…"),
        crate::tr!(
            model,
            "Clone a Git repository from URL",
            "从 URL 克隆 Git 仓库"
        ),
        None,
        PaletteAction::CloneRepo,
    );
    action(
        Icon::Search,
        crate::tr!(model, "Go to line…", "跳转到行…"),
        crate::tr!(
            model,
            "Jump to line in active file",
            "跳转到当前文件的指定行"
        ),
        Some("⌘L"),
        PaletteAction::GoToLine,
    );
    action(
        Icon::Save,
        crate::tr!(model, "Format document", "格式化文档"),
        crate::tr!(
            model,
            "Format the active file through the language server",
            "通过语言服务器格式化当前文件"
        ),
        Some("⌥⌘L"),
        PaletteAction::FormatDocument,
    );
    action(
        Icon::Search,
        crate::tr!(model, "Go to symbol…", "转到符号…"),
        crate::tr!(
            model,
            "List symbols in the active file",
            "列出当前文件中的符号"
        ),
        Some("⌥⌘O"),
        PaletteAction::GoToSymbol,
    );
    action(
        Icon::Search,
        crate::tr!(model, "Rename symbol…", "重命名符号…"),
        crate::tr!(
            model,
            "Rename a symbol and its references",
            "重命名符号及其引用"
        ),
        Some("F2"),
        PaletteAction::RenameSymbol,
    );
    action(
        Icon::Search,
        crate::tr!(model, "Go to definition", "跳转到定义"),
        crate::tr!(
            model,
            "Jump to the symbol under the caret",
            "跳转到光标下的符号定义"
        ),
        Some("F12"),
        PaletteAction::GoToDefinition,
    );
    action(
        Icon::Search,
        crate::tr!(model, "Find in files", "在文件中查找"),
        crate::tr!(model, "Search file contents", "搜索文件内容"),
        Some("⌘⇧F"),
        PaletteAction::OpenSearch,
    );
    action(
        Icon::Settings,
        crate::tr!(model, "Settings", "设置"),
        crate::tr!(model, "Theme, editor and terminal", "主题、编辑器与终端"),
        Some("⌘,"),
        PaletteAction::OpenSettings,
    );
    action(
        Icon::Sun,
        crate::tr!(model, "Toggle theme", "切换主题"),
        crate::tr!(
            model,
            "Switch between dark and light",
            "在深色与浅色之间切换"
        ),
        None,
        PaletteAction::ToggleTheme,
    );
    action(
        Icon::Plus,
        crate::tr!(model, "New console", "新建控制台"),
        crate::tr!(model, "Start a shell in the project", "在项目中启动 shell"),
        Some("⌘⇧T"),
        PaletteAction::NewTerminal,
    );
    action(
        Icon::Save,
        crate::tr!(model, "Save file", "保存文件"),
        crate::tr!(
            model,
            "Write the active editor buffer",
            "写入当前编辑器缓冲区"
        ),
        Some("⌘S"),
        PaletteAction::SaveFile,
    );
    action(
        Icon::X,
        crate::tr!(model, "Close tab", "关闭标签页"),
        crate::tr!(model, "Close the active tab", "关闭当前标签页"),
        None,
        PaletteAction::CloseTab,
    );
    action(
        Icon::Refresh,
        crate::tr!(model, "Refresh Git status", "刷新 Git 状态"),
        crate::tr!(model, "Re-read the repository", "重新读取仓库"),
        None,
        PaletteAction::RefreshGit,
    );
    action(
        Icon::Info,
        crate::tr!(model, "About LazeCode", "关于 LazeCode"),
        crate::tr!(model, "Version and architecture", "版本与架构"),
        None,
        PaletteAction::About,
    );

    for (index, project) in model.projects.iter().enumerate() {
        let matches = query.is_empty()
            || project.name.to_lowercase().contains(&query)
            || project
                .root
                .to_string_lossy()
                .to_lowercase()
                .contains(&query);
        if matches {
            items.push(PaletteItem {
                icon: Icon::Worktree,
                title: if model.settings.language.is_zh() {
                    format!("切换到 {}", project.name)
                } else {
                    format!("Switch to {}", project.name)
                },
                subtitle: project.root.display().to_string(),
                shortcut: if index < 5 {
                    Some(format!("⌘{}", index + 1))
                } else {
                    None
                },
                action: PaletteAction::SwitchProject(index),
            });
        }
    }

    // Worktrees of the active project (skip the already-open main entry).
    if let Some((project_index, project)) = model
        .active_project
        .and_then(|index| model.projects.get(index).map(|project| (index, project)))
    {
        for worktree in &project.worktrees {
            if worktree.is_main {
                continue;
            }
            let title_label = if model.settings.language.is_zh() {
                format!("切换工作树 {}", worktree.name)
            } else {
                format!("Switch worktree {}", worktree.name)
            };
            let matches = query.is_empty()
                || worktree.name.to_lowercase().contains(&query)
                || worktree
                    .path
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(&query);
            if matches {
                items.push(PaletteItem {
                    icon: Icon::Worktree,
                    title: title_label,
                    subtitle: worktree.path.display().to_string(),
                    shortcut: None,
                    action: PaletteAction::SwitchWorktree {
                        project: project_index,
                        name: worktree.name.clone(),
                    },
                });
            }
        }
    }

    if let Some(project) = model.active_project() {
        for tab in &project.tabs {
            if let TabKind::File(file) = &tab.kind {
                let name = file.doc.title();
                let path = file.doc.path.display().to_string();
                if query.is_empty()
                    || name.to_lowercase().contains(&query)
                    || path.to_lowercase().contains(&query)
                {
                    items.push(PaletteItem {
                        icon: Icon::File,
                        title: name,
                        subtitle: path,
                        shortcut: None,
                        action: PaletteAction::OpenFile(file.doc.path.clone()),
                    });
                }
            }
        }
        if !query.is_empty() {
            let mut files = Vec::new();
            flatten_files(&project.tree.nodes, &mut files);
            let mut matches = 0;
            for path in files {
                let path_text = path.to_string_lossy();
                if path_text.to_lowercase().contains(&query) {
                    items.push(PaletteItem {
                        icon: Icon::File,
                        title: path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        subtitle: path_text.into_owned(),
                        shortcut: None,
                        action: PaletteAction::OpenFile(project.root.join(&path)),
                    });
                    matches += 1;
                    if matches >= 30 {
                        break;
                    }
                }
            }
        }
    }
    items
}

fn flatten_files(nodes: &[lazecode_fs::tree::TreeNode], out: &mut Vec<PathBuf>) {
    for node in nodes {
        match node.kind {
            lazecode_fs::tree::NodeKind::File => out.push(node.path.clone()),
            lazecode_fs::tree::NodeKind::Folder => flatten_files(&node.children, out),
        }
    }
}

fn execute(action: PaletteAction, model: &mut AppModel, rt: &mut UiRuntime) {
    model.ui.command_palette_open = false;
    match action {
        PaletteAction::OpenFile(path) => {
            let settings = model.settings.clone();
            if let Some(project) = model.active_project_mut() {
                match project.open_file(path, &settings) {
                    Ok(id) => rt.set_editor_focus(id),
                    Err(err) => model.toast(err, ToastKind::Error),
                }
            }
        }
        PaletteAction::ToggleSidebar => model.ui.sidebar_visible = !model.ui.sidebar_visible,
        PaletteAction::ToggleConsole => model.ui.panel_visible = !model.ui.panel_visible,
        PaletteAction::OpenSettings => {
            rt.settings_open = true;
            model.ui.settings_section = 0;
        }
        PaletteAction::ToggleTheme => {
            model.toggle_theme();
            rt.applied_theme = None;
        }
        PaletteAction::OpenProject => model.queue(Command::OpenProjectDialog),
        PaletteAction::NewTerminal => model.queue(Command::NewTerminal),
        PaletteAction::SaveFile => {
            if let Some(project) = model.active_project() {
                if let Some(id) = project.active_tab {
                    model.queue(Command::SaveTab(id));
                } else {
                    model.queue(Command::SaveAll);
                }
            }
        }
        PaletteAction::CloseTab => {
            if let Some(project) = model.active_project_mut() {
                if let Some(id) = project.active_tab {
                    let dirty = project.tab_by_id(id).is_some_and(|tab| tab.is_dirty());
                    if dirty {
                        rt.dialog = crate::runtime::DialogState::CloseDirtyTab {
                            tab_id: id,
                            name: project
                                .tab_by_id(id)
                                .map(|tab| tab.title())
                                .unwrap_or_default(),
                        };
                    } else {
                        project.close_tab(id);
                    }
                }
            }
        }
        PaletteAction::OpenGitLog => {
            model.ui.panel_visible = true;
            model.ui.panel_tab = lazecode_core::PanelTab::Git;
            model.queue(Command::RefreshLog);
        }
        PaletteAction::RefreshGit => {
            model.queue(Command::ReloadGit);
            model.queue(Command::RefreshLog);
        }
        PaletteAction::OpenSearch => {
            rt.search.open = true;
            rt.search.focus_requested = true;
        }
        PaletteAction::OpenPush => {
            let Some(project) = model.active_project() else {
                return;
            };
            let branch = project.branch();
            let remote = project
                .remotes
                .first()
                .map(|info| info.name.clone())
                .unwrap_or_else(|| "origin".to_string());
            rt.dialog = crate::runtime::DialogState::Push {
                remote,
                branch,
                set_upstream: false,
                status: None,
            };
        }
        PaletteAction::OpenPull => {
            rt.dialog = crate::runtime::DialogState::Pull {
                mode: lazecode_git::PullMode::Merge,
                status: None,
            };
        }
        PaletteAction::OpenRemotes => {
            let Some(project) = model.active_project() else {
                return;
            };
            let remotes = project.remotes.clone();
            rt.dialog = crate::runtime::DialogState::Remotes {
                remotes,
                new_name: String::new(),
                new_url: String::new(),
                error: None,
            };
        }
        PaletteAction::FetchAll => model.queue(Command::FetchAll),
        PaletteAction::StashChanges => {
            model.queue(Command::StashPush {
                message: None,
                include_untracked: true,
            });
        }
        PaletteAction::ShelvedChanges | PaletteAction::ShelfManager => {
            rt.dialog = crate::runtime::DialogState::ShelfManager {
                shelves: Vec::new(),
                selected: 0,
                error: None,
                loaded: false,
            };
        }
        PaletteAction::StashManager => {
            rt.dialog = crate::runtime::DialogState::StashManager {
                stashes: Vec::new(),
                selected: 0,
                error: None,
                loaded: false,
            };
        }
        PaletteAction::CloneRepo => {
            rt.dialog = crate::runtime::DialogState::CloneRepo {
                url: String::new(),
                dest: String::new(),
                error: None,
            };
        }
        PaletteAction::GitTools => {
            rt.dialog = crate::runtime::DialogState::GitTools {
                tab: crate::runtime::GitToolsTab::Submodules,
                submodules: Vec::new(),
                lfs_status: None,
                loaded: (false, false),
            };
        }
        PaletteAction::CompareBranches => {
            let (left, right) = if let Some(project) = model.active_project() {
                let current = project.branch();
                let other = project
                    .branches
                    .iter()
                    .find(|b| b.name != current)
                    .map(|b| b.name.clone())
                    .unwrap_or_else(|| "main".into());
                (current, other)
            } else {
                ("HEAD".into(), "main".into())
            };
            crate::editor::open_compare_page(model, left, right);
        }
        PaletteAction::GoToLine => {
            rt.dialog = crate::runtime::DialogState::GoToLine {
                value: String::new(),
                error: None,
            };
        }
        PaletteAction::FormatDocument => {
            if let Some(tab_id) = model
                .active_project()
                .and_then(|project| project.active_tab)
            {
                rt.lsp.format_request = Some(tab_id);
            }
        }
        PaletteAction::GoToSymbol => {
            if let Some(tab_id) = model
                .active_project()
                .and_then(|project| project.active_tab)
            {
                rt.lsp.symbols_request = Some(tab_id);
            }
        }
        PaletteAction::RenameSymbol => {
            let info = model.active_project().and_then(|project| {
                let tab_id = project.active_tab?;
                let tab = project.tab_by_id(tab_id)?;
                match &tab.kind {
                    TabKind::File(file) => Some((
                        tab_id,
                        file.doc.path.to_string_lossy().to_string(),
                        file.doc.cursor.line,
                        file.doc.cursor.col,
                        crate::editor::word_at(&file.doc, file.doc.cursor),
                    )),
                    _ => None,
                }
            });
            if let Some((tab_id, path, line, col, value)) = info {
                rt.dialog = crate::runtime::DialogState::RenameSymbol {
                    tab_id,
                    path,
                    line,
                    col,
                    value,
                    error: None,
                };
            }
        }
        PaletteAction::GoToDefinition => {
            let info = model.active_project().and_then(|project| {
                let tab_id = project.active_tab?;
                let tab = project.tab_by_id(tab_id)?;
                match &tab.kind {
                    TabKind::File(file) => Some((tab_id, file.doc.cursor)),
                    _ => None,
                }
            });
            if let Some((tab_id, cursor)) = info {
                rt.lsp.goto_request = Some((tab_id, cursor));
            }
        }
        PaletteAction::About => {
            rt.settings_open = true;
            model.ui.settings_section = 3;
        }
        PaletteAction::SwitchProject(index) => {
            model.switch_project(index);
            rt.clear_terminal_cache();
        }
        PaletteAction::SwitchWorktree { project, name } => {
            if let Some(project_state) = model.projects.get_mut(project) {
                if let Err(err) = project_state.ensure_worktree(&name) {
                    model.toast(err, ToastKind::Error);
                }
            }
            rt.clear_terminal_cache();
        }
        PaletteAction::Quit => {
            let dirty = model
                .projects
                .iter()
                .any(|project| project.tabs.iter().any(|tab| tab.is_dirty()));
            if dirty {
                rt.dialog = crate::runtime::DialogState::Quit;
            } else {
                model.should_quit = true;
            }
        }
    }
}
