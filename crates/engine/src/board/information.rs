use std::collections::BTreeSet;

use crate::model::{PieceId, ShortPieceId};
use crate::pieces::Color;

use super::{Board, PreBoard};

impl Board {
    /// Returns informed piece shpid -> info
    pub fn calc_info(
        &self,
        opt_past: Option<&PreBoard>,
        opt_fut: Option<&PreBoard>,
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

            // Enpass moves to future if past preboard is avalible
            if let Some(past_pboard) = opt_past {
                let enpass_mvs = p.en_passant_moves(self, past_pboard);
                for (_sq, eaten) in enpass_mvs {
                    let enemy = self.get_piece(eaten.0, eaten.1).unwrap();
                    info.insert(PieceId::from_piv(*enemy));
                }
            }

            // Enpass moves to past if future preboard is availible
            if let Some(fut_pboard) = opt_fut {
                let enpass_mvs = p.en_passant_moves(self, fut_pboard);
                for (_sq, eaten) in enpass_mvs {
                    let enemy = self.get_piece(eaten.0, eaten.1).unwrap();
                    info.insert(PieceId::from_piv(*enemy));
                }
            }

            out.push((ShortPieceId::from_piv(p), info));
        }

        out
    }
}
