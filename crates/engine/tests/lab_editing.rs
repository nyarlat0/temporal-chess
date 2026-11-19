use temporal_chess_engine::{
    engine::{Engine, LabEdit},
    model::{Color, Move, PieceAt, PieceId, PieceKind, Sq, TimeDirection},
    record::GameRecord,
};

fn empty() -> Engine {
    Engine::new_lab(Default::default())
        .unwrap()
        .with_interactive_calculation()
        .unwrap()
}
fn boards(e: &mut Engine, beginning: i64, horizon: i64) {
    e.edit_lab(LabEdit::Boards { beginning, horizon }).unwrap();
}
fn put(e: &mut Engine, color: Color, t: i64, sq: Sq, inverted: bool, kind: PieceKind) {
    e.edit_lab(LabEdit::Place {
        t,
        square: sq,
        kind,
        color,
        inverted,
    })
    .unwrap();
}
fn state(e: &Engine, wid: usize, age: usize) -> PieceAt {
    let s = &e.history().worldlines()[wid][age];
    PieceAt {
        piece: PieceId::from_state(s, wid),
        t: s.t,
    }
}
fn stable(e: &Engine) {
    assert_eq!(
        GameRecord::load(&GameRecord::from_lab(e).to_text())
            .unwrap()
            .0,
        *e
    );
}
fn play(e: &mut Engine, wid: usize, t: i64, to: Sq, direction: TimeDirection) -> Move {
    e.edit_lab(LabEdit::Turn(e.history().worldlines()[wid][0].color))
        .unwrap();
    let p = e
        .move_sources()
        .find(|p| p.origin == wid && p.t == t)
        .unwrap();
    let mv = e
        .possible_moves(wid, p.age)
        .unwrap()
        .into_iter()
        .find(|m| m.sq == to && m.direction == direction)
        .unwrap();
    e.apply_new_mv(mv).unwrap();
    mv
}

#[test]
fn adding_boards_extends_normal_and_inverted_boundary_lines_on_both_ends() {
    let mut e = empty();
    put(&mut e, Color::White, 0, Sq(0, 0), false, PieceKind::N);
    put(&mut e, Color::Black, 0, Sq(1, 7), true, PieceKind::N);
    assert_eq!(e.history().beginning(), -1);
    assert_eq!(
        e.history().worldlines()[0][0].t,
        -1,
        "placement-created boards also prolong boundary ancestors"
    );
    let turn = e.turn();
    boards(&mut e, -3, 2);
    let normal = &e.history().worldlines()[0];
    let inv = &e.history().worldlines()[1];
    assert_eq!(
        normal.iter().map(|s| s.t).collect::<Vec<_>>(),
        [-3, -2, -1, 0, 1, 2]
    );
    assert_eq!(
        inv.iter().map(|s| s.t).collect::<Vec<_>>(),
        [2, 1, 0, -1, -2, -3]
    );
    assert!(
        normal
            .iter()
            .all(|s| !s.inverted && s.fut_seen == s.t && (s.x, s.y) == (0, 0))
    );
    assert!(
        inv.iter()
            .all(|s| s.inverted && s.fut_seen == 2 && (s.x, s.y) == (1, 7))
    );
    assert_eq!(e.turn(), turn);
    assert!(e.history().moves().is_empty());
    stable(&e);
}

#[test]
fn manual_prolongation_works_inside_existing_history_and_can_create_boards() {
    for inverted in [false, true] {
        let mut e = empty();
        boards(&mut e, -4, 4);
        put(&mut e, Color::White, 0, Sq(1, 0), inverted, PieceKind::N);
        put(&mut e, Color::White, 2, Sq(6, 0), false, PieceKind::N);
        let other = e.history().worldlines()[1].clone();
        e.edit_lab(LabEdit::Prolong {
            wid: 0,
            direction: TimeDirection::Future,
            count: 3,
        })
        .unwrap();
        e.edit_lab(LabEdit::Prolong {
            wid: 0,
            direction: TimeDirection::Past,
            count: 3,
        })
        .unwrap();
        let line = &e.history().worldlines()[0];
        assert_eq!(line.len(), 7);
        assert_eq!(line.iter().map(|s| s.t).min(), Some(-3));
        assert_eq!(line.iter().map(|s| s.t).max(), Some(3));
        assert!(
            line.iter()
                .all(|s| s.inverted == inverted && (s.x, s.y) == (1, 0) && s.loop_turn == 0)
        );
        assert_eq!(e.history().worldlines()[1], other);
        assert!(e.history().moves().is_empty());
        stable(&e);
    }
    let mut e = empty();
    put(&mut e, Color::White, 0, Sq(1, 0), false, PieceKind::N);
    put(&mut e, Color::White, 0, Sq(6, 0), false, PieceKind::N);
    e.edit_lab(LabEdit::Prolong {
        wid: 0,
        direction: TimeDirection::Future,
        count: 3,
    })
    .unwrap();
    assert_eq!(e.history().horizon(), 3);
    assert!(
        e.history()
            .worldlines()
            .iter()
            .all(|line| line.last().unwrap().t == 3)
    );
    stable(&e);
}

#[test]
fn removing_a_state_erases_its_causal_suffix_across_a_completed_loop() {
    let mut e = empty();
    boards(&mut e, -4, 6);
    put(&mut e, Color::White, 0, Sq(1, 0), false, PieceKind::N);
    let first = play(&mut e, 0, 0, Sq(2, 2), TimeDirection::Future);
    play(&mut e, 0, 2, Sq(1, 4), TimeDirection::Past);
    play(&mut e, 0, 0, Sq(0, 2), TimeDirection::Future);
    play(&mut e, 0, 2, Sq(1, 4), TimeDirection::Future);
    let at = state(&e, 0, 3);
    assert_eq!(at.t, 1);
    e.edit_lab(LabEdit::RemoveState { state: at }).unwrap();
    let line = &e.history().worldlines()[0];
    assert_eq!(line.iter().map(|s| s.t).collect::<Vec<_>>(), [0, 1, 2]);
    assert!(line.iter().all(|s| !s.inverted && s.loop_turn == 0));
    assert!(line.last().unwrap().rights.ordinary);
    assert_eq!(
        e.history().moves().values().copied().collect::<Vec<_>>(),
        [first]
    );
    stable(&e);
}

#[test]
fn deleting_middle_boards_cuts_to_the_boundary_and_keeps_zero() {
    let mut e = empty();
    put(&mut e, Color::White, 0, Sq(1, 0), false, PieceKind::N);
    boards(&mut e, -4, 6);
    e.edit_lab(LabEdit::RemoveBoard { t: 3 }).unwrap();
    assert_eq!((e.history().beginning(), e.history().horizon()), (-4, 2));
    e.edit_lab(LabEdit::RemoveBoard { t: -2 }).unwrap();
    assert_eq!((e.history().beginning(), e.history().horizon()), (-1, 2));
    assert_eq!(
        e.history().worldlines()[0]
            .iter()
            .map(|s| s.t)
            .collect::<Vec<_>>(),
        [-1, 0, 1, 2]
    );
    let before = e.clone();
    assert!(e.edit_lab(LabEdit::RemoveBoard { t: 0 }).is_err());
    assert_eq!(before, e);
    stable(&e);
}

#[test]
fn future_trimming_keeps_inverted_knowledge_and_surviving_prepared_moves() {
    let mut e = empty();
    boards(&mut e, -3, 1);
    put(&mut e, Color::Black, 1, Sq(1, 7), true, PieceKind::N);
    let old = play(&mut e, 0, 1, Sq(2, 5), TimeDirection::Past);
    boards(&mut e, -3, 3);
    let remapped = e.history().remap_move(old);
    assert_eq!(remapped.pid.fut_seen, 3);
    assert!(e.history().moves().contains_key(&remapped.id()));
    e.edit_lab(LabEdit::RemoveBoard { t: 3 }).unwrap();
    assert_eq!(e.history().worldlines()[0][0].t, 2);
    assert_eq!(e.history().worldlines()[0][0].fut_seen, 3);
    assert!(e.history().moves().contains_key(&remapped.id()));
    play(&mut e, 0, -1, Sq(4, 4), TimeDirection::Future);
    stable(&e);
}

#[test]
fn deleting_a_castling_arrival_or_board_restores_both_participants() {
    for direction in [TimeDirection::Future, TimeDirection::Past] {
        for whole_board in [false, true] {
            let mut e = empty();
            boards(&mut e, -4, 6);
            put(
                &mut e,
                Color::White,
                0,
                Sq(4, 0),
                false,
                PieceKind::K { moved: false },
            );
            put(
                &mut e,
                Color::White,
                0,
                Sq(7, 0),
                false,
                PieceKind::R { moved: false },
            );
            let mv = e
                .possible_moves(0, 0)
                .unwrap()
                .into_iter()
                .find(|m| m.castling().is_some() && m.direction == direction)
                .unwrap();
            e.apply_new_mv(mv).unwrap();
            if whole_board {
                e.edit_lab(LabEdit::RemoveBoard {
                    t: direction.step(0).unwrap(),
                })
                .unwrap();
            } else {
                let at = state(&e, 1, 1);
                e.edit_lab(LabEdit::RemoveState { state: at }).unwrap();
            }
            assert!(e.history().moves().is_empty());
            for (wid, x) in [(0, 4), (1, 7)] {
                assert!(
                    e.history().worldlines()[wid]
                        .iter()
                        .all(|s| s.x == x && s.y == 0 && !s.inverted)
                );
            }
            stable(&e);
        }
    }
    let mut e = empty();
    boards(&mut e, -2, 4);
    put(
        &mut e,
        Color::White,
        0,
        Sq(4, 0),
        false,
        PieceKind::K { moved: false },
    );
    put(
        &mut e,
        Color::White,
        0,
        Sq(7, 0),
        false,
        PieceKind::R { moved: false },
    );
    play(&mut e, 0, 0, Sq(6, 0), TimeDirection::Future);
    e.edit_lab(LabEdit::Remove { wid: 0 }).unwrap();
    assert_eq!(e.history().worldlines().len(), 1);
    assert!(
        e.history().worldlines()[0]
            .iter()
            .all(|s| s.x == 7 && s.kind == PieceKind::R { moved: false })
    );
    stable(&e);
}

#[test]
fn prolongation_does_not_silently_overwrite_a_different_causal_move() {
    let mut e = empty();
    boards(&mut e, -3, 3);
    put(&mut e, Color::Black, 1, Sq(1, 7), true, PieceKind::N);
    put(&mut e, Color::Black, 3, Sq(6, 7), true, PieceKind::N);
    e.edit_lab(LabEdit::Prolong {
        wid: 1,
        direction: TimeDirection::Past,
        count: 2,
    })
    .unwrap();
    play(&mut e, 0, 1, Sq(2, 5), TimeDirection::Past);
    play(&mut e, 1, 1, Sq(5, 5), TimeDirection::Past);
    assert_eq!(e.history().moves().len(), 2);
    let before = e.clone();
    assert!(
        e.edit_lab(LabEdit::Prolong {
            wid: 0,
            direction: TimeDirection::Future,
            count: 2
        })
        .is_err()
    );
    assert_eq!(e, before);
    stable(&e);
}
