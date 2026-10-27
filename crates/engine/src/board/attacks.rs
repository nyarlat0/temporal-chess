use crate::pieces::{BISHOP_DIRS, Color, KING_DELTAS, KNIGHT_DELTAS, PieceKind, ROOK_DIRS, Sq};

use super::Board;

impl Board {
    pub fn is_inv_attacked(&self, fut_board: &Board, by: Color, sq: Sq) -> bool {
        let now_board = self;

        // pawns
        let dir = by.pawn_dir();
        for &dx in &[-1, 1] {
            if let Some(Sq(x, y)) = sq.shift(dx, -dir)
                && let Some(p) = fut_board.get_piece(x, y)
                && p.color == by
                && matches!(p.kind, PieceKind::P { .. })
                && p.inverted
            {
                return true;
            }
        }

        // knights
        for &(dx, dy) in KNIGHT_DELTAS {
            if let Some(Sq(x, y)) = sq.shift(dx, dy)
                && let Some(p) = fut_board.get_piece(x, y)
                && p.color == by
                && matches!(p.kind, PieceKind::N)
                && p.inverted
            {
                return true;
            }
        }

        // kings
        for &(dx, dy) in KING_DELTAS {
            if let Some(Sq(x, y)) = sq.shift(dx, dy)
                && let Some(p) = fut_board.get_piece(x, y)
                && p.color == by
                && matches!(p.kind, PieceKind::K { .. })
                && p.inverted
            {
                return true;
            }
        }

        // bishops/queens
        for &(dx, dy) in BISHOP_DIRS {
            let mut cur = sq;
            while let Some(nsq @ Sq(x, y)) = cur.shift(dx, dy) {
                if let Some(p) = fut_board.get_piece(x, y)
                    && p.color == by
                    && matches!(p.kind, PieceKind::B | PieceKind::Q)
                    && p.inverted
                {
                    return true;
                }
                if now_board.get_piece(x, y).is_some() {
                    break;
                }
                cur = nsq;
            }
        }

        // rooks/queens
        for &(dx, dy) in ROOK_DIRS {
            let mut cur = sq;
            while let Some(nsq @ Sq(x, y)) = cur.shift(dx, dy) {
                if let Some(p) = fut_board.get_piece(x, y)
                    && p.color == by
                    && matches!(p.kind, PieceKind::R { .. } | PieceKind::Q)
                    && p.inverted
                {
                    return true;
                }
                if now_board.get_piece(x, y).is_some() {
                    break;
                }
                cur = nsq;
            }
        }

        false
    }

    pub fn is_attacked(&self, by: Color, sq: Sq) -> bool {
        let board = self;

        // pawns
        let dir = by.pawn_dir();
        for &dx in &[-1, 1] {
            if let Some(Sq(x, y)) = sq.shift(dx, -dir)
                && let Some(p) = board.get_piece(x, y)
                && p.color == by
                && matches!(p.kind, PieceKind::P { .. })
            {
                return true;
            }
        }

        // knights
        for &(dx, dy) in KNIGHT_DELTAS {
            if let Some(Sq(x, y)) = sq.shift(dx, dy)
                && let Some(p) = board.get_piece(x, y)
                && p.color == by
                && matches!(p.kind, PieceKind::N)
            {
                return true;
            }
        }

        // kings
        for &(dx, dy) in KING_DELTAS {
            if let Some(Sq(x, y)) = sq.shift(dx, dy)
                && let Some(p) = board.get_piece(x, y)
                && p.color == by
                && matches!(p.kind, PieceKind::K { .. })
            {
                return true;
            }
        }

        // bishops/queens
        for &(dx, dy) in BISHOP_DIRS {
            let mut cur = sq;
            while let Some(nsq @ Sq(x, y)) = cur.shift(dx, dy) {
                if let Some(p) = board.get_piece(x, y) {
                    if p.color == by && matches!(p.kind, PieceKind::B | PieceKind::Q) {
                        return true;
                    }
                    break; // blocked
                }
                cur = nsq;
            }
        }

        // rooks/queens
        for &(dx, dy) in ROOK_DIRS {
            let mut cur = sq;
            while let Some(nsq @ Sq(x, y)) = cur.shift(dx, dy) {
                if let Some(p) = board.get_piece(x, y) {
                    if p.color == by && matches!(p.kind, PieceKind::R { .. } | PieceKind::Q) {
                        return true;
                    }
                    break;
                }
                cur = nsq;
            }
        }

        false
    }
}
