use temporal_chess_engine::{
    engine::Engine,
    model::{Color, PieceKind, Sq, State},
};

#[test]
fn standard_game_preview_includes_rook_continued_by_knights_turn() {
    // Minimal c3-b1 regression: neither endpoint has an explicit t=5 state.
    // The same turn must continue the rook, even in the ordinary game mode.
    let mut rook = State::new(PieceKind::R { moved: true }, Color::White, Sq(1, 0));
    rook.t = 4;
    rook.fut_seen = 4;
    let mut knight = rook.clone();
    knight.kind = PieceKind::N;
    knight.x = 2;
    knight.y = 2;
    let mut anchor = vec![State::new(PieceKind::N, Color::Black, Sq(7, 7))];
    for _ in 0..6 {
        anchor.push(anchor.last().unwrap().prolongate().unwrap());
    }
    let mut engine =
        Engine::from_worldlines(vec![vec![rook], vec![knight], anchor], Color::White).unwrap();
    assert!(engine.board(5).unwrap().get_piece(1, 0).is_none());
    let moves = engine.possible_moves(1, 0).unwrap();
    assert!(!moves.iter().any(|mv| !mv.to_past() && mv.sq == Sq(1, 0)));
    let valid = moves.into_iter().find(|mv| !mv.to_past()).unwrap();
    engine.apply_new_mv(valid).unwrap();
    assert_eq!(engine.board(5).unwrap().get_piece(1, 0).unwrap().origin, 0);
    assert_eq!(
        Engine::from_history(engine.history().clone(), engine.turn()).unwrap(),
        engine
    );
}
