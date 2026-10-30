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
fn skipping_only_arrives_on_own_color_in_both_orientations() {
    for color in [Color::White, Color::Black] {
        for inverted in [false, true] {
            let start = if color == Color::White { 4 } else { 5 };
            let mut engine = Engine::from_worldlines(vec![timeline(color, 7)], color).unwrap();
            if inverted {
                let state = engine.history.worldlines[0][start].clone();
                engine.history.worldlines[0].states.truncate(start + 1);
                let next = state.step(TimeDirection::Past).unwrap();
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
fn inversion_discards_future_by_default_and_preserves_it_only_in_gap_mode() {
    for gaps in [false, true] {
        let mut engine =
            Engine::from_worldlines(vec![timeline(Color::White, 8)], Color::White).unwrap();
        engine.history.causal_gaps = gaps;
        let mv = engine
            .possible_moves(0, 4)
            .unwrap()
            .into_iter()
            .find(|mv| mv.to_past() && mv.sq == Sq(2, 2))
            .unwrap();
        engine.apply_new_mv(mv).unwrap();
        let line = &engine.history.worldlines[0];
        assert_eq!(!line.gaps().is_empty(), gaps);
        assert_eq!(line.iter().any(|state| state.t > 4), gaps);
        assert!(line.iter().any(|state| state.t == 2 && state.inverted));
    }
    assert!(!Engine::default().history().causal_gaps());
}

#[test]
fn record_keeps_rules_and_legacy_records_opt_into_gaps() {
    use crate::record::GameRecord;
    for gaps in [false, true] {
        let record = GameRecord::new(gaps);
        let (engine, restored) = GameRecord::load(&record.to_text()).unwrap();
        assert_eq!(engine.history().causal_gaps(), gaps);
        assert_eq!(restored, record);
    }
    assert!(
        GameRecord::load("NOLAN-CHESS 1\n")
            .unwrap()
            .0
            .history()
            .causal_gaps()
    );
}
