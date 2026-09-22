use crate::pieces::{Color, PieceKind};

use super::{Castling, Engine, EngineErr, Move, MoveEffect, PieceId, Promotion, TimeDirection};

impl Engine {
    /// Spatially legal candidates at a worldline occurrence, including past-directed
    /// candidates. This does not check turn ownership, optional reaction rights or
    /// whether constructing a new temporal loop is implemented.
    pub fn possible_moves(&self, wid: usize, age: usize) -> Result<Vec<Move>, EngineErr> {
        let piece = self.get_piv(wid, age).ok_or(EngineErr::WrongPiece)?;
        let board = self
            .projection
            .boards
            .get(&piece.t)
            .ok_or(EngineErr::AbsentBoard)?;
        let candidate = |sq, direction, effect| Move {
            pid: PieceId::from_piv(piece),
            t: piece.t,
            sq,
            direction,
            effect,
        };
        let mut moves = Vec::new();
        for square in piece.basic_moves(board) {
            moves.push(candidate(square, TimeDirection::Future, MoveEffect::Normal));
            if piece.t > 1 {
                moves.push(candidate(square, TimeDirection::Past, MoveEffect::Normal));
            }
        }
        for (square, castling) in piece.castling_moves(board) {
            moves.push(candidate(
                square,
                TimeDirection::Future,
                MoveEffect::Castle(castling),
            ));
        }
        for direction in [TimeDirection::Future, TimeDirection::Past] {
            let previous_time = match direction {
                TimeDirection::Future => piece.t.checked_sub(1),
                TimeDirection::Past if piece.t > 1 => piece.t.checked_add(1),
                TimeDirection::Past => None,
            };
            if let Some(previous) = previous_time.and_then(|t| self.projection.preboards.get(&t)) {
                for (square, captured) in piece.en_passant_moves(board, previous) {
                    moves.push(candidate(
                        square,
                        direction,
                        MoveEffect::EnPassant { captured },
                    ));
                }
            }
        }

        if matches!(piece.kind, PieceKind::P { .. }) {
            let last_y = if piece.color == Color::White { 7 } else { 0 };
            moves = moves
                .into_iter()
                .flat_map(|mv| {
                    if mv.sq.1 == last_y {
                        Promotion::ALL
                            .into_iter()
                            .map(|kind| Move {
                                effect: MoveEffect::Promote(kind),
                                ..mv
                            })
                            .collect()
                    } else {
                        vec![mv]
                    }
                })
                .collect();
        }

        // Check every king of this color (a time slice can contain several ages).
        moves.retain(|mv| {
            let mut pretend = board.clone();
            pretend.remove_piece(piece.sq);
            pretend.remove_piece(mv.sq);
            if let Some(square) = mv.en_passant() {
                pretend.remove_piece(square);
            }
            if let Some(castling) = mv.castling() {
                let rook_x = match castling {
                    Castling::Long => 0,
                    Castling::Short => 7,
                };
                if let Some(rook) = pretend.get_piece(rook_x, piece.sq.1).copied() {
                    pretend.remove_piece(rook.sq);
                    pretend.add_piece(rook.apply_mv(*mv));
                }
            }
            pretend.add_piece(piece.apply_mv(*mv));
            pretend
                .pieces
                .iter()
                .filter(|p| p.color == piece.color && matches!(p.kind, PieceKind::K { .. }))
                .all(|king| !pretend.is_attacked(piece.color.opposite(), king.sq))
        });
        Ok(moves)
    }
}
