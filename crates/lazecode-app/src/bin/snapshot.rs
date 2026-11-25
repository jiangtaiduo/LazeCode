use std::path::{Path, PathBuf};

use lazecode_core::{
    AppModel, PanelTab, ProjectState, Settings, SidebarView, ThemeMode, ThemeSkin, UiState,
};
use lazecode_ui::UiRuntime;
use std::collections::HashSet;

const W: f32 = 1440.0;
const H: f32 = 900.0;
const SCALE: f32 = 2.0;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let output_path = PathBuf::from(
        args.next()
            .unwrap_or_else(|| "/tmp/laze-snapshot.png".into()),
    );
    let project_root = PathBuf::from(
        args.next()
            .unwrap_or_else(|| std::env::current_dir().unwrap().display().to_string()),
    );
    let mode = args.next().unwrap_or_else(|| "editor".to_string());
    let language = args.next().unwrap_or_else(|| "en".to_string());

    let mut settings = lazecode_core::Settings {
        // The offscreen snapshot is a deterministic visual harness; spawning
        // real language servers there would add latency and orphan processes.
        lsp_enabled: false,
        ..Settings::default()
    };
    if language == "zh" {
        settings.language = lazecode_core::Language::Chinese;
    }
    // Visual harness override, e.g. LAZE_UI_FONT="SF Pro".
    if let Ok(font) = std::env::var("LAZE_UI_FONT") {
        settings.ui_font = font;
    }
    if let Ok(font) = std::env::var("LAZE_EDITOR_FONT") {
        settings.editor.font_family = font;
    }
    if let Ok(font) = std::env::var("LAZE_TERMINAL_FONT") {
        settings.terminal.font_family = font;
    }
    // Visual harness override, e.g. LAZE_THEME=light LAZE_SKIN=tundra.
    if let Ok(value) = std::env::var("LAZE_THEME") {
        settings.theme = if value == "light" {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        };
    }
    if let Ok(value) = std::env::var("LAZE_SKIN") {
        if let Some(skin) = ThemeSkin::ALL.into_iter().find(|skin| skin.id() == value) {
            settings.set_skin(skin);
        }
    }
    let mut pending_jump: Option<(u64, usize)> = None;
    let mut model = AppModel {
        settings: settings.clone(),
        projects: Vec::new(),
        active_project: None,
        ui: UiState::default(),
        toasts: Vec::new(),
        recent: Vec::new(),
        recent_files: Vec::new(),
        commands: Vec::new(),
        notifier: None,
        lsp: lazecode_core::LspManager::new(settings.lsp_enabled, None),
        should_quit: false,
        version: env!("CARGO_PKG_VERSION"),
        favorite_branches: HashSet::new(),
        git_cmd_log: Vec::new(),
    };
    if mode == "single" || mode == "single_menu" {
        let mut files = Vec::new();
        let readme = project_root.join("README.md");
        if readme.exists() {
            files.push(readme);
        }
        let source = project_root.join("crates/lazecode-ui/src/editor.rs");
        if source.exists() {
            files.push(source);
        }
        let docs = project_root.join("docs/design/architecture.md");
        if docs.exists() {
            files.push(docs);
        }
        model
            .open_files(files)
            .map_err(|err| anyhow::anyhow!(err))?;
    } else if mode == "home_recents" {
        let readme = project_root.join("README.md");
        let source = project_root.join("crates/lazecode-ui/src/editor.rs");
        model.recent = vec![project_root.clone()];
        model.recent_files = vec![readme, source];
    } else if mode != "welcome" {
        let mut project = ProjectState::open(project_root.clone(), &settings, None)
            .map_err(|err| anyhow::anyhow!(err))?;
        let readme = project_root.join("README.md");
        if readme.exists() {
            let _ = project.open_file(readme, &settings);
        }
        let source = project_root.join("crates/lazecode-ui/src/editor.rs");
        if source.exists() {
            let _ = project.open_file(source, &settings);
        }
        let docs = project_root.join("docs/design/architecture.md");
        if docs.exists() {
            let _ = project.open_file(docs, &settings);
        }
        if mode == "diff" || mode == "cmpdiff" || mode == "cmpdiff3" {
            if mode == "cmpdiff" || mode == "cmpdiff3" {
                // Compare Versions 会话（稳定版 CompareDrawer + Diff 标签），
                // 与用户提供的稳定版截图同一对提交/文件。
                let from = "3cd41b6".to_string();
                let to = "cddd4c3".to_string();
                if let Ok(files) = lazecode_git::compare_commits(&project_root, &from, &to) {
                    project.compare_session = Some(lazecode_core::model::CompareSession {
                        from: from.clone(),
                        to: to.clone(),
                        files,
                        nav_order: lazecode_core::model::compare_nav_order(
                            &lazecode_git::compare_commits(&project_root, &from, &to)
                                .unwrap_or_default(),
                        ),
                        group_by_dir: true,
                        root_expanded: true,
                        selected: Some("crates/lazecode-ui/src/app_ui.rs".to_string()),
                        ..Default::default()
                    });
                    model.ui.sidebar_view = SidebarView::Compare;
                    model.ui.sidebar_visible = true;
                }
                let source = lazecode_core::DiffSource::CompareCommits {
                    from: from.clone(),
                    to: to.clone(),
                    path: "crates/lazecode-ui/src/app_ui.rs".to_string(),
                };
                let jump_group: usize = if mode == "cmpdiff3" { 24 } else { 0 };
                project.open_diff_source(source.clone());
                if let Ok(fd) = lazecode_git::diff_commits(
                    &project_root,
                    &from,
                    &to,
                    "crates/lazecode-ui/src/app_ui.rs",
                ) {
                    project.set_diff_for_source(&project_root, &source, Ok(fd));
                }
                if let Some(diff_tab) =
                    project.tabs.iter_mut().find_map(|tab| match &mut tab.kind {
                        lazecode_core::TabKind::Diff(diff) => Some(diff),
                        _ => None,
                    })
                {
                    // 跳到第 3 组并保留（非首开），验证滚动后蓝框跟随内容。
                    diff_tab.current_hunk = jump_group;
                    diff_tab.revealed = true;
                    if jump_group > 0 {
                        pending_jump = Some((project.active_tab.unwrap_or(0), jump_group));
                    }
                }
                model.ui.terminal_visible = false;
                model.ui.panel_visible = true;
            } else {
                let source = lazecode_core::DiffSource::Commit {
                    commit: "e338822".to_string(),
                    path: "images.py".to_string(),
                };
                project.open_diff_source(source.clone());
                if let Ok(fd) =
                    lazecode_git::commit_file_diff(&project_root, "e338822", "images.py")
                {
                    project.set_diff_for_source(&project_root, &source, Ok(fd));
                }
            }
        }
        if mode == "compare" {
            // 分支比较页：开当前分支与 main 的比较（feat-rust 仓库内有效）。
            // snapshot 单帧渲染拿不到后台 job 回填，这里同步预取注入。
            let title = "Compare: feat-rust and main".to_string();
            project.open_compare_branches("feat-rust".to_string(), "main".to_string(), title);
            if let Ok(data) =
                lazecode_git::compare_branches(&project_root, "feat-rust", "main", None, None, 1000)
            {
                model.ui.compare_branches.root = project_root.clone();
                model.ui.compare_branches.left_ref = "feat-rust".to_string();
                model.ui.compare_branches.right_ref = "main".to_string();
                model.ui.compare_branches.loading = false;
                // 复现 bug 场景：上侧选中首个提交（详情同步注入），下侧
                // 保持未选中——下侧详情必须为空，不得串用上侧数据。
                if let Some(first) = data.left.commits.first().cloned() {
                    model.ui.compare_branches.left_view.selected = vec![first.id.clone()];
                    if let Ok(files) = lazecode_git::commit_files(&project_root, &first.id) {
                        model.ui.compare_branches.left_view.detail =
                            Some(lazecode_core::CompareDetail {
                                key: format!("c:{}", first.id),
                                files,
                                ..Default::default()
                            });
                    }
                }
                model.ui.compare_branches.sides = Some(data);
            }
            model.ui.panel_visible = false;
            model.ui.terminal_visible = false;
        }
        if mode == "panes" {
            for direction in [
                lazecode_core::PaneSplit::Row,
                lazecode_core::PaneSplit::Column,
                lazecode_core::PaneSplit::Row,
            ] {
                let Some(pane) = project.active_worktree().active_terminal else {
                    break;
                };
                if project
                    .split_terminal(pane, direction, &settings, None)
                    .is_err()
                {
                    break;
                }
            }
        }
        if mode == "editor" {
            if let Some(index) = project.tabs.iter().position(|tab| {
                matches!(&tab.kind, lazecode_core::TabKind::File(file)
                    if file.doc.path.ends_with("crates/lazecode-ui/src/editor.rs"))
            }) {
                project.active_tab = Some(project.tabs[index].id);
            }
        }
        if mode == "compare_local" {
            if let Some(commit) = project.log.get(1).or_else(|| project.log.first()) {
                let commit_id = commit.id.clone();
                let root = project.active_root().to_path_buf();
                if let Ok(files) = lazecode_git::compare_commit_to_worktree(&root, &commit_id) {
                    let nav_order = lazecode_core::model::compare_nav_order(&files);
                    let selected = files.first().map(|file| file.path.clone());
                    project.compare_session = Some(lazecode_core::model::CompareSession {
                        from: commit_id.clone(),
                        to: String::new(),
                        local: true,
                        files,
                        nav_order,
                        group_by_dir: true,
                        root_expanded: true,
                        selected: selected.clone(),
                        ..Default::default()
                    });
                    if let Some(path) = selected {
                        let source = lazecode_core::DiffSource::CompareWithLocal {
                            commit: commit_id.clone(),
                            path: path.clone(),
                        };
                        project.open_diff_source(source.clone());
                        if let Ok(diff) = lazecode_git::diff_ref_worktree(&root, &commit_id, &path)
                        {
                            project.set_diff_for_source(&root, &source, Ok(diff));
                        }
                    }
                    model.ui.sidebar_view = SidebarView::Compare;
                    model.ui.sidebar_visible = true;
                }
            }
        }
        if let Some(commit) = project.log.first() {
            let id = commit.id.clone();
            project.selected_commit = Some(id.clone());
            project.commit_details = lazecode_git::commit_details(&project_root, &id).ok();
        }
        match mode.as_str() {
            "git" => {
                model.ui.panel_tab = PanelTab::Git;
                model.ui.panel_visible = true;
                model.ui.terminal_visible = false;
                model.ui.sidebar_view = SidebarView::Git;
            }
            "terminal" | "panes" => {
                model.ui.terminal_visible = true;
                model.ui.panel_visible = false;
                model.ui.panel_visible = true;
                model.ui.sidebar_view = SidebarView::Consoles;
            }
            "compare" => {
                // 分支比较页：无终端、无底部面板。
                model.ui.terminal_visible = false;
                model.ui.panel_visible = false;
                model.ui.sidebar_view = SidebarView::Files;
            }
            "cmpdiff" | "cmpdiff3" => {
                // 用户截图形态：Compare 抽屉 + Diff 标签 + Git 底部面板（无终端）。
                model.ui.sidebar_view = SidebarView::Compare;
                model.ui.sidebar_visible = true;
                model.ui.terminal_visible = false;
                model.ui.panel_visible = true;
                model.ui.panel_height = 460.0;
            }
            "compare_local" => {
                model.ui.sidebar_view = SidebarView::Compare;
                model.ui.sidebar_visible = true;
                model.ui.terminal_visible = false;
                model.ui.panel_visible = false;
            }
            _ => {
                model.ui.panel_visible = true;
                model.ui.terminal_visible = true;
                model.ui.panel_visible = false;
                model.ui.sidebar_view = SidebarView::Files;
            }
        }
        model.add_loaded_project(project);
        // Give the PTY a moment to print its prompt.
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    let mut runtime = UiRuntime::default();
    if let Some((tab_id, group)) = pending_jump {
        runtime.diff_scroll = Some((tab_id, group));
    }
    if mode == "update_failed" {
        runtime.update_open = true;
        runtime.update.error =
            Some("network error: failed to fetch the signed manifest: connection refused".into());
    } else if mode.starts_with("update") {
        runtime.update_open = true;
        let update_available = mode != "update_uptodate";
        runtime.update.available = Some(lazecode_core::updater::CheckOutcome {
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            manifest: lazecode_core::updater::Manifest {
                version: if update_available { "9.9.9".into() } else { env!("CARGO_PKG_VERSION").into() },
                url: "https://updates.example.test/LazeCode-9.9.9.app.tar.gz".into(),
                sha256: "aa".repeat(32),
                signature: String::new(),
                notes: "### Added\n\n- A proper update page with **Markdown** release notes.\n- The title-bar badge links here.\n\n### Fixed\n\n- Long notes scroll instead of clipping.\n\n---\n\n### 新增\n\n- 带 **Markdown** 发布说明的更新页面。\n- 顶栏提示直接打开本页。".into(),
                date: Some("2026-11-16".into()),
                min_os: None,
                arch: None,
                kind: "app-bundle".into(),
            },
            update_available,
        });
    }
    match mode.as_str() {
        "stash" | "stash_untracked" | "stash_collapsed" | "stash_menu" | "stash_file_menu"
        | "stash_dir_menu" | "stash_header_menu" | "unstash" => {
            // 贮藏页签：打开 Git 抽屉并切到 Stash。
            model.ui.sidebar_view = SidebarView::Git;
            model.ui.sidebar_visible = true;
            runtime.commit_tab = lazecode_ui::runtime::CommitTab::Stash;
            // 预加载选择（真实运行由抽屉首帧加载；快照需要确定性选择）。
            runtime.stash_entries = lazecode_git::stash_list(&project_root).unwrap_or_default();
            runtime.stash_loaded =
                Some(std::fs::canonicalize(&project_root).unwrap_or_else(|_| project_root.clone()));
            let selected = if mode == "stash_untracked" {
                "stash@{1}"
            } else {
                "stash@{0}"
            };
            runtime.stash_selected = Some(selected.to_string());
            if mode != "stash_collapsed" {
                runtime.stash_expanded.insert(selected.to_string());
            }
            if mode == "stash_menu" {
                runtime.menu = Some(lazecode_ui::runtime::MenuState {
                    kind: lazecode_ui::runtime::MenuKind::StashEntryMenu { project: 0 },
                    anchor: egui::Rect::from_min_size(
                        egui::pos2(150.0, 62.0),
                        egui::vec2(1.0, 1.0),
                    ),
                    payload: Some("stash@{0}".to_string()),
                    opened_frame: 0,
                });
            }
            if mode == "stash_file_menu" || mode == "stash_dir_menu" {
                let (kind, path, anchor_y) = if mode == "stash_file_menu" {
                    ("f", "backend/app/queue/pipeline_registry.py", 260.0)
                } else {
                    ("d", "backend", 300.0)
                };
                runtime.stash_file_sel = Some(path.to_string());
                runtime.menu = Some(lazecode_ui::runtime::MenuState {
                    kind: lazecode_ui::runtime::MenuKind::StashFileMenu { project: 0 },
                    anchor: egui::Rect::from_min_size(
                        egui::pos2(160.0, anchor_y),
                        egui::vec2(1.0, 1.0),
                    ),
                    payload: Some(format!("stash@{{0}}\u{1}{kind}\u{1}{path}")),
                    opened_frame: 0,
                });
            }
            if mode == "stash_header_menu" {
                // 抽屉级 ⋮ 菜单：与 Shelf/Commit 页签共用同一套条目。
                runtime.menu = Some(lazecode_ui::runtime::MenuState {
                    kind: lazecode_ui::runtime::MenuKind::CommitExtras,
                    anchor: egui::Rect::from_min_size(
                        egui::pos2(300.0, 60.0),
                        egui::vec2(1.0, 1.0),
                    ),
                    payload: None,
                    opened_frame: 0,
                });
            }
            if mode == "unstash" {
                runtime.dialog = lazecode_ui::runtime::DialogState::Unstash {
                    stash: "stash@{0}".to_string(),
                    message: runtime
                        .stash_entries
                        .first()
                        .map(|entry| entry.message())
                        .unwrap_or_default(),
                    branch: "feat-expert-mode".to_string(),
                    as_new_branch: String::new(),
                    pop: false,
                    reinstate_index: false,
                    error: None,
                };
            }
        }
        "close_terminal_dialog" => {
            runtime.dialog = lazecode_ui::runtime::DialogState::ConfirmTerminalClose {
                action: lazecode_ui::runtime::TerminalCloseAction::Pane(0),
                commands: vec!["omp".to_string()],
            };
        }
        "consoles_busy" => {
            model.ui.sidebar_view = SidebarView::Consoles;
            for project in &model.projects {
                for worktree in &project.worktrees {
                    for pane in worktree.terminals.iter().flat_map(|tab| tab.panes.iter()) {
                        runtime.terminal_busy.insert(
                            pane.id,
                            lazecode_ui::runtime::TerminalBusyState {
                                busy: true,
                                outputting: true,
                                commands: vec!["omp".to_string()],
                                last_output_ms: None,
                            },
                        );
                    }
                }
            }
        }
        "settings" => runtime.settings_open = true,
        "settings-editor" => {
            runtime.settings_open = true;
            model.ui.settings_section = 1;
        }
        "settings-terminal" => {
            runtime.settings_open = true;
            model.ui.settings_section = 2;
        }
        "settings-agents" => {
            runtime.settings_open = true;
            model.ui.settings_section = 3;
        }
        "settings-updates" => {
            runtime.settings_open = true;
            model.ui.settings_section = 6;
            model.settings.last_update_check_time = 1_763_260_000;
            model.settings.ignored_update_version = "0.4.5".into();
        }
        "agent_menu" => {
            runtime.open_agent_menu(egui::Rect::from_min_size(
                egui::pos2(230.0, 10.0),
                egui::vec2(1.0, 1.0),
            ));
        }
        "palette" => model.ui.command_palette_open = true,
        "search" => {
            runtime.search.open = true;
            runtime.search.query = "terminal".to_string();
            runtime.search.results = lazecode_fs::search_in_files(
                &project_root,
                "terminal",
                &lazecode_fs::SearchOptions {
                    max_results: 40,
                    ..Default::default()
                },
                None,
            )
            .unwrap_or_default();
            runtime.search.searched = true;
        }
        "branch_menu" => {
            runtime.open_menu(
                lazecode_ui::MenuKind::Branch,
                egui::Rect::from_min_size(egui::pos2(270.0, 10.0), egui::vec2(100.0, 24.0)),
                None,
            );
        }
        "worktree_menu" => {
            runtime.open_menu(
                lazecode_ui::MenuKind::Worktree,
                egui::Rect::from_min_size(egui::pos2(190.0, 10.0), egui::vec2(80.0, 24.0)),
                None,
            );
        }
        "remove_worktree_dialog" => {
            runtime.dialog = lazecode_ui::runtime::DialogState::RemoveWorktree {
                name: "feature-login".to_string(),
                worktree_path: project_root.join(".worktree").join("feature-login"),
                dirty: true,
                changes: 2,
            };
        }
        "new_worktree_dialog" => {
            runtime.dialog = lazecode_ui::runtime::DialogState::NewWorktree {
                name: "feature-login".to_string(),
                branch: String::new(),
                create_new: true,
                location: project_root
                    .join(".worktree")
                    .join("feature-login")
                    .display()
                    .to_string(),
                error: None,
            };
        }
        "push_dialog" => {
            let commit = |id: &str, message: &str, author: &str| lazecode_git::CommitInfo {
                id: id.to_string(),
                short: id[..7.min(id.len())].to_string(),
                message: message.to_string(),
                body: String::new(),
                author: author.to_string(),
                email: "dev@example.com".to_string(),
                time: 1_763_000_000,
                parents: Vec::new(),
                in_head: Some(true),
                refs: Vec::new(),
                lane: 0,
                lane_count: 1,
                edges: Vec::new(),
            };
            runtime.dialog = lazecode_ui::runtime::DialogState::Push {
                remote: "origin".to_string(),
                branch: "feat-expert-mode".to_string(),
                set_upstream: true,
                force: false,
                push_tags: false,
                remotes: vec![
                    lazecode_git::RemoteInfo {
                        name: "origin".to_string(),
                        url: "git@github.com:jiangtaiduo/LazeCode.git".to_string(),
                    },
                    lazecode_git::RemoteInfo {
                        name: "upstream".to_string(),
                        url: "https://github.com/example/LazeCode.git".to_string(),
                    },
                ],
                pending: vec![
                    commit(
                        "0123456789abcdef",
                        "feat(ui): split the context menus",
                        "Ann",
                    ),
                    commit("fedcba9876543210", "fix(git): keep merge base", "Bo"),
                ],
                status: None,
            };
        }
        "pull_dialog" => {
            runtime.dialog = lazecode_ui::runtime::DialogState::Pull {
                mode: lazecode_git::PullMode::Rebase,
                status: None,
            };
        }
        "remotes_dialog" => {
            runtime.dialog = lazecode_ui::runtime::DialogState::Remotes {
                remotes: vec![
                    lazecode_git::RemoteInfo {
                        name: "origin".to_string(),
                        url: "git@github.com:jiangtaiduo/LazeCode.git".to_string(),
                    },
                    lazecode_git::RemoteInfo {
                        name: "upstream".to_string(),
                        url: "https://github.com/example/LazeCode.git".to_string(),
                    },
                ],
                new_name: "backup".to_string(),
                new_url: "https://gitlab.com/example/lazecode.git".to_string(),
                editing: None,
                error: None,
            };
        }
        "reset_dialog" => {
            runtime.dialog = lazecode_ui::runtime::DialogState::Reset {
                hash: "0123456".to_string(),
                mode: lazecode_git::ResetMode::Mixed,
            };
        }
        "new_tag_dialog" => {
            runtime.dialog = lazecode_ui::runtime::DialogState::NewTag {
                commit: "0123456".to_string(),
                value: "v0.4.13".to_string(),
                error: None,
            };
        }
        "new_branch_dialog" => {
            runtime.dialog = lazecode_ui::runtime::DialogState::NewBranch {
                value: "feat/new-branch".to_string(),
                start_point: Some("main".to_string()),
                checkout: true,
                error: None,
            };
        }
        "new_file_dialog" => {
            runtime.dialog = lazecode_ui::runtime::DialogState::NewFile {
                dir: project_root.clone(),
                value: "notes.md".to_string(),
                error: None,
            };
        }
        "file_history_dialog" => {
            runtime.dialog = lazecode_ui::runtime::DialogState::FileHistory {
                path: "crates/lazecode-ui/src/overlays/dialogs.rs".to_string(),
                entries: Vec::new(),
                selected: 0,
                loaded: true,
            };
        }
        "single_menu" | "project_menu" => {
            runtime.open_menu(
                lazecode_ui::MenuKind::Project,
                egui::Rect::from_min_size(egui::pos2(70.0, 10.0), egui::vec2(120.0, 24.0)),
                None,
            );
        }
        _ => {}
    }
    let ctx = egui::Context::default();
    lazecode_ui::install_fonts(&ctx, &settings);
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(W, H));
    let mut viewports = egui::ViewportIdMap::default();
    viewports.insert(
        egui::ViewportId::ROOT,
        egui::ViewportInfo {
            native_pixels_per_point: Some(SCALE),
            inner_rect: Some(rect),
            outer_rect: Some(rect),
            focused: Some(true),
            ..Default::default()
        },
    );
    let raw = egui::RawInput {
        screen_rect: Some(rect),
        viewports,
        time: Some(1.0),
        ..Default::default()
    };
    // `LAZE_POINTER=x,y` (logical points) hovers a widget; a second frame lets
    // egui's tooltip delay elapse so hover-only surfaces land in the capture.
    let hover = std::env::var("LAZE_POINTER").ok().and_then(|raw| {
        let (x, y) = raw.split_once(',')?;
        Some(egui::pos2(x.trim().parse().ok()?, y.trim().parse().ok()?))
    });
    let input_for = |time: f64, move_pointer: bool| {
        let mut input = raw.clone();
        input.time = Some(time);
        if move_pointer {
            if let Some(pos) = hover {
                input.events = vec![egui::Event::PointerMoved(pos)];
            }
        }
        input
    };
    let mut earlier_textures = Vec::new();
    let mut output = ctx.run_ui(input_for(1.0, true), |ui| {
        lazecode_ui::draw(ui, &mut model, &mut runtime);
    });
    if hover.is_some() {
        // Keep the pointer still: egui only opens hover tooltips once the
        // pointer has settled. The first frame that opens a tooltip is a
        // sizing pass, so a third frame actually paints it.
        for time in [2.0, 3.0] {
            earlier_textures.push(output.textures_delta);
            output = ctx.run_ui(input_for(time, false), |ui| {
                lazecode_ui::draw(ui, &mut model, &mut runtime);
            });
        }
    }
    let width = (W * SCALE) as u32;
    let height = (H * SCALE) as u32;
    let pixels = render(&ctx, output, earlier_textures, width, height)?;

    save_png(&output_path, width, height, &pixels)?;
    println!("wrote {}", output_path.display());
    Ok(())
}

fn render(
    ctx: &egui::Context,
    mut output: egui::FullOutput,
    mut earlier_textures: Vec<egui::TexturesDelta>,
    width: u32,
    height: u32,
) -> anyhow::Result<Vec<u8>> {
    use egui_wgpu::wgpu;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .map_err(|err| anyhow::anyhow!("no adapter: {err}"))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("lazecode-snapshot"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        ..Default::default()
    }))?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("snapshot"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut renderer =
        egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());
    let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
    eprintln!("primitives={}", primitives.len());
    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [width, height],
        pixels_per_point: output.pixels_per_point,
    };
    let mut apply_textures = |batch: &egui::TexturesDelta| {
        for (id, deltas) in &batch.set {
            for delta in deltas {
                renderer.update_texture(&device, &queue, *id, delta);
            }
        }
        for id in &batch.free {
            renderer.free_texture(id);
        }
    };
    for batch in &mut earlier_textures {
        apply_textures(batch);
        batch.clear();
    }
    apply_textures(&output.textures_delta);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("snapshot"),
    });
    renderer.update_buffers(&device, &queue, &mut encoder, &primitives, &screen);
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    {
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.043,
                            g: 0.055,
                            b: 0.067,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            })
            .forget_lifetime();
        renderer.render(&mut pass, &primitives, &screen);
        drop(pass);
    }
    let bytes_per_row = (width * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("snapshot-readback"),
        size: (bytes_per_row * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));
    output.textures_delta.clear();
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    rx.recv()??;
    let data = slice
        .get_mapped_range()
        .map_err(|err| anyhow::anyhow!("map readback: {err}"))?;
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height as usize {
        let start = row * bytes_per_row as usize;
        pixels.extend_from_slice(&data[start..start + width as usize * 4]);
    }
    drop(data);
    buffer.unmap();
    Ok(pixels)
}

fn save_png(path: &Path, width: u32, height: u32, pixels: &[u8]) -> anyhow::Result<()> {
    let file = std::fs::File::create(path)?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(pixels)?;
    Ok(())
}
