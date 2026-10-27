use crate::pieces::Sq;

use super::{Engine, EngineErr, ShortPieceId, ValidRes};

impl Engine {
    /// Cancel one invalid move in chronological/causal order, then rebuild.
    fn validate_moves(&mut self) -> Result<ValidRes, EngineErr> {
        let barrier = self.calculation_barrier();
        for (id, mv) in self.history.moves.clone() {
            if barrier.is_some_and(|limit| mv.t > limit) {
                continue;
            }
            // A forward move is revalidated only after its actual destination
            // has been resolved. An unresolved preboard is not an absent board.
            if self.board(mv.t).is_none()
                || mv
                    .direction
                    .step(mv.t)
                    .is_some_and(|t| self.preboard(t).is_some() && self.board(t).is_none())
            {
                continue;
            }
            let ShortPieceId { wid, age } = *self
                .projection
                .pieces
                .get(&mv.source())
                .ok_or(EngineErr::WrongPiece)?;
            if self.history.worldlines[wid].is_borrowed(age) {
                continue;
            }
            if self.history.worldlines[wid][age].alive
                && !self.candidate_moves(wid, age)?.contains(&mv)
            {
                self.undo_mv(id)?;
                return Ok(ValidRes::HistChanged);
            }
        }
        Ok(ValidRes::Unchanged)
    }

    /// Restore one dead suffix whose original collision has disappeared.
    /// Later collisions will be resolved on the next pass, from fresh boards.
    fn resurrect(&mut self) -> Result<ValidRes, EngineErr> {
        let barrier = self.calculation_barrier();
        for line in &mut self.history.worldlines {
            let Some(age) = line.iter().position(|state| !state.alive) else {
                continue;
            };
            if line.is_borrowed(age) {
                continue;
            }
            let state = &line[age];
            if barrier.is_some_and(|limit| state.t > limit) {
                continue;
            }
            if !self.projection.boards.contains_key(&state.t) {
                continue;
            }
            let board = self
                .projection
                .preboards
                .get(&state.t)
                .ok_or(EngineErr::AbsentBoard)?;
            if !board.space[state.x][state.y].is_empty() {
                continue;
            }
            // En passant leaves the victim's square empty, so retain its recorded cause.
            let en_passant = self.history.moves.values().any(|mv| {
                let arrival = mv.direction.step(mv.t);
                mv.en_passant() == Some(Sq(state.x, state.y))
                    && arrival == Some(state.t)
                    && self.projection.pieces.get(&mv.source()).is_some_and(|id| {
                        // Worldlines are mutably borrowed above; the alive mover's
                        // source is also available in the preboard projection.
                        self.projection
                            .preboards
                            .get(&mv.t)
                            .and_then(|pb| pb.find_piv(id.wid, id.age))
                            .is_some_and(|piece| piece.color != state.color)
                    })
            });
            if en_passant {
                continue;
            }
            let end = line.segment_end(age);
            for state in &mut line[age..end] {
                state.alive = true;
            }
            return Ok(ValidRes::HistChanged);
        }
        Ok(ValidRes::Unchanged)
    }

    /// Reach a fixed point of collision resolution, move validation and revival.
    /// Cycle detection uses the source state, including dead pieces and move records.
    pub(super) fn reconcile(&mut self) -> Result<(), EngineErr> {
        let mut seen = Vec::new();
        loop {
            if seen.contains(&self.history) {
                return Err(EngineErr::InfLoop);
            }
            seen.push(self.history.clone());
            self.update_bufs()?;
            if self.reconcile_boards()? == ValidRes::HistChanged {
                continue;
            }

            self.update_infos()?;
            // Only reaction flags changed. Reproject them for all board times.
            self.update_bufs()?;
            if self.build_boards(self.history.horizon)? == ValidRes::HistChanged {
                continue;
            }
            return Ok(());
        }
    }

    fn reconcile_boards(&mut self) -> Result<ValidRes, EngineErr> {
        // Advance the validation frontier chronologically. Building every board
        // first would resolve future collisions before discovering an earlier
        // cancelled move, and destroy exactly the preboards the UI must expose.
        let times = if self.history.calculation.interactive {
            self.timeline_times().collect::<Vec<_>>()
        } else {
            vec![self.history.horizon]
        };
        for through in times {
            if self.calculation_barrier().is_some_and(|t| through > t) {
                break;
            }
            if self.build_boards(through)? == ValidRes::HistChanged
                || self.validate_moves()? == ValidRes::HistChanged
                || self.resurrect()? == ValidRes::HistChanged
            {
                return Ok(ValidRes::HistChanged);
            }
        }
        Ok(ValidRes::Unchanged)
    }
}
