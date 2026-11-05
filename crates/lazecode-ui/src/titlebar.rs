use egui::{
    Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, ViewportCommand,
};
use lazecode_core::{AppModel, Command, PanelTab};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::{MenuKind, MenuState, UiRuntime};
use crate::theme::Theme;
use crate::widgets::{icon_button, live_dot, text_clipped};

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    let painter = ui.painter().clone();
    let top = rect.top();

    // Window dragging on the empty titlebar surface.
    let drag_rect = Rect::from_min_max(rect.min, Pos2::new(rect.left() + 6.0, rect.bottom()));
    let drag = ui.interact(
        drag_rect,
        ui.scope_id().with("title-drag-left"),
        Sense::click_and_drag(),
    );
    if drag.dragged() {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if drag.double_clicked() {
        let maximized = ui
            .ctx()
            .input(|input| input.viewport().maximized.unwrap_or(false));
        ui.ctx()
            .send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    }

    let mut x = rect.left() + 12.0;
    let mut open_project_dialog = false;
    let project = model.active_project();
    let has_project = project.is_some();
    let project_name = project
        .map(|project| project.name.clone())
        .unwrap_or_else(|| crate::tr!(model, "Open a project", "打开项目").to_string());
    let project_path = project
        .map(|project| lazecode_fs::display_path(&project.root))
        .unwrap_or_default();
    let initials = project
        .map(|project| project.initials.clone())
        .unwrap_or_default();
    let tone = project
        .map(|project| Color32::from_rgb(project.tone.0, project.tone.1, project.tone.2))
        .unwrap_or(p.accent);

    let project_rect = Rect::from_min_size(Pos2::new(x, top + 6.0), Vec2::new(238.0, 28.0));
    let response = ui.interact(
        project_rect,
        ui.scope_id().with("project-id"),
        Sense::click(),
    );
    if response.hovered() {
        painter.rect_filled(project_rect, egui::CornerRadius::same(6), p.hover);
    }
    let badge = Rect::from_center_size(
        Pos2::new(x + 15.0, project_rect.center().y),
        Vec2::splat(22.0),
    );
    painter.rect_filled(
        badge,
        egui::CornerRadius::same(6),
        tone.gamma_multiply(0.16),
    );
    if has_project {
        painter.text(
            badge.center(),
            Align2::CENTER_CENTER,
            &initials,
            FontId::proportional(9.5),
            tone,
        );
    } else {
        paint_icon(ui.painter(), badge.shrink(5.0), Icon::FolderOpen, p.text3);
    }
    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(x + 32.0, top + 7.0),
            Pos2::new(x + 150.0, top + 22.0),
        ),
        &project_name,
        FontId::proportional(12.5),
        p.text,
        Align2::LEFT_CENTER,
    );
    text_clipped(
        ui.painter(),
        Rect::from_min_max(
            Pos2::new(x + 32.0, top + 21.0),
            Pos2::new(x + 198.0, top + 34.0),
        ),
        &project_path,
        FontId::proportional(9.5),
        p.text3,
        Align2::LEFT_CENTER,
    );
    paint_icon(
        ui.painter(),
        Rect::from_center_size(
            Pos2::new(project_rect.right() - 14.0, project_rect.center().y),
            Vec2::splat(12.0),
        ),
        Icon::ChevronDown,
        p.text3,
    );
    if response.clicked() {
        if has_project {
            rt.menu = Some(MenuState {
                kind: MenuKind::Project,
                anchor: project_rect,
                payload: None,
            });
        } else {
            open_project_dialog = true;
        }
    }
    x = project_rect.right() + 7.0;

    painter.vline(
        x - 3.5,
        (top + 10.0)..=(rect.bottom() - 10.0),
        Stroke::new(1.0, p.border),
    );

    // Branch chip only exists with an open project.
    let mut left_end = x;
    if project.is_some() {
        let branch = project
            .map(|project| project.branch())
            .unwrap_or_else(|| crate::tr!(model, "no repository", "无仓库").to_string());
        let ahead = project
            .and_then(|project| project.git.as_ref())
            .map(|git| git.ahead)
            .unwrap_or(0);
        let behind = project
            .and_then(|project| project.git.as_ref())
            .map(|git| git.behind)
            .unwrap_or(0);
        let branch_width = (branch.chars().count() as f32 * 6.4
            + 58.0
            + usize::from(ahead > 0) as f32 * 24.0
            + usize::from(behind > 0) as f32 * 24.0)
            .clamp(90.0, 236.0);
        let branch_rect =
            Rect::from_min_size(Pos2::new(x + 4.0, top + 7.0), Vec2::new(branch_width, 26.0));
        let response = ui.interact(
            branch_rect,
            ui.scope_id().with("branch-chip"),
            Sense::click(),
        );
        painter.rect_filled(branch_rect, egui::CornerRadius::same(6), p.panel2);
        painter.rect_stroke(
            branch_rect,
            egui::CornerRadius::same(6),
            Stroke::new(
                1.0,
                if response.hovered() {
                    p.border_strong
                } else {
                    p.border
                },
            ),
            StrokeKind::Inside,
        );
        paint_icon(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(branch_rect.left() + 13.0, branch_rect.center().y),
                Vec2::splat(14.0),
            ),
            Icon::Git,
            p.accent,
        );
        let mut bx = branch_rect.left() + 22.0;
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(bx, branch_rect.top()),
                Pos2::new(branch_rect.right() - 36.0, branch_rect.bottom()),
            ),
            &branch,
            FontId::proportional(11.5),
            p.text2,
            Align2::LEFT_CENTER,
        );
        bx = branch_rect.right() - 34.0;
        if ahead > 0 {
            painter.text(
                Pos2::new(bx, branch_rect.center().y),
                Align2::LEFT_CENTER,
                format!("↑{ahead}"),
                FontId::monospace(9.0),
                p.green,
            );
            bx += 20.0;
        }
        if behind > 0 {
            painter.text(
                Pos2::new(bx, branch_rect.center().y),
                Align2::LEFT_CENTER,
                format!("↓{behind}"),
                FontId::monospace(9.0),
                p.orange,
            );
        }
        paint_icon(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(branch_rect.right() - 12.0, branch_rect.center().y),
                Vec2::splat(11.0),
            ),
            Icon::ChevronDown,
            p.text3,
        );
        if response.clicked() && has_project {
            rt.menu = Some(MenuState {
                kind: MenuKind::Branch,
                anchor: branch_rect,
                payload: None,
            });
        }
        // Worktree chip next to the branch chip.
        let wt_width = 66.0;
        let worktree_rect = Rect::from_min_size(
            Pos2::new(branch_rect.right() + 4.0, top + 7.0),
            Vec2::new(wt_width, 26.0),
        );
        let worktree_response = ui.interact(
            worktree_rect,
            ui.scope_id().with("worktree-chip"),
            Sense::click(),
        );
        painter.rect_filled(
            worktree_rect,
            egui::CornerRadius::same(6),
            if worktree_response.hovered() {
                p.hover
            } else {
                p.panel2
            },
        );
        paint_icon(
            &painter,
            Rect::from_center_size(
                Pos2::new(worktree_rect.left() + 12.0, worktree_rect.center().y),
                Vec2::splat(11.0),
            ),
            Icon::Worktree,
            p.text3,
        );
        let worktree_name = project
            .map(|project| project.active_worktree().name.clone())
            .unwrap_or_else(|| "main".to_string());
        text_clipped(
            &painter,
            Rect::from_min_max(
                Pos2::new(worktree_rect.left() + 22.0, worktree_rect.top()),
                Pos2::new(worktree_rect.right() - 6.0, worktree_rect.bottom()),
            ),
            &worktree_name,
            FontId::proportional(10.5),
            p.text2,
            Align2::LEFT_CENTER,
        );
        if worktree_response.clicked() && has_project {
            rt.menu = Some(MenuState {
                kind: MenuKind::Worktree,
                anchor: worktree_rect,
                payload: None,
            });
        }
        worktree_response.on_hover_text(crate::tr!(
            model,
            "Switch worktree · create linked checkouts with independent consoles",
            "切换工作树 · 可创建独立控制台的关联检出"
        ));
        left_end = worktree_rect.right() + 8.0;
    }

    // Right cluster.
    let mut rx = rect.right() - 8.0;
    let panel_rect = Rect::from_min_size(Pos2::new(rx - 28.0, top + 7.0), Vec2::splat(26.0));
    let panel_active = model.ui.panel_visible;
    if icon_button(ui, panel_rect, Icon::Panel, panel_active, theme).clicked() {
        model.ui.panel_visible = !model.ui.panel_visible;
    }
    rx -= 30.0;
    let sidebar_rect = Rect::from_min_size(Pos2::new(rx - 28.0, top + 7.0), Vec2::splat(26.0));
    if icon_button(
        ui,
        sidebar_rect,
        Icon::Sidebar,
        model.ui.sidebar_visible,
        theme,
    )
    .clicked()
    {
        model.ui.sidebar_visible = !model.ui.sidebar_visible;
    }
    rx -= 36.0;
    painter.vline(
        rx,
        (top + 11.0)..=(rect.bottom() - 11.0),
        Stroke::new(1.0, p.border),
    );
    rx -= 7.0;

    let total = model.total_running();
    let running_rect = Rect::from_min_size(Pos2::new(rx - 92.0, top + 8.0), Vec2::new(92.0, 24.0));
    let response = ui.interact(
        running_rect,
        ui.scope_id().with("running-chip"),
        Sense::click(),
    );
    if response.hovered() {
        painter.rect_filled(running_rect, egui::CornerRadius::same(6), p.hover);
    }
    live_dot(
        ui.painter(),
        Pos2::new(running_rect.left() + 13.0, running_rect.center().y),
        total > 0,
        if total > 0 { p.running } else { p.text3 },
    );
    painter.text(
        Pos2::new(running_rect.left() + 24.0, running_rect.center().y),
        Align2::LEFT_CENTER,
        if model.settings.language.is_zh() {
            format!("{total} 个运行中")
        } else {
            format!("{total} running")
        },
        FontId::proportional(11.0),
        if total > 0 { p.text2 } else { p.text3 },
    );
    if response.clicked() {
        model.ui.sidebar_view = lazecode_core::SidebarView::Consoles;
        model.ui.sidebar_visible = true;
        model.ui.panel_visible = true;
        model.ui.panel_tab = PanelTab::Console;
    }
    rx = running_rect.left() - 8.0;

    // Search bar centred in the remaining space.
    let available_left = left_end;
    let available_right = rx;
    let available = (available_right - available_left).max(0.0);
    let search_width = (available - 24.0).clamp(120.0, 520.0);
    if available > 140.0 {
        let search_rect = Rect::from_center_size(
            Pos2::new((available_left + available_right) * 0.5, rect.center().y),
            Vec2::new(search_width, 26.0),
        );
        let response = ui.interact(search_rect, ui.scope_id().with("searchbar"), Sense::click());
        let fill = if response.hovered() { p.elev } else { p.panel2 };
        painter.rect_filled(search_rect, egui::CornerRadius::same(6), fill);
        painter.rect_stroke(
            search_rect,
            egui::CornerRadius::same(6),
            Stroke::new(
                1.0,
                if response.hovered() {
                    p.border_strong
                } else {
                    p.border
                },
            ),
            StrokeKind::Inside,
        );
        paint_icon(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(search_rect.left() + 15.0, search_rect.center().y),
                Vec2::splat(13.0),
            ),
            Icon::Search,
            p.text3,
        );
        text_clipped(
            ui.painter(),
            Rect::from_min_max(
                Pos2::new(search_rect.left() + 27.0, search_rect.top()),
                Pos2::new(search_rect.right() - 44.0, search_rect.bottom()),
            ),
            crate::tr!(
                model,
                "Search files, projects, commands",
                "搜索文件、项目与命令"
            ),
            FontId::proportional(11.5),
            p.text3,
            Align2::LEFT_CENTER,
        );
        let kbd_rect = Rect::from_center_size(
            Pos2::new(search_rect.right() - 23.0, search_rect.center().y),
            Vec2::new(28.0, 17.0),
        );
        crate::widgets::kbd(ui.painter(), kbd_rect, "⌘K", theme);
        if response.clicked() {
            rt.open_palette();
            model.ui.command_palette_open = true;
        }
    }

    let _ = MenuKind::Project;
    if open_project_dialog {
        model.queue(Command::OpenProjectDialog);
    }
}
