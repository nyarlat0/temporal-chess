use super::*;
use crate::{
    model::{Color, GameRules, Observation, PieceKind, PieceView, Sq, StartingPosition},
    record::{Command, GameRecord},
};

#[test]
fn observation_ignores_identity_and_moved_flags_but_keeps_square_kind_and_orientation() {
    let first = State::new(PieceKind::R { moved: false }, Color::Black, Sq(2, 3));
    let mut other = first.clone();
    other.kind = PieceKind::R { moved: true };
    other.fut_seen = 20;
    other.loop_turn = 4;
    assert_eq!(
        Observation::from_piv(PieceView::from_state(&first, 0, 0)),
        Observation::from_piv(PieceView::from_state(&other, 9, 25))
    );
    for (x, kind, inverted) in [
        (3, other.kind, false),
        (2, PieceKind::B, false),
        (2, other.kind, true),
    ] {
        let mut changed = other.clone();
        changed.x = x;
        changed.kind = kind;
        changed.inverted = inverted;
        assert_ne!(
            Observation::from_state(&first),
            Observation::from_state(&changed)
        );
    }
}

#[test]
fn same_enemy_appearance_with_different_future_ids_reacts_once_or_in_chain() {
    for chain in [false, true] {
        let mut observer = vec![State::new(
            PieceKind::R { moved: true },
            Color::White,
            Sq(0, 0),
        )];
        let mut enemy = vec![State::new(PieceKind::N, Color::Black, Sq(0, 4))];
        for _ in 0..4 {
            observer.push(observer.last().unwrap().prolongate().unwrap());
            enemy.push(enemy.last().unwrap().prolongate().unwrap());
        }
        for state in &mut observer {
            state.rights = Default::default();
        }
        let engine = Engine::from_worldlines_with_rules(
            vec![observer, enemy],
            Color::White,
            GameRules {
                chain_info_activation: chain,
                ..Default::default()
            },
        )
        .unwrap();
        let reactive: Vec<_> = engine.history.worldlines[0]
            .iter()
            .filter(|s| s.rights.new_information)
            .map(|s| s.t)
            .collect();
        assert_eq!(reactive, if chain { vec![0, 2] } else { vec![0] });
    }
}

#[test]
fn paid_and_free_standalone_waits_replay_and_paid_wait_is_undoable() {
    for free in [false, true] {
        let rules = GameRules {
            wait_before_move: free,
            ..Default::default()
        };
        let start = StartingPosition::TimeWar { separation: 7 };
        let mut engine = Engine::with_start(rules, start)
            .unwrap()
            .with_interactive_calculation()
            .unwrap();
        let initial = engine.clone();
        let source = Engine::piece_at(engine.get_piv(1, 0).unwrap());
        assert!(engine.possible_skips(source).contains(&2));
        let mut record = GameRecord::with_start(rules, start);
        let command = Command::Advance {
            source,
            from: Sq(1, 0),
            to_t: 2,
        };
        record.apply(&mut engine, command).unwrap();
        assert_eq!(
            engine.turn(),
            if free { Color::White } else { Color::Black }
        );
        assert!(engine.history.moves.is_empty());
        assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, engine);
        if free {
            assert!(record.without_last_move().is_none());
        } else {
            let (prefix, removed) = record.without_last_move().unwrap();
            assert_eq!(removed, command);
            assert_eq!(GameRecord::load(&prefix.to_text()).unwrap().0, initial);
        }
    }
    assert!(GameRules::default().global_reaction_reset);
    let text = GameRecord::default().to_text();
    assert!(text.contains("react_multiple off"));
    assert!(text.contains("free_wait off"));
}

#[test]
fn loop_exit_always_waits_and_cannot_be_reactivated_for_a_spatial_move() {
    for free in [false, true] {
        let mut knight = vec![State::new(PieceKind::N, Color::White, Sq(1, 0))];
        let mut enemy = vec![State::new(
            PieceKind::R { moved: true },
            Color::Black,
            Sq(7, 7),
        )];
        for _ in 0..4 {
            knight.push(knight.last().unwrap().prolongate().unwrap());
        }
        for _ in 0..8 {
            enemy.push(enemy.last().unwrap().prolongate().unwrap());
        }
        let mut engine = Engine::from_worldlines_with_rules(
            vec![knight, enemy],
            Color::White,
            GameRules {
                wait_before_move: free,
                ..Default::default()
            },
        )
        .unwrap();
        let departure = engine
            .possible_moves(0, 4)
            .unwrap()
            .into_iter()
            .find(|m| m.to_past() && m.sq == Sq(2, 2))
            .unwrap();
        engine.apply_new_mv(departure).unwrap();
        engine.turn = Color::White;
        let age = engine.history.worldlines[0].len() - 1;
        let returning = engine
            .possible_moves(0, age)
            .unwrap()
            .into_iter()
            .find(|m| !m.to_past() && m.sq == Sq(1, 4))
            .unwrap();
        assert!(engine.possible_move_waits(returning).unwrap().contains(&6));
        let waited = engine.after_move_with_wait(returning, 6).unwrap();
        engine.apply_new_mv(returning).unwrap();
        assert_eq!(waited, engine);
        let exit = engine.history.worldlines[0]
            .iter()
            .position(State::is_loop_exit)
            .unwrap();
        assert_eq!(engine.history.worldlines[0][exit].t, 4);
        assert!(!engine.history.worldlines[0][exit].rights.is_movable());
        assert_eq!(engine.history.worldlines[0].last().unwrap().t, 6);
        engine.history.worldlines[0][exit].rights.cancelled_move = true;
        engine.history.worldlines[0][exit].rights.ordinary = true;
        assert!(engine.possible_moves(0, exit).unwrap().is_empty());
        engine.reconcile().unwrap();
        assert!(!engine.history.worldlines[0][exit].rights.is_movable());
    }
}

#[test]
fn immobile_cancelled_reaction_is_removed_but_mobile_reaction_survives() {
    for kind in [PieceKind::P { moved: true }, PieceKind::N] {
        let mut state = State::new(kind, Color::White, Sq(0, 7));
        state.rights.ordinary = false;
        state.rights.cancelled_move = true;
        let engine = Engine::from_worldlines(vec![vec![state]], Color::White).unwrap();
        assert_eq!(
            engine.history.worldlines[0][0].rights.cancelled_move,
            kind == PieceKind::N
        );
    }
}

#[test]
fn paid_wait_resets_reactions_according_to_react_multiple_and_free_wait_does_not() {
    for free in [false, true] {
        for global in [false, true] {
            let mut lines = Vec::new();
            for (kind, color, sq) in [
                (PieceKind::N, Color::White, Sq(7, 0)),
                (PieceKind::R { moved: true }, Color::White, Sq(0, 0)),
                (PieceKind::N, Color::Black, Sq(0, 4)),
            ] {
                let mut states = vec![State::new(kind, color, sq)];
                for _ in 0..6 {
                    states.push(states.last().unwrap().prolongate().unwrap());
                }
                if color == Color::White && sq == Sq(0, 0) {
                    for s in &mut states {
                        s.rights = Default::default();
                    }
                }
                lines.push(states);
            }
            let mut engine = Engine::from_worldlines_with_rules(
                lines,
                Color::White,
                GameRules {
                    wait_before_move: free,
                    global_reaction_reset: global,
                    chain_info_activation: true,
                },
            )
            .unwrap();
            assert!(engine.history.worldlines[1][4].rights.new_information);
            let source = Engine::piece_at(engine.get_piv(0, 0).unwrap());
            engine.skip_ordinary(source, 2).unwrap();
            assert_eq!(
                engine.history.worldlines[1][4].rights.new_information,
                free || !global
            );
            assert_eq!(
                engine.history.worldlines[1][4].remembered.is_empty(),
                free || !global
            );
        }
    }
}

#[test]
fn nested_loop_exits_wait_through_both_pivots_even_in_free_wait_mode() {
    let mut exit = State::new(PieceKind::N, Color::White, Sq(1, 0));
    exit.t = 4;
    exit.fut_seen = 6;
    exit.loop_turn = 4;
    exit.loop_pivots = vec![6, 4];
    let mut other = vec![State::new(PieceKind::N, Color::Black, Sq(7, 7))];
    for _ in 0..8 {
        other.push(other.last().unwrap().prolongate().unwrap());
    }
    let engine = Engine::from_worldlines_with_rules(
        vec![vec![exit], other],
        Color::White,
        GameRules {
            wait_before_move: true,
            ..Default::default()
        },
    )
    .unwrap();
    let line = &engine.history.worldlines[0];
    assert_eq!(line.last().unwrap().t, 8);
    assert_eq!(line.last().unwrap().loop_turn, 0);
    assert!(
        line.iter()
            .filter(|s| s.t == 4 || s.t == 6)
            .all(|s| !s.rights.is_movable())
    );
}
