use crate::pieces::{Color, PieceView, Sq};

use super::{Castling, Engine, EngineErr, Move, MoveId};

impl Engine {
    pub fn apply_new_mv(&mut self, mv: Move) -> Result<(), EngineErr> {
        // Check that moved piece can be played
        let piv = self.get_piv_from_mv(mv).ok_or(EngineErr::WrongPiece)?;
        if !piv.is_movable() {
            return Err(EngineErr::WrongPiece);
        }

        // Check that move belongs to possible moves
        if !self.possible_moves(piv.origin, piv.age)?.contains(&mv) {
            return Err(EngineErr::WrongMove);
        }

        // If new move substitudes another move then remove
        // and undo it, but only if not to_past -> to_past of the same piece,
        // since undoing would clear the backward loop and we need to fix it,
        // else new move will just replace old by insert
        let mv_id = MoveId::from_piv(piv);
        if let Some(old_mv) = self.move_history.get(&mv_id)
            && !(old_mv.pid == mv.pid && old_mv.to_past && mv.to_past)
        {
            self.undo_mv(mv_id)?;
        }

        // Deactivate pieces with the same order (fut_seen, loop_turn)
        self.deact_same_order(piv)?;

        // Deactivate all optionally movable pieces
        self.deact_prev_reactable(piv.color, piv.t);

        // If this move is from the last accessible t
        // then prolongate the worldlines to build new board
        if piv.t + 1 == self.preboards.len() {
            self.prolongate_wlines();
        }

        // If move is to the

        // Apply new move
        self.fix_position(mv)?;

        self.move_history.insert(mv_id, mv);

        Ok(())
    }

    /// Applies new position to newly moved piece.
    /// Should be used after old move undo,
    /// but only if old move wasnt of the same piece to_past -> to_past
    /// in order to NOT clear the backward loop, but fix it.
    pub fn fix_position(&mut self, new_mv: Move) -> Result<(), EngineErr> {
        // Piv before move
        let bf_piv = self.get_piv_from_mv(new_mv).ok_or(EngineErr::WrongPiece)?;

        // Piv just after undid move,
        // is used to know what posion to undo
        let ud_piv = self
            .get_piv_necro(bf_piv.origin, bf_piv.age + 1)
            .ok_or(EngineErr::WrongPiece)?;

        let wl = self
            .wlines
            .get_mut(bf_piv.origin)
            .ok_or(EngineErr::WrongPiece)?;

        // Apply new move to afterward states that are already in history
        for st in wl.iter_mut().skip(bf_piv.age + 1) {
            if (st.x, st.y) == (ud_piv.sq.0, ud_piv.sq.1) {
                st.apply_mv(new_mv);
            } else {
                break;
            }
        }

        Ok(())
    }

    pub fn prolongate_wlines(&mut self) {
        let end_t = self.preboards.len().saturating_sub(1);
        for wl in self.wlines.iter_mut() {
            let last_st = wl.last().unwrap();
            if last_st.t == end_t && !last_st.inverted {
                let new_st = last_st.prolongate();
                wl.push(new_st);
            }
        }
    }

    pub fn deact_same_order(&mut self, piv: PieceView) -> Result<(), EngineErr> {
        let board = self.boards.get(&piv.t).ok_or(EngineErr::AbsentBoard)?;
        for p in board.pieces.iter() {
            if p.is_movable() && p.same_order(piv) {
                self.wlines
                    .get_mut(p.origin)
                    .ok_or(EngineErr::WrongPiece)?
                    .get_mut(p.age)
                    .ok_or(EngineErr::WrongPiece)?
                    .make_unmovable();
            }
        }

        Ok(())
    }

    pub fn deact_prev_reactable(&mut self, of: Color, until_t: usize) {
        for wl in self.wlines.iter_mut() {
            for st in wl.iter_mut() {
                if st.color == of && st.t <= until_t && st.is_react_movable() {
                    st.make_unrectable();
                }
            }
        }
    }
}
