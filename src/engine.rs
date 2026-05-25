use std::{cmp::min, collections::HashMap};

use arrayvec::ArrayVec;

use crate::board::*;
use crate::pieces::*;

#[derive(Clone)]
pub struct Engine {
    pub wlines: Vec<Vec<State>>, // wid -> age -> state
    pub move_history: HashMap<MoveId, Move>,
    pub preboards: HashMap<usize, PreBoard>,
    pub boards: HashMap<usize, Board>,
    pub pbuf: HashMap<PieceId, ShortPieceId>, // pieceid -> (wid, age)
    pub turn: Color,
    pub action_lines: (Option<usize>, Option<usize>),
}

#[derive(Clone)]
pub struct State {
    pub t: usize,
    pub x: usize,
    pub y: usize,
    pub fut_seen: usize,
    pub loop_turn: usize,
    pub alive: bool,
    pub active: bool,
    pub kind: PieceKind,
    pub color: Color,
    pub inverted: bool,
    pub informed: ArrayVec<PieceId, 8>,
}

/// Long-term identification of a move
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct MoveId {
    t: usize,
    fut_seen: usize,
    loop_turn: usize,
}

/// Long-term identification of a piece
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PieceId {
    pub wid: usize,
    pub fut_seen: usize,
    pub loop_turn: usize,
}

/// Short-term identification of a piece
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShortPieceId {
    pub wid: usize,
    pub age: usize,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Castling {
    Long,
    Short,
}

#[derive(Clone, Copy, PartialEq)]
pub struct Move {
    pub pid: PieceId,
    pub t: usize,
    pub sq: Sq,
    pub to_past: bool,
    pub castling: Option<Castling>,
    pub en_passant: Option<Sq>,
    pub promotion: Option<PieceKind>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ValidRes {
    HistChanged,
    Unchanged,
}

#[derive(Clone)]
pub enum EngineErr {
    WrongPiece,
    AbsentBoard,
    WrongMove,
    InfLoop,
    NoMoves,
    BoardErr(BoardErr),
}

#[derive(Clone)]
pub enum BoardErr {
    InvalidTime,
    InvalidBoard(BoardConflict),
}

#[derive(Clone)]
pub struct BoardConflict {
    pivs: Vec<PieceView>,
}

#[derive(Clone, Copy)]
pub struct PieceCollision {
    bf_collider: PieceView,
    collider: PieceView,
    collided: PieceView,
}

impl PieceId {
    pub fn from_state(st: &State, wid: usize) -> Self {
        Self {
            wid,
            fut_seen: st.fut_seen,
            loop_turn: st.loop_turn,
        }
    }
    pub fn from_piv(piv: PieceView) -> Self {
        Self {
            wid: piv.origin,
            fut_seen: piv.fut_seen,
            loop_turn: piv.loop_turn,
        }
    }
}

impl ShortPieceId {
    pub fn from_piv(piv: &PieceView) -> Self {
        Self {
            wid: piv.origin,
            age: piv.age,
        }
    }
}

impl MoveId {
    pub fn from_piv(piv: PieceView) -> Self {
        Self {
            t: piv.t,
            fut_seen: piv.fut_seen,
            loop_turn: piv.loop_turn,
        }
    }

    pub fn from_state(st: &State) -> Self {
        Self {
            t: st.t,
            fut_seen: st.fut_seen,
            loop_turn: st.loop_turn,
        }
    }
}

impl State {
    /// Copies everything besides info,
    /// should be used only for boards of opposite color.
    pub fn shallow_cp(&self) -> Self {
        Self {
            t: self.t,
            x: self.x,
            y: self.y,
            fut_seen: self.fut_seen,
            loop_turn: self.loop_turn,
            alive: self.alive,
            active: self.active,
            kind: self.kind,
            color: self.color,
            inverted: self.inverted,
            informed: ArrayVec::new(),
        }
    }
}

impl Default for Engine {
    fn default() -> Self {
        // Board coordinate system:
        // (x, y) with x=0..7 left->right, y=0..7 bottom->top.
        // White starts at y=0/1, Black at y=7/6.

        let mut wlines = Vec::new();
        let pbuf = HashMap::new();

        // Helper: create initial state at t=0, fut_seen=0, loop_turn=0.
        let mut spawn = |kind: PieceKind, color: Color, x: usize, y: usize| {
            let st = State {
                t: 0,
                x,
                y,
                fut_seen: 0,
                loop_turn: 0,
                alive: true,
                active: true,
                kind,
                color,
                inverted: false,
                informed: ArrayVec::new(),
            };

            // one worldline per piece, age 0 only
            wlines.push(vec![st]);
        };

        // --- WHITE ---
        let w = Color::White;

        // rank 0 (y=0)
        spawn(PieceKind::R { moved: false }, w, 0, 0);
        spawn(PieceKind::N, w, 1, 0);
        spawn(PieceKind::B, w, 2, 0);
        spawn(PieceKind::Q, w, 3, 0);
        spawn(PieceKind::K { moved: false }, w, 4, 0);
        spawn(PieceKind::B, w, 5, 0);
        spawn(PieceKind::N, w, 6, 0);
        spawn(PieceKind::R { moved: false }, w, 7, 0);

        // rank 1 (y=1)
        for x in 0..8 {
            spawn(PieceKind::P { moved: false }, w, x, 1);
        }

        // --- BLACK ---
        let b = Color::Black;

        // rank 7 (y=7)
        spawn(PieceKind::R { moved: false }, b, 0, 7);
        spawn(PieceKind::N, b, 1, 7);
        spawn(PieceKind::B, b, 2, 7);
        spawn(PieceKind::Q, b, 3, 7);
        spawn(PieceKind::K { moved: false }, b, 4, 7);
        spawn(PieceKind::B, b, 5, 7);
        spawn(PieceKind::N, b, 6, 7);
        spawn(PieceKind::R { moved: false }, b, 7, 7);

        // rank 6 (y=6)
        for x in 0..8 {
            spawn(PieceKind::P { moved: false }, b, x, 6);
        }

        Self {
            wlines,
            move_history: HashMap::new(),
            preboards: HashMap::new(),
            boards: HashMap::new(),
            pbuf,
            turn: Color::White,
            action_lines: (Some(0), None),
        }
    }
}

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

    /// Returns error if piece is dead!!!
    pub fn possible_moves(&self, wid: usize, age: usize) -> Result<Vec<Move>, EngineErr> {
        let piv = self.get_piv(wid, age).ok_or(EngineErr::WrongPiece)?;
        let pid = PieceId::from_piv(piv);
        let t = piv.t;
        let pres_board = self.boards.get(&piv.t).ok_or(EngineErr::AbsentBoard)?;
        let mut mvs = Vec::new();

        // Basic moves to future and to past
        let basic_sqs = piv.basic_moves(pres_board);
        for sq in basic_sqs {
            mvs.push(Move {
                pid,
                t,
                sq,
                to_past: false,
                castling: None,
                en_passant: None,
                promotion: None,
            });

            if piv.t > 1 {
                mvs.push(Move {
                    pid,
                    t,
                    sq,
                    to_past: true,
                    castling: None,
                    en_passant: None,
                    promotion: None,
                });
            }
        }

        // Castiling moves if any
        let cast_mvs = piv.castling_moves(pres_board);
        for (sq, cast) in cast_mvs {
            mvs.push(Move {
                pid,
                t,
                sq,
                to_past: false,
                castling: Some(cast),
                en_passant: None,
                promotion: None,
            });
        }

        // En passant moves to future
        if piv.t > 0 {
            let past_pboard = self
                .preboards
                .get(&piv.t.saturating_sub(1))
                .ok_or(EngineErr::AbsentBoard)?;
            let enpass_sqs = piv.en_passant_moves(pres_board, past_pboard);

            for (sq, eat_sq) in enpass_sqs {
                mvs.push(Move {
                    pid,
                    t,
                    sq,
                    to_past: false,
                    castling: None,
                    en_passant: Some(eat_sq),
                    promotion: None,
                });
            }
        }

        // En passant moves to past
        if piv.t > 1
            && let Some(fut_pboard) = self.preboards.get(&(piv.t + 1))
        {
            let enpass_sqs = piv.en_passant_moves(pres_board, fut_pboard);

            for (sq, eat_sq) in enpass_sqs {
                mvs.push(Move {
                    pid,
                    t,
                    sq,
                    to_past: true,
                    castling: None,
                    en_passant: Some(eat_sq),
                    promotion: None,
                });
            }
        }

        // Check for promotion if pawn
        if matches!(piv.kind, PieceKind::P { .. }) {
            let mut prom_mvs = Vec::new();
            for mv in mvs.iter() {
                let last_y = match piv.color {
                    Color::White => 7,
                    Color::Black => 0,
                };
                if mv.sq.1 == last_y {
                    let mut new_mv = *mv;
                    use PieceKind::*;
                    for kind in [N, B, R { moved: true }, Q] {
                        new_mv.promotion = Some(kind);
                        prom_mvs.push(new_mv);
                    }
                }
            }
            mvs.append(&mut prom_mvs);
        }

        // If there is an attack on king in present,
        // then leave only moves that protect king
        if let Some(king_piv) = pres_board
            .pieces
            .iter()
            .find(|p| p.color == piv.color && matches!(p.kind, PieceKind::K { .. }))
        {
            let mut pretend_board = pres_board.clone();
            pretend_board.remove_piece(piv.sq);

            // If on board without this piv
            // king isnt attacked, then it wont be no matter the move,
            // unless piece is a pawn that does en passant
            if pretend_board.is_attacked(piv.color.opposite(), king_piv.sq)
                || matches!(piv.kind, PieceKind::P { .. })
            {
                let mut rm_ids = Vec::new();

                for (mv_id, mv) in mvs.iter().enumerate() {
                    // Create new temporary pretend board
                    // for killing and moving the pieces
                    let mut temp_pret_board = pretend_board.clone();

                    // Remove eaten piece
                    if temp_pret_board.is_enemy(piv.color, mv.sq) {
                        temp_pret_board.remove_piece(mv.sq);
                    }
                    if let Some(eating_sq) = mv.en_passant {
                        temp_pret_board.remove_piece(eating_sq);
                    }

                    // Move pretend piv and add it to board
                    let mut pretend_piv = piv;
                    pretend_piv.sq = mv.sq;
                    temp_pret_board.add_piece(pretend_piv);

                    if temp_pret_board.is_attacked(piv.color.opposite(), king_piv.sq) {
                        rm_ids.push(mv_id);
                    }
                }

                let mut i = 0;
                mvs.retain(|_| {
                    let to_keep = !rm_ids.contains(&i);
                    i += 1;

                    to_keep
                });
            }
        }

        Ok(mvs)
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
        for st in wl[piv.t + 1..].iter_mut() {}

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

    /// Conflict is any intersection of n pieces.
    /// Is fixed by separating pieces to pairs,
    /// tracking each pair collision (root of conflict) and fixing it.
    /// This function only fixes one collision and updates global state.
    pub fn try_fix_conflict(&mut self, mut bc: BoardConflict) -> Result<(), EngineErr> {
        // Sort by future seen for casuality
        bc.pivs.sort_by_key(|a| a.fut_seen);

        // Get pairs of conflicting pieces
        let piv = *bc.pivs.first().ok_or(EngineErr::WrongPiece)?;
        let other_piv = *bc.pivs.get(1).ok_or(EngineErr::WrongPiece)?;

        let col = self
            .track_collision(piv, other_piv)
            .ok_or(EngineErr::WrongPiece)?;

        let _prev_survived = Some(self.fix_collision(col, piv.t)?);

        Ok(())
    }

    /// Collision is the root of intersection of figures (aka conflict).
    /// Collision time is the first conflict time.
    /// Method undoes move or kills piece, returns the piece that stays unchanged.
    /// Gets conflict time as input to return correct survivor in case of mutual destruction.
    pub fn fix_collision(
        &mut self,
        col: PieceCollision,
        conf_t: usize,
    ) -> Result<PieceView, EngineErr> {
        let PieceCollision {
            bf_collider,
            collider,
            collided,
        } = col;
        let surv: PieceView;

        // Pieces of the same side
        if collider.color == collided.color {
            // In oncoming collision
            // collider = ordinary,
            // collided = inverted
            if collider.inverted != collided.inverted {
                // Inverted kills ordinary on board of its color
                if Color::from_board(collider.t) == collider.color {
                    self.kill_piece(collider.origin, collider.age)?;
                    surv = collided;
                } else {
                    // Else undo inverted move
                    self.undo_last_mv(collided)?;
                    surv = collider;
                }
            } else {
                // In co-directional undo collider move
                self.undo_last_mv(collider)?;
                surv = collided;
            }
        // Enemy pieces
        } else {
            // In oncoming collision
            // collider = ordinary,
            // collided = inverted
            if collider.inverted != collided.inverted {
                // If ordinary moved and knew future before collision
                // then it kills inverted in collision board,
                // but is killed after collision board
                if bf_collider.sq != collider.sq && bf_collider.knows_future() {
                    self.kill_piece(collided.origin, collided.age)?;
                    self.kill_piece(collider.origin, collider.age + 1)?;

                    if conf_t == collider.t {
                        surv = collider;
                    } else {
                        surv = collided;
                    }
                } else {
                    // Else just kill ordinary
                    self.kill_piece(collider.origin, collider.age)?;
                    surv = collided;
                }
            } else {
                // Else collided is killed
                self.kill_piece(collided.origin, collided.age)?;
                surv = collider;
            }
        }

        Ok(surv)
    }

    /// Collision is the root of intersection of figures (aka conflict).
    /// Collision time is the first conflict time.
    /// If oncoming (inverted vs ordinary) collision,
    /// always return ordinary as collider and inv as collided.
    pub fn track_collision(&self, piv: PieceView, other_piv: PieceView) -> Option<PieceCollision> {
        // First checked for sq change is piv with
        // most future seen in case both figures move
        // at the same time
        let (piv, other_piv) = if piv.fut_seen >= other_piv.fut_seen {
            (piv, other_piv)
        } else {
            (other_piv, piv)
        };

        // Oncoming collision can have some exotic situations
        let oncoming = piv.inverted != other_piv.inverted;

        if !oncoming {
            let mut prev_piv = piv;
            let mut prev_other_piv = other_piv;

            for d in 1..=min(piv.age, other_piv.age) {
                let piv_bf = self.get_piv(piv.origin, piv.age.saturating_sub(d))?;
                let other_piv_bf =
                    self.get_piv(other_piv.origin, other_piv.age.saturating_sub(d))?;

                if piv_bf.sq != piv.sq {
                    let bf_collider = piv_bf;
                    let collider = prev_piv;
                    let collided = prev_other_piv;
                    return Some(PieceCollision {
                        bf_collider,
                        collider,
                        collided,
                    });
                }

                if other_piv_bf.sq != other_piv.sq {
                    let bf_collider = other_piv_bf;
                    let collider = prev_other_piv;
                    let collided = prev_piv;
                    return Some(PieceCollision {
                        bf_collider,
                        collider,
                        collided,
                    });
                }

                prev_piv = piv_bf;
                prev_other_piv = other_piv_bf;
            }
        } else {
            let (inv, ord) = if piv.inverted {
                (piv, other_piv)
            } else {
                (other_piv, piv)
            };

            let mut prev_inv = inv;
            let mut prev_ord = ord;

            for d in 1..=ord.age {
                let ord_bf = self.get_piv(piv.origin, piv.age.saturating_sub(d))?;
                let mb_inv_aft = self.get_piv(other_piv.origin, other_piv.age + d);

                if let Some(inv_aft) = mb_inv_aft {
                    if ord_bf.sq != ord.sq {
                        let bf_collider = ord_bf;
                        let collider = prev_ord;
                        let collided = prev_inv;
                        return Some(PieceCollision {
                            bf_collider,
                            collider,
                            collided,
                        });
                    }

                    if inv_aft.sq != inv.sq {
                        let bf_collider = ord_bf;
                        let collider = prev_ord;
                        let collided = prev_inv;
                        return Some(PieceCollision {
                            bf_collider,
                            collider,
                            collided,
                        });
                    }

                    prev_ord = ord_bf;
                    prev_inv = inv_aft;
                } else {
                    // If inverted isnt present its just before collision
                    let bf_collider = ord_bf;
                    let collider = prev_ord;
                    let collided = prev_inv;
                    return Some(PieceCollision {
                        bf_collider,
                        collider,
                        collided,
                    });
                }
            }
        }

        None
    }

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
    pub fn undo_mv(&mut self, mvid: MoveId) -> Result<(), EngineErr> {
        // Remove old move
        let mv = self
            .move_history
            .remove(&mvid)
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
