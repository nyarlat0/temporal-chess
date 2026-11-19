use temporal_chess_engine::{
    engine::{Engine, LabEdit},
    model::{Castling, Color, GameRules, PieceKind, Sq, TimeDirection},
    record::{Command, GameRecord},
};

fn lab() -> Engine {
    let mut engine = Engine::new_lab(GameRules::default())
        .unwrap()
        .with_interactive_calculation()
        .unwrap();
    engine
        .edit_lab(LabEdit::Boards {
            beginning: -4,
            horizon: 6,
        })
        .unwrap();
    engine
}
fn place(engine: &mut Engine, kind: PieceKind, color: Color, t: i64, sq: Sq, inverted: bool) {
    engine
        .edit_lab(LabEdit::Place {
            t,
            square: sq,
            kind,
            color,
            inverted,
        })
        .unwrap();
}
fn play(
    engine: &mut Engine,
    record: &mut GameRecord,
    wid: usize,
    t: i64,
    to: Sq,
    direction: TimeDirection,
) {
    let piece = engine
        .move_sources()
        .find(|p| p.origin == wid && p.t == t)
        .unwrap();
    let mv = engine
        .possible_moves(wid, piece.age)
        .unwrap()
        .into_iter()
        .find(|mv| mv.sq == to && mv.direction == direction)
        .unwrap();
    record
        .apply(engine, Command::Move { mv, from: piece.sq })
        .unwrap();
}

#[test]
fn lab_places_independent_lines_on_negative_and_positive_boards_and_sets_moved() {
    for color in [Color::White, Color::Black] {
        for inverted in [false, true] {
            let home = if color == Color::White { 0 } else { 7 };
            let pawn_rank = if color == Color::White { 1 } else { 6 };
            let t = -2 + color.first_board();
            for (kind, square, moved) in [
                (PieceKind::K { moved: true }, Sq(4, home), false),
                (PieceKind::K { moved: false }, Sq(3, home), true),
                (PieceKind::R { moved: true }, Sq(0, home), false),
                (PieceKind::R { moved: false }, Sq(7, home), false),
                (PieceKind::R { moved: false }, Sq(4, 4), true),
                (PieceKind::P { moved: true }, Sq(3, pawn_rank), false),
                (PieceKind::P { moved: false }, Sq(3, 3), true),
            ] {
                let mut e = lab();
                place(&mut e, kind, color, t, square, inverted);
                let line = &e.history().worldlines()[0];
                assert_eq!(line.len(), 1);
                assert_eq!(line[0].kind, kind.set_moved(moved));
                assert_eq!(line[0].fut_seen, t);
                assert_eq!(line[0].inverted, inverted);
                assert!(line[0].rights.ordinary);
                assert_eq!(
                    GameRecord::load(&GameRecord::from_lab(&e).to_text())
                        .unwrap()
                        .0,
                    e
                );
            }
        }
    }
    let mut e = lab();
    place(&mut e, PieceKind::N, Color::White, -2, Sq(1, 0), false);
    place(&mut e, PieceKind::N, Color::White, 2, Sq(1, 0), true);
    assert_eq!(e.history().worldlines().len(), 2);
    assert_eq!(e.history().beginning(), -4);
    assert_eq!(e.history().horizon(), 6);
}

#[test]
fn opposite_parity_placement_gets_its_passive_tick_and_failed_edits_are_atomic() {
    let mut e = lab();
    place(&mut e, PieceKind::N, Color::Black, 0, Sq(1, 7), true);
    assert_eq!(
        e.history().worldlines()[0]
            .iter()
            .map(|s| s.t)
            .collect::<Vec<_>>(),
        [0, -1]
    );
    assert!(e.history().worldlines()[0][1].rights.ordinary);
    let before = e.clone();
    assert!(
        e.edit_lab(LabEdit::Place {
            t: 0,
            square: Sq(1, 7),
            kind: PieceKind::Q,
            color: Color::White,
            inverted: false
        })
        .is_err()
    );
    assert!(
        e.edit_lab(LabEdit::Boards {
            beginning: 0,
            horizon: 0
        })
        .is_err()
    );
    assert_eq!(e, before);
}

#[test]
fn prepared_moves_and_loops_are_in_the_setting_then_gameplay_replays_and_undoes() {
    let mut e = lab();
    place(&mut e, PieceKind::N, Color::White, 0, Sq(1, 0), false);
    place(&mut e, PieceKind::N, Color::Black, 1, Sq(1, 7), false);
    let mut preparation = GameRecord::from_lab(&e);
    play(
        &mut e,
        &mut preparation,
        0,
        0,
        Sq(2, 2),
        TimeDirection::Future,
    );
    play(
        &mut e,
        &mut preparation,
        1,
        1,
        Sq(2, 5),
        TimeDirection::Past,
    );
    let initial = e.clone();
    let mut game = GameRecord::from_lab(&e);
    assert!(game.commands().is_empty());
    assert_eq!(e.history().moves().len(), 2);
    assert!(game.to_text().starts_with("NOLAN-CHESS 7\n"));
    assert_eq!(GameRecord::load(&game.to_text()).unwrap().0, e);
    play(&mut e, &mut game, 0, 2, Sq(3, 4), TimeDirection::Future);
    assert_eq!(game.commands().len(), 1);
    assert_eq!(GameRecord::load(&game.to_text()).unwrap().0, e);
    let (prefix, _) = game.without_last_move().unwrap();
    assert_eq!(GameRecord::load(&prefix.to_text()).unwrap().0, initial);
}

#[test]
fn removing_a_line_remaps_prepared_commands_and_serialized_targets() {
    let mut e = lab();
    place(&mut e, PieceKind::N, Color::Black, 1, Sq(1, 7), false);
    place(&mut e, PieceKind::N, Color::White, 0, Sq(1, 0), false);
    let mut record = GameRecord::from_lab(&e);
    play(&mut e, &mut record, 1, 0, Sq(2, 2), TimeDirection::Future);
    e.edit_lab(LabEdit::Remove { wid: 0 }).unwrap();
    assert!(e.history().moves().values().all(|mv| mv.pid.wid == 0));
    assert_eq!(e.history().worldlines().len(), 1);
    assert_eq!(
        GameRecord::load(&GameRecord::from_lab(&e).to_text())
            .unwrap()
            .0,
        e
    );
}

#[test]
fn castling_requires_matching_orientation_in_both_temporal_directions() {
    for color in [Color::White, Color::Black] {
        for side in [Castling::Long, Castling::Short] {
            for king_inv in [false, true] {
                for rook_inv in [false, true] {
                    let mut e = lab();
                    let t = color.first_board();
                    let y = if color == Color::White { 0 } else { 7 };
                    place(
                        &mut e,
                        PieceKind::K { moved: false },
                        color,
                        t,
                        Sq(4, y),
                        king_inv,
                    );
                    place(
                        &mut e,
                        PieceKind::R { moved: false },
                        color,
                        t,
                        Sq(if side == Castling::Short { 7 } else { 0 }, y),
                        rook_inv,
                    );
                    e.edit_lab(LabEdit::Turn(color)).unwrap();
                    for dir in [TimeDirection::Future, TimeDirection::Past] {
                        let moves = e.possible_moves(0, 0).unwrap();
                        assert_eq!(
                            moves
                                .iter()
                                .any(|mv| mv.castling() == Some(side) && mv.direction == dir),
                            king_inv == rook_inv
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn lab_snapshot_preserves_setup_capture_targets_and_en_passant() {
    let mut e = lab();
    place(
        &mut e,
        PieceKind::R { moved: false },
        Color::White,
        0,
        Sq(0, 0),
        false,
    );
    place(&mut e, PieceKind::N, Color::Black, 1, Sq(0, 2), false);
    let mut preparation = GameRecord::from_lab(&e);
    play(
        &mut e,
        &mut preparation,
        0,
        0,
        Sq(0, 2),
        TimeDirection::Future,
    );
    assert!(!e.history().worldlines()[1].last().unwrap().alive);
    assert_eq!(
        GameRecord::load(&GameRecord::from_lab(&e).to_text())
            .unwrap()
            .0,
        e
    );

    let mut e = lab();
    place(
        &mut e,
        PieceKind::P { moved: false },
        Color::White,
        4,
        Sq(4, 4),
        false,
    );
    place(
        &mut e,
        PieceKind::P { moved: false },
        Color::Black,
        3,
        Sq(3, 6),
        false,
    );
    e.edit_lab(LabEdit::Turn(Color::Black)).unwrap();
    let mut preparation = GameRecord::from_lab(&e);
    play(
        &mut e,
        &mut preparation,
        1,
        3,
        Sq(3, 4),
        TimeDirection::Future,
    );
    let mut game = GameRecord::from_lab(&e);
    let ep = e
        .possible_moves(0, 0)
        .unwrap()
        .into_iter()
        .find(|mv| mv.en_passant().is_some() && !mv.to_past())
        .unwrap();
    game.apply(
        &mut e,
        Command::Move {
            mv: ep,
            from: Sq(4, 4),
        },
    )
    .unwrap();
    assert_eq!(GameRecord::load(&game.to_text()).unwrap().0, e);
}

#[test]
fn lab_has_no_chess_set_piece_count_or_time_war_separation_limit() {
    let mut e = lab();
    e.edit_lab(LabEdit::Boards {
        beginning: -260,
        horizon: 260,
    })
    .unwrap();
    for i in 0..40 {
        place(
            &mut e,
            PieceKind::N,
            Color::White,
            i * 2,
            Sq(1, 0),
            i % 2 == 1,
        );
    }
    assert_eq!(e.history().worldlines().len(), 40);
    assert_eq!(
        GameRecord::load(&GameRecord::from_lab(&e).to_text())
            .unwrap()
            .0,
        e
    );
}
