use temporal_chess_engine::{
    engine::Engine,
    model::{Castling, Color, GameRules, Move, PieceKind, Sq, TimeDirection},
    record::{Command, GameRecord},
};

fn sq(name: &str) -> Sq {
    Sq(
        (name.as_bytes()[0] - b'a') as usize,
        (name.as_bytes()[1] - b'1') as usize,
    )
}

fn prepared(color: Color, side: Castling, rules: GameRules) -> (Engine, GameRecord) {
    let moves = match (color, side) {
        (_, Castling::Short) => "g2g3 g7g6 f1g2 f8g7 g1f3 g8f6 a2a3 a7a6",
        (Color::White, Castling::Long) => "d2d4 d7d5 c1e3 g8f6 d1d2 e7e6 b1c3 f8e7 a2a3 a7a6",
        (Color::Black, Castling::Long) => "d2d4 d7d5 g1f3 c8e6 e2e3 d8d7 f1e2 b8c6 a2a3 a7a6",
    };
    let mut engine = Engine::new(rules).with_interactive_calculation().unwrap();
    let mut record = GameRecord::new(rules);
    for text in moves
        .split_whitespace()
        .chain((color == Color::Black).then_some("h2h3"))
    {
        let t = engine.history().horizon();
        let piece = *engine
            .board(t)
            .unwrap()
            .get_piece(sq(&text[..2]).0, sq(&text[..2]).1)
            .unwrap();
        let mv = engine
            .possible_moves(piece.origin, piece.age)
            .unwrap()
            .into_iter()
            .find(|mv| mv.sq == sq(&text[2..]) && mv.direction == TimeDirection::Future)
            .unwrap_or_else(|| panic!("opening {text} on t={t}: {piece:?}\n{}", record.to_text()));
        record
            .apply(&mut engine, Command::Move { mv, from: piece.sq })
            .unwrap();
    }
    assert_eq!(engine.turn(), color);
    (engine, record)
}

fn castle(engine: &Engine, side: Castling, direction: TimeDirection) -> Move {
    let y = if engine.turn() == Color::White { 0 } else { 7 };
    let king = *engine
        .board(engine.history().horizon())
        .unwrap()
        .get_piece(4, y)
        .unwrap();
    assert_eq!(king.kind, PieceKind::K { moved: false });
    engine
        .possible_moves(king.origin, king.age)
        .unwrap()
        .into_iter()
        .find(|mv| mv.castling() == Some(side) && mv.direction == direction)
        .unwrap_or_else(|| panic!("missing {side:?} {direction:?} for {:?}", engine.turn()))
}

fn assert_replays(engine: &Engine, record: &GameRecord) {
    assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, *engine);
    assert_eq!(
        Engine::from_history(engine.history().clone(), engine.turn()).unwrap(),
        *engine
    );
}

fn assert_arrivals(engine: &Engine, mv: Move, color: Color) {
    let t = mv.direction.step(mv.t).unwrap();
    let y = if color == Color::White { 0 } else { 7 };
    let (king_x, rook_x, rook_wid) = if mv.castling() == Some(Castling::Short) {
        (6, 5, if color == Color::White { 7 } else { 23 })
    } else {
        (2, 3, if color == Color::White { 0 } else { 16 })
    };
    let king = engine.board(t).unwrap().get_piece(king_x, y).unwrap();
    let rook = engine.board(t).unwrap().get_piece(rook_x, y).unwrap();
    assert_eq!(king.origin, mv.pid.wid);
    assert_eq!(rook.origin, rook_wid);
    assert_eq!(king.kind, PieceKind::K { moved: true });
    assert_eq!(rook.kind, PieceKind::R { moved: true });
    for piece in [king, rook] {
        assert_eq!(piece.inverted, mv.to_past());
        assert_eq!(piece.loop_turn, usize::from(mv.to_past()));
        assert_eq!(piece.fut_seen, mv.t.max(t));
    }
    if mv.to_past() {
        for wid in [king.origin, rook.origin] {
            assert!(
                engine
                    .board(mv.t + 1)
                    .unwrap()
                    .find_by_origin(wid)
                    .is_empty(),
                "departing participant duplicated in the future"
            );
        }
    }
}

#[test]
fn standard_castles_in_both_directions_and_journal_undo_restores_both_pieces() {
    for color in [Color::White, Color::Black] {
        for side in [Castling::Short, Castling::Long] {
            for direction in [TimeDirection::Future, TimeDirection::Past] {
                for rules in [
                    GameRules::default(),
                    GameRules {
                        chain_info_activation: false,
                        global_reaction_reset: false,
                        ..Default::default()
                    },
                ] {
                    let (mut engine, mut record) = prepared(color, side, rules);
                    let before = engine.clone();
                    let mv = castle(&engine, side, direction);
                    record
                        .apply(
                            &mut engine,
                            Command::Move {
                                mv,
                                from: Sq(4, if color == Color::White { 0 } else { 7 }),
                            },
                        )
                        .unwrap();
                    assert!(
                        engine.history().moves().contains_key(&mv.id()),
                        "castling cancelled itself"
                    );
                    assert_arrivals(&engine, mv, color);
                    assert_replays(&engine, &record);
                    let (prefix, _) = record.without_last_move().unwrap();
                    assert_eq!(GameRecord::load(&prefix.to_text()).unwrap().0, before);
                }
            }
        }
    }
}

#[test]
fn standard_inverted_castling_with_each_offered_wait_replays() {
    for color in [Color::White, Color::Black] {
        for side in [Castling::Short, Castling::Long] {
            let (engine, record) = prepared(color, side, GameRules::default());
            let mv = castle(&engine, side, TimeDirection::Past);
            let waits = engine.possible_move_waits(mv).unwrap();
            assert!(!waits.is_empty());
            for until in waits {
                let mut next = engine.clone();
                let mut journal = record.clone();
                journal
                    .apply(
                        &mut next,
                        Command::MoveAndWait {
                            mv,
                            from: Sq(4, if color == Color::White { 0 } else { 7 }),
                            until,
                        },
                    )
                    .unwrap();
                assert_replays(&next, &journal);
            }
        }
    }
}

#[test]
fn all_highlighted_king_moves_from_standard_castling_positions_are_accepted() {
    for color in [Color::White, Color::Black] {
        for side in [Castling::Short, Castling::Long] {
            let (engine, record) = prepared(color, side, GameRules::default());
            let y = if color == Color::White { 0 } else { 7 };
            let king = *engine
                .board(engine.history().horizon())
                .unwrap()
                .get_piece(4, y)
                .unwrap();
            for mv in engine.possible_moves(king.origin, king.age).unwrap() {
                let mut next = engine.clone();
                let mut journal = record.clone();
                journal
                    .apply(&mut next, Command::Move { mv, from: king.sq })
                    .unwrap_or_else(|error| panic!("{mv:?}: {error}"));
                assert_replays(&next, &journal);
            }
        }
    }
}
