use temporal_chess_engine::record::GameRecord;

#[test]
fn black_king_survives_inverted_capture_and_boundary_return() {
    let text = "NOLAN-CHESS 4
chain_info_activation off
global_reaction_reset off
start standard
move 0 6:0:0 g1 f3 + normal
move 1 31:1:0 h7 h6 + normal
move 2 15:2:0 h2 h3 + normal
move 3 17:3:0 b8 c6 + normal
move 4 6:4:0 f3 e5 - normal
move 5 29:5:0 f7 f6 + normal
move 2 6:4:1 e5 f7 - normal
move 1 20:1:0 e8 f7 - normal
";
    for text in [
        text.to_owned(),
        text.replace("NOLAN-CHESS 4", "NOLAN-CHESS 5")
            .replace("start standard", "wait_before_move off\nstart standard"),
    ] {
        let (engine, record) = GameRecord::load(&text).unwrap();
        for t in [0, 1] {
            let king = engine.board(t).unwrap().get_piece(5, 6).unwrap();
            assert_eq!(king.origin, 20, "Black's king wins at t={t}");
            assert_eq!(king.inverted, t == 0);
            assert!(
                !engine.history().worldlines()[6]
                    .iter()
                    .any(|s| s.t == t && s.x == 5 && s.y == 6 && s.alive)
            );
        }
        assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, engine);
        let (prefix, _) = record.without_last_move().unwrap();
        let (before, _) = GameRecord::load(&prefix.to_text()).unwrap();
        for t in [0, 1] {
            assert_eq!(before.board(t).unwrap().get_piece(5, 6).unwrap().origin, 6);
        }
    }
}
