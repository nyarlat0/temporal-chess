use crate::view::{CAPTURE, INFORMATION, ORDINARY, REACTION};
use bevy_egui::egui::{self, Color32, Pos2, Rect, TextureHandle, Vec2};
use std::{collections::HashMap, sync::Mutex};
use temporal_chess_engine::model::{Color, PieceKind};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Outline {
    #[default]
    None,
    Ordinary,
    Information,
    Cancelled,
    Check,
    InvertedCheck,
    Capture,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PieceStyle {
    pub inverted: bool,
    pub outline: Outline,
    pub hovered: bool,
}

impl Outline {
    fn color(self) -> Color32 {
        match self {
            Self::None => Color32::TRANSPARENT,
            Self::Ordinary => ORDINARY,
            Self::Information => INFORMATION,
            Self::Cancelled => REACTION,
            Self::Check | Self::Capture => CAPTURE,
            Self::InvertedCheck => Color32::from_rgb(239, 137, 185),
        }
    }
}

/// Embedded, conventional chess artwork. Rasterized once; no network or asset
/// directory lookup is needed when running the game.
pub struct PieceArt {
    sources: [&'static [u8]; 12],
    textures: Mutex<TextureCache>,
}

#[derive(Default)]
struct TextureCache {
    entries: HashMap<(usize, usize, PieceStyle), CachedTexture>,
    pruned_frame: Option<u64>,
}

struct CachedTexture {
    handle: TextureHandle,
    last_used: u64,
}

impl PieceArt {
    pub fn new(_ctx: &egui::Context) -> Self {
        let sources: [&[u8]; 12] = [
            include_bytes!("../assets/pieces/cburnett/wK.svg"),
            include_bytes!("../assets/pieces/cburnett/wQ.svg"),
            include_bytes!("../assets/pieces/cburnett/wR.svg"),
            include_bytes!("../assets/pieces/cburnett/wB.svg"),
            include_bytes!("../assets/pieces/cburnett/wN.svg"),
            include_bytes!("../assets/pieces/cburnett/wP.svg"),
            include_bytes!("../assets/pieces/cburnett/bK.svg"),
            include_bytes!("../assets/pieces/cburnett/bQ.svg"),
            include_bytes!("../assets/pieces/cburnett/bR.svg"),
            include_bytes!("../assets/pieces/cburnett/bB.svg"),
            include_bytes!("../assets/pieces/cburnett/bN.svg"),
            include_bytes!("../assets/pieces/cburnett/bP.svg"),
        ];
        Self {
            sources,
            textures: Mutex::new(TextureCache::default()),
        }
    }

    pub fn paint(
        &self,
        painter: &egui::Painter,
        kind: PieceKind,
        side: Color,
        rect: Rect,
        alpha: f32,
    ) {
        self.paint_styled(painter, kind, side, rect, alpha, PieceStyle::default());
    }

    #[allow(clippy::too_many_arguments)]
    pub fn paint_styled(
        &self,
        painter: &egui::Painter,
        kind: PieceKind,
        side: Color,
        rect: Rect,
        alpha: f32,
        style: PieceStyle,
    ) {
        let kind = match kind {
            PieceKind::K { .. } => 0,
            PieceKind::Q => 1,
            PieceKind::R { .. } => 2,
            PieceKind::B => 3,
            PieceKind::N => 4,
            PieceKind::P { .. } => 5,
        };
        let index = kind + if side == Color::Black { 6 } else { 0 };
        // Native vector edge coverage, then one texel per physical pixel. Avoid
        // a second low-pass filter from downsampling and fractional placement.
        let scale = painter.ctx().pixels_per_point();
        let size = (rect.width() * scale).round().clamp(16.0, 384.0) as usize;
        let rect = pixel_aligned(rect, scale, size);
        // Both hover states live in one texture. Entering a square changes
        // only UVs, never a GPU texture allocation or bindless texture slot.
        let highlighted = style.hovered && style.outline != Outline::None;
        let style = PieceStyle {
            hovered: false,
            ..style
        };
        let key = (index, size, style);
        let frame = painter.ctx().cumulative_frame_nr();
        let mut cache = self.textures.lock().expect("piece textures");
        if cache.pruned_frame != Some(frame) {
            cache.pruned_frame = Some(frame);
            if cache.entries.len() > 384 {
                // Never free textures already referenced by this frame, or by
                // the previous frames still being consumed by the renderer.
                cache
                    .entries
                    .retain(|_, entry| frame.saturating_sub(entry.last_used) <= 2);
            }
        }
        let texture = cache.entries.entry(key).or_insert_with(|| CachedTexture {
            handle: painter.ctx().load_texture(
                format!("piece-{key:?}"),
                hover_atlas(self.sources[index], size, side, style),
                egui::TextureOptions::NEAREST,
            ),
            last_used: frame,
        });
        texture.last_used = frame;
        let uv = if style.outline == Outline::None {
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0))
        } else {
            let left = if highlighted { 0.5 } else { 0.0 };
            Rect::from_min_max(Pos2::new(left, 0.0), Pos2::new(left + 0.5, 1.0))
        };
        painter.image(
            texture.handle.id(),
            rect.expand(padding(size) as f32 / scale),
            uv,
            Color32::WHITE.gamma_multiply(alpha),
        );
    }
}

fn hover_atlas(svg: &[u8], size: usize, side: Color, style: PieceStyle) -> egui::ColorImage {
    let normal = raster(svg, size, side, style);
    if style.outline == Outline::None {
        return normal;
    }
    let hovered = raster(
        svg,
        size,
        side,
        PieceStyle {
            hovered: true,
            ..style
        },
    );
    let [width, height] = normal.size;
    let mut pixels = Vec::with_capacity(width * height * 2);
    for (left, right) in normal
        .pixels
        .chunks_exact(width)
        .zip(hovered.pixels.chunks_exact(width))
    {
        pixels.extend_from_slice(left);
        pixels.extend_from_slice(right);
    }
    egui::ColorImage::new([width * 2, height], pixels)
}

fn pixel_aligned(rect: Rect, scale: f32, size: usize) -> Rect {
    Rect::from_min_size(
        Pos2::new(
            (rect.min.x * scale).round() / scale,
            (rect.min.y * scale).round() / scale,
        ),
        Vec2::splat(size as f32 / scale),
    )
}

fn padding(size: usize) -> usize {
    (size as f32 * 0.15).ceil() as usize
}

fn raster(svg: &[u8], size: usize, side: Color, style: PieceStyle) -> egui::ColorImage {
    let pad = padding(size);
    let n = size + 2 * pad;
    let tree = resvg::usvg::Tree::from_data(svg, &Default::default()).expect("embedded SVG");
    let mut pixmap = resvg::tiny_skia::Pixmap::new(n as u32, n as u32).expect("piece pixels");
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(
            size as f32 / tree.size().width(),
            size as f32 / tree.size().height(),
        )
        .post_translate(pad as f32, pad as f32),
        &mut pixmap.as_mut(),
    );
    let source = pixmap.pixels();
    let alpha: Vec<_> = source.iter().map(|p| p.alpha()).collect();
    let palette = style.inverted.then(|| match side {
        Color::White => ([35_u32, 88, 166], [198_u32, 226, 250]),
        Color::Black => ([100_u32, 18, 42], [234_u32, 183, 195]),
    });
    let outer = (style.outline != Outline::None).then(|| {
        let radius =
            ((size as f32 * if style.hovered { 0.047 } else { 0.032 }).round() as usize).max(2);
        let mask = dilate_mask(&alpha, n, radius);
        if style.outline == Outline::Capture {
            // Blur only the exterior mask, never the chess artwork. Repeated
            // box filters approximate a Gaussian with a smooth opacity falloff.
            let radius =
                ((size as f32 * if style.hovered { 0.045 } else { 0.032 }).round() as usize).max(1);
            let mut glow = mask;
            for _ in 0..3 {
                glow = blur_mask(&glow, n, radius);
            }
            glow
        } else {
            mask
        }
    });
    let color = style.outline.color();
    let mut pixels = vec![Color32::TRANSPARENT; n * n];
    for y in 0..n {
        for x in 0..n {
            let i = y * n + x;
            let p = source[i];
            let a = p.alpha() as u32;
            let mut rgb = [p.red() as u32, p.green() as u32, p.blue() as u32];
            if let Some((dark, light)) = palette {
                // Recolor the original grayscale artwork without changing its
                // strokes or coverage. Keep edge pixels premultiplied by alpha.
                let tone = rgb[0];
                for channel in 0..3 {
                    rgb[channel] = (dark[channel] * (a - tone) + light[channel] * tone) / 255;
                }
            }
            let halo = outer
                .as_ref()
                .map_or(0, |mask| (mask[i] as u32 * (255 - a)) / 255);
            pixels[y * n + x] = Color32::from_rgba_premultiplied(
                (rgb[0] + color.r() as u32 * halo / 255) as u8,
                (rgb[1] + color.g() as u32 * halo / 255) as u8,
                (rgb[2] + color.b() as u32 * halo / 255) as u8,
                (a + halo) as u8,
            );
        }
    }
    egui::ColorImage::new([n, n], pixels)
}

fn blur_mask(alpha: &[u8], n: usize, radius: usize) -> Vec<u8> {
    let mut temp = vec![0; alpha.len()];
    let mut out = vec![0; alpha.len()];
    let width = (2 * radius + 1) as u32;
    for y in 0..n {
        for x in 0..n {
            temp[y * n + x] = ((x.saturating_sub(radius)..=(x + radius).min(n - 1))
                .map(|xx| alpha[y * n + xx] as u32)
                .sum::<u32>()
                / width) as u8;
        }
    }
    for y in 0..n {
        for x in 0..n {
            out[y * n + x] = ((y.saturating_sub(radius)..=(y + radius).min(n - 1))
                .map(|yy| temp[yy * n + x] as u32)
                .sum::<u32>()
                / width) as u8;
        }
    }
    out
}

// Separable alpha dilation for activity, check and capture outlines.
fn dilate_mask(alpha: &[u8], n: usize, radius: usize) -> Vec<u8> {
    let mut temp = vec![0; alpha.len()];
    let mut out = vec![0; alpha.len()];
    for y in 0..n {
        for x in 0..n {
            let range = x.saturating_sub(radius)..=(x + radius).min(n - 1);
            temp[y * n + x] = range.map(|xx| alpha[y * n + xx]).max().unwrap();
        }
    }
    for y in 0..n {
        for x in 0..n {
            let range = y.saturating_sub(radius)..=(y + radius).min(n - 1);
            out[y * n + x] = range.map(|yy| temp[yy * n + x]).max().unwrap();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_is_placed_one_to_one_on_physical_pixels_at_fractional_dpi() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let rect = Rect::from_min_size(Pos2::new(10.3, 20.7), Vec2::splat(62.72));
            let size = (rect.width() * scale).round() as usize;
            let aligned = pixel_aligned(rect, scale, size);
            for value in [
                aligned.min.x * scale,
                aligned.min.y * scale,
                aligned.width() * scale,
            ] {
                assert!((value - value.round()).abs() < 0.0001);
            }
            assert!((aligned.width() * scale - size as f32).abs() < 0.0001);
        }
    }

    #[test]
    fn inverted_palette_preserves_coverage_and_check_outline_is_red_and_antialiased() {
        let svg = include_bytes!("../assets/pieces/cburnett/wK.svg");
        let plain = raster(svg, 63, Color::White, PieceStyle::default());
        let inverse = raster(
            svg,
            63,
            Color::White,
            PieceStyle {
                inverted: true,
                ..Default::default()
            },
        );
        let check = raster(
            svg,
            63,
            Color::White,
            PieceStyle {
                outline: Outline::Check,
                ..Default::default()
            },
        );
        assert!(plain.pixels.iter().any(|p| p.a() > 0 && p.a() < 255));
        assert!(
            plain
                .pixels
                .iter()
                .zip(&inverse.pixels)
                .all(|(a, b)| a.a() == b.a())
        );
        assert!(
            inverse
                .pixels
                .iter()
                .all(|p| p.r() <= p.a() && p.g() <= p.a() && p.b() <= p.a())
        );
        assert!(
            plain
                .pixels
                .iter()
                .zip(&inverse.pixels)
                .any(|(a, b)| a.r() == 255 && *b == Color32::from_rgb(198, 226, 250))
        );
        assert!(
            plain
                .pixels
                .iter()
                .zip(&check.pixels)
                .any(|(a, b)| a.a() == 0 && b.a() > 0 && b.r() > b.g() * 2)
        );
    }
    #[test]
    fn capture_glow_has_an_opacity_gradient_without_blurring_the_piece() {
        let svg = include_bytes!("../assets/pieces/cburnett/bN.svg");
        let plain = raster(svg, 64, Color::Black, PieceStyle::default());
        let glow = raster(
            svg,
            64,
            Color::Black,
            PieceStyle {
                outline: Outline::Capture,
                ..Default::default()
            },
        );
        let hovered = raster(
            svg,
            64,
            Color::Black,
            PieceStyle {
                outline: Outline::Capture,
                hovered: true,
                ..Default::default()
            },
        );
        let mut levels = std::collections::BTreeSet::new();
        for (p, g) in plain.pixels.iter().zip(&glow.pixels) {
            if p.a() == 255 {
                assert_eq!(p, g);
            }
            if p.a() == 0 && g.a() > 0 {
                levels.insert(g.a());
                assert!(g.r() >= g.g() * 2);
            }
        }
        assert!(
            levels.len() > 20,
            "glow must have a smooth opacity gradient"
        );
        assert!(
            hovered.pixels.iter().map(|p| p.a() as u64).sum::<u64>()
                > glow.pixels.iter().map(|p| p.a() as u64).sum::<u64>()
        );
        let n = glow.size[0];
        assert!(
            glow.pixels
                .iter()
                .enumerate()
                .filter(|(i, _)| i % n == 0 || i % n == n - 1 || *i < n || *i >= n * (n - 1))
                .all(|(_, p)| p.a() == 0),
            "canvas must not clip the glow"
        );
        for outline in [Outline::Ordinary, Outline::Information, Outline::Cancelled] {
            let styled = raster(
                svg,
                64,
                Color::Black,
                PieceStyle {
                    outline,
                    ..Default::default()
                },
            );
            assert!(
                plain
                    .pixels
                    .iter()
                    .zip(&styled.pixels)
                    .any(|(p, g)| p.a() == 0 && *g == outline.color())
            );
            assert!(
                plain
                    .pixels
                    .iter()
                    .zip(&styled.pixels)
                    .filter(|(p, _)| p.a() == 255)
                    .all(|(p, g)| p == g)
            );
        }
    }
}
