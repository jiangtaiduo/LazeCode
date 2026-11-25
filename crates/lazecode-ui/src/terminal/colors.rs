//! Cell colour resolution: theme palette mapping, contrast and brightening.

use super::prelude::*;

pub(super) fn resolve(color: ColorSpec, p: &crate::theme::Palette) -> Color32 {
    match color {
        ColorSpec::DefaultFg => p.text,
        ColorSpec::DefaultBg => p.terminal_bg,
        ColorSpec::Cursor => p.accent,
        ColorSpec::Indexed(index) => indexed_color(index, p),
        ColorSpec::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    }
}

/// xterm 256-color palette: 0-15 come from the app's ANSI theme, 16-231 are
/// the 6×6×6 cube, 232-255 the grayscale ramp.
pub(super) fn indexed_color(index: u8, p: &crate::theme::Palette) -> Color32 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    if index < 16 {
        return readable_on_terminal(p.ansi[index as usize], p);
    }
    if index < 232 {
        let offset = (index - 16) as usize;
        let r = LEVELS[offset / 36];
        let g = LEVELS[(offset % 36) / 6];
        let b = LEVELS[offset % 6];
        return readable_on_terminal(Color32::from_rgb(r, g, b), p);
    }
    let value = 8 + (index - 232) * 10;
    readable_on_terminal(Color32::from_rgb(value, value, value), p)
}

pub(super) fn luminance(c: Color32) -> f32 {
    let linear = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
}

pub(super) fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// The 6×6×6 cube and the grayscale ramp are authored for dark terminals.
/// On a light canvas, darken low-contrast indexed colours toward the text
/// colour so shell prompts and TUI output stay readable.
pub(super) fn readable_on_terminal(color: Color32, p: &crate::theme::Palette) -> Color32 {
    let bg = p.terminal_bg;
    if luminance(bg) <= 0.5 || contrast(color, bg) >= 3.5 {
        return color;
    }
    let text = p.text;
    for step in 1..=16 {
        let t = step as f32 / 16.0;
        let mix = |a: u8, b: u8| -> u8 {
            (a as f32 + (b as f32 - a as f32) * t)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        let candidate = Color32::from_rgb(
            mix(color.r(), text.r()),
            mix(color.g(), text.g()),
            mix(color.b(), text.b()),
        );
        if contrast(candidate, bg) >= 4.0 {
            return candidate;
        }
    }
    text
}

pub(super) fn brighten(fg: Color32, spec: ColorSpec, p: &crate::theme::Palette) -> Color32 {
    if let ColorSpec::Indexed(index) = spec {
        if index < 8 {
            return p.ansi[index as usize + 8];
        }
    }
    if luminance(p.terminal_bg) > 0.5 {
        fg.gamma_multiply(0.88)
    } else {
        fg.gamma_multiply(1.12)
    }
}
