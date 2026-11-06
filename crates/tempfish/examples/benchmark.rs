use std::time::Instant;
use tempfish::{Limits, Search};
use temporal_chess_engine::{
    engine::Engine,
    model::{GameRules, StartingPosition},
    record::GameRecord,
};
fn main() {
    let nodes = std::env::args()
        .nth(1)
        .and_then(|n| n.parse().ok())
        .unwrap_or(800);
    let positions = [
        (
            "start",
            Engine::default().with_interactive_calculation().unwrap(),
        ),
        (
            "opening",
            GameRecord::load(
                "NOLAN-CHESS 1\nmove 0 12:0:0 e2 e4 + normal\nmove 1 30:1:0 g7 g6 + normal\n",
            )
            .unwrap()
            .0,
        ),
        (
            "time-war",
            Engine::with_start(
                GameRules::default(),
                StartingPosition::TimeWar { separation: 5 },
            )
            .unwrap()
            .with_interactive_calculation()
            .unwrap(),
        ),
    ];
    let threads = std::env::args()
        .nth(2)
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(0);
    for (name, engine) in positions {
        let start = Instant::now();
        let result = if threads > 0 {
            tempfish::parallel::run(
                engine,
                Limits {
                    nodes,
                    ..Default::default()
                },
                threads,
                &std::sync::atomic::AtomicBool::new(false),
                |_| {},
            )
        } else {
            let mut search = Search::new(
                engine,
                Limits {
                    nodes,
                    ..Default::default()
                },
            );
            while !search.is_done() {
                search.step();
            }
            search.result()
        };
        println!(
            "{name}: {:.3}s nodes={} depth={} score={} move={:?}",
            start.elapsed().as_secs_f64(),
            result.nodes,
            result.completed_depth,
            result.score,
            result.command
        );
    }
}
