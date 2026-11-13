use super::collision_rules::{CollisionEffect, collision_effects};
use super::*;
use crate::pieces::{Color, PieceKind, PieceView, Sq};

fn state(color: Color, sq: Sq, t: usize, inverted: bool) -> State {
    let mut s = State::new(PieceKind::N, color, sq);
    s.t = t;
    s.rights.ordinary = Color::from_board(t) == color;
    s.fut_seen = 10;
    s.inverted = inverted;
    s.loop_turn = usize::from(inverted);
    if inverted {
        s.loop_pivots.push(10);
    }
    s
}

#[test]
fn cancelled_return_leaves_an_ordinary_endpoint_for_either_color() {
    for color in [Color::White, Color::Black] {
        for gaps in [false, true] {
            let first = color.first_board();
            let source = state(color, Sq(1, 0), first + 4, true);
            let mut engine = Engine::from_worldlines(vec![vec![source]], color).unwrap();
            engine.history.rules.chain_info_activation = gaps;
            let mv = engine
                .possible_moves(0, 0)
                .unwrap()
                .into_iter()
                .find(|mv| !mv.to_past() && mv.sq == Sq(2, 2))
                .unwrap();
            engine.apply_move_inner(mv).unwrap();
            engine.undo_mv(mv.id()).unwrap();
            let line = &engine.history.worldlines[0];
            assert!(!line[0].rights.cancelled_move);
            assert_eq!(line.len(), 1);
            let active: Vec<_> = line.iter().filter(|s| s.rights.ordinary).collect();
            assert_eq!(active.len(), 1);
            assert_eq!(active[0].t, first + 4);
            assert!(
                line.iter()
                    .all(|s| s.alive && s.inverted && s.x == 1 && s.y == 0)
            );
            engine.history.worldlines[0][0].expire_reactions();
            assert!(engine.history.worldlines[0].last().unwrap().rights.ordinary);
            engine.reconcile().unwrap();
            assert_eq!(
                Engine::from_history(engine.history.clone(), color).unwrap(),
                engine
            );
        }
    }
}

#[test]
fn waiting_stops_before_any_piece_on_an_intermediate_board() {
    for color in [Color::White, Color::Black] {
        for inverted in [false, true] {
            for blocker_color in [color, color.opposite()] {
                let start = color.first_board() + if inverted { 6 } else { 0 };
                let time = |distance| {
                    if inverted {
                        start - distance
                    } else {
                        start + distance
                    }
                };
                let source = state(color, Sq(1, 2), start, inverted);
                let blocker = state(blocker_color, Sq(1, 2), time(3), false);
                let anchor = state(color.opposite(), Sq(7, 7), 8, false);
                let mut engine = super::legacy_from_worldlines(
                    vec![vec![source], vec![blocker], vec![anchor]],
                    color,
                )
                .unwrap();
                let source = Engine::piece_at(engine.get_piv(0, 0).unwrap());
                let before = engine.clone();
                assert_eq!(
                    engine.skip_ordinary(source, time(4)),
                    Err(EngineErr::WrongMove)
                );
                assert_eq!(engine, before);
                assert!(!engine.possible_skips(source).contains(&time(4)));
                engine.skip_ordinary(source, time(2)).unwrap();
                assert!(engine.history.moves.is_empty());
                assert_eq!(engine.turn(), color);
                // A blocker on the actual own-color destination also prevents waiting.
                let mut blocked = before;
                blocked.history.worldlines[1][0].t = time(2);
                blocked.update_bufs().unwrap();
                blocked.build_boards(8).unwrap();
                assert_eq!(
                    blocked.skip_ordinary(source, time(2)),
                    Err(EngineErr::WrongMove)
                );
            }
        }
    }
}

#[test]
fn later_capture_does_not_revalidate_or_cancel_a_wait() {
    let white = state(Color::White, Sq(1, 2), 0, false);
    let black = state(Color::Black, Sq(3, 3), 1, false);
    let anchor = state(Color::Black, Sq(7, 7), 2, false);
    let mut engine =
        Engine::from_worldlines(vec![vec![white], vec![black], vec![anchor]], Color::White)
            .unwrap();
    let source = Engine::piece_at(engine.get_piv(0, 0).unwrap());
    engine.skip_ordinary(source, 2).unwrap();
    assert!(engine.history.moves.is_empty());
    engine.turn = Color::Black;
    let capture = engine
        .possible_moves(1, 0)
        .unwrap()
        .into_iter()
        .find(|mv| !mv.to_past() && mv.sq == Sq(1, 2))
        .unwrap();
    engine.apply_new_mv(capture).unwrap();
    assert!(engine.history.worldlines[0][0].alive);
    assert!(!engine.history.worldlines[0][2].alive);
    assert!(
        engine.history.worldlines[0]
            .iter()
            .all(|s| !s.rights.cancelled_move)
    );
    assert_eq!(engine.history.moves.len(), 1);
}

#[test]
fn harmless_cancelled_continuation_does_not_pause() {
    let mut white = state(Color::White, Sq(3, 3), 2, false);
    white.rights.cancelled_move = true;
    white.rights.ordinary = false;
    let next = white.prolongate().unwrap();
    let engine = Engine::from_worldlines(vec![vec![white, next]], Color::White)
        .unwrap()
        .with_interactive_calculation()
        .unwrap();
    assert_eq!(engine.calculation_status(), CalculationStatus::Complete);
    assert!(engine.board(3).is_some());
}

#[test]
fn pawn_collision_uses_capture_geometry_before_promotion() {
    for color in [Color::White, Color::Black] {
        for inverted in [false, true] {
            for dy in [1, 2] {
                let mut before = state(color, Sq(3, 3), 4, inverted);
                before.kind = PieceKind::P { moved: false };
                let mut after = before.prolongate().unwrap();
                after.y = (3_i8 + dy * color.pawn_dir()) as usize;
                for promoted in [false, true] {
                    after.kind = if promoted { PieceKind::Q } else { before.kind };
                    let bf = PieceView::from_state(&before, 0, 0);
                    let mover = PieceView::from_state(&after, 0, 1);
                    let mut target = mover;
                    target.origin = 1;
                    target.color = color.opposite();
                    let col = PieceCollision {
                        bf_collider: bf,
                        collider: mover,
                        collided: target,
                    };
                    assert_eq!(
                        collision_effects(col),
                        vec![CollisionEffect::CancelLastMove(mover)]
                    );
                    if dy == 1 {
                        let mut diagonal = mover;
                        diagonal.sq.0 += 1;
                        target.sq = diagonal.sq;
                        assert_eq!(
                            collision_effects(PieceCollision {
                                collider: diagonal,
                                collided: target,
                                ..col
                            }),
                            vec![CollisionEffect::KillFrom(ShortPieceId::from_piv(&target))]
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn dependent_move_is_not_cancelled_before_confirmation() {
    let mut line = vec![state(Color::White, Sq(1, 0), 0, false)];
    for _ in 0..6 {
        line.push(line.last().unwrap().prolongate().unwrap());
    }
    let mut engine = Engine::from_worldlines(vec![line], Color::White).unwrap();
    let first = engine
        .possible_moves(0, 0)
        .unwrap()
        .into_iter()
        .find(|mv| !mv.to_past() && mv.sq == Sq(2, 2))
        .unwrap();
    engine.apply_move_inner(first).unwrap();
    engine.reconcile().unwrap();
    let later = engine
        .possible_moves(0, 2)
        .unwrap()
        .into_iter()
        .find(|mv| !mv.to_past() && mv.sq == Sq(3, 4))
        .unwrap();
    engine.apply_move_inner(later).unwrap();
    engine.history.calculation.interactive = true;
    engine.undo_mv(first.id()).unwrap();
    engine.reconcile().unwrap();
    assert_eq!(
        engine.calculation_status(),
        CalculationStatus::Paused {
            t: 0,
            side: Color::White
        }
    );
    assert!(engine.history.moves.contains_key(&later.id()));
    assert!(!engine.history.worldlines[0][2].rights.cancelled_move);
    engine.continue_calculation().unwrap();
    assert!(!engine.history.moves.contains_key(&later.id()));
    assert!(engine.history.worldlines[0][2].rights.cancelled_move);
}

#[test]
fn cancellation_pauses_before_opponent_information_is_activated() {
    let white = state(Color::White, Sq(3, 3), 2, false);
    let future = white.prolongate().unwrap();
    let mut black = state(Color::Black, Sq(3, 7), 3, false);
    black.kind = PieceKind::R { moved: true };
    black.rights.ordinary = false;
    let mut engine = Engine::from_worldlines(
        vec![
            vec![white, future],
            vec![black.clone(), black.prolongate().unwrap()],
        ],
        Color::White,
    )
    .unwrap();
    let mv = engine
        .possible_moves(0, 0)
        .unwrap()
        .into_iter()
        .find(|m| !m.to_past() && m.sq == Sq(5, 4))
        .unwrap();
    engine.apply_move_inner(mv).unwrap();
    engine.reconcile().unwrap();
    engine.history.calculation.interactive = true;
    engine.undo_mv(mv.id()).unwrap();
    engine.reconcile().unwrap();
    assert_eq!(
        engine.calculation_status(),
        CalculationStatus::Paused {
            t: 2,
            side: Color::White
        }
    );
    assert!(!engine.history.worldlines[1][0].rights.new_information);
    assert!(engine.board(3).is_none());
    engine.continue_calculation().unwrap();
    assert_eq!(engine.calculation_status(), CalculationStatus::Complete);
    assert_eq!(engine.turn(), Color::White);
}

fn harmless_cancellation() -> Engine {
    let start = state(Color::White, Sq(1, 0), 0, false);
    let mut line = vec![start];
    for _ in 0..4 {
        line.push(line.last().unwrap().prolongate().unwrap());
    }
    let mut engine = Engine::from_worldlines(vec![line], Color::White).unwrap();
    let mv = engine
        .possible_moves(0, 0)
        .unwrap()
        .into_iter()
        .find(|m| !m.to_past() && m.sq == Sq(2, 2))
        .unwrap();
    engine.apply_move_inner(mv).unwrap();
    engine.reconcile().unwrap();
    engine.history.calculation.interactive = true;
    engine.undo_mv(mv.id()).unwrap();
    engine
}

#[test]
fn unrelated_information_and_conflicts_do_not_pause_a_cancelled_piece() {
    let mut engine = harmless_cancellation();
    let mut observer = state(Color::Black, Sq(7, 7), 3, false);
    observer.kind = PieceKind::R { moved: true };
    observer.rights.ordinary = false;
    let target = state(Color::White, Sq(7, 3), 3, false);
    engine.history.worldlines.extend([
        vec![observer.clone(), observer.prolongate().unwrap()].into(),
        vec![target].into(),
    ]);
    engine.update_bufs().unwrap();
    assert_eq!(
        engine.calculation_barrier(),
        None,
        "independent new information"
    );
    let collision = state(Color::White, Sq(7, 3), 3, false);
    engine.history.worldlines.push(vec![collision].into());
    engine.update_bufs().unwrap();
    assert_eq!(engine.calculation_barrier(), None, "independent conflict");
    // Moving that conflict onto a state introduced by the cancellation makes it causal.
    for wid in [2, 3] {
        engine.history.worldlines[wid][0].x = 1;
        engine.history.worldlines[wid][0].y = 0;
    }
    engine.update_bufs().unwrap();
    assert_eq!(engine.calculation_barrier(), Some(0));
}

#[test]
fn unrelated_invalid_move_does_not_pause_a_cancelled_piece() {
    let mut engine = harmless_cancellation();
    let source = state(Color::White, Sq(4, 3), 2, false);
    engine
        .history
        .worldlines
        .push(vec![source.clone(), source.prolongate().unwrap()].into());
    engine.update_bufs().unwrap();
    engine.reconcile().unwrap();
    let mv = engine
        .possible_moves(1, 0)
        .unwrap()
        .into_iter()
        .find(|m| !m.to_past() && m.sq == Sq(6, 4))
        .unwrap();
    engine.apply_move_inner(mv).unwrap();
    // Reactivate the original cancellation: playing a later move had expired it.
    engine.history.worldlines[0][0].rights.cancelled_move = true;
    engine.history.worldlines[1][0].y = 2;
    engine.update_bufs().unwrap();
    assert_eq!(engine.calculation_barrier(), None);
    engine.reconcile().unwrap();
    assert!(!engine.history.moves.contains_key(&mv.id()));
    assert_eq!(engine.calculation_barrier(), None);
}

#[test]
fn decline_reactions_requires_only_reactions_and_preserves_states() {
    use crate::record::{Command, GameRecord};
    for color in [Color::White, Color::Black] {
        let t = color.first_board();
        let mut cancelled = state(color, Sq(1, 0), t, false);
        cancelled.rights.ordinary = false;
        cancelled.rights.cancelled_move = true;
        let mut informed = state(color, Sq(2, 0), t, false);
        informed.rights.ordinary = false;
        informed.rights.new_information = true;
        let opponent = state(
            color.opposite(),
            Sq(7, 7),
            color.opposite().first_board(),
            false,
        );
        let mut engine = Engine::from_worldlines(
            vec![
                vec![cancelled.clone(), cancelled.prolongate().unwrap()],
                vec![informed.clone(), informed.prolongate().unwrap()],
                vec![opponent],
            ],
            color,
        )
        .unwrap();
        let initial = engine.clone();
        assert!(engine.can_decline_reactions());
        let mut record = GameRecord::default();
        record
            .apply(&mut engine, Command::DeclineReactions)
            .unwrap();
        assert_eq!(engine.turn(), color.opposite());
        assert_eq!(engine.history.moves, initial.history.moves);
        assert_eq!(engine.history.horizon, initial.history.horizon);
        for wid in [0, 1] {
            assert!(!engine.history.worldlines[wid][0].rights.is_movable());
            assert!(engine.history.worldlines[wid][0].alive);
        }
        assert_eq!(engine.history.worldlines[2], initial.history.worldlines[2]);
        assert_eq!(record.commands(), &[Command::DeclineReactions]);
        let mut replayed = initial.clone();
        for command in record.commands() {
            GameRecord::default()
                .apply(&mut replayed, command.to_string().parse().unwrap())
                .unwrap();
        }
        assert_eq!(replayed, engine);
        for ordinary in [false, true] {
            let mut rejected = initial.clone();
            for line in &mut rejected.history.worldlines {
                if line[0].color == color {
                    line[0].rights.expire_reactions();
                    line[0].rights.ordinary = ordinary;
                }
            }
            if ordinary {
                rejected.history.worldlines[0][0].rights.cancelled_move = true;
            }
            rejected.reconcile().unwrap();
            let before = rejected.clone();
            assert!(!rejected.can_decline_reactions());
            assert_eq!(rejected.decline_reactions(), Err(EngineErr::WrongMove));
            assert_eq!(rejected, before);
        }
    }
}

#[test]
fn barrier_belongs_to_the_causal_cancellation_not_an_earlier_harmless_one() {
    let mut engine = harmless_cancellation();
    let source = state(Color::White, Sq(3, 3), 2, false);
    let mut observer = state(Color::Black, Sq(3, 7), 3, false);
    observer.kind = PieceKind::R { moved: true };
    observer.rights.ordinary = false;
    engine.history.worldlines.extend([
        vec![source.clone(), source.prolongate().unwrap()].into(),
        vec![observer.clone(), observer.prolongate().unwrap()].into(),
    ]);
    engine.update_bufs().unwrap();
    engine.reconcile().unwrap();
    let mv = engine
        .possible_moves(1, 0)
        .unwrap()
        .into_iter()
        .find(|m| !m.to_past() && m.sq == Sq(5, 4))
        .unwrap();
    engine.apply_move_inner(mv).unwrap();
    engine.history.worldlines[0][0].rights.cancelled_move = true;
    engine.undo_mv(mv.id()).unwrap();
    assert_eq!(engine.calculation_barrier(), Some(2));
}

#[test]
fn decline_reactions_accepts_a_paused_continuation_and_does_not_reactivate_it() {
    let source = state(Color::White, Sq(3, 3), 2, false);
    let mut observer = state(Color::Black, Sq(3, 7), 3, false);
    observer.kind = PieceKind::R { moved: true };
    observer.rights.ordinary = false;
    let mut engine = Engine::from_worldlines(
        vec![
            vec![source.clone(), source.prolongate().unwrap()],
            vec![observer.clone(), observer.prolongate().unwrap()],
        ],
        Color::White,
    )
    .unwrap();
    let mv = engine
        .possible_moves(0, 0)
        .unwrap()
        .into_iter()
        .find(|m| !m.to_past() && m.sq == Sq(5, 4))
        .unwrap();
    engine.apply_move_inner(mv).unwrap();
    engine.reconcile().unwrap();
    engine.history.calculation.interactive = true;
    engine.undo_mv(mv.id()).unwrap();
    engine.reconcile().unwrap();
    let end = engine.history.worldlines[0]
        .last()
        .unwrap()
        .prolongate()
        .unwrap();
    engine.history.worldlines[0].states.push(end);
    for s in engine.history.worldlines[0].iter_mut() {
        s.rights.ordinary = false;
    }
    engine.reconcile().unwrap();
    assert_eq!(engine.calculation_barrier(), Some(2));
    assert!(engine.can_decline_reactions());
    engine.decline_reactions().unwrap();
    assert_eq!(engine.turn(), Color::Black);
    assert_eq!(engine.calculation_status(), CalculationStatus::Complete);
    assert!(engine.board(3).is_some());
    assert!(engine.history.worldlines[1][0].rights.new_information);
    assert!(!engine.history.worldlines[0][0].rights.is_movable());
    engine.turn = Color::White;
    engine.reconcile().unwrap();
    assert!(!engine.history.worldlines[0][0].rights.is_movable());
}

#[test]
fn cancelling_closed_loop_preserves_connected_future_in_both_modes() {
    for gaps in [false, true] {
        let mut source = State::new(PieceKind::R { moved: true }, Color::White, Sq(0, 0));
        source.t = 4;
        source.fut_seen = 4;
        source.rights.ordinary = false;
        let mut states = vec![source];
        for direction in [
            TimeDirection::Past,
            TimeDirection::Past,
            TimeDirection::Future,
            TimeDirection::Future,
            TimeDirection::Future,
            TimeDirection::Future,
            TimeDirection::Future,
        ] {
            let mut next = states.last().unwrap().step(direction).unwrap();
            next.x = 1;
            states.push(next);
        }
        states[7].x = 2;
        let begin = Move {
            pid: PieceId::from_state(&states[0], 0),
            t: 4,
            direction: TimeDirection::Past,
            sq: Sq(1, 0),
            effect: MoveEffect::Normal,
        };
        let later = Move {
            pid: PieceId::from_state(&states[6], 0),
            t: 6,
            direction: TimeDirection::Future,
            sq: Sq(2, 0),
            effect: MoveEffect::Normal,
        };
        let mut engine =
            Engine::from_worldlines(vec![vec![states[0].clone()]], Color::White).unwrap();
        engine.history.worldlines[0] = states.into();
        engine.history.horizon = 7;
        engine.update_bufs().unwrap();
        engine.history.rules.chain_info_activation = gaps;
        engine.history.moves.insert(begin.id(), begin);
        engine.history.moves.insert(later.id(), later);
        engine.undo_mv(begin.id()).unwrap();
        let line = &engine.history.worldlines[0];
        assert_eq!(line.iter().map(|s| s.t).collect::<Vec<_>>(), [4, 5, 6, 7]);
        assert_eq!(line.iter().map(|s| s.x).collect::<Vec<_>>(), [0, 0, 0, 2]);
        assert!(line.gaps().is_empty());
        assert!(line[0].rights.cancelled_move);
        assert!(!line[0].rights.ordinary);
        assert_eq!(engine.history.moves.get(&later.id()), Some(&later));
        engine.reconcile().unwrap();
        assert_eq!(engine.history.moves.get(&later.id()), Some(&later));
        assert_eq!(
            Engine::from_history(engine.history.clone(), engine.turn()).unwrap(),
            engine
        );
    }
}

#[test]
fn cancelling_open_loop_does_not_create_passive_states() {
    for color in [Color::White, Color::Black] {
        for gaps in [false, true] {
            let source = state(color, Sq(1, 0), color.first_board() + 4, false);
            let anchor = state(color.opposite(), Sq(7, 7), source.t - 1, false);
            let mut engine =
                Engine::from_worldlines(vec![vec![source], vec![anchor]], color).unwrap();
            engine.history.rules.chain_info_activation = gaps;
            let mv = engine
                .possible_moves(0, 0)
                .unwrap()
                .into_iter()
                .find(|m| m.to_past() && m.sq == Sq(2, 2))
                .unwrap();
            engine.apply_move_inner(mv).unwrap();
            engine.undo_mv(mv.id()).unwrap();
            engine.reconcile().unwrap();
            let line = &engine.history.worldlines[0];
            assert_eq!(line.len(), 1);
            assert!(line[0].rights.ordinary);
            assert!(!line[0].rights.is_reaction());
            assert!(line.gaps().is_empty());
        }
    }
}

#[test]
fn pawn_cancellation_pauses_before_it_blocks_inverted_bishop() {
    use crate::record::{Command, GameRecord};
    let text = "NOLAN-CHESS 2\ncausal_gaps off\nmove 0 12:0:0 e2 e4 + normal\nmove 1 28:1:0 e7 e5 + normal\nmove 2 6:2:0 g1 f3 - normal\nmove 3 21:3:0 f8 b4 - normal\nmove 0 6:2:1 f3 e5 + normal\n";
    let (mut engine, mut record) = GameRecord::load(text).unwrap();
    let Command::Move { mv: pawn, .. } = record.commands()[1] else {
        unreachable!()
    };
    let Command::Move { mv: bishop, .. } = record.commands()[3] else {
        unreachable!()
    };
    assert_eq!(
        engine.calculation_status(),
        CalculationStatus::Paused {
            t: 1,
            side: Color::Black
        }
    );
    assert!(!engine.history.moves.contains_key(&pawn.id()));
    assert!(
        engine
            .piece_state(pawn.source())
            .unwrap()
            .rights
            .cancelled_move
    );
    assert_eq!(engine.history.moves.get(&bishop.id()), Some(&bishop));
    assert!(engine.board(1).is_some());
    assert!(engine.board(2).is_none());
    record.apply(&mut engine, Command::Continue).unwrap();
    assert!(!engine.history.moves.contains_key(&bishop.id()));
    assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, engine);
}

#[test]
fn every_offered_move_is_accepted_in_king_inversion_position() {
    use crate::record::GameRecord;
    let (engine, _) = GameRecord::load("NOLAN-CHESS 2\ncausal_gaps off\nmove 0 12:0:0 e2 e3 + normal\nmove 1 25:1:0 b7 b6 + normal\n").unwrap();
    let king = *engine.board(2).unwrap().get_piece(4, 0).unwrap();
    let requested = engine
        .possible_moves(king.origin, king.age)
        .unwrap()
        .into_iter()
        .find(|mv| mv.to_past() && mv.sq == Sq(4, 1))
        .expect("king move stays offered");
    let mut probe = engine.clone();
    probe.apply_new_mv(requested).unwrap();
    assert!(probe.get_piv(king.origin, king.age + 1).is_some());
    assert!(!probe.history.worldlines[12][0].alive);
    for piece in engine.move_sources() {
        for mv in engine.possible_moves(piece.origin, piece.age).unwrap() {
            let mut played = engine.clone();
            assert_eq!(played.apply_new_mv(mv), Ok(()), "offered {mv:?}");
        }
    }
}

#[test]
fn oncoming_contact_at_a_worldline_origin_uses_existing_policy_for_both_colors() {
    for color in [Color::White, Color::Black] {
        for other_color in [color, color.opposite()] {
            let ordinary = state(color, Sq(4, 1), color.first_board(), false);
            let inverted = state(other_color, Sq(4, 1), ordinary.t, true);
            let mut engine = Engine::default();
            engine.history.worldlines = vec![vec![ordinary].into(), vec![inverted].into()];
            let ordinary = engine.get_piv(0, 0).unwrap();
            let inverted = engine.get_piv(1, 0).unwrap();
            for pair in [(ordinary, inverted), (inverted, ordinary)] {
                let collision = engine.track_collision(pair.0, pair.1).unwrap();
                assert_eq!(collision.collider, ordinary);
                assert_eq!(collision.collided, inverted);
                assert_eq!(
                    collision_effects(collision),
                    vec![CollisionEffect::KillFrom(ShortPieceId::from_piv(&ordinary))]
                );
            }
        }
    }
}

#[test]
fn cancelling_a_dead_movers_command_preserves_death_in_both_directions() {
    for color in [Color::White, Color::Black] {
        for gaps in [false, true] {
            for inverted in [false, true] {
                for direction in [TimeDirection::Past, TimeDirection::Future] {
                    let source = state(color, Sq(1, 0), color.first_board() + 4, inverted);
                    let anchor = state(color.opposite(), Sq(7, 7), source.t - 1, false);
                    let mut engine =
                        Engine::from_worldlines(vec![vec![source], vec![anchor]], color).unwrap();
                    engine.history.rules.chain_info_activation = gaps;
                    let mv = engine
                        .possible_moves(0, 0)
                        .unwrap()
                        .into_iter()
                        .find(|mv| mv.direction == direction && mv.sq == Sq(2, 2))
                        .unwrap();
                    engine.apply_move_inner(mv).unwrap();
                    engine.kill_piece(0, 0).unwrap();
                    engine.update_bufs().unwrap();
                    // A dead source cannot submit commands, but its recorded
                    // command remains editable when another piece replaces it.
                    assert_eq!(engine.apply_new_mv(mv), Err(EngineErr::WrongPiece));
                    engine.undo_mv(mv.id()).unwrap();
                    assert!(!engine.history.moves.contains_key(&mv.id()));
                    assert!(engine.history.worldlines[0].iter().all(|s| !s.alive));
                    assert_eq!(engine.get_piv_necro(0, 0).unwrap().sq, Sq(1, 0));
                }
            }
        }
    }
}

#[test]
fn shared_turn_continues_idle_endpoints_only_to_their_next_own_board() {
    for color in [Color::White, Color::Black] {
        for inverted in [false, true] {
            let t = color.first_board() + 4;
            let mover = state(color, Sq(1, 0), t, inverted);
            let mut idle = state(color, Sq(7, 3), t, inverted);
            idle.kind = PieceKind::R { moved: false };
            let anchor = state(color.opposite(), Sq(7, 7), t - 1, false);
            let future = state(color.opposite(), Sq(0, 7), t + 4, false);
            let mut engine = Engine::from_worldlines(
                vec![vec![mover], vec![idle], vec![anchor], vec![future]],
                color,
            )
            .unwrap();
            assert_eq!(engine.history.worldlines[1].len(), 1);
            let mv = engine
                .possible_moves(0, 0)
                .unwrap()
                .into_iter()
                .find(|mv| mv.sq == Sq(2, 2) && mv.to_past() == inverted)
                .unwrap();
            engine.apply_new_mv(mv).unwrap();
            let line = &engine.history.worldlines[1];
            assert_eq!(line.len(), 3);
            assert_eq!(line[2].t, if inverted { t - 2 } else { t + 2 });
            assert!(!line[0].rights.ordinary);
            assert!(!line[1].rights.ordinary);
            assert!(line[2].rights.ordinary);
            assert!(line.iter().all(|s| s.alive
                && Sq(s.x, s.y) == Sq(7, 3)
                && s.kind == PieceKind::R { moved: false }));
            assert!(engine.history.moves.values().all(|mv| mv.pid.wid != 1));
        }
    }
}

#[test]
fn shared_turn_does_not_discard_an_inverted_reserve_at_the_boundary() {
    for color in [Color::White, Color::Black] {
        let t = color.first_board();
        let mover = state(color, Sq(1, 0), t, true);
        let reserve = state(color, Sq(7, 3), t, true);
        let mut engine = Engine::from_worldlines(vec![vec![mover], vec![reserve]], color).unwrap();
        let mv = engine
            .possible_moves(0, 0)
            .unwrap()
            .into_iter()
            .find(|mv| mv.sq == Sq(2, 2) && !mv.to_past())
            .unwrap();
        engine.apply_new_mv(mv).unwrap();
        let line = &engine.history.worldlines[1];
        assert_eq!(line.len(), 1);
        assert!(line[0].alive && line[0].rights.ordinary && line[0].inverted);
    }
}

#[test]
fn backward_bishop_command_replaces_a_dead_queens_slot() {
    let mut queen = state(Color::White, Sq(2, 3), 4, false);
    queen.kind = PieceKind::Q;
    queen.alive = false;
    let mut bishop = state(Color::White, Sq(3, 2), 4, false);
    bishop.kind = PieceKind::B;
    let mut enemy = state(Color::Black, Sq(2, 3), 5, true);
    enemy.kind = PieceKind::Q;
    let mut enemy_line = vec![enemy];
    for _ in 0..2 {
        enemy_line.push(
            enemy_line
                .last()
                .unwrap()
                .step(TimeDirection::Past)
                .unwrap(),
        );
    }
    let mut engine = Engine::from_worldlines(
        vec![vec![queen.clone()], vec![bishop], enemy_line],
        Color::White,
    )
    .unwrap();
    let old = Move {
        pid: PieceId::from_state(&queen, 0),
        t: 4,
        sq: Sq(1, 2),
        direction: TimeDirection::Future,
        effect: MoveEffect::Normal,
    };
    let mut arrival = queen.step(TimeDirection::Future).unwrap();
    arrival.apply_mv(old);
    engine.history.worldlines[0].states.push(arrival);
    engine.history.moves.insert(old.id(), old);
    engine.update_bufs().unwrap();
    assert_eq!(engine.build_boards(5), Ok(ValidRes::Unchanged));
    let mv = engine
        .possible_moves(1, 0)
        .unwrap()
        .into_iter()
        .find(|m| m.to_past() && m.sq == Sq(2, 3))
        .unwrap();
    assert_eq!(mv.id(), old.id());
    assert!(!engine.piece_state(old.source()).unwrap().alive);
    engine.apply_new_mv(mv).unwrap();
    assert_ne!(engine.history.moves.get(&old.id()), Some(&old));
    assert!(
        engine.history.worldlines[2]
            .iter()
            .any(|s| s.t == 3 && !s.alive)
    );
    assert_eq!(
        Engine::from_history(engine.history.clone(), engine.turn()).unwrap(),
        engine
    );
}
