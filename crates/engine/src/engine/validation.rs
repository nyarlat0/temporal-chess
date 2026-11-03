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
            let ShortPieceId { wid, age } =
                *self.projection.pieces.get(&mv.source()).ok_or_else(|| {
                    EngineErr::InvalidHistory("recorded move source is missing")
                        .during(format!("validating {mv:?}"))
                })?;
            if self.history.worldlines[wid].is_borrowed(age) {
                continue;
            }
            if self.history.worldlines[wid][age].alive
                && !self
                    .recorded_move_candidates(wid, age, mv)
                    .map_err(|e| e.during(format!("validating {mv:?}")))?
                    .contains(&mv)
            {
                self.undo_mv(id)
                    .map_err(|e| e.during(format!("cancelling {mv:?}")))?;
                return Ok(ValidRes::HistChanged);
            }
        }
        Ok(ValidRes::Unchanged)
    }

    /// Restore one dead suffix whose original collision has disappeared.
    /// Later collisions will be resolved on the next pass, from fresh boards.
    fn resurrect(&mut self) -> Result<ValidRes, EngineErr> {
        let barrier = self.calculation_barrier();
        for wid in 0..self.history.worldlines.len() {
            let Some(age) = self.revival_age(wid) else {
                continue;
            };
            let line = &mut self.history.worldlines[wid];
            if barrier.is_some_and(|limit| line[age].t > limit) {
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

    /// Shared by reconciliation and causal lookahead, including en passant's
    /// off-square capture cause. An empty cell alone does not imply resurrection.
    pub(super) fn revival_age(&self, wid: usize) -> Option<usize> {
        let line = self.history.worldlines.get(wid)?;
        let age = line.iter().position(|s| !s.alive)?;
        let state = &line[age];
        if line.is_borrowed(age)
            || self.board(state.t).is_none()
            || !self.preboard(state.t)?.space[state.x][state.y].is_empty()
        {
            return None;
        }
        let en_passant = self.history.moves.values().any(|mv| {
            mv.en_passant() == Some(Sq(state.x, state.y))
                && mv.direction.step(mv.t) == Some(state.t)
                && self
                    .piece_state(mv.source())
                    .is_some_and(|source| source.alive && source.color != state.color)
        });
        (!en_passant).then_some(age)
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
            if let Some(limit) = self.calculation_barrier() {
                // Publish only conflict-free snapshots. Do not let an inverted
                // continuation resolve collisions or cancel moves at earlier t
                // before the player has accepted this checkpoint.
                self.projection.boards = self
                    .timeline_times()
                    .filter(|&t| t <= limit)
                    .filter_map(|t| self.make_board(t).ok().map(|board| (t, board)))
                    .collect();
                return Ok(());
            }
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
