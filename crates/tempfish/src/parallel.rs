//! Root splitting with a single global node budget and completed-depth barrier.
//! Each worker searches one root alternative; a partial round never replaces
//! the last fully completed iteration. Free actions retain their real side.
use crate::{Cache, INF, Limits, MATE, MAX_DEPTH, Search, SearchResult, successor};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use temporal_chess_engine::{engine::Engine, model::Color, record::Command};

pub const MAX_THREADS: usize = 32;
const QUANTUM: u64 = 128;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Work {
    pub id: u64,
    pub command: String,
    pub depth: u8,
    pub alpha: i32,
    pub beta: i32,
    pub nodes: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Report {
    pub id: u64,
    pub nodes: u64,
    pub cache_hits: u64,
    pub rejected: u64,
    pub completed: bool,
    pub score: Option<i32>,
    pub error: Option<String>,
}
struct Assignment {
    work: Work,
    index: usize,
    leased: u64,
}
pub struct Coordinator {
    preparation: Search,
    commands: Vec<Command>,
    scores: Vec<Option<i32>>,
    order: Vec<usize>,
    cursor: usize,
    finished: usize,
    depth: u8,
    limits: Limits,
    maximizing: bool,
    active: HashMap<usize, Assignment>,
    next_id: u64,
    prepared: bool,
    done: bool,
    best: Option<(usize, i32)>,
    result: SearchResult,
}
impl Coordinator {
    pub fn new(engine: Engine, limits: Limits) -> Self {
        Self::with_cache(engine, limits, Cache::default())
    }
    pub fn with_cache(engine: Engine, limits: Limits, cache: Cache) -> Self {
        let maximizing = engine.turn() == Color::White;
        Self {
            preparation: Search::with_cache(engine, limits, cache),
            commands: vec![],
            scores: vec![],
            order: vec![],
            cursor: 0,
            finished: 0,
            depth: 1,
            limits: Limits {
                depth: limits.depth.clamp(1, MAX_DEPTH),
                nodes: limits.nodes.max(1),
            },
            maximizing,
            active: HashMap::new(),
            next_id: 0,
            prepared: false,
            done: false,
            best: None,
            result: SearchResult::default(),
        }
    }
    pub fn take_cache(&mut self) -> Cache {
        self.preparation.take_cache()
    }
    pub fn prepare_step(&mut self) {
        if self.prepared || self.done {
            return;
        }
        if self.preparation.root.outcome().is_some() {
            self.done = true;
            return;
        }
        // Preparation generates commands only; never finish a cached root
        // score through Search::step (which would consume the root frame).
        let frame = &mut self.preparation.stack[0];
        if !frame.probed {
            frame.probed = true;
            if let Some(commands) = self
                .preparation
                .cache
                .entry(frame.key, &frame.engine)
                .and_then(|entry| entry.commands.as_ref())
            {
                frame.commands = commands.clone();
                frame.generated = frame.sources.len();
                self.result.cache_hits += 1;
            }
        }
        if !self.preparation.stack[0].ordered {
            self.preparation.step();
            return;
        }
        self.commands = self.preparation.stack[0].commands.clone();
        self.scores = vec![None; self.commands.len()];
        self.order = (0..self.commands.len()).collect();
        self.result.rejected = self.preparation.result.rejected;
        self.result.first_error = self.preparation.result.first_error.clone();
        self.prepared = true;
        self.done = self.commands.is_empty();
    }
    pub fn is_prepared(&self) -> bool {
        self.prepared
    }
    pub fn is_done(&self) -> bool {
        self.done
    }
    pub fn stop(&mut self) {
        self.done = true;
    }
    pub fn result(&self) -> SearchResult {
        let mut result = self.result.clone();
        if result.command.is_none()
            && let Some((index, score)) = self.best
        {
            result.command = Some(self.commands[index]);
            result.score = score;
        }
        result
    }
    fn window(&self) -> (i32, i32) {
        match self.best {
            Some((_, score)) if self.maximizing => (score, INF),
            Some((_, score)) => (-INF, score),
            None => (-INF, INF),
        }
    }
    pub fn next_work(&mut self, worker: usize) -> Option<Work> {
        if !self.prepared || self.done {
            return None;
        }
        let leased: u64 = self.active.values().map(|a| a.leased).sum();
        let available = self.limits.nodes.saturating_sub(self.result.nodes + leased);
        if available == 0 {
            if leased == 0 {
                self.done = true;
            }
            return None;
        }
        let nodes = available.min(QUANTUM);
        if let Some(a) = self.active.get_mut(&worker) {
            if a.leased != 0 {
                return None;
            }
            a.work.nodes = nodes;
            a.leased = nodes;
            return Some(a.work.clone());
        }
        // Establish a useful root bound before opening sibling searches.
        if (self.finished == 0 && !self.active.is_empty()) || self.cursor == self.order.len() {
            return None;
        }
        let index = self.order[self.cursor];
        self.cursor += 1;
        let (alpha, beta) = self.window();
        self.next_id += 1;
        let work = Work {
            id: self.next_id,
            command: self.commands[index].to_string(),
            depth: self.depth,
            alpha,
            beta,
            nodes,
        };
        self.active.insert(
            worker,
            Assignment {
                work: work.clone(),
                index,
                leased: nodes,
            },
        );
        Some(work)
    }
    pub fn accept(&mut self, worker: usize, report: Report) {
        let Some(a) = self.active.get_mut(&worker) else {
            return;
        };
        if a.work.id != report.id || report.nodes > a.leased {
            return;
        }
        a.leased = 0;
        self.result.nodes += report.nodes;
        self.result.cache_hits += report.cache_hits;
        self.result.rejected += report.rejected;
        if let Some(error) = report.error {
            self.result.first_error.get_or_insert(error);
        }
        if !report.completed {
            return;
        }
        let index = a.index;
        self.active.remove(&worker);
        self.finished += 1;
        self.scores[index] = report.score;
        if let Some(score) = report.score
            && self.best.is_none_or(|(_, best)| {
                if self.maximizing {
                    score > best
                } else {
                    score < best
                }
            })
        {
            self.best = Some((index, score));
        }
        if self.finished == self.commands.len() {
            self.result.command = self.best.map(|(i, _)| self.commands[i]);
            self.result.score = self.best.map_or(0, |(_, s)| s);
            self.result.completed_depth = self.depth;
            if self.depth == self.limits.depth
                || self.best.is_none()
                || self.result.score.abs() >= MATE - MAX_DEPTH as i32
                || self.result.nodes >= self.limits.nodes
            {
                self.done = true;
                return;
            }
            let best = self.best.map(|(i, _)| i);
            self.order.sort_by_key(|&i| {
                (
                    Some(i) != best,
                    self.scores[i].map_or(i32::MAX, |s| if self.maximizing { -s } else { s }),
                    i,
                )
            });
            self.depth += 1;
            self.cursor = 0;
            self.finished = 0;
            self.best = None;
        }
    }
}

pub struct Worker {
    root: Arc<Engine>,
    key: u64,
    cache: Cache,
    search: Option<Search>,
    id: Option<u64>,
}
impl Worker {
    pub fn new(engine: Engine) -> Self {
        Self::with_cache(engine, Cache::default())
    }
    pub fn with_cache(engine: Engine, mut cache: Cache) -> Self {
        cache.next_turn();
        let key = engine.analysis_key();
        Self {
            root: Arc::new(engine),
            key,
            cache,
            search: None,
            id: None,
        }
    }
    /// Keep completed subtrees even if the previous node budget interrupted
    /// the surrounding branch. Work IDs belong only to the new root.
    pub fn reset(&mut self, engine: Engine) {
        self.cache = self.take_cache();
        self.cache.next_turn();
        self.key = engine.analysis_key();
        self.root = Arc::new(engine);
    }
    pub fn take_cache(&mut self) -> Cache {
        self.id = None;
        match self.search.take() {
            Some(mut search) => search.take_cache(),
            None => std::mem::take(&mut self.cache),
        }
    }
    pub fn run(&mut self, work: Work, cancelled: impl Fn() -> bool) -> Report {
        let mut report = Report {
            id: work.id,
            ..Default::default()
        };
        if self.id != Some(work.id) {
            let command = match work.command.parse::<Command>() {
                Ok(c) => c,
                Err(e) => {
                    report.error = Some(e);
                    report.completed = true;
                    return report;
                }
            };
            let cached = self
                .cache
                .entry(self.key, &self.root)
                .and_then(|e| e.children.get(&command))
                .cloned();
            let next = if let Some(next) = cached {
                report.cache_hits += 1;
                next
            } else {
                let next = successor(&self.root, command).map(Arc::new);
                self.cache
                    .child(self.key, self.root.clone(), command, next.clone());
                next
            };
            report.nodes = 1;
            let next = match next {
                Ok(next) if next != self.root => next,
                Ok(_) => {
                    report.completed = true;
                    return report;
                }
                Err(e) => {
                    report.completed = true;
                    report.rejected = 1;
                    if command.spatial_move().is_some() {
                        report.error = Some(e);
                    }
                    return report;
                }
            };
            self.search = Some(Search::subtree(
                next,
                work.depth - 1,
                work.alpha,
                work.beta,
                std::mem::take(&mut self.cache),
            ));
            self.id = Some(work.id);
        }
        let search = self.search.as_mut().expect("assigned search");
        let before = search.result();
        while !search.is_done() && !cancelled() {
            let used = report.nodes + search.nodes() - before.nodes;
            if used >= work.nodes && search.needs_node() {
                break;
            }
            search.step();
        }
        let result = search.result();
        report.nodes += result.nodes - before.nodes;
        report.cache_hits += result.cache_hits - before.cache_hits;
        report.rejected += result.rejected - before.rejected;
        report.error = result.first_error;
        report.completed = search.is_done();
        if report.completed {
            report.score = Some(result.score);
            self.cache = search.take_cache();
            self.search = None;
            self.id = None;
        }
        report
    }
}

/// Bounded per-worker search state, carried between consecutive turns.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub struct Memory {
    preparation: Cache,
    workers: Vec<Cache>,
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run(
    engine: Engine,
    limits: Limits,
    threads: usize,
    stop: &std::sync::atomic::AtomicBool,
    progress: impl FnMut(&SearchResult),
) -> SearchResult {
    run_with_memory(
        engine,
        limits,
        threads,
        stop,
        &mut Memory::default(),
        progress,
    )
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run_with_memory(
    engine: Engine,
    limits: Limits,
    threads: usize,
    stop: &std::sync::atomic::AtomicBool,
    memory: &mut Memory,
    mut progress: impl FnMut(&SearchResult),
) -> SearchResult {
    use std::{
        sync::{atomic::Ordering, mpsc},
        time::Duration,
    };
    let mut coordinator = Coordinator::with_cache(
        engine.clone(),
        limits,
        std::mem::take(&mut memory.preparation),
    );
    while !coordinator.is_prepared() && !coordinator.is_done() && !stop.load(Ordering::Relaxed) {
        coordinator.prepare_step();
    }
    std::thread::scope(|scope| {
        let (tx, rx) = mpsc::channel();
        let mut senders = vec![];
        let mut handles = vec![];
        let mut caches = std::mem::take(&mut memory.workers).into_iter();
        for id in 0..threads.clamp(1, MAX_THREADS) {
            let (work_tx, work_rx) = mpsc::channel::<Work>();
            senders.push(work_tx);
            let tx = tx.clone();
            let root = engine.clone();
            let cache = caches.next().unwrap_or_default();
            handles.push(scope.spawn(move || {
                let engine_for_recovery = root.clone();
                let mut worker = Worker::with_cache(root, cache);
                while let Ok(work) = work_rx.recv() {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let work_id = work.id;
                    let work_nodes = work.nodes;
                    let report = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        worker.run(work, || stop.load(Ordering::Relaxed))
                    }))
                    .unwrap_or_else(|_| {
                        // Report the failed branch instead of leaving its lease
                        // outstanding forever after a panicking worker exits.
                        worker = Worker::new(engine_for_recovery.clone());
                        Report {
                            id: work_id,
                            nodes: work_nodes,
                            rejected: 1,
                            completed: true,
                            error: Some("TempFish: engine panicked during search".into()),
                            ..Default::default()
                        }
                    });
                    if tx.send((id, report)).is_err() {
                        break;
                    }
                }
                worker.take_cache()
            }));
        }
        drop(tx);
        while !coordinator.is_done() && !stop.load(Ordering::Relaxed) {
            for (id, sender) in senders.iter().enumerate() {
                if let Some(work) = coordinator.next_work(id)
                    && sender.send(work).is_err()
                {
                    coordinator.stop();
                    break;
                }
            }
            progress(&coordinator.result());
            if coordinator.is_done() {
                break;
            }
            match rx.recv_timeout(Duration::from_millis(10)) {
                Ok((id, report)) => coordinator.accept(id, report),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
        drop(senders);
        memory.workers = handles
            .into_iter()
            .map(|h| h.join().unwrap_or_default())
            .collect();
    });
    memory.preparation = coordinator.take_cache();
    coordinator.result()
}
