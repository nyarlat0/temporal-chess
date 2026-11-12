//! Headless, independent WASM instance per worker. No Bevy, DOM or shared memory.
use tempfish::{
    Limits,
    parallel::{Coordinator, Report, Work, Worker},
};
use temporal_chess_engine::{engine::Engine, record::GameRecord};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Session {
    engine: Engine,
    record: GameRecord,
    coordinator: Option<Coordinator>,
    worker: Option<Worker>,
}
fn error(e: impl ToString) -> JsValue {
    JsValue::from_str(&e.to_string())
}
#[wasm_bindgen]
impl Session {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            engine: Engine::default()
                .with_interactive_calculation()
                .expect("initial game"),
            record: GameRecord::default(),
            coordinator: None,
            worker: None,
        }
    }
    pub fn start(&mut self, text: &str, nodes: u32, coordinator: bool) -> Result<(), JsValue> {
        // The common case is an append-only journal. Reuse each worker's last
        // position rather than replaying the entire game on every bot turn.
        let previous = self.record.to_text();
        let continuing = text.starts_with(&previous);
        let cache = self
            .coordinator
            .take()
            .filter(|_| continuing)
            .map(|mut c| c.take_cache())
            .unwrap_or_default();
        if !continuing {
            self.worker = None;
        }
        if let Some(tail) = text.strip_prefix(&previous) {
            for line in tail.lines().filter(|s| !s.trim().is_empty()) {
                let command = line.parse().map_err(error)?;
                self.record
                    .apply(&mut self.engine, command)
                    .map_err(error)?;
            }
        } else {
            (self.engine, self.record) = GameRecord::load(text).map_err(error)?;
        }
        if coordinator {
            self.coordinator = Some(Coordinator::with_cache(
                self.engine.clone(),
                Limits {
                    nodes: u64::from(nodes),
                    ..Default::default()
                },
                cache,
            ));
        } else if let Some(worker) = &mut self.worker {
            worker.reset(self.engine.clone());
        } else {
            self.worker = Some(Worker::new(self.engine.clone()));
        }
        Ok(())
    }
    pub fn prepare(&mut self) -> bool {
        let c = self.coordinator.as_mut().expect("coordinator");
        c.prepare_step();
        c.is_prepared() || c.is_done()
    }
    pub fn next_work(&mut self, id: usize) -> String {
        serde_json::to_string(
            &self
                .coordinator
                .as_mut()
                .expect("coordinator")
                .next_work(id),
        )
        .expect("work JSON")
    }
    pub fn accept(&mut self, id: usize, text: &str) -> Result<(), JsValue> {
        let report: Report = serde_json::from_str(text).map_err(error)?;
        self.coordinator
            .as_mut()
            .expect("coordinator")
            .accept(id, report);
        Ok(())
    }
    pub fn work(&mut self, text: &str) -> Result<String, JsValue> {
        let work: Work = serde_json::from_str(text).map_err(error)?;
        Ok(
            serde_json::to_string(&self.worker.as_mut().expect("worker").run(work, || false))
                .expect("report JSON"),
        )
    }
    pub fn result(&self) -> String {
        let c = self.coordinator.as_ref().expect("coordinator");
        let r = c.result();
        serde_json::json!({"done":c.is_done(),"command":r.command.map(|c|c.to_string()),"score":r.score,"depth":r.completed_depth,"nodes":r.nodes,"cache_hits":r.cache_hits,"rejected":r.rejected,"error":r.first_error}).to_string()
    }
}
impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(session: &mut Session, command: &str, depth: u8) -> Report {
        let work = Work {
            id: 1,
            command: command.into(),
            depth,
            alpha: -2_000_000,
            beta: 2_000_000,
            nodes: 100_000,
        };
        serde_json::from_str(
            &session
                .work(&serde_json::to_string(&work).unwrap())
                .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn consecutive_worker_sessions_reuse_search_but_replacing_the_record_resets_it() {
        let opening = GameRecord::default().to_text();
        let first = "move 0 12:0:0 e2 e4 + normal";
        let mut session = Session::new();
        session.start(&opening, 100_000, false).unwrap();
        assert!(run(&mut session, first, 3).completed);
        let continued = format!("{opening}{first}\n");
        session.start(&continued, 100_000, false).unwrap();
        // The played line follows the previous search's best continuation;
        // other replies may only have a bound from an alpha-beta cutoff.
        let mut reference = tempfish::Search::new(
            session.engine.clone(),
            Limits {
                depth: 2,
                nodes: 100_000,
            },
        );
        while !reference.is_done() {
            reference.step();
        }
        let reply = reference.result().command.unwrap().to_string();
        let warm = run(&mut session, &reply, 2);
        let mut cold = Session::new();
        cold.start(&continued, 100_000, false).unwrap();
        let fresh = run(&mut cold, &reply, 2);
        assert!(warm.completed && fresh.completed);
        assert_eq!(warm.score, fresh.score);
        assert!(warm.cache_hits > 0);
        assert!(warm.nodes < fresh.nodes, "warm={warm:?}; fresh={fresh:?}");
        // Undo/import of another journal cannot retain the old work assignment.
        session.start(&opening, 100_000, false).unwrap();
        let reset = run(&mut session, first, 1);
        assert!(reset.completed);
        assert_eq!(reset.cache_hits, 0);
    }
}
