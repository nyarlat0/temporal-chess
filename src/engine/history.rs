use crate::pieces::{Color, PieceView, Sq};

use super::{Castling, Engine, EngineErr, MoveId};

impl Engine {
    pub fn kill_piece(&mut self, wid: usize, age: usize) -> Result<(), EngineErr> {
        let wl = self.wlines.get_mut(wid).ok_or(EngineErr::WrongPiece)?;

        for st in &mut wl[age..] {
            st.alive = false;
        }

        self.update_bufs();
        Ok(())
    }

    pub fn undo_last_mv(&mut self, piv: PieceView) -> Result<(), EngineErr> {
        let wl = self.wlines.get(piv.origin).ok_or(EngineErr::WrongPiece)?;

        for st in wl[..piv.age].iter().rev() {
            if (st.x, st.y) != (piv.sq.0, piv.sq.1) {
                self.undo_mv(MoveId::from_state(st))?;
                break;
            }
        }

        Ok(())
    }

    /// Clears move from move history,
    /// undoes position change of the piece
    /// and makes it active again,
    /// updates buffers and action lines after.
    pub fn undo_mv(&mut self, mv_id: MoveId) -> Result<(), EngineErr> {
        // Remove old move
        let mv = self
            .move_history
            .remove(&mv_id)
            .ok_or(EngineErr::WrongMove)?;

        // Piv before move, undid states will be set to it
        let bf_piv = self.get_piv_from_mv(mv).ok_or(EngineErr::WrongPiece)?;

        // Piv just after undid move,
        // is used to know what posion to undo
        let mut ud_piv = self
            .get_piv_necro(bf_piv.origin, bf_piv.age + 1)
            .ok_or(EngineErr::WrongPiece)?;

        let wl = self
            .wlines
            .get_mut(bf_piv.origin)
            .ok_or(EngineErr::WrongPiece)?;

        // Make piece active again
        {
            let mv_st = wl.get_mut(bf_piv.age).ok_or(EngineErr::WrongPiece)?;
            mv_st.active = true;
        }

        // If move was to past then clear the whole backward loop
        // and its moves
        if mv.to_past {
            let mut del_age = bf_piv.age + 1;
            let mut prev_piv = bf_piv;

            for (age, st) in wl.iter_mut().enumerate().skip(bf_piv.age + 1) {
                // As soon as state time is past action point
                // backward loop is finished
                if st.t > mv.t {
                    break;
                }

                // If it moved again then clear move and update undid piv
                if (st.x, st.y) != (ud_piv.sq.0, ud_piv.sq.1) {
                    self.move_history
                        .remove(&MoveId::from_piv(prev_piv))
                        .ok_or(EngineErr::WrongMove)?;

                    ud_piv = PieceView::from_state(st, bf_piv.origin, age);
                }

                // Update clear state border
                del_age = age;

                prev_piv = PieceView::from_state(st, bf_piv.origin, age);
            }

            // Clear backward loop states from worldline
            wl.drain((bf_piv.age + 1)..=del_age);
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
        if let Some(cast) = mv.castling {
            let (_old_sq, _new_sq, wid) = match cast {
                Castling::Long => match bf_piv.color {
                    Color::White => (Sq(0, 0), Sq(3, 0), 0),
                    Color::Black => (Sq(0, 7), Sq(3, 7), 16),
                },
                Castling::Short => match bf_piv.color {
                    Color::White => (Sq(7, 0), Sq(5, 0), 7),
                    Color::Black => (Sq(7, 7), Sq(5, 7), 23),
                },
            };

            let bf_rk = self
                .get_piv_necro(wid, bf_piv.age)
                .ok_or(EngineErr::WrongPiece)?;
            let ud_rk = self
                .get_piv_necro(wid, bf_piv.age + 1)
                .ok_or(EngineErr::WrongPiece)?;

            let wl = self.wlines.get_mut(wid).ok_or(EngineErr::WrongPiece)?;
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

        self.update_bufs();

        Ok(())
    }
}
