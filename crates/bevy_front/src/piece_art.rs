use crate::theme::{CAPTURE, INFORMATION, ORDINARY, REACTION};
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
#[derive(bevy::prelude::Resource)]
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
        // The approved Python preview uses the white cburnett SVG for both
        // inverted sides and applies a side-specific amethyst palette.
        let source_index = if style.inverted { kind } else { index };
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
                hover_atlas(self.sources[source_index], size, side, style),
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
    // RGB values and compositing order from render_amethyst.py (styled_piece).
    // Both inverted sides use the white source SVG: white areas are the body,
    // dark engravings are the ink, independently of the original piece color.
    let palette = style.inverted.then(|| match side {
        Color::White => (
            [251_u32, 247, 255], // clean near-white body
            [103_u32, 48, 158],  // purple ink
            [160_u32, 73, 255],  // soft amethyst light
            [133_u32, 59, 225],  // thin highlight
        ),
        Color::Black => (
            [33_u32, 17, 54],    // deep plum body
            [231_u32, 187, 255], // lavender ink
            [163_u32, 64, 255],  // soft amethyst light
            [169_u32, 83, 251],  // thin highlight
        ),
    });
    // Python: dilation by MaxFilter 9/13 at 2x, GaussianBlur sigma=4.5%
    // of the piece size, and a narrow rim from MaxFilter 7. Work at native
    // pixel resolution, approximating the Gaussian with three box filters.
    let inverted_glow = style.inverted.then(|| {
        let spread = if size >= 120 { 3 } else { 2 };
        let rim_radius = if size >= 64 { 2 } else { 1 };
        let blur = ((size as f32 * 0.045).round() as usize).max(1);
        let mut soft = dilate_mask(&alpha, n, spread);
        for _ in 0..3 {
            soft = blur_mask(&soft, n, blur);
        }
        (
            soft,
            dilate_mask(&alpha, n, rim_radius),
            exterior_transparency(&alpha, n),
        )
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
            if let Some((body, ink, _, _)) = palette {
                // tiny-skia gives premultiplied RGB: tone <= a. Retain
                // antialiased coverage while recoloring the white fill and
                // the black engravings separately, like the Python reference.
                let tone = (rgb[0] + rgb[1] + rgb[2]) / 3;
                for channel in 0..3 {
                    rgb[channel] = (body[channel] * tone + ink[channel] * (a - tone)) / 255;
                }
            }
            // Glow first, bright exterior rim second, source silhouette last.
            let mut light_rgb = [0_u32; 3];
            let mut light_alpha = 0_u32;
            if let (Some((_, _, glow, stroke)), Some((soft, rim, exterior))) =
                (palette, &inverted_glow)
            {
                if exterior[i] {
                    // Pillow uses 0.55 opacity on the soft glow, 0.83 on
                    // the thin outside rim. Keep both behind the artwork.
                    light_alpha = (soft[i] as u32 * 140) / 255;
                    for channel in 0..3 {
                        light_rgb[channel] = glow[channel] * light_alpha / 255;
                    }
                    let rim_alpha = (rim[i].saturating_sub(alpha[i]) as u32 * 212) / 255;
                    for channel in 0..3 {
                        light_rgb[channel] = (stroke[channel] * rim_alpha
                            + light_rgb[channel] * (255 - rim_alpha))
                            / 255;
                    }
                    light_alpha += rim_alpha * (255 - light_alpha) / 255;
                }
            }
            // Move highlights (including red checks and captures) override the halo.
            let outline_alpha = outer.as_ref().map_or(0, |mask| mask[i] as u32);
            for (channel, tint) in [color.r(), color.g(), color.b()].into_iter().enumerate() {
                light_rgb[channel] = (tint as u32 * outline_alpha
                    + light_rgb[channel] * (255 - outline_alpha))
                    / 255;
            }
            light_alpha += outline_alpha * (255 - light_alpha) / 255;
            for channel in 0..3 {
                rgb[channel] += light_rgb[channel] * (255 - a) / 255;
            }
            pixels[y * n + x] = Color32::from_rgba_premultiplied(
                rgb[0] as u8,
                rgb[1] as u8,
                rgb[2] as u8,
                (a + light_alpha * (255 - a) / 255) as u8,
            );
        }
    }
    egui::ColorImage::new([n, n], pixels)
}

// Prevent neon lighting inside closed gaps in the source SVG.
fn exterior_transparency(alpha: &[u8], n: usize) -> Vec<bool> {
    let mut exterior = vec![false; n * n];
    let mut pending = std::collections::VecDeque::new();
    for x in 0..n {
        for i in [x, (n - 1) * n + x, x * n, x * n + n - 1] {
            if alpha[i] == 0 && !exterior[i] {
                exterior[i] = true;
                pending.push_back(i);
            }
        }
    }
    while let Some(i) = pending.pop_front() {
        let x = i % n;
        let y = i / n;
        for neighbor in [
            (x > 0).then(|| i - 1),
            (x + 1 < n).then_some(i + 1),
            (y > 0).then(|| i - n),
            (y + 1 < n).then_some(i + n),
        ]
        .into_iter()
        .flatten()
        {
            if alpha[neighbor] == 0 && !exterior[neighbor] {
                exterior[neighbor] = true;
                pending.push_back(neighbor);
            }
        }
    }

    // The flood fill above intentionally walks *only* transparent pixels.
    // However, the SVG edge is antialiased: the adjacent partially covered
    // texels also belong to the outside of the piece. Without including them,
    // a thin board-colored seam appears between the purple ink and its rim.
    // Extend the mask by one antialiased texel without allowing the flood
    // fill to pass through the SVG stroke into enclosed holes.
    let mut with_antialiasing = exterior.clone();
    for i in 0..alpha.len() {
        if alpha[i] == 0 || alpha[i] == 255 {
            continue;
        }
        let x = i % n;
        let y = i / n;
        if (x > 0 && exterior[i - 1])
            || (x + 1 < n && exterior[i + 1])
            || (y > 0 && exterior[i - n])
            || (y + 1 < n && exterior[i + n])
        {
            with_antialiasing[i] = true;
        }
    }
    with_antialiasing
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
    fn inverted_palette_matches_approved_python_preview() {
        // The reference recolors the same white SVG for both sides.
        let svg = include_bytes!("../assets/pieces/cburnett/wP.svg");
        let normal = raster(svg, 92, Color::White, PieceStyle::default());
        for (side, body, ink) in [
            (
                Color::White,
                Color32::from_rgb(251, 247, 255),
                Color32::from_rgb(103, 48, 158),
            ),
            (
                Color::Black,
                Color32::from_rgb(33, 17, 54),
                Color32::from_rgb(231, 187, 255),
            ),
        ] {
            let inverted = raster(
                svg,
                92,
                side,
                PieceStyle {
                    inverted: true,
                    ..Default::default()
                },
            );
            assert!(inverted.pixels.contains(&body));
            assert!(inverted.pixels.contains(&ink));
            // The source silhouette stays opaque, without any glow inside.
            assert!(
                normal
                    .pixels
                    .iter()
                    .zip(&inverted.pixels)
                    .all(|(src, result)| src.a() != 255 || result.a() == 255)
            );
            // The halo is visible *outside* the original silhouette.
            assert!(
                normal
                    .pixels
                    .iter()
                    .zip(&inverted.pixels)
                    .any(|(src, result)| src.a() == 0 && result.a() > 0)
            );
            assert!(
                inverted
                    .pixels
                    .iter()
                    .all(|p| p.r() <= p.a() && p.g() <= p.a() && p.b() <= p.a())
            );
        }
    }

    #[test]
    fn inverted_glow_does_not_enter_enclosed_svg_holes() {
        let n = 25;
        let mut alpha = vec![0; n * n];
        for y in 5..20 {
            for x in 5..20 {
                alpha[y * n + x] = 255;
            }
        }
        for y in 10..15 {
            for x in 10..15 {
                alpha[y * n + x] = 0;
            }
        }
        let outside = exterior_transparency(&alpha, n);
        assert!(outside[0]);
        assert!(!outside[12 * n + 12]);
        assert!(!outside[7 * n + 7]);
    }

    #[test]
    fn inverted_glow_covers_antialiased_outer_edge_but_not_internal_holes() {
        let n = 25;
        let mut alpha = vec![0; n * n];
        // Opaque ring with a one-pixel antialiased outer boundary.
        for y in 5..20 {
            for x in 5..20 {
                alpha[y * n + x] = 255;
            }
        }
        for y in 10..15 {
            for x in 10..15 {
                alpha[y * n + x] = 0;
            }
        }
        for p in 5..20 {
            alpha[5 * n + p] = 128;
            alpha[19 * n + p] = 128;
            alpha[p * n + 5] = 128;
            alpha[p * n + 19] = 128;
        }
        let outside = exterior_transparency(&alpha, n);
        assert!(outside[5 * n + 12], "exterior antialias pixels need glow");
        assert!(outside[12 * n + 5], "side antialias pixels need glow");
        assert!(!outside[12 * n + 12], "enclosed holes must remain dark");
        assert!(
            !outside[9 * n + 12],
            "opaque SVG edges must not be affected"
        );
    }

    #[test]
    fn inverted_piece_has_no_unlit_gap_at_partial_coverage() {
        let svg = include_bytes!("../assets/pieces/cburnett/wN.svg");
        let regular = raster(svg, 92, Color::White, PieceStyle::default());
        let inverted = raster(
            svg,
            92,
            Color::White,
            PieceStyle {
                inverted: true,
                ..Default::default()
            },
        );
        // Some antialiased edge texels must gain halo coverage, rather than
        // letting the board background show through between two purple bands.
        assert!(
            regular
                .pixels
                .iter()
                .zip(&inverted.pixels)
                .any(|(src, lit)| { (1..255).contains(&src.a()) && lit.a() > src.a() })
        );
    }

    #[test]
    fn check_outline_stays_red() {
        let svg = include_bytes!("../assets/pieces/cburnett/wK.svg");
        let plain = raster(svg, 63, Color::White, PieceStyle::default());
        let check = raster(
            svg,
            63,
            Color::White,
            PieceStyle {
                outline: Outline::Check,
                ..Default::default()
            },
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
