use crate::pieces::{Color, PieceKind};

use super::{Engine, EngineErr, ShortPieceId, ValidRes};

impl Engine {
    /// Validates moves of alive pieces till this turn action line,
    /// undoes invalid, removes them from history and updates buffers and action_lines.
    pub fn validate_moves(&mut self) -> Result<ValidRes, EngineErr> {
        let last_t = match self.turn {
            Color::White => self.action_lines.0.ok_or(EngineErr::NoMoves)?,
            Color::Black => self.action_lines.1.ok_or(EngineErr::NoMoves)?,
        };
        let mut res = ValidRes::Unchanged;

        for (mv_id, mv) in self
            .move_history
            .clone()
            .iter()
            .filter(|(id, _mv)| id.t <= last_t)
        {
            let ShortPieceId { wid, age } = *self.pbuf.get(&mv.pid).ok_or(EngineErr::WrongPiece)?;
            if self
                .wlines
                .get(wid)
                .ok_or(EngineErr::WrongPiece)?
                .get(age)
                .ok_or(EngineErr::WrongPiece)?
                .alive
                && !self.possible_moves(wid, age)?.contains(mv)
            {
                res = ValidRes::HistChanged;
                self.undo_mv(*mv_id)?;
            }
        }

        Ok(res)
    }

    pub fn mass_ressurection(&mut self) -> Result<ValidRes, EngineErr> {
        let mut out = ValidRes::Unchanged;

        for wl in self.wlines.iter_mut() {
            let mut res_line = true;

            for st in wl.iter_mut() {
                if !st.alive {
                    let pb = self.preboards.get(&st.t).ok_or(EngineErr::AbsentBoard)?;

                    for piv in pb.space[st.x][st.y].iter() {
                        if (piv.sq.0, piv.sq.1) == (st.x, st.y)
                            || (matches!(piv.kind, PieceKind::P { .. })
                                && match st.color {
                                    Color::White => (piv.sq.0, piv.sq.1 + 1) == (st.x, st.y),
                                    Color::Black => (piv.sq.0, piv.sq.1) == (st.x, st.y + 1),
                                })
                        {
                            res_line = false;
                        }
                    }
                    if res_line {
                        out = ValidRes::HistChanged;
                        st.alive = true;
                    }
                }
            }
        }

        Ok(out)
    }

    pub fn global_validation(&mut self) -> Result<(), EngineErr> {
        let mut pb_buf = vec![];

        loop {
            // Stage 0 -- compare and save engine state

            if pb_buf.contains(&self.preboards) {
                // If state of engine repeats then throw infinite loop err
                return Err(EngineErr::InfLoop);
            } else {
                // Else save new state
                pb_buf.push(self.preboards.clone());
            }

            // Stage 1 -- build boards

            self.build_boards()?;

            // Stage 2 -- validate moves

            match self.validate_moves()? {
                ValidRes::HistChanged => continue,
                ValidRes::Unchanged => (),
            }

            // Stage 3 -- ressurect pieces

            match self.mass_ressurection()? {
                ValidRes::HistChanged => continue,
                ValidRes::Unchanged => (),
            }

            // Stage 4 -- update info of all pieces BUT leave action lines unchanged
            // action lines will only point to the board on which you MUST make a move

            self.update_infos();
            break;
        }

        Ok(())
    }
}
