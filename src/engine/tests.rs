use std::collections::BTreeMap;

use crate::pieces::{Color, PieceKind, PieceView, Sq};

use super::collision_rules::{CollisionEffect, collision_effects};
use super::*;

fn play(engine: &mut Engine, from: Sq, to: Sq) -> Move {
    let t = engine.move_sources().map(|piece| piece.t).max().unwrap();
    let piece = *engine.board(t).unwrap().get_piece(from.0, from.1).unwrap();
    let mv = engine
        .possible_moves(piece.origin, piece.age)
        .unwrap()
        .into_iter()
        .find(|mv| mv.sq == to && !mv.to_past() && mv.promotion().is_none())
        .unwrap();
    engine.apply_new_mv(mv).unwrap();
    mv
}

#[test]
fn cancelling_an_earlier_move_revalidates_later_moves_and_revives_the_victim() {
    let mut engine = Engine::default();
    let first = play(&mut engine, Sq(4, 1), Sq(4, 3));
    let black = play(&mut engine, Sq(3, 6), Sq(3, 4));
    let capture = play(&mut engine, Sq(4, 3), Sq(3, 4));
    assert!(engine.get_piv(27, 3).is_none());
    engine.undo_mv(first.id()).unwrap();
    engine.reconcile().unwrap();
    assert_eq!(engine.history.moves.len(), 1);
    assert!(engine.history.moves.contains_key(&black.id()));
    assert!(!engine.history.moves.contains_key(&capture.id()));
    assert_eq!(engine.get_piv(27, 3).unwrap().sq, Sq(3, 4));
    assert_eq!(engine.get_piv(12, 3).unwrap().sq, Sq(4, 1));
    let cancelled = &engine.history.worldlines[12][2];
    assert!(cancelled.rights.cancelled_move);
    assert!(!cancelled.rights.ordinary);
    assert_eq!(
        Engine::from_history(engine.history.clone(), engine.turn).unwrap(),
        engine
    );
}

#[test]
fn undoing_en_passant_removes_its_capture_cause() {
    let mut engine = Engine::default();
    play(&mut engine, Sq(4, 1), Sq(4, 3));
    play(&mut engine, Sq(0, 6), Sq(0, 5));
    play(&mut engine, Sq(4, 3), Sq(4, 4));
    play(&mut engine, Sq(3, 6), Sq(3, 4));
    let capture = play(&mut engine, Sq(4, 4), Sq(3, 5));
    engine.undo_mv(capture.id()).unwrap();
    engine.reconcile().unwrap();
    assert_eq!(engine.get_piv(27, 5).unwrap().sq, Sq(3, 4));
    assert_eq!(engine.get_piv(12, 5).unwrap().sq, Sq(4, 4));
}

#[test]
fn an_existing_inverted_segment_can_move_and_be_undone_without_erasing_its_states() {
    let mut state = State::new(PieceKind::N, Color::White, Sq(1, 0));
    state.t = 4;
    state.fut_seen = 4;
    state.loop_turn = 1;
    state.inverted = true;
    let middle = state.prolongate().unwrap();
    let last = middle.prolongate().unwrap();
    let mut engine =
        Engine::from_worldlines(vec![vec![state, middle, last]], Color::White).unwrap();
    let mv = engine
        .possible_moves(0, 0)
        .unwrap()
        .into_iter()
        .find(|mv| mv.to_past() && mv.sq == Sq(2, 2))
        .unwrap();
    engine.apply_new_mv(mv).unwrap();
    assert_eq!(engine.get_piv(0, 1).unwrap().t, 3);
    assert_eq!(engine.get_piv(0, 1).unwrap().sq, Sq(2, 2));
    engine.undo_mv(mv.id()).unwrap();
    engine.reconcile().unwrap();
    assert_eq!(engine.history.worldlines[0].len(), 3);
    assert_eq!(engine.get_piv(0, 1).unwrap().sq, Sq(1, 0));
    assert_eq!(engine.get_piv(0, 2).unwrap().sq, Sq(1, 0));
}

#[test]
fn castling_undo_restores_rook_position_and_rights() {
    let mut engine = Engine::from_worldlines(
        vec![
            vec![State::new(
                PieceKind::K { moved: false },
                Color::White,
                Sq(4, 0),
            )],
            vec![State::new(
                PieceKind::R { moved: false },
                Color::White,
                Sq(7, 0),
            )],
        ],
        Color::White,
    )
    .unwrap();
    let mv = engine
        .possible_moves(0, 0)
        .unwrap()
        .into_iter()
        .find(|mv| mv.castling() == Some(Castling::Short))
        .unwrap();
    engine.apply_new_mv(mv).unwrap();
    engine.undo_mv(mv.id()).unwrap();
    engine.reconcile().unwrap();
    assert_eq!(engine.get_piv(0, 1).unwrap().sq, Sq(4, 0));
    assert_eq!(engine.get_piv(1, 1).unwrap().sq, Sq(7, 0));
    assert_eq!(
        engine.get_piv(1, 1).unwrap().kind,
        PieceKind::R { moved: false }
    );
}

#[test]
fn new_information_grants_an_optional_reaction_without_granting_an_ordinary_move() {
    let mut rook = State::new(PieceKind::R { moved: true }, Color::White, Sq(0, 0));
    rook.rights.ordinary = false;
    let mut engine = Engine::from_worldlines(
        vec![
            vec![rook],
            vec![State::new(
                PieceKind::P { moved: true },
                Color::Black,
                Sq(0, 4),
            )],
        ],
        Color::White,
    )
    .unwrap();
    let piece = engine.get_piv(0, 0).unwrap();
    assert!(piece.rights.new_information);
    assert!(!piece.rights.ordinary);
    assert_eq!(engine.move_sources().count(), 1);
    assert_eq!(engine.board(0).unwrap().get_piece(0, 0).unwrap(), &piece);

    engine.history.worldlines[0][0].rights.new_information = false;
    engine.history.worldlines[1][0].x = 1;
    engine.reconcile().unwrap();
    assert!(!engine.get_piv(0, 0).unwrap().rights.new_information);
}

#[test]
fn oncoming_tracking_uses_orientation_even_when_inverted_piece_knows_more_future() {
    let ordinary = State::new(PieceKind::R { moved: true }, Color::White, Sq(0, 0));
    let mut ordinary_next = ordinary.prolongate().unwrap();
    ordinary_next.y = 1;
    let mut inverted = State::new(PieceKind::N, Color::Black, Sq(0, 1));
    inverted.t = 2;
    inverted.fut_seen = 2;
    inverted.inverted = true;
    inverted.loop_turn = 1;
    let inverted_next = inverted.prolongate().unwrap();
    let mut inverted_last = inverted_next.prolongate().unwrap();
    inverted_last.x = 1;
    inverted_last.y = 3;
    let engine = Engine {
        history: History {
            worldlines: vec![
                vec![ordinary, ordinary_next].into(),
                vec![inverted, inverted_next, inverted_last].into(),
            ],
            moves: BTreeMap::new(),
            targets: BTreeMap::new(),
            horizon: 2,
            status: GameStatus::Ongoing,
        },
        projection: Projection::default(),
        turn: Color::White,
    };
    let ordinary = engine.get_piv(0, 1).unwrap();
    let inverted = engine.get_piv(1, 1).unwrap();
    let collision = engine.track_collision(ordinary, inverted).unwrap();
    assert_eq!(collision.collider, ordinary);
    assert_eq!(collision.collided, inverted);
    assert_eq!(collision.bf_collider, engine.get_piv(0, 0).unwrap());
    assert_eq!(engine.track_collision(inverted, ordinary), Some(collision));
}

#[test]
fn collision_policy_covers_colors_directions_and_future_knowledge() {
    use CollisionEffect::*;
    let before = PieceView::from_state(&State::new(PieceKind::N, Color::White, Sq(0, 0)), 0, 0);
    let mut mover = before;
    mover.t = 1;
    mover.age = 1;
    mover.sq = Sq(1, 2);
    let mut target = mover;
    target.origin = 1;
    target.color = Color::Black;
    let plan = |bf_collider, collider, collided| {
        collision_effects(PieceCollision {
            bf_collider,
            collider,
            collided,
        })
    };

    assert_eq!(
        plan(before, mover, target),
        vec![KillFrom(ShortPieceId { wid: 1, age: 1 })]
    );
    target.color = Color::White;
    assert_eq!(plan(before, mover, target), vec![CancelLastMove(mover)]);
    target.inverted = true;
    assert_eq!(plan(before, mover, target), vec![CancelLastMove(target)]);
    mover.t = 2;
    assert_eq!(
        plan(before, mover, target),
        vec![KillFrom(ShortPieceId { wid: 0, age: 1 })]
    );
    target.color = Color::Black;
    assert_eq!(
        plan(before, mover, target),
        vec![KillFrom(ShortPieceId { wid: 0, age: 1 })]
    );
    let mut informed = before;
    informed.fut_seen = 3;
    assert_eq!(
        plan(informed, mover, target),
        vec![
            KillFrom(ShortPieceId { wid: 1, age: 1 }),
            KillFrom(ShortPieceId { wid: 0, age: 2 })
        ]
    );
}
