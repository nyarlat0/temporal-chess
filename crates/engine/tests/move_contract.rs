use temporal_chess_engine::{
    engine::Engine,
    model::Sq,
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
    for gaps in [false, true] {
        for seed in 0..8_u64 {
            let mut rng = seed + 1;
            let mut engine = Engine::new(gaps).with_interactive_calculation().unwrap();
            let mut record = GameRecord::new(gaps);
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
    for gaps in ["off", "on"] {
        let (engine, record) = GameRecord::load(&format!("NOLAN-CHESS 2\ncausal_gaps {gaps}\nmove 0 12:0:0 e2 e3 + normal\nmove 1 25:1:0 b7 b6 + normal\n")).unwrap();
        let (_, result, record) = accepted_moves(&engine, &record).into_iter().find(|(command, _, _)| {
            matches!(command, Command::Move { mv, from } if *from == Sq(4, 0) && mv.to_past() && mv.sq == Sq(4, 1))
        }).expect("e1,t=2 -> e2,t=1 must remain available");
        assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, result);
    }
}
