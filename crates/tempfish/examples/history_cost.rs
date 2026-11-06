//! Reproducible growing-history benchmark with no captures or inversions.
use std::time::Instant;
use tempfish::{Limits, Search};
use temporal_chess_engine::{
    engine::Engine,
    model::{Sq, TimeDirection},
    record::{Command, GameRecord},
};
fn main() {
    let mut engine = Engine::default().with_interactive_calculation().unwrap();
    let mut record = GameRecord::default();
    for t in 0..=64 {
        let (from, to) = match t % 4 {
            0 => (Sq(6, 0), Sq(5, 2)),
            1 => (Sq(6, 7), Sq(5, 5)),
            2 => (Sq(5, 2), Sq(6, 0)),
            _ => (Sq(5, 5), Sq(6, 7)),
        };
        let p = *engine.board(t).unwrap().get_piece(from.0, from.1).unwrap();
        let mv = engine
            .possible_moves(p.origin, p.age)
            .unwrap()
            .into_iter()
            .find(|m| m.sq == to && m.direction == TimeDirection::Future)
            .unwrap();
        if t % 16 == 0 {
            let start = Instant::now();
            for _ in 0..10 {
                std::hint::black_box(engine.after_move(mv).unwrap());
            }
            let apply_ms = start.elapsed().as_secs_f64() * 100.;
            let start = Instant::now();
            let mut search = Search::new(
                engine.clone(),
                Limits {
                    nodes: 200,
                    ..Default::default()
                },
            );
            while !search.is_done() {
                search.step();
            }
            let result = search.result();
            println!(
                "t={t} states={} key={} apply_ms={apply_ms:.3} search_s={:.3} nodes={} depth={}",
                engine
                    .history()
                    .worldlines()
                    .iter()
                    .map(|l| l.len())
                    .sum::<usize>(),
                engine.analysis_key(),
                start.elapsed().as_secs_f64(),
                result.nodes,
                result.completed_depth
            );
        }
        if t < 64 {
            record
                .apply(&mut engine, Command::Move { mv, from })
                .unwrap();
        }
    }
}
