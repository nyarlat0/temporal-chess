use crate::pieces::PieceView;
use std::collections::{BTreeMap, BTreeSet};

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

        let effects = self.encounter_effects(piv, other_piv).ok_or_else(|| {
            EngineErr::InvalidHistory("collision has no traceable cause").during(format!(
                "collision at t={} {:?}, pieces {}:{} and {}:{}",
                piv.t, piv.sq, piv.origin, piv.age, other_piv.origin, other_piv.age
            ))
        })?;

        for effect in effects {
            match effect {
                CollisionEffect::KillFrom(id) => self.kill_piece(id.wid, id.age),
                CollisionEffect::CancelLastMove(piece) => self.undo_last_mv(piece),
            }
            .map_err(|e| e.during(format!("resolving collision: {effect:?}")))?;
        }

        Ok(())
    }

    /// A connected stay on one square, including dead states. Death must not
    /// erase the cause of a contact while recorded moves are being revalidated.
    fn contact_segment(&self, piece: PieceView) -> std::ops::Range<usize> {
        let line = &self.history.worldlines[piece.origin];
        let same_square = |age: usize| {
            let state = &line[age];
            state.x == piece.sq.0 && state.y == piece.sq.1
        };
        let mut start = piece.age;
        while start > 0 && line.gap_after(start - 1).is_none() && same_square(start - 1) {
            start -= 1;
        }
        let mut end = piece.age + 1;
        while end < line.len() && line.gap_after(end - 1).is_none() && same_square(end) {
            end += 1;
        }
        start..end
    }

    /// Repeated contacts during the same stay have one winner. In particular,
    /// a rebound must be resolved at the arrival before the turn, even when
    /// chronological board construction encounters its return leg first.
    /// Spatial departures and causal gaps start a new encounter.
    pub(super) fn encounter_effects(
        &self,
        a: PieceView,
        b: PieceView,
    ) -> Option<Vec<CollisionEffect>> {
        if a.color == b.color {
            return self.track_collision(a, b).map(collision_effects);
        }
        let a_range = self.contact_segment(a);
        let b_range = self.contact_segment(b);
        let mut root = (a, b);
        let ordered = if a.fut_seen >= b.fut_seen {
            [(a, &a_range, b, &b_range), (b, &b_range, a, &a_range)]
        } else {
            [(b, &b_range, a, &a_range), (a, &a_range, b, &b_range)]
        };
        for (piece, range, other, other_range) in ordered {
            let line = &self.history.worldlines[piece.origin];
            if !line[range.clone()]
                .windows(2)
                .any(|pair| pair[0].inverted != pair[1].inverted)
            {
                continue;
            }
            let mut other_times = BTreeMap::new();
            for age in other_range.clone() {
                other_times
                    .entry(self.history.worldlines[other.origin][age].t)
                    .or_insert(age);
            }
            let contact = range.clone().find_map(|age| {
                let &other_age = other_times.get(&line[age].t)?;
                Some((
                    self.get_piv_necro(piece.origin, age)?,
                    self.get_piv_necro(other.origin, other_age)?,
                ))
            });
            if let Some(contact) = contact {
                root = contact;
                break;
            }
        }
        let mut effects = collision_effects(self.track_collision(root.0, root.1)?);
        for effect in &mut effects {
            if let CollisionEffect::KillFrom(id) = effect {
                let (loser, loser_range, winner, winner_range) = if id.wid == a.origin {
                    (a, &a_range, b, &b_range)
                } else {
                    (b, &b_range, a, &a_range)
                };
                // Propagate defeat to earlier causal contacts as well: an
                // inverted victim can meet the returning winner before its
                // already-dead occurrence in its own worldline order.
                let winner_times: BTreeSet<_> = winner_range
                    .clone()
                    .map(|i| self.history.worldlines[winner.origin][i].t)
                    .collect();
                if let Some(age) = loser_range.clone().find(|&age| {
                    winner_times.contains(&self.history.worldlines[loser.origin][age].t)
                }) {
                    id.age = id.age.min(age);
                }
            }
        }
        Some(effects)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Color, PieceKind, Sq, State};

    #[test]
    fn rebound_encounter_has_one_winner_at_either_boundary_in_either_board_order() {
        for upper in [false, true] {
            let time = |t| if upper { 5 - t } else { t };
            let color = if upper { Color::White } else { Color::Black };
            let mut source = State::new(PieceKind::K { moved: true }, color, Sq(4, 7));
            source.t = time(1);
            source.fut_seen = 8;
            source.inverted = !upper;
            let mut arrival = source.clone();
            arrival.t = time(0);
            arrival.x = 5;
            arrival.y = 6;
            let mut returned = arrival.clone();
            returned.t = time(1);
            returned.inverted = upper;
            let enemy: Vec<_> = [2, 1, 0]
                .into_iter()
                .map(|t| {
                    let mut s = State::new(PieceKind::N, color.opposite(), Sq(5, 6));
                    s.t = time(t);
                    s.fut_seen = 6;
                    s.inverted = !upper;
                    s
                })
                .collect();
            let mut engine = Engine::default();
            engine.history.worldlines = vec![vec![source, arrival, returned].into(), enemy.into()];
            let expected = vec![CollisionEffect::KillFrom(super::super::ShortPieceId {
                wid: 1,
                age: 1,
            })];
            for (king_age, enemy_age) in [(1, 2), (2, 1)] {
                let king = engine.get_piv_necro(0, king_age).unwrap();
                let knight = engine.get_piv_necro(1, enemy_age).unwrap();
                assert_eq!(
                    engine.encounter_effects(king, knight),
                    Some(expected.clone())
                );
                assert_eq!(
                    engine.encounter_effects(knight, king),
                    Some(expected.clone())
                );
            }
            engine.kill_piece(1, 1).unwrap();
            assert_eq!(
                engine.encounter_effects(
                    engine.get_piv(0, 2).unwrap(),
                    engine.get_piv_necro(1, 1).unwrap()
                ),
                Some(expected)
            );
            assert!(engine.history.worldlines[0].iter().all(|s| s.alive));
            // Leaving the square terminates the encounter, so a later meeting
            // does not inherit the old winner's protection.
            let mut departure = engine.history.worldlines[0][2].clone();
            departure.x = 4;
            let mut reentry = departure.clone();
            reentry.x = 5;
            engine.history.worldlines[0]
                .states
                .extend([departure, reentry]);
            assert_eq!(engine.contact_segment(engine.get_piv(0, 4).unwrap()), 4..5);
        }
    }
}
