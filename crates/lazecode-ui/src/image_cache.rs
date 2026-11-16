//! Bounded, process-local cache for image tabs: downscaled to the viewport,
//! dropped when unreferenced, decoded off-thread, failures cached briefly.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use egui::{ColorImage, Context, TextureHandle, TextureOptions};

/// Keep at most four average 4K thumbnails or many smaller images.
const DEFAULT_BUDGET: usize = 64 * 1024 * 1024;
/// Refuse pathological decodes before `image` allocates the RGBA buffer
/// (64 MP is 256 MiB for the temporary decode).
const MAX_DECODE_PIXELS: u64 = 64 * 1024 * 1024;
const MIN_SIDE: usize = 256;
const MAX_SIDE: usize = 4096;
/// How long a failed decode is remembered before retrying. Retrying every
/// frame turned a single broken image into 60 decode attempts per second.
const FAILED_RETRY_AFTER: Duration = Duration::from_secs(2);

pub struct CachedImage {
    pub texture: TextureHandle,
    pub original_size: [u32; 2],
    /// The longest side requested when this texture was decoded.
    requested_side: usize,
    last_used: u64,
    bytes: usize,
}

enum Entry {
    Ready(CachedImage),
    /// A worker thread is decoding; the texture uploads on a later frame.
    Pending {
        rx: Receiver<DecodeOutcome>,
        last_used: u64,
    },
    /// The last decode failed. Kept so the draw path can show the error and
    /// the cache does not retry per frame.
    Failed {
        error: String,
        retry_at: Instant,
    },
}

type DecodeOutcome = Result<Decoded, String>;

pub struct ImageCache {
    entries: HashMap<PathBuf, Entry>,
    bytes: usize,
    budget: usize,
}

/// What the draw path should render for one image tab this frame.
pub enum ImageLoad<'a> {
    Ready(&'a CachedImage),
    Loading,
    Failed(String),
}

/// What the path currently holds, copied out so the borrow of `entries` is
/// released before the mutable work below.
enum State {
    Ready { requested_side: usize },
    Pending,
    Failed { retry_at: Instant },
    Missing,
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

    /// Drop entries whose tab closed; pending decodes are dropped too and their
    /// worker ends on a closed channel.
    pub fn retain_paths(&mut self, open: &std::collections::HashSet<PathBuf>) {
        let before = self.entries.len();
        self.entries.retain(|path, _| open.contains(path));
        if self.entries.len() != before {
            self.recount_bytes();
        }
    }

    /// Recompute `bytes` from Ready entries; Pending/Failed hold no texture.
    fn recount_bytes(&mut self) {
        self.bytes = self
            .entries
            .values()
            .filter_map(|entry| match entry {
                Entry::Ready(image) => Some(image.bytes),
                _ => None,
            })
            .sum();
    }

    /// Texture sized for the viewport (`max_side` in physical px), decoded on
    /// first use and regenerated after a >25% growth; `Loading` until then.
    pub fn get_or_load(
        &mut self,
        ctx: &Context,
        path: &Path,
        max_side: usize,
        frame: u64,
    ) -> ImageLoad<'_> {
        let target = max_side.clamp(MIN_SIDE, MAX_SIDE);
        let state = match self.entries.get(path) {
            Some(Entry::Ready(entry)) => State::Ready {
                requested_side: entry.requested_side,
            },
            Some(Entry::Pending { .. }) => State::Pending,
            Some(Entry::Failed { retry_at, .. }) => State::Failed {
                retry_at: *retry_at,
            },
            None => State::Missing,
        };
        match state {
            State::Ready { requested_side } => {
                if target > requested_side + requested_side / 4 {
                    self.start_decode(path, target, frame);
                } else if let Some(Entry::Ready(entry)) = self.entries.get_mut(path) {
                    entry.last_used = frame;
                }
            }
            State::Pending => {
                if let Some(Entry::Pending { last_used, .. }) = self.entries.get_mut(path) {
                    *last_used = frame;
                }
                // Take the receiver out to poll it; the entry is put back
                // (either as Ready, Failed, or still Pending) below.
                let rx = match self.entries.remove(path) {
                    Some(Entry::Pending { rx, .. }) => rx,
                    _ => unreachable!("state said Pending"),
                };
                match rx.try_recv() {
                    Ok(Ok(decoded)) => self.finish_decode(ctx, path, decoded, target, frame),
                    Ok(Err(error)) => {
                        self.entries.insert(
                            path.to_path_buf(),
                            Entry::Failed {
                                error,
                                retry_at: Instant::now() + FAILED_RETRY_AFTER,
                            },
                        );
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        // The worker died without sending; treat it like a
                        // failed decode so the UI shows why.
                        self.entries.insert(
                            path.to_path_buf(),
                            Entry::Failed {
                                error: "image decode worker ended unexpectedly".to_string(),
                                retry_at: Instant::now() + FAILED_RETRY_AFTER,
                            },
                        );
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        self.entries.insert(
                            path.to_path_buf(),
                            Entry::Pending {
                                rx,
                                last_used: frame,
                            },
                        );
                    }
                }
            }
            State::Failed { retry_at } => {
                if Instant::now() >= retry_at {
                    self.start_decode(path, target, frame);
                }
            }
            State::Missing => self.start_decode(path, target, frame),
        }
        match self.entries.get(path) {
            Some(Entry::Ready(entry)) => ImageLoad::Ready(entry),
            Some(Entry::Pending { .. }) => ImageLoad::Loading,
            Some(Entry::Failed { error, .. }) => ImageLoad::Failed(error.clone()),
            None => ImageLoad::Loading,
        }
    }

    /// Replace the path's entry with a Pending decode, subtracting any Ready
    /// texture so the budget never double-counts.
    fn start_decode(&mut self, path: &Path, target: usize, frame: u64) {
        if let Some(Entry::Ready(old)) = self.entries.get(path) {
            self.bytes = self.bytes.saturating_sub(old.bytes);
        }
        let (tx, rx) = channel();
        let job_path = path.to_path_buf();
        let spawned = std::thread::Builder::new()
            .name("lazecode-image-decode".to_string())
            .spawn(move || {
                let outcome = decode(&job_path, target);
                let _ = tx.send(outcome);
            });
        let entry = if spawned.is_ok() {
            Entry::Pending {
                rx,
                last_used: frame,
            }
        } else {
            Entry::Failed {
                error: "cannot spawn image decode worker".to_string(),
                retry_at: Instant::now() + FAILED_RETRY_AFTER,
            }
        };
        self.entries.insert(path.to_path_buf(), entry);
    }

    /// Upload a finished decode as a texture, then enforce the budget.
    fn finish_decode(
        &mut self,
        ctx: &Context,
        path: &Path,
        decoded: Decoded,
        target: usize,
        frame: u64,
    ) {
        let bytes = decoded.image.width() * decoded.image.height() * 4;
        let texture = ctx.load_texture(
            format!("image-tab:{}", path.display()),
            decoded.image,
            TextureOptions::LINEAR,
        );
        self.entries.insert(
            path.to_path_buf(),
            Entry::Ready(CachedImage {
                texture,
                original_size: decoded.original_size,
                requested_side: target,
                last_used: frame,
                bytes,
            }),
        );
        self.bytes += bytes;
        self.evict(path);
    }

    fn evict(&mut self, keep: &Path) {
        while self.bytes > self.budget && self.entries.len() > 1 {
            let victim = self
                .entries
                .iter()
                .filter(|(path, entry)| path.as_path() != keep && matches!(entry, Entry::Ready(_)))
                .min_by_key(|(_, entry)| match entry {
                    Entry::Ready(image) => image.last_used,
                    _ => u64::MAX,
                })
                .map(|(path, _)| path.clone());
            let Some(victim) = victim else {
                break;
            };
            if let Some(Entry::Ready(image)) = self.entries.remove(&victim) {
                self.bytes = self.bytes.saturating_sub(image.bytes);
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

    fn temp_png(name: &str, width: u32, height: u32) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "lazecode-image-cache-{}-{name}.png",
            std::process::id()
        ));
        image::RgbaImage::from_pixel(width, height, image::Rgba([20, 40, 60, 255]))
            .save(&path)
            .unwrap();
        path
    }

    /// Pump the cache until the pending decode lands (or the wait times out).
    fn wait_ready(cache: &mut ImageCache, ctx: &Context, path: &Path, max_side: usize) {
        for _ in 0..500 {
            if matches!(
                cache.get_or_load(ctx, path, max_side, 1),
                ImageLoad::Ready(_)
            ) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("image did not finish decoding in time");
    }

    #[test]
    fn downscales_and_drops_closed_entries() {
        let path = temp_png("down", 512, 256);
        let ctx = Context::default();
        let mut cache = ImageCache::with_budget(16 * 1024 * 1024);
        wait_ready(&mut cache, &ctx, &path, 256);
        let image = match cache.get_or_load(&ctx, &path, 256, 1) {
            ImageLoad::Ready(image) => image,
            _ => panic!("expected ready"),
        };
        assert_eq!(image.original_size, [512, 256]);
        assert_eq!(image.texture.size(), [256, 128]);
        cache.retain_paths(&HashSet::new());
        assert!(cache.is_empty());
        assert_eq!(cache.bytes, 0);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn evicts_least_recently_used_entry_under_budget() {
        let first = temp_png("evict-a", 64, 64);
        let second = temp_png("evict-b", 32, 32);
        let ctx = Context::default();
        let mut cache = ImageCache::with_budget(1);
        wait_ready(&mut cache, &ctx, &first, 64);
        wait_ready(&mut cache, &ctx, &second, 32);
        assert_eq!(cache.len(), 1);
        assert!(cache.entries.contains_key(&second));
        let _ = std::fs::remove_file(first);
        let _ = std::fs::remove_file(second);
    }

    #[test]
    fn reload_replaces_byte_accounting_instead_of_accumulating() {
        let path = temp_png("reload", 512, 512);
        let ctx = Context::default();
        let mut cache = ImageCache::with_budget(64 * 1024 * 1024);
        wait_ready(&mut cache, &ctx, &path, 256);
        let first_bytes = cache.bytes;
        assert!(first_bytes > 0);
        // Growing the viewport by more than 25% regenerates the thumbnail.
        wait_ready(&mut cache, &ctx, &path, 512);
        assert_eq!(
            cache.bytes,
            first_bytes * 4,
            "bytes must reflect only the live texture, not accumulate across reloads"
        );
        cache.retain_paths(&HashSet::new());
        assert_eq!(cache.bytes, 0);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn failed_decode_is_negatively_cached() {
        let path = std::env::temp_dir().join(format!(
            "lazecode-image-cache-{}-broken.png",
            std::process::id()
        ));
        std::fs::write(&path, b"this is not a png").unwrap();
        let ctx = Context::default();
        let mut cache = ImageCache::with_budget(16 * 1024 * 1024);
        let mut error = None;
        for _ in 0..500 {
            if let ImageLoad::Failed(err) = cache.get_or_load(&ctx, &path, 256, 1) {
                error = Some(err);
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let error = error.expect("decode should have failed");
        // Immediately repeated calls reuse the failure instead of re-decoding
        // (no new Pending entry, identical error).
        for _ in 0..3 {
            match cache.get_or_load(&ctx, &path, 256, 1) {
                ImageLoad::Failed(err) => assert_eq!(err, error),
                _ => panic!("failed entry must stay failed within the retry window"),
            }
        }
        assert!(matches!(
            cache.entries.get(&path),
            Some(Entry::Failed { .. })
        ));
        let _ = std::fs::remove_file(path);
    }
}
