use crate::engine::{Castling, Move, State};

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

    pub const PROM: [PieceKind; 5] = [
        PieceKind::N,
        PieceKind::B,
        PieceKind::R { moved: true },
        PieceKind::Q,
        PieceKind::K { moved: true },
    ];
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
    // piece is activated when it moves in time to a new board
    pub active: bool,
    // piece can react if its information changes, meaning it can attack more pieces
    pub can_react: bool,
    // piece can re-move if its move was cancelled
    pub move_cancelled: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Sq(pub usize, pub usize);

impl Sq {
    pub fn on_board(&self) -> bool {
        let Sq(x, y) = *self;
        x <= 7 && y <= 7
    }

    pub fn shift(&self, dx: i8, dy: i8) -> Option<Sq> {
        let Sq(x, y) = *self;
        let (nx, ny) = (x as i8 + dx, y as i8 + dy);

        if (0..=7).contains(&nx) && (0..=7).contains(&ny) {
            Some(Sq(nx as usize, ny as usize))
        } else {
            None
        }
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
            active,
            can_react,
            move_cancelled,
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
            active,
            can_react,
            move_cancelled,
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
        self.active || self.can_react || self.move_cancelled
    }

    /// Only one piece of the same order can make a move,
    /// the order is defined by (fut_seen, loop_turn),
    /// but for safety we also check color.
    pub fn same_order(&self, piv: PieceView) -> bool {
        (self.color == piv.color)
            && (self.fut_seen == piv.fut_seen)
            && (self.loop_turn == piv.loop_turn)
    }

    /// Used for pretending a move only
    pub fn apply_mv(&self, mv: Move) -> PieceView {
        let mut new_piv = *self;
        // Apply new position
        new_piv.sq = mv.sq;
        // Apply time-polarity
        new_piv.inverted = mv.to_past;
        // Make piece older
        new_piv.age += 1;

        // Piece-dependent state changes
        match &mut new_piv.kind {
            PieceKind::P { moved } => {
                *moved = true;

                if let Some(new_kind) = mv.promotion {
                    new_piv.kind = new_kind;
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
                    new_piv.sq = new_piv.sq.shift(dx_from_king, 0).unwrap();
                }
            }
            PieceKind::K { moved } => {
                *moved = true;
            }
            _ => {}
        }

        new_piv
    }
}
