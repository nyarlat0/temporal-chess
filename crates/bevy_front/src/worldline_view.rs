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

/// Subdivide the actual Bezier curve until its control polygon is flat to a
/// fraction of a screen point. Tight bends receive more samples than long leads.
fn append_bezier(control: &[Pos2], output: &mut Vec<Pos2>, depth: usize) {
    let first = control[0];
    let last = *control.last().unwrap();
    let chord = last - first;
    let length = chord.length();
    let flat = length > 0.0001
        && control.iter().all(|point| {
            let delta = *point - first;
            let along = delta.dot(chord) / length;
            (delta.x * chord.y - delta.y * chord.x).abs() / length <= 0.15
                && along >= 0.0
                && along <= length
        });
    let smooth = length > 0.0001
        && control.windows(2).all(|pair| {
            let tangent = pair[1] - pair[0];
            tangent.length_sq() < 1e-10 || tangent.dot(chord) / (tangent.length() * length) > 0.998
        });
    if (flat && smooth) || depth == 14 {
        output.push(last);
        return;
    }
    let mut work = control.to_vec();
    let mut left = vec![first];
    let mut right = vec![last];
    for remaining in (1..control.len()).rev() {
        for i in 0..remaining {
            work[i] = work[i].lerp(work[i + 1], 0.5);
        }
        left.push(work[0]);
        right.push(work[remaining - 1]);
    }
    right.reverse();
    append_bezier(&left, output, depth + 1);
    append_bezier(&right, output, depth + 1);
}

/// Smooth S-shaped connection, with zero curvature at its vertical endpoints.
/// There is no straight diagonal section between separately rounded corners.
fn rounded_route(a: Pos2, b: Pos2) -> Vec<Pos2> {
    let handle = Vec2::new(0.0, (b.y - a.y) * 0.2);
    let mut route = vec![a];
    append_bezier(
        &[
            a,
            a + handle,
            a + handle * 2.0,
            b - handle * 2.0,
            b - handle,
            b,
        ],
        &mut route,
        0,
    );
    route
}

/// One degree-seven Bezier carries a reversal all the way to the next state.
/// Collinear, equally spaced endpoint controls give vertical tangents and zero
/// endpoint curvature; the inner controls shape a compact, flowing hairpin.
fn state_route(a: Pos2, b: Pos2, cell: f32, inverted: bool, next_inverted: bool) -> Vec<Pos2> {
    if inverted == next_inverted {
        return rounded_route(a, b);
    }
    let old_direction = Vec2::new(0.0, if inverted { 1.0 } else { -1.0 });
    let side = Vec2::new(if b.x < a.x { -1.0 } else { 1.0 }, 0.0);
    let radius = cell * 0.5;
    let start_handle = old_direction * radius * 0.8;
    let end_handle = old_direction * (b.y - a.y).abs() * 0.18;
    let mut route = vec![a];
    append_bezier(
        &[
            a,
            a + start_handle,
            a + start_handle * 2.0,
            a + side * radius * 2.0 + start_handle * 2.0,
            a + side * radius * 2.0 - old_direction * radius,
            b + end_handle * 2.0,
            b + end_handle,
            b,
        ],
        &mut route,
        0,
    );
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
                    assert!(route[1].y * old_direction > 0.0);
                    assert!(route[1].distance(a) < cell * 0.5);
                    assert!(route.iter().any(|p| p.y * old_direction > cell * 0.1));
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
