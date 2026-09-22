use crate::pieces::PieceView;

use super::{Engine, EngineErr, MoveId};

impl Engine {
    pub(super) fn kill_piece(&mut self, wid: usize, age: usize) -> Result<(), EngineErr> {
        let wl = self
            .history
            .worldlines
            .get_mut(wid)
            .ok_or(EngineErr::WrongPiece)?;

        for st in wl.get_mut(age..).ok_or(EngineErr::WrongPiece)? {
            st.alive = false;
        }

        Ok(())
    }

    pub(super) fn undo_last_mv(&mut self, piv: PieceView) -> Result<(), EngineErr> {
        let wl = self
            .history
            .worldlines
            .get(piv.origin)
            .ok_or(EngineErr::WrongPiece)?;

        let id = wl
            .get(..piv.age)
            .ok_or(EngineErr::WrongPiece)?
            .iter()
            .rev()
            .map(MoveId::from_state)
            .find(|id| {
                self.history
                    .moves
                    .get(id)
                    .is_some_and(|mv| mv.pid.wid == piv.origin)
            })
            .ok_or(EngineErr::WrongMove)?;
        self.undo_mv(id)
    }

    /// Clears move from move history,
    /// undoes position change of the piece
    /// and grants a reaction at its source. Rebuilds lookup indexes afterwards.
    pub(super) fn undo_mv(&mut self, mv_id: MoveId) -> Result<(), EngineErr> {
        // Remove old move
        let mv = self
            .history
            .moves
            .remove(&mv_id)
            .ok_or(EngineErr::WrongMove)?;

        // Piv before move, undid states will be set to it
        let bf_piv = self.get_piv_from_mv(mv).ok_or(EngineErr::WrongPiece)?;
        let rook = self.castling_rook(mv, bf_piv)?;
        if mv.to_past() != bf_piv.inverted {
            return Err(EngineErr::UnsupportedMove(
                "undoing a time-direction change requires loop reconstruction",
            ));
        }

        // Piv just after undid move,
        // is used to know what posion to undo
        let ud_piv = self
            .get_piv_necro(bf_piv.origin, bf_piv.age + 1)
            .ok_or(EngineErr::WrongPiece)?;

        let wl = self
            .history
            .worldlines
            .get_mut(bf_piv.origin)
            .ok_or(EngineErr::WrongPiece)?;

        // Cancellation grants a reaction without granting an ordinary move.
        {
            let mv_st = wl.get_mut(bf_piv.age).ok_or(EngineErr::WrongPiece)?;
            mv_st.rights.cancelled_move = true;
        }

        for st in wl.iter_mut().skip(bf_piv.age + 1) {
            if (st.x, st.y) == (ud_piv.sq.0, ud_piv.sq.1) {
                st.x = bf_piv.sq.0;
                st.y = bf_piv.sq.1;
                st.kind = bf_piv.kind;
            } else {
                break;
            }
        }

        // For castling we need to undo rook move as well
        if let Some(bf_rk) = rook {
            let ud_rk = self
                .get_piv_necro(bf_rk.origin, bf_rk.age + 1)
                .ok_or(EngineErr::WrongPiece)?;

            let wl = self
                .history
                .worldlines
                .get_mut(bf_rk.origin)
                .ok_or(EngineErr::WrongPiece)?;
            for st in wl.iter_mut().skip(bf_rk.age + 1) {
                if (st.x, st.y) == (ud_rk.sq.0, ud_rk.sq.1) {
                    st.x = bf_rk.sq.0;
                    st.y = bf_rk.sq.1;
                    st.kind = bf_rk.kind;
                } else {
                    break;
                }
            }
        }

        self.update_bufs()?;

        Ok(())
    }
}
