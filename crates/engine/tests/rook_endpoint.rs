use temporal_chess_engine::record::GameRecord;

const SUFFIX: &str = "advance 14 3:14:0 c4 16
move 16 3:16:0 c4 b3 + normal
move 17 19:19:1 c5 c4 - normal
move 16 5:16:0 d3 c4 - normal
move 20 12:20:0 e4 d5 + normal
move 21 22:21:0 g8 f6 + normal
move 22 9:22:0 c3 c4 + normal
move 23 16:23:0 a8 a4 + normal
move 14 5:16:1 c4 b3 + normal
advance 16 5:16:2 b3 24
move 24 5:24:0 b3 a4 + normal
move 25 22:25:0 f6 e4 - normal
move 26 7:26:0 f1 e1 - normal
move 23 22:25:1 e4 c5 - normal
move 24 7:26:1 e1 e6 + normal
move 25 29:25:0 f7 e6 + normal
move 26 12:26:0 d5 e6 + normal
continue
";

#[test]
fn replacing_a_rook_loop_preserves_its_passive_future() {
    let text = format!(
        "{}{}",
        include_str!("fixtures/castling_rook_collision.nolan"),
        SUFFIX
    );
    let (mut engine, mut record) = GameRecord::load(&text).unwrap();
    for t in [26, 27] {
        let rook = engine
            .board(t)
            .unwrap()
            .get_piece(5, 0)
            .expect("rook stays on f1");
        assert_eq!(rook.origin, 7);
        assert!(!rook.inverted);
    }
    assert!(
        !engine
            .history()
            .moves()
            .values()
            .any(|mv| mv.pid.wid == 7 && mv.t == 26)
    );
    record
        .apply(
            &mut engine,
            "move 27 31:27:0 h7 h6 + normal".parse().unwrap(),
        )
        .unwrap();
    let rook = engine
        .board(28)
        .unwrap()
        .get_piece(5, 0)
        .expect("rook continues after Black's reply");
    assert_eq!(rook.origin, 7);
    assert!(rook.rights.ordinary);
    assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, engine);
}
