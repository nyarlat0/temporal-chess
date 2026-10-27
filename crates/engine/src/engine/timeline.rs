use crate::pieces::PieceView;

use super::{Engine, Move, ShortPieceId};

impl Engine {
    pub(super) fn previous_piece(&self, piece: PieceView) -> Option<PieceView> {
        let age = piece.age.checked_sub(1)?;
        if self.history.worldlines[piece.origin]
            .gap_after(age)
            .is_some()
        {
            return None;
        }
        self.get_piv(piece.origin, age)
    }

    pub(super) fn next_piece(&self, piece: PieceView) -> Option<PieceView> {
        if self.history.worldlines[piece.origin]
            .gap_after(piece.age)
            .is_some()
        {
            return None;
        }
        self.get_piv(piece.origin, piece.age.checked_add(1)?)
    }

    /// Returns None if figure is dead
    pub fn get_piv(&self, wid: usize, age: usize) -> Option<PieceView> {
        let line = self.history.worldlines.get(wid)?;
        let state = line.get(age)?;

        if !state.alive {
            return None;
        }

        Some(PieceView::from_state(state, wid, age))
    }

    /// Returns None only if state not found.
    /// Looses info about live/death!!!
    pub fn get_piv_necro(&self, wid: usize, age: usize) -> Option<PieceView> {
        let line = self.history.worldlines.get(wid)?;
        let state = line.get(age)?;

        Some(PieceView::from_state(state, wid, age))
    }

    /// Returns None if figure is dead
    pub fn get_piv_from_mv(&self, mv: Move) -> Option<PieceView> {
        let ShortPieceId { wid, age } = *self.projection.pieces.get(&mv.source())?;
        self.get_piv(wid, age)
    }
}
