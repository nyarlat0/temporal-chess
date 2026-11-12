use super::*;
use crate::model::{Color, PieceKind, Sq};

fn timeline(color: Color, end: usize) -> Vec<State> {
    let mut line = vec![State::new(PieceKind::N, color, Sq(1, 0))];
    for _ in 0..end {
        line.push(line.last().unwrap().prolongate().unwrap());
    }
    line
}

#[test]
fn inverted_continuation_stops_on_first_own_board() {
    for color in [Color::White, Color::Black] {
        let boundary = color.first_board();
        let mut source = State::new(PieceKind::N, color, Sq(1, 0));
        source.t = boundary + 2;
        source.fut_seen = source.t;
        let arriving = source.step(TimeDirection::Past).unwrap();
        let endpoint = arriving.prolongate().unwrap();
        assert_eq!(endpoint.t, boundary);
        assert!(endpoint.rights.ordinary);
        assert!(endpoint.prolongate().is_none());
        assert_eq!(
            endpoint.step(TimeDirection::Past).map(|s| s.t),
            if color == Color::Black { Some(0) } else { None }
        );
        assert_eq!(
            endpoint.step(TimeDirection::Future).unwrap().t,
            boundary + 1
        );
    }
}

#[test]
fn skipping_only_arrives_on_own_color_in_both_orientations() {
    for color in [Color::White, Color::Black] {
        for inverted in [false, true] {
            let start = if color == Color::White { 4 } else { 5 };
            let mut engine = Engine::from_worldlines(vec![timeline(color, 7)], color).unwrap();
            if inverted {
                let state = engine.history.worldlines[0][start].clone();
                engine.history.worldlines[0].states.truncate(start + 1);
                let mut next = state.step(TimeDirection::Past).unwrap();
                next.x = 2;
                next.y = 2;
                engine.history.worldlines[0].states.push(next.clone());
                engine.history.worldlines[0]
                    .states
                    .push(next.prolongate().unwrap());
                engine.update_bufs().unwrap();
                engine.build_boards(7).unwrap();
            }
            let age = if inverted { start + 2 } else { start };
            let piece = engine.get_piv(0, age).unwrap();
            let source = Engine::piece_at(piece);
            let wrong = if inverted { piece.t - 1 } else { piece.t + 1 };
            let right = if inverted { piece.t - 2 } else { piece.t + 2 };
            let before = engine.clone();
            assert_eq!(
                engine.skip_ordinary(source, wrong),
                Err(EngineErr::WrongMove)
            );
            assert_eq!(engine, before);
            assert!(!engine.possible_skips(source).contains(&wrong));
            assert!(engine.possible_skips(source).contains(&right));
        }
    }
}

#[test]
fn inversion_discards_future_in_every_reaction_mode() {
    for chain_info_activation in [false, true] {
        for global_reaction_reset in [false, true] {
            let mut engine =
                Engine::from_worldlines(vec![timeline(Color::White, 8)], Color::White).unwrap();
            engine.history.rules = crate::model::GameRules {
                wait_before_move: false,
                chain_info_activation,
                global_reaction_reset,
            };
            let mv = engine
                .possible_moves(0, 4)
                .unwrap()
                .into_iter()
                .find(|mv| mv.to_past() && mv.sq == Sq(2, 2))
                .unwrap();
            engine.apply_new_mv(mv).unwrap();
            let line = &engine.history.worldlines[0];
            assert!(line.gaps().is_empty());
            assert!(!line.iter().any(|state| state.t > 4));
            assert!(line.iter().any(|state| state.t == 2 && state.inverted));
        }
    }
}

#[test]
fn records_keep_independent_rules_and_reject_removed_gap_mode() {
    use crate::record::GameRecord;
    for chain_info_activation in [false, true] {
        for global_reaction_reset in [false, true] {
            let rules = crate::model::GameRules {
                wait_before_move: false,
                chain_info_activation,
                global_reaction_reset,
            };
            let record = GameRecord::new(rules);
            let (engine, restored) = GameRecord::load(&record.to_text()).unwrap();
            assert_eq!(engine.history().rules(), rules);
            assert_eq!(restored, record);
        }
    }
    assert_eq!(Engine::default().history().rules(), Default::default());
    assert!(GameRecord::load("NOLAN-CHESS 2\ncausal_gaps on\n").is_err());
    assert_eq!(
        GameRecord::load("NOLAN-CHESS 2\ncausal_gaps off\n")
            .unwrap()
            .1
            .rules(),
        crate::model::GameRules {
            wait_before_move: true,
            global_reaction_reset: false,
            ..Default::default()
        }
    );
}
