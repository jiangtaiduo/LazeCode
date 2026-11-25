//! Sidebar: explorer, source control, consoles, shelf, stash and compare
//! drawers, split into one module per view.

mod changes;
mod compare;
mod consoles;
mod prelude;
mod shelf;
mod stash;
mod tree;

use changes::*;
use compare::*;
use consoles::*;
use prelude::*;
use shelf::*;
use stash::*;
use tree::*;

const HEAD_H: f32 = 36.0;
const FOOT_H: f32 = 25.0;
const ROW_H: f32 = 25.0;

/// The sidebar only draws when a project is open, so `active_project` is an
/// invariant; route accesses through these helpers for one clear panic site.
fn active(model: &AppModel) -> &lazecode_core::ProjectState {
    model
        .active_project()
        .expect("sidebar drawn without an active project")
}

fn active_mut(model: &mut AppModel) -> &mut lazecode_core::ProjectState {
    model
        .active_project_mut()
        .expect("sidebar drawn without an active project")
}

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.sidebar);
    let Some(_) = model.active_project() else {
        let text = crate::tr!(model, "No project open", "未打开项目");
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(12.5),
            p.text3,
        );
        return;
    };
    match model.ui.sidebar_view {
        SidebarView::Files => explorer(ui, model, rt, theme, rect),
        SidebarView::Git => source_control(ui, model, rt, theme, rect),
        SidebarView::Consoles => consoles(ui, model, rt, theme, rect),
        SidebarView::Compare => compare_drawer(ui, model, rt, theme, rect),
    }
}

struct SideAction {
    icon: Icon,
    id: &'static str,
    active: bool,
    tooltip: String,
}

fn side_head(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    title: &str,
    actions: &[SideAction],
) -> Option<usize> {
    let p = theme.palette;
    ui.painter().text(
        Pos2::new(rect.left() + 14.0, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(12.5),
        p.text3,
    );
    let mut right = rect.right() - 8.0;
    let mut clicked = None;
    for (index, action) in actions.iter().enumerate().rev() {
        let button =
            Rect::from_center_size(Pos2::new(right - 11.0, rect.center().y), Vec2::splat(24.0));
        right -= 25.0;
        let resp = icon_button(ui, button, action.icon, action.active, theme);
        if !action.tooltip.is_empty() && resp.hovered() {
            resp.show_tooltip_text(&action.tooltip);
        }
        if resp.clicked() {
            clicked = Some(index);
        }
        let _ = action.id;
    }
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, p.border_soft),
    );
    clicked
}

pub(crate) fn file_tone(path: &str, theme: &Theme) -> Color32 {
    let p = theme.palette;
    match path.rsplit('.').next().unwrap_or_default() {
        "rs" => p.orange,
        "ts" | "tsx" => p.blue,
        "js" | "jsx" | "mjs" => p.amber,
        "json" | "jsonc" => p.amber,
        "css" | "scss" | "less" => p.violet,
        "html" | "htm" => p.orange,
        "py" => p.blue,
        "go" => p.green,
        "sh" | "zsh" | "bash" => p.green,
        "md" | "markdown" => p.text2,
        "toml" | "yaml" | "yml" => p.pink,
        _ => p.text3,
    }
}
