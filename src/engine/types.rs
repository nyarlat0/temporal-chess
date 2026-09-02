use std::collections::HashMap;

use arrayvec::ArrayVec;

use crate::board::{Board, PreBoard};
use crate::pieces::{Color, PieceKind, PieceView, Sq};

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
    // furthest seen t
    pub fut_seen: usize,
    // number of time-turnes. Its incremented +1 when changing time orientation
    // and decremented -2 when passing (t > t_loop) loop pivot
    pub loop_turn: usize,
    pub alive: bool,
    // piece is activated when it moves in time to a new board
    pub active: bool,
    // piece can react if its information changes, meaning it can attack more pieces
    pub can_react: bool,
    // piece can re-move if its move was cancelled
    pub move_cancelled: bool,
    pub kind: PieceKind,
    pub color: Color,
    pub inverted: bool,
    pub informed: ArrayVec<PieceId, 8>,
    // points in time, on which piece inverted to and began moving to past.
    pub loop_pivots: Vec<usize>,
}

/// Long-term identification of a move
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct MoveId {
    pub(super) t: usize,
    pub(super) fut_seen: usize,
    pub(super) loop_turn: usize,
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
    pub(super) pivs: Vec<PieceView>,
}

#[derive(Clone, Copy)]
pub struct PieceCollision {
    pub(super) bf_collider: PieceView,
    pub(super) collider: PieceView,
    pub(super) collided: PieceView,
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
            can_react: self.can_react,
            move_cancelled: self.move_cancelled,
            kind: self.kind,
            color: self.color,
            inverted: self.inverted,
            informed: ArrayVec::new(),
            loop_pivots: self.loop_pivots.clone(),
        }
    }

    pub fn is_movable(&self) -> bool {
        self.active || self.can_react || self.move_cancelled
    }

    /// If the piece can react to changes in history - new info or its move cancel
    pub fn is_react_movable(&self) -> bool {
        self.can_react || self.move_cancelled
    }

    pub fn make_unmovable(&mut self) {
        self.active = false;
        self.can_react = false;
        self.move_cancelled = false;
    }

    pub fn make_unrectable(&mut self) {
        self.can_react = false;
        self.move_cancelled = false;
    }

    /// Used for applying new move to the same time-oriented pieces
    pub fn apply_mv(&mut self, mv: Move) {
        // Apply position
        self.x = mv.sq.0;
        self.y = mv.sq.1;

        // Piece-dependent state changes
        match &mut self.kind {
            PieceKind::P { moved } => {
                *moved = true;

                if let Some(new_kind) = mv.promotion {
                    self.kind = new_kind;
                };
            }
            PieceKind::R { moved } => {
                *moved = true;

                if let Some(castl) = mv.castling {
                    let dx_from_king = match castl {
                        // to the left from king
                        Castling::Short => -1,
                        // to the right from king
                        Castling::Long => 1,
                    };
                    self.x = (self.x as i8 + dx_from_king) as usize;
                }
            }
            PieceKind::K { moved } => {
                *moved = true;
            }
            _ => {}
        }
    }

    /// Creates a new state from the last in the world line,
    /// OR from last in the time-loop
    pub fn prolongate(&self) -> Self {
        let mut new_st = self.shallow_cp();

        // If its next board of the same color then make it active
        new_st.active = Color::from_board(self.t + 1) == self.color;

        // Moves backward in time if inverted and forward if isnt
        if new_st.inverted {
            new_st.t = new_st.t.saturating_sub(1);
        } else {
            new_st.t += 1;
        }

        new_st.fut_seen = new_st.fut_seen.max(new_st.t);

        // If state is past pivot t then decrease loop_turn
        if new_st
            .loop_pivots
            .pop_if(|pivot_t| new_st.t > *pivot_t)
            .is_some()
        {
            new_st.loop_turn = new_st.loop_turn.saturating_sub(2);
        }

        new_st
    }
}
