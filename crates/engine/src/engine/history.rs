use crate::pieces::PieceView;

use super::{Engine, EngineErr, MoveId};

impl Engine {
    pub(super) fn kill_piece(&mut self, wid: usize, age: usize) -> Result<(), EngineErr> {
        let wl = self
            .history
            .worldlines
            .get_mut(wid)
            .ok_or(EngineErr::WrongPiece)?;

        let end = wl.segment_end(age);
        for st in wl.get_mut(age..end).ok_or(EngineErr::WrongPiece)? {
            st.alive = false;
        }

        Ok(())
    }

    pub(super) fn undo_last_mv(&mut self, piv: PieceView) -> Result<(), EngineErr> {
        let wl = self
            .history
            .worldlines
            .get(piv.origin)
            .ok_or(EngineErr::WrongPiece)?;

        let start = (0..piv.age)
            .rfind(|&age| wl.gap_after(age).is_some())
            .map_or(0, |age| age + 1);
        let id = wl
            .get(start..piv.age)
            .ok_or(EngineErr::WrongPiece)?
            .iter()
            .rev()
            .map(MoveId::from_state)
            .find(|id| {
                self.history
                    .moves
                    .get(id)
                    .is_some_and(|mv| mv.pid.wid == piv.origin)
            })
            .ok_or(EngineErr::WrongMove)?;
        self.undo_mv(id)
    }

    /// Clears move from move history,
    /// undoes position change of the piece
    /// and grants a reaction at its source. Rebuilds lookup indexes afterwards.
    pub(super) fn undo_mv(&mut self, mv_id: MoveId) -> Result<(), EngineErr> {
        let before = self.history.worldlines.clone();
        let source = self
            .history
            .moves
            .get(&mv_id)
            .ok_or(EngineErr::WrongMove)?
            .source();
        self.undo_mv_inner(mv_id)?;
        let lines = before
            .into_iter()
            .zip(&self.history.worldlines)
            .enumerate()
            .filter(|(_, (before, after))| before != *after)
            .map(|(wid, (before, after))| (wid, before, after.clone()))
            .collect();
        self.history
            .calculation
            .cancellations
            .retain(|c| c.source != source);
        self.history
            .calculation
            .cancellations
            .push(super::types::Cancellation { source, lines });
        Ok(())
    }

    fn undo_mv_inner(&mut self, mv_id: MoveId) -> Result<(), EngineErr> {
        // Remove old move
        let mv = self
            .history
            .moves
            .remove(&mv_id)
            .ok_or(EngineErr::WrongMove)?;
        self.history.targets.remove(&mv_id);

        // Piv before move, undid states will be set to it
        let bf_piv = self.get_piv_from_mv(mv).ok_or(EngineErr::WrongPiece)?;
        self.undo_castling_rook(mv, bf_piv)?;
        if mv.to_past() != bf_piv.inverted {
            if mv.to_past() {
                self.remove_past_route(bf_piv)?;
            } else {
                self.remove_return_route(bf_piv)?;
            }
            let horizon = self
                .projection
                .preboards
                .last_key_value()
                .map(|(&t, _)| t)
                .unwrap_or(bf_piv.t);
            let wait = super::Move {
                sq: bf_piv.sq,
                direction: super::TimeDirection::from_inverted(bf_piv.inverted),
                effect: super::MoveEffect::Normal,
                ..mv
            };
            // Keep the endpoint on its earliest own-color board so the
            // player can still react and turn it back towards the future.
            if !(bf_piv.inverted && bf_piv.t <= bf_piv.color.first_board()) {
                self.apply_route(bf_piv, wait, horizon)?;
                let source = self.history.worldlines[bf_piv.origin][bf_piv.age].clone();
                self.relocate_from(bf_piv.origin, bf_piv.age + 1, &source);
            }
            self.history.worldlines[bf_piv.origin][bf_piv.age]
                .rights
                .cancelled_move = true;
            self.history.worldlines[bf_piv.origin][bf_piv.age]
                .rights
                .new_information = false;
            if bf_piv.inverted {
                self.extend_cancelled_return(bf_piv)?;
            }
            self.update_bufs()?;
            return Ok(());
        }

        // Piv just after undid move,
        // is used to know what posion to undo
        let ud_piv = self
            .get_piv_necro(bf_piv.origin, bf_piv.age + 1)
            .ok_or(EngineErr::WrongPiece)?;

        let wl = self
            .history
            .worldlines
            .get_mut(bf_piv.origin)
            .ok_or(EngineErr::WrongPiece)?;

        // Cancellation grants a reaction without granting an ordinary move.
        {
            let mv_st = wl.get_mut(bf_piv.age).ok_or(EngineErr::WrongPiece)?;
            mv_st.rights.cancelled_move = true;
            mv_st.rights.new_information = false;
        }

        let end = wl.segment_end(bf_piv.age);
        for st in wl[bf_piv.age + 1..end].iter_mut() {
            if (st.x, st.y) == (ud_piv.sq.0, ud_piv.sq.1) {
                st.x = bf_piv.sq.0;
                st.y = bf_piv.sq.1;
                st.kind = bf_piv.kind;
            } else {
                break;
            }
        }

        self.update_bufs()?;

        Ok(())
    }

    /// Automatic restoration is not a player-issued wait. Reconciliation still
    /// resolves collisions along this path; only its endpoint gets a new turn.
    fn extend_cancelled_return(&mut self, piece: PieceView) -> Result<(), EngineErr> {
        let boundary = piece.color.first_board();
        let mut age = piece.age;
        loop {
            let line = &mut self.history.worldlines[piece.origin];
            let state = &mut line[age];
            state.rights.ordinary = state.alive && state.t == boundary;
            if state.t == boundary {
                break;
            }
            let next = state.prolongate().ok_or(EngineErr::WrongMove)?;
            if line.gap_after(age).is_none()
                && line
                    .get(age + 1)
                    .is_some_and(|existing| existing.inverted && existing.t == next.t)
            {
                age += 1;
            } else {
                age = self.advance_route(piece.origin, age, next)?.0;
            }
        }
        Ok(())
    }

    pub(super) fn undo_castling_rook(
        &mut self,
        mv: super::Move,
        king: PieceView,
    ) -> Result<(), EngineErr> {
        if let Some(rook) = self.castling_rook(mv, king)? {
            let line = &self.history.worldlines[rook.origin];
            if line.get(rook.age + 1).is_none() {
                return Err(EngineErr::WrongPiece);
            }
            let source = line[rook.age].clone();
            self.relocate_from(rook.origin, rook.age + 1, &source);
        }
        Ok(())
    }
}
