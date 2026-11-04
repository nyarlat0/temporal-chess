//! TempFish: incremental iterative-deepening alpha-beta search through the public
//! engine API. Depth counts commands, including free temporal/calculation actions.
//! Scores always belong to White; consecutive actions by one color never negate
//! the score merely because another command was played.
use temporal_chess_engine::{
    engine::{CalculationStatus, Engine, GameOutcome},
    model::{Color, MoveEffect, PieceAt, PieceId, PieceKind, PieceView},
    record::{Command, GameRecord},
};

pub const MAX_DEPTH: u8 = 6;
pub const NODE_BUDGETS: [u64; 5] = [50, 200, 800, 3_000, 10_000];

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
    pub first_error: Option<String>,
}

struct Frame {
    engine: Engine,
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
        engine: Engine,
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
        Self {
            engine,
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
    root: Engine,
    target_depth: u8,
    node_limit: u64,
    iteration: u8,
    stack: Vec<Frame>,
    result: SearchResult,
    done: bool,
}
impl Search {
    pub fn new(engine: Engine, limits: Limits) -> Self {
        let score = evaluate(&engine, 0);
        let frame = Frame::new(engine.clone(), 1, 0, None, -INF, INF);
        Self {
            root: engine,
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
        if self.result.nodes >= self.node_limit && self.result().command.is_some() {
            self.done = true;
            return;
        }
        let frame = self.stack.last_mut().expect("active search frame");
        if frame.depth == 0 || frame.engine.outcome().is_some() {
            let score = evaluate(&frame.engine, frame.ply);
            self.finish_frame(score);
            return;
        }
        if let Some(&piece) = frame.sources.get(frame.generated) {
            frame.generated += 1;
            match frame.engine.possible_moves(piece.origin, piece.age) {
                Ok(moves) => frame.commands.extend(
                    moves
                        .into_iter()
                        .map(|mv| Command::Move { mv, from: piece.sq }),
                ),
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
            frame
                .commands
                .sort_by_cached_key(|command| std::cmp::Reverse(priority(&frame.engine, *command)));
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
        let command = frame.commands[frame.cursor];
        frame.cursor += 1;
        let mut next = frame.engine.clone();
        self.result.nodes += 1;
        // The exact public command path also handles losing paradoxes and
        // commands which erase themselves during history reconciliation.
        if let Err(error) = GameRecord::new(next.history().rules()).apply(&mut next, command) {
            if matches!(command, Command::Move { .. }) {
                self.result
                    .first_error
                    .get_or_insert_with(|| error.to_string());
            }
            self.result.rejected += 1;
            return;
        }
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
            if self.iteration == self.target_depth
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
        Command::Move { mv, .. } => {
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
