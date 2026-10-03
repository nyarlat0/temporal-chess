use super::*;
use crate::model::{Color, PieceKind, Sq};

fn line(last: usize) -> Vec<State> {
    let mut states = vec![State::new(PieceKind::N, Color::White, Sq(1, 0))];
    for _ in 0..last {
        states.push(states.last().unwrap().prolongate().unwrap());
    }
    states
}

fn game(last: usize) -> Engine {
    let white = line(last);
    let black: Vec<_> = white
        .iter()
        .map(|st| State {
            kind: PieceKind::R { moved: true },
            color: Color::Black,
            x: 7,
            y: 7,
            rights: MoveRights {
                ordinary: !st.t.is_multiple_of(2),
                ..MoveRights::default()
            },
            ..st.clone()
        })
        .collect();
    Engine::from_worldlines(vec![white, black], Color::White).unwrap()
}

// These worldline fixtures select the acting side explicitly; the public command
// must still flip it, and ordinary integration tests exercise alternating play.
fn play(engine: &mut Engine, t: usize, loop_turn: usize, direction: TimeDirection, sq: Sq) -> Move {
    engine.turn = Color::White;
    engine.reconcile().unwrap();
    let piece = engine
        .move_sources()
        .find(|p| p.origin == 0 && p.t == t && p.loop_turn == loop_turn)
        .unwrap_or_else(|| {
            panic!(
                "missing source t={t} loop={loop_turn}: {:?}",
                engine.history.worldlines[0]
            )
        });
    let mv = engine
        .possible_moves(0, piece.age)
        .unwrap()
        .into_iter()
        .find(|mv| mv.direction == direction && mv.sq == sq)
        .unwrap_or_else(|| panic!("missing move t={t} loop={loop_turn} to {sq:?}"));
    engine.apply_new_mv(mv).unwrap();
    assert_eq!(engine.turn, Color::Black);
    mv
}

#[test]
fn latest_board_departure_has_no_copy_on_the_new_future_board() {
    let mut engine = game(4);
    play(&mut engine, 4, 0, TimeDirection::Past, Sq(2, 2));
    assert!(engine.board(5).unwrap().find_by_origin(0).is_empty());
    assert_eq!(engine.board(5).unwrap().find_by_origin(1).len(), 1);
    let states = &engine.history.worldlines[0];
    assert_eq!(
        states.iter().map(|s| s.t).collect::<Vec<_>>(),
        [0, 1, 2, 3, 4, 3, 2]
    );
    assert_eq!(states[5].loop_turn, 1);
    assert_eq!(states[6].loop_pivots, [4]);
    assert!(states[6].rights.ordinary);
    assert_eq!(
        Engine::from_history(engine.history.clone(), engine.turn).unwrap(),
        engine
    );
}

#[test]
fn borrowed_future_is_unchanged_until_return_crosses_the_pivot() {
    let mut engine = game(6);
    let tail = engine.history.worldlines[0][5..].to_vec();
    play(&mut engine, 4, 0, TimeDirection::Past, Sq(2, 2));
    assert_eq!(&engine.history.worldlines[0][7..], tail);
    assert_eq!(engine.history.worldlines[0].gaps().len(), 1);
    assert_eq!(
        Engine::from_history(engine.history.clone(), engine.turn).unwrap(),
        engine
    );
    play(&mut engine, 2, 1, TimeDirection::Future, Sq(4, 3));
    assert_eq!(&engine.history.worldlines[0][9..], tail);
    assert_eq!(engine.history.worldlines[0][8].loop_turn, 2); // At pivot: still in loop.
    play(&mut engine, 4, 2, TimeDirection::Future, Sq(2, 4));
    let states = &engine.history.worldlines[0];
    assert!(states.gaps().is_empty());
    assert_eq!(states.len(), 11); // The preserved future was joined, not duplicated.
    assert_eq!(
        (states[9].t, states[9].loop_turn, states[9].fut_seen),
        (5, 0, 5)
    );
    assert_eq!(Sq(states[9].x, states[9].y), Sq(2, 4));
    assert_eq!(Sq(states[10].x, states[10].y), Sq(2, 4));
    assert_eq!(
        Engine::from_history(engine.history.clone(), engine.turn).unwrap(),
        engine
    );
}

#[test]
fn continuing_backwards_does_not_increment_the_turn_counter() {
    let mut engine = game(8);
    play(&mut engine, 6, 0, TimeDirection::Past, Sq(2, 2));
    play(&mut engine, 4, 1, TimeDirection::Past, Sq(4, 3));
    let states = &engine.history.worldlines[0];
    assert_eq!(states[10].t, 2);
    assert_eq!(states[10].loop_turn, 1);
    assert_eq!(states[10].loop_pivots, [6]);
}

#[test]
fn nested_turns_repay_each_pivot_once() {
    let mut engine = game(10);
    play(&mut engine, 6, 0, TimeDirection::Past, Sq(2, 2));
    play(&mut engine, 4, 1, TimeDirection::Past, Sq(4, 3));
    play(&mut engine, 2, 1, TimeDirection::Future, Sq(5, 5));
    play(&mut engine, 4, 2, TimeDirection::Past, Sq(7, 4));
    play(&mut engine, 2, 3, TimeDirection::Future, Sq(5, 3));
    let tip = engine.history.worldlines[0].gaps()[0].after;
    assert_eq!(tip.loop_turn, 4);
    play(&mut engine, 4, 4, TimeDirection::Future, Sq(3, 4));
    let tip = engine.history.worldlines[0].gaps()[0].after;
    assert_eq!((tip.t, tip.loop_turn), (6, 2));
    play(&mut engine, 6, 2, TimeDirection::Future, Sq(2, 6));
    assert!(engine.history.worldlines[0].gaps().is_empty());
    assert_eq!(
        Engine::from_history(engine.history.clone(), engine.turn).unwrap(),
        engine
    );
}

#[test]
fn replacing_past_with_future_removes_the_excursion_and_its_moves() {
    let mut engine = game(8);
    let root = play(&mut engine, 6, 0, TimeDirection::Past, Sq(2, 2));
    let child = play(&mut engine, 4, 1, TimeDirection::Past, Sq(4, 3));
    let source = engine.projection.pieces[&root.source()];
    engine.history.worldlines[0][source.age]
        .rights
        .new_information = true;
    play(&mut engine, 6, 0, TimeDirection::Future, Sq(0, 2));
    assert_eq!(engine.history.worldlines[0].len(), 9);
    assert!(engine.history.worldlines[0].gaps().is_empty());
    assert!(!engine.history.moves.contains_key(&child.id()));
    assert!(
        engine.history.worldlines[0]
            .iter()
            .all(|state| state.loop_turn == 0)
    );
    assert_eq!(
        engine.history.moves[&root.id()].direction,
        TimeDirection::Future
    );
}

#[test]
fn replacing_past_with_another_past_move_preserves_the_loop_and_future() {
    let mut engine = game(6);
    let tail = engine.history.worldlines[0][5..].to_vec();
    let root = play(&mut engine, 4, 0, TimeDirection::Past, Sq(2, 2));
    let source = engine.projection.pieces[&root.source()];
    engine.history.worldlines[0][source.age]
        .rights
        .new_information = true;
    play(&mut engine, 4, 0, TimeDirection::Past, Sq(0, 2));
    assert_eq!(engine.history.worldlines[0].len(), 9);
    assert_eq!(&engine.history.worldlines[0][7..], tail);
    assert_eq!(engine.history.worldlines[0][5].loop_turn, 1);
    assert_eq!(
        Sq(
            engine.history.worldlines[0][5].x,
            engine.history.worldlines[0][5].y
        ),
        Sq(0, 2)
    );
}

#[test]
fn future_moves_see_future_blockers_and_past_moves_see_past_blockers() {
    let white = line(4);
    let mut blocker = line(4);
    for state in &mut blocker {
        state.x = 7;
        state.y = 4;
        state.kind = PieceKind::N;
    }
    blocker[1].x = 2;
    blocker[1].y = 2;
    blocker[3].x = 0;
    blocker[3].y = 2;
    let engine = Engine::from_worldlines(vec![white, blocker], Color::White).unwrap();
    let moves = engine.possible_moves(0, 2).unwrap();
    assert!(
        moves
            .iter()
            .any(|mv| mv.direction == TimeDirection::Future && mv.sq == Sq(2, 2))
    );
    assert!(
        !moves
            .iter()
            .any(|mv| mv.direction == TimeDirection::Past && mv.sq == Sq(2, 2))
    );
    assert!(
        moves
            .iter()
            .any(|mv| mv.direction == TimeDirection::Past && mv.sq == Sq(0, 2))
    );
    assert!(
        !moves
            .iter()
            .any(|mv| mv.direction == TimeDirection::Future && mv.sq == Sq(0, 2))
    );
}

#[test]
fn replacing_an_existing_forward_move_with_past_keeps_its_future_positions() {
    let mut engine = game(8);
    let forward = play(&mut engine, 4, 0, TimeDirection::Future, Sq(0, 2));
    let tail = engine.history.worldlines[0][5..].to_vec();
    let source = engine.projection.pieces[&forward.source()];
    engine.history.worldlines[0][source.age]
        .rights
        .new_information = true;
    play(&mut engine, 4, 0, TimeDirection::Past, Sq(2, 2));
    assert_eq!(&engine.history.worldlines[0][7..], tail);
    assert_eq!(
        Engine::from_history(engine.history.clone(), engine.turn).unwrap(),
        engine
    );
}

#[test]
fn returning_to_the_future_revalidates_its_preexisting_moves() {
    let mut engine = game(8);
    let future_move = play(&mut engine, 6, 0, TimeDirection::Future, Sq(0, 2));
    let tail = engine.history.worldlines[0][5..].to_vec();
    play(&mut engine, 4, 0, TimeDirection::Past, Sq(2, 2));
    play(&mut engine, 2, 1, TimeDirection::Future, Sq(4, 3));
    assert_eq!(&engine.history.worldlines[0][9..], tail);
    assert!(engine.history.moves.contains_key(&future_move.id()));
    play(&mut engine, 4, 2, TimeDirection::Future, Sq(2, 4));
    assert!(!engine.history.moves.contains_key(&future_move.id()));
    assert!(
        engine
            .get_piv_from_mv(future_move)
            .unwrap()
            .rights
            .cancelled_move
    );
    assert!(engine.history.worldlines[0].gaps().is_empty());
}

#[test]
fn a_completed_loop_can_be_replaced_without_leaving_stale_moves_or_states() {
    let mut engine = game(6);
    let root = play(&mut engine, 4, 0, TimeDirection::Past, Sq(2, 2));
    let returning = play(&mut engine, 2, 1, TimeDirection::Future, Sq(4, 3));
    let closing = play(&mut engine, 4, 2, TimeDirection::Future, Sq(2, 4));
    let source = engine.projection.pieces[&root.source()];
    engine.history.worldlines[0][source.age]
        .rights
        .new_information = true;
    play(&mut engine, 4, 0, TimeDirection::Future, Sq(0, 2));
    assert_eq!(engine.history.worldlines[0].len(), 7);
    assert!(!engine.history.moves.contains_key(&returning.id()));
    assert!(!engine.history.moves.contains_key(&closing.id()));
    assert!(engine.history.worldlines[0].gaps().is_empty());
    assert_eq!(
        Engine::from_history(engine.history.clone(), engine.turn).unwrap(),
        engine
    );
}

#[test]
fn a_nested_gap_joins_its_own_tail_before_the_outer_loop_closes() {
    let mut engine = game(8);
    play(&mut engine, 6, 0, TimeDirection::Past, Sq(2, 2));
    play(&mut engine, 4, 1, TimeDirection::Past, Sq(4, 3));
    let returning = play(&mut engine, 2, 1, TimeDirection::Future, Sq(5, 5));
    let inner_source = engine.projection.pieces[&returning.source()].age + 2;
    play(&mut engine, 4, 2, TimeDirection::Future, Sq(3, 4));
    let tail = engine.history.worldlines[0][inner_source + 1..].to_vec();
    engine.history.worldlines[0][inner_source]
        .rights
        .new_information = true;
    play(&mut engine, 4, 2, TimeDirection::Past, Sq(7, 4));
    assert_eq!(engine.history.worldlines[0].gaps().len(), 2);
    assert_eq!(&engine.history.worldlines[0][inner_source + 3..], tail);
    play(&mut engine, 2, 3, TimeDirection::Future, Sq(5, 3));
    play(&mut engine, 4, 4, TimeDirection::Future, Sq(3, 4));
    assert_eq!(engine.history.worldlines[0].gaps().len(), 1);
    play(&mut engine, 6, 2, TimeDirection::Future, Sq(2, 6));
    assert!(engine.history.worldlines[0].gaps().is_empty());
    assert_eq!(
        Engine::from_history(engine.history.clone(), engine.turn).unwrap(),
        engine
    );
}

#[test]
fn counter_overflow_rejects_the_entire_command_even_after_rights_were_consumed() {
    let mut state = line(4).pop().unwrap();
    state.inverted = true;
    state.loop_turn = usize::MAX;
    let mut engine = Engine::from_worldlines(vec![vec![state]], Color::White).unwrap();
    let before = engine.clone();
    let mv = engine
        .possible_moves(0, 0)
        .unwrap()
        .into_iter()
        .find(|mv| !mv.to_past())
        .unwrap();
    assert_eq!(engine.apply_new_mv(mv), Err(EngineErr::WrongMove));
    assert_eq!(engine, before);
}

#[test]
fn cancelling_a_return_restores_inverted_direction_and_preserves_the_debt() {
    let mut engine = game(6);
    let tail = engine.history.worldlines[0][5..].to_vec();
    play(&mut engine, 4, 0, TimeDirection::Past, Sq(2, 2));
    let returning = play(&mut engine, 2, 1, TimeDirection::Future, Sq(4, 3));
    engine.undo_mv(returning.id()).unwrap();
    engine.reconcile().unwrap();
    assert_eq!(&engine.history.worldlines[0][9..], tail);
    assert_eq!(engine.history.worldlines[0][8].t, 0);
    assert_eq!(engine.history.worldlines[0][8].loop_turn, 1);
    assert!(engine.history.worldlines[0][8].inverted);
    assert_eq!(
        Engine::from_history(engine.history.clone(), engine.turn).unwrap(),
        engine
    );
}

#[test]
fn empty_future_board_survives_departure_and_snapshot_restore() {
    let mut engine = Engine::from_worldlines(vec![line(4)], Color::White).unwrap();
    play(&mut engine, 4, 0, TimeDirection::Past, Sq(2, 2));
    assert_eq!(engine.history.horizon(), 5);
    assert!(engine.board(5).unwrap().pieces().is_empty());
    assert_eq!(
        Engine::from_history(engine.history.clone(), engine.turn).unwrap(),
        engine
    );
}

#[test]
fn a_frontier_loop_closes_without_duplicating_its_incarnation() {
    let mut engine = game(4);
    play(&mut engine, 4, 0, TimeDirection::Past, Sq(2, 2));
    play(&mut engine, 2, 1, TimeDirection::Future, Sq(4, 3));
    play(&mut engine, 4, 2, TimeDirection::Future, Sq(2, 4));
    assert!(engine.history.worldlines[0].gaps().is_empty());
    assert_eq!(engine.board(5).unwrap().find_by_origin(0).len(), 1);
    assert_eq!(engine.history.worldlines[0].last().unwrap().loop_turn, 0);
    assert_eq!(
        Engine::from_history(engine.history.clone(), engine.turn).unwrap(),
        engine
    );
}

#[test]
fn a_backward_pawn_capture_survives_validation_and_undo_restores_the_victim() {
    let white: Vec<_> = line(4)
        .into_iter()
        .map(|state| State {
            x: 3,
            y: 3,
            kind: PieceKind::P { moved: true },
            ..state
        })
        .collect();
    let black: Vec<_> = line(4)
        .into_iter()
        .map(|state| State {
            x: 4,
            y: 4,
            color: Color::Black,
            rights: MoveRights::default(),
            ..state
        })
        .collect();
    let mut engine = Engine::from_worldlines(vec![white, black], Color::White).unwrap();
    let capture = play(&mut engine, 4, 0, TimeDirection::Past, Sq(4, 4));
    assert!(engine.history.moves.contains_key(&capture.id()));
    assert!(engine.history.worldlines[1][1].alive);
    assert!(
        engine.history.worldlines[1][2..]
            .iter()
            .all(|state| !state.alive)
    );
    assert_eq!(
        Engine::from_history(engine.history.clone(), engine.turn).unwrap(),
        engine
    );
    engine.undo_mv(capture.id()).unwrap();
    engine.reconcile().unwrap();
    assert!(engine.history.worldlines[1].iter().all(|state| state.alive));
    assert_eq!(
        Engine::from_history(engine.history.clone(), engine.turn).unwrap(),
        engine
    );
}

#[test]
fn king_safety_is_checked_on_the_selected_target_board() {
    let white: Vec<_> = line(4)
        .into_iter()
        .map(|state| State {
            x: 4,
            y: 0,
            kind: PieceKind::K { moved: true },
            ..state
        })
        .collect();
    let black: Vec<_> = line(4)
        .into_iter()
        .map(|state| State {
            x: if state.t == 1 { 3 } else { 5 },
            y: 7,
            kind: PieceKind::R { moved: true },
            color: Color::Black,
            rights: MoveRights::default(),
            ..state
        })
        .collect();
    let engine = Engine::from_worldlines(vec![white, black], Color::White).unwrap();
    let moves = engine.possible_moves(0, 2).unwrap();
    for (direction, allowed, attacked) in [
        (TimeDirection::Past, Sq(5, 0), Sq(3, 0)),
        (TimeDirection::Future, Sq(3, 0), Sq(5, 0)),
    ] {
        assert!(
            moves
                .iter()
                .any(|mv| mv.direction == direction && mv.sq == allowed)
        );
        assert!(
            !moves
                .iter()
                .any(|mv| mv.direction == direction && mv.sq == attacked)
        );
    }
}

#[test]
fn replacing_a_deeper_backward_move_removes_only_its_excursion() {
    let mut engine = game(8);
    let root = play(&mut engine, 6, 0, TimeDirection::Past, Sq(2, 2));
    let child = play(&mut engine, 4, 1, TimeDirection::Past, Sq(4, 3));
    let returning = play(&mut engine, 2, 1, TimeDirection::Future, Sq(5, 5));
    play(&mut engine, 4, 2, TimeDirection::Future, Sq(3, 4));
    let source = engine.projection.pieces[&child.source()];
    engine.history.worldlines[0][source.age]
        .rights
        .new_information = true;
    play(&mut engine, 4, 1, TimeDirection::Future, Sq(0, 3));
    assert_eq!(engine.history.moves[&root.id()], root);
    assert!(!engine.history.moves.contains_key(&returning.id()));
    assert_eq!(engine.history.worldlines[0].gaps().len(), 1);
    assert_eq!(
        Engine::from_history(engine.history.clone(), engine.turn).unwrap(),
        engine
    );
}

#[test]
fn a_new_capture_on_an_adjacent_board_grants_a_reaction() {
    for target_time in [1, 3] {
        let mut white = line(4);
        white[2].rights.ordinary = false;
        let black: Vec<_> = line(4)
            .into_iter()
            .map(|state| State {
                x: 7,
                y: 7,
                color: Color::Black,
                rights: MoveRights::default(),
                ..state
            })
            .collect();
        let mut engine = Engine::from_worldlines(vec![white, black], Color::White).unwrap();
        assert!(!engine.history.worldlines[0][2].rights.is_movable());
        engine.history.worldlines[1][target_time].x = 2;
        engine.history.worldlines[1][target_time].y = 2;
        engine.reconcile().unwrap();
        let source = &engine.history.worldlines[0][2];
        assert!(source.rights.new_information);
        assert!(!source.rights.ordinary);
        assert!(source.informed.contains(&PieceId::from_state(
            &engine.history.worldlines[1][target_time],
            1
        )));
    }
}

#[test]
fn mixed_direction_games_keep_restorable_history_and_atomic_failures() {
    let mut backward_moves = 0;
    for initial_seed in [11_u64, 29, 47, 83] {
        let mut seed = initial_seed;
        let mut engine = Engine::default();
        for turn in 0..24 {
            let moves: Vec<_> = engine
                .move_sources()
                .flat_map(|piece| engine.possible_moves(piece.origin, piece.age).unwrap())
                .collect();
            if moves.is_empty() {
                break;
            }
            for _ in 0..20 {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let mv = moves[(seed as usize) % moves.len()];
                let before = engine.clone();
                match engine.apply_new_mv(mv) {
                    Ok(()) => {
                        backward_moves += usize::from(mv.to_past());
                        let restored = Engine::from_history(engine.history.clone(), engine.turn)
                            .unwrap_or_else(|err| {
                                panic!("seed {initial_seed}, turn {turn}, {mv:?}: {err:?}")
                            });
                        assert_eq!(restored, engine, "seed {initial_seed}, turn {turn}");
                        break;
                    }
                    Err(_) => assert_eq!(engine, before),
                }
            }
        }
    }
    assert!(
        backward_moves >= 8,
        "only {backward_moves} backward moves exercised"
    );
}

#[test]
fn equal_pivots_are_both_repaid_only_after_their_board_time() {
    let mut state = line(6).pop().unwrap();
    for direction in [
        TimeDirection::Past,
        TimeDirection::Future,
        TimeDirection::Past,
        TimeDirection::Future,
    ] {
        state = state.step(direction).unwrap();
    }
    assert_eq!((state.t, state.loop_turn), (6, 4));
    assert_eq!(state.loop_pivots, [6, 6]);
    state = state.step(TimeDirection::Future).unwrap();
    assert_eq!((state.t, state.loop_turn), (7, 0));
    assert!(state.loop_pivots.is_empty());
}

#[test]
fn replacing_castling_restores_rook_and_inversion_preserves_king_future() {
    let king: Vec<_> = line(4)
        .into_iter()
        .map(|state| State {
            x: 4,
            y: 0,
            kind: PieceKind::K { moved: false },
            ..state
        })
        .collect();
    let rook: Vec<_> = line(4)
        .into_iter()
        .map(|state| State {
            x: 7,
            y: 0,
            kind: PieceKind::R { moved: false },
            ..state
        })
        .collect();
    let mut engine = Engine::from_worldlines(vec![king, rook], Color::White).unwrap();
    let castling = play(&mut engine, 2, 0, TimeDirection::Future, Sq(6, 0));
    assert!(castling.castling().is_some());
    engine.history.worldlines[0][2].rights.new_information = true;
    let before = engine.clone();
    play(&mut engine, 2, 0, TimeDirection::Future, Sq(5, 0));
    assert_eq!(engine.get_piv(1, 3).unwrap().sq, Sq(7, 0));
    assert_eq!(
        engine.get_piv(1, 3).unwrap().kind,
        PieceKind::R { moved: false }
    );
    assert_eq!(engine.get_piv(0, 3).unwrap().sq, Sq(5, 0));
    assert_eq!(
        Engine::from_history(engine.history.clone(), engine.turn).unwrap(),
        engine
    );

    engine = before;
    let tail = engine.history.worldlines[0][3..].to_vec();
    play(&mut engine, 2, 0, TimeDirection::Past, Sq(3, 1));
    assert_eq!(&engine.history.worldlines[0][5..], tail);
    assert_eq!(engine.get_piv(1, 3).unwrap().sq, Sq(7, 0));
    assert_eq!(
        engine.get_piv(1, 3).unwrap().kind,
        PieceKind::R { moved: false }
    );
    assert_eq!(
        Engine::from_history(engine.history.clone(), engine.turn).unwrap(),
        engine
    );
}

#[test]
fn cancelling_a_return_at_time_zero_keeps_the_inverted_endpoint() {
    let mut engine = game(6);
    let tail = engine.history.worldlines[0][5..].to_vec();
    play(&mut engine, 4, 0, TimeDirection::Past, Sq(2, 2));
    play(&mut engine, 2, 1, TimeDirection::Past, Sq(4, 3));
    let returning = play(&mut engine, 0, 1, TimeDirection::Future, Sq(5, 5));
    engine.undo_mv(returning.id()).unwrap();
    engine.reconcile().unwrap();
    assert_eq!(&engine.history.worldlines[0][9..], tail);
    let endpoint = &engine.history.worldlines[0][8];
    assert_eq!(endpoint.t, 0);
    assert!(endpoint.inverted);
    assert!(endpoint.rights.cancelled_move);
    assert_eq!(
        engine.history.worldlines[0].gaps()[0].after,
        MoveId::from_state(endpoint)
    );
    assert_eq!(
        Engine::from_history(engine.history.clone(), engine.turn).unwrap(),
        engine
    );
}
