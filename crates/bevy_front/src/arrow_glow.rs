//! Cached distance-field halo for the shaft, head and cancellation slash.
use bevy_egui::egui::{self, Color32, Pos2, Rect, Vec2};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct Cache {
    textures: HashMap<(u32, u32), (egui::TextureHandle, u64)>,
}

pub fn paint(painter: &egui::Painter, start: Pos2, end: Pos2, cell: f32, color: Color32) {
    let length = start.distance(end);
    if length < 1.0 {
        return;
    }
    let scale = painter.ctx().pixels_per_point();
    let key = (
        (cell * scale).round().clamp(8.0, 256.0) as u32,
        (length * scale).round().max(1.0) as u32,
    );
    let cache = painter.ctx().data_mut(|data| {
        data.get_temp_mut_or_default::<Arc<Mutex<Cache>>>(egui::Id::new("arrow_glow_textures"))
            .clone()
    });
    let mut cache = cache.lock().expect("arrow glow textures");
    let frame = painter.ctx().cumulative_frame_nr();
    if cache.textures.len() > 48 {
        cache
            .textures
            .retain(|_, (_, used)| frame.saturating_sub(*used) <= 2);
    }
    let entry = cache.textures.entry(key).or_insert_with(|| {
        (
            painter.ctx().load_texture(
                format!("arrow-glow-{key:?}"),
                raster(key.0 as f32, key.1 as f32),
                egui::TextureOptions::LINEAR,
            ),
            frame,
        )
    });
    entry.1 = frame;
    let mut mesh = egui::Mesh::with_texture(entry.0.id());
    let padding = cell * 0.32;
    let rect = Rect::from_min_max(
        Pos2::new(-padding, -cell * 0.2 - padding),
        Pos2::new(length + padding, cell * 0.2 + padding),
    );
    mesh.add_rect_with_uv(
        rect,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        color,
    );
    let direction = (end - start) / length;
    let normal = Vec2::new(-direction.y, direction.x);
    for vertex in &mut mesh.vertices {
        vertex.pos = start + direction * vertex.pos.x + normal * vertex.pos.y;
    }
    painter.add(egui::Shape::mesh(mesh));
}

fn segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    (p - (a + ab * ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0))).length()
}
fn raster(cell: f32, length: f32) -> egui::ColorImage {
    let pad = cell * 0.32;
    let width = (length + 2.0 * pad).ceil() as usize;
    let height = (cell * 0.4 + 2.0 * pad).ceil() as usize;
    let a = Vec2::new(length - cell * 0.06, 0.0);
    let b = Vec2::new(length - cell * 0.36, cell * 0.20);
    let c = Vec2::new(b.x, -b.y);
    let slash = Vec2::splat(cell * 0.25 * std::f32::consts::FRAC_1_SQRT_2);
    let center = Vec2::new(length * 0.5, 0.0);
    let sigma = cell * 0.095;
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let p = Vec2::new(x as f32 + 0.5 - pad, y as f32 + 0.5 - cell * 0.2 - pad);
            let inside = [(a, b), (b, c), (c, a)]
                .iter()
                .all(|(a, b)| (*b - *a).rot90().dot(p - *a) >= 0.0);
            let head = if inside {
                0.0
            } else {
                segment(p, a, b).min(segment(p, b, c)).min(segment(p, c, a))
            };
            let shaft = segment(p, Vec2::new(cell * 0.1, 0.0), Vec2::new(b.x, 0.0)) - cell * 0.055;
            let strike = segment(p, center - slash, center + slash) - cell * 0.029;
            let distance = head.min(shaft).min(strike).max(0.0);
            let alpha = (245.0 * (-0.5 * (distance / sigma).powi(2)).exp()).round() as u8;
            pixels.push(Color32::from_rgba_unmultiplied(255, 255, 255, alpha));
        }
    }
    egui::ColorImage::new([width, height], pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn halo_is_bright_near_the_edge_and_fades_smoothly_outward() {
        let cell = 64.0;
        let image = raster(cell, cell * 4.0);
        let x = (cell * 0.32 + cell * 0.8) as usize;
        let sample =
            |offset: f32| image.pixels[(cell * 0.52 + offset) as usize * image.size[0] + x].a();
        assert!(sample(7.0) > 180);
        assert!(sample(7.0) > sample(12.0) && sample(12.0) > sample(18.0));
        assert!(sample(25.0) < 5);
        assert!(
            image
                .pixels
                .iter()
                .map(|p| p.a())
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                > 100
        );
    }
}
