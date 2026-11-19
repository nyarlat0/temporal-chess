//! Causal-order worldline overlay. Geometry includes offscreen states so clipping
//! and scrolling never change the tangents of the visible curve.
use crate::{
    board_view::square_rect,
    controller::GameUi,
    piece_art::{PieceArt, PieceStyle},
};
use bevy_egui::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use temporal_chess_engine::model::BoardTime;
use temporal_chess_engine::model::{Color, PieceAt, PieceId, Sq, State};

/// Focus scales the artwork around the cell center; callers clip to the cell.
pub fn focused_rect(ctx: &egui::Context, at: PieceAt, rect: Rect, selected: bool) -> Rect {
    let amount = focus_amount(ctx, at, selected);
    Rect::from_center_size(
        rect.center(),
        rect.size() * (1.0 + 0.12 * crate::animation::ease(amount)),
    )
}

pub fn focus_amount(ctx: &egui::Context, at: PieceAt, selected: bool) -> f32 {
    let id = egui::Id::new(("worldline_state_focus", at));
    let fresh = ctx.data_mut(|d| {
        let fresh = d.get_temp::<bool>(id).is_none();
        d.insert_temp(id, true);
        fresh
    });
    if fresh {
        ctx.animate_bool_with_time(id, false, 0.22);
    }
    ctx.animate_bool_with_time(id, selected, 0.22)
}

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
    times: &[BoardTime],
    board_at: &impl Fn(usize) -> Rect,
    cell: f32,
) {
    let Some(wid) = game.worldline_selected else {
        return;
    };
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
        let at = PieceAt {
            piece: PieceId::from_state(state, wid),
            t: state.t,
        };
        let focused = focused_rect(
            painter.ctx(),
            at,
            rect,
            game.worldline_transition.is_none() && game.worldline_state == Some(at),
        );
        if !rect.intersects(painter.clip_rect()) {
            continue;
        }
        art.paint_focus(
            &painter.with_clip_rect(rect.intersect(painter.clip_rect())),
            state.kind,
            if state.alive {
                state.color
            } else {
                Color::White
            },
            rect,
            focused,
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
    fn navigation_shrinks_then_scrolls_then_grows() {
        let ctx = egui::Context::default();
        let old = PieceAt {
            piece: PieceId {
                wid: 0,
                fut_seen: 0,
                loop_turn: 0,
            },
            t: 0,
        };
        let target = PieceAt { t: 2, ..old };
        let mut game = GameUi {
            worldline_mode: true,
            worldline_selected: Some(0),
            worldline_state: Some(old),
            ..Default::default()
        };
        for time in [0.0, 0.1, 0.25] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    ..Default::default()
                },
                |ui| {
                    focus_amount(ui.ctx(), old, true);
                },
            );
            output.textures_delta.clear();
        }
        game.jump_to = None;
        game.focus_worldline(target, true);
        for time in [0.3, 0.6, 0.7, 0.8, 0.95] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    ..Default::default()
                },
                |ui| {
                    game.update_worldline_transition(ui.ctx());
                    let amount =
                        focus_amount(ui.ctx(), target, game.worldline_transition.is_none());
                    if time == 0.3 {
                        assert!(game.jump_to.is_none());
                        assert!(!game.worldline_transition.unwrap().scrolling);
                        assert_eq!(amount, 0.0);
                    } else if time == 0.6 {
                        assert_eq!(game.jump_to, Some(2));
                        assert!(game.worldline_transition.unwrap().scrolling);
                        assert_eq!(amount, 0.0);
                        game.jump_to = None;
                        game.viewport.scroll = Some(crate::animation::SmoothScroll::new(2, 0.0, 1));
                    } else if time == 0.7 {
                        assert!(game.worldline_transition.is_some());
                        assert_eq!(amount, 0.0);
                        game.viewport.scroll = None;
                    } else {
                        assert!(game.worldline_transition.is_none());
                        assert!(amount > 0.0);
                    }
                },
            );
            output.textures_delta.clear();
        }
    }

    #[test]
    fn focus_grows_and_shrinks_smoothly_when_switching_states() {
        let ctx = egui::Context::default();
        let at = |t| PieceAt {
            piece: PieceId {
                wid: 0,
                fut_seen: 0,
                loop_turn: 0,
            },
            t,
        };
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(64.0));
        let mut sizes = Vec::new();
        for (time, selected) in [(0.0, 0), (0.1, 0), (0.24, 0), (0.34, 1), (0.6, 1)] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    ..Default::default()
                },
                |ui| {
                    sizes.push((
                        focused_rect(ui.ctx(), at(0), rect, selected == 0).width(),
                        focused_rect(ui.ctx(), at(1), rect, selected == 1).width(),
                    ));
                },
            );
            output.textures_delta.clear();
        }
        assert!(sizes[0].0 < sizes[1].0 && sizes[1].0 <= sizes[2].0);
        assert!(sizes[3].0 < sizes[2].0 && sizes[4].0 <= sizes[3].0);
        assert!(sizes[4].1 > sizes[3].1);
        assert!(
            sizes
                .iter()
                .all(|(a, b)| (64.0..=71.69).contains(a) && (64.0..=71.69).contains(b))
        );
    }

    #[test]
    fn state_navigation_follows_causality_through_reversal_and_death_and_empty_click_resets() {
        use temporal_chess_engine::{
            engine::Engine,
            model::{PieceKind, TimeDirection},
        };
        let mut state = State::new(PieceKind::N, Color::White, Sq(1, 0));
        state.t = -2;
        state.fut_seen = -2;
        let mut line = vec![state];
        for (direction, sq) in [
            (TimeDirection::Future, Sq(2, 2)),
            (TimeDirection::Past, Sq(1, 4)),
            (TimeDirection::Past, Sq(0, 6)),
        ] {
            let mut next = line.last().unwrap().step(direction).unwrap();
            next.x = sq.0;
            next.y = sq.1;
            line.push(next);
        }
        line.last_mut().unwrap().alive = false;
        // A real opposing arrival sustains the death during reconciliation.
        let mut rook = State::new(PieceKind::R { moved: true }, Color::Black, Sq(0, 5));
        rook.t = -4;
        rook.fut_seen = -1;
        let mut arrival = rook.step(TimeDirection::Future).unwrap();
        arrival.y = 6;
        let engine =
            Engine::from_worldlines(vec![line, vec![rook, arrival]], Color::White).unwrap();
        let before = engine.clone();
        let mut game = GameUi {
            engine,
            worldline_mode: true,
            ..Default::default()
        };
        game.click_square(-2, Sq(1, 0));
        let original = game.worldline_state.unwrap();
        let ctx = egui::Context::default();
        game.navigate_active(true, false);
        game.update_worldline_transition(&ctx);
        assert_eq!(game.jump_to, Some(-1));
        game.navigate_active(true, true);
        game.update_worldline_transition(&ctx);
        assert_eq!(game.jump_to, Some(-2));
        assert_ne!(game.worldline_state.unwrap(), original);
        game.navigate_active(true, false);
        game.update_worldline_transition(&ctx);
        assert_eq!(game.jump_to, Some(-3));
        assert!(
            !game
                .engine
                .piece_state(game.worldline_state.unwrap())
                .unwrap()
                .alive
        );
        game.navigate_active(true, false);
        game.update_worldline_transition(&ctx);
        assert_eq!(game.worldline_state, Some(original));
        game.navigate_active(false, false);
        game.update_worldline_transition(&ctx);
        assert_eq!(game.jump_to, Some(-3));
        game.navigate_boundary(false);
        assert!(game.worldline_transition.is_some());
        assert_eq!(game.worldline_state, Some(original));
        game.update_worldline_transition(&ctx);
        assert_eq!(game.jump_to, Some(-2));
        game.navigate_boundary(true);
        game.update_worldline_transition(&ctx);
        assert_eq!(game.jump_to, Some(-3));
        assert!(
            !game
                .engine
                .piece_state(game.worldline_state.unwrap())
                .unwrap()
                .alive
        );
        game.click_square(-2, Sq(7, 7));
        assert!(game.worldline_mode);
        assert!(game.worldline_selected.is_none() && game.worldline_state.is_none());
        assert_eq!(game.engine, before);
    }

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
