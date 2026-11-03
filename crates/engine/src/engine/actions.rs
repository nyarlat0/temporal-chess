use super::{Castling, Engine, EngineErr, Move};
use crate::pieces::{Color, PieceView};

impl Engine {
    /// Reactions are optional, but ordinary reserves cannot be passed over.
    pub fn can_decline_reactions(&self) -> bool {
        self.outcome().is_none()
            && self.move_sources().any(|p| p.rights.is_reaction())
            && !self
                .history
                .worldlines
                .iter()
                .flat_map(|line| line.iter())
                .any(|s| {
                    s.alive
                        && s.color == self.turn
                        && Color::from_board(s.t) == self.turn
                        && s.rights.ordinary
                })
    }

    /// Accept the current continuations, retire all own reactions and hand over.
    /// This is a journaled turn action; it never creates a spatial move or board.
    pub fn decline_reactions(&mut self) -> Result<(), EngineErr> {
        self.transaction(|candidate| {
            if !candidate.can_decline_reactions() {
                return Err(EngineErr::WrongMove);
            }
            candidate.update_infos()?;
            for state in candidate
                .history
                .worldlines
                .iter_mut()
                .flat_map(|line| line.iter_mut())
            {
                if state.color == candidate.turn {
                    state.expire_reactions();
                }
            }
            candidate.turn = candidate.turn.opposite();
            candidate.reconcile()
        })
    }

    /// Commit the move, temporal edits, reconciliation and turn change atomically.
    pub fn apply_new_mv(&mut self, mv: Move) -> Result<(), EngineErr> {
        if self.outcome().is_some() {
            return Err(EngineErr::GameOver);
        }
        let piece = self.move_source(mv)?;
        // Admission uses the very same list as highlighting, before any edits
        // (including resetting calculation acknowledgements). Reconciliation
        // may cancel this move later; that cannot retroactively reject it.
        if !self.possible_moves(piece.origin, piece.age)?.contains(&mv) {
            return Err(EngineErr::WrongMove);
        }
        self.transaction(|candidate| {
            candidate
                .apply_admitted_move(mv, piece)
                .map_err(|e| e.during(format!("applying {mv:?}")))?;
            candidate.turn = candidate.turn.opposite();
            // A legal command may rewrite its own cause out of history. Commit
            // the resulting history even when validation cancels this move;
            // GameRecord independently retains the accepted player command.
            candidate
                .reconcile()
                .map_err(|e| e.during(format!("reconciling after {mv:?}")))
        })
    }

    fn move_source(&self, mv: Move) -> Result<PieceView, EngineErr> {
        let piece = self.get_piv_from_mv(mv).ok_or(EngineErr::WrongPiece)?;
        if self.board(piece.t).is_none() {
            return Err(EngineErr::AbsentBoard);
        }
        if piece.color != self.turn || !piece.can_move() {
            return Err(EngineErr::WrongTurn);
        }
        if !piece.is_movable() {
            return Err(EngineErr::WrongPiece);
        }
        Ok(piece)
    }

    /// Internal probes check spatial rules without recursively probing inverse
    /// safety. Public commands are admitted through `possible_moves` above.
    pub(super) fn apply_move_inner(&mut self, mv: Move) -> Result<(), EngineErr> {
        let piece = self.move_source(mv)?;
        if !self.candidate_moves(piece.origin, piece.age)?.contains(&mv) {
            return Err(EngineErr::WrongMove);
        }
        self.apply_admitted_move(mv, piece)
    }

    fn apply_admitted_move(&mut self, mv: Move, mut piece: PieceView) -> Result<(), EngineErr> {
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
        let idle_endpoints = self.deact_same_order(piece);
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
        for idle in idle_endpoints {
            self.continue_idle_endpoint(idle, horizon)?;
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

    fn deact_same_order(&mut self, piece: PieceView) -> Vec<PieceView> {
        let mut idle_endpoints = Vec::new();
        for (origin, line) in self.history.worldlines.iter_mut().enumerate() {
            let last_age = line.len() - 1;
            for (age, state) in line.iter_mut().enumerate() {
                if state.t == piece.t && PieceView::from_state(state, origin, age).same_order(piece)
                {
                    if origin != piece.origin
                        && age == last_age
                        && state.alive
                        && state.rights.ordinary
                    {
                        // A cancelled turn can leave a deliberate active
                        // endpoint. Once another piece takes its shared turn,
                        // it must wait, not become an inactive stranded end.
                        if state.inverted && state.t <= state.color.first_board() {
                            // There is no earlier own board to wait on. Keep
                            // the boundary reserve available for a future turn.
                            state.expire_reactions();
                            continue;
                        }
                        idle_endpoints.push(PieceView::from_state(state, origin, age));
                    }
                    state.make_unmovable();
                }
            }
        }
        idle_endpoints
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
