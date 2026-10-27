//! Pure collision policy. Tracking a collision and mutating history are separate.
use crate::pieces::{Color, PieceView};

use super::{PieceCollision, ShortPieceId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CollisionEffect {
    KillFrom(ShortPieceId),
    CancelLastMove(PieceView),
}

pub(super) fn collision_effects(collision: PieceCollision) -> Vec<CollisionEffect> {
    use CollisionEffect::*;
    let PieceCollision {
        bf_collider,
        collider,
        collided,
    } = collision;
    let kill = |piece: PieceView| KillFrom(ShortPieceId::from_piv(&piece));
    let oncoming = collider.inverted != collided.inverted;
    // For an oncoming collision the tracker always puts the ordinary piece first.
    if collider.color == collided.color {
        if oncoming {
            if Color::from_board(collider.t) == collider.color {
                vec![kill(collider)]
            } else {
                vec![CancelLastMove(collided)]
            }
        } else {
            vec![CancelLastMove(collider)]
        }
    } else if collider.fut_seen > collided.fut_seen {
        // Knowledge at the collision wins, regardless of movement or temporal
        // orientation. The winner must not receive a delayed death either.
        vec![kill(collided)]
    } else if collided.fut_seen > collider.fut_seen {
        vec![kill(collider)]
    } else if oncoming {
        // Equal knowledge retains the existing tie-breaking rules.
        if bf_collider.sq != collider.sq && bf_collider.knows_future() {
            vec![
                kill(collided),
                KillFrom(ShortPieceId {
                    wid: collider.origin,
                    age: collider.age + 1,
                }),
            ]
        } else {
            vec![kill(collider)]
        }
    } else {
        vec![kill(collided)]
    }
}
