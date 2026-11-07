use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Painter, Pos2, Rect,
    Stroke, TextStyle,
};

use lazecode_core::ThemeMode;

/// Design tokens ported verbatim from the stable app (branch `main`):
/// `src/styles/theme.css` (structure) + `src/styles/variants/variant-1.css`
/// (macOS Native dark, default) + `variant-2.css` (macOS Native light).
/// Every colour used by the shell comes from here; views never hard-code
/// RGB values. Panel fills carry the stable glass alpha — the window body
/// paints the stable gradient underneath, so translucent panels read as
/// Liquid Glass exactly like the web version.
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
    /// Stable GitGraphCell GRAPH_PALETTE — identical in both themes.
    pub lane: [Color32; 8],
    /// Terminal ANSI-16. The stable terminal keeps a dark canvas in both
    /// themes (PTY semantics assume dark), so one set serves both.
    pub ansi: [Color32; 16],
    pub scrollbar: Color32,
    pub scrollbar_hover: Color32,
    // --- menu family (--menu-*) ---
    pub menu_bg: Color32,
    pub menu_border: Color32,
    pub menu_item_hover: Color32,
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
    // --- window body gradient (stable body[data-variant]) ---
    pub grad_a: Color32,
    pub grad_b: Color32,
}

impl Palette {
    pub fn dark() -> Self {
        let rgba = |r: u8, g: u8, b: u8, a: u8| Color32::from_rgba_unmultiplied(r, g, b, a);
        Self {
            bg: Color32::from_rgb(0x23, 0x23, 0x26), // --bg-app
            rail: rgba(20, 20, 22, 217),             // --bg-sidebar-strip
            rail_border: rgba(255, 255, 255, 15),    // --border-subtle
            rail_text: Color32::from_rgb(0xa1, 0xa1, 0xa6),
            titlebar: rgba(40, 40, 43, 158), // --bg-header
            panel: rgba(26, 26, 28, 184),    // --bg-bottom
            panel2: Color32::from_rgb(0x19, 0x1a, 0x1c), // --bg-app-deep
            sidebar: rgba(26, 26, 28, 173),  // --bg-sidebar
            editor: Color32::from_rgb(0x1c, 0x1c, 0x1e), // --bg-editor
            tabstrip: rgba(40, 40, 43, 158),
            terminal_bg: Color32::from_rgb(0x1c, 0x1c, 0x1e), // --term-bg
            statusbar: rgba(26, 26, 28, 184),
            elev: Color32::from_rgb(0x2a, 0x2a, 0x2d), // --bg-tab-active
            hover: rgba(255, 255, 255, 20),            // --overlay-hover
            active: rgba(255, 255, 255, 36),           // --overlay-active
            border: rgba(255, 255, 255, 26),           // --border-color 0.10
            border_soft: rgba(255, 255, 255, 15),      // 0.06
            border_strong: rgba(255, 255, 255, 56),    // 0.22
            current_line: Color32::from_rgb(0x23, 0x25, 0x29), // monaco lineHighlight
            selection: rgba(10, 132, 255, 66),         // --bg-selected 0.26
            text: Color32::from_rgb(0xe8, 0xe8, 0xea), // --text-primary
            text2: Color32::from_rgb(0xa1, 0xa1, 0xa6), // --text-secondary
            text3: Color32::from_rgb(0x74, 0x74, 0x7a), // --text-muted
            text_bright: Color32::from_rgb(0xff, 0xff, 0xff),
            accent: Color32::from_rgb(0x0a, 0x84, 0xff), // macOS dark blue
            accent_on_fill: Color32::from_rgb(0xff, 0xff, 0xff),
            accent_soft: rgba(10, 132, 255, 46),  // 0.18
            accent_line: rgba(10, 132, 255, 115), // 0.45
            accent_hover: Color32::from_rgb(0x40, 0x9c, 0xff),
            accent_press: Color32::from_rgb(0x07, 0x67, 0xc4),
            green: Color32::from_rgb(0x32, 0xd7, 0x4b), // --git-green
            running: Color32::from_rgb(0x28, 0xc8, 0x40), // --tab-state-static
            amber: Color32::from_rgb(0xff, 0x9f, 0x0a), // --warn / --git-tag
            orange: Color32::from_rgb(0xf0, 0x88, 0x3e),
            rose: Color32::from_rgb(0xff, 0x45, 0x3a), // --git-del
            blue: Color32::from_rgb(0x64, 0xd2, 0xff), // --git-blue / --text-blue-mod
            violet: Color32::from_rgb(0xbf, 0x5a, 0xf2), // --git-purple
            pink: Color32::from_rgb(0xd2, 0xa8, 0xff),
            diff_add: rgba(50, 215, 75, 46), // --git-add-bg 0.18
            diff_add_strong: rgba(50, 215, 75, 89), // --git-add-line 0.35
            diff_del: rgba(255, 69, 58, 46), // --git-del-bg 0.18
            diff_del_strong: rgba(255, 69, 58, 89), // --git-del-line 0.35
            conflict_bg: Color32::from_rgb(0x54, 0x26, 0x28), // --git-conflict-bg
            tok_kw: Color32::from_rgb(0xcf, 0x8e, 0x6d), // monaco ide-jetbrains-dark keyword
            tok_type: Color32::from_rgb(0x56, 0xa8, 0xf5), // type/function
            tok_str: Color32::from_rgb(0x6a, 0xab, 0x73), // string
            tok_num: Color32::from_rgb(0x2a, 0xac, 0xb8), // number
            tok_fn: Color32::from_rgb(0x56, 0xa8, 0xf5),
            tok_com: Color32::from_rgb(0x7a, 0x7e, 0x85), // comment (italic)
            tok_punc: Color32::from_rgb(0xbc, 0xbe, 0xc4), // default foreground
            tok_lit: Color32::from_rgb(0x2a, 0xac, 0xb8),
            tok_var: Color32::from_rgb(0xbc, 0xbe, 0xc4),
            tok_prop: Color32::from_rgb(0xbc, 0xbe, 0xc4),
            tok_attr: Color32::from_rgb(0xcf, 0x8e, 0x6d),
            lane: [
                Color32::from_rgb(0x3f, 0xb9, 0x50),
                Color32::from_rgb(0xa3, 0x71, 0xf7),
                Color32::from_rgb(0xf0, 0x88, 0x3e),
                Color32::from_rgb(0x39, 0xc5, 0xcf),
                Color32::from_rgb(0xe3, 0xb3, 0x41),
                Color32::from_rgb(0x58, 0xa6, 0xff),
                Color32::from_rgb(0xf8, 0x51, 0x49),
                Color32::from_rgb(0xd2, 0xa8, 0xff),
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
            scrollbar: rgba(255, 255, 255, 46),       // 0.18
            scrollbar_hover: rgba(255, 255, 255, 77), // 0.30
            menu_bg: rgba(30, 30, 32, 245),           // --menu-bg 0.96
            menu_border: rgba(255, 255, 255, 26),
            menu_item_hover: rgba(10, 132, 255, 230), // 0.90
            scrim_subtle: rgba(0, 0, 0, 71),          // 0.28
            scrim: rgba(0, 0, 0, 107),                // 0.42
            scrim_strong: rgba(0, 0, 0, 140),         // 0.55
            dialog: Color32::from_rgb(0x28, 0x28, 0x2b), // --bg-dialog
            dialog_sidebar: Color32::from_rgb(0x20, 0x20, 0x23),
            dialog_footer: Color32::from_rgb(0x22, 0x22, 0x25),
            dialog_input: Color32::from_rgb(0x1c, 0x1c, 0x1f),
            dialog_card: Color32::from_rgb(0x32, 0x32, 0x36),
            selected_row: Color32::from_rgb(0x26, 0x41, 0x5f), // --bg-selected-row
            win_close: Color32::from_rgb(0xff, 0x5f, 0x57),
            grad_a: Color32::from_rgb(0x2b, 0x2b, 0x3d), // body radial #2b2b3d
            grad_b: Color32::from_rgb(0x1e, 0x1e, 0x20), // body radial #1e1e20
        }
    }

    pub fn light() -> Self {
        let rgba = |r: u8, g: u8, b: u8, a: u8| Color32::from_rgba_unmultiplied(r, g, b, a);
        Self {
            bg: Color32::from_rgb(0xe4, 0xe4, 0xe8),
            rail: rgba(246, 246, 248, 209), // --bg-sidebar-strip
            rail_border: rgba(0, 0, 0, 18), // 0.07
            rail_text: Color32::from_rgb(0x6e, 0x6e, 0x73),
            titlebar: rgba(246, 246, 248, 199), // 0.78
            panel: rgba(250, 250, 252, 204),    // 0.80
            panel2: Color32::from_rgb(0xe2, 0xe2, 0xe6),
            sidebar: rgba(255, 255, 255, 209), // 0.82
            editor: Color32::from_rgb(0xf7, 0xf7, 0xf9),
            tabstrip: rgba(246, 246, 248, 199),
            terminal_bg: Color32::from_rgb(0x1e, 0x1e, 0x20), // terminal stays dark
            statusbar: rgba(250, 250, 252, 204),
            elev: Color32::from_rgb(0xff, 0xff, 0xff),
            hover: rgba(0, 0, 0, 15),         // 0.06
            active: rgba(0, 0, 0, 31),        // 0.12
            border: rgba(0, 0, 0, 31),        // 0.12
            border_soft: rgba(0, 0, 0, 18),   // 0.07
            border_strong: rgba(0, 0, 0, 66), // 0.26
            current_line: Color32::from_rgb(0xee, 0xf1, 0xf6),
            selection: rgba(10, 132, 255, 36), // 0.14
            text: Color32::from_rgb(0x1d, 0x1d, 0x1f),
            text2: Color32::from_rgb(0x6e, 0x6e, 0x73),
            text3: Color32::from_rgb(0x8e, 0x8e, 0x93),
            text_bright: Color32::from_rgb(0x00, 0x00, 0x00),
            accent: Color32::from_rgb(0x00, 0x71, 0xe3), // macOS light blue
            accent_on_fill: Color32::from_rgb(0xff, 0xff, 0xff),
            accent_soft: rgba(0, 113, 227, 31),  // 0.12
            accent_line: rgba(0, 113, 227, 102), // 0.40
            accent_hover: Color32::from_rgb(0x00, 0x77, 0xed),
            accent_press: Color32::from_rgb(0x00, 0x68, 0xd0),
            green: Color32::from_rgb(0x24, 0x8a, 0x3d),
            running: Color32::from_rgb(0x28, 0xc8, 0x40),
            amber: Color32::from_rgb(0xb2, 0x50, 0x00),
            orange: Color32::from_rgb(0xb2, 0x50, 0x00),
            rose: Color32::from_rgb(0xd7, 0x00, 0x15),
            blue: Color32::from_rgb(0x00, 0x71, 0xe3),
            violet: Color32::from_rgb(0x89, 0x44, 0xab),
            pink: Color32::from_rgb(0x89, 0x44, 0xab),
            diff_add: rgba(36, 138, 61, 31),        // 0.12
            diff_add_strong: rgba(36, 138, 61, 89), // 0.35
            diff_del: rgba(215, 0, 21, 31),         // 0.12
            diff_del_strong: rgba(215, 0, 21, 77),  // 0.30
            conflict_bg: Color32::from_rgb(0xff, 0xdc, 0xdc),
            tok_kw: Color32::from_rgb(0x9a, 0x34, 0xc4), // monaco ide-light keyword
            tok_type: Color32::from_rgb(0x0b, 0x4f, 0x8a), // type/function
            tok_str: Color32::from_rgb(0x0b, 0x75, 0x00), // string
            tok_num: Color32::from_rgb(0x0b, 0x75, 0xc4), // number
            tok_fn: Color32::from_rgb(0x0b, 0x4f, 0x8a),
            tok_com: Color32::from_rgb(0x8a, 0x91, 0x9c), // comment (italic)
            tok_punc: Color32::from_rgb(0x5a, 0x5a, 0x60), // delimiter
            tok_lit: Color32::from_rgb(0x0b, 0x75, 0xc4),
            tok_var: Color32::from_rgb(0x1d, 0x1d, 0x1f),
            tok_prop: Color32::from_rgb(0x0b, 0x4f, 0x8a),
            tok_attr: Color32::from_rgb(0x9a, 0x34, 0xc4),
            lane: Palette::dark().lane, // stable GRAPH_PALETTE is theme-independent
            ansi: Palette::dark().ansi, // terminal canvas stays dark in light theme
            scrollbar: rgba(0, 0, 0, 56), // 0.22
            scrollbar_hover: rgba(0, 0, 0, 89), // 0.35
            menu_bg: rgba(250, 250, 252, 245),
            menu_border: rgba(0, 0, 0, 31),
            menu_item_hover: Color32::from_rgb(0x00, 0x71, 0xe3),
            scrim_subtle: rgba(255, 255, 255, 115), // 0.45
            scrim: rgba(255, 255, 255, 140),        // 0.55
            scrim_strong: rgba(255, 255, 255, 166), // 0.65
            dialog: Color32::from_rgb(0xff, 0xff, 0xff),
            dialog_sidebar: Color32::from_rgb(0xf7, 0xf7, 0xf9),
            dialog_footer: Color32::from_rgb(0xf2, 0xf2, 0xf4),
            dialog_input: Color32::from_rgb(0xf7, 0xf7, 0xf9),
            dialog_card: Color32::from_rgb(0xf2, 0xf2, 0xf5),
            selected_row: Color32::from_rgb(0xcf, 0xe4, 0xff),
            win_close: Color32::from_rgb(0xff, 0x5f, 0x57),
            grad_a: Color32::from_rgb(0xf2, 0xf2, 0xf5), // body linear start
            grad_b: Color32::from_rgb(0xe6, 0xe6, 0xea), // body linear end
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

/// Install the macOS system fonts so the shell matches the design instead of
/// falling back to egui's bundled Ubuntu/Hack defaults. UI stack mirrors the
/// stable `--font-ui`: SF Pro → PingFang SC → (CJK fallbacks) → emoji.
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
    let has_pingfang = add("PingFang SC", "/System/Library/Fonts/PingFang.ttc", 0);
    let has_cjk = add(
        "Hiragino Sans GB",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        0,
    );
    let has_heiti = add("Heiti SC", "/System/Library/Fonts/STHeiti Light.ttc", 1);
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
    if has_pingfang {
        proportional.push("PingFang SC".to_owned());
    }
    if has_heiti {
        proportional.push("Heiti SC".to_owned());
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
    if has_pingfang {
        mono.push("PingFang SC".to_owned());
    }
    if has_heiti {
        mono.push("Heiti SC".to_owned());
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

pub const BODY: f32 = 13.0;
pub const SMALL: f32 = 12.0;
pub const TINY: f32 = 11.0;
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

// ---------------------------------------------------------------------------
// Stable layout rhythm (theme.css heights) — shared by every view so the
// row/toolbar metrics stay consistent page to page.
// ---------------------------------------------------------------------------

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

/// Paint the stable window body gradient under everything:
/// dark = `radial-gradient(1200px 600px at 18% -8%, #2b2b3d, #1e1e20 58%)`,
/// light = `linear-gradient(160deg, #f2f2f5, #e6e6ea)`.
pub fn body_gradient(painter: &Painter, rect: Rect, dark: bool) {
    let p = if dark {
        Palette::dark()
    } else {
        Palette::light()
    };
    let cols = 8usize;
    let rows = 5usize;
    let color_at = |x: f32, y: f32| -> Color32 {
        let lerp = |a: Color32, b: Color32, t: f32| {
            let t = t.clamp(0.0, 1.0);
            let mix = |av: u8, bv: u8| -> u8 { (av as f32 + (bv as f32 - av as f32) * t) as u8 };
            Color32::from_rgba_unmultiplied(
                mix(a.r(), b.r()),
                mix(a.g(), b.g()),
                mix(a.b(), b.b()),
                255,
            )
        };
        if dark {
            // Elliptical radial: distance normalised by (1200px, 600px),
            // colour stops at 0% and 58%, flat beyond.
            let cx = rect.left() + rect.width() * 0.18;
            let cy = rect.top() - rect.height() * 0.08;
            let dx = (x - cx) / 1200.0;
            let dy = (y - cy) / 600.0;
            let t = ((dx * dx + dy * dy).sqrt() / 0.58).clamp(0.0, 1.0);
            lerp(p.grad_a, p.grad_b, t)
        } else {
            // 160deg linear: direction (sin160°, -cos160°) with CSS y-down.
            let dir = (160f32.to_radians().sin(), -160f32.to_radians().cos());
            let diag = (rect.width() * dir.0.abs() + rect.height() * dir.1.abs()).max(1.0);
            let local = (x - rect.left()) * dir.0 + (y - rect.top()) * dir.1;
            lerp(p.grad_a, p.grad_b, local / diag)
        }
    };
    let mut mesh = egui::Mesh::default();
    for row in 0..=rows {
        for col in 0..=cols {
            let x = rect.left() + rect.width() * col as f32 / cols as f32;
            let y = rect.top() + rect.height() * row as f32 / rows as f32;
            mesh.colored_vertex(Pos2::new(x, y), color_at(x, y));
        }
    }
    let stride = cols + 1;
    for row in 0..rows {
        for col in 0..cols {
            let i = row * stride + col;
            mesh.add_triangle(i as u32, i as u32 + 1, (i + stride + 1) as u32);
            mesh.add_triangle(i as u32, (i + stride + 1) as u32, (i + stride) as u32);
        }
    }
    painter.add(egui::Shape::Mesh(std::sync::Arc::new(mesh)));
}
