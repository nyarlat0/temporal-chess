use super::*;
use crate::model::{BoardTime, Color, PieceKind, PieceView, Sq};

fn pair(color: Color, side: Castling, inverted: bool) -> (Engine, PieceAt) {
    let t = 6 + color.first_board();
    let y = if color == Color::White { 0 } else { 7 };
    let rook_x = if side == Castling::Short { 7 } else { 0 };
    let lines = [
        (PieceKind::K { moved: false }, 4),
        (PieceKind::R { moved: false }, rook_x),
    ]
    .into_iter()
    .map(|(kind, x)| {
        let mut first = State::new(kind, color, Sq(x, y));
        first.t = if inverted { t + 2 } else { 0 };
        first.fut_seen = first.t;
        first.inverted = inverted;
        let mut line = vec![first];
        for _ in 0..t + 2 {
            line.push(line.last().unwrap().prolongate().unwrap());
        }
        for s in &mut line {
            s.rights = MoveRights {
                ordinary: s.t == t,
                ..Default::default()
            };
        }
        line
    })
    .collect();
    let engine = Engine::from_worldlines(lines, color).unwrap();
    let source = engine
        .move_sources()
        .find(|p| p.origin == 0 && p.t == t)
        .unwrap();
    let source = Engine::piece_at(source);
    (engine, source)
}

fn select(engine: &Engine, source: PieceAt, side: Castling, direction: TimeDirection) -> Move {
    let piece = engine.occurrence(source).unwrap();
    engine
        .possible_moves(piece.origin, piece.age)
        .unwrap()
        .into_iter()
        .find(|mv| mv.castling() == Some(side) && mv.direction == direction)
        .unwrap()
}

fn stable(engine: &Engine) {
    assert!(
        engine
            .history
            .worldlines
            .iter()
            .all(|line| line.gaps().is_empty())
    );
    assert_eq!(
        Engine::from_history(engine.history.clone(), engine.turn).unwrap(),
        *engine
    );
}

fn play_from(engine: &mut Engine, wid: usize, t: BoardTime, direction: TimeDirection, to: Sq) {
    engine.turn = engine.history.worldlines[wid][0].color;
    engine.reconcile().unwrap();
    let piece = engine
        .move_sources()
        .find(|p| p.origin == wid && p.t == t)
        .unwrap();
    let mv = engine
        .possible_moves(wid, piece.age)
        .unwrap()
        .into_iter()
        .find(|m| m.sq == to && m.direction == direction)
        .unwrap();
    engine.apply_new_mv(mv).unwrap();
}

#[test]
fn a_rook_with_a_different_temporal_identity_still_cancels_its_castling() {
    for color in [Color::White, Color::Black] {
        for side in [Castling::Short, Castling::Long] {
            for direction in [TimeDirection::Future, TimeDirection::Past] {
                let (mut engine, source) = pair(color, side, false);
                let mut rook = engine.history.worldlines[1].last().unwrap().clone();
                rook.inverted = true;
                let mut line = vec![rook];
                for _ in 0..engine.history.horizon {
                    line.push(line.last().unwrap().prolongate().unwrap());
                }
                for state in &mut line {
                    state.rights = MoveRights::default();
                }
                engine.history.worldlines[1] = line.into();
                engine.update_bufs().unwrap();
                engine.reconcile().unwrap();
                let mv = select(&engine, source, side, direction);
                engine.apply_new_mv(mv).unwrap();
                let king = engine.occurrence(source).unwrap();
                let rook = engine.historical_castling_rook(mv, king).unwrap();
                assert_ne!(MoveId::from_piv(rook), mv.id());
                let arrival = PieceView::from_state(
                    &engine.history.worldlines[1][rook.age + 1],
                    1,
                    rook.age + 1,
                );
                engine.undo_last_mv(arrival).unwrap();
                engine.reconcile().unwrap();
                assert!(!engine.history.moves.contains_key(&mv.id()));
                stable(&engine);
            }
        }
    }
}

#[test]
fn cancelling_a_completed_castling_loop_retains_and_rejoins_both_futures() {
    for color in [Color::White, Color::Black] {
        for side in [Castling::Short, Castling::Long] {
            let (mut engine, source) = pair(color, side, false);
            let mv = select(&engine, source, side, TimeDirection::Past);
            engine.apply_new_mv(mv).unwrap();
            let (king_x, rook_x) = if side == Castling::Short {
                (6, 5)
            } else {
                (2, 3)
            };
            let (rank1, rank2) = if color == Color::White {
                (1, 2)
            } else {
                (6, 5)
            };
            play_from(
                &mut engine,
                0,
                source.t - 2,
                TimeDirection::Future,
                Sq(king_x, rank1),
            );
            play_from(
                &mut engine,
                0,
                source.t,
                TimeDirection::Future,
                Sq(king_x, rank2),
            );
            play_from(
                &mut engine,
                1,
                source.t - 4,
                TimeDirection::Future,
                Sq(rook_x, rank1),
            );
            play_from(
                &mut engine,
                1,
                source.t - 2,
                TimeDirection::Future,
                Sq(rook_x, rank2),
            );
            for wid in [0, 1] {
                assert!(
                    engine.history.worldlines[wid]
                        .iter()
                        .any(|s| s.t > source.t)
                );
            }
            engine.undo_mv(mv.id()).unwrap();
            engine.reconcile().unwrap();
            restored(&engine, side, color, false);
            assert!(
                engine.history.moves.is_empty(),
                "moves inside both erased loops must disappear"
            );
            for wid in [0, 1] {
                assert_eq!(
                    engine.history.worldlines[wid].last().unwrap().t,
                    source.t + 2
                );
            }
        }
    }
}

fn restored(engine: &Engine, side: Castling, color: Color, inverted: bool) {
    let y = if color == Color::White { 0 } else { 7 };
    for (wid, x, kind) in [
        (0, 4, PieceKind::K { moved: false }),
        (
            1,
            if side == Castling::Short { 7 } else { 0 },
            PieceKind::R { moved: false },
        ),
    ] {
        for state in engine.history.worldlines[wid].iter() {
            assert_eq!((state.x, state.y), (x, y), "wid={wid}, {state:?}");
            assert_eq!(state.kind, kind);
            assert_eq!(state.inverted, inverted);
            assert!(state.loop_pivots.is_empty());
            assert_eq!(state.loop_turn, 0);
        }
    }
    stable(engine);
}

#[test]
fn cancellation_of_castling_restores_both_temporal_routes_and_rights() {
    for color in [Color::White, Color::Black] {
        for side in [Castling::Short, Castling::Long] {
            for inverted in [false, true] {
                for direction in [TimeDirection::Future, TimeDirection::Past] {
                    for cancel_rook in [false, true] {
                        let (mut engine, source) = pair(color, side, inverted);
                        let mv = select(&engine, source, side, direction);
                        engine.apply_new_mv(mv).unwrap();
                        assert!(engine.history.moves.contains_key(&mv.id()));
                        if cancel_rook {
                            let rook_source = engine
                                .historical_castling_rook(mv, engine.occurrence(source).unwrap())
                                .unwrap();
                            let arrival = engine.get_piv_necro(1, rook_source.age + 1).unwrap();
                            engine.undo_last_mv(arrival).unwrap();
                        } else {
                            engine.undo_mv(mv.id()).unwrap();
                        }
                        engine.reconcile().unwrap();
                        assert!(!engine.history.moves.contains_key(&mv.id()));
                        restored(&engine, side, color, inverted);
                        let king = engine.occurrence(source).unwrap();
                        let rook = engine.historical_castling_rook(mv, king).unwrap();
                        assert!(
                            !rook.rights.cancelled_move,
                            "one shared command must not grant a separate rook reaction"
                        );
                        if inverted != direction.is_inverted() && !inverted {
                            for wid in [0, 1] {
                                let end = engine.history.worldlines[wid].last().unwrap();
                                assert_eq!(end.t, source.t);
                                assert!(end.rights.ordinary);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn replacing_castling_with_a_king_move_removes_only_the_old_rook_route() {
    for color in [Color::White, Color::Black] {
        for side in [Castling::Short, Castling::Long] {
            for old_direction in [TimeDirection::Future, TimeDirection::Past] {
                for new_direction in [TimeDirection::Future, TimeDirection::Past] {
                    let (mut engine, source) = pair(color, side, false);
                    let mv = select(&engine, source, side, old_direction);
                    engine.apply_new_mv(mv).unwrap();
                    let king = engine.occurrence(source).unwrap();
                    engine.history.worldlines[0][king.age].rights.cancelled_move = true;
                    engine.turn = color;
                    engine.update_bufs().unwrap();
                    engine.reconcile().unwrap();
                    let target = Sq(3, if color == Color::White { 1 } else { 6 });
                    let replacement = engine
                        .possible_moves(0, king.age)
                        .unwrap()
                        .into_iter()
                        .find(|m| m.sq == target && m.direction == new_direction)
                        .unwrap();
                    engine.apply_new_mv(replacement).unwrap();
                    assert_eq!(engine.history.moves.get(&mv.id()), Some(&replacement));
                    for state in engine.history.worldlines[1].iter() {
                        assert!(!state.inverted);
                        assert_eq!(state.x, if side == Castling::Short { 7 } else { 0 });
                        assert_eq!(state.kind, PieceKind::R { moved: false });
                        assert!(state.loop_pivots.is_empty());
                    }
                    stable(&engine);
                }
            }
        }
    }
}

#[test]
fn a_new_passive_blocker_on_the_rook_arrival_cancels_the_entire_castling() {
    for color in [Color::White, Color::Black] {
        for side in [Castling::Short, Castling::Long] {
            for direction in [TimeDirection::Future, TimeDirection::Past] {
                let (mut engine, source) = pair(color, side, false);
                let mv = select(&engine, source, side, direction);
                engine.apply_new_mv(mv).unwrap();
                let mut blocker = State::new(
                    PieceKind::N,
                    color,
                    Sq(
                        if side == Castling::Short { 5 } else { 3 },
                        if color == Color::White { 0 } else { 7 },
                    ),
                );
                blocker.t = direction.step(mv.t).unwrap();
                blocker.fut_seen = blocker.t;
                let first_t = blocker.t;
                let mut line = vec![blocker];
                for _ in first_t..engine.history.horizon {
                    line.push(line.last().unwrap().prolongate().unwrap());
                }
                engine.history.worldlines.push(line.into());
                engine.update_bufs().unwrap();
                engine.reconcile().unwrap();
                assert!(
                    !engine.history.moves.contains_key(&mv.id()),
                    "{color:?} {side:?} {direction:?}: {:?}",
                    engine.status()
                );
                // The newly inserted blocker is deliberately excluded here.
                for wid in [0, 1] {
                    let line = &engine.history.worldlines[wid];
                    assert!(line.iter().all(|s| !s.inverted));
                    assert!(line.iter().all(|s| s.x
                        == if wid == 0 {
                            4
                        } else if side == Castling::Short {
                            7
                        } else {
                            0
                        }));
                }
                stable(&engine);
            }
        }
    }
}

#[test]
fn castling_checks_the_selected_adjacent_board_and_requires_an_unmoved_rook() {
    for color in [Color::White, Color::Black] {
        for side in [Castling::Short, Castling::Long] {
            for blocked in [TimeDirection::Future, TimeDirection::Past] {
                let (mut engine, source) = pair(color, side, false);
                let mut blocker = State::new(
                    PieceKind::N,
                    color.opposite(),
                    Sq(
                        if side == Castling::Short { 5 } else { 3 },
                        if color == Color::White { 0 } else { 7 },
                    ),
                );
                blocker.t = blocked.step(source.t).unwrap();
                blocker.fut_seen = blocker.t;
                engine.history.worldlines.push(vec![blocker].into());
                engine.update_bufs().unwrap();
                engine.reconcile().unwrap();
                let king = engine.occurrence(source).unwrap();
                let castles: Vec<_> = engine
                    .possible_moves(0, king.age)
                    .unwrap()
                    .into_iter()
                    .filter(|m| m.castling() == Some(side))
                    .collect();
                assert_eq!(castles.len(), 1);
                assert_ne!(castles[0].direction, blocked);
                engine.apply_new_mv(castles[0]).unwrap();
                stable(&engine);
            }
            for moved_wid in [0, 1] {
                let (mut engine, source) = pair(color, side, false);
                for state in engine.history.worldlines[moved_wid].iter_mut() {
                    state.kind = state.kind.set_moved(true);
                }
                engine.update_bufs().unwrap();
                engine.reconcile().unwrap();
                let king = engine.occurrence(source).unwrap();
                assert!(
                    engine
                        .possible_moves(0, king.age)
                        .unwrap()
                        .iter()
                        .all(|m| m.castling().is_none())
                );
            }
        }
    }
}

#[test]
fn losing_the_historical_rook_cancels_normal_and_inverted_castling() {
    for color in [Color::White, Color::Black] {
        for side in [Castling::Short, Castling::Long] {
            for direction in [TimeDirection::Future, TimeDirection::Past] {
                let (mut engine, source) = pair(color, side, false);
                let mv = select(&engine, source, side, direction);
                engine.apply_new_mv(mv).unwrap();
                for state in engine.history.worldlines[1].iter_mut() {
                    state.alive = false;
                }
                engine.update_bufs().unwrap();
                engine.reconcile().unwrap();
                assert!(!engine.history.moves.contains_key(&mv.id()));
                restored(&engine, side, color, false);
            }
        }
    }
}

#[test]
fn inverted_castling_at_the_standard_boundary_creates_negative_boards_and_cancels() {
    for color in [Color::White, Color::Black] {
        for side in [Castling::Short, Castling::Long] {
            let t = color.first_board();
            let y = if color == Color::White { 0 } else { 7 };
            let lines = [
                (PieceKind::K { moved: false }, 4),
                (
                    PieceKind::R { moved: false },
                    if side == Castling::Short { 7 } else { 0 },
                ),
            ]
            .into_iter()
            .map(|(kind, x)| {
                let mut state = State::new(kind, color, Sq(x, y));
                state.t = t;
                state.fut_seen = t;
                state.rights.ordinary = true;
                vec![state]
            })
            .collect();
            let mut engine = Engine::from_worldlines(lines, color).unwrap();
            let source = Engine::piece_at(engine.get_piv(0, 0).unwrap());
            let mv = select(&engine, source, side, TimeDirection::Past);
            engine.apply_new_mv(mv).unwrap();
            assert_eq!(engine.history.beginning, t - 2);
            for wid in [0, 1] {
                assert!(engine.board(t + 1).unwrap().find_by_origin(wid).is_empty());
                let end = engine.history.worldlines[wid].last().unwrap();
                assert_eq!(end.t, t - 2);
                assert!(end.inverted && end.rights.ordinary);
            }
            stable(&engine);
            engine.undo_mv(mv.id()).unwrap();
            engine.reconcile().unwrap();
            restored(&engine, side, color, false);
            let king = engine.occurrence(source).unwrap();
            assert!(
                engine
                    .possible_moves(0, king.age)
                    .unwrap()
                    .iter()
                    .any(|m| m.castling() == Some(side) && m.to_past()),
                "cancelled castling must be available again"
            );
        }
    }
}
