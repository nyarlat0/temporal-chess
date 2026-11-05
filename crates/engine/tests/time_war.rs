use temporal_chess_engine::{
    engine::Engine,
    model::{Color, GameRules, Sq, StartingPosition, TimeDirection},
    record::{Command, GameRecord},
};

fn game(n: usize) -> Engine {
    Engine::with_start(
        GameRules::default(),
        StartingPosition::TimeWar { separation: n },
    )
    .unwrap()
    .with_interactive_calculation()
    .unwrap()
}
fn play(
    engine: &mut Engine,
    record: &mut GameRecord,
    t: usize,
    from: Sq,
    to: Sq,
    direction: TimeDirection,
) {
    let p = *engine
        .board(t)
        .unwrap()
        .get_piece(from.0, from.1)
        .expect("source");
    let moves = engine.possible_moves(p.origin, p.age).unwrap();
    let mv = *moves
        .iter()
        .find(|mv| mv.sq == to && mv.direction == direction)
        .unwrap_or_else(|| panic!("no move {from:?}->{to:?} at {t}: {moves:?}"));
    record.apply(engine, Command::Move { mv, from }).unwrap();
}
#[test]
fn separate_armies_advance_in_opposite_directions_and_replay() {
    let mut engine = game(9);
    for t in 0..=9 {
        let b = engine.board(t).expect("all empty boards exist");
        assert_eq!(b.pieces().len(), if t == 0 || t == 9 { 16 } else { 0 });
        assert!(
            b.pieces()
                .iter()
                .all(|p| p.color == if t == 0 { Color::White } else { Color::Black })
        );
    }
    assert!(!engine.has_past_debt(Color::White));
    assert!(!engine.has_past_debt(Color::Black));
    let mut record = GameRecord::with_start(
        GameRules::default(),
        StartingPosition::TimeWar { separation: 9 },
    );
    for (t, from, to, direction) in [
        (0, Sq(4, 1), Sq(4, 3), TimeDirection::Future),
        (9, Sq(4, 6), Sq(4, 4), TimeDirection::Past),
        (2, Sq(6, 0), Sq(5, 2), TimeDirection::Future),
        (7, Sq(1, 7), Sq(2, 5), TimeDirection::Past),
    ] {
        play(&mut engine, &mut record, t, from, to, direction);
        assert_eq!(engine.history().horizon(), 9);
        assert!(
            engine
                .history()
                .worldlines()
                .iter()
                .flat_map(|l| l.iter())
                .all(|s| s.t <= 9)
        );
        assert_eq!(
            Engine::from_history(engine.history().clone(), engine.turn())
                .unwrap()
                .history(),
            engine.history()
        );
    }
    assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, engine);
    let (prefix, _) = record.without_last_move().unwrap();
    assert_eq!(
        GameRecord::load(&prefix.to_text())
            .unwrap()
            .0
            .history()
            .starting_position(),
        StartingPosition::TimeWar { separation: 9 }
    );
}
#[test]
fn invalid_bounds_rejected() {
    for n in [0, 1, 2, 4, 256, usize::MAX] {
        assert!(
            Engine::with_start(
                GameRules::default(),
                StartingPosition::TimeWar { separation: n }
            )
            .is_err()
        );
    }
}
#[test]
fn offered_moves_always_apply_with_fixed_bounds() {
    for n in [3, 5, 9] {
        let mut engine = game(n);
        for _ in 0..8 {
            let sources: Vec<_> = engine.move_sources().collect();
            let mut successor = None;
            for p in sources {
                for mv in engine.possible_moves(p.origin, p.age).unwrap() {
                    assert!(mv.direction.step(mv.t).unwrap() <= n);
                    let mut next = engine.clone();
                    next.apply_new_mv(mv)
                        .unwrap_or_else(|e| panic!("n={n} {mv:?}: {e}"));
                    assert_eq!(next.history().horizon(), n);
                    assert!(
                        next.history()
                            .worldlines()
                            .iter()
                            .flat_map(|l| l.iter())
                            .all(|s| s.t <= n)
                    );
                    if successor.is_none() {
                        successor = Some(next);
                    }
                }
            }
            let Some(next) = successor else { break };
            engine = next;
        }
    }
}

fn advance(engine: &mut Engine, record: &mut GameRecord, t: usize, square: Sq, to_t: usize) {
    let piece = *engine
        .board(t)
        .unwrap()
        .get_piece(square.0, square.1)
        .unwrap();
    let source = temporal_chess_engine::model::PieceAt {
        piece: temporal_chess_engine::model::PieceId::from_piv(piece),
        t,
    };
    assert!(engine.possible_skips(source).contains(&to_t));
    record
        .apply(
            engine,
            Command::Advance {
                source,
                from: square,
                to_t,
            },
        )
        .unwrap();
}

#[test]
fn both_boundaries_keep_reserves_and_allow_return_but_never_escape() {
    let mut engine = game(9);
    let mut record = GameRecord::default();
    for sq in [Sq(1, 0), Sq(6, 0)] {
        advance(&mut engine, &mut record, 0, sq, 8);
    }
    let knight = *engine.board(8).unwrap().get_piece(6, 0).unwrap();
    let moves = engine.possible_moves(knight.origin, knight.age).unwrap();
    assert!(!moves.is_empty());
    assert!(moves.iter().all(|mv| mv.direction == TimeDirection::Past));
    let mut forbidden = moves[0];
    forbidden.direction = TimeDirection::Future;
    let snapshot = engine.clone();
    assert!(engine.apply_new_mv(forbidden).is_err());
    assert_eq!(engine, snapshot);
    play(
        &mut engine,
        &mut record,
        8,
        Sq(6, 0),
        Sq(5, 2),
        TimeDirection::Past,
    );
    assert!(
        engine
            .board(8)
            .unwrap()
            .get_piece(1, 0)
            .unwrap()
            .rights
            .ordinary
    );
    assert_eq!(engine.turn(), Color::Black);
    for sq in [Sq(1, 7), Sq(6, 7)] {
        advance(&mut engine, &mut record, 9, sq, 1);
    }
    let knight = *engine.board(1).unwrap().get_piece(1, 7).unwrap();
    let moves = engine.possible_moves(knight.origin, knight.age).unwrap();
    assert!(!moves.is_empty());
    assert!(moves.iter().all(|mv| mv.direction == TimeDirection::Future));
    let mut forbidden = moves[0];
    forbidden.direction = TimeDirection::Past;
    let snapshot = engine.clone();
    assert!(engine.apply_new_mv(forbidden).is_err());
    assert_eq!(engine, snapshot);
    play(
        &mut engine,
        &mut record,
        1,
        Sq(1, 7),
        Sq(2, 5),
        TimeDirection::Future,
    );
    assert!(
        engine
            .board(1)
            .unwrap()
            .get_piece(6, 7)
            .unwrap()
            .rights
            .ordinary
    );
    assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, engine);
    assert_eq!(engine.history().horizon(), 9);
}

#[test]
fn natural_inversion_can_turn_twice_without_a_synthetic_loop() {
    let mut engine = game(9);
    let mut record = GameRecord::default();
    play(
        &mut engine,
        &mut record,
        0,
        Sq(4, 1),
        Sq(4, 3),
        TimeDirection::Future,
    );
    play(
        &mut engine,
        &mut record,
        9,
        Sq(6, 7),
        Sq(5, 5),
        TimeDirection::Past,
    );
    play(
        &mut engine,
        &mut record,
        2,
        Sq(3, 1),
        Sq(3, 3),
        TimeDirection::Future,
    );
    play(
        &mut engine,
        &mut record,
        7,
        Sq(5, 5),
        Sq(7, 4),
        TimeDirection::Future,
    );
    play(
        &mut engine,
        &mut record,
        4,
        Sq(6, 0),
        Sq(5, 2),
        TimeDirection::Future,
    );
    play(
        &mut engine,
        &mut record,
        9,
        Sq(7, 4),
        Sq(5, 3),
        TimeDirection::Past,
    );
    assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, engine);
    assert_eq!(
        Engine::from_history(engine.history().clone(), engine.turn())
            .unwrap()
            .history(),
        engine.history()
    );
    let knight = engine.history().worldlines()[22].last().unwrap();
    assert_eq!(knight.loop_turn, 2);
    assert!(knight.inverted);
    assert_eq!(knight.loop_pivots, [9]);
}

#[test]
fn starting_position_and_options_round_trip_even_without_commands() {
    for chain in [false, true] {
        for global in [false, true] {
            let rules = GameRules {
                chain_info_activation: chain,
                global_reaction_reset: global,
            };
            for starting_position in [
                StartingPosition::Standard,
                StartingPosition::TimeWar { separation: 3 },
                StartingPosition::TimeWar { separation: 255 },
            ] {
                let record = GameRecord::with_start(rules, starting_position);
                let (engine, restored) = GameRecord::load(&record.to_text()).unwrap();
                assert_eq!(record, restored);
                assert_eq!(engine.history().rules(), rules);
                assert_eq!(engine.history().starting_position(), starting_position);
            }
        }
    }
    for text in [
        "NOLAN-CHESS 1",
        "NOLAN-CHESS 2\ncausal_gaps off",
        "NOLAN-CHESS 3\nchain_info_activation off\nglobal_reaction_reset off",
    ] {
        assert_eq!(
            GameRecord::load(text)
                .unwrap()
                .0
                .history()
                .starting_position(),
            StartingPosition::Standard
        );
    }
    assert!(
        GameRecord::load(
            "NOLAN-CHESS 4\nchain_info_activation off\nglobal_reaction_reset off\nstart time_war 8"
        )
        .is_err()
    );
}
