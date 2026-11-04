use crate::board::{Board, PreBoard};
use crate::model::{MoveId, ShortPieceId};
use crate::pieces::{Color, PieceView};

use super::{BoardConflict, BoardErr, Engine, EngineErr, PieceAt, PieceId, Projection, ValidRes};

impl Engine {
    pub fn make_board(&self, t: usize) -> Result<Board, BoardErr> {
        let preboard = self
            .projection
            .preboards
            .get(&t)
            .ok_or(BoardErr::InvalidTime)?;
        if let Some(cell) = preboard.space.iter().flatten().find(|cell| cell.len() > 1) {
            return Err(BoardErr::InvalidBoard(BoardConflict { pivs: cell.clone() }));
        }
        preboard.try_into_board().ok_or(BoardErr::InvalidTime)
    }

    /// Resolve at most one intersection. The caller rebuilds every derived view
    /// before continuing, so no conflict is resolved against stale projections.
    pub(super) fn build_boards(&mut self, through: usize) -> Result<ValidRes, EngineErr> {
        self.projection.boards.clear();
        let limit = self.calculation_barrier().unwrap_or(through).min(through);
        let times: Vec<_> = self
            .projection
            .preboards
            .range(..=limit)
            .map(|(&t, _)| t)
            .collect();
        for t in times {
            match self.make_board(t) {
                Ok(board) => {
                    self.projection.boards.insert(t, board);
                }
                Err(BoardErr::InvalidBoard(conflict)) => {
                    self.try_fix_conflict(conflict)?;
                    return Ok(ValidRes::HistChanged);
                }
                Err(error) => return Err(EngineErr::BoardErr(error)),
            }
        }
        Ok(ValidRes::Unchanged)
    }

    /// Derive reactions from the current view and acknowledged memory. Never
    /// learn merely by calculating a board: doing so would consume a reaction.
    pub(super) fn update_infos(&mut self) -> Result<(), EngineErr> {
        for line in &mut self.history.worldlines {
            for state in line.iter_mut() {
                state.informed.clear();
                state.rights.new_information = false;
            }
        }
        for (&t, board) in &self.projection.boards {
            // Adjacent boards supply only the double-step evidence for en passant;
            // targets and blockers still come from this occurrence's own board.
            let adjacent: Vec<_> = [t.checked_sub(1), t.checked_add(1)]
                .into_iter()
                .flatten()
                .filter_map(|time| self.projection.preboards.get(&time))
                .collect();
            for (id, info) in board.calc_info_with_en_passant(&adjacent) {
                if !self.history.worldlines[id.wid].is_borrowed(id.age) {
                    self.history.worldlines[id.wid][id.age].informed = info;
                }
            }
        }
        for (wid, line) in self.history.worldlines.iter_mut().enumerate() {
            let mut first_seen = std::collections::BTreeSet::new();
            // Subjective age, not board time: this also orders inverted states
            // and separate incarnations of the same worldline causally.
            for state in line.iter_mut() {
                let has_own_move = self
                    .history
                    .moves
                    .get(&MoveId::from_state(state))
                    .is_some_and(|mv| mv.pid == PieceId::from_state(state, wid));
                if !state.alive
                    || state.rights.ordinary
                    || state.rights.cancelled_move
                    || has_own_move
                {
                    continue;
                }
                for target in state.informed.difference(&state.remembered) {
                    if self.history.rules.chain_info_activation || first_seen.insert(*target) {
                        state.rights.new_information = true;
                    }
                }
            }
        }
        Ok(())
    }

    /// Accept the pre-command view on the boards whose reactions expire. Include
    /// suppressed later observations too, so deduplication cannot postpone an
    /// already acknowledged target's reaction until another recalculation.
    pub(super) fn remember_information(&mut self, side: Color, until_t: usize) {
        for state in self
            .history
            .worldlines
            .iter_mut()
            .flat_map(|line| line.iter_mut())
        {
            if state.color == side && state.t <= until_t {
                state.remembered.extend(state.informed.iter().copied());
            }
        }
    }

    /// Rebuild all indexes from the authoritative worldlines, including dead states
    /// for historical move lookup. Empty slices are kept even if everyone died.
    pub(super) fn update_bufs(&mut self) -> Result<(), EngineErr> {
        let mut projection = Projection::default();
        projection
            .preboards
            .insert(self.history.horizon, PreBoard::new(self.history.horizon));
        for (origin, line) in self.history.worldlines.iter().enumerate() {
            for (age, state) in line.iter().enumerate() {
                let board = projection
                    .preboards
                    .entry(state.t)
                    .or_insert_with(|| PreBoard::new(state.t));
                if state.alive {
                    board.space[state.x][state.y].push(PieceView::from_state(state, origin, age));
                }
                let occurrence = PieceAt {
                    piece: PieceId::from_state(state, origin),
                    t: state.t,
                };
                if projection
                    .pieces
                    .insert(occurrence, ShortPieceId { wid: origin, age })
                    .is_some()
                {
                    return Err(EngineErr::InvalidHistory("ambiguous piece occurrence"));
                }
            }
        }
        self.projection = projection;
        Ok(())
    }
}
