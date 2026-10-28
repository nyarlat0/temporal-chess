use crate::model::{Color, PieceAt, PieceId, State};

use super::{CalculationStatus, Engine, EngineErr, GameOutcome, GameStatus, WinReason};

impl Engine {
    /// Opt into incremental reconciliation. The default engine remains eager for
    /// analysis/backwards compatibility. This setting is included in History.
    pub fn with_interactive_calculation(mut self) -> Result<Self, EngineErr> {
        self.history.calculation.interactive = true;
        self.history.calculation.acknowledged.clear();
        self.reconcile()?;
        Ok(self)
    }

    pub fn calculation_status(&self) -> CalculationStatus {
        match self.calculation_barrier() {
            Some(t) => CalculationStatus::Paused { t, side: self.turn },
            None => CalculationStatus::Complete,
        }
    }

    pub(super) fn calculation_barrier(&self) -> Option<usize> {
        if !self.history.calculation.interactive || self.outcome().is_some() {
            return None;
        }
        let mut barrier: Option<usize> = None;
        for (wid, line) in self.history.worldlines.iter().enumerate() {
            for (age, state) in line.iter().enumerate() {
                if !state.alive
                    || state.color != self.turn
                    || Color::from_board(state.t) != self.turn
                    || !state.rights.cancelled_move
                {
                    continue;
                }
                let id = PieceAt {
                    piece: PieceId::from_state(state, wid),
                    t: state.t,
                };
                if self.history.calculation.acknowledged.contains(&id) {
                    continue;
                }
                // A cancelled move is only a checkpoint when its affected
                // causal continuation overlaps another live occurrence later
                // in the board ribbon. Do not inspect the borrowed future on
                // the far side of a causal gap: cancellation cannot change it.
                let conflicts = line[age + 1..line.segment_end(age)].iter().any(|next| {
                    next.alive
                        && next.t > state.t
                        && self
                            .projection
                            .preboards
                            .get(&next.t)
                            .is_some_and(|board| board.space[next.x][next.y].len() > 1)
                });
                if conflicts {
                    barrier = Some(barrier.map_or(state.t, |t| t.min(state.t)));
                }
            }
        }
        barrier
    }

    /// Accept the current conflicting continuation, stopping at the next one.
    /// Does not consume rights or a player's turn. Any deferred paradox is
    /// charged to the author of the history-changing command, not the inspector.
    pub fn continue_calculation(&mut self) -> Result<CalculationStatus, EngineErr> {
        if self.outcome().is_some() {
            return Err(EngineErr::GameOver);
        }
        let Some(t) = self.calculation_barrier() else {
            return Ok(CalculationStatus::Complete);
        };
        let mut candidate = self.clone();
        for (wid, line) in self.history.worldlines.iter().enumerate() {
            for state in line
                .iter()
                .filter(|s| s.t == t && s.alive && s.color == self.turn && s.rights.cancelled_move)
            {
                candidate.history.calculation.acknowledged.push(PieceAt {
                    piece: PieceId::from_state(state, wid),
                    t,
                });
            }
        }
        let author = self.history.calculation.responsible.unwrap_or(self.turn);
        match candidate
            .reconcile()
            .and_then(|()| candidate.finish_command(author))
        {
            Ok(()) => {
                *self = candidate;
            }
            Err(EngineErr::InfLoop) => {
                self.history.status = GameStatus::Finished(GameOutcome::Victory {
                    winner: author.opposite(),
                    reason: WinReason::Paradox,
                });
            }
            Err(error) => return Err(error),
        }
        Ok(self.calculation_status())
    }

    /// All actual times, including not-yet-reconciled preboards.
    pub fn timeline_times(&self) -> impl DoubleEndedIterator<Item = usize> + '_ {
        self.projection.preboards.keys().copied()
    }

    /// Stable identity lookup for inspectors (including dead occurrences).
    pub fn piece_state(&self, id: PieceAt) -> Option<&State> {
        let location = self.projection.pieces.get(&id)?;
        self.history.worldlines.get(location.wid)?.get(location.age)
    }
}
