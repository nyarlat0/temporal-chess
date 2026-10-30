use crate::model::{CausalGap, MoveId, PieceAt, PieceId, State, TimeDirection};
use crate::pieces::{Color, PieceView, Sq};

use super::{Engine, EngineErr, Move};

impl Engine {
    /// Remove the old backward excursion, retaining the first continuation in
    /// future board time. Journal entries are removed by their source identities.
    pub(super) fn remove_past_route(&mut self, piece: PieceView) -> Result<(), EngineErr> {
        if !self.history.causal_gaps {
            return self.prune_route(piece, self.history.worldlines[piece.origin].len());
        }
        let line = &self.history.worldlines[piece.origin];
        let end = (piece.age + 1..line.len())
            .find(|&i| line[i].t > piece.t)
            .unwrap_or(line.len());
        self.prune_route(piece, end)
    }

    pub(super) fn remove_return_route(&mut self, piece: PieceView) -> Result<(), EngineErr> {
        if !self.history.causal_gaps {
            return self.prune_route(piece, self.history.worldlines[piece.origin].len());
        }
        let line = &self.history.worldlines[piece.origin];
        let end = (piece.age + 1..line.segment_end(piece.age))
            .find(|&i| line[i].t > piece.t && line[i].loop_turn < piece.loop_turn)
            .unwrap_or_else(|| line.segment_end(piece.age));
        self.prune_route(piece, end)
    }

    fn prune_route(&mut self, piece: PieceView, end: usize) -> Result<(), EngineErr> {
        let line = &mut self.history.worldlines[piece.origin];
        let removed: Vec<_> = line[piece.age + 1..end]
            .iter()
            .map(MoveId::from_state)
            .collect();
        let source = MoveId::from_piv(piece);
        let resume = line.get(end).map(MoveId::from_state);
        line.gaps
            .retain(|gap| !removed.contains(&gap.entry) && gap.entry != source);
        for gap in &mut line.gaps {
            if removed.contains(&gap.after) {
                gap.after = source;
                gap.resume = resume;
            }
            if gap.resume.is_some_and(|key| removed.contains(&key)) {
                gap.resume = resume;
            }
        }
        line.states.drain(piece.age + 1..end);
        // Until the replacement step joins the tail, preserve the causal boundary.
        if resume.is_some() && line.gap_after(piece.age).is_none() {
            line.gaps.push(CausalGap {
                entry: source,
                after: source,
                resume,
            });
        }
        for id in removed {
            if self
                .history
                .moves
                .get(&id)
                .is_some_and(|mv| mv.pid.wid == piece.origin)
            {
                self.history.moves.remove(&id);
                self.history.targets.remove(&id);
            }
        }
        self.update_bufs()?;
        Ok(())
    }

    /// Apply a spatial change only along an already connected portion of the line.
    /// A recorded move, or an outstanding debt, terminates propagation.
    pub(super) fn relocate_from(&mut self, wid: usize, age: usize, next: &State) {
        let line = &mut self.history.worldlines[wid];
        let old_sq = Sq(line[age].x, line[age].y);
        let end = line.segment_end(age);
        for i in age..end {
            if Sq(line[i].x, line[i].y) != old_sq {
                break;
            }
            line[i].x = next.x;
            line[i].y = next.y;
            line[i].kind = next.kind;
            if self
                .history
                .moves
                .get(&MoveId::from_state(&line[i]))
                .is_some_and(|mv| mv.pid.wid == wid)
            {
                break;
            }
        }
    }

    /// Insert one subjective tick or join the continuation whose causality has
    /// just been repaid. Returns the resulting age and whether a tail was joined.
    pub(super) fn advance_route(
        &mut self,
        wid: usize,
        age: usize,
        next: State,
    ) -> Result<(usize, bool), EngineErr> {
        let line = &mut self.history.worldlines[wid];
        let key = MoveId::from_state(&next);
        let gap_index = line.gap_after(age);
        if let Some(index) = gap_index {
            let gap = &line.gaps[index];
            if gap.resume == Some(key) {
                if line.get(age + 1).map(MoveId::from_state) != Some(key) {
                    return Err(EngineErr::InvalidHistory("missing preserved continuation"));
                }
                line.gaps.remove(index);
                self.relocate_from(wid, age + 1, &next);
                return Ok((age + 1, true));
            }
            if gap
                .resume
                .is_some_and(|resume| next.t >= resume.t && !next.inverted)
            {
                return Err(EngineErr::InvalidHistory(
                    "loop returned with a different causal identity",
                ));
            }
        }
        line.states.insert(age + 1, next);
        if let Some(index) = gap_index {
            line.gaps[index].after = key;
            if line.gaps[index].resume.is_none()
                && key.t > line.gaps[index].entry.t
                && key.loop_turn <= line.gaps[index].entry.loop_turn
            {
                line.gaps.remove(index);
            }
        }
        Ok((age + 1, false))
    }

    pub(super) fn apply_route(
        &mut self,
        piece: PieceView,
        mv: Move,
        horizon: usize,
    ) -> Result<(), EngineErr> {
        let source = self.history.worldlines[piece.origin][piece.age].clone();
        let mut next = source.step(mv.direction).ok_or(EngineErr::WrongMove)?;
        next.apply_mv(mv);
        let line = &self.history.worldlines[piece.origin];
        let connected = line.gap_after(piece.age).is_none();
        let same_route = connected
            && line.get(piece.age + 1).is_some_and(|st| {
                MoveId::from_state(st) == MoveId::from_state(&next) && st.inverted == next.inverted
            });
        let age;
        if same_route {
            self.relocate_from(piece.origin, piece.age + 1, &next);
            age = piece.age + 1;
        } else {
            if !self.history.causal_gaps {
                // Discard the old causal suffix in either turning direction.
                // No preserved tail means no gap and no borrowed states.
                self.prune_route(piece, self.history.worldlines[piece.origin].len())?;
            }
            if self.history.causal_gaps
                && connected
                && line.get(piece.age + 1).is_some_and(|st| st.t < piece.t)
                && mv.direction == TimeDirection::Future
            {
                self.remove_past_route(piece)?;
            }
            let line = &mut self.history.worldlines[piece.origin];
            if self.history.causal_gaps
                && line.gap_after(piece.age).is_none()
                && (line.get(piece.age + 1).is_some() || mv.to_past())
            {
                let key = MoveId::from_piv(piece);
                line.gaps.push(CausalGap {
                    entry: key,
                    after: key,
                    resume: line.get(piece.age + 1).map(MoveId::from_state),
                });
            }
            let (arrived, joined) = self.advance_route(piece.origin, piece.age, next)?;
            age = arrived;
            if joined {
                return Ok(());
            }
        }
        // A move arrives on the opponent's parity. Within existing history the
        // piece also has its passive tick to its next own-color board.
        let line = &self.history.worldlines[piece.origin];
        if Color::from_board(line[age].t) != piece.color
            && (age + 1 == line.len() || line.gap_after(age).is_some())
        {
            let waiting = line[age].prolongate().ok_or(EngineErr::WrongMove)?;
            if waiting.t <= horizon {
                self.advance_route(piece.origin, age, waiting)?;
            }
        }
        Ok(())
    }

    pub(super) fn prolongate_wlines(
        &mut self,
        end_t: usize,
        departing: Option<usize>,
    ) -> Result<(), EngineErr> {
        for wid in 0..self.history.worldlines.len() {
            if departing == Some(wid) {
                continue;
            }
            let line = &self.history.worldlines[wid];
            let age = line.len() - 1;
            let last = &line[age];
            if last.t == end_t && !last.inverted {
                let next = last.prolongate().ok_or(EngineErr::WrongMove)?;
                self.advance_route(wid, age, next)?;
            }
        }
        Ok(())
    }

    pub(super) fn occurrence(&self, id: PieceAt) -> Option<PieceView> {
        let location = self.projection.pieces.get(&id)?;
        self.get_piv_necro(location.wid, location.age)
    }

    pub(super) fn piece_at(piece: PieceView) -> PieceAt {
        PieceAt {
            piece: PieceId::from_piv(piece),
            t: piece.t,
        }
    }
}
