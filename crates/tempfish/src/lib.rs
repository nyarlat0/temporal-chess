//! TempFish: incremental iterative-deepening alpha-beta search through the public
//! engine API. Depth counts commands, including free temporal/calculation actions.
//! Scores always belong to White; consecutive actions by one color never negate
//! the score merely because another command was played.
use temporal_chess_engine::{
    engine::{CalculationStatus, Engine, GameOutcome},
    model::{Color, MoveEffect, PieceAt, PieceId, PieceKind, PieceView},
    record::{Command, GameRecord},
};

use std::sync::Arc;
mod cache;
pub use cache::Cache;
use cache::{Bound, Solved};
pub mod parallel;

pub const MAX_DEPTH: u8 = 32;
pub const NODE_BUDGETS: [u64; 7] = [50, 200, 800, 3_000, 10_000, 30_000, 100_000];

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub depth: u8,
    pub nodes: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            depth: MAX_DEPTH,
            nodes: NODE_BUDGETS[1],
        }
    }
}

const MATE: i32 = 1_000_000;
const INF: i32 = 2_000_000;

#[derive(Clone, Debug, Default)]
pub struct SearchResult {
    pub command: Option<Command>,
    pub score: i32,
    pub completed_depth: u8,
    pub nodes: u64,
    pub rejected: u64,
    pub cache_hits: u64,
    pub first_error: Option<String>,
}

struct Frame {
    engine: Arc<Engine>,
    key: u64,
    probed: bool,
    original_alpha: i32,
    original_beta: i32,
    depth: u8,
    ply: u8,
    incoming: Option<Command>,
    sources: Vec<PieceView>,
    generated: usize,
    commands: Vec<Command>,
    ordered: bool,
    cursor: usize,
    best: Option<(Command, i32)>,
    alpha: i32,
    beta: i32,
}
impl Frame {
    fn new(
        engine: Arc<Engine>,
        depth: u8,
        ply: u8,
        incoming: Option<Command>,
        alpha: i32,
        beta: i32,
    ) -> Self {
        let sources = if depth > 0 && engine.outcome().is_none() {
            engine.move_sources().collect()
        } else {
            vec![]
        };
        let key = engine.analysis_key();
        Self {
            engine,
            key,
            probed: false,
            original_alpha: alpha,
            original_beta: beta,
            depth,
            ply,
            incoming,
            sources,
            generated: 0,
            commands: vec![],
            ordered: false,
            cursor: 0,
            best: None,
            alpha,
            beta,
        }
    }
}

pub struct Search {
    root: Arc<Engine>,
    cache: Cache,
    fixed_depth: bool,
    target_depth: u8,
    node_limit: u64,
    iteration: u8,
    stack: Vec<Frame>,
    result: SearchResult,
    done: bool,
}
impl Search {
    pub fn new(engine: Engine, limits: Limits) -> Self {
        let engine = Arc::new(engine);
        let score = evaluate(&engine, 0);
        let frame = Frame::new(engine.clone(), 1, 0, None, -INF, INF);
        Self {
            root: engine,
            cache: Cache::default(),
            fixed_depth: false,
            target_depth: limits.depth.clamp(1, MAX_DEPTH),
            node_limit: limits.nodes.max(1),
            iteration: 1,
            stack: vec![frame],
            result: SearchResult {
                score,
                ..Default::default()
            },
            done: false,
        }
    }
    /// An exact-depth subtree for a root-split worker. Scores retain root ply.
    pub(crate) fn subtree(
        engine: Arc<Engine>,
        depth: u8,
        alpha: i32,
        beta: i32,
        cache: Cache,
    ) -> Self {
        Self {
            root: engine.clone(),
            cache,
            fixed_depth: true,
            target_depth: depth,
            node_limit: u64::MAX,
            iteration: depth,
            stack: vec![Frame::new(engine, depth, 1, None, alpha, beta)],
            result: SearchResult::default(),
            done: false,
        }
    }
    pub(crate) fn take_cache(&mut self) -> Cache {
        std::mem::take(&mut self.cache)
    }
    pub fn nodes(&self) -> u64 {
        self.result.nodes
    }
    pub fn completed_depth(&self) -> u8 {
        self.result.completed_depth
    }
    pub(crate) fn needs_node(&self) -> bool {
        self.stack.last().is_some_and(|f| {
            f.probed
                && f.depth > 0
                && f.engine.outcome().is_none()
                && f.ordered
                && f.alpha < f.beta
                && f.cursor < f.commands.len()
        })
    }
    pub fn is_done(&self) -> bool {
        self.done
    }
    pub fn result(&self) -> SearchResult {
        let mut result = self.result.clone();
        // A user may request a move before even the first iteration completes.
        if result.command.is_none()
            && let Some((command, score)) = self.stack.first().and_then(|frame| frame.best)
        {
            result.command = Some(command);
            result.score = score;
        }
        result
    }
    pub fn finish_now(&mut self) {
        self.done = true;
    }

    /// One bounded search operation: generate one occurrence's moves, apply one
    /// command, or finish one node. Callers can yield between operations on WASM.
    /// An individual engine operation remains atomic and may be expensive.
    pub fn step(&mut self) {
        if self.done {
            return;
        }
        let frame = self.stack.last_mut().expect("active search frame");
        if !frame.probed {
            frame.probed = true;
            if let Some(entry) = self.cache.entry(frame.key, &frame.engine) {
                if let Some(solved) = entry.solved.filter(|s| s.depth == frame.depth) {
                    let score = score_from_cache(solved.score, frame.ply);
                    let usable = match solved.bound {
                        Bound::Exact => true,
                        Bound::Lower => score >= frame.beta,
                        Bound::Upper => score <= frame.alpha,
                    };
                    if usable {
                        self.result.cache_hits += 1;
                        frame.best = entry.best.map(|c| (c, score));
                        self.finish_frame(score);
                        return;
                    }
                }
                if let Some(commands) = &entry.commands {
                    frame.commands = commands.clone();
                    frame.generated = frame.sources.len();
                }
            }
        }
        if frame.depth == 0 || frame.engine.outcome().is_some() {
            let score = evaluate(&frame.engine, frame.ply);
            self.finish_frame(score);
            return;
        }
        // Finish the last leaf before stopping so a one-node budget can still
        // produce a command. Rejected candidates consume budget as well.
        if let Some(&piece) = frame.sources.get(frame.generated) {
            frame.generated += 1;
            match frame.engine.possible_moves(piece.origin, piece.age) {
                Ok(moves) => {
                    for mv in moves {
                        frame.commands.push(Command::Move { mv, from: piece.sq });
                        if let Ok(times) = frame.engine.possible_move_waits(mv) {
                            for until in times {
                                frame.commands.push(Command::MoveAndWait {
                                    mv,
                                    from: piece.sq,
                                    until,
                                });
                            }
                        }
                    }
                }
                Err(error) => {
                    self.result
                        .first_error
                        .get_or_insert_with(|| error.to_string());
                    self.result.rejected += 1;
                }
            }
            if piece.rights.ordinary {
                let source = PieceAt {
                    piece: PieceId::from_piv(piece),
                    t: piece.t,
                };
                // Final validation uses skip_ordinary, not a second speculative
                // clone for each preview. Invalid candidates are never played.
                frame.commands.extend(
                    frame
                        .engine
                        .timeline_times()
                        .filter(|&t| {
                            Color::from_board(t) == piece.color
                                && if piece.inverted {
                                    t < piece.t
                                } else {
                                    t > piece.t
                                }
                        })
                        .map(|to_t| Command::Advance {
                            source,
                            from: piece.sq,
                            to_t,
                        }),
                );
            }
            return;
        }
        if !frame.ordered {
            if matches!(
                frame.engine.calculation_status(),
                CalculationStatus::Paused { .. }
            ) {
                frame.commands.push(Command::Continue);
            }
            if frame.engine.can_decline_reactions() {
                frame.commands.push(Command::DeclineReactions);
            }
            let preferred = self
                .cache
                .entry(frame.key, &frame.engine)
                .and_then(|e| e.best);
            frame.commands.sort_by_cached_key(|command| {
                std::cmp::Reverse((
                    Some(*command) == preferred,
                    priority(&frame.engine, *command),
                ))
            });
            frame.commands.dedup();
            self.cache
                .commands(frame.key, frame.engine.clone(), frame.commands.clone());
            frame.ordered = true;
            return;
        }
        if frame.alpha >= frame.beta || frame.cursor == frame.commands.len() {
            let score = frame
                .best
                .map_or_else(|| evaluate(&frame.engine, frame.ply), |(_, score)| score);
            self.finish_frame(score);
            return;
        }
        if self.result.nodes >= self.node_limit {
            self.done = true;
            return;
        }
        let command = frame.commands[frame.cursor];
        frame.cursor += 1;
        self.result.nodes += 1;
        let cached = self
            .cache
            .entry(frame.key, &frame.engine)
            .and_then(|e| e.children.get(&command))
            .cloned();
        let next = if let Some(next) = cached {
            self.result.cache_hits += 1;
            next
        } else {
            let next = successor(&frame.engine, command).map(Arc::new);
            self.cache
                .child(frame.key, frame.engine.clone(), command, next.clone());
            next
        };
        let next = match next {
            Ok(next) => next,
            Err(error) => {
                if command.spatial_move().is_some() {
                    self.result.first_error.get_or_insert(error);
                }
                self.result.rejected += 1;
                return;
            }
        };
        if next == frame.engine {
            return;
        }
        let child = Frame::new(
            next,
            frame.depth - 1,
            frame.ply + 1,
            Some(command),
            frame.alpha,
            frame.beta,
        );
        self.stack.push(child);
    }

    fn finish_frame(&mut self, score: i32) {
        let frame = self.stack.pop().unwrap();
        let bound = if score <= frame.original_alpha {
            Bound::Upper
        } else if score >= frame.original_beta {
            Bound::Lower
        } else {
            Bound::Exact
        };
        self.cache.solved(
            frame.key,
            frame.engine.clone(),
            frame.best.map(|(c, _)| c),
            Solved {
                depth: frame.depth,
                score: score_to_cache(score, frame.ply),
                bound,
            },
        );
        if let Some(parent) = self.stack.last_mut() {
            let maximizing = parent.engine.turn() == Color::White;
            if parent.best.is_none_or(|(_, best)| {
                if maximizing {
                    score > best
                } else {
                    score < best
                }
            }) {
                parent.best = Some((frame.incoming.unwrap(), score));
            }
            if maximizing {
                parent.alpha = parent.alpha.max(score);
            } else {
                parent.beta = parent.beta.min(score);
            }
        } else {
            self.result.command = frame.best.map(|(command, _)| command);
            self.result.score = score;
            self.result.completed_depth = self.iteration;
            if self.fixed_depth
                || self.iteration == self.target_depth
                || self.result.command.is_none()
                || score.abs() >= MATE - MAX_DEPTH as i32
            {
                self.done = true;
            } else {
                self.iteration += 1;
                self.stack.push(Frame::new(
                    self.root.clone(),
                    self.iteration,
                    0,
                    None,
                    -INF,
                    INF,
                ));
            }
        }
    }
}

fn successor(engine: &Engine, command: Command) -> Result<Engine, String> {
    if let Command::Move { mv, from } | Command::MoveAndWait { mv, from, .. } = command {
        if !engine
            .piece_state(mv.source())
            .is_some_and(|s| s.alive && s.x == from.0 && s.y == from.1)
        {
            return Err("piece occurrence is absent or cannot act".into());
        }
        return match command.wait_until() {
            Some(until) => engine.after_move_with_wait(mv, until),
            None => engine.after_move(mv),
        }
        .map_err(|e| e.to_string());
    }
    let mut next = engine.clone();
    GameRecord::new(next.history().rules())
        .apply(&mut next, command)
        .map_err(|e| e.to_string())?;
    Ok(next)
}
fn score_to_cache(score: i32, ply: u8) -> i32 {
    if score >= MATE - MAX_DEPTH as i32 {
        score + ply as i32
    } else if score <= -MATE + MAX_DEPTH as i32 {
        score - ply as i32
    } else {
        score
    }
}
fn score_from_cache(score: i32, ply: u8) -> i32 {
    if score >= MATE - MAX_DEPTH as i32 {
        score - ply as i32
    } else if score <= -MATE + MAX_DEPTH as i32 {
        score + ply as i32
    } else {
        score
    }
}
fn value(kind: PieceKind) -> i32 {
    match kind {
        PieceKind::P { .. } => 100,
        PieceKind::N => 320,
        PieceKind::B => 330,
        PieceKind::R { .. } => 500,
        PieceKind::Q => 900,
        PieceKind::K { .. } => 0,
    }
}
fn priority(engine: &Engine, command: Command) -> i32 {
    match command {
        Command::Move { mv, .. } | Command::MoveAndWait { mv, .. } => {
            let target_time = mv.direction.step(mv.t).unwrap_or(mv.t);
            let target = engine.board(target_time).or_else(|| engine.board(mv.t));
            let square = mv.en_passant().unwrap_or(mv.sq);
            let capture = target
                .and_then(|b| b.get_piece(square.0, square.1))
                .map_or(0, |p| {
                    value(p.kind)
                        + if matches!(p.kind, PieceKind::K { .. }) {
                            10000
                        } else {
                            0
                        }
                });
            capture * 10
                + if matches!(mv.effect, MoveEffect::Promote(_)) {
                    8000
                } else {
                    0
                }
        }
        Command::Continue => 500,
        Command::DeclineReactions => -500,
        Command::Advance { .. } => -1000,
    }
}
fn evaluate(engine: &Engine, ply: u8) -> i32 {
    if let Some(outcome) = engine.outcome() {
        return match outcome {
            GameOutcome::Stalemate => 0,
            GameOutcome::Victory { winner, .. } => {
                if winner == Color::White {
                    MATE - ply as i32
                } else {
                    -MATE + ply as i32
                }
            }
        };
    }
    let mut score = 0;
    for line in engine.history().worldlines() {
        let Some(piece) = line.last() else {
            continue;
        };
        // Count each worldline once, never once per board/incarnation.
        let material = if piece.alive {
            let centre = 14 - (piece.x as i32 * 2 - 7).abs() - (piece.y as i32 * 2 - 7).abs();
            let position = match piece.kind {
                PieceKind::N | PieceKind::B => centre * 3,
                PieceKind::P { .. } => {
                    (if piece.color == Color::White {
                        piece.y
                    } else {
                        7 - piece.y
                    }) as i32
                        * 5
                        + centre
                }
                PieceKind::Q => centre,
                _ => 0,
            };
            value(piece.kind) + position
        } else {
            0
        };
        let reserves = line
            .iter()
            .filter(|s| s.alive && s.rights.is_movable() && Color::from_board(s.t) == s.color)
            .take(3)
            .count() as i32
            * 4;
        score += if piece.color == Color::White {
            material + reserves
        } else {
            -material - reserves
        };
    }
    score.clamp(-MATE / 2, MATE / 2)
}

#[cfg(test)]
mod tests;
