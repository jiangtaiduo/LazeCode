use egui::{
    Color32, CornerRadius, FontDefinitions, FontFamily, FontId, FontTweak, Stroke, TextStyle,
};

use lazecode_core::ThemeMode;

/// Runtime design tokens shared by every view; views never hard-code colours.
/// `skins.rs` fills this from the seven curated theme pairs.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bg: Color32,
    pub rail: Color32,
    pub rail_border: Color32,
    pub rail_text: Color32,
    pub titlebar: Color32,
    pub panel: Color32,
    pub panel2: Color32,
    pub sidebar: Color32,
    pub editor: Color32,
    pub tabstrip: Color32,
    pub terminal_bg: Color32,
    pub statusbar: Color32,
    pub elev: Color32,
    pub hover: Color32,
    pub active: Color32,
    pub border: Color32,
    pub border_soft: Color32,
    pub border_strong: Color32,
    pub current_line: Color32,
    pub selection: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub text3: Color32,
    pub text_bright: Color32,
    pub accent: Color32,
    pub accent_on_fill: Color32,
    pub accent_soft: Color32,
    pub accent_line: Color32,
    pub accent_hover: Color32,
    pub accent_press: Color32,
    pub green: Color32,
    pub running: Color32,
    pub amber: Color32,
    pub orange: Color32,
    pub rose: Color32,
    pub blue: Color32,
    pub violet: Color32,
    pub pink: Color32,
    pub diff_add: Color32,
    pub diff_add_strong: Color32,
    pub diff_del: Color32,
    pub diff_del_strong: Color32,
    /// Monaco diff overrides from stable theme.css: insert #2c4134/#385542,
    /// delete #482b2b/#5e3436 (hard-coded in both variants).
    pub diff_line_add: Color32,
    pub diff_line_del: Color32,
    pub diff_char_add: Color32,
    pub diff_char_del: Color32,
    /// PyCharm blue used by `.diff-hunk-active-box` / `.compare-file-row.selected`
    /// / `.diff-edge-balloon` borders — deliberately not --accent in stable.
    pub jb_accent: Color32,
    /// `.diff-edge-balloon` background #23272e, text #dfe1e5.
    pub balloon_bg: Color32,
    pub balloon_text: Color32,
    pub conflict_bg: Color32,
    pub tok_kw: Color32,
    pub tok_type: Color32,
    pub tok_str: Color32,
    pub tok_num: Color32,
    pub tok_fn: Color32,
    pub tok_com: Color32,
    pub tok_punc: Color32,
    pub tok_lit: Color32,
    pub tok_var: Color32,
    pub tok_prop: Color32,
    pub tok_attr: Color32,
    /// PyCharm git ignored file/folder muted color (置灰效果).
    pub git_ignored: Color32,
    /// Stable GitGraphCell GRAPH_PALETTE — identical in both themes.
    pub lane: [Color32; 8],
    /// Terminal ANSI-16. The stable terminal keeps a dark canvas in both
    /// themes (PTY semantics assume dark), so one set serves both.
    pub ansi: [Color32; 16],
    pub scrollbar: Color32,
    pub scrollbar_hover: Color32,
    /// --overlay-faint rgba(255,255,255,0.06)（行分隔 hairline / counter 底）。
    pub hairline: Color32,
    // --- menu family (--menu-*) ---
    pub menu_bg: Color32,
    pub menu_border: Color32,
    pub menu_item_hover: Color32,
    /// Menu item text; checkmarks and labels use the same readable ink.
    pub menu_item_text: Color32,
    // --- scrims (--scrim-*) ---
    pub scrim_subtle: Color32,
    pub scrim: Color32,
    pub scrim_strong: Color32,
    // --- dialog surfaces (--bg-dialog-*) ---
    pub dialog: Color32,
    pub dialog_sidebar: Color32,
    pub dialog_footer: Color32,
    pub dialog_input: Color32,
    pub dialog_card: Color32,
    // --- misc identity colours ---
    pub selected_row: Color32,
    pub win_close: Color32,
    /// LeftStrip active button fill — stable `.strip-btn.active`:
    /// `rgba(255,255,255,0.10)` dark / `rgba(0,0,0,0.08)` light.
    pub strip_active: Color32,
}

impl Palette {
    /// Default dark palette (Graphite), used by tests and callers that only
    /// know the mode.
    pub fn dark() -> Self {
        crate::skins::palette(lazecode_core::ThemeSkin::Graphite, true)
    }

    /// Default light palette (Graphite).
    pub fn light() -> Self {
        crate::skins::palette(lazecode_core::ThemeSkin::Graphite, false)
    }
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    pub palette: Palette,
}

impl Theme {
    /// Theme for `mode` using the skin remembered for that mode.
    pub fn new(mode: ThemeMode, skin: lazecode_core::ThemeSkin) -> Self {
        Self {
            dark: mode == ThemeMode::Dark,
            palette: crate::skins::palette(skin, mode == ThemeMode::Dark),
        }
    }

    /// Default Graphite theme, used by tests and callers that only know the
    /// mode.
    pub fn for_mode(mode: ThemeMode) -> Self {
        Self::new(mode, lazecode_core::ThemeSkin::Graphite)
    }

    pub fn apply(&self, ctx: &egui::Context) {
        ctx.set_theme(if self.dark {
            egui::ThemePreference::Dark
        } else {
            egui::ThemePreference::Light
        });
        let egui_theme = if self.dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        };
        ctx.style_mut_of(egui_theme, |style| {
            // Stable type scale: 11 tiny / 12 caption / 13 body / 14 title.
            style.text_styles = [
                (TextStyle::Heading, FontId::proportional(20.0)),
                (TextStyle::Body, FontId::proportional(13.0)),
                (TextStyle::Button, FontId::proportional(13.0)),
                (TextStyle::Small, FontId::proportional(12.0)),
                (TextStyle::Monospace, FontId::monospace(13.0)),
            ]
            .into();
            style.spacing.item_spacing = egui::vec2(6.0, 4.0);
            style.spacing.button_padding = egui::vec2(8.0, 4.0);
            style.spacing.interact_size = egui::vec2(22.0, 22.0);
            style.spacing.scroll.bar_width = 9.0;
            style.spacing.scroll.handle_min_length = 18.0;
            let p = self.palette;
            let mut visuals = if self.dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            };
            visuals.panel_fill = p.panel;
            visuals.window_fill = p.dialog;
            visuals.extreme_bg_color = p.dialog_input;
            visuals.faint_bg_color = p.hover;
            visuals.override_text_color = Some(p.text);
            visuals.hyperlink_color = p.accent;
            visuals.selection.bg_fill = p.selection;
            visuals.selection.stroke = Stroke::new(1.0, p.accent);
            visuals.window_stroke = Stroke::new(1.0, p.border);
            visuals.window_corner_radius = CornerRadius::same(10);
            visuals.menu_corner_radius = CornerRadius::same(6);
            visuals.widgets.noninteractive.bg_fill = p.panel;
            visuals.widgets.noninteractive.weak_bg_fill = p.panel2;
            visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border_soft);
            visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text2);
            visuals.widgets.inactive.bg_fill = p.panel2;
            visuals.widgets.inactive.weak_bg_fill = p.panel2;
            visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, p.border);
            visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text2);
            visuals.widgets.hovered.bg_fill = p.hover;
            visuals.widgets.hovered.weak_bg_fill = p.hover;
            visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, p.border_strong);
            visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, p.text);
            visuals.widgets.active.bg_fill = p.active;
            visuals.widgets.active.weak_bg_fill = p.active;
            visuals.widgets.active.bg_stroke = Stroke::new(1.0, p.accent_line);
            visuals.widgets.active.fg_stroke = Stroke::new(1.0, p.text);
            visuals.widgets.open.bg_fill = p.elev;
            visuals.widgets.open.weak_bg_fill = p.elev;
            visuals.widgets.open.bg_stroke = Stroke::new(1.0, p.border);
            style.visuals = visuals;
        });
    }
}

/// Install macOS system fonts (SF Pro, PingFang SC and CJK fallbacks) so the
/// shell matches the design; users can override families in Settings.
pub fn install_fonts(ctx: &egui::Context, settings: &lazecode_core::Settings) {
    use std::sync::Arc;

    use egui::epaint::text::VariationCoords;

    let mut fonts = FontDefinitions::default();

    let mut add_path = |name: &str, path: &str, index: u32, tweak: Option<FontTweak>| {
        if let Some(mut data) = crate::fonts::load_path(std::path::Path::new(path), index) {
            data.tweak = tweak.unwrap_or_default();
            fonts.font_data.insert(name.to_owned(), Arc::new(data));
            true
        } else {
            false
        }
    };

    // SF Pro's variable font defaults to the 28pt Display optical size, which
    // is too tight for 11-15pt UI text; 17pt is the Text optical size.
    let sf_tweak = FontTweak {
        coords: VariationCoords::new([("opsz", 17.0)]),
        ..Default::default()
    };
    let has_ui = add_path(
        "SF Pro",
        "/System/Library/Fonts/SFNS.ttf",
        0,
        Some(sf_tweak.clone()),
    );
    let has_mono = add_path("SF Mono", "/System/Library/Fonts/SFNSMono.ttf", 0, None);
    let has_menlo = add_path("Menlo", "/System/Library/Fonts/Menlo.ttc", 0, None);
    let has_monaco = add_path("Monaco", "/System/Library/Fonts/Monaco.ttf", 0, None);
    let has_cjk = add_path(
        "Hiragino Sans GB",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        0,
        None,
    );
    let has_heiti = add_path(
        "Heiti SC",
        "/System/Library/Fonts/STHeiti Light.ttc",
        1,
        None,
    );
    let has_cjk_serif = add_path(
        "Songti SC",
        "/System/Library/Fonts/Supplemental/Songti.ttc",
        0,
        None,
    );
    let has_emoji = add_path(
        "Apple Color Emoji",
        "/System/Library/Fonts/Apple Color Emoji.ttc",
        0,
        None,
    );

    // PingFang lives in AssetsV2 on recent macOS releases; fontdb finds it
    // through the platform font directories.
    let has_pingfang = crate::fonts::face("PingFang SC")
        .and_then(crate::fonts::load)
        .map(|data| {
            fonts
                .font_data
                .insert("PingFang SC".to_owned(), Arc::new(data));
        })
        .is_some();

    let mut add_custom = |key: &str, family: &str| {
        let face = crate::fonts::face(family)?;
        let mut data = crate::fonts::load(face)?;
        if family.eq_ignore_ascii_case("SF Pro") {
            data = data.tweak(sf_tweak.clone());
        }
        fonts.font_data.insert(key.to_owned(), Arc::new(data));
        Some(())
    };
    let ui_custom = add_custom("@ui-font", &settings.ui_font).is_some();
    let editor_custom = add_custom("@editor-font", &settings.editor.font_family).is_some();
    let terminal_custom = add_custom("@terminal-font", &settings.terminal.font_family).is_some();

    fn push_unique(stack: &mut Vec<String>, name: &str) {
        if !stack.iter().any(|existing| existing == name) {
            stack.push(name.to_owned());
        }
    }

    let mut proportional = Vec::new();
    if ui_custom {
        push_unique(&mut proportional, "@ui-font");
    } else if settings.language.is_zh() {
        if has_pingfang {
            push_unique(&mut proportional, "PingFang SC");
        }
        if has_ui {
            push_unique(&mut proportional, "SF Pro");
        }
    } else {
        if has_ui {
            push_unique(&mut proportional, "SF Pro");
        }
        if has_pingfang {
            push_unique(&mut proportional, "PingFang SC");
        }
    }
    if has_cjk {
        push_unique(&mut proportional, "Hiragino Sans GB");
    }
    if has_heiti {
        push_unique(&mut proportional, "Heiti SC");
    }
    if has_cjk_serif {
        push_unique(&mut proportional, "Songti SC");
    }
    if has_emoji {
        push_unique(&mut proportional, "Apple Color Emoji");
    }
    push_unique(&mut proportional, "Ubuntu-Light");
    push_unique(&mut proportional, "Hack");
    fonts
        .families
        .insert(FontFamily::Proportional, proportional);

    let mut mono = Vec::new();
    if editor_custom {
        push_unique(&mut mono, "@editor-font");
    }
    if has_menlo {
        push_unique(&mut mono, "Menlo");
    }
    if has_mono {
        push_unique(&mut mono, "SF Mono");
    }
    if has_monaco {
        push_unique(&mut mono, "Monaco");
    }
    if has_pingfang {
        push_unique(&mut mono, "PingFang SC");
    }
    if has_cjk {
        push_unique(&mut mono, "Hiragino Sans GB");
    }
    if has_heiti {
        push_unique(&mut mono, "Heiti SC");
    }
    if has_emoji {
        push_unique(&mut mono, "Apple Color Emoji");
    }
    push_unique(&mut mono, "Hack");
    fonts.families.insert(FontFamily::Monospace, mono);

    // Terminal cells are painted at a fixed advance width, so they get their
    // own family and can follow the editor without changing editor metrics.
    let mut terminal = Vec::new();
    if terminal_custom {
        push_unique(&mut terminal, "@terminal-font");
    } else if editor_custom {
        push_unique(&mut terminal, "@editor-font");
    }
    if has_menlo {
        push_unique(&mut terminal, "Menlo");
    }
    if has_mono {
        push_unique(&mut terminal, "SF Mono");
    }
    if has_monaco {
        push_unique(&mut terminal, "Monaco");
    }
    if has_pingfang {
        push_unique(&mut terminal, "PingFang SC");
    }
    if has_cjk {
        push_unique(&mut terminal, "Hiragino Sans GB");
    }
    if has_emoji {
        push_unique(&mut terminal, "Apple Color Emoji");
    }
    push_unique(&mut terminal, "Hack");
    fonts
        .families
        .insert(FontFamily::Name("lazecode-terminal".into()), terminal);

    ctx.set_fonts(fonts);
}

/// Font used for terminal cells (custom family so it can differ from the
/// editor).
pub fn terminal_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("lazecode-terminal".into()))
}

pub const BODY: f32 = 13.5;
pub const SMALL: f32 = 12.5;
pub const TINY: f32 = 11.5;
pub const MONO: f32 = 13.0;

pub fn body_font() -> FontId {
    FontId::proportional(BODY)
}
pub fn small_font() -> FontId {
    FontId::proportional(SMALL)
}
pub fn tiny_font() -> FontId {
    FontId::proportional(TINY)
}
pub fn mono_font(size: f32) -> FontId {
    FontId::monospace(size)
}

// Stable layout rhythm (theme.css heights), shared so row and toolbar metrics
// stay consistent.

pub const TITLEBAR_H: f32 = 40.0;
pub const TOOLBAR_H: f32 = 28.0;
pub const PANEL_TITLE_H: f32 = 26.0;
pub const ROW_LOG: f32 = 26.0;
pub const ROW_TREE: f32 = 24.0;
pub const ROW_CHANGE: f32 = 22.0;
pub const ROW_DETAILS: f32 = 20.0;
pub const TABBAR_H: f32 = 30.0;
pub const TERM_TAB_H: f32 = 26.0;
pub const TOOL_WINDOW_HEAD_H: f32 = 34.0;
pub const STATUSBAR_H: f32 = 26.0;
pub const BTN_H: f32 = 28.0;
pub const STRIP_BTN: f32 = 32.0;
/// Stable context-menu width (.branch-ctx-menu).
pub const MENU_W: f32 = 260.0;

#[cfg(test)]
mod tests {
    use super::*;
    use lazecode_core::{Language, Settings};

    fn install(settings: &Settings) -> egui::Context {
        let ctx = egui::Context::default();
        install_fonts(&ctx, settings);
        let mut output = ctx.run_ui(Default::default(), |_| {});
        output.textures_delta.clear();
        ctx
    }

    fn first(ctx: &egui::Context, family: FontFamily) -> String {
        ctx.fonts(|fonts| {
            fonts
                .definitions()
                .families
                .get(&family)
                .and_then(|stack| stack.first())
                .cloned()
                .unwrap_or_default()
        })
    }

    #[test]
    fn custom_font_families_lead_their_stacks() {
        let settings = Settings {
            ui_font: "SF Mono".into(),
            editor: lazecode_core::settings::EditorSettings {
                font_family: "Menlo".into(),
                ..Default::default()
            },
            terminal: lazecode_core::settings::TerminalSettings {
                font_family: "SF Mono".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let ctx = install(&settings);
        assert_eq!(first(&ctx, FontFamily::Proportional), "@ui-font");
        assert_eq!(first(&ctx, FontFamily::Monospace), "@editor-font");
        assert_eq!(
            first(&ctx, FontFamily::Name("lazecode-terminal".into())),
            "@terminal-font"
        );
    }

    #[test]
    fn terminal_follows_editor_until_overridden() {
        let settings = Settings {
            editor: lazecode_core::settings::EditorSettings {
                font_family: "Menlo".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let ctx = install(&settings);
        assert_eq!(first(&ctx, FontFamily::Monospace), "@editor-font");
        assert_eq!(
            first(&ctx, FontFamily::Name("lazecode-terminal".into())),
            "@editor-font"
        );
    }

    #[test]
    fn chinese_ui_prefers_pingfang_when_installed() {
        if crate::fonts::face("PingFang SC").is_none() {
            return;
        }
        let settings = Settings {
            language: Language::Chinese,
            ..Settings::default()
        };
        let ctx = install(&settings);
        assert_eq!(first(&ctx, FontFamily::Proportional), "PingFang SC");
    }
}
