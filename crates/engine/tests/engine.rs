use temporal_chess_engine::board::{Board, PreBoard};
use temporal_chess_engine::engine::{Engine, EngineErr};
use temporal_chess_engine::model::{Castling, Move, PieceId, State};
use temporal_chess_engine::pieces::{Color, PieceKind, PieceView, Sq};

fn play(engine: &mut Engine, from: Sq, to: Sq) {
    let t = engine.move_sources().map(|piece| piece.t).max().unwrap();
    let piece = *engine.board(t).unwrap().get_piece(from.0, from.1).unwrap();
    let mv = engine
        .possible_moves(piece.origin, piece.age)
        .unwrap()
        .into_iter()
        .find(|mv| mv.sq == to && !mv.to_past() && mv.promotion().is_none())
        .unwrap();
    engine.apply_new_mv(mv).unwrap();
}

fn view(kind: PieceKind, color: Color, square: Sq, origin: usize) -> PieceView {
    PieceView::from_state(&State::new(kind, color, square), origin, 0)
}

#[test]
fn starting_engine_is_ready_and_has_twenty_white_moves() {
    let engine = Engine::default();
    let board = engine.board(0).unwrap();
    assert_eq!(board.pieces().len(), 32);
    assert!(
        engine
            .move_sources()
            .all(|piece| piece.t == 0 && piece.color == Color::White)
    );
    let count: usize = board
        .pieces()
        .iter()
        .filter(|p| p.color == Color::White)
        .map(|p| engine.possible_moves(p.origin, p.age).unwrap().len())
        .sum();
    assert_eq!(count, 20);
    for piece in board.pieces() {
        assert_eq!(board.get_piece(piece.sq.0, piece.sq.1), Some(piece));
        assert_eq!(
            Board::default().get_piece(piece.sq.0, piece.sq.1),
            Some(piece)
        );
    }
}

fn stationary_line(kind: PieceKind, color: Color, sq: Sq, last_t: usize) -> Vec<State> {
    let mut line = vec![State::new(kind, color, sq)];
    for _ in 0..last_t {
        line.push(line.last().unwrap().prolongate().unwrap());
    }
    line
}

#[test]
fn a_later_ordinary_move_is_selectable_and_preserves_earlier_ordinary_moves() {
    let mut engine = Engine::from_worldlines(
        vec![
            stationary_line(PieceKind::N, Color::White, Sq(1, 0), 2),
            stationary_line(PieceKind::N, Color::Black, Sq(6, 7), 2),
        ],
        Color::White,
    )
    .unwrap();
    assert!(engine.move_sources().any(|p| p.t == 0));
    assert!(engine.move_sources().any(|p| p.t == 2));
    let mv = engine
        .possible_moves(0, 2)
        .unwrap()
        .into_iter()
        .find(|mv| !mv.to_past() && mv.sq == Sq(2, 2))
        .unwrap();
    engine.apply_new_mv(mv).unwrap();
    assert_eq!(engine.turn(), Color::Black);
    assert!(engine.get_piv(0, 0).unwrap().rights.ordinary);
    assert!(!engine.get_piv(0, 2).unwrap().rights.ordinary);
    play(&mut engine, Sq(6, 7), Sq(5, 5));
    assert!(engine.move_sources().any(|p| p.t == 0 && p.rights.ordinary));
    assert!(engine.move_sources().any(|p| p.t == 4 && p.rights.ordinary));
}

#[test]
fn move_expires_only_own_reactions_on_the_same_or_earlier_boards() {
    let mut reactor = stationary_line(PieceKind::P { moved: true }, Color::White, Sq(0, 1), 4);
    for state in &mut reactor {
        state.rights.ordinary = false;
        state.rights.new_information = state.t.is_multiple_of(2);
        state.rights.cancelled_move = state.t.is_multiple_of(2);
    }
    let mut opponent = stationary_line(PieceKind::N, Color::Black, Sq(6, 7), 4);
    opponent[1].rights.new_information = true;
    let mut engine = Engine::from_worldlines(
        vec![
            stationary_line(PieceKind::N, Color::White, Sq(1, 0), 4),
            reactor,
            opponent,
        ],
        Color::White,
    )
    .unwrap();
    let mv = engine
        .possible_moves(0, 2)
        .unwrap()
        .into_iter()
        .find(|mv| !mv.to_past() && mv.sq == Sq(2, 2))
        .unwrap();
    engine.apply_new_mv(mv).unwrap();
    for t in [0, 2] {
        assert!(!engine.get_piv(1, t).unwrap().rights.is_movable());
    }
    assert!(engine.get_piv(1, 4).unwrap().rights.is_reaction());
    assert!(engine.get_piv(0, 0).unwrap().rights.ordinary);
    assert!(engine.get_piv(0, 4).unwrap().rights.ordinary);
    assert!(engine.get_piv(2, 1).unwrap().rights.is_reaction());
}

#[test]
fn ordinary_turns_update_history_boards_and_side_together() {
    let mut engine = Engine::default();
    play(&mut engine, Sq(4, 1), Sq(4, 3));
    assert_eq!(engine.turn(), Color::Black);
    assert!(engine.move_sources().all(|piece| piece.t == 1));
    assert!(engine.board(0).unwrap().get_piece(4, 1).is_some());
    assert!(engine.board(1).unwrap().get_piece(4, 1).is_none());
    assert!(engine.board(1).unwrap().get_piece(4, 3).is_some());
    play(&mut engine, Sq(4, 6), Sq(4, 4));
    assert_eq!(engine.turn(), Color::White);
    assert_eq!(engine.history().moves().len(), 2);
    assert_eq!(
        Engine::from_history(engine.history().clone(), engine.turn()).unwrap(),
        engine
    );
}

#[test]
fn failed_move_preserves_the_complete_engine() {
    let mut engine = Engine::default();
    let snapshot = engine.clone();
    let mut mv = engine.possible_moves(8, 0).unwrap()[0];
    mv.sq = Sq(100, 100);
    assert_eq!(engine.apply_new_mv(mv), Err(EngineErr::WrongMove));
    assert_eq!(engine, snapshot);
    let mv = engine.possible_moves(24, 0).unwrap()[0];
    assert_eq!(engine.apply_new_mv(mv), Err(EngineErr::WrongTurn));
    assert_eq!(engine, snapshot);
}

#[test]
fn an_occupied_future_square_is_rejected_before_applying_a_move() {
    let first = State::new(PieceKind::R { moved: true }, Color::White, Sq(0, 0));
    let mut other = State::new(PieceKind::R { moved: true }, Color::White, Sq(7, 0));
    other.fut_seen = 5;
    let mut other_next = other.prolongate().unwrap();
    other_next.x = 0;
    other_next.y = 1;
    let black = State::new(PieceKind::K { moved: true }, Color::Black, Sq(4, 7));
    let black_next = black.prolongate().unwrap();
    let mut engine = Engine::from_worldlines(
        vec![
            vec![first],
            vec![other, other_next],
            vec![black, black_next],
        ],
        Color::White,
    )
    .unwrap();
    let snapshot = engine.clone();
    assert!(
        !engine
            .possible_moves(0, 0)
            .unwrap()
            .iter()
            .any(|mv| mv.sq == Sq(0, 1))
    );
    let mv = Move {
        pid: PieceId::from_piv(engine.get_piv(0, 0).unwrap()),
        t: 0,
        sq: Sq(0, 1),
        direction: temporal_chess_engine::model::TimeDirection::Future,
        effect: temporal_chess_engine::model::MoveEffect::Normal,
    };
    assert_eq!(engine.apply_new_mv(mv), Err(EngineErr::WrongMove));
    assert_eq!(engine, snapshot);
}

#[test]
fn inversion_from_an_ordinary_opening_creates_a_playable_past_incarnation() {
    let mut engine = Engine::default();
    play(&mut engine, Sq(4, 1), Sq(4, 3));
    play(&mut engine, Sq(4, 6), Sq(4, 4));
    let piece = *engine.board(2).unwrap().get_piece(6, 0).unwrap();
    let mv = engine
        .possible_moves(piece.origin, piece.age)
        .unwrap()
        .into_iter()
        .find(|mv| mv.to_past() && mv.sq == Sq(7, 2))
        .unwrap();
    engine.apply_new_mv(mv).unwrap();
    assert_eq!(engine.turn(), Color::Black);
    assert!(
        engine
            .board(3)
            .unwrap()
            .find_by_origin(piece.origin)
            .is_empty()
    );
    let past = engine.board(0).unwrap().get_piece(7, 2).unwrap();
    assert_eq!(
        (past.origin, past.loop_turn, past.fut_seen),
        (piece.origin, 1, 2)
    );
    assert!(past.inverted && past.rights.ordinary);
    // A real opponent move hands the turn to the new past incarnation.
    play(&mut engine, Sq(1, 7), Sq(2, 5));
    assert!(
        engine
            .move_sources()
            .any(|p| p.origin == piece.origin && p.t == 0 && p.inverted)
    );
}

#[test]
fn capture_kills_only_the_victims_future() {
    let mut engine = Engine::default();
    play(&mut engine, Sq(4, 1), Sq(4, 3));
    play(&mut engine, Sq(3, 6), Sq(3, 4));
    play(&mut engine, Sq(4, 3), Sq(3, 4));
    assert!(engine.get_piv(27, 2).is_some());
    assert!(engine.get_piv(27, 3).is_none());
    assert_eq!(engine.board(3).unwrap().get_piece(3, 4).unwrap().origin, 12);
}

#[test]
fn en_passant_keeps_the_victim_dead_on_its_empty_square() {
    let mut engine = Engine::default();
    play(&mut engine, Sq(4, 1), Sq(4, 3));
    play(&mut engine, Sq(0, 6), Sq(0, 5));
    play(&mut engine, Sq(4, 3), Sq(4, 4));
    play(&mut engine, Sq(3, 6), Sq(3, 4));
    play(&mut engine, Sq(4, 4), Sq(3, 5));
    assert!(engine.board(5).unwrap().get_piece(3, 4).is_none());
    assert_eq!(engine.board(5).unwrap().get_piece(3, 5).unwrap().origin, 12);
    assert!(engine.get_piv(27, 4).is_some());
    assert!(engine.get_piv(27, 5).is_none());
    assert_eq!(
        Engine::from_history(engine.history().clone(), engine.turn()).unwrap(),
        engine
    );
}

#[test]
fn castling_moves_the_actual_rook_without_fixed_worldline_numbers() {
    let mut engine = Engine::from_worldlines(
        vec![
            vec![State::new(
                PieceKind::K { moved: false },
                Color::White,
                Sq(4, 0),
            )],
            vec![State::new(
                PieceKind::K { moved: false },
                Color::Black,
                Sq(4, 7),
            )],
            vec![State::new(
                PieceKind::R { moved: false },
                Color::White,
                Sq(7, 0),
            )],
        ],
        Color::White,
    )
    .unwrap();
    let mv = engine
        .possible_moves(0, 0)
        .unwrap()
        .into_iter()
        .find(|mv| mv.castling() == Some(Castling::Short))
        .unwrap();
    engine.apply_new_mv(mv).unwrap();
    let board = engine.board(1).unwrap();
    assert_eq!(board.get_piece(6, 0).unwrap().origin, 0);
    assert_eq!(board.get_piece(5, 0).unwrap().origin, 2);
    assert!(board.get_piece(7, 0).is_none());
}

#[test]
fn king_cannot_move_into_attack_but_can_escape_check() {
    let engine = Engine::from_worldlines(
        vec![
            vec![State::new(
                PieceKind::K { moved: true },
                Color::White,
                Sq(4, 0),
            )],
            vec![State::new(
                PieceKind::R { moved: true },
                Color::Black,
                Sq(5, 7),
            )],
        ],
        Color::White,
    )
    .unwrap();
    let moves = engine.possible_moves(0, 0).unwrap();
    assert!(!moves.iter().any(|mv| mv.sq.0 == 5));
    let engine = Engine::from_worldlines(
        vec![
            vec![State::new(
                PieceKind::K { moved: true },
                Color::White,
                Sq(4, 0),
            )],
            vec![State::new(
                PieceKind::R { moved: true },
                Color::Black,
                Sq(4, 7),
            )],
        ],
        Color::White,
    )
    .unwrap();
    assert!(
        engine
            .possible_moves(0, 0)
            .unwrap()
            .iter()
            .any(|mv| mv.sq == Sq(3, 0))
    );
}

#[test]
fn promotion_is_required_and_has_four_choices() {
    let mut engine = Engine::from_worldlines(
        vec![vec![State::new(
            PieceKind::P { moved: true },
            Color::White,
            Sq(0, 6),
        )]],
        Color::White,
    )
    .unwrap();
    let moves = engine.possible_moves(0, 0).unwrap();
    assert_eq!(moves.len(), 4);
    assert!(moves.iter().all(|mv| mv.promotion().is_some()));
    let mv = *moves
        .iter()
        .find(|mv| mv.promotion() == Some(PieceKind::Q))
        .unwrap();
    engine.apply_new_mv(mv).unwrap();
    assert_eq!(
        engine.board(1).unwrap().get_piece(0, 7).unwrap().kind,
        PieceKind::Q
    );
}

#[test]
fn temporal_lookup_distinguishes_occurrences_with_the_same_piece_id() {
    let mut first = State::new(PieceKind::N, Color::White, Sq(0, 0));
    first.t = 4;
    first.fut_seen = 4;
    first.loop_turn = 1;
    first.inverted = true;
    first.rights.ordinary = false;
    let mut second = first.prolongate().unwrap();
    second.rights.ordinary = false;
    let mut third = second.prolongate().unwrap();
    third.rights.ordinary = false;
    let pid = PieceId::from_state(&first, 0);
    let engine = Engine::from_worldlines(vec![vec![first, second, third]], Color::White).unwrap();
    for (t, age) in [(4, 0), (3, 1), (2, 2)] {
        let mv = Move {
            pid,
            t,
            sq: Sq(1, 2),
            direction: temporal_chess_engine::model::TimeDirection::Past,
            effect: temporal_chess_engine::model::MoveEffect::Normal,
        };
        assert_eq!(engine.get_piv_from_mv(mv).unwrap().age, age);
    }
}

#[test]
fn malformed_history_is_rejected_without_panics() {
    let mut state = State::new(PieceKind::N, Color::White, Sq(8, 0));
    assert!(matches!(
        Engine::from_worldlines(vec![vec![state.clone()]], Color::White),
        Err(EngineErr::InvalidHistory(_))
    ));
    assert!(matches!(
        Engine::from_worldlines(vec![vec![]], Color::White),
        Err(EngineErr::InvalidHistory(_))
    ));
    state.x = 0;
    state.inverted = true;
    assert!(state.prolongate().is_none());
    assert!(Sq(usize::MAX, 0).shift(1, 0).is_none());

    state.t = 2;
    state.fut_seen = 2;
    let middle = state.prolongate().unwrap();
    let last = state.clone(); // Returns to the same t without a new causal identity.
    assert!(matches!(
        Engine::from_worldlines(
            vec![vec![
                state,
                middle,
                State {
                    inverted: false,
                    ..last
                }
            ]],
            Color::White
        ),
        Err(EngineErr::InvalidHistory(
            "inconsistent temporal transition"
        ))
    ));
}

#[test]
fn overflowing_time_is_a_rejected_command_without_mutation() {
    let mut state = State::new(PieceKind::N, Color::Black, Sq(1, 7));
    state.t = usize::MAX;
    state.fut_seen = usize::MAX;
    state.rights.ordinary = true;
    let mut engine = Engine::from_worldlines(vec![vec![state]], Color::Black).unwrap();
    let snapshot = engine.clone();
    let mv = engine
        .possible_moves(0, 0)
        .unwrap()
        .into_iter()
        .find(|mv| !mv.to_past())
        .unwrap();
    assert_eq!(engine.apply_new_mv(mv), Err(EngineErr::WrongMove));
    assert_eq!(engine, snapshot);
}

#[test]
fn board_removal_keeps_all_other_occupancy_entries_valid() {
    let mut board = Board::default();
    for square in [Sq(0, 0), Sq(3, 0), Sq(7, 6), Sq(2, 1)] {
        board.remove_piece(square);
        assert!(board.is_empty(square));
        for piece in board.pieces() {
            assert_eq!(board.get_piece(piece.sq.0, piece.sq.1), Some(piece));
        }
    }
}

#[test]
fn preboard_conversion_does_not_transpose_coordinates() {
    let mut preboard = PreBoard::new(0);
    let piece = view(PieceKind::N, Color::White, Sq(2, 5), 0);
    preboard.space[2][5].push(piece);
    let board = preboard.try_into_board().unwrap();
    assert_eq!(board.get_piece(2, 5), Some(&piece));
    assert!(board.get_piece(5, 2).is_none());
}

#[test]
fn present_blockers_stop_inverted_sliding_attacks() {
    let mut present = Board::empty(0);
    present.add_piece(view(PieceKind::N, Color::White, Sq(0, 3), 0));
    let mut future = Board::empty(1);
    let mut rook = view(PieceKind::R { moved: true }, Color::Black, Sq(0, 7), 1);
    rook.inverted = true;
    future.add_piece(rook);
    assert!(!present.is_inv_attacked(&future, Color::Black, Sq(0, 0)));
    present.remove_piece(Sq(0, 3));
    assert!(present.is_inv_attacked(&future, Color::Black, Sq(0, 0)));
}

#[test]
fn orthodox_opening_has_four_hundred_two_ply_sequences() {
    let initial = Engine::default();
    let mut leaves = 0;
    for piece in initial
        .board(0)
        .unwrap()
        .pieces()
        .iter()
        .filter(|p| p.color == Color::White)
    {
        for mv in initial.possible_moves(piece.origin, piece.age).unwrap() {
            let mut next = initial.clone();
            next.apply_new_mv(mv).unwrap();
            for opponent in next
                .board(1)
                .unwrap()
                .pieces()
                .iter()
                .filter(|p| p.color == Color::Black)
            {
                for reply in next.possible_moves(opponent.origin, opponent.age).unwrap() {
                    let mut leaf = next.clone();
                    leaf.apply_new_mv(reply).unwrap();
                    leaves += 1;
                }
            }
        }
    }
    assert_eq!(leaves, 400);
}
