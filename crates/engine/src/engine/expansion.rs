use super::{Engine, EngineErr, Move, PieceAt, PieceId};
use crate::model::{BoardTime, MoveRights, PieceView, StartingPosition};

impl Engine {
    /// Only an admitted spatial command expands the physical timeline.
    pub(super) fn expand_for_move(&mut self, mut mv: Move) -> Result<Move, EngineErr> {
        let arrival = mv.direction.step(mv.t).ok_or(EngineErr::WrongMove)?;
        let own = mv.direction.step(arrival).ok_or(EngineErr::WrongMove)?;
        let lower = self.history.beginning.min(own);
        let upper = if self.history.starting_position == StartingPosition::Standard {
            if mv.t == self.history.horizon {
                self.history
                    .horizon
                    .checked_add(1)
                    .ok_or(EngineErr::WrongMove)?
            } else {
                self.history.horizon
            }
        } else {
            self.history.horizon.max(own)
        };

        self.extend_timeline(lower, upper, Some(mv.pid.wid))?;
        mv = self.history.remap_move(mv);
        self.update_bufs()?;
        Ok(mv)
    }

    /// Shared by spatial expansion and Lab board creation. Passive suffixes
    /// follow their orientation; causal ancestors are prepended on the other end.
    pub(super) fn extend_timeline(
        &mut self,
        lower: BoardTime,
        upper: BoardTime,
        departing: Option<usize>,
    ) -> Result<(), EngineErr> {
        while self.history.beginning > lower {
            let t = self
                .history
                .beginning
                .checked_sub(1)
                .ok_or(EngineErr::WrongMove)?;
            for (wid, line) in self.history.worldlines.iter_mut().enumerate() {
                if line.first().is_some_and(|s| !s.inverted && s.t == t + 1) {
                    let prefix = Self::ancestral_state(&line[0], t);
                    line.states.insert(0, prefix);
                } else if Some(wid) != departing
                    && line.last().is_some_and(|s| s.inverted && s.t == t + 1)
                {
                    let next = line
                        .last()
                        .unwrap()
                        .prolongate()
                        .ok_or(EngineErr::WrongMove)?;
                    line.states.push(next);
                }
            }
            self.history.beginning = t;
        }
        while self.history.horizon < upper {
            let t = self
                .history
                .horizon
                .checked_add(1)
                .ok_or(EngineErr::WrongMove)?;
            let mut raised = Vec::new();
            for (wid, line) in self.history.worldlines.iter_mut().enumerate() {
                if line.first().is_some_and(|s| s.inverted && s.t == t - 1) {
                    let prefix = Self::ancestral_state(&line[0], t);
                    line.states.insert(0, prefix);
                    raised.push(wid);
                } else if Some(wid) != departing
                    && line.last().is_some_and(|s| !s.inverted && s.t == t - 1)
                {
                    let next = line
                        .last()
                        .unwrap()
                        .prolongate()
                        .ok_or(EngineErr::WrongMove)?;
                    line.states.push(next);
                }
            }
            self.raise_ancestral_knowledge(&raised, t);
            self.history.horizon = t;
        }
        Ok(())
    }

    pub(super) fn ancestral_state(
        source: &crate::model::State,
        t: BoardTime,
    ) -> crate::model::State {
        let mut prefix = source.clone();
        prefix.t = t;
        prefix.fut_seen = if source.inverted {
            source.fut_seen.max(t)
        } else if source.fut_seen > source.t {
            source.fut_seen
        } else {
            t
        };
        prefix.rights = MoveRights::default();
        prefix
    }

    /// An inverted ancestral prefix has really seen the newly exposed future.
    /// Rewrite every persistent reference together; ages are rebuilt separately.
    pub(super) fn raise_ancestral_knowledge(&mut self, affected: &[usize], t: BoardTime) {
        let pid = |mut id: PieceId| {
            if affected.contains(&id.wid) {
                id.fut_seen = id.fut_seen.max(t);
            }
            id
        };
        let at = |mut id: PieceAt| {
            id.piece = pid(id.piece);
            id
        };
        let route = |wid: usize, line: &mut crate::model::Worldline| {
            if affected.contains(&wid) {
                for state in line.iter_mut() {
                    state.fut_seen = state.fut_seen.max(t);
                }
                for gap in &mut line.gaps {
                    gap.entry.fut_seen = gap.entry.fut_seen.max(t);
                    gap.after.fut_seen = gap.after.fut_seen.max(t);
                    if let Some(id) = &mut gap.resume {
                        id.fut_seen = id.fut_seen.max(t);
                    }
                }
            }
        };
        for (wid, line) in self.history.worldlines.iter_mut().enumerate() {
            route(wid, line);
        }
        let mut keys = std::collections::BTreeMap::new();
        self.history.moves = std::mem::take(&mut self.history.moves)
            .into_iter()
            .map(|(old, mut mv)| {
                mv.pid = pid(mv.pid);
                keys.insert(old, mv.id());
                (mv.id(), mv)
            })
            .collect();
        self.history.targets = std::mem::take(&mut self.history.targets)
            .into_iter()
            .map(|(key, values)| {
                (
                    keys.get(&key).copied().unwrap_or(key),
                    values.into_iter().map(at).collect(),
                )
            })
            .collect();
        for id in &mut self.history.calculation.acknowledged {
            *id = at(*id);
        }
        for edit in &mut self.history.calculation.cancellations {
            edit.source = at(edit.source);
            for (wid, a, b) in &mut edit.lines {
                route(*wid, a);
                route(*wid, b);
            }
        }
    }

    pub(super) fn refreshed_piece(&self, old: PieceView) -> Result<PieceView, EngineErr> {
        self.history.worldlines[old.origin]
            .iter()
            .enumerate()
            .find(|(_, s)| {
                s.t == old.t && s.loop_turn == old.loop_turn && s.fut_seen >= old.fut_seen
            })
            .map(|(age, s)| PieceView::from_state(s, old.origin, age))
            .ok_or(EngineErr::WrongPiece)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Color, PieceKind, Sq, State, TimeDirection};

    #[test]
    fn negative_ancestor_can_react_and_occupy_its_own_causal_slot() {
        let mut engine = Engine::default();
        let mv = engine
            .possible_moves(6, 0)
            .unwrap()
            .into_iter()
            .find(|mv| mv.to_past() && mv.sq == Sq(5, 2))
            .unwrap();
        engine.apply_new_mv(mv).unwrap();
        // A newly introduced opponent becomes visible on the ancestral board.
        let mut enemy = State::new(PieceKind::N, Color::Black, Sq(2, 2));
        enemy.t = -2;
        enemy.inverted = true;
        engine.history.worldlines.push(vec![enemy].into());
        engine.turn = Color::White;
        engine.reconcile().unwrap();

        let ancestor = engine
            .move_sources()
            .find(|p| p.origin == 1 && p.t == -2)
            .expect("info-active ancestor");
        assert!(ancestor.rights.new_information);
        assert!(!ancestor.rights.ordinary);
        assert_eq!(ancestor.fut_seen, -2);
        assert!(!ancestor.knows_future());
        let reply = engine
            .possible_moves(ancestor.origin, ancestor.age)
            .unwrap()
            .into_iter()
            .find(|mv| mv.sq == Sq(0, 2) && mv.direction == TimeDirection::Future)
            .unwrap();
        engine.apply_new_mv(reply).unwrap();
        assert_eq!(engine.history.moves.get(&reply.id()), Some(&reply));
        assert_eq!(
            (reply.id().t, reply.id().fut_seen, reply.id().loop_turn),
            (-2, -2, 0)
        );
        assert_eq!(engine.history.moves.get(&mv.id()), Some(&mv));
        assert_eq!(
            Engine::from_history(engine.history.clone(), engine.turn).unwrap(),
            engine
        );
    }
}
