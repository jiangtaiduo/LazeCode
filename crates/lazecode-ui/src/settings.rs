//! Settings window: navigation plus one module per section.

mod about;
mod agents;
mod appearance;
mod editor;
mod files;
mod keybindings;
mod prelude;
mod terminal;
mod updates;
mod widgets;

use about::*;
use agents::*;
use appearance::*;
use editor::*;
use files::*;
use keybindings::*;
use terminal::*;
use updates::*;

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use lazecode_core::AppModel;

use crate::icons::{paint as paint_icon, Icon};
use crate::runtime::UiRuntime;
use crate::theme::Theme;

const SECTIONS_EN: [&str; 8] = [
    "Appearance",
    "Editor",
    "Terminal",
    "Coding Agents",
    "Files",
    "Shortcuts",
    "Updates",
    "About",
];
const SECTIONS_ZH: [&str; 8] = [
    "外观",
    "编辑器",
    "终端",
    "编码智能体",
    "文件",
    "快捷键",
    "更新",
    "关于",
];

pub fn draw(ui: &mut Ui, model: &mut AppModel, rt: &mut UiRuntime, theme: &Theme) {
    let p = theme.palette;
    let screen = ui.max_rect();
    ui.painter()
        .rect_filled(screen, 0.0, Color32::from_black_alpha(120));
    let panel = Rect::from_center_size(
        screen.center(),
        Vec2::new(
            screen.width().min(880.0) - 80.0,
            screen.height().min(640.0) - 80.0,
        ),
    );
    ui.painter()
        .rect_filled(panel, egui::CornerRadius::same(12), p.elev);
    ui.painter().rect_stroke(
        panel,
        egui::CornerRadius::same(12),
        Stroke::new(1.0, p.border_strong),
        StrokeKind::Inside,
    );
    let nav = Rect::from_min_max(panel.min, Pos2::new(panel.left() + 190.0, panel.bottom()));
    ui.painter().rect_filled(
        nav,
        egui::CornerRadius {
            nw: 12,
            sw: 12,
            ne: 0,
            se: 0,
        },
        p.panel,
    );
    ui.painter().text(
        Pos2::new(nav.left() + 22.0, nav.top() + 34.0),
        Align2::LEFT_CENTER,
        crate::tr!(model, "Settings", "设置"),
        FontId::proportional(18.0),
        p.text,
    );
    let mut y = nav.top() + 66.0;
    let sections: &[&str; 8] = if model.settings.language.is_zh() {
        &SECTIONS_ZH
    } else {
        &SECTIONS_EN
    };
    for (index, label) in sections.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(nav.left() + 10.0, y),
            Vec2::new(nav.width() - 20.0, 32.0),
        );
        let response = ui.interact(
            row,
            ui.scope_id().with(("settings-section", index)),
            Sense::click(),
        );
        if model.ui.settings_section == index {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(5), p.active);
            ui.painter().rect_filled(
                Rect::from_min_size(Pos2::new(row.left(), row.top() + 6.0), Vec2::new(2.0, 20.0)),
                egui::CornerRadius::same(1),
                p.accent,
            );
        } else if response.hovered() {
            ui.painter()
                .rect_filled(row, egui::CornerRadius::same(5), p.hover);
        }
        ui.painter().text(
            Pos2::new(row.left() + 16.0, row.center().y),
            Align2::LEFT_CENTER,
            *label,
            FontId::proportional(12.0),
            if model.ui.settings_section == index {
                p.text
            } else {
                p.text2
            },
        );
        if response.clicked() {
            model.ui.settings_section = index;
        }
        y += 35.0;
    }
    ui.painter().text(
        Pos2::new(nav.left() + 22.0, nav.bottom() - 24.0),
        Align2::LEFT_CENTER,
        format!("LazeCode {}", model.version),
        FontId::proportional(9.5),
        p.text3,
    );

    let content = Rect::from_min_max(
        Pos2::new(panel.left() + 214.0, panel.top() + 30.0),
        Pos2::new(panel.right() - 24.0, panel.bottom() - 24.0),
    );
    let close = Rect::from_center_size(
        Pos2::new(panel.right() - 20.0, panel.top() + 18.0),
        Vec2::splat(22.0),
    );
    let close_response = ui.interact(close, ui.scope_id().with("settings-close"), Sense::click());
    paint_icon(
        ui.painter(),
        close.shrink(4.0),
        Icon::X,
        if close_response.hovered() {
            p.text
        } else {
            p.text3
        },
    );
    if close_response.clicked() {
        rt.settings_open = false;
        model.save_settings();
    }

    match model.ui.settings_section {
        0 => appearance(ui, model, rt, theme, content),
        1 => editor(ui, model, rt, theme, content),
        2 => terminal(ui, model, rt, theme, content),
        3 => agents(ui, model, theme, content),
        4 => files(ui, model, rt, theme, content),
        5 => keybindings_section(ui, model, rt, theme, content),
        6 => updates(ui, model, rt, theme, content),
        _ => about(ui, model, theme, content),
    }
}

#[cfg(test)]
mod skin_ui_tests {
    use super::terminal::scrollback_options;
    use super::*;
    use lazecode_core::{Settings, ThemeMode, ThemeSkin};

    #[test]
    fn appearance_screen_renders_for_every_skin() {
        let mut model = AppModel::new(Settings::default(), None);
        model.ui.settings_section = 0;
        let mut rt = UiRuntime::default();
        for mode in [ThemeMode::Dark, ThemeMode::Light] {
            for skin in ThemeSkin::ALL {
                model.settings.theme = mode;
                model.settings.set_skin(skin);
                let theme = Theme::new(mode, skin);
                let ctx = egui::Context::default();
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 700.0))),
                    ..Default::default()
                };
                let mut output = ctx.run_ui(input, |ui| {
                    draw(ui, &mut model, &mut rt, &theme);
                });
                let shapes = output.shapes.len();
                output.textures_delta.clear();
                assert!(
                    shapes > 60,
                    "{} {:?}: settings screen drew {} shapes",
                    skin.id(),
                    mode,
                    shapes
                );
            }
        }
    }

    #[test]
    fn scrollback_options_keep_default_and_custom_value_selectable() {
        let defaults = scrollback_options(10_000);
        assert_eq!(defaults, vec![5_000, 10_000, 20_000, 50_000]);
        let custom = scrollback_options(100_000);
        assert_eq!(custom, vec![5_000, 10_000, 20_000, 50_000, 100_000]);
        let odd = scrollback_options(1_500);
        assert_eq!(odd, vec![1_500, 5_000, 10_000, 20_000, 50_000]);
    }
}
