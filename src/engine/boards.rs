use std::collections::HashMap;

use crate::board::{Board, PreBoard};
use crate::pieces::{Color, PieceView};

use super::{BoardConflict, BoardErr, Engine, EngineErr, PieceId, ShortPieceId};

impl Engine {
    pub fn make_board(&self, t: usize) -> Result<Board, BoardErr> {
        let mut pieces = Vec::new();
        let mut occ = [[None; 8]; 8];

        let preboard = self.preboards.get(&t).ok_or(BoardErr::InvalidTime)?;

        for (y, row) in preboard.space.iter().enumerate() {
            for (x, prep) in row.iter().enumerate() {
                if prep.len() > 1 {
                    return Err(BoardErr::InvalidBoard(BoardConflict { pivs: prep.clone() }));
                } else {
                    if let Some(p) = prep.first() {
                        let idx = pieces.len();
                        pieces.push(*p);
                        occ[x][y] = Some(idx);
                    }
                }
            }
        }

        Ok(Board {
            pieces,
            occ,
            age: t,
        })
    }

    pub fn build_boards(&mut self) -> Result<(), EngineErr> {
        let mut boards = HashMap::new();
        let mut t = 0;
        let mut pb_buf = vec![self.preboards.clone()];

        while let Some(pb) = self.preboards.get(&t) {
            let last_t = match self.turn {
                Color::White => self.action_lines.0.ok_or(EngineErr::NoMoves)?,
                Color::Black => self.action_lines.1.ok_or(EngineErr::NoMoves)?,
            };

            if t > last_t {
                break;
            }

            if pb.is_valid() {
                //
                // If board is valid then push it and go forward
                //
                let brd = pb.try_into_board().ok_or(EngineErr::AbsentBoard)?;
                boards.insert(t, brd);
                t += 1;
            } else {
                //
                // Else fix one conflict on the board
                //
                if let Err(BoardErr::InvalidBoard(bc)) = self.make_board(t) {
                    self.try_fix_conflict(bc)?;
                }
                if pb_buf.contains(&self.preboards) {
                    // If state of engine repeats then throw infinite loop err
                    return Err(EngineErr::InfLoop);
                } else {
                    // Else save new state and start boards calculation anew
                    pb_buf.push(self.preboards.clone());
                    t = 0;
                }
            }
        }

        self.boards = boards;
        Ok(())
    }

    /// Updates info for pieces playable this turn,
    /// makes figures with changed info active but doesnt change action_lines,
    /// so this pieces are only optionally movable
    /// and will become inactive again after the turn changes.
    pub fn update_infos(&mut self) {
        let oddity = match self.turn {
            Color::White => 0,
            Color::Black => 1,
        };

        for (t, board) in self.boards.iter_mut() {
            if t % 2 != oddity {
                continue;
            }

            // Optional past means that en passant moves
            // to future will be calculted in the info
            //
            // Optional future means that en passant moves
            // to past will be calculted in the info
            let (opt_past, opt_fut) = if *t > 1 {
                // if there is enough time for inverted en pass
                // then get both opt_past and opt_fut
                (
                    self.preboards.get(&t.saturating_sub(1)),
                    self.preboards.get(&(t + 1)),
                )
            } else if *t == 1 {
                // if there is past, but no time for inverted en pass
                // then leave opt_fut empty
                (self.preboards.get(&t.saturating_sub(1)), None)
            } else {
                // If no past and no time for inverted en pass
                // then leave both empty
                (None, None)
            };
            let infos = board.calc_info(opt_past, opt_fut);

            // Make pieces with changed info active again and NOT update action_lines
            for (shpid, info) in infos {
                let st = self
                    .wlines
                    .get_mut(shpid.wid)
                    .unwrap()
                    .get_mut(shpid.age)
                    .unwrap();

                if !st.active && info != st.informed {
                    st.active = true;
                    board.get_mut_piece(st.x, st.y).unwrap().active = true;
                    self.preboards
                        .get_mut(&st.t)
                        .unwrap()
                        .get_mut_piv(shpid.wid, shpid.age)
                        .unwrap()
                        .active = true;
                    /*
                    match st.color {
                        Color::White => {
                            self.action_lines.0 =
                                Some(self.action_lines.0.map_or(st.t, |l| l.min(st.t)))
                        }
                        Color::Black => {
                            self.action_lines.1 =
                                Some(self.action_lines.1.map_or(st.t, |l| l.min(st.t)))
                        }
                    }
                    */
                }
                st.informed = info;
            }
        }
    }

    /// Cycles through all world lines
    /// builds preboards and fills piece buffer,
    /// updates action lines.
    ///
    /// Preboards contain only alive pieces,
    /// but pbuf contains all,
    pub fn update_bufs(&mut self) {
        let mut pbs = HashMap::<usize, PreBoard>::new();
        let mut pbuf = HashMap::<PieceId, ShortPieceId>::new();
        let mut actl = (None::<usize>, None::<usize>);

        for (origin, wline) in self.wlines.iter().enumerate() {
            for (age, state) in wline.iter().enumerate() {
                // push to preboards only alive pieces
                if state.alive {
                    // if state is active update the action line
                    if state.active {
                        match state.color {
                            Color::White => {
                                actl.0 = Some(actl.0.map_or(state.t, |l| l.min(state.t)))
                            }
                            Color::Black => {
                                actl.1 = Some(actl.1.map_or(state.t, |l| l.min(state.t)))
                            }
                        }
                    }

                    if let Some(pb) = pbs.get_mut(&state.t) {
                        pb.space[state.x][state.y].push(PieceView::from_state(state, origin, age));
                    } else {
                        let mut pb = PreBoard::new(state.t);
                        pb.space[state.x][state.y].push(PieceView::from_state(state, origin, age));
                        pbs.insert(state.t, pb);
                    }
                }
                // push to pbuf indifferently
                pbuf.insert(
                    PieceId::from_state(state, origin),
                    ShortPieceId { wid: origin, age },
                );
            }
        }

        self.preboards = pbs;
        self.pbuf = pbuf;
        self.action_lines = actl;
    }
}
