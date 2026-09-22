//! Values shared by the timeline, board projection and move rules.
//!
//! `t` is board time; `age` is an index along a piece's own worldline.
//! Age increases even while time runs backwards.

use std::collections::BTreeSet;

mod pieces;

pub use pieces::{Color, PieceKind, PieceView, Sq};

/// Independent reasons why an occurrence may act. Expiring reactions never
/// consumes an ordinary move; only playing in its causal slot consumes that.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MoveRights {
    pub ordinary: bool,
    pub new_information: bool,
    pub cancelled_move: bool,
}

impl MoveRights {
    pub fn is_movable(self) -> bool {
        self.ordinary || self.is_reaction()
    }
    pub fn is_reaction(self) -> bool {
        self.new_information || self.cancelled_move
    }
    pub fn expire_reactions(&mut self) {
        self.new_information = false;
        self.cancelled_move = false;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
    pub rights: MoveRights,
    pub kind: PieceKind,
    pub color: Color,
    pub inverted: bool,
    pub informed: BTreeSet<PieceId>,
    // points in time, on which piece inverted to and began moving to past.
    pub loop_pivots: Vec<usize>,
}

/// A move slot shared by pieces of the same causal order on one board.
/// No worldline ID: one piece's reaction can replace another piece's move.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MoveId {
    pub t: usize,
    pub fut_seen: usize,
    pub loop_turn: usize,
}

/// Long-term identification of a piece
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PieceId {
    pub wid: usize,
    pub fut_seen: usize,
    pub loop_turn: usize,
}

/// A location in a worldline. Invalidated when earlier states are removed.
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

/// Direction selected for this move, independently of the previous move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeDirection {
    Future,
    Past,
}

impl TimeDirection {
    pub fn from_inverted(inverted: bool) -> Self {
        if inverted { Self::Past } else { Self::Future }
    }

    pub fn is_inverted(self) -> bool {
        self == Self::Past
    }

    pub fn step(self, t: usize) -> Option<usize> {
        match self {
            Self::Future => t.checked_add(1),
            Self::Past => t.checked_sub(1),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Promotion {
    Knight,
    Bishop,
    Rook,
    Queen,
}

impl Promotion {
    pub const ALL: [Self; 4] = [Self::Knight, Self::Bishop, Self::Rook, Self::Queen];

    pub fn piece_kind(self) -> PieceKind {
        match self {
            Self::Knight => PieceKind::N,
            Self::Bishop => PieceKind::B,
            Self::Rook => PieceKind::R { moved: true },
            Self::Queen => PieceKind::Q,
        }
    }
}

/// Special moves are mutually exclusive; promotion to a king is unrepresentable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveEffect {
    Normal,
    Castle(Castling),
    EnPassant { captured: Sq },
    Promote(Promotion),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Move {
    pub pid: PieceId,
    pub t: usize,
    pub sq: Sq,
    pub direction: TimeDirection,
    pub effect: MoveEffect,
}

/// An occurrence of a causal incarnation on a particular board.
/// `PieceId` alone repeats at several times inside an inverted worldline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PieceAt {
    pub piece: PieceId,
    pub t: usize,
}

impl Move {
    pub fn to_past(self) -> bool {
        self.direction.is_inverted()
    }
    pub fn castling(self) -> Option<Castling> {
        match self.effect {
            MoveEffect::Castle(side) => Some(side),
            _ => None,
        }
    }
    pub fn en_passant(self) -> Option<Sq> {
        match self.effect {
            MoveEffect::EnPassant { captured } => Some(captured),
            _ => None,
        }
    }
    pub fn promotion(self) -> Option<PieceKind> {
        match self.effect {
            MoveEffect::Promote(kind) => Some(kind.piece_kind()),
            _ => None,
        }
    }

    /// Shared spatial transformation for real states and board simulations.
    /// The caller handles temporal transitions and the other castling piece.
    pub(crate) fn piece_result(self, kind: PieceKind) -> (Sq, PieceKind) {
        let mut sq = self.sq;
        let mut next_kind = kind.set_moved(true);
        if matches!(kind, PieceKind::P { .. }) {
            if let Some(promotion) = self.promotion() {
                next_kind = promotion;
            }
        } else if matches!(kind, PieceKind::R { .. })
            && let Some(side) = self.castling()
        {
            sq = Sq(
                match side {
                    Castling::Long => 3,
                    Castling::Short => 5,
                },
                self.sq.1,
            );
        }
        (sq, next_kind)
    }

    pub fn source(self) -> PieceAt {
        PieceAt {
            piece: self.pid,
            t: self.t,
        }
    }

    pub fn id(self) -> MoveId {
        MoveId {
            t: self.t,
            fut_seen: self.pid.fut_seen,
            loop_turn: self.pid.loop_turn,
        }
    }
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
    pub fn new(kind: PieceKind, color: Color, sq: Sq) -> Self {
        Self {
            t: 0,
            x: sq.0,
            y: sq.1,
            fut_seen: 0,
            loop_turn: 0,
            alive: true,
            rights: MoveRights {
                ordinary: color == Color::White,
                ..MoveRights::default()
            },
            kind,
            color,
            inverted: false,
            informed: BTreeSet::new(),
            loop_pivots: Vec::new(),
        }
    }

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
            rights: self.rights,
            kind: self.kind,
            color: self.color,
            inverted: self.inverted,
            informed: BTreeSet::new(),
            loop_pivots: self.loop_pivots.clone(),
        }
    }

    pub fn is_movable(&self) -> bool {
        self.rights.is_movable()
    }

    /// If the piece can react to changes in history - new info or its move cancel
    pub fn is_react_movable(&self) -> bool {
        self.rights.is_reaction()
    }

    pub fn make_unmovable(&mut self) {
        self.rights = MoveRights::default();
    }

    pub fn expire_reactions(&mut self) {
        self.rights.expire_reactions();
    }

    /// Used for applying new move to the same time-oriented pieces
    pub fn apply_mv(&mut self, mv: Move) {
        let (square, kind) = mv.piece_result(self.kind);
        self.x = square.0;
        self.y = square.1;
        self.kind = kind;
    }

    /// Creates a new state from the last in the world line,
    /// OR from last in the time-loop
    pub fn prolongate(&self) -> Option<Self> {
        let mut new_st = self.shallow_cp();

        // Moves backward in time if inverted and forward if isnt
        new_st.t = TimeDirection::from_inverted(new_st.inverted).step(new_st.t)?;

        new_st.rights = MoveRights {
            ordinary: new_st.alive && Color::from_board(new_st.t) == new_st.color,
            ..MoveRights::default()
        };

        new_st.fut_seen = new_st.fut_seen.max(new_st.t);

        // If state is past pivot t then decrease loop_turn
        while new_st
            .loop_pivots
            .pop_if(|pivot_t| new_st.t > *pivot_t)
            .is_some()
        {
            new_st.loop_turn = new_st.loop_turn.saturating_sub(2);
        }

        Some(new_st)
    }
}
