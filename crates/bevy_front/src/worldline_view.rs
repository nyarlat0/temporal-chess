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

/// Vertical leads keep bends away from the artwork. Circular fillets have a
/// radius of at most half a cell, independent of the distance between boards.
fn rounded_route(a: Pos2, b: Pos2, cell: f32) -> Vec<Pos2> {
    let lead = cell.min((b.y - a.y).abs() * 0.25);
    let vertical = Vec2::new(0.0, (b.y - a.y).signum() * lead);
    let corners = [a, a + vertical, b - vertical, b];
    let mut route = vec![a];
    for i in 1..3 {
        let incoming = corners[i] - corners[i - 1];
        let outgoing = corners[i + 1] - corners[i];
        let u = incoming.normalized();
        let v = outgoing.normalized();
        let angle = (u.x * v.y - u.y * v.x).atan2(u.dot(v));
        if angle.abs() < 0.001 {
            route.push(corners[i]);
            continue;
        }
        let tangent = (angle.abs() * 0.5).tan();
        let trim = (cell * 0.5 * tangent)
            .min(incoming.length() * 0.45)
            .min(outgoing.length() * 0.45);
        let radius = trim / tangent;
        let entry = corners[i] - u * trim;
        let center = entry + Vec2::new(-u.y, u.x) * (radius * angle.signum());
        let radial = entry - center;
        for step in 0..=16 {
            let t = angle * step as f32 / 16.0;
            route.push(
                center
                    + Vec2::new(
                        radial.x * t.cos() - radial.y * t.sin(),
                        radial.x * t.sin() + radial.y * t.cos(),
                    ),
            );
        }
    }
    route.push(b);
    route
}

/// A temporal reversal is a U-turn beyond the source occurrence, not a cusp
/// at its center. The outgoing tangent must agree with the incoming worldline.
fn state_route(a: Pos2, b: Pos2, cell: f32, inverted: bool, next_inverted: bool) -> Vec<Pos2> {
    if inverted == next_inverted {
        return rounded_route(a, b, cell);
    }
    let old_direction = if inverted { 1.0 } else { -1.0 };
    let side = if b.x < a.x { -1.0 } else { 1.0 };
    let entry = a + Vec2::new(0.0, old_direction * cell);
    let radius = cell * 0.5;
    let steps = ((std::f32::consts::PI * radius / 1.5).ceil() as usize).clamp(32, 192);
    let mut route = vec![a, entry];
    for step in 1..=steps {
        let angle = std::f32::consts::PI * step as f32 / steps as f32;
        route.push(
            entry
                + Vec2::new(
                    side * radius * (1.0 - angle.cos()),
                    old_direction * radius * angle.sin(),
                ),
        );
    }
    let exit = entry + Vec2::new(side * radius * 2.0, 0.0);
    // Use the exact semicircle endpoint, avoiding floating point seams.
    *route.last_mut().unwrap() = exit;
    route.extend(rounded_route(exit, b, cell).into_iter().skip(1));
    route
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
    let width = (cell * 0.045).clamp(1.4, 3.0);
    for i in 0..nodes.len().saturating_sub(1) {
        let (a, b) = (nodes[i].1.center(), nodes[i + 1].1.center());
        let route = state_route(a, b, cell, nodes[i].0.inverted, nodes[i + 1].0.inverted);
        if !Rect::from_points(&route)
            .expand(width * 5.0)
            .intersects(painter.clip_rect())
        {
            continue;
        }
        let total: f32 = route.windows(2).map(|p| p[0].distance(p[1])).sum();
        if total < 0.001 {
            continue;
        }
        let mut points = vec![(a, color(nodes[i].0))];
        let mut distance = 0.0;
        let mut middle = a;
        let mut arrow_direction = (b - a).normalized();
        for pair in route.windows(2) {
            let length = pair[0].distance(pair[1]);
            if length < 0.0001 {
                continue;
            }
            if distance <= total * 0.5 && distance + length >= total * 0.5 {
                middle = pair[0].lerp(pair[1], (total * 0.5 - distance) / length);
                arrow_direction = (pair[1] - pair[0]).normalized();
            }
            let steps = (length / 5.0).ceil().max(1.0) as usize;
            for j in 1..=steps {
                let fraction = j as f32 / steps as f32;
                points.push((
                    pair[0].lerp(pair[1], fraction),
                    blend(
                        color(nodes[i].0),
                        color(nodes[i + 1].0),
                        (distance + length * fraction) / total,
                    ),
                ));
            }
            distance += length;
        }
        let steps = points.len() - 1;
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
        let dir = arrow_direction;
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
    fn reversal_continues_old_direction_then_turns_without_a_cusp() {
        for cell in [9.6, 64.0, 90.0] {
            for inverted in [false, true] {
                for horizontal in [-3.0, 0.0, 3.0] {
                    let old_direction = if inverted { 1.0 } else { -1.0 };
                    let a = Pos2::ZERO;
                    let b = Pos2::new(horizontal * cell, -old_direction * 8.0 * cell);
                    let route = state_route(a, b, cell, inverted, !inverted);
                    assert_eq!(route[0], a);
                    assert_eq!(*route.last().unwrap(), b);
                    assert_eq!(route[1], a + Vec2::new(0.0, old_direction * cell));
                    assert!(route.iter().any(|p| p.y * old_direction > cell * 1.49));
                    assert!(route.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
                    let directions: Vec<_> = route
                        .windows(2)
                        .map(|pair| pair[1] - pair[0])
                        .filter(|delta| delta.length_sq() > 0.0001)
                        .map(|delta| delta.normalized())
                        .collect();
                    for pair in directions.windows(2) {
                        assert!(pair[0].dot(pair[1]) > 0.98, "kink: {pair:?}");
                    }
                    assert!(directions.last().unwrap().y * old_direction < -0.99);
                }
            }
        }
    }

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
