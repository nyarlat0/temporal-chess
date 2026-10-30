use crate::{
    animation::{BoardTransition, SmoothScroll, ease, scroll_curve, travel_curve},
    controller::{GameUi, identity},
    icons::{Icon, icon_button},
    piece_art::{Outline, PieceArt, PieceStyle},
    theme::{BACKGROUND, CAPTURE, INVERTED, MUTED, direction_color},
};
use bevy_egui::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use temporal_chess_engine::{
    engine::CheckStatus,
    model::{Color, Sq},
    record::Command,
};

const LIGHT: Color32 = Color32::from_rgb(240, 217, 181);
const DARK: Color32 = Color32::from_rgb(181, 136, 99);

pub fn ribbon(ui: &mut egui::Ui, game: &mut GameUi, art: &PieceArt) {
    // Own the viewport rather than combining two independent scroll animators.
    // Content height contains real boards only: no blank padding rows.
    let (viewport, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    let cell = game
        .cell_size
        .min(((viewport.width() - 160.0) / 9.04).max(18.0));
    let gap = 1.04 * cell + 24.0;
    let height = 8.0 * cell + gap;
    let times = game.times();
    let max_offset = (times.len() as f32 * height - viewport.height()).max(0.0);
    let frame = ui.ctx().cumulative_frame_nr();
    if game.viewport.frame != Some(frame) {
        let mut offset = game.viewport.anchored_offset(&times, height);
        let raw = game
            .transition
            .as_ref()
            .map_or(0.0, BoardTransition::progress);
        if raw >= 1.0 {
            if let Some(motion) = game.transition.take() {
                // Finish at exactly the same camera position as the last
                // animation frame before starting the active-board navigation.
                if let Some(row) = times.iter().position(|&t| t == motion.to_t) {
                    offset = (row as f32 * height).min(max_offset);
                }
                game.jump_to = game.active_times().first().copied().or(Some(motion.to_t));
            }
        }
        if let Some(motion) = &mut game.transition {
            game.viewport.scroll = None;
            motion.start(offset / height);
            if let Some(row) = times.iter().position(|&t| t == motion.to_t) {
                let target = (row as f32 * height).min(max_offset);
                let progress = travel_curve((raw - 0.08) / 0.76);
                offset = motion.camera_from_rows * height
                    + (target - motion.camera_from_rows * height) * progress;
            }
        } else {
            offset = offset.clamp(0.0, max_offset);
            let wheel = if ui.is_enabled()
                && ui.ctx().pointer_hover_pos().is_some_and(|pos| {
                    viewport.contains(pos)
                        && ui
                            .ctx()
                            .layer_id_at(pos)
                            .is_none_or(|layer| layer == ui.layer_id())
                }) {
                ui.ctx().input_mut(|input| {
                    let delta = input.smooth_scroll_delta.y;
                    input.smooth_scroll_delta = Vec2::ZERO;
                    delta
                })
            } else {
                0.0
            };
            if wheel != 0.0 {
                game.viewport.scroll = None;
                game.jump_to = None;
                game.viewport.focused = None;
                offset = (offset - wheel).clamp(0.0, max_offset);
            }
            if let Some(target) = game.jump_to.take()
                && let Some(row) = times.iter().position(|&t| t == target)
            {
                game.viewport.focused = Some(target);
                let target_offset = (row as f32 * height).min(max_offset);
                if game.viewport.times.is_empty() || (target_offset - offset).abs() < 0.5 {
                    game.viewport.scroll = None;
                    offset = target_offset;
                } else {
                    game.viewport.scroll = Some(SmoothScroll::new(target, offset / height, row));
                }
            }
            if let Some(scroll) = &game.viewport.scroll {
                if let Some(row) = times.iter().position(|&t| t == scroll.target) {
                    let progress = scroll.progress();
                    let target = (row as f32 * height).min(max_offset);
                    let from = (scroll.from_rows * height).min(max_offset);
                    offset = from + (target - from) * scroll_curve(progress);
                    if progress >= 1.0 {
                        game.viewport.scroll = None;
                    }
                } else {
                    game.viewport.scroll = None;
                }
            }
        }
        game.viewport.times = times.clone();
        game.viewport.stride = height;
        game.viewport.offset = offset;
        game.viewport.motion_progress = raw;
        game.viewport.frame = Some(frame);
    }
    game.facing.update(
        game.engine.turn() == Color::Black,
        game.transition.is_some(),
        frame,
    );
    let flipped = game.facing.black_bottom;
    let offset = game.viewport.offset;
    let moving = game.transition.is_some() || game.facing.animating();
    let painter = ui
        .painter()
        .with_clip_rect(viewport.intersect(ui.clip_rect()));
    let board_left = viewport.center().x + 40.0 - cell * 4.0;
    let rail_x = board_left - (cell * 0.62 + 24.0).max(39.0);
    let board_at = |index: usize| {
        let left = board_left;
        Rect::from_min_size(
            Pos2::new(
                left,
                viewport.top() + index as f32 * height - offset + gap * 0.5,
            ),
            Vec2::splat(cell * 8.0),
        )
    };
    if !times.is_empty() {
        painter.line_segment(
            [
                Pos2::new(rail_x, board_at(0).top() - gap * 0.5),
                Pos2::new(rail_x, board_at(times.len() - 1).bottom() + gap * 0.5),
            ],
            Stroke::new(1.5, Color32::from_rgb(73, 65, 83)),
        );
    }
    let first = (offset.max(0.0) / height).floor() as usize;
    let last = ((offset + viewport.height()) / height).ceil() as usize;
    let mut rows = Vec::new();
    let mut hovered = None;
    for index in first..last.min(times.len()) {
        let t = times[index];
        let board = board_at(index);
        let response = ui.interact(
            board.intersect(viewport),
            ui.make_persistent_id(("timeline_board", t)),
            Sense::click(),
        );
        if !moving && let Some(pos) = response.hover_pos() {
            hovered = Some((t, square_at(board, cell, pos, flipped)));
        }
        rows.push((t, board, response));
    }
    let mut board_painter = painter.clone();
    board_painter.multiply_opacity(game.facing.opacity);
    for (t, board, response) in rows {
        let own_active = game
            .pieces(t)
            .iter()
            .any(|p| p.color == game.engine.turn() && p.can_move() && p.is_movable());
        let dot = Pos2::new(rail_x, board.center().y);
        painter.circle_filled(
            dot,
            if own_active { 4.0 } else { 3.0 },
            if own_active {
                INVERTED
            } else {
                MUTED.gamma_multiply(0.5)
            },
        );
        painter.text(
            dot - Vec2::new(12.0, 0.0),
            Align2::RIGHT_CENTER,
            t.to_string(),
            FontId::proportional(14.0),
            MUTED,
        );
        // The moving board owns the destination until it settles. Never
        // composite two copies of its squares and antialiased pieces.
        let covered_by_transition = game
            .transition
            .as_ref()
            .is_some_and(|motion| motion.to_t == t && game.viewport.motion_progress < 0.84);
        if !covered_by_transition {
            paint_board(&board_painter, game, art, t, board, cell, hovered);
        }
        if !moving
            && game.viewport.scroll.is_none()
            && crate::input::clicked(&response)
            && let Some(pos) = response.interact_pointer_pos()
        {
            // Apply next Update, never halfway through drawing a history frame.
            game.pending_square = Some((t, square_at(board, cell, pos, flipped)));
            ui.ctx().request_repaint();
        }
    }
    if let Some(motion) = &game.transition
        && game.viewport.motion_progress < 0.84
        && let Some(from) = times.iter().position(|&t| t == motion.from_t)
        && let Some(to) = times.iter().position(|&t| t == motion.to_t)
    {
        paint_transition(
            &board_painter,
            game,
            art,
            motion,
            board_at(from),
            board_at(to),
            game.viewport.motion_progress,
        );
    }
    // Pinned beside the rail, independent of board scroll and zoom.
    let controls = Rect::from_min_size(
        Pos2::new(
            (rail_x - 87.0).max(viewport.left() + 2.0),
            viewport.center().y - 46.0,
        ),
        Vec2::new(46.0, 92.0),
    );
    painter.rect_filled(controls, 8.0, BACKGROUND);
    let mut nav = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("timeline_navigation")
            .max_rect(controls.shrink(4.0))
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    let enabled = !moving && game.replay_job.is_none() && !game.active_times().is_empty();
    if icon_button(
        &mut nav,
        Icon::Up,
        "Предыдущая активная доска",
        false,
        enabled,
    )
    .clicked()
    {
        game.navigate_active(false);
    }
    if icon_button(
        &mut nav,
        Icon::Down,
        "Следующая активная доска",
        false,
        enabled,
    )
    .clicked()
    {
        game.navigate_active(true);
    }
    if game.transition.is_some()
        || game.facing.animating()
        || game.viewport.scroll.is_some()
        || game.jump_to.is_some()
    {
        ui.ctx().request_repaint();
    }
}

fn mix_color(from: Color32, to: Color32, progress: f32) -> Color32 {
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * progress).round() as u8;
    Color32::from_rgb(
        mix(from.r(), to.r()),
        mix(from.g(), to.g()),
        mix(from.b(), to.b()),
    )
}

fn active_board_colors(side: Color) -> (Color32, Color32) {
    match side {
        Color::White => (
            Color32::from_rgb(248, 248, 240),
            Color32::from_rgb(225, 227, 220),
        ),
        Color::Black => (Color32::from_rgb(37, 37, 37), Color32::from_rgb(66, 65, 76)),
    }
}

// Rounded double frame; its extent always fits inside the row gap.
// Dimensions scale with the squares rather than fixed UI pixels.
fn paint_frame(
    painter: &egui::Painter,
    board: Rect,
    cell: f32,
    outer: Color32,
    inner: Color32,
    activity: f32,
    opacity: f32,
) {
    let padding = (cell * 0.12).max(4.0) * (1.0 - activity) + cell * 0.52 * activity;
    let corner = cell * (0.10 + 0.40 * activity);
    let border = cell * 0.12 * activity;
    let frame = board.expand(padding);
    painter.rect_filled(frame, corner, outer.gamma_multiply(opacity));
    painter.rect_filled(
        frame.shrink(border),
        (corner - border).max(0.0),
        inner.gamma_multiply(opacity),
    );
}

fn paint_transition(
    painter: &egui::Painter,
    game: &GameUi,
    art: &PieceArt,
    motion: &BoardTransition,
    from: Rect,
    to: Rect,
    raw: f32,
) {
    let phase = (raw / 0.84).clamp(0.0, 1.0);
    let progress = travel_curve(phase);
    let center = from.center() + (to.center() - from.center()) * progress;
    let lift = (phase * std::f32::consts::PI).sin();
    let board = Rect::from_center_size(center, from.size());
    let cell = board.width() / 8.0;
    // Match the destination's normal opacity, including unresolved preboards.
    // Completion switches renderers; it does not fade one board over another.
    let target_opacity = if game.engine.board(motion.to_t).is_some() {
        1.0
    } else {
        0.4
    };
    let opacity = 1.0 + (target_opacity - 1.0) * progress;
    let (from_outer, from_inner) = active_board_colors(Color::from_board(motion.from_t));
    let (to_outer, to_inner) = active_board_colors(Color::from_board(motion.to_t));
    let activity = |target: bool| {
        motion.tracks.iter().any(|track| {
            let (visible, piece) = if target {
                (track.visible_to, track.to)
            } else {
                (track.visible_from, track.from)
            };
            visible && piece.can_move() && piece.is_movable()
        }) as u8 as f32
    };
    paint_frame(
        painter,
        board,
        cell,
        mix_color(from_outer, to_outer, progress),
        mix_color(from_inner, to_inner, progress),
        activity(false) + (activity(true) - activity(false)) * progress,
        opacity,
    );
    for x in 0..8 {
        for y in 0..8 {
            let rect = Rect::from_min_size(
                square_rect(board, cell, Sq(x, y), game.facing.black_bottom).min,
                Vec2::splat(cell),
            );
            painter.rect_filled(
                rect,
                0.0,
                (if (x + y) % 2 == 0 { DARK } else { LIGHT }).gamma_multiply(opacity),
            );
        }
    }
    // Match the static renderer's placement of overlapping incarnations at
    // both ends, so settling into a preboard doesn't resize or jump pieces.
    let placement = |index: usize, target: bool| {
        let endpoint = |track: &crate::animation::PieceMotion| {
            if target {
                (track.to, track.visible_to)
            } else {
                (track.from, track.visible_from)
            }
        };
        let (piece, visible) = endpoint(&motion.tracks[index]);
        let pieces = if target {
            &motion.target
        } else {
            &motion.source
        };
        let count = if visible {
            pieces.iter().filter(|p| p.sq == piece.sq).count()
        } else {
            1
        };
        let rank = pieces
            .iter()
            .filter(|p| p.sq == piece.sq)
            .position(|p| identity(*p) == identity(piece))
            .unwrap_or(0);
        let shift = if count > 1 {
            (rank as f32 / (count - 1) as f32 - 0.5) * cell * 0.28
        } else {
            0.0
        };
        let center = square_rect(board, cell, piece.sq, game.facing.black_bottom).center()
            + Vec2::new(shift, -shift);
        (center, cell * if count > 1 { 0.82 } else { 0.98 })
    };
    {
        // Moving pieces draw last, above stationary pieces and fading victims.
        for moving in [false, true] {
            for (index, track) in motion
                .tracks
                .iter()
                .enumerate()
                .filter(|(_, track)| (track.from.sq != track.to.sq) == moving)
            {
                let (a, from_size) = placement(index, false);
                let (b, to_size) = placement(index, true);
                let center = a + (b - a) * progress;
                let visibility = match (track.visible_from, track.visible_to) {
                    (true, true) => 1.0,
                    (true, false) => 1.0 - ease((phase - 0.65) / 0.35),
                    (false, true) => ease(phase / 0.35),
                    (false, false) => 0.0,
                };
                if track.reversed_move && moving {
                    painter.line_segment(
                        [a, center],
                        Stroke::new(cell * 0.035, INVERTED.gamma_multiply(0.3 * lift * opacity)),
                    );
                }
                let rect = Rect::from_center_size(
                    center,
                    Vec2::splat(from_size + (to_size - from_size) * progress),
                );
                let alpha = opacity * visibility;
                let style = |piece: temporal_chess_engine::model::PieceView| {
                    piece_style(game, piece, &game.checks, None)
                };
                if track.from.kind == track.to.kind && track.from.inverted == track.to.inverted {
                    art.paint_styled(
                        painter,
                        track.to.kind,
                        track.to.color,
                        rect,
                        alpha,
                        style(track.to),
                    );
                } else {
                    let change = ease((phase - 0.5) / 0.35);
                    art.paint_styled(
                        painter,
                        track.from.kind,
                        track.from.color,
                        rect,
                        alpha * (1.0 - change),
                        style(track.from),
                    );
                    art.paint_styled(
                        painter,
                        track.to.kind,
                        track.to.color,
                        rect,
                        alpha * change,
                        style(track.to),
                    );
                }
            }
        }
    }
    for index in 0..8 {
        let file_color = if index % 2 == 0 { LIGHT } else { DARK };
        painter.text(
            board.min + Vec2::new((index + 1) as f32 * cell - 3.0, board.height() - 1.0),
            Align2::RIGHT_BOTTOM,
            char::from(
                b'a' + if game.facing.black_bottom {
                    7 - index
                } else {
                    index
                } as u8,
            ),
            FontId::proportional((cell * 0.20).max(9.0)),
            file_color.gamma_multiply(opacity),
        );
        let rank_color = if (7 - index) % 2 == 0 { LIGHT } else { DARK };
        painter.text(
            board.min + Vec2::new(3.0, index as f32 * cell + 3.0),
            Align2::LEFT_TOP,
            if game.facing.black_bottom {
                index + 1
            } else {
                8 - index
            },
            FontId::proportional((cell * 0.20).max(9.0)),
            rank_color.gamma_multiply(opacity),
        );
    }
}

/// Board-local mapping is shared by pieces, arrows, hints and pointer input.
fn square_rect(board: Rect, cell: f32, sq: Sq, flipped: bool) -> Rect {
    let (column, row) = if flipped {
        (7 - sq.0, sq.1)
    } else {
        (sq.0, 7 - sq.1)
    };
    Rect::from_min_size(
        board.min + Vec2::new(column as f32 * cell, row as f32 * cell),
        Vec2::splat(cell),
    )
}

fn square_at(board: Rect, cell: f32, pos: Pos2, flipped: bool) -> Sq {
    let column = (((pos.x - board.left()) / cell) as usize).min(7);
    let row = (((pos.y - board.top()) / cell) as usize).min(7);
    if flipped {
        Sq(7 - column, row)
    } else {
        Sq(column, 7 - row)
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_board(
    painter: &egui::Painter,
    game: &GameUi,
    art: &PieceArt,
    t: usize,
    board: Rect,
    cell: f32,
    hovered: Option<(usize, Sq)>,
) {
    let computed = game.engine.board(t).is_some();
    let alpha = if computed { 1.0 } else { 0.4 };
    let pieces = game.pieces(t);
    let active = pieces.iter().any(|p| p.can_move() && p.is_movable());
    let side = Color::from_board(t);
    let (outer, inner) = active_board_colors(side);
    paint_frame(
        painter,
        board,
        cell,
        outer,
        inner,
        active as u8 as f32,
        alpha,
    );
    let checked = &game.checks;
    // Mark the latest retained move originating on this board only. A terminal
    // paradox is accepted in the journal even though its timeline cannot settle.
    let previous = game.record.commands().iter().rev().find_map(|command| {
        if let Command::Move { mv, from } = command
            && mv.t == t
            && (game.engine.history().moves().get(&mv.id()) == Some(mv)
                || (game.record.commands().last() == Some(command)
                    && matches!(
                        game.engine.outcome(),
                        Some(temporal_chess_engine::engine::GameOutcome::Victory {
                            reason: temporal_chess_engine::engine::WinReason::Paradox,
                            ..
                        })
                    )))
        {
            Some((*mv, *from))
        } else {
            None
        }
    });
    let mut artwork = Vec::new();
    for x in 0..8 {
        for y in 0..8 {
            let sq = Sq(x, y);
            let rect = Rect::from_min_size(
                square_rect(board, cell, Sq(x, y), game.facing.black_bottom).min,
                Vec2::splat(cell),
            );
            let dark = (x + y) % 2 == 0;
            let base = if dark { DARK } else { LIGHT };
            painter.rect_filled(rect, 0.0, base.gamma_multiply(alpha));
            let hovering = hovered == Some((t, sq));
            let occupants: Vec<_> = pieces.iter().filter(|p| p.sq == sq).collect();
            let mut marker = None;
            let fatal = game.selected.as_ref().is_some_and(|selection| {
                selection.piece.t == t
                    && selection
                        .fatal_captures
                        .iter()
                        .any(|(mv, _)| mv.direction == selection.direction && mv.sq == sq)
            });
            if let Some(selection) = &game.selected {
                if selection.piece.t == t && selection.piece.sq == sq {
                    painter.rect_filled(
                        rect,
                        0.0,
                        direction_color(selection.direction).gamma_multiply(if hovering {
                            0.53
                        } else {
                            0.36
                        }),
                    );
                }
                if !selection.moves_to(t, sq).is_empty() {
                    let capture = selection
                        .captures
                        .iter()
                        .any(|(mv, _)| mv.direction == selection.direction && mv.sq == sq);
                    marker = Some((
                        direction_color(selection.direction),
                        capture || fatal,
                        false,
                    ));
                } else if selection.skip_to(t, sq) {
                    marker = Some((direction_color(selection.direction), false, true));
                }
            }
            if let Some((color, _, _)) = marker {
                painter.rect_filled(
                    rect,
                    0.0,
                    color.gamma_multiply(if hovering { 0.28 } else { 0.08 }),
                );
                if hovering {
                    painter.rect_stroke(
                        rect.shrink(1.0),
                        3.0,
                        Stroke::new(2.0, color.gamma_multiply(0.85)),
                        StrokeKind::Inside,
                    );
                }
            }
            for (index, piece) in occupants.iter().enumerate() {
                let count = occupants.len();
                let shift = if count > 1 {
                    (index as f32 / (count - 1) as f32 - 0.5) * cell * 0.28
                } else {
                    0.0
                };
                let size = cell * if count > 1 { 0.82 } else { 0.98 };
                artwork.push((
                    **piece,
                    Rect::from_center_size(
                        rect.center() + Vec2::new(shift, -shift),
                        Vec2::splat(size),
                    ),
                ));
            }

            if let Some((color, capture, skip)) = marker {
                // The capture indication belongs to the victim's silhouette.
                // Empty source squares still need a destination marker when
                // their victim exists only on the temporal target board.
                if fatal {
                    if occupants.is_empty() {
                        paint_skull(painter, rect, alpha, hovering);
                    }
                } else if skip {
                    painter.circle_stroke(
                        rect.center(),
                        cell * if hovering { 0.24 } else { 0.20 },
                        Stroke::new(2.5, color.gamma_multiply(0.85)),
                    );
                    painter.circle_filled(rect.center(), cell * 0.065, color);
                } else if occupants.is_empty() || !capture {
                    painter.circle_filled(
                        rect.center(),
                        cell * if hovering { 0.19 } else { 0.145 },
                        (if capture { CAPTURE } else { color }).gamma_multiply(if hovering {
                            0.95
                        } else {
                            0.78
                        }),
                    );
                }
            }
            if occupants.len() > 1 {
                painter.text(
                    rect.right_bottom() - Vec2::splat(4.0),
                    Align2::RIGHT_BOTTOM,
                    format!("×{}", occupants.len()),
                    FontId::proportional(12.0),
                    Color32::WHITE,
                );
            }
            if game
                .inspected
                .is_some_and(|id| occupants.iter().any(|p| identity(**p) == id))
            {
                painter.rect_stroke(
                    rect.shrink(1.0),
                    0.0,
                    Stroke::new(2.0, INVERTED),
                    StrokeKind::Inside,
                );
            }
            let coordinate_color = if dark { LIGHT } else { DARK }.gamma_multiply(alpha);
            if x == if game.facing.black_bottom { 7 } else { 0 } {
                painter.text(
                    rect.left_top() + Vec2::splat(3.0),
                    Align2::LEFT_TOP,
                    y + 1,
                    FontId::proportional((cell * 0.20).max(9.0)),
                    coordinate_color,
                );
            }
            if y == if game.facing.black_bottom { 7 } else { 0 } {
                painter.text(
                    rect.right_bottom() - Vec2::new(3.0, 1.0),
                    Align2::RIGHT_BOTTOM,
                    char::from(b'a' + x as u8),
                    FontId::proportional((cell * 0.20).max(9.0)),
                    coordinate_color,
                );
            }
        }
    }
    // Paint after every square, so neighboring squares cannot crop the glow.
    {
        for &(piece, rect) in &artwork {
            art.paint_styled(
                painter,
                piece.kind,
                piece.color,
                rect,
                alpha,
                piece_style(game, piece, checked, hovered),
            );
        }
    }
    if let Some((mv, from)) = previous {
        let highlighted =
            hovered.is_some_and(|(time, sq)| time == t && (sq == from || sq == mv.sq));
        paint_move_arrow(
            painter,
            board,
            cell,
            from,
            mv.sq,
            game.facing.black_bottom,
            direction_color(mv.direction)
                .gamma_multiply(alpha * if highlighted { 0.95 } else { 0.78 }),
        );
    }
    // Badges are drawn after all pieces so neighboring artwork cannot cover them.
    for (piece, rect) in artwork {
        if let Some(selection) = &game.selected {
            let (fatal, emphasized) =
                target_marker(selection, &selection.fatal_captures, piece, hovered);
            if fatal {
                paint_skull(
                    painter,
                    rect,
                    alpha,
                    emphasized || hovered == Some((piece.t, piece.sq)),
                );
            }
        }
    }
}

fn paint_move_arrow(
    painter: &egui::Painter,
    board: Rect,
    cell: f32,
    from: Sq,
    to: Sq,
    flipped: bool,
    color: Color32,
) {
    let center = |sq: Sq| square_rect(board, cell, sq, flipped).center();
    let start = center(from);
    let end = center(to);
    let delta = end - start;
    if delta.length_sq() < 1.0 {
        return;
    }
    let direction = delta.normalized();
    let normal = Vec2::new(-direction.y, direction.x);
    let tip = end - direction * cell * 0.06;
    let neck = tip - direction * cell * 0.30;
    painter.line_segment(
        [start + direction * cell * 0.10, neck],
        Stroke::new(cell * 0.11, color),
    );
    painter.add(egui::Shape::convex_polygon(
        vec![
            tip,
            neck + normal * cell * 0.20,
            neck - normal * cell * 0.20,
        ],
        color,
        Stroke::NONE,
    ));
}

fn target_marker(
    selection: &crate::controller::Selection,
    targets: &[(
        temporal_chess_engine::model::Move,
        temporal_chess_engine::model::PieceView,
    )],
    piece: temporal_chess_engine::model::PieceView,
    hovered: Option<(usize, Sq)>,
) -> (bool, bool) {
    let mut marked = false;
    let mut emphasized = false;
    for (mv, target) in targets {
        if mv.direction == selection.direction
            && target.origin == piece.origin
            && target.sq == piece.sq
            && target.inverted == piece.inverted
            && target.loop_turn == piece.loop_turn
            && (identity(*target) == identity(piece) || piece.t == selection.piece.t)
        {
            marked = true;
            emphasized |= hovered == Some((selection.piece.t, mv.sq));
        }
    }
    (marked, emphasized)
}

fn paint_skull(painter: &egui::Painter, piece_rect: Rect, alpha: f32, hovered: bool) {
    let size = (piece_rect.width() * if hovered { 0.36 } else { 0.31 }).clamp(13.0, 28.0);
    let rect = Rect::from_min_size(
        piece_rect.right_top() + Vec2::new(-size, 1.0),
        Vec2::splat(size),
    );
    painter.circle_filled(
        rect.center(),
        size * 0.57,
        BACKGROUND.gamma_multiply(alpha * 0.95),
    );
    crate::icons::paint(
        painter,
        Icon::Skull,
        rect,
        Stroke::new(1.5, CAPTURE.gamma_multiply(alpha)),
    );
}

fn piece_style(
    game: &GameUi,
    piece: temporal_chess_engine::model::PieceView,
    checked: &CheckStatus,
    hovered: Option<(usize, Sq)>,
) -> PieceStyle {
    let (capture, capture_hovered) = game.selected.as_ref().map_or((false, false), |selection| {
        target_marker(selection, &selection.captures, piece, hovered)
    });
    let outline = if capture {
        Outline::Capture
    } else if checked.ordinary.contains(&identity(piece)) {
        Outline::Check
    } else if checked.inverted.contains(&identity(piece)) {
        Outline::InvertedCheck
    } else if !piece.can_move() || piece.t == game.engine.history().horizon() {
        Outline::None
    } else if piece.rights.cancelled_move {
        Outline::Cancelled
    } else if piece.rights.new_information {
        Outline::Information
    } else if piece.rights.ordinary {
        Outline::Ordinary
    } else {
        Outline::None
    };
    PieceStyle {
        inverted: piece.inverted,
        outline,
        hovered: capture_hovered || hovered == Some((piece.t, piece.sq)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_outline_follows_direction_and_hover_without_changing_the_game() {
        let mut game = GameUi::default();
        game.click_square(0, Sq(4, 1));
        game.click_square(0, Sq(4, 3));
        game.click_square(1, Sq(3, 6));
        game.click_square(1, Sq(3, 4));
        game.click_square(2, Sq(4, 3));
        let before = game.engine.clone();
        let victim = *game.engine.board(2).unwrap().get_piece(3, 4).unwrap();
        let quiet = piece_style(&game, victim, &CheckStatus::default(), None);
        assert_eq!(quiet.outline, Outline::Capture);
        assert!(!quiet.hovered);
        let hovered = piece_style(&game, victim, &CheckStatus::default(), Some((2, Sq(3, 4))));
        assert_eq!(hovered.outline, Outline::Capture);
        assert!(hovered.hovered);
        game.toggle_direction();
        assert_ne!(
            piece_style(&game, victim, &CheckStatus::default(), None).outline,
            Outline::Capture
        );
        assert_eq!(game.engine, before);
    }
}
