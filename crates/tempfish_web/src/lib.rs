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
        self.coordinator = None;
        self.worker = None;
        // The common case is an append-only journal. Reuse each worker's last
        // position rather than replaying the entire game on every bot turn.
        let previous = self.record.to_text();
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
            self.coordinator = Some(Coordinator::new(
                self.engine.clone(),
                Limits {
                    nodes: u64::from(nodes),
                    ..Default::default()
                },
            ));
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
