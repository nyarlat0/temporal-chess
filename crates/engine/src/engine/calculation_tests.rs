use super::*;
use crate::model::{Color, PieceKind, Sq};

fn line(color: Color, sq: Sq, end: usize) -> Vec<State> {
    let mut states = vec![State::new(PieceKind::N, color, sq)];
    for _ in 0..end {
        states.push(states.last().unwrap().prolongate().unwrap());
    }
    states
}

#[test]
fn calculation_advances_one_own_cancelled_board_without_spending_rights_or_turn() {
    let mut white = line(Color::White, Sq(1, 0), 6);
    white[2].rights.cancelled_move = true;
    white[4].rights.cancelled_move = true;
    let mut black = line(Color::Black, Sq(6, 7), 6);
    black[1].rights.cancelled_move = true;
    let mut engine = super::legacy_from_worldlines(vec![white, black], Color::White)
        .unwrap()
        .with_interactive_calculation()
        .unwrap();
    assert_eq!(
        engine.calculation_status(),
        CalculationStatus::Paused {
            t: 2,
            side: Color::White
        }
    );
    assert!(engine.board(2).is_some());
    assert!(engine.board(3).is_none());
    assert!(engine.preboard(6).is_some());
    assert_eq!(
        engine.timeline_times().collect::<Vec<_>>(),
        (0..=6).collect::<Vec<_>>()
    );
    let lines = engine.history.worldlines.clone();
    assert_eq!(
        Engine::from_history(engine.history.clone(), engine.turn()).unwrap(),
        engine
    );
    assert_eq!(
        engine.continue_calculation().unwrap(),
        CalculationStatus::Paused {
            t: 4,
            side: Color::White
        }
    );
    assert!(engine.board(4).is_some());
    assert!(engine.board(5).is_none());
    assert_eq!(
        engine.continue_calculation().unwrap(),
        CalculationStatus::Complete
    );
    assert!(engine.board(6).is_some());
    assert_eq!(engine.turn(), Color::White);
    assert_eq!(engine.history.worldlines, lines);
    let unchanged = engine.clone();
    assert_eq!(
        engine.continue_calculation().unwrap(),
        CalculationStatus::Complete
    );
    assert_eq!(engine, unchanged);
}

fn play(engine: &mut Engine, t: usize, from: Sq, to: Sq) -> Move {
    let piece = *engine.board(t).unwrap().get_piece(from.0, from.1).unwrap();
    let mv = engine
        .possible_moves(piece.origin, piece.age)
        .unwrap()
        .into_iter()
        .find(|mv| mv.direction == TimeDirection::Future && mv.sq == to)
        .unwrap();
    engine.apply_new_mv(mv).unwrap();
    mv
}

#[test]
fn later_invalid_capture_and_revival_are_deferred_until_user_advances() {
    let mut engine = Engine::new(true);
    let first = play(&mut engine, 0, Sq(4, 1), Sq(4, 3));
    play(&mut engine, 1, Sq(3, 6), Sq(3, 4));
    let capture = play(&mut engine, 2, Sq(4, 3), Sq(3, 4));
    engine.undo_mv(first.id()).unwrap();
    engine.turn = Color::White;
    engine = engine.with_interactive_calculation().unwrap();
    assert_eq!(
        engine.calculation_status(),
        CalculationStatus::Paused {
            t: 0,
            side: Color::White
        }
    );
    assert!(engine.history.moves.contains_key(&capture.id()));
    assert!(!engine.history.worldlines[27][3].alive);
    assert_eq!(
        engine.continue_calculation().unwrap(),
        CalculationStatus::Paused {
            t: 2,
            side: Color::White
        }
    );
    assert!(!engine.history.moves.contains_key(&capture.id()));
    assert!(!engine.history.worldlines[27][3].alive);
    assert_eq!(
        engine.continue_calculation().unwrap(),
        CalculationStatus::Complete
    );
    assert!(engine.history.worldlines[27][3].alive);
    assert_eq!(engine.turn(), Color::White);
}

#[test]
fn preboards_preserve_all_overlapping_live_pieces_and_reject_uncomputed_sources() {
    let mut engine = super::legacy_from_worldlines(
        vec![
            line(Color::White, Sq(1, 0), 4),
            line(Color::White, Sq(2, 0), 4),
        ],
        Color::White,
    )
    .unwrap();
    engine.history.worldlines[0][2].rights.cancelled_move = true;
    for state in &mut engine.history.worldlines[1][3..] {
        state.x = 1;
    }
    engine = engine.with_interactive_calculation().unwrap();
    assert_eq!(engine.preboard(3).unwrap().space[1][0].len(), 2);
    assert!(engine.preboard(3).unwrap().space[1][0].iter().all(|p| {
        engine
            .piece_state(PieceAt {
                piece: PieceId::from_piv(*p),
                t: 3,
            })
            .unwrap()
            .alive
    }));
    assert!(engine.board(3).is_none());
    assert_eq!(engine.possible_moves(0, 4), Err(EngineErr::AbsentBoard));
    let source = PieceAt {
        piece: PieceId::from_state(&engine.history.worldlines[0][4], 0),
        t: 4,
    };
    let before = engine.clone();
    assert_eq!(engine.skip_ordinary(source, 4), Err(EngineErr::AbsentBoard));
    assert_eq!(engine, before);
    // No fallback to t=2 when t=3 exists but is unresolved and conflicting.
    assert!(
        engine
            .possible_moves(0, 2)
            .unwrap()
            .iter()
            .all(|mv| mv.direction == TimeDirection::Past)
    );
}

#[test]
fn skip_preview_includes_intermediate_times_and_ignores_wrong_orientation() {
    let engine =
        super::legacy_from_worldlines(vec![line(Color::White, Sq(1, 0), 6)], Color::White).unwrap();
    let source = PieceAt {
        piece: PieceId::from_state(&engine.history.worldlines[0][2], 0),
        t: 2,
    };
    assert_eq!(engine.possible_skips(source), [3, 4, 5, 6]);
    let mut waiting = engine.clone();
    waiting.skip_ordinary(source, 4).unwrap();
    assert_eq!(waiting.turn(), Color::White);
    assert!(!waiting.history.worldlines[0][2].rights.ordinary);
    assert!(waiting.history.worldlines[0][4].rights.ordinary);
    assert_eq!(waiting.history.horizon, 6);
}
