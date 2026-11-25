//! Welcome tab: hero, shortcuts, recent projects and actions.

use super::*;

fn welcome_action_button(
    ui: &mut Ui,
    rect: Rect,
    id: &'static str,
    label: &str,
    icon: Icon,
    primary: bool,
    theme: &Theme,
) -> egui::Response {
    let p = theme.palette;
    let response = ui.interact(
        rect,
        ui.scope_id().with(("welcome-action", id)),
        Sense::click(),
    );
    let hovered = response.hovered();
    let (fill, text_color) = if primary {
        (
            if hovered {
                p.accent.gamma_multiply(1.06)
            } else {
                p.accent
            },
            p.accent_on_fill,
        )
    } else {
        (
            if hovered {
                p.hover
            } else {
                Color32::TRANSPARENT
            },
            p.text2,
        )
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(6), fill);
    if !primary {
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(6),
            Stroke::new(1.0, p.border),
            StrokeKind::Inside,
        );
    }
    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(rect.left() + 16.0, rect.center().y),
            Vec2::splat(14.0),
        ),
        icon,
        if primary { text_color } else { p.text2 },
    );
    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(rect.left() + 34.0, rect.top()),
            Pos2::new(rect.right() - 8.0, rect.bottom()),
        ),
        label,
        FontId::proportional(12.5),
        text_color,
        Align2::LEFT_CENTER,
    );
    response
}

fn welcome_kbd(painter: &egui::Painter, rect: Rect, label: &str, theme: &Theme) {
    let p = theme.palette;
    painter.rect_filled(rect, egui::CornerRadius::same(4), p.panel2);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(4),
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    text_clipped(
        painter,
        rect.shrink2(Vec2::new(4.0, 0.0)),
        label,
        FontId::proportional(13.0),
        p.text,
        Align2::CENTER_CENTER,
    );
}

fn welcome_recent_card(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    title: &str,
    empty: &str,
    items: &[(PathBuf, bool)],
) -> Option<(PathBuf, bool)> {
    let p = theme.palette;
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(8), p.panel);
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, rect.top() + 18.0),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(13.0),
        p.text,
    );
    if items.is_empty() {
        ui.painter().text(
            Pos2::new(rect.left() + 14.0, rect.top() + 44.0),
            Align2::LEFT_CENTER,
            empty,
            FontId::proportional(12.5),
            p.text3,
        );
        return None;
    }
    let mut clicked = None;
    for (index, (path, is_project)) in items.iter().take(5).enumerate() {
        let row_y = rect.top() + 32.0 + index as f32 * 27.0;
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, row_y),
            Vec2::new(rect.width() - 16.0, 24.0),
        );
        let response = ui.interact(
            row,
            ui.scope_id()
                .with(("welcome-recent", path.clone(), *is_project)),
            Sense::click(),
        );
        if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(4), p.hover);
        }
        let (icon, tone) = if *is_project {
            (Icon::Folder, p.accent)
        } else {
            tab_icon(path.to_str().unwrap_or_default(), theme)
        };
        paint_icon(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(row.left() + 10.0, row.center().y),
                Vec2::splat(12.0),
            ),
            icon,
            tone,
        );
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(row.left() + 22.0, row.top()),
                Pos2::new(row.right() - 8.0, row.bottom()),
            ),
            &name,
            FontId::proportional(13.0),
            p.text,
            Align2::LEFT_CENTER,
        );
        let response = response.on_hover_text(path.display().to_string());
        if response.clicked() {
            clicked = Some((path.clone(), *is_project));
        }
    }
    clicked
}

pub(super) fn draw_welcome(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    rect: Rect,
    index: Option<usize>,
) {
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.editor);
    let card = Rect::from_center_size(
        rect.center(),
        Vec2::new(
            720.0_f32.min(rect.width() - 60.0),
            460.0_f32.min(rect.height() - 60.0),
        ),
    );
    let left = card.left() + 34.0;
    let mut y = card.top() + 40.0;

    // Use the packaged application icon rather than a synthetic `>_` tile.
    let mark = Rect::from_min_size(Pos2::new(left, y), Vec2::splat(84.0));
    let icon_texture = rt.app_icon_texture(ui.ctx());
    ui.painter().image(
        icon_texture,
        mark,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
    y += 96.0;
    let name = index
        .and_then(|index| model.projects.get(index))
        .map(|project| project.name.clone())
        .unwrap_or_else(|| "LazeCode".to_string());
    ui.painter().text(
        Pos2::new(left, y),
        Align2::LEFT_CENTER,
        &name,
        FontId::proportional(24.0),
        p.text,
    );
    y += 28.0;
    let subtitle = if index.is_some() {
        if model.settings.language.is_zh() {
            format!("{} 个项目已打开。所有进程一目了然。", model.projects.len())
        } else {
            format!(
                "{} projects open. Every process in view.",
                model.projects.len()
            )
        }
    } else {
        crate::tr!(
            model,
            "A quiet place for many projects. Open a folder or files to begin.",
            "一个安静的工作空间。打开目录或文件即可开始。"
        )
        .to_string()
    };
    ui.painter().text(
        Pos2::new(left, y),
        Align2::LEFT_CENTER,
        subtitle,
        FontId::proportional(13.5),
        p.text2,
    );
    y += 30.0;

    let actions = [
        (
            crate::tr!(model, "Open", "打开"),
            Icon::FolderOpen,
            "open-project",
        ),
        (
            crate::tr!(model, "Clone repo", "克隆仓库"),
            Icon::Worktree,
            "clone-repo",
        ),
        (
            crate::tr!(model, "New file", "新建文件"),
            Icon::Plus,
            "new-file",
        ),
        (
            crate::tr!(model, "New console", "新建控制台"),
            Icon::Terminal,
            "new-console",
        ),
        (
            crate::tr!(model, "Find in files", "在文件中查找"),
            Icon::Search,
            "search",
        ),
    ];
    let mut x = left;
    // Keep the five actions on one row on narrow windows as well.
    let action_width = ((card.width() - 68.0 - 32.0) / 5.0).clamp(96.0, 124.0);
    for (label, icon, id) in actions {
        let width = action_width;
        let button = Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, 30.0));
        let primary = id == "open-project";
        let response = welcome_action_button(ui, button, id, label, icon, primary, theme);
        if response.clicked() {
            match id {
                "open-project" => model.queue(Command::OpenProjectDialog),
                "clone-repo" => {
                    rt.dialog = crate::runtime::DialogState::CloneRepo {
                        url: String::new(),
                        dest: String::new(),
                        error: None,
                    };
                }
                "new-file" => {
                    let dir = model
                        .active_project()
                        .map(|p| p.active_root().to_path_buf())
                        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")));
                    rt.dialog = crate::runtime::DialogState::NewFile {
                        dir,
                        value: String::new(),
                        error: None,
                    };
                }
                "new-console" => model.queue(Command::NewTerminal),
                _ => {
                    rt.search.open = true;
                    rt.search.focus_requested = true;
                }
            }
        }
        x += width + 8.0;
    }
    y += 52.0;

    // Recent / open files and shortcuts, two columns when there is room.
    let column_w = (card.width() - 88.0) * 0.56;
    let recent_rect = Rect::from_min_size(Pos2::new(left, y), Vec2::new(column_w, 168.0));
    ui.painter()
        .rect_filled(recent_rect, egui::CornerRadius::same(8), p.panel);
    let home_groups = index.is_none();
    let left_title = if home_groups {
        crate::tr!(model, "Recent projects", "最近项目")
    } else {
        crate::tr!(model, "Open files", "打开的文件")
    };
    let left_empty = if home_groups {
        crate::tr!(model, "No recent projects", "暂无最近项目")
    } else {
        crate::tr!(
            model,
            "Click a file in the explorer",
            "在资源管理器中点击文件"
        )
    };
    let left_items: Vec<(PathBuf, bool)> = if let Some(index) = index {
        model.projects[index]
            .active_worktree_tabs()
            .filter_map(|tab| match &tab.kind {
                TabKind::File(file) => Some((file.doc.path.clone(), false)),
                TabKind::Image(img) => Some((img.path.clone(), false)),
                _ => None,
            })
            .take(5)
            .collect()
    } else {
        model
            .recent
            .iter()
            .take(5)
            .cloned()
            .map(|path| (path, true))
            .collect()
    };
    if let Some((path, is_project)) =
        welcome_recent_card(ui, theme, recent_rect, left_title, left_empty, &left_items)
    {
        if is_project {
            model.queue(Command::OpenProject(path));
        } else if let Some(active) = model.active_project {
            let _ = model.open_file_in(active, path);
        } else if let Err(err) = model.open_files(vec![path]) {
            model.toast(err, lazecode_core::ToastKind::Error);
        }
    }

    let right_rect = Rect::from_min_size(
        Pos2::new(recent_rect.right() + 16.0, y),
        Vec2::new(card.right() - 34.0 - (recent_rect.right() + 16.0), 168.0),
    );
    if home_groups {
        let recent_files: Vec<(PathBuf, bool)> = model
            .recent_files
            .iter()
            .take(5)
            .cloned()
            .map(|path| (path, false))
            .collect();
        if let Some((path, _)) = welcome_recent_card(
            ui,
            theme,
            right_rect,
            crate::tr!(model, "Recent files", "最近文件"),
            crate::tr!(model, "No recent files", "暂无最近文件"),
            &recent_files,
        ) {
            if let Err(err) = model.open_files(vec![path]) {
                model.toast(err, lazecode_core::ToastKind::Error);
            }
        }
    }

    let shortcuts = right_rect;
    if !home_groups {
        ui.painter().text(
            Pos2::new(shortcuts.left() + 14.0, shortcuts.top() + 18.0),
            Align2::LEFT_CENTER,
            crate::tr!(model, "Keyboard", "快捷键"),
            FontId::proportional(13.0),
            p.text,
        );
        let entries = [
            (crate::tr!(model, "Command palette", "命令面板"), "⌘⇧A"),
            (crate::tr!(model, "Toggle explorer", "切换资源管理器"), "⌘1"),
            (crate::tr!(model, "Commit", "提交"), "⌘K"),
            (crate::tr!(model, "Open Git log", "打开 Git 日志"), "⌘9"),
            (crate::tr!(model, "Find in files", "在文件中查找"), "⌘⇧F"),
        ];
        let key_font = FontId::proportional(13.0);
        let mut row_y = shortcuts.top() + 36.0;
        for (label, key) in entries.iter() {
            let key_width = text_width(ui.painter(), key, key_font.clone()) + 16.0;
            let kbd_rect = Rect::from_center_size(
                Pos2::new(shortcuts.right() - 14.0 - key_width * 0.5, row_y),
                Vec2::new(key_width, 22.0),
            );
            text_clipped(
                ui.painter(),
                Rect::from_min_max(
                    Pos2::new(shortcuts.left() + 14.0, row_y - 10.0),
                    Pos2::new(kbd_rect.left() - 8.0, row_y + 10.0),
                ),
                label,
                FontId::proportional(12.5),
                p.text2,
                Align2::LEFT_CENTER,
            );
            welcome_kbd(ui.painter(), kbd_rect, key, theme);
            row_y += 27.0;
        }
    }

    let foot_y = card.bottom() - 20.0;
    ui.painter().text(
        Pos2::new(left, foot_y),
        Align2::LEFT_CENTER,
        if model.settings.language.is_zh() {
            format!(
                "{} 个运行中 · {} 个项目",
                rt.busy_count(),
                model.projects.len()
            )
        } else {
            format!(
                "{} running across {} projects",
                rt.busy_count(),
                model.projects.len()
            )
        },
        FontId::proportional(12.0),
        p.text3,
    );
    if let Some(index) = index {
        ui.painter().text(
            Pos2::new(card.right() - 34.0, foot_y),
            Align2::RIGHT_CENTER,
            format!(
                "{} · {}",
                model.projects[index].name,
                model.projects[index].branch()
            ),
            FontId::proportional(10.0),
            p.text3,
        );
    }
    let _ = rt;
}
