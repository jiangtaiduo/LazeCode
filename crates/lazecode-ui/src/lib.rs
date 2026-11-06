//! egui rendering for LazeCode.

pub mod activity;
pub mod app_ui;
pub mod editor;
pub mod i18n;
pub mod icons;
pub mod overlays;
pub mod palette;
pub mod panel;
pub mod rail;
pub mod runtime;
pub mod search;
pub mod settings;
pub mod shortcuts;
pub mod sidebar;
pub mod statusbar;
pub mod terminal;
pub mod theme;
pub mod titlebar;
pub mod toast;
pub mod widgets;

pub use app_ui::draw;
pub use runtime::{MenuKind, MenuState, UiRuntime};
pub use theme::install_fonts;
