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
    assert!(moves.iter().any(|mv| mv.direction == TimeDirection::Future));
    play(
        &mut engine,
        &mut record,
        8,
        Sq(6, 0),
        Sq(5, 2),
        TimeDirection::Future,
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
    assert!(moves.iter().any(|mv| mv.direction == TimeDirection::Past));
    play(
        &mut engine,
        &mut record,
        1,
        Sq(1, 7),
        Sq(2, 5),
        TimeDirection::Past,
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
    let white = engine.board(8).unwrap().get_piece(5, 2).unwrap();
    assert!(white.inverted && white.rights.ordinary);
    let black = engine.board(1).unwrap().get_piece(2, 5).unwrap();
    assert!(!black.inverted && black.rights.ordinary);
    assert!(engine.board(9).unwrap().get_piece(5, 2).is_some());
    assert!(engine.board(0).unwrap().get_piece(2, 5).is_some());
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

#[test]
fn castling_at_white_frontier_uses_source_rook_and_replays() {
    let (mut engine, mut record) = GameRecord::load(CASTLING_FRONTIER).unwrap();
    let before = engine.clone();
    assert!(engine.board(11).unwrap().pieces().is_empty());
    let king = *engine.board(10).unwrap().get_piece(4, 0).unwrap();
    let castle = engine
        .possible_moves(king.origin, king.age)
        .unwrap()
        .into_iter()
        .find(|mv| mv.castling() == Some(temporal_chess_engine::model::Castling::Short))
        .expect("the rook on t=10 participates even though t=11 is empty");
    record
        .apply(
            &mut engine,
            Command::Move {
                mv: castle,
                from: king.sq,
            },
        )
        .unwrap();
    for (x, wid) in [(6, 4), (5, 7)] {
        assert_eq!(
            engine.board(11).unwrap().get_piece(x, 0).unwrap().origin,
            wid
        );
    }
    assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, engine);
    assert_eq!(
        Engine::from_history(engine.history().clone(), engine.turn())
            .unwrap()
            .history(),
        engine.history()
    );
    let (prefix, _) = record.without_last_move().unwrap();
    assert_eq!(GameRecord::load(&prefix.to_text()).unwrap().0, before);
}

const CASTLING_FRONTIER: &str = "NOLAN-CHESS 4
chain_info_activation off
global_reaction_reset on
start time_war 23
move 0 14:0:0 g2 g3 + normal
move 23 17:23:0 b8 c6 - normal
move 2 6:2:0 g1 f3 + normal
move 21 28:23:0 e7 e5 - normal
move 4 5:4:0 f1 g2 + normal
move 19 22:23:0 g8 f6 - normal
move 6 11:6:0 d2 d4 + normal
move 17 27:23:0 d7 d5 - normal
move 8 1:8:0 b1 c3 + normal
move 15 21:23:0 f8 d6 - normal";

#[test]
fn inverted_black_castles_at_past_frontier_and_replays() {
    let text = format!("{CASTLING_FRONTIER}\nmove 10 12:10:0 e2 e4 + normal");
    let (mut engine, mut record) = GameRecord::load(&text).unwrap();
    let before = engine.clone();
    assert!(
        engine
            .board(12)
            .unwrap()
            .pieces()
            .iter()
            .all(|p| p.color == Color::White)
    );
    let king = *engine.board(13).unwrap().get_piece(4, 7).unwrap();
    let rook = *engine.board(13).unwrap().get_piece(7, 7).unwrap();
    assert!(king.inverted && rook.inverted);
    let castle = engine
        .possible_moves(king.origin, king.age)
        .unwrap()
        .into_iter()
        .find(|mv| {
            mv.castling() == Some(temporal_chess_engine::model::Castling::Short)
                && mv.direction == TimeDirection::Past
        })
        .expect("inverted king and rook can castle backwards");
    record
        .apply(
            &mut engine,
            Command::Move {
                mv: castle,
                from: king.sq,
            },
        )
        .unwrap();
    for (x, wid) in [(6, king.origin), (5, rook.origin)] {
        let p = engine.board(12).unwrap().get_piece(x, 7).unwrap();
        assert_eq!(p.origin, wid);
        assert!(p.inverted);
    }
    assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, engine);
    assert_eq!(
        Engine::from_history(engine.history().clone(), engine.turn())
            .unwrap()
            .history(),
        engine.history()
    );
    let (prefix, _) = record.without_last_move().unwrap();
    assert_eq!(GameRecord::load(&prefix.to_text()).unwrap().0, before);
}

#[test]
fn frontier_move_generation_includes_passive_friendly_blockers() {
    for (t, color, direction) in [
        (0, Color::White, TimeDirection::Future),
        (9, Color::Black, TimeDirection::Past),
    ] {
        let mut engine = game(9);
        if color == Color::Black {
            play(
                &mut engine,
                &mut GameRecord::default(),
                0,
                Sq(4, 1),
                Sq(4, 3),
                TimeDirection::Future,
            );
        }
        let pieces = engine.board(t).unwrap().pieces();
        let mut count = 0;
        for piece in pieces.iter().filter(|p| p.color == color) {
            for mv in engine
                .possible_moves(piece.origin, piece.age)
                .unwrap()
                .into_iter()
                .filter(|mv| mv.direction == direction)
            {
                let mut next = engine.clone();
                next.apply_new_mv(mv).unwrap();
                assert_eq!(
                    next.history().moves().get(&mv.id()),
                    Some(&mv),
                    "frontier move must not immediately cancel: {mv:?}"
                );
                count += 1;
            }
        }
        assert_eq!(count, 20, "only normal opening moves fit through the army");
    }
}
