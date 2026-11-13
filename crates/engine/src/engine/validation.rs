use super::MoveId;
use crate::pieces::Sq;
use std::collections::BTreeSet;

use super::{Engine, EngineErr, ShortPieceId, ValidRes};

impl Engine {
    /// An undo reaction is useful only if this occurrence has a spatial reply.
    /// Run after automatic continuation, so a blocked terminal reserve can
    /// first move to a board where it can act.
    fn prune_immobile_undo_reactions(&mut self) -> Result<bool, EngineErr> {
        let mut clear = Vec::new();
        let mut exits = Vec::new();
        for (wid, line) in self.history.worldlines.iter().enumerate() {
            for (age, state) in line.iter().enumerate() {
                if state.is_loop_exit()
                    && (state.rights.is_reaction()
                        || (age + 1 < line.len() && state.rights.ordinary))
                {
                    exits.push((wid, age, age + 1 == line.len()));
                }
                if state.alive
                    && state.rights.cancelled_move
                    && self.board(state.t).is_some()
                    && self.candidate_moves(wid, age)?.is_empty()
                {
                    clear.push((wid, age));
                }
            }
        }
        for &(wid, age) in &clear {
            self.history.worldlines[wid][age].rights.cancelled_move = false;
        }
        for &(wid, age, terminal) in &exits {
            if terminal {
                self.history.worldlines[wid][age].rights.expire_reactions();
            } else {
                self.history.worldlines[wid][age].rights = Default::default();
            }
        }
        Ok(!clear.is_empty() || !exits.is_empty())
    }
    /// A collision cycle can prevent the chronological frontier from reaching
    /// the move which created that collision. Before declaring a paradox, cancel
    /// a recorded move already impossible from its changed source, regardless
    /// of blockers. Full board-dependent validation still follows the frontier.
    fn cancel_impossible_cycle_move(&mut self) -> Result<ValidRes, EngineErr> {
        for (id, mv) in self.history.moves.clone() {
            let Some(piece) = self.get_piv_from_mv(mv) else {
                continue;
            };
            if !self.history.worldlines[piece.origin].is_borrowed(piece.age)
                && !self.recorded_move_has_geometry(piece, mv)
            {
                self.undo_mv(id)
                    .map_err(|e| e.during(format!("cancelling cycle cause {mv:?}")))?;
                return Ok(ValidRes::HistChanged);
            }
        }
        Ok(ValidRes::Unchanged)
    }

    /// Cancel one invalid move in chronological/causal order, then rebuild.
    fn validate_moves(
        &mut self,
        verified: &mut BTreeSet<MoveId>,
        reuse: bool,
    ) -> Result<ValidRes, EngineErr> {
        let barrier = self.calculation_barrier();
        for (&id, &mv) in &self.history.moves {
            if verified.contains(&id) {
                continue;
            }
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
            // candidates_for reads the source, both adjacent destinations and
            // immutable history/preboards (including en passant evidence).
            // Once this whole neighbourhood is resolved, later prefix growth
            // cannot change its answer. Never reuse across a history mutation.
            if reuse
                && [mv.t.checked_sub(1), mv.t.checked_add(1)]
                    .into_iter()
                    .flatten()
                    .all(|t| self.preboard(t).is_none() || self.board(t).is_some())
            {
                verified.insert(id);
            }
        }
        Ok(ValidRes::Unchanged)
    }

    /// Restore one dead suffix whose original collision has disappeared.
    /// Later collisions will be resolved on the next pass, from fresh boards.
    fn resurrect(
        &mut self,
        candidates: &[usize],
        checked: &mut BTreeSet<usize>,
        reuse: bool,
    ) -> Result<ValidRes, EngineErr> {
        let barrier = self.calculation_barrier();
        for &wid in candidates {
            if checked.contains(&wid) {
                continue;
            }
            // The first dead state and its collision/EP causes stay fixed in
            // this pass. Revisit only while its board is still unavailable.
            if reuse
                && self.history.worldlines[wid]
                    .iter()
                    .find(|s| !s.alive)
                    .is_none_or(|s| self.board(s.t).is_some())
            {
                checked.insert(wid);
            }
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
        self.reconcile_with_auto_wait(true)
    }

    pub(super) fn reconcile_with_auto_wait(&mut self, auto: bool) -> Result<(), EngineErr> {
        let mut auto_waited = Vec::new();
        let mut seen = Vec::new();
        loop {
            if seen.contains(&self.history) {
                if self.cancel_impossible_cycle_move()? == ValidRes::HistChanged {
                    continue;
                }
                return Err(EngineErr::InfLoop);
            }
            seen.push(self.history.clone());
            // A living causal endpoint is always an ordinary reserve on its
            // own-color board, regardless of how its former tail disappeared.
            for line in &mut self.history.worldlines {
                if let Some(state) = line.states.last_mut()
                    && state.alive
                    && crate::model::Color::from_board(state.t) == state.color
                {
                    state.rights.ordinary = true;
                }
            }
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
                self.update_infos()?;
                // A paused calculation still publishes fresh observations and
                // removes reactions whose targets have disappeared or are no
                // longer on a calculated board.
                self.update_bufs()?;
                self.projection.boards = self
                    .timeline_times()
                    .filter(|&t| t <= limit)
                    .filter_map(|t| self.make_board(t).ok().map(|board| (t, board)))
                    .collect();
                if self.auto_wait_blocked_endpoint(&mut auto_waited, auto)? {
                    continue;
                }
                if self.prune_immobile_undo_reactions()? {
                    continue;
                }
                if self.acknowledge_auto_wait(&auto_waited)? {
                    continue;
                }
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
            if self.acknowledge_auto_wait(&auto_waited)? {
                continue;
            }
            if self.auto_wait_blocked_endpoint(&mut auto_waited, auto)? {
                continue;
            }
            if self.prune_immobile_undo_reactions()? {
                continue;
            }
            return Ok(());
        }
    }

    fn reconcile_boards(&mut self) -> Result<ValidRes, EngineErr> {
        self.reconcile_boards_reusing_prefix(true)
    }

    fn reconcile_boards_reusing_prefix(&mut self, reuse: bool) -> Result<ValidRes, EngineErr> {
        let mut verified = BTreeSet::new();
        let mut checked_deaths = BTreeSet::new();
        let candidates: Vec<_> = (0..self.history.worldlines.len())
            .filter(|&wid| !reuse || self.history.worldlines[wid].iter().any(|s| !s.alive))
            .collect();
        // Advance the validation frontier chronologically. Building every board
        // first would resolve future collisions before discovering an earlier
        // cancelled move, and destroy exactly the preboards the UI must expose.
        let times = if self.history.calculation.interactive {
            self.timeline_times().collect::<Vec<_>>()
        } else {
            vec![self.history.horizon]
        };
        self.projection.boards.clear();
        for through in times {
            if self.calculation_barrier().is_some_and(|t| through > t) {
                break;
            }
            let built = if reuse {
                self.extend_boards(through)?
            } else {
                self.build_boards(through)?
            };
            if built == ValidRes::HistChanged
                || self.validate_moves(&mut verified, reuse)? == ValidRes::HistChanged
                || self.resurrect(&candidates, &mut checked_deaths, reuse)? == ValidRes::HistChanged
            {
                return Ok(ValidRes::HistChanged);
            }
        }
        Ok(ValidRes::Unchanged)
    }
}

#[cfg(test)]
mod reuse_tests {
    use super::*;
    use crate::record::{Command, GameRecord};

    #[test]
    fn impossible_knight_geometry_is_cancelled_before_declaring_a_cycle() {
        use crate::model::{Color, PieceKind, State};
        let mut knight = State::new(PieceKind::N, Color::White, Sq(1, 0));
        knight.t = 4;
        knight.fut_seen = 4;
        let mut engine = Engine::from_worldlines(vec![vec![knight]], Color::White).unwrap();
        let mv = engine
            .possible_moves(0, 0)
            .unwrap()
            .into_iter()
            .find(|m| !m.to_past() && m.sq == Sq(2, 2))
            .unwrap();
        engine.apply_move_inner(mv).unwrap();
        assert_eq!(
            engine.cancel_impossible_cycle_move(),
            Ok(ValidRes::Unchanged)
        );
        // An earlier cancellation relocated the source while its later move
        // still targets c3. a1-c3 has invalid knight geometry.
        engine.history.worldlines[0][0].x = 0;
        engine.update_bufs().unwrap();
        assert_eq!(
            engine.cancel_impossible_cycle_move(),
            Ok(ValidRes::HistChanged)
        );
        assert!(!engine.history.moves.contains_key(&mv.id()));
        engine.reconcile().unwrap();
        assert!(engine.outcome().is_none());
        assert!(
            engine.history.worldlines[0]
                .iter()
                .all(|s| s.alive && s.x == 0 && s.y == 0)
        );
    }

    fn compare_passes(mut position: Engine) {
        position.update_bufs().unwrap();
        let mut reference = position.clone();
        let optimized = position.reconcile_boards_reusing_prefix(true);
        let original = reference.reconcile_boards_reusing_prefix(false);
        assert_eq!(optimized, original);
        assert_eq!(position, reference);
    }
    #[test]
    fn reuse_matches_rebuilding_after_temporal_moves_collisions_and_cancellations() {
        for text in [
            "NOLAN-CHESS 2\ncausal_gaps off\nmove 0 12:0:0 e2 e4 + normal\nmove 1 28:1:0 e7 e5 + normal\nmove 2 6:2:0 g1 f3 - normal\nmove 3 21:3:0 f8 b4 - normal\nmove 0 6:2:1 f3 e5 + normal\n",
            "NOLAN-CHESS 4\nchain_info_activation on\nglobal_reaction_reset on\nstart time_war 5\nmove 0 12:0:0 e2 e4 + normal\nmove 5 28:5:0 e7 e5 - normal\n",
        ] {
            let (end, journal) = GameRecord::load(text).unwrap();
            compare_passes(end);
            let mut engine = Engine::with_start(journal.rules(), journal.starting_position())
                .unwrap()
                .with_interactive_calculation()
                .unwrap();
            let mut record = GameRecord::with_start(journal.rules(), journal.starting_position());
            for &command in journal.commands() {
                if let Command::Move { mv, .. } = command {
                    let mut intermediate = engine.clone();
                    intermediate.history.calculation.acknowledged.clear();
                    intermediate.history.calculation.responsible = Some(intermediate.turn);
                    if intermediate.apply_move_inner(mv).is_ok() {
                        intermediate.turn = intermediate.turn.opposite();
                        compare_passes(intermediate);
                    }
                }
                record.apply(&mut engine, command).unwrap();
                compare_passes(engine.clone());
            }
        }
    }
}
