use super::{Engine, EngineErr, MoveId, PieceAt, TimeDirection};

impl Engine {
    /// Structurally valid waiting destinations for UI previews. Just like
    /// possible_moves, final causality checks are performed by the command.
    pub fn possible_skips(&self, source: PieceAt) -> Vec<usize> {
        self.timeline_times()
            .filter(|&t| t != source.t)
            .filter(|&t| {
                let mut candidate = self.clone();
                candidate.skip_inner(source, t).is_ok()
            })
            .collect()
    }
    /// Advance to another existing time in the piece's current orientation.
    /// Does not consume a player's turn or another piece's rights. Staying at
    /// the same time cannot relinquish a right.
    pub fn skip_ordinary(&mut self, source: PieceAt, to_t: usize) -> Result<(), EngineErr> {
        self.transaction(|candidate| {
            candidate.skip_inner(source, to_t)?;
            candidate.reconcile()
        })
    }

    pub(super) fn skip_inner(&mut self, source: PieceAt, to_t: usize) -> Result<(), EngineErr> {
        let location = *self
            .projection
            .pieces
            .get(&source)
            .ok_or(EngineErr::WrongPiece)?;
        let piece = self
            .get_piv(location.wid, location.age)
            .ok_or(EngineErr::WrongPiece)?;
        if self.board(piece.t).is_none() {
            return Err(EngineErr::AbsentBoard);
        }
        if piece.color != self.turn || !piece.can_move() {
            return Err(EngineErr::WrongTurn);
        }
        if !piece.rights.ordinary {
            return Err(EngineErr::WrongPiece);
        }
        if to_t == piece.t
            || to_t > self.history.horizon
            || (piece.inverted && to_t > piece.t)
            || (!piece.inverted && to_t < piece.t)
        {
            return Err(EngineErr::WrongMove);
        }
        let wid = piece.origin;
        let mut age = piece.age;
        self.history.worldlines[wid][age].rights.ordinary = false;
        while self.history.worldlines[wid][age].t != to_t {
            let line = &self.history.worldlines[wid];
            let state = &line[age];
            if self
                .history
                .moves
                .get(&MoveId::from_state(state))
                .is_some_and(|mv| mv.pid.wid == wid)
            {
                // Waiting cannot overwrite an already recorded spatial move.
                return Err(EngineErr::WrongMove);
            }
            let next = state
                .step(TimeDirection::from_inverted(piece.inverted))
                .ok_or(EngineErr::WrongMove)?;
            if line.gap_after(age).is_none()
                && let Some(existing) = line.get(age + 1)
            {
                if MoveId::from_state(existing) != MoveId::from_state(&next)
                    || existing.inverted != next.inverted
                    || existing.x != next.x
                    || existing.y != next.y
                    || existing.kind != next.kind
                    || !existing.alive
                {
                    return Err(EngineErr::WrongMove);
                }
                age += 1;
            } else {
                age = self.advance_route(wid, age, next)?.0;
            }
            if self.history.worldlines[wid][age].t != to_t {
                self.history.worldlines[wid][age].rights.ordinary = false;
            }
        }
        // Existing arrival rights are retained, never re-granted. Newly inserted
        // states get parity-correct rights from State::step, without set_moved.
        self.update_bufs()?;
        Ok(())
    }
}
