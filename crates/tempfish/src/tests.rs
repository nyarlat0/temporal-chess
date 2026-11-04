use super::*;
use temporal_chess_engine::model::{GameRules, Sq, State};

fn solve(engine: &Engine, depth: u8, nodes: u64) -> SearchResult {
    let mut search = Search::new(engine.clone(), Limits { depth, nodes });
    for _ in 0..500_000 {
        if search.is_done() {
            return search.result();
        }
        search.step();
    }
    panic!("search did not finish");
}
fn commands(engine: &Engine) -> Vec<Command> {
    let mut commands = Vec::new();
    for piece in engine.move_sources() {
        commands.extend(
            engine
                .possible_moves(piece.origin, piece.age)
                .unwrap()
                .into_iter()
                .map(|mv| Command::Move { mv, from: piece.sq }),
        );
        let source = PieceAt {
            piece: PieceId::from_piv(piece),
            t: piece.t,
        };
        if piece.rights.ordinary {
            commands.extend(engine.possible_skips(source).into_iter().map(|to_t| {
                Command::Advance {
                    source,
                    to_t,
                    from: piece.sq,
                }
            }));
        }
    }
    if engine.can_decline_reactions() {
        commands.push(Command::DeclineReactions);
    }
    if matches!(
        engine.calculation_status(),
        CalculationStatus::Paused { .. }
    ) {
        commands.push(Command::Continue);
    }
    commands
}
fn exhaustive(engine: &Engine, depth: u8, ply: u8) -> i32 {
    if depth == 0 || engine.outcome().is_some() {
        return evaluate(engine, ply);
    }
    let mut scores = vec![];
    for command in commands(engine) {
        let mut next = engine.clone();
        if GameRecord::new(next.history().rules())
            .apply(&mut next, command)
            .is_ok()
            && next != *engine
        {
            scores.push(exhaustive(&next, depth - 1, ply + 1));
        }
    }
    let best = if engine.turn() == Color::White {
        scores.into_iter().max()
    } else {
        scores.into_iter().min()
    };
    best.unwrap_or_else(|| evaluate(engine, ply))
}

#[test]
fn node_budget_is_bounded_and_selected_action_replays_in_every_ruleset() {
    for chain_info_activation in [false, true] {
        for global_reaction_reset in [false, true] {
            let rules = GameRules {
                chain_info_activation,
                global_reaction_reset,
            };
            let engine = Engine::new(rules).with_interactive_calculation().unwrap();
            let before = engine.clone();
            let result = solve(&engine, MAX_DEPTH, 50);
            assert!(result.nodes <= 50);
            assert!(result.completed_depth >= 1);
            assert_eq!(engine, before);
            let mut next = engine.clone();
            let mut record = GameRecord::new(rules);
            record.apply(&mut next, result.command.unwrap()).unwrap();
            assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, next);
        }
    }
}

#[test]
fn alpha_beta_matches_exhaustive_search_including_same_side_free_actions() {
    let line = |side, sq| {
        let start = State::new(PieceKind::N, side, sq);
        let middle = start.prolongate().unwrap();
        let last = middle.prolongate().unwrap();
        vec![start, middle, last]
    };
    for turn in [Color::White, Color::Black] {
        let engine = Engine::from_worldlines(
            vec![line(Color::White, Sq(1, 0)), line(Color::Black, Sq(6, 7))],
            turn,
        )
        .unwrap();
        if turn == Color::White {
            let free = commands(&engine)
                .into_iter()
                .find(|c| matches!(c, Command::Advance { .. }))
                .unwrap();
            let mut next = engine.clone();
            GameRecord::default().apply(&mut next, free).unwrap();
            assert_eq!(next.turn(), engine.turn());
        }
        let result = solve(&engine, 2, 100_000);
        assert_eq!(result.completed_depth, 2);
        assert_eq!(result.score, exhaustive(&engine, 2, 0));
        let mut next = engine.clone();
        GameRecord::default()
            .apply(&mut next, result.command.unwrap())
            .unwrap();
    }
}

#[test]
fn terminal_king_capture_is_preferred_and_finished_games_have_no_command() {
    let engine = Engine::from_worldlines(
        vec![
            vec![State::new(
                PieceKind::K { moved: false },
                Color::White,
                Sq(0, 0),
            )],
            vec![State::new(PieceKind::Q, Color::White, Sq(6, 6))],
            vec![State::new(
                PieceKind::K { moved: false },
                Color::Black,
                Sq(7, 7),
            )],
        ],
        Color::White,
    )
    .unwrap();
    let result = solve(&engine, 2, 1000);
    assert!(result.score >= MATE - 2);
    let mut next = engine;
    GameRecord::default()
        .apply(&mut next, result.command.unwrap())
        .unwrap();
    assert!(matches!(
        next.outcome(),
        Some(GameOutcome::Victory {
            winner: Color::White,
            ..
        })
    ));
    assert!(solve(&next, 2, 100).command.is_none());
}

#[test]
fn last_completed_iteration_is_retained_when_a_deeper_search_runs_out() {
    let engine = Engine::default();
    let first = solve(&engine, 1, 100_000);
    let interrupted = solve(&engine, 3, first.nodes + 1);
    assert_eq!(interrupted.completed_depth, 1);
    assert_eq!(interrupted.command, first.command);
    assert_eq!(interrupted.score, first.score);
    let one_node = solve(&engine, 3, 1);
    assert_eq!(one_node.nodes, 1);
    assert!(one_node.command.is_some());
}

#[test]
fn cancellation_returns_a_valid_partial_result() {
    let engine = Engine::default();
    let mut search = Search::new(engine.clone(), Limits::default());
    while search.result().command.is_none() {
        search.step();
    }
    search.finish_now();
    let result = search.result();
    search.step();
    assert!(search.is_done());
    assert_eq!(result.nodes, search.result().nodes);
    let mut next = engine;
    GameRecord::default()
        .apply(&mut next, result.command.unwrap())
        .unwrap();
}

#[test]
fn material_is_not_multiplied_by_passive_history_length() {
    let mut white = vec![State::new(PieceKind::N, Color::White, Sq(2, 2))];
    let mut black = vec![State::new(PieceKind::N, Color::Black, Sq(5, 5))];
    let initial =
        Engine::from_worldlines(vec![white.clone(), black.clone()], Color::White).unwrap();
    for _ in 0..10 {
        white.push(white.last().unwrap().prolongate().unwrap());
        black.push(black.last().unwrap().prolongate().unwrap());
    }
    let longer = Engine::from_worldlines(vec![white, black], Color::White).unwrap();
    assert!((evaluate(&longer, 0) - evaluate(&initial, 0)).abs() <= 12);
}
