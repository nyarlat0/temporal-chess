use super::{Castling, Engine, EngineErr, Move};
use crate::pieces::{Color, PieceView};

impl Engine {
    /// Commit the move, temporal edits, reconciliation and turn change atomically.
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

    pub(super) fn apply_move_inner(&mut self, mv: Move) -> Result<(), EngineErr> {
        let mut piece = self.get_piv_from_mv(mv).ok_or(EngineErr::WrongPiece)?;
        if piece.color != self.turn || !piece.can_move() {
            return Err(EngineErr::WrongTurn);
        }
        if !piece.is_movable() {
            return Err(EngineErr::WrongPiece);
        }
        if !self.possible_moves(piece.origin, piece.age)?.contains(&mv) {
            return Err(EngineErr::WrongMove);
        }
        let checking = self.move_board(piece, mv.direction)?;
        let target_sq = mv.en_passant().unwrap_or(mv.sq);
        let target = checking
            .get_piece(target_sq.0, target_sq.1)
            .copied()
            .filter(|target| target.color != piece.color);
        let rook = self.castling_rook(mv, piece)?;
        if let Some(old) = self.history.moves.get(&mv.id()).copied() {
            if old.pid != mv.pid || (old.castling().is_some() && !mv.to_past()) {
                self.undo_mv(old.id())?;
            } else if old.to_past() && !mv.to_past() {
                self.remove_past_route(piece)?;
            } else if old.castling().is_some() {
                // Preserve the king's future when inverting, but the replaced
                // castling command no longer moves the separate rook worldline.
                self.undo_castling_rook(old, piece)?;
            }
            self.history.moves.remove(&old.id());
            self.history.targets.remove(&old.id());
            piece = self.get_piv_from_mv(mv).ok_or(EngineErr::WrongPiece)?;
        }
        self.deact_same_order(piece);
        self.deact_prev_reactable(piece.color, piece.t);

        let end_t = self
            .projection
            .preboards
            .last_key_value()
            .map(|(&t, _)| t)
            .unwrap_or(0);
        let horizon = if piece.t == end_t {
            self.prolongate_wlines(end_t, mv.to_past().then_some(piece.origin))?;
            end_t.checked_add(1).ok_or(EngineErr::WrongMove)?
        } else {
            end_t
        };
        self.history.horizon = horizon;
        self.apply_route(piece, mv, horizon)?;
        if let Some(rook) = rook {
            // Castling never inserts a backward segment; both source ages remain valid.
            self.apply_route(rook, mv, horizon)?;
        }
        self.update_bufs()?;
        if let Some(target) = target {
            let arrival = mv.direction.step(mv.t).ok_or(EngineErr::WrongMove)?;
            let victim = if target.t == arrival {
                Some(target)
            } else {
                self.history.worldlines[target.origin]
                    .iter()
                    .enumerate()
                    .find(|(_, state)| state.t == arrival && state.loop_turn == target.loop_turn)
                    .map(|(age, state)| PieceView::from_state(state, target.origin, age))
            };
            if let Some(victim) = victim {
                self.history
                    .targets
                    .insert(mv.id(), vec![Self::piece_at(victim)]);
                if mv.en_passant().is_some() {
                    self.kill_piece(victim.origin, victim.age)?;
                }
            }
        }
        self.history.moves.insert(mv.id(), mv);
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
        for state in self
            .history
            .worldlines
            .iter_mut()
            .flat_map(|line| line.iter_mut())
        {
            if state.color == side && state.t <= until_t {
                state.expire_reactions();
            }
        }
    }
}
