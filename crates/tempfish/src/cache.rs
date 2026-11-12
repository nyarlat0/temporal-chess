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
    weight: usize,
    stamp: u64,
    priority: u8,
    pub engine: Arc<Engine>,
    pub commands: Option<Vec<Command>>,
    pub children: HashMap<Command, Result<Arc<Engine>, String>>,
    pub best: Option<Command>,
    pub solutions: Vec<(Solved, Option<Command>)>,
}
#[derive(Default)]
pub struct Cache {
    entries: HashMap<u64, Entry>,
    weight: usize,
    clock: u64,
}
impl Cache {
    pub(crate) fn next_turn(&mut self) {
        // Old deep branches must eventually yield space to the live game.
        for entry in self.entries.values_mut() {
            entry.priority = entry.priority.saturating_sub(1);
        }
    }
    pub(crate) fn entry(&self, key: u64, engine: &Engine) -> Option<&Entry> {
        self.entries
            .get(&key)
            .filter(|e| e.engine.as_ref() == engine)
    }
    fn make_room(&mut self, extra: usize, keep: u64, priority: u8) -> bool {
        if extra > MAX_WEIGHT {
            return false;
        }
        while self.weight + extra > MAX_WEIGHT
            || (!self.entries.contains_key(&keep) && self.entries.len() >= MAX_ENTRIES)
        {
            // Keep deeply searched positions instead of wiping the entire tree
            // at the memory limit. Leaves are cheap to calculate again.
            let victim = self
                .entries
                .iter()
                .filter(|(key, e)| **key != keep && e.priority <= priority)
                .min_by_key(|(_, e)| (e.priority, e.stamp))
                .map(|(&key, _)| key);
            let Some(key) = victim else {
                return false;
            };
            self.weight -= self.entries.remove(&key).unwrap().weight;
        }
        true
    }
    #[cfg(test)]
    pub(crate) fn insert(&mut self, key: u64, engine: Arc<Engine>) {
        self.insert_at_depth(key, engine, 0);
    }
    fn insert_at_depth(&mut self, key: u64, engine: Arc<Engine>, priority: u8) {
        self.clock = self.clock.wrapping_add(1);
        if let Some(entry) = self.entries.get_mut(&key).filter(|e| e.engine == engine) {
            entry.stamp = self.clock;
            entry.priority = entry.priority.max(priority);
            return;
        }
        if let Some(old) = self.entries.remove(&key) {
            self.weight -= old.weight;
        }
        let weight = engine.analysis_weight();
        if !self.make_room(weight, key, priority) {
            return;
        }
        self.weight += weight;
        self.entries.insert(
            key,
            Entry {
                engine,
                weight,
                stamp: self.clock,
                priority,
                commands: None,
                children: HashMap::new(),
                best: None,
                solutions: Vec::new(),
            },
        );
    }
    pub(crate) fn commands(&mut self, key: u64, engine: Arc<Engine>, commands: Vec<Command>) {
        self.insert_at_depth(key, engine.clone(), 1);
        if self
            .entry(key, &engine)
            .is_none_or(|e| e.commands.is_some())
            || !self.make_room(commands.len() * 2, key, 1)
        {
            return;
        }
        if let Some(e) = self.entries.get_mut(&key).filter(|e| e.engine == engine) {
            self.weight += commands.len() * 2;
            e.weight += commands.len() * 2;
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
        self.insert_at_depth(key, engine.clone(), 1);
        if self
            .entry(key, &engine)
            .is_none_or(|e| e.children.contains_key(&command))
            || !self.make_room(weight, key, 1)
        {
            return;
        }
        if let Some(e) = self.entries.get_mut(&key).filter(|e| e.engine == engine)
            && let std::collections::hash_map::Entry::Vacant(slot) = e.children.entry(command)
        {
            slot.insert(child);
            self.weight += weight;
            e.weight += weight;
        }
    }
    pub(crate) fn solved(
        &mut self,
        key: u64,
        engine: Arc<Engine>,
        best: Option<Command>,
        solved: Solved,
    ) {
        self.insert_at_depth(
            key,
            engine.clone(),
            solved.depth.saturating_mul(2).saturating_add(1),
        );
        if let Some(e) = self.entries.get_mut(&key).filter(|e| e.engine == engine) {
            if e.solutions.iter().all(|(s, _)| s.depth <= solved.depth) {
                e.best = best;
            }
            if let Some(slot) = e
                .solutions
                .iter_mut()
                .find(|(s, _)| s.depth == solved.depth)
            {
                // A narrow-window bound must not discard an exact result.
                if !matches!(slot.0.bound, Bound::Exact) || matches!(solved.bound, Bound::Exact) {
                    *slot = (solved, best);
                }
            } else {
                e.solutions.push((solved, best));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use temporal_chess_engine::model::{Color, Observation, PieceKind, Sq, State};
    #[test]
    fn pressure_evicts_shallow_entries_without_losing_the_deep_result() {
        let engine = Arc::new(Engine::default());
        let mut cache = Cache::default();
        cache.solved(
            42,
            engine.clone(),
            None,
            Solved {
                depth: 4,
                score: 12,
                bound: Bound::Exact,
            },
        );
        for key in 100..2500 {
            cache.insert(key, engine.clone());
            assert!(cache.weight <= MAX_WEIGHT);
            assert!(cache.entries.len() <= MAX_ENTRIES);
        }
        assert_eq!(cache.entry(42, &engine).unwrap().solutions[0].0.score, 12);
        assert!(cache.entries.len() > 1);
        // Aging eventually makes even an old deep branch replaceable.
        for _ in 0..10 {
            cache.next_turn();
        }
        for key in 2500..5000 {
            cache.insert(key, engine.clone());
        }
        assert!(cache.entry(42, &engine).is_none());
        assert_eq!(cache.weight, cache.entries.values().map(|e| e.weight).sum());
    }
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
