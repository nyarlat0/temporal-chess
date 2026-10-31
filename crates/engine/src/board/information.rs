use std::collections::BTreeSet;

use crate::model::{PieceId, ShortPieceId};
use crate::pieces::Color;

use super::{Board, PreBoard};

impl Board {
    /// Capturable enemy identities visible on this board, without temporal context.
    pub fn calc_info(&self) -> Vec<(ShortPieceId, BTreeSet<PieceId>)> {
        self.calc_info_with_en_passant(&[])
    }

    /// Adjacent boards establish a double pawn step in either time direction;
    /// the victim and the free capture square must still be on this board.
    pub(crate) fn calc_info_with_en_passant(
        &self,
        adjacent: &[&PreBoard],
    ) -> Vec<(ShortPieceId, BTreeSet<PieceId>)> {
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

            for previous in adjacent {
                for (_, captured) in p.en_passant_moves(self, previous) {
                    if let Some(enemy) = self.get_piece(captured.0, captured.1) {
                        info.insert(PieceId::from_piv(*enemy));
                    }
                }
            }
            out.push((ShortPieceId::from_piv(p), info));
        }

        out
    }
}
