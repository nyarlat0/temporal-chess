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
            kind: self.kind,
            color: self.color,
            inverted: self.inverted,
            informed: ArrayVec::new(),
        }
    }
}
