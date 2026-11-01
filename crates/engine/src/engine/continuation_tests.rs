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
fn cancelled_return_grants_only_one_boundary_reserve_for_either_color() {
    for color in [Color::White, Color::Black] {
        for gaps in [false, true] {
            let first = color.first_board();
            let source = state(color, Sq(1, 0), first + 4, true);
            let mut engine = Engine::from_worldlines(vec![vec![source]], color).unwrap();
            engine.history.causal_gaps = gaps;
            let mv = engine
                .possible_moves(0, 0)
                .unwrap()
                .into_iter()
                .find(|mv| !mv.to_past() && mv.sq == Sq(2, 2))
                .unwrap();
            engine.apply_move_inner(mv).unwrap();
            engine.undo_mv(mv.id()).unwrap();
            let line = &engine.history.worldlines[0];
            assert!(line[0].rights.cancelled_move);
            let active: Vec<_> = line.iter().filter(|s| s.rights.ordinary).collect();
            assert_eq!(active.len(), 1);
            assert_eq!(active[0].t, first);
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
                let mut engine =
                    Engine::from_worldlines(vec![vec![source], vec![blocker], vec![anchor]], color)
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
    let mut engine =
        Engine::from_worldlines(vec![vec![white, future], vec![black]], Color::White).unwrap();
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
    engine
        .history
        .worldlines
        .extend([vec![observer].into(), vec![target].into()]);
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
        let mut engine =
            Engine::from_worldlines(vec![vec![cancelled], vec![informed], vec![opponent]], color)
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
        vec![observer].into(),
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
            vec![observer],
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
