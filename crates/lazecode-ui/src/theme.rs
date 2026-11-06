use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
};

use lazecode_core::ThemeMode;

/// Design tokens from the UI specification. Every colour used by the shell
/// comes from here; views never hard-code RGB values.
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
    pub accent: Color32,
    pub accent_on_fill: Color32,
    pub accent_soft: Color32,
    pub accent_line: Color32,
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
    pub lane: [Color32; 6],
    pub ansi: [Color32; 16],
    pub scrollbar: Color32,
    pub scrollbar_hover: Color32,
}

impl Palette {
    pub fn dark() -> Self {
        let rgba = |r: u8, g: u8, b: u8, a: u8| Color32::from_rgba_unmultiplied(r, g, b, a);
        Self {
            bg: Color32::from_rgb(0x0b, 0x0e, 0x11),
            rail: Color32::from_rgb(0x07, 0x08, 0x0a),
            rail_border: Color32::from_rgb(0x19, 0x1c, 0x21),
            rail_text: Color32::from_rgb(0x73, 0x7b, 0x88),
            titlebar: Color32::from_rgb(0x0d, 0x10, 0x14),
            panel: Color32::from_rgb(0x10, 0x14, 0x19),
            panel2: Color32::from_rgb(0x14, 0x19, 0x20),
            sidebar: Color32::from_rgb(0x10, 0x14, 0x19),
            editor: Color32::from_rgb(0x0c, 0x0f, 0x13),
            tabstrip: Color32::from_rgb(0x0a, 0x0d, 0x10),
            terminal_bg: Color32::from_rgb(0x0a, 0x0d, 0x10),
            statusbar: Color32::from_rgb(0x0b, 0x0e, 0x12),
            elev: Color32::from_rgb(0x18, 0x1d, 0x24),
            hover: rgba(255, 255, 255, 12),
            active: rgba(255, 255, 255, 19),
            border: Color32::from_rgb(0x23, 0x29, 0x32),
            border_soft: Color32::from_rgb(0x19, 0x1f, 0x26),
            border_strong: Color32::from_rgb(0x30, 0x38, 0x44),
            current_line: rgba(255, 255, 255, 7),
            selection: rgba(168, 189, 131, 46),
            text: Color32::from_rgb(0xe9, 0xed, 0xf3),
            text2: Color32::from_rgb(0x98, 0xa2, 0xb1),
            text3: Color32::from_rgb(0x6a, 0x74, 0x83),
            accent: Color32::from_rgb(0xa8, 0xbd, 0x83),
            accent_on_fill: Color32::from_rgb(0x14, 0x1a, 0x0d),
            accent_soft: rgba(168, 189, 131, 26),
            accent_line: rgba(168, 189, 131, 77),
            green: Color32::from_rgb(0x57, 0xd9, 0xa3),
            running: Color32::from_rgb(0x5b, 0xe3, 0xa6),
            amber: Color32::from_rgb(0xf0, 0xc6, 0x74),
            orange: Color32::from_rgb(0xf5, 0xa9, 0x7f),
            rose: Color32::from_rgb(0xfb, 0x8b, 0x8b),
            blue: Color32::from_rgb(0x8a, 0xb4, 0xff),
            violet: Color32::from_rgb(0xb9, 0xa7, 0xff),
            pink: Color32::from_rgb(0xf0, 0xa6, 0xca),
            diff_add: rgba(87, 217, 163, 22),
            diff_add_strong: rgba(87, 217, 163, 41),
            diff_del: rgba(251, 139, 139, 20),
            diff_del_strong: rgba(251, 139, 139, 37),
            tok_kw: Color32::from_rgb(0xc7, 0xb3, 0xff),
            tok_type: Color32::from_rgb(0x8f, 0xd6, 0xff),
            tok_str: Color32::from_rgb(0xad, 0xdb, 0x8c),
            tok_num: Color32::from_rgb(0xf0, 0xc4, 0x8a),
            tok_fn: Color32::from_rgb(0xd8, 0xe3, 0xf3),
            tok_com: Color32::from_rgb(0x5b, 0x65, 0x74),
            tok_punc: Color32::from_rgb(0x7e, 0x88, 0x97),
            tok_lit: Color32::from_rgb(0xf5, 0xa9, 0x7f),
            tok_var: Color32::from_rgb(0xce, 0xd6, 0xe2),
            tok_prop: Color32::from_rgb(0xa8, 0xc7, 0xe8),
            tok_attr: Color32::from_rgb(0xe2, 0xc0, 0x8d),
            lane: [
                Color32::from_rgb(0xa8, 0xbd, 0x83),
                Color32::from_rgb(0x8a, 0xb4, 0xff),
                Color32::from_rgb(0xb9, 0xa7, 0xff),
                Color32::from_rgb(0xf0, 0xc6, 0x74),
                Color32::from_rgb(0xf5, 0xa9, 0x7f),
                Color32::from_rgb(0xf0, 0xa6, 0xca),
            ],
            ansi: [
                Color32::from_rgb(0x1d, 0x1f, 0x21),
                Color32::from_rgb(0xcc, 0x66, 0x66),
                Color32::from_rgb(0x98, 0xc3, 0x79),
                Color32::from_rgb(0xf0, 0xc6, 0x74),
                Color32::from_rgb(0x81, 0xa2, 0xbe),
                Color32::from_rgb(0xb2, 0x94, 0xbb),
                Color32::from_rgb(0x8a, 0xbe, 0xb8),
                Color32::from_rgb(0xc5, 0xc8, 0xc6),
                Color32::from_rgb(0x66, 0x66, 0x66),
                Color32::from_rgb(0xd5, 0x4e, 0x53),
                Color32::from_rgb(0xb9, 0xca, 0x76),
                Color32::from_rgb(0xe7, 0xc5, 0x47),
                Color32::from_rgb(0x7a, 0xa6, 0xda),
                Color32::from_rgb(0xc6, 0x78, 0xdd),
                Color32::from_rgb(0x70, 0xc0, 0xb0),
                Color32::from_rgb(0xea, 0xea, 0xea),
            ],
            scrollbar: rgba(255, 255, 255, 36),
            scrollbar_hover: rgba(255, 255, 255, 61),
        }
    }

    pub fn light() -> Self {
        let rgba = |r: u8, g: u8, b: u8, a: u8| Color32::from_rgba_unmultiplied(r, g, b, a);
        Self {
            bg: Color32::from_rgb(0xf0, 0xee, 0xe8),
            rail: Color32::from_rgb(0xf1, 0xee, 0xe6),
            rail_border: Color32::from_rgb(0xdf, 0xdb, 0xd0),
            rail_text: Color32::from_rgb(0x74, 0x78, 0x6f),
            titlebar: Color32::from_rgb(0xf7, 0xf5, 0xef),
            panel: Color32::from_rgb(0xf6, 0xf4, 0xee),
            panel2: Color32::from_rgb(0xef, 0xed, 0xe6),
            sidebar: Color32::from_rgb(0xf5, 0xf3, 0xec),
            editor: Color32::from_rgb(0xfb, 0xfa, 0xf6),
            tabstrip: Color32::from_rgb(0xef, 0xed, 0xe5),
            terminal_bg: Color32::from_rgb(0xf4, 0xf2, 0xeb),
            statusbar: Color32::from_rgb(0xed, 0xea, 0xe2),
            elev: Color32::from_rgb(0xff, 0xff, 0xff),
            hover: rgba(24, 27, 30, 12),
            active: rgba(24, 27, 30, 19),
            border: Color32::from_rgb(0xe1, 0xdd, 0xd2),
            border_soft: Color32::from_rgb(0xea, 0xe6, 0xdc),
            border_strong: Color32::from_rgb(0xcf, 0xc9, 0xbb),
            current_line: rgba(24, 27, 30, 13),
            selection: rgba(82, 107, 59, 46),
            text: Color32::from_rgb(0x1a, 0x1c, 0x1f),
            text2: Color32::from_rgb(0x5a, 0x60, 0x68),
            text3: Color32::from_rgb(0x74, 0x78, 0x70),
            accent: Color32::from_rgb(0x52, 0x6b, 0x3b),
            accent_on_fill: Color32::from_rgb(0x14, 0x1a, 0x0d),
            accent_soft: Color32::from_rgb(0xed, 0xf1, 0xe5),
            accent_line: Color32::from_rgb(0xc5, 0xd1, 0xb0),
            green: Color32::from_rgb(0x0e, 0x7a, 0x51),
            running: Color32::from_rgb(0x0f, 0x7f, 0x55),
            amber: Color32::from_rgb(0x95, 0x62, 0x0a),
            orange: Color32::from_rgb(0xb1, 0x5d, 0x29),
            rose: Color32::from_rgb(0xb2, 0x3b, 0x3b),
            blue: Color32::from_rgb(0x2f, 0x5f, 0xbf),
            violet: Color32::from_rgb(0x67, 0x47, 0xc7),
            pink: Color32::from_rgb(0xa8, 0x43, 0x7a),
            diff_add: rgba(15, 127, 85, 24),
            diff_add_strong: rgba(15, 127, 85, 43),
            diff_del: rgba(178, 59, 59, 20),
            diff_del_strong: rgba(178, 59, 59, 38),
            tok_kw: Color32::from_rgb(0x7c, 0x3f, 0xa3),
            tok_type: Color32::from_rgb(0x17, 0x6d, 0x87),
            tok_str: Color32::from_rgb(0x3f, 0x77, 0x17),
            tok_num: Color32::from_rgb(0x9a, 0x56, 0x05),
            tok_fn: Color32::from_rgb(0x24, 0x4b, 0x80),
            tok_com: Color32::from_rgb(0x8b, 0x8d, 0x82),
            tok_punc: Color32::from_rgb(0x6e, 0x72, 0x68),
            tok_lit: Color32::from_rgb(0xa0, 0x52, 0x2d),
            tok_var: Color32::from_rgb(0x2b, 0x30, 0x38),
            tok_prop: Color32::from_rgb(0x20, 0x5f, 0x83),
            tok_attr: Color32::from_rgb(0x8a, 0x5a, 0x14),
            lane: [
                Color32::from_rgb(0x64, 0x7d, 0x49),
                Color32::from_rgb(0x3d, 0x68, 0xc9),
                Color32::from_rgb(0x67, 0x47, 0xc7),
                Color32::from_rgb(0x95, 0x62, 0x0a),
                Color32::from_rgb(0xb1, 0x5d, 0x29),
                Color32::from_rgb(0xa8, 0x43, 0x7a),
            ],
            ansi: [
                Color32::from_rgb(0x1d, 0x1f, 0x21),
                Color32::from_rgb(0xb2, 0x3b, 0x3b),
                Color32::from_rgb(0x0e, 0x7a, 0x51),
                Color32::from_rgb(0x95, 0x62, 0x0a),
                Color32::from_rgb(0x2f, 0x5f, 0xbf),
                Color32::from_rgb(0x67, 0x47, 0xc7),
                Color32::from_rgb(0x0f, 0x7f, 0x7f),
                Color32::from_rgb(0x4a, 0x4e, 0x52),
                Color32::from_rgb(0x6a, 0x6e, 0x72),
                Color32::from_rgb(0xb2, 0x3b, 0x3b),
                Color32::from_rgb(0x0e, 0x7a, 0x51),
                Color32::from_rgb(0x95, 0x62, 0x0a),
                Color32::from_rgb(0x2f, 0x5f, 0xbf),
                Color32::from_rgb(0x67, 0x47, 0xc7),
                Color32::from_rgb(0x0f, 0x7f, 0x7f),
                Color32::from_rgb(0x1a, 0x1c, 0x1f),
            ],
            scrollbar: rgba(24, 27, 30, 41),
            scrollbar_hover: rgba(24, 27, 30, 71),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    pub palette: Palette,
}

impl Theme {
    pub fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self {
                dark: true,
                palette: Palette::dark(),
            },
            ThemeMode::Light => Self {
                dark: false,
                palette: Palette::light(),
            },
        }
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
            style.text_styles = [
                (TextStyle::Heading, FontId::proportional(20.0)),
                (TextStyle::Body, FontId::proportional(13.5)),
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
            visuals.window_fill = p.elev;
            visuals.extreme_bg_color = p.panel2;
            visuals.faint_bg_color = p.panel2;
            visuals.override_text_color = Some(p.text);
            visuals.hyperlink_color = p.blue;
            visuals.selection.bg_fill = p.selection;
            visuals.selection.stroke = Stroke::new(1.0, p.accent);
            visuals.window_stroke = Stroke::new(1.0, p.border);
            visuals.window_corner_radius = CornerRadius::same(10);
            visuals.menu_corner_radius = CornerRadius::same(8);
            visuals.widgets.noninteractive.bg_fill = p.panel;
            visuals.widgets.noninteractive.weak_bg_fill = p.panel2;
            visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border_soft);
            visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text2);
            visuals.widgets.inactive.bg_fill = p.panel2;
            visuals.widgets.inactive.weak_bg_fill = p.panel2;
            visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, p.border);
            visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text2);
            visuals.widgets.hovered.bg_fill = p.elev;
            visuals.widgets.hovered.weak_bg_fill = p.elev;
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

/// Install the macOS system fonts so the shell matches the design instead of
/// falling back to egui's bundled Ubuntu/Hack defaults.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    let mut add = |name: &str, path: &str, index: u32| {
        if let Ok(bytes) = std::fs::read(path) {
            let data = FontData {
                index,
                ..FontData::from_owned(bytes)
            };
            fonts
                .font_data
                .insert(name.to_owned(), std::sync::Arc::new(data));
            true
        } else {
            false
        }
    };

    let has_ui = add("SF Pro", "/System/Library/Fonts/SFNS.ttf", 0);
    let has_mono = add("SF Mono", "/System/Library/Fonts/SFNSMono.ttf", 0);
    let has_menlo = add("Menlo", "/System/Library/Fonts/Menlo.ttc", 0);
    let has_cjk = add(
        "Hiragino Sans GB",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        0,
    );
    let has_cjk_serif = add(
        "Songti SC",
        "/System/Library/Fonts/Supplemental/Songti.ttc",
        0,
    );
    let has_emoji = add(
        "Apple Color Emoji",
        "/System/Library/Fonts/Apple Color Emoji.ttc",
        0,
    );

    let mut proportional = Vec::new();
    if has_ui {
        proportional.push("SF Pro".to_owned());
    }
    if has_cjk {
        proportional.push("Hiragino Sans GB".to_owned());
    }
    if has_cjk_serif {
        proportional.push("Songti SC".to_owned());
    }
    if has_emoji {
        proportional.push("Apple Color Emoji".to_owned());
    }
    proportional.extend(["Ubuntu-Light".to_owned(), "Hack".to_owned()]);
    fonts
        .families
        .insert(FontFamily::Proportional, proportional);

    let mut mono = Vec::new();
    if has_mono {
        mono.push("SF Mono".to_owned());
    }
    if has_menlo {
        mono.push("Menlo".to_owned());
    }
    if has_cjk {
        mono.push("Hiragino Sans GB".to_owned());
    }
    if has_emoji {
        mono.push("Apple Color Emoji".to_owned());
    }
    mono.push("Hack".to_owned());
    fonts.families.insert(FontFamily::Monospace, mono);

    ctx.set_fonts(fonts);
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
