use temporal_chess_engine::{
    engine::Engine,
    model::{Color, GameRules, PieceKind, Sq, StartingPosition, State},
    record::{Command, GameRecord},
};

fn play(engine: &mut Engine, record: &mut GameRecord, wid: usize, t: i64, sq: Sq, past: bool) {
    let p = engine
        .move_sources()
        .find(|p| p.origin == wid && p.t == t)
        .unwrap_or_else(|| {
            panic!(
                "active wid={wid} t={t} turn={:?} sources={:?}",
                engine.turn(),
                engine.move_sources().collect::<Vec<_>>()
            )
        });
    let mv = engine
        .possible_moves(wid, p.age)
        .unwrap()
        .into_iter()
        .find(|m| m.sq == sq && m.to_past() == past)
        .expect("offered move");
    record
        .apply(engine, Command::Move { mv, from: p.sq })
        .unwrap();
    assert_eq!(
        Engine::from_history(engine.history().clone(), engine.turn()).unwrap(),
        *engine
    );
    assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, *engine);
}

#[test]
fn standard_white_inversion_creates_negative_boards_and_prepends_passive_roots() {
    let mut e = Engine::new(GameRules::default())
        .with_interactive_calculation()
        .unwrap();
    let mut r = GameRecord::default();
    play(&mut e, &mut r, 6, 0, Sq(5, 2), true);
    assert_eq!(e.history().beginning(), -2);
    for t in [-2, -1] {
        assert!(e.preboard(t).is_some());
    }
    let knight = e.history().worldlines()[6].last().unwrap();
    assert_eq!(
        (knight.t, knight.inverted, Sq(knight.x, knight.y)),
        (-2, true, Sq(5, 2))
    );
    let rook = &e.history().worldlines()[0];
    assert_eq!(rook[0].t, -2);
    assert_eq!(rook.iter().position(|s| s.t == 0), Some(2));
    assert!(rook.iter().all(|s| Sq(s.x, s.y) == Sq(0, 0)));
    assert!(rook.iter().filter(|s| s.t < 0).all(|s| s.fut_seen == s.t));
    assert_eq!(
        knight.fut_seen, 0,
        "the inverted traveller retains future knowledge"
    );
    play(&mut e, &mut r, 17, 1, Sq(2, 5), false);
    play(&mut e, &mut r, 6, -2, Sq(7, 3), true);
    assert_eq!(e.history().beginning(), -4);
    assert!(r.to_text().contains("move -2"));
    assert_eq!(
        e.history().worldlines()[0].iter().position(|s| s.t == 0),
        Some(4)
    );
    assert!(
        e.history().worldlines()[0]
            .iter()
            .filter(|s| s.t < 0)
            .all(|s| s.fut_seen == s.t)
    );
}

#[test]
fn time_war_can_extend_above_its_original_end_without_changing_orientation() {
    let rules = GameRules::default();
    let start = StartingPosition::TimeWar { separation: 3 };
    let mut e = Engine::with_start(rules, start)
        .unwrap()
        .with_interactive_calculation()
        .unwrap();
    let mut r = GameRecord::with_start(rules, start);
    play(&mut e, &mut r, 6, 0, Sq(5, 2), false);
    play(&mut e, &mut r, 17, 3, Sq(2, 5), true);
    play(&mut e, &mut r, 1, 2, Sq(2, 2), false);
    assert_eq!(e.history().horizon(), 4);
    assert!(!e.history().worldlines()[1].last().unwrap().inverted);
    let black = &e.history().worldlines()[17];
    assert_eq!((black[0].t, black[0].inverted), (4, true));
    assert_eq!(black.iter().find(|s| s.t == 3).unwrap().fut_seen, 4);
}

#[test]
fn inverted_black_arrival_at_zero_continues_to_minus_one() {
    let mut n = State::new(PieceKind::N, Color::Black, Sq(1, 7));
    n.t = 1;
    n.fut_seen = 1;
    n.inverted = true;
    n.rights.ordinary = true;
    let mut e = Engine::from_worldlines(vec![vec![n]], Color::Black).unwrap();
    let mv = e
        .possible_moves(0, 0)
        .unwrap()
        .into_iter()
        .find(|m| m.to_past() && m.sq == Sq(2, 5))
        .unwrap();
    e.apply_new_mv(mv).unwrap();
    let end = e.history().worldlines()[0].last().unwrap();
    assert_eq!((end.t, end.inverted), (-1, true));
    assert_eq!(e.history().beginning(), -1);
    assert_eq!(
        Engine::from_history(e.history().clone(), e.turn()).unwrap(),
        e
    );
}

#[test]
fn waiting_and_calculation_do_not_expand_history_and_negative_parity_is_correct() {
    assert_eq!(Color::from_board(-1), Color::Black);
    assert_eq!(Color::from_board(-2), Color::White);
    let mut e = Engine::new(GameRules {
        wait_before_move: true,
        ..Default::default()
    });
    let source = e.move_sources().find(|p| p.origin == 6).unwrap();
    let at = temporal_chess_engine::model::PieceAt {
        piece: temporal_chess_engine::model::PieceId::from_piv(source),
        t: 0,
    };
    let before = e.clone();
    assert!(e.skip_ordinary(at, -2).is_err());
    assert_eq!(e, before);
    e.continue_calculation().unwrap();
    assert_eq!((e.history().beginning(), e.history().horizon()), (0, 0));
}
