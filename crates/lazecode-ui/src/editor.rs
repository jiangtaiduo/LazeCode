mod body;
mod chrome;
mod diff;
mod find;
mod input;
mod pages;
mod prelude;
mod welcome;

use body::*;
use chrome::*;
use diff::*;
use find::*;
use input::*;
use pages::*;
use prelude::*;
use welcome::*;

pub(crate) use input::word_at;
pub(crate) use pages::{format_json_tab, open_compare_page};

const TABBAR_H: f32 = 30.0;
const CRUMBS_H: f32 = 26.0;
const GUTTER: f32 = 52.0;
const SCROLLBAR_W: f32 = 9.0;

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme, rect: Rect) {
    if rect.height() < 35.0 || rect.width() < 10.0 {
        return;
    }
    let p = theme.palette;
    ui.painter().rect_filled(rect, 0.0, p.editor);
    let Some(index) = model.active_project else {
        draw_welcome(ui, model, rt, theme, rect, None);
        return;
    };
    let tab_count = model.projects[index].active_worktree_tabs().count();
    if tab_count == 0 {
        // The virtual Single files checkout may exist only to host a console
        // opened from the empty home screen. Keep the home screen intact.
        let welcome_index = if model.projects[index].is_single_file {
            None
        } else {
            Some(index)
        };
        draw_welcome(ui, model, rt, theme, rect, welcome_index);
        return;
    }

    let tabbar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), TABBAR_H));
    draw_tabbar(ui, model, rt, theme, tabbar, index);

    // Closing the last tab can end the checkout mid-frame:
    // settle_single_files_workspace drops an emptied Single files workspace or
    // switches to another project, so the index captured above may now point
    // past the vec. Re-resolve the target instead of using the stale index.
    let Some(index) = model.active_project else {
        draw_welcome(ui, model, rt, theme, rect, None);
        return;
    };
    let active = model.projects[index].active_tab;
    let Some(active) = active else {
        draw_welcome(ui, model, rt, theme, rect, Some(index));
        return;
    };
    let content = Rect::from_min_max(Pos2::new(rect.left(), tabbar.bottom()), rect.max);
    enum ActiveKind {
        Compare(String, String),
        File,
        Image(lazecode_core::ImageTab),
        Diff,
    }
    let kind = model.projects[index]
        .tab_by_id(active)
        .map(|tab| match &tab.kind {
            lazecode_core::TabKind::Compare(c) => {
                ActiveKind::Compare(c.left.clone(), c.right.clone())
            }
            lazecode_core::TabKind::File(_) => ActiveKind::File,
            lazecode_core::TabKind::Image(img) => ActiveKind::Image(img.clone()),
            lazecode_core::TabKind::Diff(_) => ActiveKind::Diff,
        });
    if let Some(kind) = kind {
        match kind {
            ActiveKind::Compare(left, right) => {
                crate::compare::draw_page(ui, model, rt, theme, content, &left, &right);
            }
            ActiveKind::File => {
                let is_md =
                    model.projects[index]
                        .tab_by_id(active)
                        .is_some_and(|tab| match &tab.kind {
                            TabKind::File(file) => file
                                .doc
                                .path
                                .extension()
                                .is_some_and(|ext| ext == "md" || ext == "markdown"),
                            _ => false,
                        });
                if is_md {
                    let toolbar_rect =
                        Rect::from_min_size(content.min, Vec2::new(content.width(), 32.0));
                    draw_markdown_toolbar(ui, model, theme, toolbar_rect, index, active);
                    let body = Rect::from_min_max(
                        Pos2::new(content.left(), toolbar_rect.bottom()),
                        content.max,
                    );
                    draw_file(ui, model, rt, theme, body, index, active);
                } else {
                    let crumbs =
                        Rect::from_min_size(content.min, Vec2::new(content.width(), CRUMBS_H));
                    draw_crumbs(ui, model, theme, crumbs, index, active);
                    let body =
                        Rect::from_min_max(Pos2::new(content.left(), crumbs.bottom()), content.max);
                    draw_file(ui, model, rt, theme, body, index, active);
                }
            }
            ActiveKind::Image(img) => {
                let crumbs = Rect::from_min_size(content.min, Vec2::new(content.width(), CRUMBS_H));
                draw_crumbs(ui, model, theme, crumbs, index, active);
                let body =
                    Rect::from_min_max(Pos2::new(content.left(), crumbs.bottom()), content.max);
                draw_image_tab(ui, model, rt, p, theme, body, &img, active);
            }
            ActiveKind::Diff => {
                draw_diff(ui, model, rt, theme, content, index, active);
            }
        }
    }
}
