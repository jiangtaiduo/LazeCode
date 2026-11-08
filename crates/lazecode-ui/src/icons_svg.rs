//! Stable-app icon sources + rasterization.
//!
//! Every SVG below is extracted verbatim from the stable frontend
//! (`src/components/FileIcons.tsx` on branch `main`), so icon geometry is
//! pixel-identical to the web version. Monochrome icons (the stable set
//! parameterises colour via `currentColor`) are normalised to white strokes
//! and rasterised once into an alpha texture; the draw site tints the
//! texture with the active palette colour. Multicolour icons (brand file
//! icons, folders) are rasterised as-is and never tinted.
//!
//! Rasterization uses `resvg` (pure Rust) at 2× (32×32 for a 16×16 viewBox)
//! so @2x displays render crisply; drawing scales the texture down with a
//! linear filter.

use egui::{Color32, Context, Pos2, Rect, TextureHandle, TextureOptions};

/// Raster size for a 16×16 viewBox (2× for Retina).
const RASTER: f32 = 32.0;

macro_rules! mono {
    ($sw:expr, $body:expr) => {
        concat!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 16 16\" fill=\"none\">",
            "<g stroke=\"#fff\" stroke-width=\"",
            $sw,
            "\" stroke-linecap=\"round\" stroke-linejoin=\"round\" fill=\"none\">",
            $body,
            "</g></svg>"
        )
    };
}

/// Cached rasterized textures, keyed by icon name. The app has a single
/// egui context, so one process-wide map is enough.
fn texture_cache(
) -> std::sync::MutexGuard<'static, std::collections::HashMap<&'static str, TextureHandle>> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<&'static str, TextureHandle>>,
    > = std::sync::OnceLock::new();
    CACHE
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// (name, svg source, multicolor). Multicolor sources carry their own
/// palette and must be drawn without tint.
static SVGS: &[(&str, &str, bool)] = &[
    (
        "project",
        mono!("1.4", "<path d=\"M1.9 4.35c0-.8.65-1.45 1.45-1.45h2.8c.42 0 .82.18 1.1.5l.74.85c.28.32.68.5 1.1.5h4.36c.8 0 1.45.65 1.45 1.45v6.6c0 .8-.65 1.45-1.45 1.45H3.35c-.8 0-1.45-.65-1.45-1.45V4.35z\"/><path d=\"M2.1 6.55h11.8\" stroke-width=\"1.2\" opacity=\"0.5\"/>"),
        false,
    ),
    (
        "branch",
        mono!("1.5", "<circle cx=\"4.5\" cy=\"3.5\" r=\"2\"/><circle cx=\"4.5\" cy=\"12.5\" r=\"2\"/><circle cx=\"11.5\" cy=\"5.5\" r=\"2\"/><path d=\"M4.5 5.5v5\"/><path d=\"M4.5 7.5c0 2 2.5 3 7 3v-3\"/>"),
        false,
    ),
    (
        "commit",
        mono!("1.4", "<circle cx=\"8\" cy=\"8\" r=\"3.1\"/><path d=\"M1.6 8h3.2\"/><path d=\"M11.2 8h3.2\"/>"),
        false,
    ),
    (
        "gitlog",
        mono!("1.35", "<circle cx=\"4.8\" cy=\"3.4\" r=\"1.6\"/><circle cx=\"4.8\" cy=\"12.6\" r=\"1.6\"/><circle cx=\"11.6\" cy=\"8\" r=\"1.6\"/><path d=\"M4.8 5v5.9\"/><path d=\"M6.4 3.6h2.9c1.25 0 2.3 1.05 2.3 2.3v0.55\"/>"),
        false,
    ),
    (
        "terminal",
        mono!("1.4", "<rect x=\"1.9\" y=\"2.7\" width=\"12.2\" height=\"10.6\" rx=\"1.7\"/><path d=\"M4.7 6.1l2.4 2-2.4 2\"/><path d=\"M9.2 10.2h2.7\"/>"),
        false,
    ),
    (
        "gear",
        mono!("1.2", "<circle cx=\"8\" cy=\"8\" r=\"2.5\" stroke-width=\"1.3\"/><path d=\"M6.8 1.8h2.4l.35 1.5c.38.13.73.32 1.04.55l1.4-.6 1.7 1.7-.6 1.4c.23.31.42.66.55 1.04l1.5.35v2.4l-1.5.35c-.13.38-.32.73-.55 1.04l.6 1.4-1.7 1.7-1.4-.6c-.31.23-.66.42-1.04.55l-.35 1.5H6.8l-.35-1.5c-.38-.13-.73-.32-1.04-.55l-1.4.6-1.7-1.7.6-1.4c-.23-.31-.42-.66-.55-1.04l-1.5-.35V8.9l1.5-.35c.13-.38.32-.73.55-1.04l-.6-1.4 1.7-1.7 1.4.6c.31-.23.66-.42 1.04-.55l.35-1.5z\"/>"),
        false,
    ),
    (
        "refresh",
        mono!("1.4", "<path d=\"M14 8a6 6 0 1 1-6-6c1.68 0 3.29.67 4.49 1.83L14 5.33\"/><path d=\"M14 2v3.33h-3.33\"/>"),
        false,
    ),
    (
        "diff",
        mono!("1.4", "<rect x=\"2\" y=\"2\" width=\"5.2\" height=\"12\" rx=\"1\"/><rect x=\"8.8\" y=\"2\" width=\"5.2\" height=\"12\" rx=\"1\"/><path d=\"M3.6 5.5h2M3.6 8h2M3.6 10.5h2\"/><path d=\"M10.4 5.5h2M10.4 8h2M10.4 10.5h2\"/>"),
        false,
    ),
    (
        "merge",
        mono!("1.4", "<circle cx=\"4.2\" cy=\"3.6\" r=\"1.7\"/><circle cx=\"4.2\" cy=\"12.4\" r=\"1.7\"/><circle cx=\"11.8\" cy=\"8\" r=\"1.7\"/><path d=\"M4.2 5.3v5.4\"/><path d=\"M5.6 4.3c1 .6 1.6 1.5 2.3 2.4.7.9 1.4 1.5 2.4 1.8\"/><path d=\"M5.6 11.7c1-.6 1.6-1.5 2.3-2.4.7-.9 1.4-1.5 2.4-1.8\"/>"),
        false,
    ),
    (
        "search",
        mono!("1.4", "<circle cx=\"7\" cy=\"7\" r=\"4.6\"/><path d=\"M10.4 10.4 13.8 13.8\"/>"),
        false,
    ),
    (
        "plus",
        mono!("1.4", "<path d=\"M8 3v10M3 8h10\"/>"),
        false,
    ),
    (
        "close_small",
        mono!("1.5", "<path d=\"M4 4l8 8M12 4l-8 8\"/>"),
        false,
    ),
    (
        "chevron_down",
        mono!("1.4", "<path d=\"M4 6l4 4 4-4\"/>"),
        false,
    ),
    (
        "chevron_up",
        mono!("1.4", "<path d=\"M4 10l4-4 4 4\"/>"),
        false,
    ),
    (
        "chevron_right",
        mono!("1.4", "<path d=\"M6 4l4 4-4 4\"/>"),
        false,
    ),
    (
        "play",
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 16 16\" fill=\"none\"><path d=\"M4.5 2.8v10.4L13 8 4.5 2.8z\" fill=\"#fff\"/></svg>",
        false,
    ),
    (
        "folder",
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 16 16\" fill=\"none\"><path d=\"M2 4.4c0-.77.63-1.4 1.4-1.4h2.9l1.6 1.6h4.7c.77 0 1.4.63 1.4 1.4v6c0 .77-.63 1.4-1.4 1.4H3.4c-.77 0-1.4-.63-1.4-1.4V4.4Z\" fill=\"#7d8794\" fill-opacity=\"0.16\" stroke=\"#7d8794\" stroke-width=\"1.1\" stroke-linejoin=\"round\"/></svg>",
        true,
    ),
    (
        "folder_open",
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 16 16\" fill=\"none\"><path d=\"M2 10.4V4.5c0-.72.58-1.3 1.3-1.3h2.8l1.6 1.6h4.9c.72 0 1.3.58 1.3 1.3v1.5H4.8c-.62 0-1.16.42-1.31 1.02L2 10.4Z\" fill=\"#7d8794\" fill-opacity=\"0.16\" stroke=\"#7d8794\" stroke-width=\"1.1\" stroke-linejoin=\"round\"/><path d=\"M4.9 8.3h8.9c.86 0 1.45.85 1.16 1.65l-1.05 2.85c-.19.5-.66.84-1.19.84H3.7c-.83 0-1.4-.83-1.13-1.61l.98-2.8c.18-.53.65-.93 1.35-.93Z\" fill=\"#7d8794\" fill-opacity=\"0.16\" stroke=\"#7d8794\" stroke-width=\"1.1\" stroke-linejoin=\"round\"/></svg>",
        true,
    ),
    (
        "text_file",
        mono!("1.2", "<path d=\"M3.5 2.5h6l3 3v8a1 1 0 0 1-1 1h-8a1 1 0 0 1-1-1v-10a1 1 0 0 1 1-1z\"/><path d=\"M9.5 2.5v3h3\"/><line x1=\"5.5\" y1=\"7.5\" x2=\"10.5\" y2=\"7.5\" stroke-width=\"1\"/><line x1=\"5.5\" y1=\"9.5\" x2=\"10.5\" y2=\"9.5\" stroke-width=\"1\"/><line x1=\"5.5\" y1=\"11.5\" x2=\"8.5\" y2=\"11.5\" stroke-width=\"1\"/>"),
        false,
    ),
    (
        "copy",
        mono!("1.2", "<rect x=\"5.5\" y=\"5.5\" width=\"8\" height=\"8\" rx=\"1\"/><path d=\"M10.5 5.5V3.5a1 1 0 0 0-1-1h-6a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2\"/>"),
        false,
    ),
    (
        "more_vert",
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 16 16\" fill=\"none\"><g fill=\"#fff\"><circle cx=\"8\" cy=\"3.5\" r=\"1.25\"/><circle cx=\"8\" cy=\"8\" r=\"1.25\"/><circle cx=\"8\" cy=\"12.5\" r=\"1.25\"/></g></svg>",
        false,
    ),
    (
        "magic_wand",
        mono!("1.5", "<path d=\"M3 13L9.5 6.5\"/><path d=\"M9.5 6.5l1-1a1 1 0 0 1 1.4 0l.6.6a1 1 0 0 1 0 1.4l-1 1\" stroke-width=\"1.2\"/><path d=\"M12 2v2M11 3h2M5 2.5l.5.5M14 6.5l-.5.5M8 1v1.5\" stroke-width=\"1.2\"/>"),
        false,
    ),
    (
        "history",
        mono!("1.4", "<circle cx=\"8\" cy=\"8\" r=\"6.2\"/><path d=\"M8 4.4V8l2.6 1.4\"/>"),
        false,
    ),
    (
        "arrow_up",
        mono!("1.5", "<path d=\"M8 12.5V3.5M3.5 8L8 3.5 12.5 8\"/>"),
        false,
    ),
    (
        "arrow_down",
        mono!("1.5", "<path d=\"M8 3.5V12.5M3.5 8L8 12.5 12.5 8\"/>"),
        false,
    ),
    (
        "arrow_right",
        mono!("1.5", "<path d=\"M3.5 8H12.5M8 3.5L12.5 8L8 12.5\"/>"),
        false,
    ),
    (
        "tag",
        mono!("1.4", "<path d=\"M2.5 7V3a.5.5 0 0 1 .5-.5h4a1 1 0 0 1 .71.29l5.79 5.8a1 1 0 0 1 0 1.41l-4 4a1 1 0 0 1-1.41 0l-5.8-5.8a1 1 0 0 1-.29-.7z\"/><circle cx=\"5.5\" cy=\"5.5\" r=\"1.1\" fill=\"#fff\" stroke=\"none\"/>"),
        false,
    ),
    (
        "trash",
        mono!("1.4", "<path d=\"M2.8 4.5h10.4\"/><path d=\"M6.2 4.5V3.3a1.2 1.2 0 0 1 1.2-1.2h1.2a1.2 1.2 0 0 1 1.2 1.2v1.2\"/><path d=\"M4.1 4.5l.6 8.2a1.3 1.3 0 0 0 1.3 1.2h4a1.3 1.3 0 0 0 1.3-1.2l.6-8.2\"/><path d=\"M6.5 7v3.9M9.5 7v3.9\"/>"),
        false,
    ),
    (
        "window_min",
        mono!("1.4", "<path d=\"M3.5 11h9\"/>"),
        false,
    ),
    (
        "window_max",
        mono!("1.4", "<rect x=\"3.5\" y=\"3.5\" width=\"9\" height=\"9\" rx=\"1\"/>"),
        false,
    ),
    (
        "new_window",
        mono!("1.4", "<rect x=\"2\" y=\"5.5\" width=\"8.5\" height=\"8.5\" rx=\"1\"/><path d=\"M5.5 5.5V4a1 1 0 0 1 1-1H13a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1h-1.5\"/>"),
        false,
    ),
    (
        "expand_all",
        mono!("1.3", "<path d=\"M3.5 6L8 2.5l4.5 3.5\"/><path d=\"M3.5 10L8 13.5l4.5-3.5\"/>"),
        false,
    ),
    (
        "collapse_all",
        mono!("1.3", "<path d=\"M3.5 3.2L8 6.6l4.5-3.4\"/><path d=\"M3.5 12.8L8 9.4l4.5 3.4\"/>"),
        false,
    ),
    (
        "double_chevron_left",
        mono!("2.0", "<path d=\"M7.5 3.5L3 8L7.5 12.5\"/><path d=\"M12 3.5L7.5 8L12 12.5\"/>"),
        false,
    ),
    (
        "double_chevron_right",
        mono!("2.0", "<path d=\"M4 3.5L8.5 8L4 12.5\"/><path d=\"M8.5 3.5L13 8L8.5 12.5\"/>"),
        false,
    ),
    (
        "warning",
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 16 16\" fill=\"none\"><path d=\"M8 1.8 15 14H1L8 1.8z\" fill=\"#fff\" opacity=\"0.9\"/><path d=\"M8 5.6v4.2\" stroke=\"#2b2d30\" stroke-width=\"1.5\" stroke-linecap=\"round\"/><circle cx=\"8\" cy=\"12\" r=\"0.9\" fill=\"#2b2d30\"/></svg>",
        false,
    ),
    (
        "star",
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 16 16\" fill=\"none\"><path d=\"M8 1.8l1.9 3.85 4.25.62-3.07 3 .72 4.23L8 11.53l-3.8 1.97.72-4.23-3.07-3 4.25-.62L8 1.8z\" fill=\"#fff\"/></svg>",
        false,
    ),
    (
        "compare",
        mono!("1.3", "<path d=\"M2.5 4.5h5.5M2.5 8h3.5M2.5 11.5h5.5\"/><path d=\"M6.5 8h6.5M10.5 5.5l2.5 2.5-2.5 2.5\"/>"),
        false,
    ),
    (
        "rollback",
        mono!("1.4", "<path d=\"M5.5 3.5L2 7l3.5 3.5\"/><path d=\"M2 7h7.2a3.8 3.8 0 0 1 3.8 3.8v0a3.8 3.8 0 0 1-3.8 3.8H6.5\"/>"),
        false,
    ),
    (
        "dock_bottom",
        mono!("1.4", "<rect x=\"2\" y=\"3\" width=\"12\" height=\"10\" rx=\"1.2\"/><path d=\"M2 9.6h12\"/><rect x=\"3.1\" y=\"10.7\" width=\"9.8\" height=\"1.6\" rx=\"0.5\" fill=\"#fff\" stroke=\"none\"/>"),
        false,
    ),
];

fn find(name: &str) -> Option<(&'static str, bool)> {
    SVGS.iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, s, m)| (*s, *m))
}

fn blank(ctx: &Context, name: &str) -> TextureHandle {
    log::warn!("svg icon missing or unparsable: {name}");
    ctx.load_texture(
        format!("icon-{name}"),
        egui::ColorImage::new([1, 1], vec![Color32::TRANSPARENT]),
        TextureOptions::LINEAR,
    )
}

/// Rasterize a stable icon at 2× and upload it as an egui texture.
pub fn rasterize(ctx: &Context, name: &str) -> TextureHandle {
    let Some((svg, _)) = find(name) else {
        return blank(ctx, name);
    };
    let opts = resvg::usvg::Options::default();
    let tree = match resvg::usvg::Tree::from_str(svg, &opts) {
        Ok(tree) => tree,
        Err(err) => {
            log::warn!("svg icon {name} failed to parse: {err}");
            return blank(ctx, name);
        }
    };
    let (w, h) = (RASTER as u32, RASTER as u32);
    let Some(mut pixmap) = resvg::tiny_skia::Pixmap::new(w, h) else {
        return blank(ctx, name);
    };
    let sx = RASTER / tree.size().width();
    let sy = RASTER / tree.size().height();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(sx, sy),
        &mut pixmap.as_mut(),
    );
    // tiny-skia stores premultiplied RGBA; egui wants unmultiplied.
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for px in pixmap.data().as_chunks::<4>().0 {
        let a = px[3];
        let (r, g, b) = if a == 0 {
            (0, 0, 0)
        } else {
            let un = |c: u8| ((c as u32 * 255 + a as u32 / 2) / a as u32) as u8;
            (un(px[0]), un(px[1]), un(px[2]))
        };
        pixels.extend_from_slice(&[r, g, b, a]);
    }
    let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels);
    ctx.load_texture(format!("icon-{name}"), image, TextureOptions::LINEAR)
}

/// Blit a stable icon into `rect`, tinting monochrome sources with `tint`.
pub fn paint_svg(painter: &egui::Painter, rect: Rect, name: &'static str, tint: Color32) {
    let multicolor = find(name).map(|(_, m)| m).unwrap_or(false);
    let tint = if multicolor { Color32::WHITE } else { tint };
    let texture = {
        let mut cache = texture_cache();
        if let Some(texture) = cache.get(name) {
            texture.clone()
        } else {
            let texture = rasterize(painter.ctx(), name);
            cache.insert(name, texture.clone());
            texture
        }
    };
    painter.image(
        texture.id(),
        rect,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        tint,
    );
}
