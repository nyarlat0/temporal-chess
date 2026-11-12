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
            let victim_id = crate::model::Observation::from_piv(engine.get_piv(1, 1).unwrap());
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

            // Replaying an earlier reaction can put a friendly passive piece
            // in the rook's arrival cell. Collision resolution must find the
            // king's castling command, rather than a nonexistent rook move.
            let mut collision = castled.clone();
            let rook_arrival = collision.get_piv(1, 1).unwrap();
            let mut blocker = State::new(PieceKind::N, color, rook_arrival.sq);
            blocker.t = color.first_board();
            blocker.fut_seen = blocker.t;
            blocker.rights = MoveRights::default();
            let next = blocker.step(TimeDirection::Future).unwrap();
            collision
                .history
                .worldlines
                .push(vec![blocker, next].into());
            collision.update_bufs().unwrap();
            assert_eq!(
                collision.build_boards(rook_arrival.t),
                Ok(ValidRes::HistChanged)
            );
            assert!(!collision.history.moves.contains_key(&castle.id()));
            collision.reconcile().unwrap();
            assert_eq!(collision.get_piv(0, 1).unwrap().sq, Sq(4, y));
            assert_eq!(collision.get_piv(1, 1).unwrap().sq, Sq(rook_x, y));

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
    let mut inverted_last = inverted_next.step(TimeDirection::Past).unwrap();
    inverted_last.x = 1;
    inverted_last.y = 3;
    let engine = Engine {
        history: History {
            rules: Default::default(),
            starting_position: Default::default(),
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
        vec![KillFrom(ShortPieceId { wid: 1, age: 1 })]
    );
}

#[test]
fn castling_separates_source_participants_from_destination_occupancy() {
    use crate::board::Board;
    let king = PieceView::from_state(
        &State::new(PieceKind::K { moved: false }, Color::White, Sq(4, 0)),
        0,
        0,
    );
    let rook = PieceView::from_state(
        &State::new(PieceKind::R { moved: false }, Color::White, Sq(7, 0)),
        1,
        0,
    );
    let mut source = Board::empty(10);
    source.add_piece(king);
    source.add_piece(rook);
    let destination = Board::empty(11);
    let moves = |source: &Board, destination: &Board| {
        king.castling_moves_with_attacks(destination, source, |_| false)
    };
    assert_eq!(
        moves(&source, &destination),
        vec![(Sq(6, 0), Castling::Short)]
    );
    let mut missing = source.clone();
    missing.remove_piece(rook.sq);
    let mut future_rook = destination.clone();
    future_rook.add_piece(rook);
    assert!(moves(&missing, &future_rook).is_empty());
    for kind in [PieceKind::R { moved: true }, PieceKind::B] {
        let mut invalid = missing.clone();
        invalid.add_piece(PieceView { kind, ..rook });
        assert!(moves(&invalid, &destination).is_empty());
    }
    for x in [5, 6] {
        let mut blocked = destination.clone();
        blocked.add_piece(PieceView {
            sq: Sq(x, 0),
            ..rook
        });
        assert!(moves(&source, &blocked).is_empty());
    }
    for x in [4, 5, 6] {
        assert!(
            king.castling_moves_with_attacks(&destination, &source, |sq| sq == Sq(x, 0))
                .is_empty()
        );
    }
    let moved_king = PieceView {
        kind: PieceKind::K { moved: true },
        ..king
    };
    assert!(
        moved_king
            .castling_moves_with_attacks(&destination, &source, |_| false)
            .is_empty()
    );
}

#[test]
fn backward_castling_requires_two_inverted_participants_and_can_be_cancelled() {
    for color in [Color::White, Color::Black] {
        for side in [Castling::Short, Castling::Long] {
            for (king_inv, rook_inv) in [(false, false), (false, true), (true, false), (true, true)]
            {
                let y = if color == Color::White { 0 } else { 7 };
                let rook_x = if side == Castling::Short { 7 } else { 0 };
                let t = 4 + color.first_board();
                let states = [
                    (PieceKind::K { moved: false }, 4, king_inv),
                    (PieceKind::R { moved: false }, rook_x, rook_inv),
                ]
                .into_iter()
                .map(|(kind, x, inverted)| {
                    let mut state = State::new(kind, color, Sq(x, y));
                    state.t = t;
                    state.fut_seen = t;
                    state.inverted = inverted;
                    state.rights.ordinary = true;
                    let mut next = state.prolongate().unwrap();
                    next.rights.ordinary = false;
                    vec![state, next]
                })
                .collect();
                let mut engine = Engine::from_worldlines(states, color).unwrap();
                let castle = engine
                    .possible_moves(0, 0)
                    .unwrap()
                    .into_iter()
                    .find(|mv| mv.castling() == Some(side) && mv.to_past());
                assert_eq!(
                    castle.is_some(),
                    king_inv && rook_inv,
                    "{color:?} {side:?} king={king_inv} rook={rook_inv}"
                );
                if let Some(castle) = castle {
                    engine.apply_new_mv(castle).unwrap();
                    let arrival = engine.get_piv(1, 1).unwrap();
                    assert_eq!(arrival.t, t - 1);
                    assert_eq!(
                        arrival.sq,
                        Sq(if side == Castling::Short { 5 } else { 3 }, y)
                    );
                    engine.undo_last_mv(arrival).unwrap();
                    engine.reconcile().unwrap();
                    assert!(!engine.history.moves.contains_key(&castle.id()));
                    for (wid, x) in [(0, 4), (1, rook_x)] {
                        let restored = engine.get_piv(wid, 1).unwrap();
                        assert_eq!(restored.sq, Sq(x, y));
                        assert!(restored.inverted);
                        assert_eq!(
                            restored.kind,
                            if wid == 0 {
                                PieceKind::K { moved: false }
                            } else {
                                PieceKind::R { moved: false }
                            }
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn cancelling_boundary_move_removes_its_automatic_turn() {
    for color in [Color::White, Color::Black] {
        let mut engine = Engine::with_start(
            crate::model::GameRules {
                wait_before_move: true,
                ..Default::default()
            },
            crate::model::StartingPosition::TimeWar { separation: 3 },
        )
        .unwrap();
        engine.turn = color;
        let (from_t, t, x, y, direction) = if color == Color::White {
            (0, 2, 6, 0, TimeDirection::Future)
        } else {
            (3, 1, 6, 7, TimeDirection::Past)
        };
        let piece = *engine.board(from_t).unwrap().get_piece(x, y).unwrap();
        engine.skip_ordinary(Engine::piece_at(piece), t).unwrap();
        let piece = *engine.board(t).unwrap().get_piece(x, y).unwrap();
        let mv = engine
            .possible_moves(piece.origin, piece.age)
            .unwrap()
            .into_iter()
            .find(|mv| {
                mv.direction == direction
                    && mv.sq == Sq(5, if color == Color::White { 2 } else { 5 })
            })
            .unwrap();
        engine.apply_new_mv(mv).unwrap();
        let line = &engine.history.worldlines[piece.origin];
        assert_eq!(line[piece.age + 2].t, t);
        assert_ne!(line[piece.age + 2].inverted, piece.inverted);
        engine.undo_mv(mv.id()).unwrap();
        engine.reconcile().unwrap();
        let line = &engine.history.worldlines[piece.origin];
        assert_eq!(line.len(), piece.age + 1);
        assert!(line[piece.age].rights.ordinary);
        assert_eq!(line[piece.age].inverted, piece.inverted);
    }
}

#[test]
fn black_rebounds_at_zero_in_standard_game_without_bouncing_idle_piece() {
    let mut knight = State::new(PieceKind::N, Color::Black, Sq(6, 7));
    knight.t = 1;
    knight.fut_seen = 1;
    knight.inverted = true;
    knight.rights.ordinary = true;
    let mut idle = knight.clone();
    idle.x = 1;
    let mut engine = Engine::from_worldlines(
        vec![
            vec![knight],
            vec![idle],
            vec![State::new(
                PieceKind::P { moved: false },
                Color::White,
                Sq(0, 1),
            )],
        ],
        Color::Black,
    )
    .unwrap();
    let mv = engine
        .possible_moves(0, 0)
        .unwrap()
        .into_iter()
        .find(|mv| mv.to_past() && mv.sq == Sq(5, 5))
        .unwrap();
    engine.apply_new_mv(mv).unwrap();
    let line = &engine.history.worldlines[0];
    assert_eq!(line.iter().map(|s| s.t).collect::<Vec<_>>(), vec![1, 0, 1]);
    assert!(!line[2].inverted && line[2].rights.ordinary);
    assert_eq!(line[2].loop_turn, 1);
    assert_eq!(engine.history.worldlines[1].len(), 1);
    assert!(engine.history.worldlines[1][0].rights.ordinary);
}
