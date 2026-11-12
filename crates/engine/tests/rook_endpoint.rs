use temporal_chess_engine::{
    engine::Engine,
    model::{Color, PieceKind, Sq, State},
};

#[test]
fn passive_rook_endpoint_survives_both_sides_subsequent_turns() {
    let at = |kind, color, sq, t| {
        let mut state = State::new(kind, color, sq);
        state.t = t;
        state.fut_seen = t;
        state.rights.ordinary = Color::from_board(t) == color;
        vec![state]
    };
    let mut engine = Engine::from_worldlines(
        vec![
            at(PieceKind::R { moved: true }, Color::White, Sq(5, 0), 26),
            at(PieceKind::N, Color::White, Sq(1, 0), 26),
            at(PieceKind::N, Color::Black, Sq(1, 7), 27),
        ],
        Color::White,
    )
    .unwrap();
    for (wid, t, square) in [(1, 26, Sq(2, 2)), (2, 27, Sq(2, 5))] {
        let source = engine
            .move_sources()
            .find(|p| p.origin == wid && p.t == t)
            .unwrap();
        let mv = engine
            .possible_moves(wid, source.age)
            .unwrap()
            .into_iter()
            .find(|m| !m.to_past() && m.sq == square)
            .unwrap();
        engine.apply_new_mv(mv).unwrap();
        let rook = engine
            .board(t + 1)
            .unwrap()
            .get_piece(5, 0)
            .expect("idle rook continues");
        assert_eq!(rook.origin, 0);
        assert!(!rook.inverted);
    }
    assert_eq!(
        Engine::from_history(engine.history().clone(), engine.turn()).unwrap(),
        engine
    );
}
