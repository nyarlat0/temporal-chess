use std::collections::BTreeSet;

use crate::model::{PieceId, ShortPieceId};
use crate::pieces::Color;

use super::Board;

impl Board {
    /// Capturable enemy identities visible on this board only. Neither temporal
    /// move legality nor en passant opportunities on neighboring boards may
    /// change the information stored for an otherwise unchanged position.
    pub fn calc_info(&self) -> Vec<(ShortPieceId, BTreeSet<PieceId>)> {
        let side = Color::from_board(self.age);
        let mut out = Vec::new();

        for p in self.pieces.iter() {
            if p.color != side {
                continue;
            }
            let mut info = BTreeSet::new();

            // Basic moves
            let sqs = p.basic_moves(self);
            for sq in sqs.iter() {
                if self.is_enemy(side, *sq) {
                    let enemy = self.get_piece(sq.0, sq.1).unwrap();
                    info.insert(PieceId::from_piv(*enemy));
                }
            }

            out.push((ShortPieceId::from_piv(p), info));
        }

        out
    }
}
