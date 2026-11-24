//! egui rendering for LazeCode.

pub mod app_ui;
pub mod compare;
pub mod diff_view;
mod editor;
pub mod fonts;
pub mod i18n;
pub mod icons;
pub mod icons_svg;
pub mod image_cache;
pub mod keybindings;
pub mod log_refs;
pub mod lsp_ui;
pub mod overlays;
pub mod palette;
pub mod panel;
pub mod runtime;
pub mod search;
pub mod settings;
pub mod shortcuts;
pub mod sidebar;
pub mod skins;
pub mod statusbar;
pub mod strip;
pub mod terminal;
pub mod theme;
pub mod titlebar;
pub mod toast;
pub mod update_ui;
pub mod widgets;

pub use app_ui::draw;
pub use runtime::{MenuKind, MenuState, UiRuntime};
pub use theme::install_fonts;

/// Align egui's double-click window with the platform preference. egui's
/// default is 300 ms; macOS often reports 500 ms, which otherwise splits a
/// legitimate system double-click into two single clicks.
pub fn set_max_double_click_delay(ctx: &egui::Context, seconds: f64) {
    ctx.memory_mut(|memory| {
        memory.options.input_options.max_double_click_delay = seconds.max(0.3);
    });
}

#[cfg(test)]
mod input_option_tests {
    use super::*;

    fn click(ctx: &egui::Context, time: f64, pos: egui::Pos2) -> bool {
        let input = egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::Vec2::new(400.0, 300.0),
            )),
            events: vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        };
        let mut double = false;
        let mut output = ctx.run_ui(input, |ui| {
            let rect = egui::Rect::from_min_size(pos, egui::Vec2::splat(24.0));
            let response = ui.interact(
                rect,
                egui::Id::unique("double-click-test"),
                egui::Sense::click(),
            );
            double = response.double_clicked();
        });
        output.textures_delta.clear();
        double
    }

    #[test]
    fn system_double_click_interval_controls_recognition() {
        let pos = egui::Pos2::new(20.0, 20.0);
        let default_ctx = egui::Context::default();
        assert!(!click(&default_ctx, 1.0, pos));
        assert!(
            !click(&default_ctx, 1.4, pos),
            "egui's 300 ms default must not accept a 400 ms double-click"
        );

        let ctx = egui::Context::default();
        set_max_double_click_delay(&ctx, 0.5);
        assert!(!click(&ctx, 1.0, pos));
        assert!(
            click(&ctx, 1.4, pos),
            "a 400 ms second click must count as a double-click"
        );
    }
}
