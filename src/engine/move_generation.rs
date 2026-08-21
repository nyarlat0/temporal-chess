use crate::pieces::{Color, PieceKind};

use super::{Engine, EngineErr, Move, PieceId};

impl Engine {
    /// Returns error if piece is dead!!!
    pub fn possible_moves(&self, wid: usize, age: usize) -> Result<Vec<Move>, EngineErr> {
        let piv = self.get_piv(wid, age).ok_or(EngineErr::WrongPiece)?;
        let pid = PieceId::from_piv(piv);
        let t = piv.t;
        let pres_board = self.boards.get(&piv.t).ok_or(EngineErr::AbsentBoard)?;
        let mut mvs = Vec::new();

        // Basic moves to future and to past
        let basic_sqs = piv.basic_moves(pres_board);
        for sq in basic_sqs {
            mvs.push(Move {
                pid,
                t,
                sq,
                to_past: false,
                castling: None,
                en_passant: None,
                promotion: None,
            });

            if piv.t > 1 {
                mvs.push(Move {
                    pid,
                    t,
                    sq,
                    to_past: true,
                    castling: None,
                    en_passant: None,
                    promotion: None,
                });
            }
        }

        // Castiling moves if any
        let cast_mvs = piv.castling_moves(pres_board);
        for (sq, cast) in cast_mvs {
            mvs.push(Move {
                pid,
                t,
                sq,
                to_past: false,
                castling: Some(cast),
                en_passant: None,
                promotion: None,
            });
        }

        // En passant moves to future
        if piv.t > 0 {
            let past_pboard = self
                .preboards
                .get(&piv.t.saturating_sub(1))
                .ok_or(EngineErr::AbsentBoard)?;
            let enpass_sqs = piv.en_passant_moves(pres_board, past_pboard);

            for (sq, eat_sq) in enpass_sqs {
                mvs.push(Move {
                    pid,
                    t,
                    sq,
                    to_past: false,
                    castling: None,
                    en_passant: Some(eat_sq),
                    promotion: None,
                });
            }
        }

        // En passant moves to past
        if piv.t > 1
            && let Some(fut_pboard) = self.preboards.get(&(piv.t + 1))
        {
            let enpass_sqs = piv.en_passant_moves(pres_board, fut_pboard);

            for (sq, eat_sq) in enpass_sqs {
                mvs.push(Move {
                    pid,
                    t,
                    sq,
                    to_past: true,
                    castling: None,
                    en_passant: Some(eat_sq),
                    promotion: None,
                });
            }
        }

        // Check for promotion if pawn
        if matches!(piv.kind, PieceKind::P { .. }) {
            let mut prom_mvs = Vec::new();
            for mv in mvs.iter() {
                let last_y = match piv.color {
                    Color::White => 7,
                    Color::Black => 0,
                };
                if mv.sq.1 == last_y {
                    let mut new_mv = *mv;
                    use PieceKind::*;
                    for kind in [N, B, R { moved: true }, Q] {
                        new_mv.promotion = Some(kind);
                        prom_mvs.push(new_mv);
                    }
                }
            }
            mvs.append(&mut prom_mvs);
        }

        // If there is an attack on king in present,
        // then leave only moves that protect king
        if let Some(king_piv) = pres_board
            .pieces
            .iter()
            .find(|p| p.color == piv.color && matches!(p.kind, PieceKind::K { .. }))
        {
            let mut pretend_board = pres_board.clone();
            pretend_board.remove_piece(piv.sq);

            // If on board without this piv
            // king isnt attacked, then it wont be no matter the move,
            // unless piece is a pawn that does en passant
            if pretend_board.is_attacked(piv.color.opposite(), king_piv.sq)
                || matches!(piv.kind, PieceKind::P { .. })
            {
                let mut rm_ids = Vec::new();

                for (mv_id, mv) in mvs.iter().enumerate() {
                    // Create new temporary pretend board
                    // for killing and moving the pieces
                    let mut temp_pret_board = pretend_board.clone();

                    // Remove eaten piece
                    if temp_pret_board.is_enemy(piv.color, mv.sq) {
                        temp_pret_board.remove_piece(mv.sq);
                    }
                    if let Some(eating_sq) = mv.en_passant {
                        temp_pret_board.remove_piece(eating_sq);
                    }

                    // Move pretend piv and add it to board
                    let mut pretend_piv = piv;
                    pretend_piv.sq = mv.sq;
                    temp_pret_board.add_piece(pretend_piv);

                    if temp_pret_board.is_attacked(piv.color.opposite(), king_piv.sq) {
                        rm_ids.push(mv_id);
                    }
                }

                let mut i = 0;
                mvs.retain(|_| {
                    let to_keep = !rm_ids.contains(&i);
                    i += 1;

                    to_keep
                });
            }
        }

        Ok(mvs)
    }
}
