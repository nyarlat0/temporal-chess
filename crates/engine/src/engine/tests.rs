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
    let mut engine = Engine::new(true);
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
fn en_passant_information_reacts_in_both_time_directions() {
    for inverted_victim in [false, true] {
        for inverted_observer in [false, true] {
            let mut observer = State::new(PieceKind::P { moved: true }, Color::White, Sq(4, 4));
            observer.t = 2;
            observer.fut_seen = 4;
            observer.inverted = inverted_observer;
            observer.loop_turn = usize::from(inverted_observer);
            observer.rights.ordinary = false;
            let mut victim = State::new(PieceKind::P { moved: false }, Color::Black, Sq(3, 6));
            victim.t = if inverted_victim { 3 } else { 1 };
            victim.fut_seen = 4;
            victim.inverted = inverted_victim;
            victim.loop_turn = usize::from(inverted_victim);
            let mut arrived = victim.prolongate().unwrap();
            arrived.y = 4;
            arrived.kind = PieceKind::P { moved: true };
            let waiting = arrived.prolongate().unwrap();
            let mut engine = Engine::from_worldlines(
                vec![vec![observer], vec![victim, arrived, waiting]],
                Color::White,
            )
            .unwrap();
            let pawn = engine.get_piv(0, 0).unwrap();
            assert!(pawn.rights.new_information);
            let victim_id = PieceId::from_piv(engine.get_piv(1, 1).unwrap());
            assert!(
                engine.history.worldlines[0][0]
                    .informed
                    .contains(&victim_id)
            );
            let direction = TimeDirection::from_inverted(inverted_victim);
            let capture = engine
                .possible_moves(0, 0)
                .unwrap()
                .into_iter()
                .find(|mv| mv.direction == direction && mv.en_passant() == Some(Sq(3, 4)))
                .expect("the observed en passant is a playable move");
            let mut played = engine.clone();
            played.apply_new_mv(capture).unwrap();
            assert!(!played.history.worldlines[1].last().unwrap().alive);
            // A blocked landing square removes the opportunity without creating a reaction.
            engine.history.worldlines[0][0].rights.new_information = false;
            let mut blocker = State::new(PieceKind::N, Color::White, Sq(3, 5));
            blocker.t = 2;
            blocker.fut_seen = 2;
            engine.history.worldlines.push(vec![blocker].into());
            engine.update_bufs().unwrap();
            engine.build_boards(4).unwrap();
            engine.update_infos().unwrap();
            assert!(
                !engine.history.worldlines[0][0]
                    .informed
                    .contains(&victim_id)
            );
            assert!(!engine.history.worldlines[0][0].rights.new_information);
            assert_eq!(capture.sq, Sq(3, 5));
        }
    }
}

#[test]
fn undoing_en_passant_removes_its_capture_cause() {
    let mut engine = Engine::new(true);
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
        super::legacy_from_worldlines(vec![vec![state, middle, last]], Color::White).unwrap();
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
    let mut engine = super::legacy_from_worldlines(
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
fn cancelling_a_rook_transition_finds_its_castling_command() {
    for color in [Color::White, Color::Black] {
        for castling in [Castling::Short, Castling::Long] {
            let y = if color == Color::White { 0 } else { 7 };
            let rook_x = if castling == Castling::Short { 7 } else { 0 };
            let states = [
                (PieceKind::K { moved: false }, 4),
                (PieceKind::R { moved: false }, rook_x),
                (PieceKind::R { moved: false }, 7 - rook_x),
            ]
            .into_iter()
            .map(|(kind, x)| {
                let mut state = State::new(kind, color, Sq(x, y));
                state.t = color.first_board();
                state.fut_seen = state.t;
                state.rights.ordinary = true;
                vec![state]
            })
            .collect();
            let mut engine = Engine::from_worldlines(states, color).unwrap();
            let castle = engine
                .possible_moves(0, 0)
                .unwrap()
                .into_iter()
                .find(|mv| mv.castling() == Some(castling))
                .unwrap();
            engine.apply_new_mv(castle).unwrap();
            let castled = engine.clone();

            // Sharing the king's MoveId is not enough: only the participating
            // rook's occurrence is a cause of this castling command.
            assert_eq!(
                engine.undo_last_mv(engine.get_piv(2, 1).unwrap()),
                Err(EngineErr::WrongMove)
            );
            assert_eq!(engine, castled);
            engine.undo_last_mv(engine.get_piv(1, 1).unwrap()).unwrap();
            engine.reconcile().unwrap();
            assert!(!engine.history.moves.contains_key(&castle.id()));
            assert_eq!(engine.get_piv(0, 1).unwrap().sq, Sq(4, y));
            assert_eq!(engine.get_piv(1, 1).unwrap().sq, Sq(rook_x, y));
            assert_eq!(
                engine.get_piv(0, 1).unwrap().kind,
                PieceKind::K { moved: false }
            );
            assert_eq!(
                engine.get_piv(1, 1).unwrap().kind,
                PieceKind::R { moved: false }
            );
            assert!(engine.get_piv(0, 0).unwrap().rights.cancelled_move);

            // A later independent rook move takes precedence over castling.
            let mut engine = castled;
            engine
                .prolongate_wlines(engine.history.horizon, None)
                .unwrap();
            engine.history.horizon += 1;
            engine.turn = color;
            engine.reconcile().unwrap();
            let rook = engine.get_piv(1, 2).unwrap();
            let target = Sq(rook.sq.0, if color == Color::White { 1 } else { 6 });
            let later = engine
                .possible_moves(1, 2)
                .unwrap()
                .into_iter()
                .find(|mv| !mv.to_past() && mv.sq == target)
                .unwrap();
            engine.apply_new_mv(later).unwrap();
            engine.undo_last_mv(engine.get_piv(1, 3).unwrap()).unwrap();
            engine.reconcile().unwrap();
            assert!(engine.history.moves.contains_key(&castle.id()));
            assert!(!engine.history.moves.contains_key(&later.id()));
            assert_eq!(engine.get_piv(1, 3).unwrap().sq, rook.sq);
        }
    }
}

#[test]
fn cancelled_move_reaction_suppresses_new_information() {
    let mut rook = State::new(PieceKind::R { moved: true }, Color::White, Sq(0, 0));
    rook.rights.ordinary = false;
    rook.rights.cancelled_move = true;
    rook.rights.new_information = true;
    let engine = super::legacy_from_worldlines(
        vec![
            vec![rook],
            vec![State::new(PieceKind::N, Color::Black, Sq(0, 4))],
        ],
        Color::White,
    )
    .unwrap();
    let piece = engine.get_piv(0, 0).unwrap();
    assert!(piece.rights.cancelled_move);
    assert!(!piece.rights.new_information);
    assert!(!engine.history.worldlines[0][0].informed.is_empty());
}

#[test]
fn new_information_grants_an_optional_reaction_without_granting_an_ordinary_move() {
    let mut rook = State::new(PieceKind::R { moved: true }, Color::White, Sq(0, 0));
    rook.rights.ordinary = false;
    let mut engine = super::legacy_from_worldlines(
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
            causal_gaps: false,
            worldlines: vec![
                vec![ordinary, ordinary_next].into(),
                vec![inverted, inverted_next, inverted_last].into(),
            ],
            moves: BTreeMap::new(),
            targets: BTreeMap::new(),
            horizon: 2,
            status: GameStatus::Ongoing,
            calculation: Default::default(),
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
