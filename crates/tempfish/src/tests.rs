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
        for mv in engine.possible_moves(piece.origin, piece.age).unwrap() {
            for until in engine.possible_move_waits(mv).unwrap_or_default() {
                commands.push(Command::MoveAndWait {
                    mv,
                    from: piece.sq,
                    until,
                });
            }
        }
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
                wait_before_move: false,
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
        let engine = Engine::from_worldlines_with_rules(
            vec![line(Color::White, Sq(1, 0)), line(Color::Black, Sq(6, 7))],
            turn,
            GameRules {
                wait_before_move: true,
                ..Default::default()
            },
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

#[test]
fn generated_actions_include_inversion_promotion_decline_and_calculation() {
    fn generated(engine: Engine) -> Vec<Command> {
        let mut search = Search::new(engine, Limits::default());
        while !search.stack[0].ordered {
            search.step();
        }
        search.stack[0].commands.clone()
    }
    let (engine, _) = GameRecord::load("NOLAN-CHESS 2\ncausal_gaps off\nmove 0 12:0:0 e2 e4 + normal\nmove 1 30:1:0 g7 g6 + normal\n").unwrap();
    assert!(
        generated(engine)
            .iter()
            .any(|c| matches!(c, Command::Move { mv, .. } if mv.to_past()))
    );
    let engine = Engine::from_worldlines(
        vec![vec![State::new(
            PieceKind::P { moved: true },
            Color::White,
            Sq(0, 6),
        )]],
        Color::White,
    )
    .unwrap();
    assert_eq!(
        generated(engine)
            .iter()
            .filter(|c| matches!(c, Command::Move { mv, .. } if mv.promotion().is_some()))
            .count(),
        4
    );
    let mut rook = State::new(PieceKind::R { moved: true }, Color::White, Sq(0, 0));
    rook.rights.ordinary = false;
    let engine = Engine::from_worldlines(
        vec![
            vec![rook],
            vec![State::new(PieceKind::N, Color::Black, Sq(0, 4))],
        ],
        Color::White,
    )
    .unwrap();
    assert!(generated(engine).contains(&Command::DeclineReactions));
    let (engine, _) = GameRecord::load("NOLAN-CHESS 2\ncausal_gaps off\nmove 0 12:0:0 e2 e4 + normal\nmove 1 28:1:0 e7 e5 + normal\nmove 2 6:2:0 g1 f3 - normal\nmove 3 21:3:0 f8 b4 - normal\nmove 0 6:2:1 f3 e5 + normal\n").unwrap();
    assert!(generated(engine).contains(&Command::Continue));
}

#[test]
fn both_colors_can_complete_and_replay_a_short_bot_game() {
    let mut engine = Engine::default().with_interactive_calculation().unwrap();
    let mut record = GameRecord::default();
    let mut colors = [false; 2];
    for _ in 0..10 {
        if engine.outcome().is_some() {
            break;
        }
        colors[usize::from(engine.turn() == Color::Black)] = true;
        let decision = solve(&engine, MAX_DEPTH, 50);
        record
            .apply(
                &mut engine,
                decision
                    .command
                    .expect("bot must return an applicable command"),
            )
            .unwrap();
    }
    assert!(colors.into_iter().all(|seen| seen));
    assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, engine);
}

#[test]
fn time_war_search_uses_fixed_timeline_and_replayable_actions() {
    use temporal_chess_engine::model::StartingPosition;
    let mut engine = Engine::with_start(
        GameRules::default(),
        StartingPosition::TimeWar { separation: 5 },
    )
    .unwrap()
    .with_interactive_calculation()
    .unwrap();
    let mut record = GameRecord::default();
    for _ in 0..4 {
        let result = solve(&engine, 2, 50);
        assert_eq!(result.rejected, 0, "{:?}", result.first_error);
        record
            .apply(&mut engine, result.command.expect("Time War action"))
            .unwrap();
        assert_eq!(engine.history().horizon(), 5);
    }
    assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, engine);
}

#[test]
fn parallel_root_split_matches_exhaustive_with_free_turns_and_small_leases() {
    use crate::parallel::{Coordinator, Worker};
    for side in [Color::White, Color::Black] {
        let line = |color, sq| {
            let a = State::new(PieceKind::N, color, sq);
            let b = a.prolongate().unwrap();
            let c = b.prolongate().unwrap();
            vec![a, b, c]
        };
        let engine = Engine::from_worldlines(
            vec![line(Color::White, Sq(1, 0)), line(Color::Black, Sq(6, 7))],
            side,
        )
        .unwrap();
        let mut coordinator = Coordinator::new(
            engine.clone(),
            Limits {
                depth: 2,
                nodes: 100_000,
            },
        );
        while !coordinator.is_prepared() {
            coordinator.prepare_step();
        }
        let mut workers: Vec<_> = (0..3).map(|_| Worker::new(engine.clone())).collect();
        for _ in 0..100_000 {
            if coordinator.is_done() {
                break;
            }
            // Deliberately report out of order, like independently timed threads.
            let tasks: Vec<_> = (0..3)
                .filter_map(|i| coordinator.next_work(i).map(|w| (i, w)))
                .collect();
            for (id, work) in tasks.into_iter().rev() {
                let report = workers[id].run(work.clone(), || false);
                assert!(report.nodes <= work.nodes);
                coordinator.accept(id, report);
            }
        }
        assert!(coordinator.is_done());
        let result = coordinator.result();
        assert_eq!(result.completed_depth, 2);
        assert_eq!(result.score, exhaustive(&engine, 2, 0));
        assert!(result.command.is_some());
    }
}

#[test]
fn parallel_budget_is_global_and_one_node_still_returns_a_move() {
    for budget in [1, 20, 50, 200, 801] {
        let engine = Engine::default().with_interactive_calculation().unwrap();
        let result = crate::parallel::run(
            engine.clone(),
            Limits {
                nodes: budget,
                ..Default::default()
            },
            3,
            &std::sync::atomic::AtomicBool::new(false),
            |_| {},
        );
        assert!(result.nodes <= budget, "{} > {budget}", result.nodes);
        let mut next = engine;
        GameRecord::default()
            .apply(&mut next, result.command.unwrap())
            .unwrap();
    }
}

#[test]
fn single_clone_move_branch_matches_journal_and_preserves_root() {
    for text in [
        "NOLAN-CHESS 1",
        "NOLAN-CHESS 1\nmove 0 12:0:0 e2 e4 + normal\nmove 1 30:1:0 g7 g6 + normal\n",
        "NOLAN-CHESS 4\nchain_info_activation on\nglobal_reaction_reset on\nstart time_war 5\n",
    ] {
        let (engine, record) = GameRecord::load(text).unwrap();
        let saved = engine.clone();
        for command in commands(&engine) {
            let mut expected = engine.clone();
            let mut journal = record.clone();
            let result = journal.apply(&mut expected, command);
            match (result, successor(&engine, command)) {
                (Ok(()), Ok(actual)) => assert_eq!(actual, expected),
                (Err(a), Err(b)) => assert_eq!(a.to_string(), b),
                pair => panic!("{pair:?}"),
            }
            assert_eq!(engine, saved);
        }
    }
}

#[test]
fn rerooted_search_reuses_completed_depths_without_changing_scores() {
    let line = |color, sq| {
        let a = State::new(PieceKind::N, color, sq);
        let b = a.prolongate().unwrap();
        let c = b.prolongate().unwrap();
        vec![a, b, c]
    };
    let root = Engine::from_worldlines(
        vec![line(Color::White, Sq(1, 0)), line(Color::Black, Sq(6, 7))],
        Color::White,
    )
    .unwrap();
    let mut previous = Search::new(
        root.clone(),
        Limits {
            depth: 3,
            nodes: 100_000,
        },
    );
    while !previous.is_done() {
        previous.step();
    }
    assert_eq!(previous.completed_depth(), 3);
    let next = successor(&root, previous.result().command.unwrap()).unwrap();
    let cold = solve(&next, 2, 100_000);
    let mut warm = Search::with_cache(
        next.clone(),
        Limits {
            depth: 2,
            nodes: 100_000,
        },
        previous.take_cache(),
    );
    while !warm.is_done() {
        warm.step();
    }
    let warm = warm.result();
    assert_eq!(warm.completed_depth, cold.completed_depth);
    assert_eq!(warm.score, cold.score);
    assert_eq!(warm.score, exhaustive(&next, 2, 0));
    assert!(warm.cache_hits > 0);
    assert!(warm.nodes < cold.nodes, "warm={warm:?}; cold={cold:?}");
    assert_eq!(
        successor(&next, warm.command.unwrap())
            .map(|e| exhaustive(&e, 1, 1))
            .unwrap(),
        warm.score
    );
    eprintln!(
        "re-root: cold {} nodes, warm {} nodes, {} hits",
        cold.nodes, warm.nodes, warm.cache_hits
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn parallel_memory_survives_the_opponents_next_turn() {
    use crate::parallel::{Memory, run, run_with_memory};
    use std::sync::atomic::AtomicBool;
    let root = Engine::default();
    let stop = AtomicBool::new(false);
    let mut memory = Memory::default();
    let initial = run_with_memory(
        root.clone(),
        Limits {
            depth: 2,
            nodes: 100_000,
        },
        2,
        &stop,
        &mut memory,
        |_| {},
    );
    assert_eq!(initial.completed_depth, 2);
    let next = successor(&root, initial.command.unwrap()).unwrap();
    assert_ne!(root.turn(), next.turn());
    let limits = Limits {
        depth: 1,
        nodes: 100_000,
    };
    let warm = run_with_memory(next.clone(), limits, 2, &stop, &mut memory, |_| {});
    let cold = run(next.clone(), limits, 2, &stop, |_| {});
    assert_eq!(
        (warm.completed_depth, warm.score),
        (cold.completed_depth, cold.score)
    );
    assert!(
        warm.cache_hits > cold.cache_hits,
        "warm={warm:?}; cold={cold:?}"
    );
    assert!(successor(&next, warm.command.unwrap()).is_ok());
}

#[test]
fn worker_reset_keeps_finished_nodes_from_an_interrupted_branch() {
    use crate::parallel::{Work, Worker};
    let engine = Engine::default();
    let command = commands(&engine)[0];
    let next = successor(&engine, command).unwrap();
    let reply = commands(&next)[0];
    let mut worker = Worker::new(engine.clone());
    let interrupted = worker.run(
        Work {
            id: 1,
            command: command.to_string(),
            depth: 3,
            alpha: -INF,
            beta: INF,
            nodes: 8,
        },
        || false,
    );
    assert!(!interrupted.completed);
    // Reset also clears the old assignment: root coordinators reuse numeric IDs.
    worker.reset(next.clone());
    let report = worker.run(
        Work {
            id: 1,
            command: reply.to_string(),
            depth: 1,
            alpha: -INF,
            beta: INF,
            nodes: 128,
        },
        || false,
    );
    assert!(report.completed);
    assert_eq!(
        report.score,
        Some(evaluate(&successor(&next, reply).unwrap(), 1))
    );
    assert!(report.error.is_none());
}
