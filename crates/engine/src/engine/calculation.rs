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
        self.history
            .calculation
            .cancellations
            .iter()
            .filter_map(|cancellation| {
                let id = cancellation.source;
                let state = self.piece_state(id)?;
                (state.alive
                    && state.color == self.turn
                    && Color::from_board(state.t) == self.turn
                    && state.rights.cancelled_move
                    && !self.history.calculation.acknowledged.contains(&id)
                    && self.cancellation_has_consequences(cancellation))
                .then_some(state.t)
            })
            .min()
    }

    /// Compare immediate effects in identical surroundings, changing only the
    /// physical states introduced by this cancellation. Do not reconcile either
    /// alternative: unrelated pending edits must not become this piece's effects.
    fn cancellation_has_consequences(&self, cancellation: &super::types::Cancellation) -> bool {
        let mut before = self.clone();
        for (wid, old, cancelled) in &cancellation.lines {
            let line = &mut before.history.worldlines[*wid];
            for after in cancelled.iter() {
                let key = super::MoveId::from_state(after);
                let previous = old.iter().find(|s| super::MoveId::from_state(s) == key);
                if previous.is_some_and(|s| same_physical_state(s, after)) {
                    continue;
                }
                if let Some(age) = line.locate(key)
                    && same_physical_state(&line[age], after)
                {
                    if let Some(previous) = previous {
                        // Keep current observations/rights in both alternatives.
                        let rights = line[age].rights;
                        let informed = line[age].informed.clone();
                        line[age] = previous.clone();
                        line[age].rights = rights;
                        line[age].informed = informed;
                    } else {
                        line.states.remove(age);
                    }
                }
            }
            // Restore removed occurrences in their original causal order.
            for (age, previous) in old.iter().enumerate().rev() {
                let key = super::MoveId::from_state(previous);
                if cancelled.locate(key).is_none() && line.locate(key).is_none() {
                    let at = old[age + 1..]
                        .iter()
                        .find_map(|s| line.locate(super::MoveId::from_state(s)))
                        .unwrap_or(line.len());
                    line.states.insert(at, previous.clone());
                }
            }
        }
        if before.update_bufs().is_err() {
            return false;
        }
        // A collision is attributable only if this edit introduced or changed
        // one of its participants. An unrelated conflict elsewhere is ignored.
        for board in self.projection.preboards.values() {
            for cell in board.space.iter().flatten().filter(|cell| cell.len() > 1) {
                for piece in cell {
                    let id = Self::piece_at(*piece);
                    if self
                        .piece_state(id)
                        .zip(before.piece_state(id))
                        .is_none_or(|(a, b)| !same_physical_state(a, b))
                    {
                        return true;
                    }
                }
            }
        }
        let mut after = self.clone();
        for engine in [&mut before, &mut after] {
            engine.history.calculation.interactive = false;
            engine.projection.boards = engine
                .timeline_times()
                .filter_map(|t| engine.make_board(t).ok().map(|b| (t, b)))
                .collect();
        }
        // A newly vacated square matters only when it actually removes the
        // death's cause (en passant can keep a piece dead on an empty square).
        for wid in 0..self.history.worldlines.len() {
            if after.revival_age(wid).is_some() && before.revival_age(wid).is_none() {
                return true;
            }
        }
        // Validate each retained command independently. A command already
        // invalid without this cancellation cannot be attributed to it.
        let legal = |engine: &Self, mv: super::Move| -> Option<bool> {
            let id = engine.projection.pieces.get(&mv.source())?;
            let state = &engine.history.worldlines[id.wid][id.age];
            if !state.alive || engine.history.worldlines[id.wid].is_borrowed(id.age) {
                return None;
            }
            engine
                .recorded_move_candidates(id.wid, id.age, mv)
                .ok()
                .map(|moves| moves.contains(&mv))
        };
        for &mv in self.history.moves.values() {
            if legal(&after, mv) == Some(false) && legal(&before, mv) == Some(true) {
                return true;
            }
        }
        // Information includes temporal en passant evidence. Only an actual new
        // reaction absent in the counterfactual is caused by this continuation.
        for side in [Color::White, Color::Black] {
            before.turn = side;
            after.turn = side;
            if before.update_infos().is_err() || after.update_infos().is_err() {
                continue;
            }
        }
        after
            .history
            .worldlines
            .iter()
            .enumerate()
            .any(|(wid, line)| {
                line.iter().any(|state| {
                    let id = PieceAt {
                        piece: PieceId::from_state(state, wid),
                        t: state.t,
                    };
                    state.alive
                        && state.rights.new_information
                        && self
                            .piece_state(id)
                            .is_some_and(|s| !s.rights.new_information)
                        && before
                            .piece_state(id)
                            .is_some_and(|s| !s.rights.new_information)
                })
            })
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

/// Rights and remembered information are observations, not physical edits.
fn same_physical_state(a: &State, b: &State) -> bool {
    a.t == b.t
        && a.x == b.x
        && a.y == b.y
        && a.fut_seen == b.fut_seen
        && a.loop_turn == b.loop_turn
        && a.alive == b.alive
        && a.kind == b.kind
        && a.color == b.color
        && a.inverted == b.inverted
        && a.loop_pivots == b.loop_pivots
}
