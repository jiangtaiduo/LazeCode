//! Terminal graphics: cached kitty image textures and placement UV mapping.

use super::prelude::*;

pub struct TerminalOutput {
    pub focus_requested: bool,
    /// The caller must not record this session wake as presented: the
    /// snapshot was held back for a synchronized update or a resize settle.
    pub refresh_pending: bool,
}

/// Per-pane egui texture cache for kitty images, mirroring the terminal's own
/// byte cap.
#[derive(Default)]
pub struct GraphicsCache {
    pub(super) images: HashMap<u32, CachedImage>,
    /// A full re-upload was already requested for the current empty state.
    resynced: bool,
}

pub(super) struct CachedImage {
    pub(super) generation: u64,
    pub(super) texture: egui::TextureHandle,
}

impl GraphicsCache {
    /// Upload decoded images and prune textures the terminal no longer holds.
    pub fn sync(&mut self, ctx: &egui::Context, session: &Session, snapshot: &Snapshot) {
        if self.images.is_empty() && !snapshot.graphics_images.is_empty() && !self.resynced {
            // The frontend lost its textures (a fresh context, a cleared cache)
            // while the terminal still holds pixels: ask for a full re-upload
            // once, then accept the terminal's answer.
            self.resynced = true;
            session.resync_graphics();
        }
        let images = session.take_graphics_updates();
        if !images.is_empty() {
            self.resynced = false;
        }
        self.apply(ctx, images, &snapshot.graphics_images);
    }

    /// Upload `images` and drop every texture not in `live_ids`.
    pub(super) fn apply(
        &mut self,
        ctx: &egui::Context,
        images: Vec<lazecode_terminal::KittyImage>,
        live_ids: &[u32],
    ) {
        let max_side = ctx.input(|input| input.max_texture_side);
        for image in images {
            if image.width == 0
                || image.height == 0
                || image.width > max_side
                || image.height > max_side
            {
                continue;
            }
            if self
                .images
                .get(&image.id)
                .is_some_and(|cached| cached.generation == image.generation)
            {
                continue;
            }
            let pixels =
                egui::ColorImage::from_rgba_unmultiplied([image.width, image.height], &image.rgba);
            let texture = ctx.load_texture(
                format!("kitty-image-{}", image.id),
                pixels,
                egui::TextureOptions::LINEAR,
            );
            self.images.insert(
                image.id,
                CachedImage {
                    generation: image.generation,
                    texture,
                },
            );
        }
        self.images.retain(|id, _| live_ids.contains(id));
    }

    /// Paint the placements that intersect the visible viewport.
    pub(super) fn paint(
        &self,
        painter: &egui::Painter,
        snapshot: &Snapshot,
        inner: Rect,
        char_w: f32,
        line_h: f32,
        rows: usize,
    ) {
        if snapshot.graphics.is_empty() && snapshot.virtual_graphics.is_empty() {
            return;
        }
        for placement in &snapshot.graphics {
            let Some(cached) = self.images.get(&placement.image_id) else {
                continue;
            };
            let row = placement.row as f32 + snapshot.display_offset as f32;
            if row + placement.rows as f32 <= 0.0 || row >= rows as f32 {
                continue;
            }
            let rect = Rect::from_min_size(
                Pos2::new(
                    inner.left() + placement.col as f32 * char_w,
                    inner.top() + row * line_h,
                ),
                Vec2::new(
                    placement.columns as f32 * char_w,
                    placement.rows as f32 * line_h,
                ),
            );
            let size = cached.texture.size_vec2();
            let uv = source_uv(placement, size);
            painter.image(cached.texture.id(), rect, uv, Color32::WHITE);
        }
        // Kitty placeholder runs: rio-vt supplies the sub-cell rect and source
        // slice; painting replaces the placeholder glyphs with the image.
        for run in &snapshot.virtual_graphics {
            let Some(cached) = self.images.get(&run.image_id) else {
                continue;
            };
            let rect = Rect::from_min_size(
                Pos2::new(
                    inner.left() + run.col * char_w,
                    inner.top() + run.row * line_h,
                ),
                Vec2::new(run.columns * char_w, run.rows * line_h),
            );
            let uv = Rect::from_min_max(
                Pos2::new(run.source_uv[0], run.source_uv[1]),
                Pos2::new(run.source_uv[2], run.source_uv[3]),
            );
            painter.image(cached.texture.id(), rect, uv, Color32::WHITE);
        }
    }
}

/// Normalise a kitty source rectangle (`x`,`y`,`w`,`h`; 0 = to the edge) into
/// the texture's UV space.
pub(super) fn source_uv(placement: &ImagePlacement, size: Vec2) -> Rect {
    let (source_x, source_y, source_w, source_h) = placement.source;
    let width = size.x.max(1.0);
    let height = size.y.max(1.0);
    let x = (source_x as f32).min(width);
    let y = (source_y as f32).min(height);
    let crop_w = if source_w > 0 {
        (source_w as f32).min(width - x)
    } else {
        width - x
    };
    let crop_h = if source_h > 0 {
        (source_h as f32).min(height - y)
    } else {
        height - y
    };
    if crop_w <= 0.0 || crop_h <= 0.0 {
        return Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    }
    Rect::from_min_max(
        Pos2::new(x / width, y / height),
        Pos2::new((x + crop_w) / width, (y + crop_h) / height),
    )
}
