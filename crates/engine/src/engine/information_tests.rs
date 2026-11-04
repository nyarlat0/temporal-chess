use super::*;
use crate::model::{Color, PieceKind, Sq};

fn line(kind: PieceKind, side: Color, sq: Sq, inverted: bool) -> Vec<State> {
    let mut first = State::new(kind, side, sq);
    first.t = if inverted { 4 } else { 0 };
    first.fut_seen = 4;
    first.inverted = inverted;
    first.loop_turn = usize::from(inverted);
    first.rights = MoveRights::default();
    let mut states = vec![first];
    while let Some(mut next) = states.last().unwrap().prolongate() {
        if next.t > 4 {
            break;
        }
        next.rights = MoveRights::default();
        states.push(next);
    }
    states
}

fn observed(inverted: bool) -> Engine {
    Engine::from_worldlines(
        vec![
            line(
                PieceKind::R { moved: true },
                Color::White,
                Sq(0, 0),
                inverted,
            ),
            line(PieceKind::N, Color::Black, Sq(0, 4), false),
        ],
        Color::White,
    )
    .unwrap()
}

fn reacting_times(engine: &Engine, wid: usize) -> Vec<usize> {
    engine.history.worldlines[wid]
        .iter()
        .filter(|s| s.rights.new_information)
        .map(|s| s.t)
        .collect()
}

#[test]
fn first_observation_is_selected_in_causal_order_and_per_worldline() {
    for inverted in [false, true] {
        let mut engine = observed(inverted);
        engine.history.worldlines.push(
            line(
                PieceKind::R { moved: true },
                Color::White,
                Sq(7, 4),
                inverted,
            )
            .into(),
        );
        engine.reconcile().unwrap();
        let first_t = if inverted { 4 } else { 0 };
        assert_eq!(reacting_times(&engine, 0), [first_t]);
        assert_eq!(reacting_times(&engine, 2), [first_t]);
        let before = engine.clone();
        engine.reconcile().unwrap();
        assert_eq!(engine, before, "recalculation must not consume a reaction");
        assert!(
            engine.history.worldlines[0]
                .iter()
                .all(|s| s.remembered.is_empty())
        );
        assert_eq!(
            Engine::from_history(engine.history.clone(), engine.turn).unwrap(),
            engine
        );
    }
}

#[test]
fn disappearance_within_the_same_turn_clears_or_reassigns_the_reaction() {
    for inverted in [false, true] {
        let mut engine = observed(inverted);
        let first_t = if inverted { 4 } else { 0 };
        engine.history.worldlines[1][first_t].x = 2;
        engine.reconcile().unwrap();
        assert_eq!(reacting_times(&engine, 0), [2]);
        for state in engine.history.worldlines[1].iter_mut() {
            state.x = 2;
        }
        engine.reconcile().unwrap();
        assert!(reacting_times(&engine, 0).is_empty());
        assert!(
            engine.history.worldlines[0]
                .iter()
                .all(|s| s.informed.is_empty() && s.remembered.is_empty())
        );
        engine.history.worldlines[1][first_t].x = 0;
        engine.reconcile().unwrap();
        assert_eq!(reacting_times(&engine, 0), [first_t]);
        assert_eq!(engine.turn(), Color::White);
    }
}

#[test]
fn remembering_is_additive_and_does_not_accept_later_board_reactions() {
    let mut engine = observed(true);
    let target = PieceId::from_state(&engine.history.worldlines[1][0], 1);
    engine.remember_information(Color::White, 2);
    engine.reconcile().unwrap();
    assert_eq!(reacting_times(&engine, 0), [4]);
    for state in engine.history.worldlines[0].iter() {
        assert_eq!(
            state.remembered.contains(&target),
            state.t <= 2 && state.t % 2 == 0
        );
    }
    engine.remember_information(Color::White, 4);
    engine.reconcile().unwrap();
    assert!(reacting_times(&engine, 0).is_empty());
    for state in engine.history.worldlines[1].iter_mut() {
        state.x = 2;
    }
    engine.reconcile().unwrap();
    engine.remember_information(Color::White, 4);
    for state in engine.history.worldlines[1].iter_mut() {
        state.x = 0;
    }
    engine.reconcile().unwrap();
    assert!(
        reacting_times(&engine, 0).is_empty(),
        "an acknowledged target stays known after disappearing and returning"
    );
    assert!(engine.history.worldlines[0][0].remembered.contains(&target));
}

#[test]
fn move_remembers_the_view_before_its_spatial_and_temporal_edits() {
    let mut engine = observed(false);
    let target = PieceId::from_state(&engine.history.worldlines[1][0], 1);
    let mv = engine
        .possible_moves(0, 0)
        .unwrap()
        .into_iter()
        .find(|m| m.sq == Sq(1, 0) && m.direction == TimeDirection::Future)
        .unwrap();
    engine.apply_new_mv(mv).unwrap();
    assert!(engine.history.worldlines[0][0].remembered.contains(&target));
    assert!(engine.history.worldlines[0][2].remembered.is_empty());
    assert!(
        engine.history.worldlines[1]
            .iter()
            .all(|s| s.remembered.is_empty()),
        "never acknowledge the opponent's view"
    );
}

#[test]
fn declining_acknowledges_all_reactions_and_prevents_reactivation() {
    let mut engine = observed(false);
    assert!(engine.can_decline_reactions());
    engine.decline_reactions().unwrap();
    assert_eq!(engine.turn(), Color::Black);
    assert!(reacting_times(&engine, 0).is_empty());
    engine.turn = Color::White;
    engine.reconcile().unwrap();
    assert!(reacting_times(&engine, 0).is_empty());
    assert!(!engine.can_decline_reactions());
}

#[test]
fn free_wait_does_not_acknowledge_information() {
    let mut engine = observed(false);
    let mut waiting = line(PieceKind::N, Color::White, Sq(7, 0), false);
    waiting[0].rights.ordinary = true;
    waiting[4].rights.ordinary = true;
    engine.history.worldlines.push(waiting.into());
    engine.reconcile().unwrap();
    let source = Engine::piece_at(engine.get_piv(2, 0).unwrap());
    engine.skip_ordinary(source, 4).unwrap();
    assert_eq!(reacting_times(&engine, 0), [0]);
    assert!(engine.history.worldlines[0][0].remembered.is_empty());
    assert_eq!(engine.turn(), Color::White);
    assert!(engine.history.moves.is_empty());
}

#[test]
fn disappearing_en_passant_dynamically_retires_the_reaction() {
    for inverted in [false, true] {
        let mut pawn = State::new(PieceKind::P { moved: true }, Color::White, Sq(4, 4));
        pawn.t = 2;
        pawn.fut_seen = 4;
        pawn.rights = MoveRights::default();
        let mut victim = State::new(PieceKind::P { moved: false }, Color::Black, Sq(3, 6));
        victim.t = if inverted { 3 } else { 1 };
        victim.fut_seen = 4;
        victim.inverted = inverted;
        victim.loop_turn = usize::from(inverted);
        let mut arrived = victim.prolongate().unwrap();
        arrived.y = 4;
        arrived.kind = PieceKind::P { moved: true };
        let mut engine =
            Engine::from_worldlines(vec![vec![pawn], vec![victim, arrived]], Color::White).unwrap();
        assert_eq!(reacting_times(&engine, 0), [2]);
        engine.history.worldlines[1][0].y = 5; // No double step any more.
        engine.reconcile().unwrap();
        assert!(reacting_times(&engine, 0).is_empty());
        assert!(engine.history.worldlines[0][0].remembered.is_empty());
    }
}

#[test]
fn free_wait_can_remove_an_existing_reaction_without_consuming_a_turn() {
    let mut engine = observed(false);
    let mut blocker = line(PieceKind::N, Color::White, Sq(0, 2), false);
    blocker.truncate(1);
    blocker[0].rights.ordinary = true;
    engine.history.worldlines.push(blocker.into());
    engine.reconcile().unwrap();
    assert_eq!(reacting_times(&engine, 0), [2]);
    let source = Engine::piece_at(engine.get_piv(2, 0).unwrap());
    engine.skip_ordinary(source, 4).unwrap();
    assert!(reacting_times(&engine, 0).is_empty());
    assert!(
        engine.history.worldlines[0]
            .iter()
            .all(|s| s.informed.is_empty() && s.remembered.is_empty())
    );
    assert_eq!(engine.turn(), Color::White);
    assert!(engine.history.moves.is_empty());
}

#[test]
fn full_target_identity_distinguishes_future_knowledge_and_loop_turn() {
    for change_loop in [false, true] {
        let mut engine = observed(false);
        // Observe the same worldline with a different causal identity at t=2.
        let later = &mut engine.history.worldlines[1][2];
        if change_loop {
            later.loop_turn += 1;
        } else {
            later.fut_seen += 1;
        }
        engine.reconcile().unwrap();
        assert_eq!(reacting_times(&engine, 0), [0, 2]);
        engine.remember_information(Color::White, 0);
        engine.reconcile().unwrap();
        assert!(engine.history.worldlines[0][2].rights.new_information);
    }
}

#[test]
fn different_targets_can_activate_different_states_of_one_line() {
    let mut engine = observed(false);
    let mut later = line(PieceKind::N, Color::Black, Sq(2, 6), false);
    later[2].x = 0;
    later[2].y = 2; // Visible at t=2, obscuring the older target.
    engine.history.worldlines.push(later.into());
    engine.reconcile().unwrap();
    assert_eq!(reacting_times(&engine, 0), [0, 2]);
}

#[test]
fn continuing_paused_calculation_refreshes_both_sides_without_learning() {
    let mut source = State::new(PieceKind::N, Color::White, Sq(3, 3));
    source.t = 2;
    source.fut_seen = 4;
    let future = source.prolongate().unwrap();
    let mut observer = State::new(PieceKind::R { moved: true }, Color::Black, Sq(3, 7));
    observer.t = 3;
    observer.fut_seen = 4;
    observer.rights = MoveRights::default();
    let mut engine =
        Engine::from_worldlines(vec![vec![source, future], vec![observer]], Color::White).unwrap();
    assert_eq!(reacting_times(&engine, 1), [3]);
    let mv = engine
        .possible_moves(0, 0)
        .unwrap()
        .into_iter()
        .find(|m| !m.to_past() && m.sq == Sq(5, 4))
        .unwrap();
    engine.apply_move_inner(mv).unwrap();
    engine.reconcile().unwrap();
    assert!(reacting_times(&engine, 1).is_empty());
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
    assert!(reacting_times(&engine, 1).is_empty());
    engine.continue_calculation().unwrap();
    assert_eq!(reacting_times(&engine, 1), [3]);
    assert!(engine.history.worldlines[1][0].remembered.is_empty());
    assert_eq!(engine.turn(), Color::White);
    assert_eq!(
        Engine::from_history(engine.history.clone(), engine.turn).unwrap(),
        engine
    );
}
