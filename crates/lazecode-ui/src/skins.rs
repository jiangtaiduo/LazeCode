//! Designer skins: 20 palettes read from the moodboard set plus the original
//! Studio palette, each with a light and a dark variant.
//!
//! A skin only declares ten anchor colours per mode. Everything else in
//! [`Palette`] (surfaces, text tiers, borders, diff tints, syntax tokens,
//! ANSI-16, git lanes) is derived in OKLCH: same neutral hue, controlled
//! chroma, and lightness fitted to a contrast target. That keeps the chrome,
//! the copy and the editor highlight in one harmonic family instead of
//! assembling unrelated colours.

use egui::Color32;
use lazecode_core::ThemeSkin;

use crate::theme::Palette;

/// Anchor colours for one mode of one skin. Token anchors only carry hue and
/// chroma; the builder moves their lightness until they clear the editor
/// background.
#[derive(Clone, Copy)]
struct Seeds {
    bg: u32,
    editor: u32,
    accent: u32,
    keyword: u32,
    type_: u32,
    string: u32,
    number: u32,
    green: u32,
    amber: u32,
    rose: u32,
}

const fn seeds(
    bg: u32,
    editor: u32,
    accent: u32,
    keyword: u32,
    type_: u32,
    string: u32,
    number: u32,
    green: u32,
    amber: u32,
    rose: u32,
) -> Seeds {
    Seeds {
        bg,
        editor,
        accent,
        keyword,
        type_,
        string,
        number,
        green,
        amber,
        rose,
    }
}

struct SkinSpec {
    dark: Seeds,
    light: Seeds,
}

fn spec(skin: ThemeSkin) -> SkinSpec {
    match skin {
        // Fallback keeps the match total for callers that bypass `palette`.
        ThemeSkin::Studio => SkinSpec {
            dark: seeds(
                0x232326, 0x1c1c1e, 0x0a84ff, 0xcf8e6d, 0x56a8f5, 0x6aab73, 0x2aacb8, 0x32d74b,
                0xff9f0a, 0xff453a,
            ),
            light: seeds(
                0xe4e4e8, 0xf7f7f9, 0x0071e3, 0x9a34c4, 0x0b4f8a, 0x0b7500, 0x0b75c4, 0x248a3d,
                0xb25000, 0xd70015,
            ),
        },

        // 01 绣球蓝 — dusty blue petals on warm paper.
        ThemeSkin::Hydrangea => SkinSpec {
            dark: seeds(
                0x16283c, 0x14263a, 0x8fb8d9, 0xc7a9e0, 0x9ec5e8, 0x9fc6a8, 0xe3c081, 0x7fb3a0,
                0xd9b36c, 0xd98f8f,
            ),
            light: seeds(
                0xf4f1ec, 0xfbfaf6, 0x3a5f7d, 0x7b4f8a, 0x2f5f8a, 0x4a7a52, 0x9a6a2f, 0x3f7a55,
                0x9a6a2f, 0xa84f5c,
            ),
        },

        // 02 庭园 — sage, moss and cream.
        ThemeSkin::Garden => SkinSpec {
            dark: seeds(
                0x141f16, 0x131c15, 0x7fb069, 0xd9b26a, 0x8fc7a6, 0x8cbf6a, 0xd9a05b, 0x6fa85a,
                0xc8a24a, 0xc07060,
            ),
            light: seeds(
                0xede9dd, 0xf7f4e9, 0x3f6b3a, 0x7a4a2f, 0x2f5c3f, 0x4f7a2f, 0x8a5a1f, 0x3f6b3a,
                0x8a6a1f, 0x9a4a3f,
            ),
        },

        // 03 皇家紫 — lilac frost to midnight tyrian.
        ThemeSkin::Royal => SkinSpec {
            dark: seeds(
                0x1a1230, 0x171126, 0x9b7bd4, 0xd1a3e8, 0x9d8ce8, 0x7fc2b0, 0xd9b06a, 0x6fbf8a,
                0xd9a85a, 0xd97a9a,
            ),
            light: seeds(
                0xf2eef6, 0xfbf9fd, 0x5b3fa0, 0x7a3fa0, 0x3f4f9a, 0x2f6f5f, 0x8a5a1f, 0x3f7a5a,
                0x8a6a1f, 0xa03f5f,
            ),
        },

        // 04 北境 — blue-grey architecture with oxide red.
        ThemeSkin::Nordic => SkinSpec {
            dark: seeds(
                0x222833, 0x1e242d, 0x8fa8bd, 0xd9a07a, 0xa9c2d1, 0xa8bfb0, 0xd9b06a, 0x8fae9a,
                0xc9a05a, 0xc97070,
            ),
            light: seeds(
                0xe6e9ec, 0xf8f9fa, 0x3f5a72, 0x7a5a4a, 0x2f4f6a, 0x4a6a5f, 0x8a6a3f, 0x4a6a5a,
                0x8a6a3f, 0x8a2f35,
            ),
        },

        // 05 灯花 — lantern blossom ember and apricot.
        ThemeSkin::Lantern => SkinSpec {
            dark: seeds(
                0x241410, 0x21120f, 0xf78358, 0xf2a65a, 0xe8b48a, 0xc9d18a, 0xf0c060, 0x9fc27a,
                0xf0a84a, 0xe86a5a,
            ),
            light: seeds(
                0xf7e7d8, 0xfff6ec, 0xa83f2a, 0x9a3f2f, 0x8a4a2f, 0x5f6a2f, 0xa05a1f, 0x4f7a3f,
                0xa05a1f, 0xa82f2f,
            ),
        },

        // 06 沙丘鼠尾草 — sand, sage and navy.
        ThemeSkin::SandSage => SkinSpec {
            dark: seeds(
                0x23241f, 0x1f201b, 0xa7b89a, 0xd9b88a, 0xa9c2a0, 0xb0c28a, 0xd9b87a, 0x8fae7a,
                0xc9a85a, 0xc98a70,
            ),
            light: seeds(
                0xefe9dc, 0xf8f4ea, 0x5f7a5f, 0x7a5a4a, 0x4a6a5a, 0x6a7a4a, 0x9a7a4a, 0x5f7a5f,
                0x9a7a3f, 0xa05f4f,
            ),
        },

        // 07 梅岭 — misty plum ridge.
        ThemeSkin::PlumRidge => SkinSpec {
            dark: seeds(
                0x22252a, 0x1e2125, 0xc98f91, 0xd9a89a, 0xa9c2c0, 0xb9c2a0, 0xd9b87a, 0x8fb0a0,
                0xc9a85a, 0xc97a80,
            ),
            light: seeds(
                0xf0ebe2, 0xfaf6ee, 0x8a5a5f, 0x7a4a4a, 0x5a6a6f, 0x6a7a5f, 0x8a6a3f, 0x5f7a6f,
                0x8a6a3f, 0x9a4f5a,
            ),
        },

        // 08 枫桥 — maple bridge mooring.
        ThemeSkin::Maple => SkinSpec {
            dark: seeds(
                0x201d18, 0x1c1a15, 0xc98f6d, 0xd9a07a, 0xb9c2a0, 0xb0c28a, 0xd9b06a, 0x8fae8a,
                0xc9a050, 0xc97a5a,
            ),
            light: seeds(
                0xf0e8da, 0xfaf3e7, 0x9d6551, 0x9a4f3f, 0x5f6a5a, 0x6a7a4f, 0xa07a3f, 0x5f7a5a,
                0xa07a3f, 0xa04f45,
            ),
        },

        // 09 金苇 — golden reeds by the river.
        ThemeSkin::Reeds => SkinSpec {
            dark: seeds(
                0x21211a, 0x1d1d16, 0xc9c28a, 0xd9c98a, 0xb9c2a0, 0xc2c98a, 0xd9c06a, 0xa0b07a,
                0xc9b05a, 0xc99a6a,
            ),
            light: seeds(
                0xf0eadb, 0xfaf5e8, 0x8a8440, 0x7a6a2f, 0x5a6a4a, 0x6a7a3f, 0x9a7a2f, 0x6f7a3f,
                0x9a7a2f, 0x9a5a3f,
            ),
        },

        // 10 紫暮山 — violet mountains at dusk.
        ThemeSkin::VioletDusk => SkinSpec {
            dark: seeds(
                0x1f1c22, 0x1b181f, 0xb8a9d1, 0xd1b0e0, 0xb0b0e0, 0xa0c2b0, 0xd9b88a, 0x8fb0a0,
                0xc9a87a, 0xc98a9a,
            ),
            light: seeds(
                0xede4dc, 0xf7f0e9, 0x797487, 0x6a4f7a, 0x5a5a7a, 0x4f6a6a, 0x8a6a4f, 0x4f6a5f,
                0x8a6a4f, 0x8a4f5f,
            ),
        },

        // 11 荷风 — lotus pond breeze.
        ThemeSkin::Lotus => SkinSpec {
            dark: seeds(
                0x1a2421, 0x16201d, 0xa0c2b0, 0xc9d1a0, 0x9fc2b0, 0xa9c2a0, 0xd9c08a, 0x8fb09a,
                0xc9b07a, 0xc98a8a,
            ),
            light: seeds(
                0xf0ebdf, 0xf9f5ea, 0x3a5751, 0x4f5a4a, 0x3f6a5f, 0x5a7a5a, 0x8a7a3f, 0x4f7a5f,
                0x8a6a3f, 0xa05f5f,
            ),
        },

        // 12 叠翠 — layered jade peaks.
        ThemeSkin::Jade => SkinSpec {
            dark: seeds(
                0x15201d, 0x121b19, 0x8fbfa9, 0xc9d1a0, 0x8fc2b0, 0xa9c2a0, 0xd9c08a, 0x7fb09a,
                0xc9b07a, 0xc98a7a,
            ),
            light: seeds(
                0xeeeade, 0xf8f5e8, 0x294a46, 0x4a5a3f, 0x2f5f5a, 0x4f7a5a, 0x8a7a3f, 0x3f7a5f,
                0x8a6a3f, 0x9a5a4f,
            ),
        },

        // 13 里维埃拉 — deep riviera blue and terracotta.
        ThemeSkin::Riviera => SkinSpec {
            dark: seeds(
                0x121c26, 0x101820, 0x7fa8c9, 0xd9a07a, 0x9fc2e0, 0xa0c2b0, 0xd9b87a, 0x8fb090,
                0xd9a85a, 0xd98a6a,
            ),
            light: seeds(
                0xf3ecde, 0xfcf7ec, 0x183451, 0x8a4a2f, 0x2f4f6a, 0x3f6a5f, 0xa0782f, 0x4f6a4f,
                0xa0782f, 0xa03f2f,
            ),
        },

        // 14 鸢尾田 — iris field.
        ThemeSkin::Iris => SkinSpec {
            dark: seeds(
                0x221b25, 0x1e1721, 0xc9a0c9, 0xd9a0d1, 0xb0a9e0, 0xb0c2a0, 0xd9c06a, 0x9fb0a0,
                0xc9b05a, 0xc98aa0,
            ),
            light: seeds(
                0xf1ebf2, 0xfaf6fb, 0x976290, 0x8a3f7a, 0x5f5a8a, 0x6a7a5f, 0x8a7a2f, 0x5f7a5f,
                0x8a6a2f, 0x9a4f6a,
            ),
        },

        // 15 可可 — cacao and oat milk.
        ThemeSkin::Cacao => SkinSpec {
            dark: seeds(
                0x211a16, 0x1d1713, 0xc9a08a, 0xd9b09a, 0xc9b8a0, 0xc2c2a0, 0xd9b07a, 0xa0b090,
                0xc9a06a, 0xc98a7a,
            ),
            light: seeds(
                0xe9e2d9, 0xf6f0e8, 0x8c634d, 0x7a4a3f, 0x6a5a4f, 0x6a6a4f, 0x9a6a3f, 0x5f6a4f,
                0x9a6a3f, 0x9a4f3f,
            ),
        },

        // 16 苔金 — moss and gold.
        ThemeSkin::MossGold => SkinSpec {
            dark: seeds(
                0x1c1e16, 0x181a13, 0xc9c25a, 0xd9d09a, 0xb0c290, 0xc2c98a, 0xe0c060, 0xa0b07a,
                0xd9b84a, 0xc98a6a,
            ),
            light: seeds(
                0xefeadc, 0xf8f4e4, 0x75760b, 0x6a5a1f, 0x4f5f3f, 0x6a7a2f, 0x8a7a1f, 0x5f7a3f,
                0x8a7a1f, 0x9a5a3f,
            ),
        },

        // 17 珊瑚海岸 — coral coast.
        ThemeSkin::Coral => SkinSpec {
            dark: seeds(
                0x241b1c, 0x201718, 0xe88a6a, 0xf0a08a, 0xa9b8d1, 0xa0c2b0, 0xe8b86a, 0x8fb09a,
                0xe8a85a, 0xe07a7a,
            ),
            light: seeds(
                0xf7ded0, 0xfdf3ec, 0xc95f42, 0xb04a3f, 0x5f6a7a, 0x4f7a6a, 0xb07a2f, 0x4f7a5f,
                0xb07a2f, 0xc04f4f,
            ),
        },

        // 18 青柠海湾 — teal and lemon.
        ThemeSkin::TealLemon => SkinSpec {
            dark: seeds(
                0x10211f, 0x0d1c1a, 0x4fc9c2, 0xe0c060, 0x5fc2c9, 0x9fc9a0, 0xe8c04a, 0x7fbf9f,
                0xd9b43a, 0xd98a6a,
            ),
            light: seeds(
                0xf1eee0, 0xfbf8ea, 0x0b6b6c, 0x8a6a1f, 0x0d5f6a, 0x2f7a5f, 0xb8860b, 0x2f7a5f,
                0xa87800, 0xb04a3f,
            ),
        },

        // 19 木槿 — hibiscus.
        ThemeSkin::Hibiscus => SkinSpec {
            dark: seeds(
                0x241a1c, 0x201619, 0xf07a8a, 0xc2d16a, 0xa9c290, 0xb9d18a, 0xe0b86a, 0x9fc27a,
                0xd9a85a, 0xf07a8a,
            ),
            light: seeds(
                0xf5e3c7, 0xfdf6ec, 0xc93f5a, 0x6a7a1f, 0x4f6a3f, 0x5f7a2f, 0xb07a2f, 0x5f8a2f,
                0xb07a2f, 0xd94f6a,
            ),
        },

        // 20 野橄榄 — wild olive and saffron.
        ThemeSkin::WildOlive => SkinSpec {
            dark: seeds(
                0x1a2019, 0x161c16, 0xe0b84a, 0xc9d18a, 0x7fb0b8, 0xa0c29a, 0xe8c04a, 0x8fae7a,
                0xe0b84a, 0xc98a6a,
            ),
            light: seeds(
                0xf2e9cf, 0xfbf5e4, 0x0d4b66, 0x5f6a2f, 0x2f5f5f, 0x4f7a5f, 0xb8860b, 0x4f7a4f,
                0xb8860b, 0xa05f3f,
            ),
        },
    }
}

/// Localised skin names for the settings picker.
pub fn display_name(skin: ThemeSkin, zh: bool) -> &'static str {
    if zh {
        match skin {
            ThemeSkin::Studio => "石墨（原配色）",
            ThemeSkin::Hydrangea => "绣球蓝",
            ThemeSkin::Garden => "庭园",
            ThemeSkin::Royal => "皇家紫",
            ThemeSkin::Nordic => "北境",
            ThemeSkin::Lantern => "灯花",
            ThemeSkin::SandSage => "沙丘鼠尾草",
            ThemeSkin::PlumRidge => "梅岭",
            ThemeSkin::Maple => "枫桥",
            ThemeSkin::Reeds => "金苇",
            ThemeSkin::VioletDusk => "紫暮山",
            ThemeSkin::Lotus => "荷风",
            ThemeSkin::Jade => "叠翠",
            ThemeSkin::Riviera => "里维埃拉",
            ThemeSkin::Iris => "鸢尾田",
            ThemeSkin::Cacao => "可可",
            ThemeSkin::MossGold => "苔金",
            ThemeSkin::Coral => "珊瑚海岸",
            ThemeSkin::TealLemon => "青柠海湾",
            ThemeSkin::Hibiscus => "木槿",
            ThemeSkin::WildOlive => "野橄榄",
        }
    } else {
        match skin {
            ThemeSkin::Studio => "Studio",
            ThemeSkin::Hydrangea => "Hydrangea Blue",
            ThemeSkin::Garden => "Garden",
            ThemeSkin::Royal => "Royal Violet",
            ThemeSkin::Nordic => "Nordic",
            ThemeSkin::Lantern => "Lantern",
            ThemeSkin::SandSage => "Sand & Sage",
            ThemeSkin::PlumRidge => "Plum Ridge",
            ThemeSkin::Maple => "Maple Bridge",
            ThemeSkin::Reeds => "Golden Reeds",
            ThemeSkin::VioletDusk => "Violet Dusk",
            ThemeSkin::Lotus => "Lotus",
            ThemeSkin::Jade => "Jade Peaks",
            ThemeSkin::Riviera => "Riviera",
            ThemeSkin::Iris => "Iris Field",
            ThemeSkin::Cacao => "Cacao",
            ThemeSkin::MossGold => "Moss & Gold",
            ThemeSkin::Coral => "Coral Coast",
            ThemeSkin::TealLemon => "Teal & Lemon",
            ThemeSkin::Hibiscus => "Hibiscus",
            ThemeSkin::WildOlive => "Wild Olive",
        }
    }
}

/// Build the palette for `skin` in the requested mode.
pub fn palette(skin: ThemeSkin, dark: bool) -> Palette {
    // 42 palettes at ~320 bytes each; derive once, then index. This keeps
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
    if skin == ThemeSkin::Studio {
        return if dark {
            Palette::studio_dark()
        } else {
            Palette::studio_light()
        };
    }
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
    Color32::from_rgb(
        (linear_to_srgb(0.0) * 255.0) as u8,
        (linear_to_srgb(0.0) * 255.0) as u8,
        (linear_to_srgb(0.0) * 255.0) as u8,
    )
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

/// Composite a (possibly translucent, premultiplied) colour over an opaque
/// base. Used to reason about contrast on glass panels.
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
/// picks the direction; the seed's hue and chroma are preserved.
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

fn surface(base: Lch, dl: f32, chroma_scale: f32) -> Color32 {
    lch_color(Lch {
        l: (base.l + dl).clamp(0.012, 0.992),
        c: base.c * chroma_scale,
        h: base.h,
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

/// Lift a colour's chroma to `floor`, keeping hue and lightness. Dark mode
/// needs it: the moodboard seeds are dusty pastels, and at pastel chroma on
/// a deep canvas they read as fog rather than colour.
fn chroma_floor(c: Color32, floor: f32) -> Color32 {
    let mut lch = lch_of(c);
    lch.c = lch.c.max(floor);
    lch_color(lch)
}

fn build(seed: Seeds, dark: bool) -> Palette {
    let bg_seed = rgb(seed.bg);
    let editor_seed = rgb(seed.editor);
    let accent_seed = rgb(seed.accent);
    let bg_lch = lch_of(bg_seed);
    let editor_lch = lch_of(editor_seed);

    // Dark canvases sink together: scale both seeds' lightness by the same
    // factor so the designed bg/editor offset survives while the moodboard's
    // airy pastels stop reading as fog. Light mode is untouched.
    let deep = if dark { 0.88 } else { 1.0 };

    // Neutral family: keep the designer's hue, tame the chroma so large
    // surfaces stay quiet.
    let neutral = Lch {
        l: bg_lch.l * deep,
        c: bg_lch.c.clamp(0.004, 0.045),
        h: bg_lch.h,
    };
    let editor = Lch {
        l: (editor_lch.l * deep).clamp(if dark { 0.11 } else { 0.93 }, 0.97),
        c: editor_lch.c.clamp(0.002, 0.04),
        h: editor_lch.h,
    };

    let bg = surface(neutral, 0.0, 1.0);
    let editor_bg = lch_color(editor);

    let text_l = if dark { 0.945 } else { 0.21 };
    let text = fit(
        lch_color(Lch {
            l: text_l,
            c: 0.008,
            h: neutral.h,
        }),
        editor_bg,
        if dark { 9.0 } else { 10.0 },
        dark,
        0.02,
    );

    // Chrome surfaces.
    let grad_a = surface(neutral, if dark { 0.075 } else { 0.035 }, 1.6);
    let grad_b = surface(neutral, if dark { -0.012 } else { -0.012 }, 1.1);
    let panel2 = surface(neutral, if dark { -0.045 } else { -0.03 }, 0.8);
    let elev = surface(editor, if dark { 0.075 } else { 0.022 }, 0.7);
    let rail = alpha(surface(neutral, if dark { -0.02 } else { 0.05 }, 0.5), 217);
    let titlebar = alpha(
        surface(neutral, if dark { 0.035 } else { 0.045 }, 0.8),
        if dark { 158 } else { 199 },
    );
    let panel = alpha(
        surface(neutral, if dark { -0.015 } else { 0.05 }, 0.7),
        if dark { 184 } else { 204 },
    );
    let sidebar = alpha(
        surface(neutral, if dark { -0.012 } else { 0.06 }, 0.7),
        if dark { 173 } else { 209 },
    );
    let statusbar = panel;
    let chrome = flatten(panel, bg);
    let text2 = fit(
        lch_color(Lch {
            l: if dark { 0.75 } else { 0.47 },
            c: 0.014,
            h: neutral.h,
        }),
        chrome,
        6.2,
        dark,
        0.03,
    );
    let text3 = fit(
        lch_color(Lch {
            l: if dark { 0.65 } else { 0.55 },
            c: 0.012,
            h: neutral.h,
        }),
        chrome,
        if dark { 4.3 } else { 4.1 },
        dark,
        0.03,
    );
    let text_bright = if dark {
        Color32::from_rgb(0xff, 0xff, 0xff)
    } else {
        mix(text, Color32::BLACK, 0.6)
    };
    let terminal_l = if dark { editor.l - 0.01 } else { 0.165 };
    let terminal_bg = lch_color(Lch {
        l: terminal_l.clamp(0.13, 0.20),
        c: editor.c.max(0.008) * 0.8,
        h: editor.h,
    });

    let dialog = surface(neutral, if dark { 0.035 } else { 0.062 }, 0.5);
    let dialog_sidebar = surface(neutral, if dark { 0.005 } else { 0.052 }, 0.38);
    let dialog_footer = surface(neutral, if dark { 0.015 } else { 0.03 }, 0.4);
    let dialog_input = surface(neutral, if dark { -0.02 } else { 0.035 }, 0.35);
    let dialog_card = surface(neutral, if dark { 0.07 } else { -0.005 }, 0.35);

    // Accent: same hue family, fitted for legibility on chrome and on fill.
    let accent = fit(accent_seed, bg, 4.2, dark, 0.17);
    let white = Color32::from_rgb(0xff, 0xff, 0xff);
    let dark_ink = mix(Color32::from_rgb(0x10, 0x10, 0x12), accent, 0.10);
    let accent_on_fill = if contrast(white, accent) >= contrast(dark_ink, accent) {
        white
    } else {
        dark_ink
    };
    let accent_hover = shift(accent, 0.035, 0.98);
    let accent_press = shift(accent, -0.05, 0.98);
    let accent_soft_alpha = if dark { 46 } else { 31 };
    let accent_line_alpha = if dark { 115 } else { 102 };

    // Token/semantic anchors: dark mode lifts their chroma to a floor (hue
    // preserved) so dusty pastels read as colour instead of fog.
    let anchor = |hex: u32| {
        if dark {
            chroma_floor(rgb(hex), 0.105)
        } else {
            rgb(hex)
        }
    };
    let token =
        |hex: u32, min: f32, cap_chroma: f32| fit(anchor(hex), editor_bg, min, dark, cap_chroma);

    let green = token(seed.green, 4.5, 0.6);
    let amber = token(seed.amber, 4.5, 0.6);
    let rose = token(seed.rose, 4.6, 0.6);
    let blue = token(seed.type_, 4.5, 0.5);
    let violet = token(seed.keyword, 4.5, 0.5);
    let pink = mix(violet, white, 0.35);

    let running = fit(
        lch_color(Lch {
            l: lch_of(green).l + 0.05,
            c: lch_of(green).c.max(0.14),
            h: lch_of(green).h,
        }),
        bg,
        3.2,
        dark,
        0.55,
    );

    // Editor tokens: hue from the anchor, lightness fitted above the editor.
    let tok_kw = token(seed.keyword, 4.8, 0.22);
    let tok_type = token(seed.type_, 4.8, 0.22);
    let tok_str = token(seed.string, 4.8, 0.22);
    let tok_num = token(seed.number, 4.8, 0.22);
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

    // Diff tints derive from this skin's editor, not the old dark literals.
    let diff_t = if dark { 0.22 } else { 0.13 };
    let diff_line_add = mix(editor_bg, green, diff_t);
    let diff_line_del = mix(editor_bg, rose, diff_t);
    let diff_char_add = mix(editor_bg, green, diff_t + 0.12);
    let diff_char_del = mix(editor_bg, rose, diff_t + 0.12);

    // ANSI-16 on the (always dark) terminal canvas.
    let ansi_hue =
        |h: f32, l: f32, c: f32| fit(lch_color(Lch { l, c, h }), terminal_bg, 4.6, true, 0.4);
    let red_h = lch_of(rose).h;
    let green_h = lch_of(green).h;
    let amber_h = lch_of(amber).h;
    let blue_h = lch_of(blue).h;
    let violet_h = lch_of(violet).h;
    let teal_h = 190.0;
    let ansi_normal = [
        ansi_hue(red_h, 0.66, 0.15),
        ansi_hue(green_h, 0.72, 0.15),
        ansi_hue(amber_h, 0.78, 0.14),
        ansi_hue(blue_h, 0.70, 0.14),
        ansi_hue(violet_h, 0.70, 0.14),
        ansi_hue(teal_h, 0.74, 0.11),
    ];
    let ansi_bright = ansi_normal.map(|c| shift(c, 0.09, 1.0));
    let ansi = [
        mix(terminal_bg, text, 0.06),
        ansi_normal[0],
        ansi_normal[1],
        ansi_normal[2],
        ansi_normal[3],
        ansi_normal[4],
        ansi_normal[5],
        fit(text2, terminal_bg, 6.0, true, 0.04),
        fit(text3, terminal_bg, 4.2, true, 0.04),
        ansi_bright[0],
        ansi_bright[1],
        ansi_bright[2],
        ansi_bright[3],
        ansi_bright[4],
        ansi_bright[5],
        fit(text, terminal_bg, 8.0, true, 0.03),
    ];

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
            0.2,
        )
    });

    let selected_row = mix(
        if dark { panel2 } else { elev },
        accent,
        if dark { 0.30 } else { 0.18 },
    );

    let border_alpha = if dark { 48 } else { 48 };
    let border_soft_alpha = if dark { 30 } else { 30 };
    let border_strong_alpha = if dark { 80 } else { 85 };

    Palette {
        bg,
        rail,
        rail_border: alpha(
            if dark { white } else { Color32::BLACK },
            if dark { 15 } else { 18 },
        ),
        rail_text: mix(text, bg, 0.35),
        titlebar,
        panel,
        panel2,
        sidebar,
        editor: editor_bg,
        tabstrip: titlebar,
        terminal_bg,
        statusbar,
        elev,
        hover: alpha(
            if dark { white } else { Color32::BLACK },
            if dark { 20 } else { 15 },
        ),
        active: alpha(
            if dark { white } else { Color32::BLACK },
            if dark { 36 } else { 31 },
        ),
        border: alpha(if dark { white } else { Color32::BLACK }, border_alpha),
        border_soft: alpha(if dark { white } else { Color32::BLACK }, border_soft_alpha),
        border_strong: alpha(
            if dark { white } else { Color32::BLACK },
            border_strong_alpha,
        ),
        current_line: surface(editor, if dark { 0.022 } else { -0.03 }, 1.0),
        selection: alpha(accent, if dark { 66 } else { 36 }),
        text,
        text2,
        text3,
        text_bright,
        accent,
        accent_on_fill,
        accent_soft: alpha(accent, accent_soft_alpha),
        accent_line: alpha(accent, accent_line_alpha),
        accent_hover,
        accent_press,
        green,
        running,
        amber,
        orange: fit(anchor(seed.amber), bg, 4.2, dark, 0.5),
        rose,
        blue,
        violet,
        pink,
        diff_add: alpha(green, if dark { 46 } else { 31 }),
        diff_add_strong: alpha(green, if dark { 89 } else { 89 }),
        diff_del: alpha(rose, if dark { 46 } else { 31 }),
        diff_del_strong: alpha(rose, if dark { 89 } else { 77 }),
        diff_line_add,
        diff_line_del,
        diff_char_add,
        diff_char_del,
        jb_accent: accent,
        balloon_bg: if dark {
            mix(dialog, text, 0.06)
        } else {
            mix(white, bg, 0.06)
        },
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
        hairline: alpha(
            if dark { white } else { Color32::BLACK },
            if dark { 15 } else { 18 },
        ),
        menu_bg: surface(neutral, if dark { 0.015 } else { 0.062 }, 0.45),
        menu_border: mix(
            surface(neutral, if dark { 0.015 } else { 0.062 }, 0.45),
            text,
            if dark { 0.22 } else { 0.18 },
        ),
        menu_item_hover: alpha(accent, 230),
        menu_item_text: accent_on_fill,
        scrim_subtle: alpha(
            if dark { Color32::BLACK } else { white },
            if dark { 71 } else { 115 },
        ),
        scrim: alpha(
            if dark { Color32::BLACK } else { white },
            if dark { 107 } else { 140 },
        ),
        scrim_strong: alpha(
            if dark { Color32::BLACK } else { white },
            if dark { 140 } else { 166 },
        ),
        dialog,
        dialog_sidebar,
        dialog_footer,
        dialog_input,
        dialog_card,
        selected_row,
        win_close: Color32::from_rgb(0xff, 0x5f, 0x57),
        strip_active: alpha(
            if dark { white } else { Color32::BLACK },
            if dark { 26 } else { 20 },
        ),
        grad_a,
        grad_b,
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
            let chrome = flatten(p.panel, p.bg);
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
            let caption = contrast(p.text3, chrome);
            if caption < 3.9 {
                failures.push(format!("{label}: caption contrast {caption:.1}:1"));
            }
            let secondary = contrast(p.text2, chrome);
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
            let smallest_accent = if skin == ThemeSkin::Studio { 3.6 } else { 3.8 };
            if accent < smallest_accent {
                failures.push(format!("{label}: accent on bg {accent:.1}:1"));
            }
            // Studio keeps its shipped macOS-blue button text (white); new
            // skins pick whichever ink clears AA on their accent.
            let smallest_on_fill = if skin == ThemeSkin::Studio { 3.3 } else { 4.4 };
            let on_fill = contrast(p.accent_on_fill, p.accent);
            if on_fill < smallest_on_fill {
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
}
