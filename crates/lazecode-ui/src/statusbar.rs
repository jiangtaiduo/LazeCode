use egui::{Align2, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use lazecode_core::{AppModel, PanelTab, SidebarView};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::UiRuntime;
use crate::theme::{Theme, TINY};
use crate::widgets::{live_dot, text_clipped};

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    let painter = ui.painter();
    let font = FontId::proportional(12.0);
    let y = rect.center().y;
    let mut x = rect.left() + 10.0;

    let (project_name, branch, ahead, behind, changes, active_file_info, diagnostic_counts) = {
        let project = model.active_project();
        let active_file_info = project.and_then(|project| {
            let id = project.active_tab?;
            let tab = project.tab_by_id(id)?;
            match &tab.kind {
                lazecode_core::TabKind::File(file) => Some((
                    file.doc
                        .highlighter
                        .language_name()
                        .unwrap_or("Plain Text")
                        .to_string(),
                    file.doc.eol.as_str().to_string(),
                    file.doc.encoding_label(),
                    file.doc.cursor.line + 1,
                    file.doc.cursor.col + 1,
                )),
                _ => None,
            }
        });
        let diagnostic_counts = project.and_then(|project| {
            let id = project.active_tab?;
            let tab = project.tab_by_id(id)?;
            match &tab.kind {
                lazecode_core::TabKind::File(file) => {
                    Some(model.lsp.diagnostic_counts(&file.doc.path))
                }
                _ => None,
            }
        });
        (
            project
                .map(|project| project.name.clone())
                .unwrap_or_default(),
            project.map(|project| project.branch()).unwrap_or_default(),
            project
                .and_then(|project| project.git.as_ref())
                .map(|git| git.ahead)
                .unwrap_or(0),
            project
                .and_then(|project| project.git.as_ref())
                .map(|git| git.behind)
                .unwrap_or(0),
            project.map(|project| project.changes().len()).unwrap_or(0),
            active_file_info,
            diagnostic_counts,
        )
    };
    let running = rt.busy_count();
    let has_project = model.active_project().is_some();

    if !has_project {
        ui.painter().text(
            Pos2::new(rect.left() + 12.0, y),
            Align2::LEFT_CENTER,
            crate::tr!(model, "No project open", "未打开项目"),
            font.clone(),
            p.text3,
        );
        ui.painter().text(
            Pos2::new(rect.right() - 12.0, y),
            Align2::RIGHT_CENTER,
            format!("LazeCode {}", model.version),
            FontId::monospace(11.0),
            p.text3,
        );
        return;
    }

    // Worktree / project.
    paint_icon(
        painter,
        Rect::from_center_size(Pos2::new(x + 7.0, y), Vec2::splat(13.0)),
        Icon::Worktree,
        p.text3,
    );
    x += 18.0;
    let name_width = 90.0;
    text_clipped(
        painter,
        Rect::from_min_max(
            Pos2::new(x, rect.top()),
            Pos2::new(x + name_width, rect.bottom()),
        ),
        &project_name,
        font.clone(),
        p.text3,
        Align2::LEFT_CENTER,
    );
    x += name_width + 2.0;

    painter.vline(
        x,
        (rect.top() + 6.0)..=(rect.bottom() - 6.0),
        Stroke::new(1.0, p.border_soft),
    );
    x += 9.0;

    // Branch (click opens the branch menu).
    let branch_rect = Rect::from_min_max(
        Pos2::new(x - 3.0, rect.top() + 2.0),
        Pos2::new(
            x + 18.0 + branch.chars().count() as f32 * 6.0 + 40.0,
            rect.bottom() - 2.0,
        ),
    );
    let response = ui.interact(
        branch_rect,
        ui.scope_id().with("status-branch"),
        Sense::click(),
    );
    if response.hovered() {
        painter.rect_filled(branch_rect, egui::CornerRadius::same(4), p.hover);
    }
    paint_icon(
        ui.painter(),
        Rect::from_center_size(Pos2::new(x + 6.0, y), Vec2::splat(12.0)),
        Icon::Git,
        p.text3,
    );
    x += 16.0;
    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(x, rect.top()),
            Pos2::new(x + 180.0, rect.bottom()),
        ),
        &branch,
        font.clone(),
        p.text2,
        Align2::LEFT_CENTER,
    );
    x += branch.chars().count() as f32 * 6.0 + 12.0;
    if ahead > 0 {
        painter.text(
            Pos2::new(x, y),
            Align2::LEFT_CENTER,
            format!("↑{ahead}"),
            FontId::monospace(11.0),
            p.green,
        );
        x += 24.0;
    }
    if behind > 0 {
        painter.text(
            Pos2::new(x, y),
            Align2::LEFT_CENTER,
            format!("↓{behind}"),
            FontId::monospace(11.0),
            p.orange,
        );
    }
    if response.clicked() {
        rt.toggle_menu(crate::runtime::MenuKind::Branch, branch_rect, None);
    }

    let mut x = branch_rect.right() + 10.0;
    painter.vline(
        x,
        (rect.top() + 6.0)..=(rect.bottom() - 6.0),
        Stroke::new(1.0, p.border_soft),
    );
    x += 9.0;

    // Running processes.
    let running_rect = Rect::from_min_max(
        Pos2::new(x, rect.top() + 2.0),
        Pos2::new(x + 78.0, rect.bottom() - 2.0),
    );
    let response = ui.interact(
        running_rect,
        ui.scope_id().with("status-running"),
        Sense::click(),
    );
    if response.hovered() {
        painter.rect_filled(running_rect, egui::CornerRadius::same(4), p.hover);
    }
    live_dot(
        ui.painter(),
        Pos2::new(x + 6.0, y),
        running > 0,
        if running > 0 { p.running } else { p.text3 },
    );
    painter.text(
        Pos2::new(x + 15.0, y),
        Align2::LEFT_CENTER,
        if model.settings.language.is_zh() {
            format!("{} 个运行中", running)
        } else {
            format!("{running} running")
        },
        font.clone(),
        if running > 0 { p.text2 } else { p.text3 },
    );
    if response.clicked() {
        model.ui.sidebar_view = SidebarView::Consoles;
        model.ui.sidebar_visible = true;
        model.ui.panel_visible = true;
        model.ui.terminal_visible = true;
        model.ui.panel_visible = false;
    }

    // Right side: measure each label so nothing overlaps on any window width.
    let language = active_file_info
        .as_ref()
        .map(|info| info.0.clone())
        .unwrap_or_else(|| "—".to_string());
    let eol = active_file_info
        .as_ref()
        .map(|info| info.1.clone())
        .unwrap_or_else(|| "LF".to_string());
    let encoding = active_file_info
        .as_ref()
        .map(|info| info.2.clone())
        .unwrap_or_else(|| "UTF-8".to_string());
    let mut rx = rect.right() - 10.0;
    let mut right_items: Vec<(String, FontId, egui::Color32)> = Vec::new();
    if changes > 0 {
        right_items.push((
            if model.settings.language.is_zh() {
                format!("{} 个变更", changes)
            } else {
                format!("{changes} changed")
            },
            font.clone(),
            p.orange,
        ));
    }
    if let Some((errors, warnings)) = diagnostic_counts {
        if errors > 0 {
            right_items.push((format!("✕ {errors}"), font.clone(), p.rose));
        }
        if warnings > 0 {
            right_items.push((format!("⚠ {warnings}"), font.clone(), p.amber));
        }
    }
    if let Some(info) = &active_file_info {
        right_items.push((
            if model.settings.language.is_zh() {
                format!("行 {}, 列 {}", info.3, info.4)
            } else {
                format!("Ln {}, Col {}", info.3, info.4)
            },
            FontId::monospace(11.5),
            p.text3,
        ));
    }
    right_items.push((encoding.clone(), font.clone(), p.text3));
    right_items.push((eol.clone(), FontId::monospace(11.0), p.text3));
    right_items.push((language.clone(), font.clone(), p.text3));
    for (label, item_font, color) in right_items.into_iter().rev() {
        let width = crate::widgets::text_width(ui.painter(), &label, item_font.clone());
        let left = rx - width;
        ui.painter().text(
            Pos2::new(left, y),
            Align2::LEFT_CENTER,
            &label,
            item_font,
            color,
        );
        rx = left - 14.0;
    }

    let _ = (TINY, rt, PanelTab::Git);
}
