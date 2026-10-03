use crate::board::{Board, PreBoard};
use crate::model::Castling;

use super::{Color, PieceKind, PieceView, Sq};

pub const ROOK_DIRS: &[(i8, i8)] = &[(1, 0), (-1, 0), (0, 1), (0, -1)];
pub const BISHOP_DIRS: &[(i8, i8)] = &[(1, 1), (1, -1), (-1, 1), (-1, -1)];
pub const KNIGHT_DELTAS: &[(i8, i8)] = &[
    (1, 2),
    (2, 1),
    (-1, 2),
    (-2, 1),
    (1, -2),
    (2, -1),
    (-1, -2),
    (-2, -1),
];
pub const KING_DELTAS: &[(i8, i8)] = &[
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

impl PieceView {
    fn ray_moves(&self, dirs: &[(i8, i8)], board: &Board) -> Vec<Sq> {
        let mut v = Vec::new();
        for &(dx, dy) in dirs {
            let mut cur = self.sq;
            loop {
                match cur.shift(dx, dy) {
                    Some(sq) if board.is_empty(sq) || board.is_fut_me(self, sq) => {
                        v.push(sq);
                        cur = sq;
                    }
                    Some(sq) if board.is_enemy(self.color, sq) => {
                        v.push(sq);
                        break;
                    }
                    _ => break,
                }
            }
        }
        v
    }

    pub fn basic_moves(&self, board: &Board) -> Vec<Sq> {
        let mut v = Vec::new();

        // basic moves
        match self.kind {
            PieceKind::N => {
                let deltas = KNIGHT_DELTAS;

                for &(dx, dy) in deltas {
                    if let Some(sq) = self.sq.shift(dx, dy)
                        && (board.is_empty(sq)
                            || board.is_enemy(self.color, sq)
                            || board.is_fut_me(self, sq))
                    {
                        v.push(sq);
                    }
                }
            }

            PieceKind::K { .. } => {
                let deltas = KING_DELTAS;

                for &(dx, dy) in deltas {
                    if let Some(sq) = self.sq.shift(dx, dy)
                        && (board.is_empty(sq)
                            || board.is_enemy(self.color, sq)
                            || board.is_fut_me(self, sq))
                    {
                        v.push(sq);
                    }
                }
            }

            PieceKind::R { .. } => v.extend(self.ray_moves(ROOK_DIRS, board)),
            PieceKind::B => v.extend(self.ray_moves(BISHOP_DIRS, board)),
            PieceKind::Q => {
                v.extend(self.ray_moves(ROOK_DIRS, board));
                v.extend(self.ray_moves(BISHOP_DIRS, board));
            }

            PieceKind::P { moved } => {
                let dir: i8 = match self.color {
                    Color::White => 1,
                    Color::Black => -1,
                };

                if let Some(one) = self.sq.shift(0, dir)
                    && (board.is_empty(one) || board.is_fut_me(self, one))
                {
                    v.push(one);
                    if !moved
                        && let Some(two) = self.sq.shift(0, 2 * dir)
                        && (board.is_empty(two) || board.is_fut_me(self, two))
                    {
                        v.push(two);
                    }
                }

                // Capture
                for &dx in &[-1, 1] {
                    if let Some(sq) = self.sq.shift(dx, dir)
                        && board.is_enemy(self.color, sq)
                    {
                        v.push(sq);
                    }
                }
            }
        }

        v
    }

    /// Calculates en passant moves.
    /// Takes as arguments present board and past preboard
    /// which is needed for en passant to past if future board is not constructed.
    ///
    /// Returs tuples (move_sq, eat_sq).
    ///
    /// To get en passant moves to past you should pass future board as past board arg.
    pub fn en_passant_moves(&self, board: &Board, past_pboard: &PreBoard) -> Vec<(Sq, Sq)> {
        let mut v = Vec::new();
        if !matches!(self.kind, PieceKind::P { .. }) {
            return v;
        }
        let fut_board = board;

        let Sq(_x, y) = self.sq;
        let dir: i8 = if matches!(self.color, Color::White) {
            1
        } else {
            -1
        };

        for &dx in &[-1, 1] {
            // adjacent square on same rank
            let Some(Sq(ax, _)) = self.sq.shift(dx, 0) else {
                continue;
            };
            // must be a pawn there
            let Some(adj) = board.get_piece(ax, y) else {
                continue;
            };
            if adj.color == self.color || !matches!(adj.kind, PieceKind::P { .. }) {
                continue;
            }
            // and it must have just moved two squares straight
            let Some(prev_adj) = past_pboard
                .space
                .iter()
                .flat_map(|row| row.iter())
                .flat_map(|prep| prep.iter())
                .find(|p| p.origin == adj.origin && p.age < adj.age && adj.age - p.age <= 2)
            else {
                continue;
            };

            if prev_adj.sq.0 != adj.sq.0 || (prev_adj.sq.1 as i8 - adj.sq.1 as i8).abs() != 2 {
                continue;
            }

            // capture square: diagonally forward into the square the pawn passed over
            if let Some(capture_sq) = self.sq.shift(dx, dir)
                && (fut_board.is_empty(capture_sq) || fut_board.is_fut_me(self, capture_sq))
            {
                v.push((capture_sq, adj.sq));
            }
        }
        v
    }

    pub fn castling_moves(&self, board: &Board) -> Vec<(Sq, Castling)> {
        let mut v = Vec::new();

        // must be an unmoved king on home rank
        let PieceKind::K { moved: false } = self.kind else {
            return v;
        };
        let (home_y, e, c, d, f, g, a, h) = match self.color {
            Color::White => (0, 4, 2, 3, 5, 6, 0, 7),
            Color::Black => (7, 4, 2, 3, 5, 6, 0, 7),
        };

        if self.sq != Sq(e, home_y) {
            return v;
        }

        // helper closures
        let empty = |x: usize| board.is_empty(Sq(x, home_y));
        let attacked = |x: usize| board.is_attacked(self.color.opposite(), Sq(x, home_y));

        // king must not be in check now
        if attacked(e) {
            return v;
        }

        // ---- queen-side (long) ----
        if let Some(rook) = board.get_piece(a, home_y)
            && rook.color == self.color
            && let PieceKind::R { moved: false } = rook.kind
            // squares between rook and king empty: b, c, d
            && empty(1) && empty(2) && empty(3)
            // king path squares not attacked: e, d, c
            && !attacked(d) && !attacked(c)
        {
            v.push((Sq(c, home_y), Castling::Long));
        }

        // ---- king-side (short) ----
        if let Some(rook) = board.get_piece(h, home_y)
            && rook.color == self.color
            && let PieceKind::R { moved: false } = rook.kind
            // squares between empty: f, g
            && empty(f) && empty(g)
            // king path not attacked: e, f, g
            && !attacked(f) && !attacked(g)
        {
            v.push((Sq(g, home_y), Castling::Short));
        }

        v
    }
}
