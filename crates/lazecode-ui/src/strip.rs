//! Stable LeftStrip (`.tool-sidebar-strip`): one 40px activity strip below
//! the titlebar. Top group: Project / Commit / Compare drawers; bottom
//! group: Git Log / Terminal dock / Settings. Active state = 3px accent
//! bar on the strip edge + `strip_active` fill + bright icon (stable
//! `.strip-btn.active`).

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Ui, Vec2};
use lazecode_core::{AppModel, Command, PanelTab, SidebarView};

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::UiRuntime;
use crate::theme::Theme;

const BTN: f32 = 32.0;
const GAP: f32 = 8.0;

#[allow(clippy::too_many_arguments)]
fn button(
    ui: &mut Ui,
    theme: &Theme,
    strip: Rect,
    center: Pos2,
    id: &'static str,
    icon: Icon,
    active: bool,
    dot: Option<Color32>,
    tooltip: String,
) -> egui::Response {
    let p = theme.palette;
    let btn = Rect::from_center_size(center, Vec2::splat(BTN));
    let response = ui.interact(btn, ui.scope_id().with(id), Sense::click());
    if active || response.hovered() {
        ui.painter().rect_filled(
            btn,
            CornerRadius::same(6),
            if active { p.strip_active } else { p.hover },
        );
    }
    if active {
        // Stable `.strip-btn.active::before`: 3px accent bar hugging the
        // strip's left edge, rounded on its right corners only.
        ui.painter().rect_filled(
            Rect::from_min_size(
                Pos2::new(strip.left() + 1.0, btn.center().y - 9.0),
                Vec2::new(3.0, 18.0),
            ),
            CornerRadius {
                nw: 0,
                ne: 2,
                se: 2,
                sw: 0,
            },
            p.accent,
        );
    }
    paint_icon(
        ui.painter(),
        btn.shrink(7.0),
        icon,
        if active {
            p.text_bright
        } else if response.hovered() {
            p.text
        } else {
            p.text2
        },
    );
    if let Some(color) = dot {
        let c = Pos2::new(btn.right() - 3.0, btn.top() + 3.0);
        ui.painter().circle_filled(c, 3.5, p.rail);
        ui.painter().circle_filled(c, 2.5, color);
    }
    response.on_hover_text(tooltip)
}

#[allow(dead_code)]
fn compare_tab_open(model: &AppModel) -> bool {
    model.active_project().is_some_and(|project| {
        project
            .tabs
            .iter()
            .any(|tab| matches!(tab.kind, lazecode_core::TabKind::Compare(_)))
    })
}

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    let cx = rect.left() + rect.width() * 0.5;
    let changes = model
        .active_project()
        .map(|project| project.changes().len())
        .unwrap_or(0);

    // --- top group: Project / Commit / Compare -------------------------
    let mut y = rect.top() + GAP;

    let project_active = model.ui.sidebar_visible && model.ui.sidebar_view == SidebarView::Files;
    let response = button(
        ui,
        theme,
        rect,
        Pos2::new(cx, y + BTN * 0.5),
        "strip-project",
        Icon::Files,
        project_active,
        None,
        crate::tr!(model, "Project", "项目").to_string(),
    );
    y += BTN + GAP;
    if response.clicked() {
        if project_active {
            model.ui.sidebar_visible = false;
        } else {
            model.ui.sidebar_view = SidebarView::Files;
            model.ui.sidebar_visible = true;
        }
    }

    let commit_active = model.ui.sidebar_visible && model.ui.sidebar_view == SidebarView::Git;
    let response = button(
        ui,
        theme,
        rect,
        Pos2::new(cx, y + BTN * 0.5),
        "strip-commit",
        Icon::Changes,
        commit_active,
        (changes > 0).then_some(p.rose),
        crate::tr!(model, "Commit", "提交").to_string(),
    );
    y += BTN + GAP;
    if response.clicked() {
        if commit_active {
            model.ui.sidebar_visible = false;
        } else {
            model.ui.sidebar_view = SidebarView::Git;
            model.ui.sidebar_visible = true;
        }
    }

    // 稳定版：Compare 条目仅在有 Compare Versions 会话时出现（LeftStrip
    // hasCompareSession），点击在显示/隐藏抽屉间切换。
    let has_session = model
        .active_project()
        .is_some_and(|project| project.compare_session.is_some());
    if has_session {
        let compare_active =
            model.ui.sidebar_visible && model.ui.sidebar_view == SidebarView::Compare;
        let response = button(
            ui,
            theme,
            rect,
            Pos2::new(cx, y + BTN * 0.5),
            "strip-compare",
            Icon::Compare,
            compare_active,
            None,
            crate::tr!(model, "Compare", "比较").to_string(),
        );
        let _ = y;
        if response.clicked() {
            if compare_active {
                model.ui.sidebar_view = SidebarView::Git;
            } else {
                model.ui.sidebar_view = SidebarView::Compare;
                model.ui.sidebar_visible = true;
                if let Some(project) = model.active_project_mut() {
                    if let Some(session) = project.compare_session.as_mut() {
                        session.minimized = false;
                    }
                }
            }
        }
    }

    // --- bottom group: Git Log / Terminal / Settings -------------------
    let by = rect.bottom() - GAP - BTN * 0.5;

    let settings_active = rt.settings_open;
    let response = button(
        ui,
        theme,
        rect,
        Pos2::new(cx, by),
        "strip-settings",
        Icon::Settings,
        settings_active,
        None,
        crate::tr!(model, "Settings · ⌘,", "设置 · ⌘,").to_string(),
    );
    if response.clicked() {
        rt.settings_open = !rt.settings_open;
        rt.about_open = false;
    }

    let terminal_active = model.ui.terminal_visible;
    let response = button(
        ui,
        theme,
        rect,
        Pos2::new(cx, by - (BTN + GAP)),
        "strip-terminal",
        Icon::Terminal,
        terminal_active,
        (model.total_running() > 0).then_some(p.running),
        crate::tr!(model, "Terminal", "终端").to_string(),
    );
    if response.clicked() {
        if model.ui.terminal_placement == lazecode_core::TerminalPlacement::Bottom {
            if terminal_active {
                model.ui.terminal_visible = false;
            } else {
                model.ui.terminal_visible = true;
                model.ui.panel_visible = false;
            }
        } else {
            model.ui.terminal_visible = !terminal_active;
        }
    }

    let log_active = model.ui.panel_visible && model.ui.panel_tab == PanelTab::Git;
    let response = button(
        ui,
        theme,
        rect,
        Pos2::new(cx, by - 2.0 * (BTN + GAP)),
        "strip-gitlog",
        Icon::GitLog,
        log_active,
        None,
        crate::tr!(model, "Git Log · ⌘⇧G", "Git 日志 · ⌘⇧G").to_string(),
    );
    if response.clicked() {
        if log_active {
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
}
