//! Causal-order worldline overlay. Geometry includes offscreen states so clipping
//! and scrolling never change the tangents of the visible curve.
use crate::{
    board_view::square_rect,
    controller::GameUi,
    piece_art::{PieceArt, PieceStyle},
};
use bevy_egui::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use temporal_chess_engine::model::{Color, Sq, State};

fn color(state: &State) -> Color32 {
    match (state.alive, state.inverted, state.color) {
        (false, false, _) => Color32::from_rgb(255, 55, 70),
        (false, true, _) => Color32::from_rgb(175, 25, 65),
        (true, false, _) => Color32::from_rgb(55, 245, 135),
        (true, true, Color::White) => Color32::from_rgb(160, 73, 255),
        (true, true, Color::Black) => Color32::from_rgb(163, 64, 255),
    }
}

fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let c = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(c(a.r(), b.r()), c(a.g(), b.g()), c(a.b(), b.b()))
}

fn spline(a: Pos2, b: Pos2, ta: Vec2, tb: Vec2, t: f32) -> Pos2 {
    let t2 = t * t;
    let t3 = t2 * t;
    (a.to_vec2() * (2.0 * t3 - 3.0 * t2 + 1.0)
        + ta * (t3 - 2.0 * t2 + t)
        + b.to_vec2() * (-2.0 * t3 + 3.0 * t2)
        + tb * (t3 - t2))
        .to_pos2()
}

pub fn paint(
    painter: &egui::Painter,
    game: &GameUi,
    art: &PieceArt,
    times: &[usize],
    board_at: &impl Fn(usize) -> Rect,
    cell: f32,
) {
    let Some(line) = game
        .worldline_selected
        .and_then(|wid| game.engine.history().worldlines().get(wid))
    else {
        return;
    };
    let nodes: Vec<_> = line
        .iter()
        .filter_map(|state| {
            let row = times.iter().position(|&t| t == state.t)?;
            let rect = square_rect(
                board_at(row),
                cell,
                Sq(state.x, state.y),
                game.facing.black_bottom,
            );
            Some((state, rect))
        })
        .collect();
    let tangent = |i: usize| {
        let prev = nodes[i.saturating_sub(1)].1.center();
        let next = nodes[(i + 1).min(nodes.len() - 1)].1.center();
        if i > 0 && i + 1 < nodes.len() && (next - prev).length_sq() < 1.0 {
            // A return through the same square needs a sideways tangent at the
            // temporal turn, instead of a cusp or two coincident straight legs.
            Vec2::new(cell * 1.6, 0.0)
        } else {
            (next - prev) * 0.35
        }
    };
    let width = (cell * 0.045).clamp(1.4, 3.0);
    for i in 0..nodes.len().saturating_sub(1) {
        let (a, b) = (nodes[i].1.center(), nodes[i + 1].1.center());
        let (ta, tb) = (tangent(i), tangent(i + 1));
        let bounds = Rect::from_points(&[a, a + ta / 3.0, b - tb / 3.0, b]);
        if !bounds.expand(width * 5.0).intersects(painter.clip_rect()) {
            continue;
        }
        let steps = ((a.distance(b) / 5.0).ceil() as usize).clamp(24, 160);
        let points: Vec<_> = (0..=steps)
            .map(|j| {
                let t = j as f32 / steps as f32;
                (
                    spline(a, b, ta, tb, t),
                    blend(color(nodes[i].0), color(nodes[i + 1].0), t),
                )
            })
            .collect();
        // Joined ribbons, rather than overlapping transparent line segments,
        // keep the glow smooth and prevent bright dots at subdivision joints.
        for (radius, opacity) in [
            (width * 4.5, 0.045),
            (width * 2.8, 0.09),
            (width * 1.6, 0.18),
            (width * 0.5, 0.95),
        ] {
            let mut mesh = egui::Mesh::default();
            for j in 0..points.len() {
                let delta = points[(j + 1).min(steps)].0 - points[j.saturating_sub(1)].0;
                let dir = delta.normalized();
                let normal = Vec2::new(-dir.y, dir.x) * radius;
                mesh.colored_vertex(points[j].0 - normal, points[j].1.gamma_multiply(opacity));
                mesh.colored_vertex(points[j].0 + normal, points[j].1.gamma_multiply(opacity));
                if j > 0 {
                    let k = (j * 2) as u32;
                    mesh.add_triangle(k - 2, k - 1, k);
                    mesh.add_triangle(k - 1, k + 1, k);
                }
            }
            painter.add(egui::Shape::mesh(mesh));
        }
        let middle = spline(a, b, ta, tb, 0.5);
        let dir = (spline(a, b, ta, tb, 0.52) - spline(a, b, ta, tb, 0.48)).normalized();
        let normal = Vec2::new(-dir.y, dir.x);
        let size = (cell * 0.22).clamp(5.5, 14.0);
        painter.add(egui::Shape::convex_polygon(
            vec![
                middle + dir * size,
                middle - dir * size + normal * size * 0.65,
                middle - dir * size - normal * size * 0.65,
            ],
            blend(color(nodes[i].0), color(nodes[i + 1].0), 0.5),
            Stroke::NONE,
        ));
    }
    // Selected occurrences, including ghosts, are always above the entire curve.
    for (state, rect) in nodes {
        if !rect.intersects(painter.clip_rect()) {
            continue;
        }
        art.paint_tinted(
            painter,
            state.kind,
            if state.alive {
                state.color
            } else {
                Color::White
            },
            rect,
            if state.alive { 1.0 } else { 0.55 },
            PieceStyle {
                inverted: state.alive && state.inverted,
                ..Default::default()
            },
            if state.alive {
                Color32::WHITE
            } else {
                color(state)
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worldline_selection_can_inspect_opponents_without_playing() {
        let mut game = GameUi::default();
        let before = game.engine.clone();
        game.worldline_mode = true;
        game.click_square(0, Sq(0, 6));
        let black = game.worldline_selected.unwrap();
        assert_eq!(
            game.engine.history().worldlines()[black][0].color,
            Color::Black
        );
        game.click_square(0, Sq(0, 1));
        let white = game.worldline_selected.unwrap();
        assert_ne!(white, black);
        assert_eq!(
            game.engine.history().worldlines()[white][0].color,
            Color::White
        );
        assert!(game.selected.is_none());
        assert!(game.record.commands().is_empty());
        assert_eq!(game.engine, before);
    }
}
