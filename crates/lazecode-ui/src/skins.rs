//! Seven curated theme pairs.
//!
//! Each theme is a designed dark/light pair, not two independent palettes.
//! A mode declares only its neutral hue/chroma and accent; the surface
//! ladder, text tiers, semantic colours and syntax roles are shared. That
//! keeps every theme crisp and predictable while the hue family changes.
//!
//! Theme changes atmosphere, never meaning: success/warning/danger and the
//! syntax role hues are fixed across themes. Only their lightness, chroma
//! and a small temperature shift are fitted per mode.

use egui::Color32;
use lazecode_core::ThemeSkin;

use crate::theme::Palette;

/// One mode's designed anchors. `hue`/`chroma` drive the neutral family;
/// `accent` is the exact action colour; `syntax_shift` adds a small
/// temperature bias to the fixed syntax role hues.
#[derive(Clone, Copy)]
struct ModeSpec {
    hue: f32,
    chroma: f32,
    accent: u32,
    syntax_shift: f32,
}

struct SkinSpec {
    dark: ModeSpec,
    light: ModeSpec,
}

const fn mode(hue: f32, chroma: f32, accent: u32, syntax_shift: f32) -> ModeSpec {
    ModeSpec {
        hue,
        chroma,
        accent,
        syntax_shift,
    }
}

fn spec(skin: ThemeSkin) -> SkinSpec {
    match skin {
        ThemeSkin::Graphite => SkinSpec {
            dark: mode(250.0, 0.009, 0x42a3fd, 0.0),
            light: mode(250.0, 0.005, 0x006ac0, 0.0),
        },
        ThemeSkin::RicePaper => SkinSpec {
            // Warm paper by day, cool ink by night, same cinnabar accent.
            dark: mode(260.0, 0.006, 0xee6a4a, 6.0),
            light: mode(85.0, 0.006, 0xaa3606, 8.0),
        },
        ThemeSkin::Tundra => SkinSpec {
            dark: mode(145.0, 0.013, 0x67bb6b, 2.0),
            light: mode(145.0, 0.011, 0x27762f, 2.0),
        },
        ThemeSkin::Abyss => SkinSpec {
            dark: mode(225.0, 0.015, 0x2abfd4, -4.0),
            light: mode(225.0, 0.011, 0x007489, -4.0),
        },
        ThemeSkin::Iris => SkinSpec {
            dark: mode(300.0, 0.013, 0xb28fef, 6.0),
            light: mode(300.0, 0.011, 0x714ca6, 6.0),
        },
        ThemeSkin::Amber => SkinSpec {
            dark: mode(65.0, 0.015, 0xed9d44, 8.0),
            light: mode(70.0, 0.022, 0x9e5800, 8.0),
        },
        ThemeSkin::Rosewood => SkinSpec {
            dark: mode(350.0, 0.013, 0xe85d7a, 4.0),
            light: mode(350.0, 0.012, 0xa32e52, 4.0),
        },
    }
}

/// Localised theme names for the settings picker.
pub fn display_name(skin: ThemeSkin, zh: bool) -> &'static str {
    if zh {
        match skin {
            ThemeSkin::Graphite => "石墨",
            ThemeSkin::RicePaper => "宣纸",
            ThemeSkin::Tundra => "苔原",
            ThemeSkin::Abyss => "深海",
            ThemeSkin::Iris => "鸢尾",
            ThemeSkin::Amber => "琥珀",
            ThemeSkin::Rosewood => "绯木",
        }
    } else {
        match skin {
            ThemeSkin::Graphite => "Graphite",
            ThemeSkin::RicePaper => "Rice Paper",
            ThemeSkin::Tundra => "Tundra",
            ThemeSkin::Abyss => "Abyss",
            ThemeSkin::Iris => "Iris",
            ThemeSkin::Amber => "Amber",
            ThemeSkin::Rosewood => "Rosewood",
        }
    }
}

/// Build the palette for `skin` in the requested mode.
pub fn palette(skin: ThemeSkin, dark: bool) -> Palette {
    // 14 palettes at ~320 bytes each; derive once, then index. This keeps
    // the per-frame theme lookup free of colour-space work.
    static CACHE: std::sync::LazyLock<Vec<Palette>> = std::sync::LazyLock::new(|| {
        ThemeSkin::ALL
            .into_iter()
            .flat_map(|skin| [compute(skin, true), compute(skin, false)])
            .collect()
    });
    CACHE[skin as usize * 2 + if dark { 0 } else { 1 }]
}

fn compute(skin: ThemeSkin, dark: bool) -> Palette {
    let spec = spec(skin);
    build(if dark { spec.dark } else { spec.light }, dark)
}

// ---------------------------------------------------------------------------
// OKLCH colour engine
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Lch {
    l: f32,
    c: f32,
    h: f32,
}

fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

fn to_rgb(c: Color32) -> (f32, f32, f32) {
    (
        c.r() as f32 / 255.0,
        c.g() as f32 / 255.0,
        c.b() as f32 / 255.0,
    )
}

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn lch_of(c: Color32) -> Lch {
    let (r, g, b) = to_rgb(c);
    let (r, g, b) = (srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b));
    let l = 0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_99 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_84 * g + 0.629_978_7 * b;
    let l_ = l.cbrt();
    let m_ = m.cbrt();
    let s_ = s.cbrt();
    let ok_l = 0.210_454_26 * l_ + 0.793_617_8 * m_ - 0.004_072_047 * s_;
    let ok_a = 1.977_998_5 * l_ - 2.428_592_2 * m_ + 0.450_593_7 * s_;
    let ok_b = 0.025_904_037 * l_ + 0.782_771_77 * m_ - 0.808_675_77 * s_;
    let c = (ok_a * ok_a + ok_b * ok_b).sqrt();
    let h = ok_b.atan2(ok_a).to_degrees().rem_euclid(360.0);
    Lch { l: ok_l, c, h }
}

fn oklab_to_linear(l: f32, a: f32, b: f32) -> (f32, f32, f32) {
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l, m, s) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    (
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    )
}

/// OKLCH → sRGB, reducing chroma until the colour lands in gamut.
fn lch_color(lch: Lch) -> Color32 {
    let l = lch.l.clamp(0.0, 1.0);
    let mut chroma = lch.c.max(0.0);
    let hue = lch.h.to_radians();
    let (sin_h, cos_h) = hue.sin_cos();
    for _ in 0..24 {
        let (r, g, b) = oklab_to_linear(l, chroma * cos_h, chroma * sin_h);
        let in_gamut = [r, g, b].iter().all(|v| *v >= -0.001 && *v <= 1.001);
        if in_gamut || chroma < 0.0005 {
            return Color32::from_rgb(
                (linear_to_srgb(r) * 255.0).round().clamp(0.0, 255.0) as u8,
                (linear_to_srgb(g) * 255.0).round().clamp(0.0, 255.0) as u8,
                (linear_to_srgb(b) * 255.0).round().clamp(0.0, 255.0) as u8,
            );
        }
        chroma *= 0.88;
    }
    Color32::BLACK
}

fn luminance(c: Color32) -> f32 {
    let (r, g, b) = to_rgb(c);
    0.2126 * srgb_to_linear(r) + 0.7152 * srgb_to_linear(g) + 0.0722 * srgb_to_linear(b)
}

fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix_ch = |av: u8, bv: u8| -> u8 {
        (av as f32 + (bv as f32 - av as f32) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color32::from_rgb(
        mix_ch(a.r(), b.r()),
        mix_ch(a.g(), b.g()),
        mix_ch(a.b(), b.b()),
    )
}

fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

/// Composite a (possibly translucent) colour over an opaque base.
#[cfg(test)]
fn flatten(c: Color32, base: Color32) -> Color32 {
    let inv = 255 - c.a() as u32;
    let ch = |v: u8, b: u8| -> u8 { (v as u32 + (b as u32 * inv) / 255).min(255) as u8 };
    Color32::from_rgb(
        ch(c.r(), base.r()),
        ch(c.g(), base.g()),
        ch(c.b(), base.b()),
    )
}

/// Move lightness until `color` clears `min` contrast against `bg`. `lighter`
/// picks the direction; hue and chroma are preserved.
fn fit(color: Color32, bg: Color32, min: f32, lighter: bool, chroma_cap: f32) -> Color32 {
    let mut lch = lch_of(color);
    lch.c = lch.c.min(chroma_cap);
    if contrast(lch_color(lch), bg) >= min {
        return lch_color(lch);
    }
    let step = if lighter { 0.008 } else { -0.008 };
    for _ in 0..140 {
        lch.l += step;
        if !(0.0..=1.0).contains(&lch.l) {
            break;
        }
        let candidate = lch_color(lch);
        if contrast(candidate, bg) >= min {
            return candidate;
        }
    }
    lch_color(Lch {
        l: if lighter { 0.99 } else { 0.06 },
        ..lch
    })
}

fn shift(c: Color32, dl: f32, chroma_scale: f32) -> Color32 {
    let lch = lch_of(c);
    lch_color(Lch {
        l: (lch.l + dl).clamp(0.02, 1.0),
        c: lch.c * chroma_scale,
        h: lch.h,
    })
}

/// Fixed ANSI role hues; only lightness/chroma change with the mode.
fn ansi_palette(
    terminal_bg: Color32,
    text: Color32,
    text2: Color32,
    text3: Color32,
    dark: bool,
) -> [Color32; 16] {
    let normal_l = if dark { 0.68 } else { 0.46 };
    let bright_l = if dark { 0.78 } else { 0.38 };
    let role = |h: f32, l: f32| {
        fit(
            lch_color(Lch { l, c: 0.15, h }),
            terminal_bg,
            4.5,
            dark,
            0.4,
        )
    };
    let normal = [
        role(25.0, normal_l),
        role(145.0, normal_l),
        role(75.0, normal_l),
        role(250.0, normal_l),
        role(330.0, normal_l),
        role(190.0, normal_l),
    ];
    let bright = [
        role(25.0, bright_l),
        role(145.0, bright_l),
        role(75.0, bright_l),
        role(250.0, bright_l),
        role(330.0, bright_l),
        role(190.0, bright_l),
    ];
    [
        fit(text, terminal_bg, 4.5, dark, 0.05),
        normal[0],
        normal[1],
        normal[2],
        normal[3],
        normal[4],
        normal[5],
        fit(text2, terminal_bg, 4.5, dark, 0.05),
        fit(text3, terminal_bg, 4.5, dark, 0.05),
        bright[0],
        bright[1],
        bright[2],
        bright[3],
        bright[4],
        bright[5],
        fit(text, terminal_bg, 8.0, dark, 0.03),
    ]
}

fn build(seed: ModeSpec, dark: bool) -> Palette {
    let surface = |l: f32, chroma_scale: f32| {
        lch_color(Lch {
            l,
            c: seed.chroma * chroma_scale,
            h: seed.hue,
        })
    };
    let neutral = |l: f32, c: f32| lch_color(Lch { l, c, h: seed.hue });

    // Shared surface ladder: panel < canvas < editor < raised. Light mode
    // keeps the same relationship, just mirrored in lightness.
    let (panel_l, canvas_l, editor_l, raised_l, border_l) = if dark {
        (0.190, 0.220, 0.245, 0.285, 0.360)
    } else {
        (0.925, 0.952, 0.988, 0.995, 0.855)
    };
    let (text_l, text2_l, text3_l) = if dark {
        (0.935, 0.740, 0.615)
    } else {
        (0.235, 0.445, 0.550)
    };

    let panel = surface(panel_l, 0.90);
    let bg = surface(canvas_l, 1.0);
    let editor_bg = surface(editor_l, 0.80);
    let raised = surface(raised_l, 0.65);
    let border = surface(border_l, 0.50);
    let border_soft = surface(if dark { 0.300 } else { 0.900 }, 0.45);
    let border_strong = surface(if dark { 0.450 } else { 0.750 }, 0.55);
    let rail = surface(panel_l + if dark { -0.010 } else { 0.005 }, 0.80);
    let titlebar = surface(panel_l + if dark { 0.020 } else { 0.010 }, 0.75);

    let text = fit(
        neutral(text_l, 0.004),
        editor_bg,
        if dark { 9.0 } else { 10.0 },
        dark,
        0.02,
    );
    let text2 = fit(neutral(text2_l, 0.009), panel, 6.2, dark, 0.03);
    let text3 = fit(neutral(text3_l, 0.011), panel, 4.0, dark, 0.03);
    let text_bright = if dark {
        Color32::from_rgb(0xff, 0xff, 0xff)
    } else {
        Color32::BLACK
    };

    let accent = fit(rgb(seed.accent), bg, 4.5, dark, 0.20);
    let white = Color32::from_rgb(0xff, 0xff, 0xff);
    let dark_ink = mix(Color32::from_rgb(0x10, 0x10, 0x12), accent, 0.10);
    let accent_on_fill = if contrast(white, accent) >= contrast(dark_ink, accent) {
        white
    } else {
        dark_ink
    };
    let accent_hover = shift(accent, 0.035, 0.98);
    let accent_press = shift(accent, -0.05, 0.98);

    let semantic = |h: f32, min: f32, cap: f32| {
        let l = if dark { 0.72 } else { 0.46 };
        let c = if dark { 0.14 } else { 0.15 };
        fit(lch_color(Lch { l, c, h }), editor_bg, min, dark, cap)
    };
    let green = semantic(145.0, 4.5, 0.60);
    let amber = semantic(75.0, 4.5, 0.60);
    let rose = semantic(25.0, 4.6, 0.60);
    let blue = semantic(250.0, 4.5, 0.50);
    let violet = semantic(300.0, 4.5, 0.50);
    let pink = semantic(330.0, 4.5, 0.50);
    let orange = semantic(40.0, 4.2, 0.50);
    let running = fit(
        lch_color(Lch {
            l: if dark { 0.75 } else { 0.50 },
            c: 0.16,
            h: 145.0,
        }),
        bg,
        3.2,
        dark,
        0.55,
    );

    // Fixed syntax roles: keyword violet, type/function blue, string green,
    // number amber. The theme only shifts the hue a few degrees.
    let syntax = |base: f32, min: f32, cap: f32| {
        let l = if dark { 0.73 } else { 0.46 };
        let c = if dark { 0.12 } else { 0.14 };
        fit(
            lch_color(Lch {
                l,
                c,
                h: base + seed.syntax_shift,
            }),
            editor_bg,
            min,
            dark,
            cap,
        )
    };
    let tok_kw = syntax(300.0, 4.8, 0.22);
    let tok_type = syntax(250.0, 4.8, 0.22);
    let tok_str = syntax(145.0, 4.8, 0.22);
    let tok_num = syntax(70.0, 4.8, 0.22);
    let tok_fn = tok_type;
    let tok_com = fit(
        mix(text3, editor_bg, if dark { 0.10 } else { 0.15 }),
        editor_bg,
        if dark { 3.8 } else { 3.4 },
        dark,
        0.06,
    );
    let tok_punc = fit(text2, editor_bg, 5.0, dark, 0.03);
    let tok_lit = tok_num;
    let tok_var = fit(text, editor_bg, 7.0, dark, 0.03);
    let tok_prop = tok_type;
    let tok_attr = tok_kw;

    let diff_t = if dark { 0.22 } else { 0.13 };
    let diff_line_add = mix(editor_bg, green, diff_t);
    let diff_line_del = mix(editor_bg, rose, diff_t);
    let diff_char_add = mix(editor_bg, green, diff_t + 0.12);
    let diff_char_del = mix(editor_bg, rose, diff_t + 0.12);

    let terminal_bg = if dark {
        shift(editor_bg, -0.02, 0.90)
    } else {
        editor_bg
    };
    let ansi = ansi_palette(terminal_bg, text, text2, text3, dark);

    let lane_hues = [140.0, 275.0, 25.0, 190.0, 45.0, 215.0, 355.0, 300.0];
    let lane = lane_hues.map(|h| {
        fit(
            lch_color(Lch {
                l: if dark { 0.68 } else { 0.52 },
                c: 0.14,
                h,
            }),
            if dark { bg } else { editor_bg },
            2.4,
            dark,
            0.20,
        )
    });

    let selected_row = mix(
        if dark { panel } else { raised },
        accent,
        if dark { 0.30 } else { 0.18 },
    );
    let hover = alpha(
        if dark { white } else { Color32::BLACK },
        if dark { 20 } else { 15 },
    );
    let active = alpha(
        if dark { white } else { Color32::BLACK },
        if dark { 36 } else { 31 },
    );
    let hairline = alpha(
        if dark { white } else { Color32::BLACK },
        if dark { 15 } else { 18 },
    );
    let scrim_subtle = alpha(
        if dark { Color32::BLACK } else { white },
        if dark { 71 } else { 115 },
    );
    let scrim = alpha(
        if dark { Color32::BLACK } else { white },
        if dark { 107 } else { 140 },
    );
    let scrim_strong = alpha(
        if dark { Color32::BLACK } else { white },
        if dark { 140 } else { 166 },
    );

    Palette {
        bg,
        rail,
        rail_border: border_soft,
        rail_text: mix(text, bg, 0.35),
        titlebar,
        panel,
        panel2: bg,
        sidebar: panel,
        editor: editor_bg,
        tabstrip: titlebar,
        terminal_bg,
        statusbar: panel,
        elev: raised,
        hover,
        active,
        border,
        border_soft,
        border_strong,
        current_line: shift(editor_bg, if dark { 0.022 } else { -0.030 }, 1.0),
        selection: alpha(accent, if dark { 66 } else { 36 }),
        text,
        text2,
        text3,
        text_bright,
        accent,
        accent_on_fill,
        accent_soft: alpha(accent, if dark { 46 } else { 31 }),
        accent_line: alpha(accent, if dark { 115 } else { 102 }),
        accent_hover,
        accent_press,
        green,
        running,
        amber,
        orange,
        rose,
        blue,
        violet,
        pink,
        diff_add: alpha(green, if dark { 46 } else { 31 }),
        diff_add_strong: alpha(green, 89),
        diff_del: alpha(rose, if dark { 46 } else { 31 }),
        diff_del_strong: alpha(rose, if dark { 89 } else { 77 }),
        diff_line_add,
        diff_line_del,
        diff_char_add,
        diff_char_del,
        jb_accent: accent,
        balloon_bg: raised,
        balloon_text: text,
        conflict_bg: mix(editor_bg, rose, if dark { 0.25 } else { 0.16 }),
        tok_kw,
        tok_type,
        tok_str,
        tok_num,
        tok_fn,
        tok_com,
        tok_punc,
        tok_lit,
        tok_var,
        tok_prop,
        tok_attr,
        git_ignored: fit(mix(text3, bg, 0.2), bg, 3.0, dark, 0.05),
        lane,
        ansi,
        scrollbar: alpha(
            if dark { white } else { Color32::BLACK },
            if dark { 46 } else { 56 },
        ),
        scrollbar_hover: alpha(
            if dark { white } else { Color32::BLACK },
            if dark { 77 } else { 89 },
        ),
        hairline,
        menu_bg: raised,
        menu_border: border,
        menu_item_hover: hover,
        menu_item_text: text,
        scrim_subtle,
        scrim,
        scrim_strong,
        dialog: raised,
        dialog_sidebar: panel,
        dialog_footer: bg,
        dialog_input: panel,
        dialog_card: raised,
        selected_row,
        win_close: Color32::from_rgb(0xff, 0x5f, 0x57),
        strip_active: alpha(
            if dark { white } else { Color32::BLACK },
            if dark { 26 } else { 20 },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazecode_core::ThemeSkin;

    fn all_modes() -> impl Iterator<Item = (ThemeSkin, bool, Palette)> {
        ThemeSkin::ALL
            .into_iter()
            .flat_map(|skin| [(skin, true), (skin, false)])
            .map(|(skin, dark)| (skin, dark, palette(skin, dark)))
    }

    fn min_contrast(pairs: &[(Color32, Color32)]) -> f32 {
        pairs
            .iter()
            .map(|(a, b)| contrast(*a, *b))
            .fold(f32::INFINITY, f32::min)
    }

    fn lab(c: Color32) -> (f32, f32, f32) {
        let lch = lch_of(c);
        let radians = lch.h.to_radians();
        (lch.l, lch.c * radians.cos(), lch.c * radians.sin())
    }

    fn distance(a: Color32, b: Color32) -> f32 {
        let (a, b) = (lab(a), lab(b));
        ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)).sqrt()
    }

    #[test]
    fn oklch_round_trips_srgb() {
        for hex in [
            0x000000u32,
            0xffffff,
            0x0a84ff,
            0xc98f6d,
            0x2e3d1f,
            0xf2bc28,
        ] {
            let original = rgb(hex);
            let back = lch_color(lch_of(original));
            assert!(
                (back.r() as i32 - original.r() as i32).abs() <= 1
                    && (back.g() as i32 - original.g() as i32).abs() <= 1
                    && (back.b() as i32 - original.b() as i32).abs() <= 1,
                "{hex:#08x} -> {:?}",
                back
            );
        }
    }

    #[test]
    fn text_and_syntax_clear_accessibility_targets() {
        let mut failures = Vec::new();
        for (skin, dark, p) in all_modes() {
            let label = format!("{} {}", skin.id(), if dark { "dark" } else { "light" });
            let body = contrast(p.text, p.editor);
            if body < 8.0 {
                failures.push(format!("{label}: body text {body:.1}:1"));
            }
            let tokens = min_contrast(&[
                (p.tok_kw, p.editor),
                (p.tok_type, p.editor),
                (p.tok_str, p.editor),
                (p.tok_num, p.editor),
                (p.tok_fn, p.editor),
                (p.tok_var, p.editor),
                (p.tok_prop, p.editor),
                (p.tok_attr, p.editor),
            ]);
            if tokens < 4.4 {
                failures.push(format!("{label}: token contrast {tokens:.1}:1"));
            }
            let caption = contrast(p.text3, p.panel);
            if caption < 3.9 {
                failures.push(format!("{label}: caption contrast {caption:.1}:1"));
            }
            let secondary = contrast(p.text2, p.panel);
            if secondary < 5.4 {
                failures.push(format!("{label}: secondary contrast {secondary:.1}:1"));
            }
            let comment = contrast(p.tok_com, p.editor);
            if comment < 3.2 {
                failures.push(format!("{label}: comment contrast {comment:.1}:1"));
            }
        }
        assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    }

    #[test]
    fn accent_and_terminal_are_legible() {
        let mut failures = Vec::new();
        for (skin, dark, p) in all_modes() {
            let label = format!("{} {}", skin.id(), if dark { "dark" } else { "light" });
            let accent = contrast(p.accent, p.bg);
            if accent < 4.4 {
                failures.push(format!("{label}: accent on bg {accent:.1}:1"));
            }
            let on_fill = contrast(p.accent_on_fill, p.accent);
            if on_fill < 4.4 {
                failures.push(format!("{label}: text on accent {on_fill:.1}:1"));
            }
            for (index, color) in p.ansi.iter().enumerate().take(7).skip(1) {
                let ratio = contrast(*color, p.terminal_bg);
                if ratio < 4.4 {
                    failures.push(format!("{label}: ansi {index} {ratio:.1}:1"));
                }
            }
            let ansi_text = contrast(p.ansi[15], p.terminal_bg);
            if ansi_text < 6.0 {
                failures.push(format!("{label}: ansi white {ansi_text:.1}:1"));
            }
            let diff_add = contrast(p.text, p.diff_line_add);
            if diff_add < 7.0 {
                failures.push(format!("{label}: text on diff add {diff_add:.1}:1"));
            }
            let diff_del = contrast(p.text, p.diff_line_del);
            if diff_del < 7.0 {
                failures.push(format!("{label}: text on diff delete {diff_del:.1}:1"));
            }
            let selected = contrast(p.text, flatten(p.selected_row, p.panel));
            if selected < 6.0 {
                failures.push(format!("{label}: text on selected row {selected:.1}:1"));
            }
        }
        assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    }

    #[test]
    fn every_skin_is_distinct() {
        let mut seen = std::collections::HashSet::new();
        for (index, skin) in ThemeSkin::ALL.into_iter().enumerate() {
            assert_eq!(skin as usize, index, "ALL order must match the enum");
            assert!(seen.insert(skin.id()), "duplicate id {}", skin.id());
        }
        for (skin, dark, p) in all_modes() {
            assert_ne!(p.editor, p.bg, "{} editor equals bg", skin.id());
            assert!(
                dark == (lch_of(p.bg).l < 0.5),
                "{} wrong variant",
                skin.id()
            );
        }
    }

    #[test]
    fn themes_are_far_enough_apart() {
        for dark in [true, false] {
            let palettes: Vec<_> = ThemeSkin::ALL
                .into_iter()
                .map(|skin| (skin, palette(skin, dark)))
                .collect();
            for i in 0..palettes.len() {
                for j in i + 1..palettes.len() {
                    let (a_skin, a) = &palettes[i];
                    let (b_skin, b) = &palettes[j];
                    let d = 0.5 * distance(a.bg, b.bg)
                        + 0.5 * distance(a.editor, b.editor)
                        + 0.3 * distance(a.panel, b.panel)
                        + 1.5 * distance(a.accent, b.accent);
                    assert!(
                        d > 0.060,
                        "{} vs {} ({:?}) too close: {d:.3}",
                        a_skin.id(),
                        b_skin.id(),
                        if dark { "dark" } else { "light" }
                    );
                }
            }
        }
    }
}
