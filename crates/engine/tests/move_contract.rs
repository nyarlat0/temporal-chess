use temporal_chess_engine::{
    engine::Engine,
    model::{GameRules, PieceAt, PieceId, Sq},
    record::{Command, GameRecord},
};

/// Every highlighted move is submitted independently against exactly the
/// position that offered it, through the same journal API as the client/server.
fn accepted_moves(engine: &Engine, record: &GameRecord) -> Vec<(Command, Engine, GameRecord)> {
    let mut accepted = Vec::new();
    for piece in engine.move_sources() {
        for mv in engine.possible_moves(piece.origin, piece.age).unwrap() {
            let command = Command::Move { mv, from: piece.sq };
            let mut next = engine.clone();
            let mut journal = record.clone();
            assert_eq!(
                journal.apply(&mut next, command),
                Ok(()),
                "offered command: {command}\n{}",
                record.to_text()
            );
            accepted.push((command, next, journal));
        }
    }
    accepted
}

#[test]
fn offered_moves_are_accepted_through_temporal_play_and_replay() {
    let mut checked = 0;
    for rules in [
        GameRules::default(),
        GameRules {
            chain_info_activation: true,
            ..Default::default()
        },
        GameRules {
            global_reaction_reset: true,
            ..Default::default()
        },
        GameRules {
            wait_before_move: false,
            chain_info_activation: true,
            global_reaction_reset: true,
        },
    ] {
        for seed in 0..8_u64 {
            let mut rng = seed + 1;
            let mut engine = Engine::new(rules).with_interactive_calculation().unwrap();
            let mut record = GameRecord::new(rules);
            for _ in 0..8 {
                let mut accepted = accepted_moves(&engine, &record);
                checked += accepted.len();
                if accepted.is_empty() {
                    break;
                }
                rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
                let index = (rng >> 32) as usize % accepted.len();
                let (_, next, journal) = accepted.swap_remove(index);
                engine = next;
                record = journal;
                if engine.outcome().is_some() {
                    break;
                }
            }
            assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, engine);
        }
    }
    assert!(
        checked > 1_000,
        "coverage unexpectedly fell to {checked} moves"
    );
}

#[test]
fn king_can_invert_into_a_pawns_origin_and_replay_the_result() {
    for chain in ["off", "on"] {
        let (engine, record) = GameRecord::load(&format!("NOLAN-CHESS 3\nchain_info_activation {chain}\nglobal_reaction_reset off\nmove 0 12:0:0 e2 e3 + normal\nmove 1 25:1:0 b7 b6 + normal\n")).unwrap();
        let (_, result, record) = accepted_moves(&engine, &record).into_iter().find(|(command, _, _)| {
            matches!(command, Command::Move { mv, from } if *from == Sq(4, 0) && mv.to_past() && mv.sq == Sq(4, 1))
        }).expect("e1,t=2 -> e2,t=1 must remain available");
        assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, result);
    }
}

#[test]
fn queen_reaction_can_cancel_castling_through_a_rook_collision() {
    let (mut engine, mut record) =
        GameRecord::load(include_str!("fixtures/castling_rook_collision.nolan")).unwrap();
    let queen = engine
        .move_sources()
        .find(|piece| piece.origin == 3 && piece.t == 14)
        .unwrap();
    let mv = engine
        .possible_moves(queen.origin, queen.age)
        .unwrap()
        .into_iter()
        .find(|mv| !mv.to_past() && mv.sq == Sq(1, 2))
        .expect("c4,14 -> b3,15 remains offered");
    let castling = *engine
        .history()
        .moves()
        .values()
        .find(|mv| mv.castling().is_some())
        .unwrap();
    record
        .apply(&mut engine, Command::Move { mv, from: queen.sq })
        .unwrap();
    assert!(!engine.history().moves().contains_key(&castling.id()));
    for (wid, square) in [(4, Sq(4, 0)), (7, Sq(7, 0))] {
        let state = engine.history().worldlines()[wid]
            .iter()
            .find(|s| s.t == 19)
            .unwrap();
        assert_eq!(Sq(state.x, state.y), square);
    }
    // Cancellation changes the reconstructed history, never the replay log.
    assert!(
        record
            .commands()
            .iter()
            .any(|command| { matches!(command, Command::Move { mv, .. } if *mv == castling) })
    );
    assert!(
        matches!(record.commands().last(), Some(Command::Move { mv: last, .. }) if *last == mv)
    );
    assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, engine);
}

#[test]
fn bishop_can_replace_a_dead_queens_move_with_a_backward_recapture() {
    let text = format!(
        "{}advance 14 3:14:0 c4 16\nmove 16 3:16:0 c4 b3 + normal\nmove 17 19:19:1 c5 c4 - normal\n",
        include_str!("fixtures/castling_rook_collision.nolan")
    );
    let (mut engine, mut record) = GameRecord::load(&text).unwrap();
    let bishop = engine
        .move_sources()
        .find(|p| p.origin == 5 && p.t == 16)
        .unwrap();
    let mv = engine
        .possible_moves(bishop.origin, bishop.age)
        .unwrap()
        .into_iter()
        .find(|mv| mv.to_past() && mv.sq == Sq(2, 3))
        .expect("backward recapture stays offered");
    let old = engine.history().moves()[&mv.id()];
    assert_eq!(old.pid.wid, 3);
    assert!(!engine.piece_state(old.source()).unwrap().alive);
    let victim = engine.move_capture_target(mv).unwrap();
    assert_eq!(victim.origin, 19);
    let victim = PieceAt {
        piece: PieceId::from_piv(victim),
        t: victim.t,
    };
    record
        .apply(
            &mut engine,
            Command::Move {
                mv,
                from: bishop.sq,
            },
        )
        .unwrap();
    assert_ne!(engine.history().moves().get(&old.id()), Some(&old));
    assert!(!engine.piece_state(victim).unwrap().alive);
    assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, engine);
}

#[test]
fn impossible_future_knight_move_is_cancelled_before_declaring_paradox() {
    let text = include_str!("fixtures/knight_capture_paradox.nolan");
    let (prefix, last) = text.trim_end().rsplit_once('\n').unwrap();
    let (mut engine, mut record) = GameRecord::load(prefix).unwrap();
    let invalidated = *engine
        .history()
        .moves()
        .values()
        .find(|mv| mv.pid.wid == 22 && mv.t == 37)
        .unwrap();
    let command = last.parse().unwrap();
    record.apply(&mut engine, command).unwrap();
    assert!(engine.outcome().is_none(), "{:?}", engine.outcome());
    let Command::Move { mv, .. } = command else {
        unreachable!()
    };
    assert_eq!(engine.history().moves().get(&mv.id()), Some(&mv));
    assert_eq!(
        engine.board(32).unwrap().get_piece(4, 3).unwrap().origin,
        22
    );
    assert!(
        engine.history().worldlines()[7]
            .iter()
            .any(|s| s.t == 32 && !s.alive)
    );
    assert!(!engine.history().moves().contains_key(&invalidated.id()));
    let knight = engine.piece_state(invalidated.source()).unwrap();
    assert!(knight.alive && knight.rights.ordinary);
    assert_eq!(Sq(knight.x, knight.y), Sq(4, 3));
    assert_eq!(record.commands().last(), Some(&command));
    assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, engine);
}

#[test]
fn checked_side_can_use_rook_information_reaction_to_capture_knight() {
    use temporal_chess_engine::model::{Color, PieceKind};
    let text = include_str!("fixtures/check_rook_reaction.nolan");
    // The original journal contains d1-c1 onto the passively continued white
    // bishop. Frontier occupancy now correctly rejects that old command.
    assert_eq!(GameRecord::load(text).unwrap_err().line, 25);
    let corrected = text.replace(
        "move 16 3:16:0 d1 c1 + normal",
        "move 16 3:16:0 d1 e2 + normal",
    );
    let corrected = corrected.replace(
        "move 5 16:23:0 a8 a6 - normal",
        "move 5 24:23:0 a7 a6 - normal",
    );
    let (engine, record) = GameRecord::load(&corrected).unwrap();
    assert!(engine.checks(Color::White).unwrap().is_check());
    let captures: Vec<_> = engine
        .move_sources()
        .filter(|p| matches!(p.kind, PieceKind::R { .. }) && p.rights.new_information)
        .flat_map(|p| {
            engine
                .possible_moves(p.origin, p.age)
                .unwrap()
                .into_iter()
                .map(move |mv| (p, mv))
        })
        .filter(|(_, mv)| {
            engine
                .move_capture_target(*mv)
                .is_some_and(|p| p.kind == PieceKind::N)
        })
        .collect();
    assert!(
        !captures.is_empty(),
        "an information-active rook must be able to capture the knight"
    );
    for (p, mv) in captures {
        let mut next = engine.clone();
        let mut journal = record.clone();
        journal
            .apply(&mut next, Command::Move { mv, from: p.sq })
            .unwrap();
        assert_eq!(GameRecord::load(&journal.to_text()).unwrap().0, next);
        for until in engine.possible_move_waits(mv).unwrap() {
            let mut next = engine.clone();
            let mut journal = record.clone();
            journal
                .apply(
                    &mut next,
                    Command::MoveAndWait {
                        mv,
                        from: p.sq,
                        until,
                    },
                )
                .unwrap();
            assert_eq!(GameRecord::load(&journal.to_text()).unwrap().0, next);
        }
    }
}

#[test]
fn every_offered_wait_is_an_atomic_playable_command_in_time_war() {
    use temporal_chess_engine::model::StartingPosition;
    for wait_before_move in [false, true] {
        let rules = GameRules {
            wait_before_move,
            ..Default::default()
        };
        let start = StartingPosition::TimeWar { separation: 5 };
        let mut engine = Engine::with_start(rules, start)
            .unwrap()
            .with_interactive_calculation()
            .unwrap();
        let mut record = GameRecord::with_start(rules, start);
        for _ in 0..4 {
            let mut continuation = None;
            let mut count = 0;
            for piece in engine.move_sources() {
                for mv in engine.possible_moves(piece.origin, piece.age).unwrap() {
                    for until in engine.possible_move_waits(mv).unwrap_or_default() {
                        let mut next = engine.clone();
                        let mut journal = record.clone();
                        let command = Command::MoveAndWait {
                            mv,
                            from: piece.sq,
                            until,
                        };
                        journal
                            .apply(&mut next, command)
                            .unwrap_or_else(|e| panic!("{command}: {e}\n{}", record.to_text()));
                        assert_eq!(journal.commands().len(), record.commands().len() + 1);
                        assert_eq!(GameRecord::load(&journal.to_text()).unwrap().0, next);
                        if next.outcome().is_none() {
                            continuation = Some((next, journal));
                        }
                        count += 1;
                    }
                }
            }
            assert!(count > 0);
            let Some((next, journal)) = continuation else {
                break;
            };
            engine = next;
            record = journal;
        }
    }
}
