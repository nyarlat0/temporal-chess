use crate::board::{Board, PreBoard};
use crate::model::ShortPieceId;
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
        let times: Vec<_> = self.projection.preboards.keys().copied().collect();
        let limit = self.calculation_barrier().unwrap_or(through).min(through);
        for t in times {
            if t > limit {
                break;
            }
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

    /// Discover optional reactions. Only newly available targets grant a reaction;
    /// losing information alone does not grant a new move. Targets and blockers
    /// are observed exclusively on the piece's current board.
    pub(super) fn update_infos(&mut self) -> Result<(), EngineErr> {
        let mut updates = Vec::new();
        for (&t, board) in &self.projection.boards {
            if Color::from_board(t) != self.turn {
                continue;
            }
            // Knowledge belongs to this occurrence's own board. Legal temporal
            // moves may use adjacent boards, but must never feed future or past
            // targets back into this state's informed set.
            updates.extend(
                board
                    .calc_info()
                    .into_iter()
                    .filter(|(id, _)| !self.history.worldlines[id.wid].is_borrowed(id.age)),
            );
        }
        for (id, info) in updates {
            let state = &mut self.history.worldlines[id.wid][id.age];
            if !state.rights.ordinary && !info.is_subset(&state.informed) {
                state.rights.new_information = true;
            }
            state.informed = info;
        }
        Ok(())
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
