use crate::pieces::Sq;

use super::{Engine, EngineErr, ShortPieceId, ValidRes};

impl Engine {
    /// Cancel one invalid move in chronological/causal order, then rebuild.
    fn validate_moves(&mut self) -> Result<ValidRes, EngineErr> {
        for (id, mv) in self.history.moves.clone() {
            let ShortPieceId { wid, age } = *self
                .projection
                .pieces
                .get(&mv.source())
                .ok_or(EngineErr::WrongPiece)?;
            if self.history.worldlines[wid][age].alive
                && !self.possible_moves(wid, age)?.contains(&mv)
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
        for line in &mut self.history.worldlines {
            let Some(age) = line.iter().position(|state| !state.alive) else {
                continue;
            };
            let state = &line[age];
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
            for state in &mut line[age..] {
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
            if self.build_boards()? == ValidRes::HistChanged {
                continue;
            }
            if self.validate_moves()? == ValidRes::HistChanged {
                continue;
            }
            if self.resurrect()? == ValidRes::HistChanged {
                continue;
            }

            self.update_infos();
            // Only reaction flags changed. Reproject them for all board times.
            self.update_bufs()?;
            if self.build_boards()? == ValidRes::HistChanged {
                continue;
            }
            return Ok(());
        }
    }
}
