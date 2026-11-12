//! Exact-state, bounded analysis cache. Hash collisions only cost a cache miss.
use std::{collections::HashMap, sync::Arc};
use temporal_chess_engine::{engine::Engine, record::Command};

const MAX_WEIGHT: usize = 65_536;
const MAX_ENTRIES: usize = 2048;
#[derive(Clone, Copy)]
pub(crate) enum Bound {
    Exact,
    Lower,
    Upper,
}
#[derive(Clone, Copy)]
pub(crate) struct Solved {
    pub depth: u8,
    pub score: i32,
    pub bound: Bound,
}
pub(crate) struct Entry {
    pub engine: Arc<Engine>,
    pub commands: Option<Vec<Command>>,
    pub children: HashMap<Command, Result<Arc<Engine>, String>>,
    pub best: Option<Command>,
    pub solved: Option<Solved>,
}
#[derive(Default)]
pub struct Cache {
    entries: HashMap<u64, Entry>,
    weight: usize,
}
impl Cache {
    pub(crate) fn entry(&self, key: u64, engine: &Engine) -> Option<&Entry> {
        self.entries
            .get(&key)
            .filter(|e| e.engine.as_ref() == engine)
    }
    pub(crate) fn insert(&mut self, key: u64, engine: Arc<Engine>) {
        if self.entry(key, &engine).is_some() {
            return;
        }
        let weight = engine.analysis_weight();
        if weight > MAX_WEIGHT {
            return;
        }
        if self.weight + weight > MAX_WEIGHT || self.entries.len() >= MAX_ENTRIES {
            self.entries.clear();
            self.weight = 0;
        }
        self.weight += weight;
        self.entries.insert(
            key,
            Entry {
                engine,
                commands: None,
                children: HashMap::new(),
                best: None,
                solved: None,
            },
        );
    }
    pub(crate) fn commands(&mut self, key: u64, engine: Arc<Engine>, commands: Vec<Command>) {
        self.insert(key, engine.clone());
        if let Some(e) = self.entries.get_mut(&key).filter(|e| e.engine == engine)
            && e.commands.is_none()
            && self.weight + commands.len() * 2 <= MAX_WEIGHT
        {
            self.weight += commands.len() * 2;
            e.commands = Some(commands);
        }
    }
    pub(crate) fn child(
        &mut self,
        key: u64,
        engine: Arc<Engine>,
        command: Command,
        child: Result<Arc<Engine>, String>,
    ) {
        let weight = child.as_ref().map_or(1, |e| e.analysis_weight());
        if self.weight + weight > MAX_WEIGHT {
            return;
        }
        self.insert(key, engine.clone());
        if self.weight + weight > MAX_WEIGHT {
            return;
        }
        if let Some(e) = self.entries.get_mut(&key).filter(|e| e.engine == engine)
            && let std::collections::hash_map::Entry::Vacant(slot) = e.children.entry(command)
        {
            slot.insert(child);
            self.weight += weight;
        }
    }
    pub(crate) fn solved(
        &mut self,
        key: u64,
        engine: Arc<Engine>,
        best: Option<Command>,
        solved: Solved,
    ) {
        self.insert(key, engine.clone());
        if let Some(e) = self.entries.get_mut(&key).filter(|e| e.engine == engine) {
            e.best = best;
            e.solved = Some(solved);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use temporal_chess_engine::model::{Color, Observation, PieceKind, Sq, State};
    #[test]
    fn hash_collisions_and_changed_memory_cannot_reuse_a_position() {
        let state = State::new(PieceKind::N, Color::White, Sq(1, 0));
        let a = Arc::new(Engine::from_worldlines(vec![vec![state.clone()]], Color::White).unwrap());
        let mut changed = state;
        changed.remembered.insert(Observation {
            square: Sq(3, 3),
            kind: PieceKind::N,
            inverted: true,
        });
        let b = Engine::from_worldlines(vec![vec![changed]], Color::White).unwrap();
        assert_ne!(a.analysis_key(), b.analysis_key());
        let mut cache = Cache::default();
        cache.insert(42, a.clone());
        assert!(cache.entry(42, &a).is_some());
        assert!(cache.entry(42, &b).is_none());
    }
}
