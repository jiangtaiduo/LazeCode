//! Bounded, process-local cache for image tabs.
//!
//! `egui_extras` keeps decoded images alive in its global loader cache until a
//! texture is explicitly forgotten; closing a tab only drops the texture
//! handle. Large photos therefore stayed resident after their projects were
//! closed. Image tabs use this cache instead: pixels are downscaled to the
//! visible viewport, the retained bytes are capped, and entries are dropped as
//! soon as no tab references them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{ColorImage, Context, TextureHandle, TextureOptions};

/// Keep at most four average 4K thumbnails or many smaller images.
const DEFAULT_BUDGET: usize = 64 * 1024 * 1024;
/// Refuse pathological decodes before `image` allocates the full RGBA buffer.
/// 64 MP is 256 MiB for the temporary decode; the retained thumbnail is much
/// smaller.
const MAX_DECODE_PIXELS: u64 = 64 * 1024 * 1024;
const MIN_SIDE: usize = 256;
const MAX_SIDE: usize = 4096;

pub struct CachedImage {
    pub texture: TextureHandle,
    pub original_size: [u32; 2],
    /// The longest side requested when this texture was decoded.
    requested_side: usize,
    last_used: u64,
    bytes: usize,
}

pub struct ImageCache {
    entries: HashMap<PathBuf, CachedImage>,
    bytes: usize,
    budget: usize,
}

impl Default for ImageCache {
    fn default() -> Self {
        Self::with_budget(DEFAULT_BUDGET)
    }
}

impl ImageCache {
    pub fn with_budget(budget: usize) -> Self {
        Self {
            entries: HashMap::new(),
            bytes: 0,
            budget,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }

    /// Drop entries for images that no longer have an open tab.
    pub fn retain_paths(&mut self, open: &std::collections::HashSet<PathBuf>) {
        let before = self.entries.len();
        self.entries.retain(|path, _| open.contains(path));
        if self.entries.len() != before {
            self.bytes = self.entries.values().map(|entry| entry.bytes).sum();
        }
    }

    /// Return a texture sized for the current viewport, decoding on first use.
    ///
    /// `max_side` is in physical pixels. Once the window grows by more than
    /// 25%, the thumbnail is regenerated; shrinking keeps the existing texture
    /// to avoid resize churn.
    pub fn get_or_load(
        &mut self,
        ctx: &Context,
        path: &Path,
        max_side: usize,
        frame: u64,
    ) -> Result<&CachedImage, String> {
        let target = max_side.clamp(MIN_SIDE, MAX_SIDE);
        let reload = match self.entries.get(path) {
            Some(entry) => target > entry.requested_side + entry.requested_side / 4,
            None => true,
        };
        if reload {
            self.entries.remove(path);
            let decoded = decode(path, target)?;
            let bytes = decoded.image.width() * decoded.image.height() * 4;
            let texture = ctx.load_texture(
                format!("image-tab:{}", path.display()),
                decoded.image,
                TextureOptions::LINEAR,
            );
            self.entries.insert(
                path.to_path_buf(),
                CachedImage {
                    texture,
                    original_size: decoded.original_size,
                    requested_side: target,
                    last_used: frame,
                    bytes,
                },
            );
            self.bytes += bytes;
            self.evict(path);
        } else if let Some(entry) = self.entries.get_mut(path) {
            entry.last_used = frame;
        }
        self.entries
            .get(path)
            .ok_or_else(|| "image cache insert failed".to_string())
    }

    fn evict(&mut self, keep: &Path) {
        while self.bytes > self.budget && self.entries.len() > 1 {
            let victim = self
                .entries
                .iter()
                .filter(|(path, _)| path.as_path() != keep)
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(path, _)| path.clone());
            let Some(victim) = victim else {
                break;
            };
            if let Some(entry) = self.entries.remove(&victim) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
            }
        }
    }
}

struct Decoded {
    image: ColorImage,
    original_size: [u32; 2],
}

fn decode(path: &Path, target: usize) -> Result<Decoded, String> {
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
    {
        return decode_svg(path, target);
    }

    let (width, height) =
        image::image_dimensions(path).map_err(|err| format!("image dimensions: {err}"))?;
    if width == 0 || height == 0 {
        return Err("image has zero size".to_string());
    }
    if u64::from(width).saturating_mul(u64::from(height)) > MAX_DECODE_PIXELS {
        return Err(format!(
            "image is too large to decode ({} x {} pixels)",
            width, height
        ));
    }

    let source = image::ImageReader::open(path)
        .map_err(|err| format!("open image: {err}"))?
        .with_guessed_format()
        .map_err(|err| format!("detect image format: {err}"))?
        .decode()
        .map_err(|err| format!("decode image: {err}"))?;
    let (tw, th) = fit_dimensions(width, height, target);
    let rgba = if tw == width && th == height {
        source.to_rgba8()
    } else {
        source.thumbnail(tw, th).to_rgba8()
    };
    Ok(Decoded {
        image: ColorImage::from_rgba_unmultiplied([tw as usize, th as usize], rgba.as_raw()),
        original_size: [width, height],
    })
}

fn decode_svg(path: &Path, target: usize) -> Result<Decoded, String> {
    let data = std::fs::read(path).map_err(|err| format!("read SVG: {err}"))?;
    let options = resvg::usvg::Options::default();
    let tree =
        resvg::usvg::Tree::from_data(&data, &options).map_err(|err| format!("parse SVG: {err}"))?;
    let size = tree.size();
    let width = size.width().max(1.0);
    let height = size.height().max(1.0);
    let scale = target as f32 / width.max(height);
    let tw = (width * scale).ceil().max(1.0) as u32;
    let th = (height * scale).ceil().max(1.0) as u32;
    let Some(mut pixmap) = resvg::tiny_skia::Pixmap::new(tw.max(1), th.max(1)) else {
        return Err("SVG is too large to rasterize".to_string());
    };
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let mut pixels = Vec::with_capacity((tw * th * 4) as usize);
    for px in pixmap.data().as_chunks::<4>().0 {
        let alpha = px[3];
        let un = |channel: u8| {
            if alpha == 0 {
                0
            } else {
                ((u32::from(channel) * 255 + u32::from(alpha) / 2) / u32::from(alpha)) as u8
            }
        };
        pixels.extend_from_slice(&[un(px[0]), un(px[1]), un(px[2]), alpha]);
    }
    Ok(Decoded {
        image: ColorImage::from_rgba_unmultiplied([tw as usize, th as usize], &pixels),
        original_size: [width.round() as u32, height.round() as u32],
    })
}

fn fit_dimensions(width: u32, height: u32, max_side: usize) -> (u32, u32) {
    let longest = width.max(height) as f64;
    let scale = (max_side as f64 / longest).min(1.0);
    (
        (f64::from(width) * scale).round().max(1.0) as u32,
        (f64::from(height) * scale).round().max(1.0) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn temp_png(width: u32, height: u32) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "lazecode-image-cache-{}-{}.png",
            std::process::id(),
            width * 10_000 + height
        ));
        image::RgbaImage::from_pixel(width, height, image::Rgba([20, 40, 60, 255]))
            .save(&path)
            .unwrap();
        path
    }

    #[test]
    fn downscales_and_drops_closed_entries() {
        let path = temp_png(512, 256);
        let ctx = Context::default();
        let mut cache = ImageCache::with_budget(16 * 1024 * 1024);
        let image = cache.get_or_load(&ctx, &path, 256, 1).unwrap();
        assert_eq!(image.original_size, [512, 256]);
        assert_eq!(image.texture.size(), [256, 128]);
        cache.retain_paths(&HashSet::new());
        assert!(cache.is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn evicts_least_recently_used_entry_under_budget() {
        let first = temp_png(64, 64);
        let second = temp_png(32, 32);
        let ctx = Context::default();
        let mut cache = ImageCache::with_budget(1);
        cache.get_or_load(&ctx, &first, 64, 1).unwrap();
        cache.get_or_load(&ctx, &second, 32, 2).unwrap();
        assert_eq!(cache.len(), 1);
        assert!(cache.entries.contains_key(&second));
        let _ = std::fs::remove_file(first);
        let _ = std::fs::remove_file(second);
    }
}
