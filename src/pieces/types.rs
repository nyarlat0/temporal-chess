use crate::engine::State;

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
    pub active: bool,
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
}
