use crate::pieces::PieceView;

use super::{Engine, Move, ShortPieceId};

impl Engine {
    /// Returns None if figure is dead
    pub fn get_piv(&self, wid: usize, age: usize) -> Option<PieceView> {
        let line = self.wlines.get(wid)?;
        let state = line.get(age)?.clone();

        if !state.alive {
            return None;
        }

        Some(PieceView::from_state(&state, wid, age))
    }

    /// Returns None only if state not found.
    /// Looses info about live/death!!!
    pub fn get_piv_necro(&self, wid: usize, age: usize) -> Option<PieceView> {
        let line = self.wlines.get(wid)?;
        let state = line.get(age)?.clone();

        Some(PieceView::from_state(&state, wid, age))
    }

    /// Returns None if figure is dead
    pub fn get_piv_from_mv(&self, mv: Move) -> Option<PieceView> {
        let ShortPieceId { wid, age } = *self.pbuf.get(&mv.pid)?;
        self.get_piv(wid, age)
    }
}
