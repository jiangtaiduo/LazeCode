//! Compare filter menus: user/date/ref pickers and view options.

use super::prelude::*;

pub(super) fn draw_compare_view_options_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    project_idx: usize,
) {
    let p = theme.palette;
    let zh = model.settings.language.is_zh();
    let menu_w = 180.0;
    let menu_h = 56.0;
    let rect = popover_rect(rt, screen, menu.anchor, menu_w, menu_h);
    menu_frame(ui, rect, theme);

    ui.painter().text(
        Pos2::new(rect.left() + 10.0, rect.top() + 11.0),
        Align2::LEFT_CENTER,
        if zh { "分组方式" } else { "Group by" },
        FontId::proportional(10.5),
        p.text3,
    );

    let item = Rect::from_min_size(
        Pos2::new(rect.left() + 4.0, rect.top() + 20.0),
        Vec2::new(menu_w - 8.0, 24.0),
    );
    let mut close = false;
    let is_grouped = model
        .projects
        .get(project_idx)
        .and_then(|p| p.compare_session.as_ref())
        .map(|s| s.group_by_dir)
        .unwrap_or(false);
    if menu_row(
        ui,
        theme,
        item,
        ui.scope_id().with("cmp-view-dir"),
        if zh { "目录" } else { "Directory" },
        false,
        true,
    ) {
        if let Some(session) = model
            .projects
            .get_mut(project_idx)
            .and_then(|p| p.compare_session.as_mut())
        {
            session.group_by_dir = !session.group_by_dir;
        }
        close = true;
    }
    if is_grouped {
        ui.painter().text(
            Pos2::new(item.left() + 10.0, item.center().y),
            Align2::LEFT_CENTER,
            "✓",
            FontId::proportional(11.0),
            p.menu_item_text,
        );
    }
    ui.painter().text(
        Pos2::new(item.right() - 8.0, item.center().y),
        Align2::RIGHT_CENTER,
        "^P",
        FontId::proportional(10.5),
        p.text3,
    );

    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        close = true;
    }
    if !close {
        rt.menu = Some(menu);
    }
}

/// 架藏右键菜单（稳定版 Shelf 页签）：恢复 / 补丁 / 重命名 / 删除。
#[allow(clippy::too_many_arguments)]
/// User 过滤菜单：本侧出现过的作者（首见序）。
pub(super) fn draw_compare_user_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    side: u8,
) {
    let p = theme.palette;
    let commits = match model.ui.compare_branches.sides.as_ref() {
        Some(sides) if side == 0 => sides.left.commits.clone(),
        Some(sides) => sides.right.commits.clone(),
        None => Vec::new(),
    };
    let mut authors: Vec<String> = crate::compare::unique_authors(&commits);
    authors.insert(0, crate::tr!(model, "All authors", "全部作者").to_string());
    let height = (authors.len() as f32 * 24.0 + 20.0).min(320.0);
    let rect = popover_rect(rt, screen, menu.anchor, 200.0, height);
    menu_frame(ui, rect, theme);
    let mut picked: Option<Option<String>> = None;
    for (i, author) in authors.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, rect.top() + 10.0 + i as f32 * 24.0),
            Vec2::new(rect.width() - 16.0, 22.0),
        );
        let response = ui.interact(row, ui.scope_id().with(("cbuser", i)), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
        }
        text_clipped(
            ui.painter(),
            row,
            author,
            FontId::proportional(11.0),
            p.text,
            Align2::LEFT_CENTER,
        );
        if response.clicked() {
            picked = Some(if i == 0 {
                None
            } else {
                Some(author.to_string())
            });
        }
    }
    if let Some(pick) = picked {
        let view = if side == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.author = pick;
        return;
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        return;
    }
    rt.menu = Some(menu);
}

/// Date 过滤菜单：All / Today / Last 7 days / Last 30 days。
pub(super) fn draw_compare_date_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    side: u8,
) {
    let p = theme.palette;
    let items: Vec<(String, Option<lazecode_core::CompareDateFilter>)> = vec![
        (crate::tr!(model, "All", "全部").to_string(), None),
        (
            crate::tr!(model, "Today", "今天").to_string(),
            Some(lazecode_core::CompareDateFilter::Today),
        ),
        (
            crate::tr!(model, "Last 7 days", "最近 7 天").to_string(),
            Some(lazecode_core::CompareDateFilter::Last7Days),
        ),
        (
            crate::tr!(model, "Last 30 days", "最近 30 天").to_string(),
            Some(lazecode_core::CompareDateFilter::Last30Days),
        ),
    ];
    let height = items.len() as f32 * 24.0 + 20.0;
    let rect = popover_rect(rt, screen, menu.anchor, 160.0, height);
    menu_frame(ui, rect, theme);
    let mut picked: Option<Option<lazecode_core::CompareDateFilter>> = None;
    for (i, (label, value)) in items.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, rect.top() + 10.0 + i as f32 * 24.0),
            Vec2::new(rect.width() - 16.0, 22.0),
        );
        let response = ui.interact(row, ui.scope_id().with(("cbdate", i)), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
        }
        ui.painter().text(
            Pos2::new(row.left() + 10.0, row.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(11.0),
            p.text,
        );
        if response.clicked() {
            picked = Some(*value);
        }
    }
    if let Some(pick) = picked {
        let view = if side == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.date = pick;
        return;
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        return;
    }
    rt.menu = Some(menu);
}

/// Branch(ref) 过滤菜单：本侧提交所属的引用名（首见序）。
pub(super) fn draw_compare_ref_menu(
    ui: &mut Ui,
    model: &mut AppModel,
    rt: &mut UiRuntime,
    theme: &Theme,
    screen: Rect,
    menu: MenuState,
    side: u8,
) {
    let p = theme.palette;
    let commits = match model.ui.compare_branches.sides.as_ref() {
        Some(sides) if side == 0 => sides.left.commits.clone(),
        Some(sides) => sides.right.commits.clone(),
        None => Vec::new(),
    };
    let mut refs = crate::compare::unique_refs(&commits);
    refs.insert(0, crate::tr!(model, "All Branches", "全部分支").to_string());
    let height = (refs.len() as f32 * 24.0 + 20.0).min(320.0);
    let rect = popover_rect(rt, screen, menu.anchor, 220.0, height);
    menu_frame(ui, rect, theme);
    let mut picked: Option<Option<String>> = None;
    for (i, name) in refs.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(rect.left() + 8.0, rect.top() + 10.0 + i as f32 * 24.0),
            Vec2::new(rect.width() - 16.0, 22.0),
        );
        let response = ui.interact(row, ui.scope_id().with(("cbref", i)), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(3), p.hover);
        }
        text_clipped(
            ui.painter(),
            row,
            name,
            FontId::monospace(10.0),
            p.text,
            Align2::LEFT_CENTER,
        );
        if response.clicked() {
            picked = Some(if i == 0 { None } else { Some(name.to_string()) });
        }
    }
    if let Some(pick) = picked {
        let view = if side == 0 {
            &mut model.ui.compare_branches.left_view
        } else {
            &mut model.ui.compare_branches.right_view
        };
        view.selected_ref_filter = pick;
        return;
    }
    if should_dismiss(ui, rect, menu.opened_frame, rt.frame_count) {
        return;
    }
    rt.menu = Some(menu);
}
