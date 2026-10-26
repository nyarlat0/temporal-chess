use super::*;
use crate::model::{Color, PieceKind, Sq};

fn line(kind: PieceKind, color: Color, sq: Sq, end: usize) -> Vec<State> {
    let mut states = vec![State::new(kind, color, sq)];
    for _ in 0..end {
        states.push(states.last().unwrap().prolongate().unwrap());
    }
    for state in &mut states {
        state.rights = MoveRights::default();
    }
    states
}

fn play(engine: &mut Engine, wid: usize, t: usize, direction: TimeDirection, sq: Sq) -> Move {
    let piece = engine
        .move_sources()
        .find(|p| p.origin == wid && p.t == t)
        .unwrap();
    let mv = engine
        .possible_moves(wid, piece.age)
        .unwrap()
        .into_iter()
        .find(|mv| mv.direction == direction && mv.sq == sq)
        .unwrap();
    engine.apply_new_mv(mv).unwrap();
    mv
}

fn roundtrip(engine: &Engine) {
    assert_eq!(
        Engine::from_history(engine.history().clone(), engine.turn()).unwrap(),
        *engine
    );
}

fn stalemate_lines() -> Vec<Vec<State>> {
    let mut white = line(PieceKind::K { moved: true }, Color::White, Sq(0, 0), 4);
    white[4].rights.ordinary = true;
    vec![
        white,
        line(PieceKind::K { moved: true }, Color::Black, Sq(2, 2), 4),
        line(PieceKind::Q, Color::Black, Sq(1, 2), 4),
    ]
}

#[test]
fn stalemate_is_final_only_without_past_reserves_or_opponent_ordinary_debt() {
    let engine = Engine::from_worldlines(stalemate_lines(), Color::White).unwrap();
    assert_eq!(engine.outcome(), Some(GameOutcome::Stalemate));
    assert!(!engine.checks(Color::White).unwrap().is_check());
    assert_eq!(engine.move_sources().count(), 0);
    roundtrip(&engine);

    let mut lines = stalemate_lines();
    let mut reserve = line(PieceKind::P { moved: true }, Color::White, Sq(7, 7), 4);
    reserve[0].rights.ordinary = true; // No spatial pawn moves, but can wait.
    lines.push(reserve);
    let engine = Engine::from_worldlines(lines, Color::White).unwrap();
    assert_eq!(engine.status(), GameStatus::Ongoing);
    assert!(engine.possible_moves(3, 0).unwrap().is_empty());

    let mut lines = stalemate_lines();
    let mut reactive = line(PieceKind::N, Color::Black, Sq(6, 7), 4);
    reactive[1].rights.new_information = true;
    lines.push(reactive);
    let engine = Engine::from_worldlines(lines, Color::White).unwrap();
    assert_eq!(engine.outcome(), Some(GameOutcome::Stalemate));
}

#[test]
fn blocked_player_is_rechecked_after_each_opponent_move() {
    let mut lines = stalemate_lines();
    let mut debt = line(PieceKind::N, Color::Black, Sq(6, 7), 4);
    debt[1].rights.ordinary = true;
    debt[3].rights.ordinary = true;
    lines.push(debt);
    let mut engine = Engine::from_worldlines(lines, Color::White).unwrap();
    let waiting = GameStatus::Waiting {
        player: Color::White,
        reason: WaitingReason::OpponentPastMoves,
    };
    assert_eq!(engine.status(), waiting);
    assert_eq!(engine.turn(), Color::Black);
    roundtrip(&engine);
    play(&mut engine, 3, 1, TimeDirection::Future, Sq(4, 6));
    assert_eq!(engine.status(), waiting);
    assert_eq!(engine.turn(), Color::Black);
    assert_eq!(engine.history.horizon(), 4);
    play(&mut engine, 3, 3, TimeDirection::Future, Sq(2, 5));
    assert_eq!(engine.outcome(), Some(GameOutcome::Stalemate));
    roundtrip(&engine);
}

#[test]
fn an_opponent_move_can_reactivate_the_waiting_player() {
    let mut lines = stalemate_lines();
    lines.push(line(PieceKind::N, Color::White, Sq(7, 0), 4));
    let mut debt = line(PieceKind::N, Color::Black, Sq(4, 3), 4);
    debt[1].rights.ordinary = true;
    lines.push(debt);
    let mut engine = Engine::from_worldlines(lines, Color::White).unwrap();
    assert!(matches!(engine.status(), GameStatus::Waiting { .. }));
    play(&mut engine, 4, 1, TimeDirection::Future, Sq(6, 2));
    assert_eq!(engine.status(), GameStatus::Ongoing);
    assert_eq!(engine.turn(), Color::White);
    assert!(
        engine
            .move_sources()
            .any(|p| p.origin == 3 && p.rights.new_information)
    );
    roundtrip(&engine);
}

#[test]
fn free_wait_can_close_the_last_opponent_debt_without_a_spatial_move() {
    let mut lines = stalemate_lines();
    let mut debt = line(PieceKind::N, Color::Black, Sq(6, 7), 4);
    debt[1].rights.ordinary = true;
    debt[3].rights.ordinary = true;
    lines.push(debt);
    let mut engine = Engine::from_worldlines(lines, Color::White).unwrap();
    let source = Engine::piece_at(engine.get_piv(3, 1).unwrap());
    engine.skip_ordinary(source, 4).unwrap();
    assert_eq!(engine.turn(), Color::Black);
    assert!(engine.history.moves().is_empty());
    assert_eq!(engine.outcome(), Some(GameOutcome::Stalemate));
    roundtrip(&engine);
}

#[test]
fn free_wait_is_piece_local_preserves_unmoved_flags_and_has_no_turn_cost() {
    let mut pawn = line(PieceKind::P { moved: false }, Color::White, Sq(0, 1), 4);
    for t in [0, 2, 4] {
        pawn[t].rights.ordinary = true;
    }
    let mut knight = line(PieceKind::N, Color::White, Sq(1, 0), 4);
    knight[0].rights.ordinary = true;
    knight[0].rights.new_information = true;
    let mut engine = Engine::from_worldlines(vec![pawn, knight], Color::White).unwrap();
    let source = Engine::piece_at(engine.get_piv(0, 0).unwrap());
    engine.skip_ordinary(source, 4).unwrap();
    assert_eq!(engine.turn(), Color::White);
    assert_eq!(engine.history.horizon(), 4);
    assert!(engine.history.moves().is_empty());
    for t in [0, 2] {
        assert!(!engine.get_piv(0, t).unwrap().rights.ordinary);
    }
    assert!(engine.get_piv(0, 4).unwrap().rights.ordinary);
    assert!(engine.get_piv(1, 0).unwrap().rights.ordinary);
    assert!(engine.get_piv(1, 0).unwrap().rights.new_information);
    assert!(
        engine.history.worldlines[0]
            .iter()
            .all(|s| s.kind == PieceKind::P { moved: false })
    );
    roundtrip(&engine);
}

#[test]
fn invalid_waits_are_atomic_and_reactions_alone_cannot_wait() {
    let mut knight = line(PieceKind::N, Color::White, Sq(1, 0), 4);
    knight[2].rights.ordinary = true;
    knight[0].rights.new_information = true;
    let mut engine = Engine::from_worldlines(vec![knight], Color::White).unwrap();
    let source = Engine::piece_at(engine.get_piv(0, 2).unwrap());
    let before = engine.clone();
    for to in [0, 5, usize::MAX] {
        assert_eq!(engine.skip_ordinary(source, to), Err(EngineErr::WrongMove));
        assert_eq!(engine, before);
    }
    let reaction = Engine::piece_at(engine.get_piv(0, 0).unwrap());
    assert_eq!(
        engine.skip_ordinary(reaction, 2),
        Err(EngineErr::WrongPiece)
    );
    assert_eq!(engine, before);
}

#[test]
fn ordinary_royal_capture_finishes_and_locks_the_game() {
    let mut rook = line(PieceKind::R { moved: true }, Color::White, Sq(0, 0), 0);
    rook[0].rights.ordinary = true;
    let mut engine = Engine::from_worldlines(
        vec![
            rook,
            line(PieceKind::K { moved: true }, Color::White, Sq(7, 0), 0),
            line(PieceKind::K { moved: true }, Color::Black, Sq(0, 7), 0),
        ],
        Color::White,
    )
    .unwrap();
    let source = Engine::piece_at(engine.get_piv(0, 0).unwrap());
    let mv = play(&mut engine, 0, 0, TimeDirection::Future, Sq(0, 7));
    assert_eq!(
        engine.outcome(),
        Some(GameOutcome::Victory {
            winner: Color::White,
            reason: WinReason::KingCaptured
        })
    );
    let before = engine.clone();
    assert_eq!(engine.apply_new_mv(mv), Err(EngineErr::GameOver));
    assert_eq!(engine.skip_ordinary(source, 0), Err(EngineErr::GameOver));
    assert_eq!(engine, before);
    roundtrip(&engine);
}

#[test]
fn a_noninverted_enemy_can_capture_a_past_king_even_after_it_moved_away() {
    let mut king = line(PieceKind::K { moved: true }, Color::White, Sq(0, 0), 2);
    king[2].rights.ordinary = true;
    let rook = line(PieceKind::R { moved: true }, Color::Black, Sq(0, 7), 2);
    let mut engine = Engine::from_worldlines(
        vec![
            king,
            line(PieceKind::K { moved: true }, Color::Black, Sq(7, 7), 2),
            rook,
        ],
        Color::White,
    )
    .unwrap();
    play(&mut engine, 0, 2, TimeDirection::Future, Sq(1, 0));
    assert_eq!(engine.turn(), Color::Black);
    assert_eq!(engine.get_piv(0, 3).unwrap().sq, Sq(1, 0));
    assert!(!engine.get_piv(2, 3).unwrap().inverted);
    assert!(
        engine
            .checks(Color::White)
            .unwrap()
            .inverted
            .iter()
            .any(|id| id.t == 2)
    );
    assert!(engine.outcome().is_none());
    play(&mut engine, 2, 3, TimeDirection::Past, Sq(0, 0));
    assert_eq!(
        engine.outcome(),
        Some(GameOutcome::Victory {
            winner: Color::Black,
            reason: WinReason::KingCaptured
        })
    );
    roundtrip(&engine);
}

#[test]
fn inverted_check_uses_past_blockers_not_the_future_interposition() {
    for block_in_past in [false, true] {
        let mut rook = line(PieceKind::R { moved: true }, Color::Black, Sq(0, 7), 3);
        rook[3].rights.ordinary = true;
        let mut blocker = line(PieceKind::B, Color::White, Sq(1, 3), 3);
        blocker[3].x = 0;
        if block_in_past {
            blocker[2].x = 0;
        }
        let engine = Engine::from_worldlines(
            vec![
                line(PieceKind::K { moved: true }, Color::White, Sq(0, 0), 3),
                line(PieceKind::K { moved: true }, Color::Black, Sq(7, 7), 3),
                rook,
                blocker,
            ],
            Color::Black,
        )
        .unwrap();
        assert_eq!(
            engine
                .checks(Color::White)
                .unwrap()
                .inverted
                .iter()
                .any(|id| id.t == 2),
            !block_in_past
        );
    }
}

#[test]
fn paradox_is_a_committed_loss_but_other_failures_roll_back() {
    let mut engine = Engine::default();
    let before = engine.clone();
    assert_eq!(
        engine.transaction(|candidate| {
            candidate.history.worldlines[0][0].x = 4;
            Err(EngineErr::WrongMove)
        }),
        Err(EngineErr::WrongMove)
    );
    assert_eq!(engine, before);
    engine
        .transaction(|candidate| {
            candidate.history.worldlines[0][0].x = 4;
            Err(EngineErr::InfLoop)
        })
        .unwrap();
    assert_eq!(
        engine.outcome(),
        Some(GameOutcome::Victory {
            winner: Color::Black,
            reason: WinReason::Paradox
        })
    );
    assert_eq!(engine.history.worldlines, before.history.worldlines);
    assert_eq!(engine.projection, before.projection);
    roundtrip(&engine);
}

#[test]
fn no_automatic_mate_a_checked_blocked_king_must_actually_be_captured() {
    let mut lines = stalemate_lines();
    for state in &mut lines[2] {
        state.y = 1;
    }
    let mut engine = Engine::from_worldlines(lines, Color::White).unwrap();
    assert_eq!(engine.outcome(), None);
    assert_eq!(
        engine.status(),
        GameStatus::Waiting {
            player: Color::White,
            reason: WaitingReason::KingUnderAttack
        }
    );
    assert_eq!(engine.turn(), Color::Black);
    assert_eq!(engine.history.horizon(), 5); // One forced, passive frontier turn.
    roundtrip(&engine);
    play(&mut engine, 2, 5, TimeDirection::Past, Sq(0, 0));
    assert_eq!(
        engine.outcome(),
        Some(GameOutcome::Victory {
            winner: Color::Black,
            reason: WinReason::KingCaptured
        })
    );
}

#[test]
fn capturing_the_future_attacker_removes_its_inverted_check() {
    let mut rook = line(PieceKind::R { moved: true }, Color::White, Sq(7, 7), 3);
    rook[2].rights.ordinary = true;
    let mut enemy = line(PieceKind::R { moved: true }, Color::Black, Sq(0, 7), 3);
    enemy[3].rights.ordinary = true;
    let mut engine = Engine::from_worldlines(
        vec![
            line(PieceKind::K { moved: true }, Color::White, Sq(0, 0), 3),
            line(PieceKind::K { moved: true }, Color::Black, Sq(6, 5), 3),
            rook,
            enemy,
        ],
        Color::White,
    )
    .unwrap();
    assert!(
        engine
            .checks(Color::White)
            .unwrap()
            .inverted
            .iter()
            .any(|p| p.t == 2)
    );
    play(&mut engine, 2, 2, TimeDirection::Future, Sq(0, 7));
    assert!(engine.outcome().is_none());
    assert!(
        !engine
            .checks(Color::White)
            .unwrap()
            .inverted
            .iter()
            .any(|p| p.t == 2)
    );
    roundtrip(&engine);
}

#[test]
fn an_inverted_wait_reaches_zero_and_can_relinquish_its_boundary_right() {
    let mut first = State::new(PieceKind::P { moved: false }, Color::White, Sq(0, 1));
    first.t = 4;
    first.fut_seen = 4;
    first.loop_turn = 1;
    first.loop_pivots = vec![4];
    first.inverted = true;
    let second = first.prolongate().unwrap();
    let third = second.prolongate().unwrap();
    let mut engine =
        Engine::from_worldlines(vec![vec![first, second, third]], Color::White).unwrap();
    let source = Engine::piece_at(engine.get_piv(0, 2).unwrap());
    engine.skip_ordinary(source, 0).unwrap();
    let endpoint = engine.get_piv(0, 4).unwrap();
    assert_eq!((endpoint.t, endpoint.loop_turn), (0, 1));
    assert!(endpoint.inverted && endpoint.rights.ordinary);
    assert_eq!(endpoint.kind, PieceKind::P { moved: false });
    assert_eq!(engine.turn(), Color::White);
    assert_eq!(engine.history.horizon(), 4);
    engine.skip_ordinary(Engine::piece_at(endpoint), 0).unwrap();
    assert!(!engine.get_piv(0, 4).unwrap().rights.ordinary);
    roundtrip(&engine);
}

#[test]
fn waiting_across_a_pivot_closes_the_gap_without_a_new_spatial_move() {
    let mut knight = line(PieceKind::N, Color::White, Sq(1, 0), 6);
    for t in [0, 2, 4, 6] {
        knight[t].rights.ordinary = true;
    }
    let mut engine = Engine::from_worldlines(vec![knight], Color::White).unwrap();
    play(&mut engine, 0, 4, TimeDirection::Past, Sq(2, 2));
    engine.turn = Color::White;
    let source = engine
        .move_sources()
        .find(|p| p.t == 2 && p.inverted)
        .unwrap();
    let mv = engine
        .possible_moves(0, source.age)
        .unwrap()
        .into_iter()
        .find(|mv| mv.direction == TimeDirection::Future && mv.sq == Sq(4, 3))
        .unwrap();
    engine.apply_new_mv(mv).unwrap();
    engine.turn = Color::White;
    let source = engine
        .move_sources()
        .find(|p| p.t == 4 && p.loop_turn == 2)
        .unwrap();
    let journal = engine.history.moves.clone();
    engine.skip_ordinary(Engine::piece_at(source), 6).unwrap();
    assert!(engine.history.worldlines[0].gaps().is_empty());
    assert_eq!(engine.history.worldlines[0].last().unwrap().loop_turn, 0);
    assert_eq!(engine.history.moves, journal);
    assert_eq!(engine.turn(), Color::White);
    roundtrip(&engine);
}

#[test]
fn self_annihilation_validation_cycle_loses_for_the_command_author() {
    let mut rook = line(PieceKind::R { moved: true }, Color::White, Sq(0, 0), 8);
    for t in [0, 2, 4, 6, 8] {
        rook[t].rights.ordinary = true;
    }
    let mut engine = Engine::from_worldlines(vec![rook], Color::White).unwrap();
    play(&mut engine, 0, 2, TimeDirection::Future, Sq(0, 2));
    engine.turn = Color::White;
    play(&mut engine, 0, 4, TimeDirection::Future, Sq(2, 2));
    engine.turn = Color::White;
    play(&mut engine, 0, 6, TimeDirection::Past, Sq(2, 0));
    engine.turn = Color::White;
    let source = engine
        .move_sources()
        .find(|p| p.t == 4 && p.inverted)
        .unwrap();
    let mv = engine
        .possible_moves(0, source.age)
        .unwrap()
        .into_iter()
        .find(|mv| mv.to_past() && mv.sq == Sq(0, 0))
        .unwrap();
    let before = engine.clone();
    let mut probe = before.clone();
    probe.apply_move_inner(mv).unwrap();
    probe.turn = Color::Black;
    assert_eq!(probe.reconcile(), Err(EngineErr::InfLoop));
    engine.apply_new_mv(mv).unwrap();
    assert_eq!(
        engine.outcome(),
        Some(GameOutcome::Victory {
            winner: Color::Black,
            reason: WinReason::Paradox
        })
    );
    assert_eq!(engine.history.worldlines, before.history.worldlines);
    assert_eq!(engine.history.moves, before.history.moves);
    assert_eq!(engine.projection, before.projection);
    roundtrip(&engine);
}

#[test]
fn waiting_cannot_overwrite_a_later_recorded_move_and_rolls_back_consumed_rights() {
    let mut knight = line(PieceKind::N, Color::White, Sq(1, 0), 4);
    for t in [0, 2, 4] {
        knight[t].rights.ordinary = true;
    }
    let mut engine = Engine::from_worldlines(vec![knight], Color::White).unwrap();
    play(&mut engine, 0, 2, TimeDirection::Future, Sq(2, 2));
    engine.turn = Color::White;
    let before = engine.clone();
    let source = Engine::piece_at(engine.get_piv(0, 0).unwrap());
    assert_eq!(engine.skip_ordinary(source, 4), Err(EngineErr::WrongMove));
    assert_eq!(engine, before);
    engine.turn = Color::Black;
    let before = engine.clone();
    assert_eq!(engine.skip_ordinary(source, 0), Err(EngineErr::WrongTurn));
    assert_eq!(engine, before);
}

#[test]
fn an_inverted_wait_inserts_before_the_borrowed_future_without_changing_it() {
    let mut knight = line(PieceKind::N, Color::White, Sq(1, 0), 8);
    for t in [0, 2, 4, 6, 8] {
        knight[t].rights.ordinary = true;
    }
    let mut engine = Engine::from_worldlines(vec![knight], Color::White).unwrap();
    let tail = engine.history.worldlines[0][7..].to_vec();
    play(&mut engine, 0, 6, TimeDirection::Past, Sq(2, 2));
    engine.turn = Color::White;
    let source = engine
        .move_sources()
        .find(|p| p.t == 4 && p.inverted)
        .unwrap();
    engine.skip_ordinary(Engine::piece_at(source), 2).unwrap();
    let line = &engine.history.worldlines[0];
    assert_eq!(&line[11..], tail);
    assert_eq!(line[10].t, 2);
    assert_eq!(line[10].loop_turn, 1);
    assert_eq!(line.gaps()[0].after, MoveId::from_state(&line[10]));
    roundtrip(&engine);
}
