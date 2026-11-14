use temporal_chess_engine::{
    engine::Engine,
    model::{GameRules, Sq},
    record::{Command, GameRecord},
};

#[test]
fn inverted_king_waits_replay_in_the_reported_castling_position() {
    let text = include_str!("fixtures/castling_missing_rook_revalidation.nolan");
    let (engine, record) = GameRecord::load(text).unwrap();
    accepted_moves(&engine, &record);
    let mut checked_waits = 0;
    for king in engine.move_sources().filter(|p| p.origin == 4) {
        for mv in engine
            .possible_moves(king.origin, king.age)
            .unwrap()
            .into_iter()
            .filter(|mv| mv.to_past() && mv.sq == Sq(7, 0))
        {
            let waits = engine.possible_move_waits(mv).unwrap();
            for until in waits {
                let command = Command::MoveAndWait {
                    mv,
                    from: king.sq,
                    until,
                };
                let mut branch = engine.clone();
                let mut journal = record.clone();
                journal
                    .apply(&mut branch, command)
                    .unwrap_or_else(|error| panic!("{command}: {error}"));
                assert_eq!(GameRecord::load(&journal.to_text()).unwrap().0, branch);
                checked_waits += 1;
            }
        }
    }
    assert!(
        checked_waits > 1,
        "must cover inversion and distant waiting"
    );
    assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, engine);
    assert_eq!(
        Engine::from_history(engine.history().clone(), engine.turn()).unwrap(),
        engine
    );
}

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
            global_reaction_reset: false,
            ..Default::default()
        },
        GameRules {
            wait_before_move: true,
            chain_info_activation: true,
            global_reaction_reset: false,
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

// The former long collision regressions contained spatial moves at loop exits.
// Their engine mechanisms now have minimal unit fixtures independent of that
// obsolete sequence (castling cancellation, dead-source replacement, cycles).
#[test]
fn archived_loop_exit_journals_report_the_first_obsolete_command() {
    for text in [
        include_str!("fixtures/castling_rook_collision.nolan"),
        include_str!("fixtures/knight_capture_paradox.nolan"),
    ] {
        let error = GameRecord::load(text).unwrap_err();
        // The return to t=2 automatically continues to t=4. The old explicit
        // advance from that exit is no longer an available command.
        assert_eq!(error.line, 8);
        let prefix = text.lines().take(7).collect::<Vec<_>>().join("\n");
        let (engine, record) = GameRecord::load(&prefix).unwrap();
        assert!(!accepted_moves(&engine, &record).is_empty());
        assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, engine);
    }
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
