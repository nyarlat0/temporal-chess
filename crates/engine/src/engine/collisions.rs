use crate::pieces::PieceView;

use super::collision_rules::{CollisionEffect, collision_effects};

use super::{BoardConflict, Engine, EngineErr, PieceCollision};

impl Engine {
    /// Conflict is any intersection of n pieces.
    /// Is fixed by separating pieces to pairs,
    /// tracking each pair collision (root of conflict) and fixing it.
    /// This function only fixes one collision and updates global state.
    pub(super) fn try_fix_conflict(&mut self, mut bc: BoardConflict) -> Result<(), EngineErr> {
        // Sort by future seen for casuality
        bc.pivs
            .sort_by_key(|a| (a.fut_seen, a.loop_turn, a.origin, a.age));

        // Get pairs of conflicting pieces
        let piv = *bc.pivs.first().ok_or(EngineErr::WrongPiece)?;
        let other_piv = *bc.pivs.get(1).ok_or(EngineErr::WrongPiece)?;

        let col = self.track_collision(piv, other_piv).ok_or_else(|| {
            EngineErr::InvalidHistory("collision has no traceable cause").during(format!(
                "collision at t={} {:?}, pieces {}:{} and {}:{}",
                piv.t, piv.sq, piv.origin, piv.age, other_piv.origin, other_piv.age
            ))
        })?;

        for effect in collision_effects(col) {
            match effect {
                CollisionEffect::KillFrom(id) => self.kill_piece(id.wid, id.age),
                CollisionEffect::CancelLastMove(piece) => self.undo_last_mv(piece),
            }
            .map_err(|e| e.during(format!("resolving collision: {effect:?}")))?;
        }

        Ok(())
    }

    /// Collision is the root of intersection of figures (aka conflict).
    /// Collision time is the first conflict time.
    /// If oncoming (inverted vs ordinary) collision,
    /// always return ordinary as collider and inv as collided.
    pub(super) fn track_collision(
        &self,
        piv: PieceView,
        other_piv: PieceView,
    ) -> Option<PieceCollision> {
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

            loop {
                let piv_bf = self.previous_piece(prev_piv);
                let other_piv_bf = self.previous_piece(prev_other_piv);

                if let Some(piv_bf) = piv_bf.filter(|bf| bf.sq != piv.sq) {
                    let bf_collider = piv_bf;
                    let collider = prev_piv;
                    let collided = prev_other_piv;
                    return Some(PieceCollision {
                        bf_collider,
                        collider,
                        collided,
                    });
                }

                if let Some(other_piv_bf) = other_piv_bf.filter(|bf| bf.sq != other_piv.sq) {
                    let bf_collider = other_piv_bf;
                    let collider = prev_other_piv;
                    let collided = prev_piv;
                    return Some(PieceCollision {
                        bf_collider,
                        collider,
                        collided,
                    });
                }

                match (piv_bf, other_piv_bf) {
                    (Some(a), Some(b)) if a.t == b.t => {
                        prev_piv = a;
                        prev_other_piv = b;
                    }
                    _ => {
                        // A preserved future can begin at a causal gap while
                        // the other piece is already standing on its square.
                        // Keep the contact at this boundary; find the arrival
                        // along each *own* connected line instead of requiring
                        // the two pieces to have equally long predecessors.
                        for (collider, collided) in
                            [(prev_piv, prev_other_piv), (prev_other_piv, prev_piv)]
                        {
                            if let Some(bf_collider) = self.before_arrival(collider) {
                                return Some(PieceCollision {
                                    bf_collider,
                                    collider,
                                    collided,
                                });
                            }
                        }
                        return None;
                    }
                }
            }
        } else {
            let (inv, ord) = if piv.inverted {
                (piv, other_piv)
            } else {
                (other_piv, piv)
            };

            let mut prev_inv = inv;
            let mut prev_ord = ord;

            loop {
                // An overlap can reach the beginning of an ordinary worldline
                // (or a causal segment). There is no earlier movement there,
                // but the collision is real: resolve it as a stationary
                // ordinary piece meeting an inverted one. Do not require a
                // predecessor before the beginning of history.
                let Some(ord_bf) = self.previous_piece(prev_ord) else {
                    return Some(PieceCollision {
                        bf_collider: prev_ord,
                        collider: prev_ord,
                        collided: prev_inv,
                    });
                };
                let mb_inv_aft = self.next_piece(prev_inv).filter(|p| p.t == ord_bf.t);

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
    }

    fn before_arrival(&self, piece: PieceView) -> Option<PieceView> {
        let mut cursor = piece;
        while let Some(previous) = self.previous_piece(cursor) {
            if previous.sq != piece.sq {
                return Some(previous);
            }
            cursor = previous;
        }
        None
    }
}
