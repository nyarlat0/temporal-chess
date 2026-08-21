use crate::pieces::{Color, PieceView, Sq};

use super::{Castling, Engine, EngineErr, Move, MoveId};

impl Engine {
    pub fn apply_new_mv(&mut self, mv: Move) -> Result<(), EngineErr> {
        // Check that moved piece can be played
        let piv = self.get_piv_from_mv(mv).ok_or(EngineErr::WrongPiece)?;
        if !piv.active {
            return Err(EngineErr::WrongPiece);
        }

        // Deactivate pieces with the same (fut_seen, loop_turn)
        self.deact_same(piv)?;

        // Deactivate all optionally movable pieces and update bufs
        self.deact_opt(piv.color);

        // If this move is from the last accessible t
        // then prolongate the worldlines to build new board
        if piv.t + 1 == self.preboards.len() {
            self.prolongate_wlines();
        }

        // Undo old move if there is any
        let mv_id = MoveId::from_piv(piv);
        if let Some(old_mv) = self.move_history.get(&mv_id) {
            if old_mv.to_past && mv.to_past {
                self.fix_position(*old_mv, mv)?;
            } else {
                // Simle undo move only if not to_past -> to_past,
                // or else it would clear all backward loop
                self.undo_mv(mv_id)?;
            }
        }

        // Apply new position to the states after this until moved again
        let wl = self
            .wlines
            .get_mut(piv.origin)
            .ok_or(EngineErr::WrongPiece)?;
        for _st in wl[piv.t + 1..].iter_mut() {}

        Ok(())
    }

    pub fn fix_position(&mut self, old_mv: Move, mv: Move) -> Result<(), EngineErr> {
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

        // If old move was to past and new move isnt,
        // then clear the whole backward loop and its moves
        if old_mv.to_past && !mv.to_past {
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
        Ok(())
    }

    pub fn prolongate_wlines(&mut self) {
        let end_t = self.preboards.len().saturating_sub(1);
        for wl in self.wlines.iter_mut() {
            let last_st = wl.last().unwrap();
            if last_st.t == end_t && !last_st.inverted {
                let mut new_st = if Color::from_board(end_t + 1) == last_st.color {
                    last_st.clone()
                } else {
                    last_st.shallow_cp()
                };

                new_st.fut_seen += 1;
                new_st.t += 1;
                new_st.active = true;
                wl.push(new_st);
            }
        }
    }

    pub fn deact_same(&mut self, piv: PieceView) -> Result<(), EngineErr> {
        let board = self.boards.get(&piv.t).ok_or(EngineErr::AbsentBoard)?;
        for p in board.pieces.iter() {
            if p.active && p.fut_seen == piv.fut_seen && p.loop_turn == piv.loop_turn {
                self.wlines
                    .get_mut(p.origin)
                    .ok_or(EngineErr::WrongPiece)?
                    .get_mut(p.age)
                    .ok_or(EngineErr::WrongPiece)?
                    .active = false;
            }
        }

        Ok(())
    }

    pub fn deact_opt(&mut self, of: Color) {
        let until = match of {
            Color::White => self.action_lines.0.unwrap(),
            Color::Black => self.action_lines.1.unwrap(),
        };

        for wl in self.wlines.iter_mut() {
            for st in wl.iter_mut() {
                if st.t < until && st.active {
                    st.active = false;
                }
            }
        }
    }
}
