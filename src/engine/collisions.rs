use std::cmp::min;

use crate::pieces::{Color, PieceView};

use super::{BoardConflict, Engine, EngineErr, PieceCollision};

impl Engine {
    /// Conflict is any intersection of n pieces.
    /// Is fixed by separating pieces to pairs,
    /// tracking each pair collision (root of conflict) and fixing it.
    /// This function only fixes one collision and updates global state.
    pub fn try_fix_conflict(&mut self, mut bc: BoardConflict) -> Result<(), EngineErr> {
        // Sort by future seen for casuality
        bc.pivs.sort_by_key(|a| a.fut_seen);

        // Get pairs of conflicting pieces
        let piv = *bc.pivs.first().ok_or(EngineErr::WrongPiece)?;
        let other_piv = *bc.pivs.get(1).ok_or(EngineErr::WrongPiece)?;

        let col = self
            .track_collision(piv, other_piv)
            .ok_or(EngineErr::WrongPiece)?;

        let _prev_survived = Some(self.fix_collision(col, piv.t)?);

        Ok(())
    }

    /// Collision is the root of intersection of figures (aka conflict).
    /// Collision time is the first conflict time.
    /// Method undoes move or kills piece, returns the piece that stays unchanged.
    /// Gets conflict time as input to return correct survivor in case of mutual destruction.
    pub fn fix_collision(
        &mut self,
        col: PieceCollision,
        conf_t: usize,
    ) -> Result<PieceView, EngineErr> {
        let PieceCollision {
            bf_collider,
            collider,
            collided,
        } = col;
        let surv: PieceView;

        // Pieces of the same side
        if collider.color == collided.color {
            // In oncoming collision
            // collider = ordinary,
            // collided = inverted
            if collider.inverted != collided.inverted {
                // Inverted kills ordinary on board of its color
                if Color::from_board(collider.t) == collider.color {
                    self.kill_piece(collider.origin, collider.age)?;
                    surv = collided;
                } else {
                    // Else undo inverted move
                    self.undo_last_mv(collided)?;
                    surv = collider;
                }
            } else {
                // In co-directional undo collider move
                self.undo_last_mv(collider)?;
                surv = collided;
            }
        // Enemy pieces
        } else {
            // In oncoming collision
            // collider = ordinary,
            // collided = inverted
            if collider.inverted != collided.inverted {
                // If ordinary moved and knew future before collision
                // then it kills inverted in collision board,
                // but is killed after collision board
                if bf_collider.sq != collider.sq && bf_collider.knows_future() {
                    self.kill_piece(collided.origin, collided.age)?;
                    self.kill_piece(collider.origin, collider.age + 1)?;

                    if conf_t == collider.t {
                        surv = collider;
                    } else {
                        surv = collided;
                    }
                } else {
                    // Else just kill ordinary
                    self.kill_piece(collider.origin, collider.age)?;
                    surv = collided;
                }
            } else {
                // Else collided is killed
                self.kill_piece(collided.origin, collided.age)?;
                surv = collider;
            }
        }

        Ok(surv)
    }

    /// Collision is the root of intersection of figures (aka conflict).
    /// Collision time is the first conflict time.
    /// If oncoming (inverted vs ordinary) collision,
    /// always return ordinary as collider and inv as collided.
    pub fn track_collision(&self, piv: PieceView, other_piv: PieceView) -> Option<PieceCollision> {
        // First checked for sq change is piv with
        // most future seen in case both figures move
        // at the same time
        let (piv, other_piv) = if piv.fut_seen >= other_piv.fut_seen {
            (piv, other_piv)
        } else {
            (other_piv, piv)
        };

        // Oncoming collision can have some exotic situations
        let oncoming = piv.inverted != other_piv.inverted;

        if !oncoming {
            let mut prev_piv = piv;
            let mut prev_other_piv = other_piv;

            for d in 1..=min(piv.age, other_piv.age) {
                let piv_bf = self.get_piv(piv.origin, piv.age.saturating_sub(d))?;
                let other_piv_bf =
                    self.get_piv(other_piv.origin, other_piv.age.saturating_sub(d))?;

                if piv_bf.sq != piv.sq {
                    let bf_collider = piv_bf;
                    let collider = prev_piv;
                    let collided = prev_other_piv;
                    return Some(PieceCollision {
                        bf_collider,
                        collider,
                        collided,
                    });
                }

                if other_piv_bf.sq != other_piv.sq {
                    let bf_collider = other_piv_bf;
                    let collider = prev_other_piv;
                    let collided = prev_piv;
                    return Some(PieceCollision {
                        bf_collider,
                        collider,
                        collided,
                    });
                }

                prev_piv = piv_bf;
                prev_other_piv = other_piv_bf;
            }
        } else {
            let (inv, ord) = if piv.inverted {
                (piv, other_piv)
            } else {
                (other_piv, piv)
            };

            let mut prev_inv = inv;
            let mut prev_ord = ord;

            for d in 1..=ord.age {
                let ord_bf = self.get_piv(piv.origin, piv.age.saturating_sub(d))?;
                let mb_inv_aft = self.get_piv(other_piv.origin, other_piv.age + d);

                if let Some(inv_aft) = mb_inv_aft {
                    if ord_bf.sq != ord.sq {
                        let bf_collider = ord_bf;
                        let collider = prev_ord;
                        let collided = prev_inv;
                        return Some(PieceCollision {
                            bf_collider,
                            collider,
                            collided,
                        });
                    }

                    if inv_aft.sq != inv.sq {
                        let bf_collider = ord_bf;
                        let collider = prev_ord;
                        let collided = prev_inv;
                        return Some(PieceCollision {
                            bf_collider,
                            collider,
                            collided,
                        });
                    }

                    prev_ord = ord_bf;
                    prev_inv = inv_aft;
                } else {
                    // If inverted isnt present its just before collision
                    let bf_collider = ord_bf;
                    let collider = prev_ord;
                    let collided = prev_inv;
                    return Some(PieceCollision {
                        bf_collider,
                        collider,
                        collided,
                    });
                }
            }
        }

        None
    }
}
