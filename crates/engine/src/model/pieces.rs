use super::{Move, MoveRights, State};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Color {
    White,
    Black,
}

impl Color {
    pub fn opposite(&self) -> Self {
        match self {
            Self::White => Self::Black,
            Self::Black => Self::White,
        }
    }

    pub fn from_board(board_id: usize) -> Self {
        if board_id.is_multiple_of(2) {
            Color::White
        } else {
            Color::Black
        }
    }

    pub fn pawn_dir(&self) -> i8 {
        match self {
            Color::White => 1,
            Color::Black => -1,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PieceKind {
    P { moved: bool },
    N,
    B,
    R { moved: bool },
    Q,
    K { moved: bool },
}

impl PieceKind {
    pub fn set_moved(self, moved: bool) -> Self {
        match self {
            PieceKind::P { .. } => PieceKind::P { moved },
            PieceKind::R { .. } => PieceKind::R { moved },
            PieceKind::K { .. } => PieceKind::K { moved },
            other => other,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PieceView {
    pub origin: usize, // wid
    pub age: usize,    // pid
    pub t: usize,
    pub fut_seen: usize,
    pub loop_turn: usize,
    pub kind: PieceKind,
    pub color: Color,
    pub sq: Sq,
    pub inverted: bool,
    pub rights: MoveRights,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Sq(pub usize, pub usize);

impl Sq {
    pub fn on_board(&self) -> bool {
        let Sq(x, y) = *self;
        x <= 7 && y <= 7
    }

    pub fn shift(&self, dx: i8, dy: i8) -> Option<Sq> {
        if !self.on_board() {
            return None;
        }
        let Sq(x, y) = *self;
        let next = Sq(
            x.checked_add_signed(isize::from(dx))?,
            y.checked_add_signed(isize::from(dy))?,
        );
        next.on_board().then_some(next)
    }
}

impl PieceView {
    pub fn from_state(st: &State, origin: usize, age: usize) -> Self {
        let State {
            x,
            y,
            t,
            fut_seen,
            kind,
            color,
            inverted,
            rights,
            loop_turn,
            ..
        } = *st;
        let sq = Sq(x, y);

        Self {
            origin,
            age,
            t,
            fut_seen,
            loop_turn,
            sq,
            kind,
            color,
            inverted,
            rights,
        }
    }

    pub fn knows_future(&self) -> bool {
        self.fut_seen > self.t
    }

    pub fn coinside(&self, piv: PieceView) -> bool {
        self.sq == piv.sq && self.t == piv.t
    }

    pub fn can_move(&self) -> bool {
        self.color == Color::from_board(self.t)
    }

    pub fn fix_with(&mut self, piv: PieceView) {
        self.sq = piv.sq;
        self.kind = piv.kind;
    }

    pub fn is_movable(&self) -> bool {
        self.rights.is_movable()
    }

    /// Only one piece of the same order can make a move,
    /// the order is defined by (fut_seen, loop_turn),
    /// but for safety we also check color.
    pub fn same_order(&self, piv: PieceView) -> bool {
        (self.color == piv.color)
            && (self.fut_seen == piv.fut_seen)
            && (self.loop_turn == piv.loop_turn)
    }

    /// Spatial simulation on the source board. Time and age are unchanged.
    pub fn apply_mv(&self, mv: Move) -> PieceView {
        let (sq, kind) = mv.piece_result(self.kind);
        Self { sq, kind, ..*self }
    }
}
