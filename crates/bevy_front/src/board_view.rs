use crate::{
    controller::{GameUi, identity, side_name},
    piece_art::{Outline, PieceStyle},
    view::{CAPTURE, FUTURE, INVERTED, MUTED, PLAYED_FUTURE, PLAYED_PAST, direction_color},
};
use bevy_egui::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use temporal_chess_engine::{
    model::{Color, Sq},
    record::Command,
};

const LIGHT: Color32 = Color32::from_rgb(240, 217, 181);
const DARK: Color32 = Color32::from_rgb(181, 136, 99);

pub fn ribbon(ui: &mut egui::Ui, game: &mut GameUi) {
    let times = game.times();
    let cell = game
        .cell_size
        .min(((ui.available_width() - 116.0) / 8.0).max(18.0));
    let height = 8.0 * cell + 86.0;
    let offset = game
        .jump_to
        .take()
        .and_then(|target| times.iter().position(|&t| t == target))
        .map_or_else(
            || game.viewport.anchored_offset(&times, height),
            |index| index as f32 * height,
        );
    ui.spacing_mut().item_spacing.y = 0.0;
    // Empty space below the ribbon allows its bottom board to retain its screen
    // position even in a tall window when a new board is inserted at the top.
    let padding = (ui.available_height() / height).ceil() as usize;
    let output = egui::ScrollArea::vertical()
        .id_salt("timeline")
        .auto_shrink([false, false])
        .vertical_scroll_offset(offset)
        .show_rows(ui, height, times.len() + padding, |ui, range| {
            let mut rows = Vec::new();
            let mut hovered = None;
            for index in range {
                let (row, response) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
                let Some(&t) = times.get(index) else {
                    continue;
                };
                let board = Rect::from_min_size(
                    Pos2::new(row.center().x - cell * 4.0 + 18.0, row.top() + 46.0),
                    Vec2::splat(cell * 8.0),
                );
                if let Some(pos) = response.hover_pos().filter(|pos| board.contains(*pos)) {
                    hovered = Some((t, square_at(board, cell, pos)));
                }
                rows.push((t, board, response));
            }
            for (t, board, response) in rows {
                paint_board(ui.painter(), game, t, board, cell, hovered);
                if hovered.is_some_and(|(time, _)| time == t) {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked()
                    && let Some(pos) = response.interact_pointer_pos()
                    && board.contains(pos)
                {
                    let x = (((pos.x - board.left()) / cell) as usize).min(7);
                    let y = 7 - (((pos.y - board.top()) / cell) as usize).min(7);
                    game.click_square(t, Sq(x, y));
                }
            }
        });
    game.viewport.times = times;
    game.viewport.stride = height;
    game.viewport.offset = output.state.offset.y;
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
    let rail_x = board.left() - 39.0;
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
    painter.rect_filled(
        board.expand(6.0).translate(Vec2::new(0.0, 5.0)),
        3.0,
        Color32::BLACK.gamma_multiply(0.26),
    );
    painter.rect_filled(
        board.expand(if active { 13.0 } else { 3.0 }),
        if active { 14.0 } else { 5.0 },
        side_color.gamma_multiply(alpha),
    );
    let checked: Vec<_> = [Color::White, Color::Black]
        .into_iter()
        .filter_map(|side| game.engine.checks(side).ok())
        .flat_map(|checks| checks.ordinary.into_iter().chain(checks.inverted))
        .collect();
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
                piece_style(game, piece, &checked, hovered),
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
    checked: &[temporal_chess_engine::model::PieceAt],
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
    } else if checked.contains(&identity(piece)) {
        Outline::Check
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
        let quiet = piece_style(&game, victim, &[], None);
        assert_eq!(quiet.outline, Outline::Capture);
        assert!(!quiet.hovered);
        let hovered = piece_style(&game, victim, &[], Some((2, Sq(3, 4))));
        assert_eq!(hovered.outline, Outline::Capture);
        assert!(hovered.hovered);
        game.toggle_direction();
        assert_ne!(
            piece_style(&game, victim, &[], None).outline,
            Outline::Capture
        );
        assert_eq!(game.engine, before);
    }
}
