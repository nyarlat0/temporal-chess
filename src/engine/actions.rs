use crate::pieces::{Color, PieceView, Sq};

use super::{Castling, Engine, EngineErr, Move};

impl Engine {
    /// Commit a complete turn, including reconciliation. On error nothing changes.
    pub fn apply_new_mv(&mut self, mv: Move) -> Result<(), EngineErr> {
        self.transaction(|candidate| {
            candidate.apply_move_inner(mv)?;
            candidate.turn = candidate.turn.opposite();
            candidate.reconcile()?;
            if candidate.history.moves.get(&mv.id()) != Some(&mv) {
                return Err(EngineErr::WrongMove);
            }
            Ok(())
        })
    }

    fn apply_move_inner(&mut self, mv: Move) -> Result<(), EngineErr> {
        let piece = self.get_piv_from_mv(mv).ok_or(EngineErr::WrongPiece)?;
        if piece.color != self.turn || !piece.can_move() {
            return Err(EngineErr::WrongTurn);
        }
        if !piece.is_movable() {
            return Err(EngineErr::WrongPiece);
        }
        if !self.possible_moves(piece.origin, piece.age)?.contains(&mv) {
            return Err(EngineErr::WrongMove);
        }
        // The original prototype generated these moves but never built the loop.
        // Keep the unsupported transition explicit until loop construction is defined.
        if mv.to_past() != piece.inverted {
            return Err(EngineErr::UnsupportedMove(
                "changing time direction requires loop construction",
            ));
        }
        let rook = self.castling_rook(mv, piece)?;

        if self.history.moves.contains_key(&mv.id()) {
            self.undo_mv(mv.id())?;
        }
        self.deact_same_order(piece);
        self.deact_prev_reactable(piece.color, piece.t);

        let end_t = self
            .projection
            .preboards
            .last_key_value()
            .map(|(&t, _)| t)
            .unwrap_or(0);
        if piece.t == end_t {
            self.prolongate_wlines(end_t)?;
        }
        // An inverted endpoint may advance into an already existing board.
        let line = &mut self.history.worldlines[piece.origin];
        if line.len() == piece.age + 1 {
            line.push(
                line.last()
                    .ok_or(EngineErr::WrongPiece)?
                    .prolongate()
                    .ok_or(EngineErr::WrongMove)?,
            );
        }
        self.fix_position(mv)?;
        if let Some(rook) = rook {
            self.relocate_after(rook, mv)?;
        }
        if let Some(square) = mv.en_passant() {
            let victim = self
                .projection
                .preboards
                .get(&piece.t)
                .and_then(|board| board.space[square.0][square.1].first())
                .copied()
                .ok_or(EngineErr::WrongPiece)?;
            self.kill_piece(victim.origin, victim.age + 1)?;
        }
        self.history.moves.insert(mv.id(), mv);
        Ok(())
    }

    pub(super) fn fix_position(&mut self, mv: Move) -> Result<(), EngineErr> {
        let piece = self.get_piv_from_mv(mv).ok_or(EngineErr::WrongPiece)?;
        self.relocate_after(piece, mv)
    }

    fn relocate_after(&mut self, piece: PieceView, mv: Move) -> Result<(), EngineErr> {
        let line = self
            .history
            .worldlines
            .get_mut(piece.origin)
            .ok_or(EngineErr::WrongPiece)?;
        let next = line.get(piece.age + 1).ok_or(EngineErr::WrongPiece)?;
        let old_square = Sq(next.x, next.y);
        for state in line.iter_mut().skip(piece.age + 1) {
            if Sq(state.x, state.y) != old_square {
                break;
            }
            state.apply_mv(mv);
        }
        Ok(())
    }

    pub(super) fn castling_rook(
        &self,
        mv: Move,
        king: PieceView,
    ) -> Result<Option<PieceView>, EngineErr> {
        let Some(castling) = mv.castling() else {
            return Ok(None);
        };
        let x = match castling {
            Castling::Long => 0,
            Castling::Short => 7,
        };
        let y = match king.color {
            Color::White => 0,
            Color::Black => 7,
        };
        self.projection
            .preboards
            .get(&mv.t)
            .and_then(|board| {
                board.space[x][y].iter().find(|piece| {
                    piece.color == king.color
                        && matches!(piece.kind, crate::pieces::PieceKind::R { .. })
                })
            })
            .copied()
            .map(Some)
            .ok_or(EngineErr::WrongPiece)
    }

    fn prolongate_wlines(&mut self, end_t: usize) -> Result<(), EngineErr> {
        for line in &mut self.history.worldlines {
            let last = line.last().ok_or(EngineErr::WrongPiece)?;
            if last.t == end_t && !last.inverted {
                line.push(last.prolongate().ok_or(EngineErr::WrongMove)?);
            }
        }
        Ok(())
    }

    fn deact_same_order(&mut self, piece: PieceView) {
        for (origin, line) in self.history.worldlines.iter_mut().enumerate() {
            for (age, state) in line.iter_mut().enumerate() {
                if state.t == piece.t && PieceView::from_state(state, origin, age).same_order(piece)
                {
                    state.make_unmovable();
                }
            }
        }
    }

    fn deact_prev_reactable(&mut self, side: Color, until_t: usize) {
        for state in self.history.worldlines.iter_mut().flatten() {
            if state.color == side && state.t <= until_t {
                state.expire_reactions();
            }
        }
    }
}
