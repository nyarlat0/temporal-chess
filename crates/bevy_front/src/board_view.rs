use crate::{
    animation::{BoardTransition, SmoothScroll, ease},
    controller::{GameUi, identity, side_name},
    piece_art::{Outline, PieceStyle},
    view::{CAPTURE, FUTURE, INVERTED, MUTED, PLAYED_FUTURE, PLAYED_PAST, direction_color},
};
use bevy_egui::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use temporal_chess_engine::{
    engine::CheckStatus,
    model::{Color, Sq},
    record::Command,
};

const LIGHT: Color32 = Color32::from_rgb(240, 217, 181);
const DARK: Color32 = Color32::from_rgb(181, 136, 99);

pub fn ribbon(ui: &mut egui::Ui, game: &mut GameUi) {
    if game
        .transition
        .as_ref()
        .is_some_and(|motion| motion.progress() >= 1.0)
    {
        let arrival = game.transition.take().map(|motion| motion.to_t);
        game.jump_to = game.active_times().first().copied().or(arrival);
    }
    let times = game.times();
    let cell = game
        .cell_size
        .min(((ui.available_width() - 116.0) / 8.0).max(18.0));
    let height = 8.0 * cell + 86.0;
    let mut offset = game.viewport.anchored_offset(&times, height);
    let moving = game.transition.is_some();
    if moving {
        game.viewport.scroll = None;
        if let Some(motion) = &mut game.transition {
            motion.start(offset / height);
            if let Some(row) = times.iter().position(|&t| t == motion.to_t) {
                // Let the board lead the camera slightly, so the temporal
                // direction remains visible while both endpoints stay in view.
                let progress = ease((motion.progress() - 0.12) / 0.88);
                offset = (motion.camera_from_rows
                    + (row as f32 - motion.camera_from_rows) * progress)
                    * height;
            }
        }
        ui.ctx().request_repaint();
    } else {
        // Wheel input or grabbing the scrollbar takes control from auto-scroll.
        let ribbon_rect = ui.available_rect_before_wrap();
        if ui.ctx().input(|i| {
            i.pointer
                .hover_pos()
                .is_some_and(|pos| ribbon_rect.contains(pos))
                && (i.smooth_scroll_delta.y != 0.0 || i.pointer.any_pressed())
        }) {
            game.viewport.scroll = None;
        }
        if let Some(target) = game.jump_to.take()
            && let Some(row) = times.iter().position(|&t| t == target)
        {
            if game.viewport.times.is_empty() || (offset / height - row as f32).abs() < 0.002 {
                game.viewport.scroll = None;
                offset = row as f32 * height;
            } else {
                game.viewport.scroll = Some(SmoothScroll::new(target, offset / height, row));
            }
        }
        if let Some(scroll) = &game.viewport.scroll {
            if let Some(row) = times.iter().position(|&t| t == scroll.target) {
                let progress = scroll.progress();
                offset =
                    (scroll.from_rows + (row as f32 - scroll.from_rows) * ease(progress)) * height;
                if progress >= 1.0 {
                    game.viewport.scroll = None;
                } else {
                    ui.ctx().request_repaint();
                }
            } else {
                game.viewport.scroll = None;
            }
        }
    }
    ui.spacing_mut().item_spacing.y = 0.0;
    let padding = (ui.available_height() / height).ceil() as usize;
    let output = egui::ScrollArea::vertical()
        .id_salt("timeline")
        .auto_shrink([false, false])
        .scroll_source(if moving {
            egui::scroll_area::ScrollSource::NONE
        } else {
            egui::scroll_area::ScrollSource::default()
        })
        .vertical_scroll_offset(offset)
        .show_rows(ui, height, times.len() + padding, |ui, range| {
            let mut rows = Vec::new();
            let mut hovered = None;
            let mut anchor = None;
            for index in range {
                let (_, row) = ui.allocate_space(Vec2::new(ui.available_width(), height));
                anchor.get_or_insert((index, row));
                let Some(&t) = times.get(index) else {
                    continue;
                };
                let response = ui.interact(
                    row,
                    ui.make_persistent_id(("timeline_board", t)),
                    Sense::click(),
                );
                let board = board_rect(row, cell);
                if !moving
                    && let Some(pos) = response.hover_pos().filter(|pos| board.contains(*pos))
                {
                    hovered = Some((t, square_at(board, cell, pos)));
                }
                rows.push((t, board, response));
            }
            for (t, board, response) in rows {
                paint_board(ui.painter(), game, t, board, cell, hovered);
                if !moving && hovered.is_some_and(|(time, _)| time == t) {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if !moving
                    && game.viewport.scroll.is_none()
                    && response.clicked()
                    && let Some(pos) = response.interact_pointer_pos()
                    && board.contains(pos)
                {
                    game.click_square(t, square_at(board, cell, pos));
                }
            }
            if let Some(motion) = &game.transition
                && let Some((anchor_index, anchor_row)) = anchor
                && let Some(from_index) = times.iter().position(|&t| t == motion.from_t)
                && let Some(to_index) = times.iter().position(|&t| t == motion.to_t)
            {
                let from = board_rect(
                    anchor_row.translate(Vec2::new(
                        0.0,
                        (from_index as f32 - anchor_index as f32) * height,
                    )),
                    cell,
                );
                let to = board_rect(
                    anchor_row.translate(Vec2::new(
                        0.0,
                        (to_index as f32 - anchor_index as f32) * height,
                    )),
                    cell,
                );
                paint_transition(ui.painter(), game, motion, from, to);
            }
        });
    game.viewport.times = times;
    game.viewport.stride = height;
    game.viewport.offset = output.state.offset.y;
}

fn board_rect(row: Rect, cell: f32) -> Rect {
    Rect::from_min_size(
        Pos2::new(row.center().x - cell * 4.0 + 18.0, row.top() + 46.0),
        Vec2::splat(cell * 8.0),
    )
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
        Color::Black => (
            Color32::from_rgb(37, 37, 37),
            Color32::from_rgb(66, 65, 76),
        ),
    }
}

// Wide rounded double frame with a hard offset shadow, like 5D Chess.
// Dimensions scale with the squares rather than fixed UI pixels.
fn paint_active_frame(
    painter: &egui::Painter,
    board: Rect,
    cell: f32,
    outer: Color32,
    inner: Color32,
    opacity: f32,
    lift: f32,
) {
    let padding = cell * 0.62;
    let corner = cell * 0.60;
    let border = cell * 0.14;
    let frame = board.expand(padding);
    painter.rect_filled(
        frame.translate(Vec2::new(-cell * 0.95, cell * 0.80 + lift)),
        corner,
        Color32::from_rgb(53, 53, 53).gamma_multiply(opacity * 0.95),
    );
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
    motion: &BoardTransition,
    from: Rect,
    to: Rect,
) {
    let raw = motion.progress();
    let progress = ease(raw);
    let center = from.center() + (to.center() - from.center()) * progress;
    let lift = (raw * std::f32::consts::PI).sin();
    let board = Rect::from_center_size(center, from.size());
    let cell = board.width() / 8.0;
    // Settle into the normal static renderer, including all final overlays.
    let opacity = 1.0 - ease((raw - 0.88) / 0.12);
    let (from_outer, from_inner) = active_board_colors(Color::from_board(motion.from_t));
    let (to_outer, to_inner) = active_board_colors(Color::from_board(motion.to_t));
    paint_active_frame(
        painter,
        board,
        cell,
        mix_color(from_outer, to_outer, progress),
        mix_color(from_inner, to_inner, progress),
        opacity,
        lift * 9.0,
    );
    for x in 0..8 {
        for y in 0..8 {
            let rect = Rect::from_min_size(
                board.min + Vec2::new(x as f32 * cell, (7 - y) as f32 * cell),
                Vec2::splat(cell),
            );
            painter.rect_filled(
                rect,
                0.0,
                (if (x + y) % 2 == 0 { DARK } else { LIGHT }).gamma_multiply(opacity),
            );
            if motion.played.sq == Sq(x, y) {
                painter.rect_filled(
                    rect,
                    0.0,
                    (if motion.played.to_past() {
                        PLAYED_PAST
                    } else {
                        PLAYED_FUTURE
                    })
                    .gamma_multiply(0.4 * opacity),
                );
            }
        }
    }
    let square_center =
        |sq: Sq| board.min + Vec2::new((sq.0 as f32 + 0.5) * cell, (7.5 - sq.1 as f32) * cell);
    if let Some(art) = &game.art {
        // Moving pieces draw last, above stationary pieces and fading victims.
        for moving in [false, true] {
            for track in motion
                .tracks
                .iter()
                .filter(|track| (track.from.sq != track.to.sq) == moving)
            {
                let a = square_center(track.from.sq);
                let b = square_center(track.to.sq);
                let center = a + (b - a) * progress;
                let visibility = match (track.visible_from, track.visible_to) {
                    (true, true) => 1.0,
                    (true, false) => 1.0 - ease((raw - 0.65) / 0.35),
                    (false, true) => ease(raw / 0.35),
                    (false, false) => 0.0,
                };
                if track.reversed_move && moving {
                    painter.line_segment(
                        [a, center],
                        Stroke::new(cell * 0.035, INVERTED.gamma_multiply(0.3 * lift * opacity)),
                    );
                }
                let rect = Rect::from_center_size(center, Vec2::splat(cell * 0.98));
                let alpha = opacity * visibility;
                let style = |piece: temporal_chess_engine::model::PieceView| PieceStyle {
                    inverted: piece.inverted,
                    ..Default::default()
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
                    let change = ease((raw - 0.5) / 0.35);
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
            char::from(b'a' + index as u8),
            FontId::proportional((cell * 0.20).max(9.0)),
            file_color.gamma_multiply(opacity),
        );
        let rank_color = if (7 - index) % 2 == 0 { LIGHT } else { DARK };
        painter.text(
            board.min + Vec2::new(3.0, index as f32 * cell + 3.0),
            Align2::LEFT_TOP,
            8 - index,
            FontId::proportional((cell * 0.20).max(9.0)),
            rank_color.gamma_multiply(opacity),
        );
    }
    let header = Rect::from_min_size(
        board.left_top() - Vec2::new(13.0, 35.0),
        Vec2::new(board.width() + 26.0, 27.0),
    );
    painter.rect_filled(
        header,
        5.0,
        Color32::from_rgb(32, 31, 29).gamma_multiply(opacity),
    );
    painter.text(
        board.left_top() - Vec2::new(0.0, 19.0),
        Align2::LEFT_CENTER,
        format!("t = {} → {}", motion.from_t, motion.to_t),
        FontId::proportional(15.0),
        Color32::WHITE.gamma_multiply(opacity),
    );
}

fn square_at(board: Rect, cell: f32, pos: Pos2) -> Sq {
    Sq(
        (((pos.x - board.left()) / cell) as usize).min(7),
        7 - (((pos.y - board.top()) / cell) as usize).min(7),
    )
}

fn paint_board(
    painter: &egui::Painter,
    game: &GameUi,
    t: usize,
    board: Rect,
    cell: f32,
    hovered: Option<(usize, Sq)>,
) {
    let computed = game.engine.board(t).is_some();
    let alpha = if computed { 1.0 } else { 0.4 };
    let pieces = game.pieces(t);
    let active = pieces.iter().any(|p| p.can_move() && p.is_movable());
    let own_active = pieces
        .iter()
        .any(|p| p.color == game.engine.turn() && p.can_move() && p.is_movable());
    let side = Color::from_board(t);
    let side_color = if side == Color::White {
        Color32::from_rgb(230, 227, 220)
    } else {
        Color32::from_rgb(12, 12, 12)
    };
    let rail_x = board.left() - (cell * 0.62 + 24.0).max(39.0);
    painter.line_segment(
        [
            Pos2::new(rail_x, board.top() - 46.0),
            Pos2::new(rail_x, board.bottom() + 40.0),
        ],
        Stroke::new(2.0, Color32::from_rgb(73, 65, 83)),
    );
    painter.circle_filled(
        Pos2::new(rail_x, board.top() - 19.0),
        5.0,
        if own_active {
            INVERTED
        } else {
            MUTED.gamma_multiply(0.4)
        },
    );
    if active {
        let (outer, inner) = active_board_colors(side);
        paint_active_frame(painter, board, cell, outer, inner, alpha, 0.0);
    } else {
        painter.rect_filled(
            board.expand(6.0).translate(Vec2::new(0.0, 5.0)),
            3.0,
            Color32::BLACK.gamma_multiply(0.26),
        );
        painter.rect_filled(
            board.expand(3.0),
            5.0,
            side_color.gamma_multiply(alpha),
        );
    }
    let checked = &game.checks;
    painter.text(
        board.left_top() - Vec2::new(0.0, 19.0),
        Align2::LEFT_CENTER,
        format!("t = {t}"),
        FontId::proportional(15.0),
        Color32::from_gray(222),
    );
    painter.text(
        board.right_top() - Vec2::new(0.0, 19.0),
        Align2::RIGHT_CENTER,
        if computed {
            format!(
                "{}{}",
                side_name(side),
                if t == game.engine.history().horizon() {
                    "  ·  настоящее"
                } else {
                    ""
                }
            )
        } else {
            "Предоска".to_owned()
        },
        FontId::proportional(12.0),
        MUTED,
    );
    // Follow command order, not worldline ordering: past moves can replace
    // earlier commands. Only highlight moves still retained by the engine.
    let previous = game.record.commands().iter().rev().find_map(|command| {
        if let Command::Move { mv, from } = command
            && (mv.t == t || mv.direction.step(mv.t) == Some(t))
            && game.engine.history().moves().get(&mv.id()) == Some(mv)
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
                board.min + Vec2::new(x as f32 * cell, (7 - y) as f32 * cell),
                Vec2::splat(cell),
            );
            let dark = (x + y) % 2 == 0;
            let base = if dark { DARK } else { LIGHT };
            painter.rect_filled(rect, 0.0, base.gamma_multiply(alpha));
            let hovering = hovered == Some((t, sq));
            if let Some((mv, from)) = previous
                && ((mv.t == t && from == sq)
                    || (mv.direction.step(mv.t) == Some(t) && mv.sq == sq))
            {
                let color = if mv.to_past() {
                    PLAYED_PAST
                } else {
                    PLAYED_FUTURE
                };
                painter.rect_filled(
                    rect,
                    0.0,
                    color.gamma_multiply(alpha * if hovering { 0.62 } else { 0.40 }),
                );
            }
            let occupants: Vec<_> = pieces.iter().filter(|p| p.sq == sq).collect();
            let mut marker = None;
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
                    marker = Some((direction_color(selection.direction), capture, false));
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
                if skip {
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
            if x == 0 {
                painter.text(
                    rect.left_top() + Vec2::splat(3.0),
                    Align2::LEFT_TOP,
                    y + 1,
                    FontId::proportional((cell * 0.20).max(9.0)),
                    coordinate_color,
                );
            }
            if y == 0 {
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
    if let Some(art) = &game.art {
        for (piece, rect) in artwork {
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
    if own_active && !computed {
        painter.text(
            board.left_bottom() + Vec2::new(0.0, 20.0),
            Align2::LEFT_CENTER,
            "Продолжите просчёт, чтобы ходить на этой доске",
            FontId::proportional(11.0),
            FUTURE,
        );
    }
}

fn piece_style(
    game: &GameUi,
    piece: temporal_chess_engine::model::PieceView,
    checked: &CheckStatus,
    hovered: Option<(usize, Sq)>,
) -> PieceStyle {
    let mut capture = false;
    let mut capture_hovered = false;
    if let Some(selection) = &game.selected {
        for (mv, victim) in &selection.captures {
            // Exact target occurrence, or its same-incarnation representation
            // on the source board where spatial moves are selected.
            if mv.direction == selection.direction
                && victim.origin == piece.origin
                && victim.sq == piece.sq
                && victim.inverted == piece.inverted
                && victim.loop_turn == piece.loop_turn
                && (identity(*victim) == identity(piece) || piece.t == selection.piece.t)
            {
                capture = true;
                capture_hovered |= hovered == Some((selection.piece.t, mv.sq));
            }
        }
    }
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
