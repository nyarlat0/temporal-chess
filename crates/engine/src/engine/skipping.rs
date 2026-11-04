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
        self.transaction(|candidate| candidate.skip_with_information(source, to_t))
    }

    /// Waiting acknowledges the resulting view on the traversed occurrences,
    /// after reconciliation has resolved their actual positions and targets.
    /// Other occurrences and other pieces retain their reaction memory.
    pub(super) fn skip_with_information(
        &mut self,
        source: PieceAt,
        to_t: usize,
    ) -> Result<(), EngineErr> {
        self.skip_inner(source, to_t)?;
        let location = self.projection.pieces[&source];
        let mut traversed = Vec::new();
        for state in &self.history.worldlines[location.wid][location.age..] {
            traversed.push(PieceAt {
                piece: super::PieceId::from_state(state, location.wid),
                t: state.t,
            });
            if state.t == to_t {
                break;
            }
        }
        loop {
            self.reconcile()?;
            let mut learned = false;
            for id in &traversed {
                if let Some(location) = self.projection.pieces.get(id) {
                    let state = &mut self.history.worldlines[location.wid][location.age];
                    let before = state.remembered.len();
                    state.remembered.extend(state.informed.iter().copied());
                    learned |= state.remembered.len() != before;
                }
            }
            // Acknowledging a reaction may release a calculation barrier. If it
            // does, also learn the resulting view, not a stale intermediate one.
            if !learned {
                return Ok(());
            }
        }
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
        if crate::model::Color::from_board(to_t) != piece.color
            || to_t == piece.t
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
            // Only the exact existing causal successor is our own waiting
            // continuation. Other incarnations, even of this worldline, block.
            let successor = (line.gap_after(age).is_none())
                .then(|| line.get(age + 1))
                .flatten()
                .filter(|existing| MoveId::from_state(existing) == MoveId::from_state(&next));
            if self
                .history
                .worldlines
                .iter()
                .enumerate()
                .any(|(other_wid, other)| {
                    other.iter().enumerate().any(|(other_age, occupant)| {
                        occupant.alive
                            && occupant.t == next.t
                            && occupant.x == next.x
                            && occupant.y == next.y
                            && !(other_wid == wid && other_age == age + 1 && successor.is_some())
                    })
                })
            {
                return Err(EngineErr::WrongMove);
            }
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
