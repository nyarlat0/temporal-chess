use temporal_chess_engine::{
    engine::Engine,
    model::{Color, Sq},
    record::GameRecord,
};
#[test]
fn rewritten_history_keeps_black_pawn_endpoint_ordinary_and_playable() {
    let (engine, record) = GameRecord::load(include_str!("fixtures/endpoint_pawn.nolan")).unwrap();
    let pawn = engine.history().worldlines()[28].last().unwrap();
    assert!(pawn.alive);
    assert_eq!((pawn.t, Sq(pawn.x, pawn.y)), (7, Sq(4, 4)));
    assert!(pawn.rights.ordinary);
    let p = engine.board(7).unwrap().get_piece(4, 4).unwrap();
    let moves = engine.possible_moves(p.origin, p.age).unwrap();
    assert!(!moves.is_empty());
    for mv in moves {
        engine.after_move(mv).unwrap();
    }
    for line in engine.history().worldlines() {
        let last = line.last().unwrap();
        if last.alive && Color::from_board(last.t) == last.color {
            assert!(last.rights.ordinary);
        }
    }
    assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, engine);
    assert_eq!(
        Engine::from_history(engine.history().clone(), engine.turn()).unwrap(),
        engine
    );
}
