//! Presentation-only transitions. Worldline edges, not worldline IDs alone,
//! connect occurrences across a board boundary (including reversed moves).
use std::collections::HashSet;
use temporal_chess_engine::{
    engine::Engine,
    model::{Castling, Move, MoveId, PieceAt, PieceId, PieceKind, PieceView},
};
use web_time::Instant;

pub fn ease(progress: f32) -> f32 {
    let t = progress.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Quintic ease-in-out: zero velocity and acceleration at both ends.
/// The board settles before its final appearance blends into the static scene.
pub fn travel_curve(progress: f32) -> f32 {
    let t = progress.clamp(0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Sine ease-in-out for camera navigation, with no overshoot.
pub fn scroll_curve(progress: f32) -> f32 {
    (1.0 - (progress.clamp(0.0, 1.0) * std::f32::consts::PI).cos()) * 0.5
}

fn identity(piece: PieceView) -> PieceAt {
    PieceAt {
        piece: PieceId::from_piv(piece),
        t: piece.t,
    }
}

fn pieces(engine: &Engine, t: usize) -> Vec<PieceView> {
    engine.preboard(t).map_or_else(Vec::new, |board| {
        board
            .space
            .iter()
            .flatten()
            .flat_map(|cell| cell.iter().copied())
            .collect()
    })
}

/// Associate a causal edge with its spatial command. The rook's castling
/// edge belongs to the king's command even though its PieceId is different.
fn recorded_edge(engine: &Engine, from: PieceView, to: PieceView) -> Option<Move> {
    let mv = *engine.history().moves().get(&MoveId::from_piv(from))?;
    if mv.direction.step(from.t) != Some(to.t) || from.apply_mv(mv).sq != to.sq {
        return None;
    }
    if mv.pid == PieceId::from_piv(from) {
        return Some(mv);
    }
    let castling = mv.castling()?;
    let rook_file = match castling {
        Castling::Long => 0,
        Castling::Short => 7,
    };
    let king = engine.piece_state(mv.source())?;
    (matches!(from.kind, PieceKind::R { .. })
        && from.sq.0 == rook_file
        && from.sq.1 == king.y
        && from.color == king.color
        && matches!(king.kind, PieceKind::K { .. }))
    .then_some(mv)
}

pub struct PieceMotion {
    pub from: PieceView,
    pub to: PieceView,
    pub visible_from: bool,
    pub visible_to: bool,
    pub reversed_move: bool,
}

pub struct BoardTransition {
    pub from_t: usize,
    pub to_t: usize,
    // Activity markers must use each snapshot's own latest board.
    pub source_horizon: usize,
    pub target_horizon: usize,
    pub tracks: Vec<PieceMotion>,
    pub source: Vec<PieceView>,
    pub target: Vec<PieceView>,
    pub started: Option<Instant>,
    pub camera_from_rows: f32,
}

impl BoardTransition {
    pub fn new(before: &Engine, after: &Engine, played: Move) -> Option<Self> {
        Self::between(
            before,
            after,
            played.t,
            played.direction.step(played.t)?,
            Some(played),
        )
    }

    pub fn between(
        before: &Engine,
        after: &Engine,
        from_t: usize,
        to_t: usize,
        played: Option<Move>,
    ) -> Option<Self> {
        before.preboard(from_t)?;
        after.preboard(to_t)?;
        let source = pieces(before, from_t);
        let target = pieces(after, to_t);
        let mut used_source = HashSet::new();
        let mut used_target = HashSet::new();
        let mut tracks = Vec::new();
        let mut edges = Vec::new();
        // At a temporal pivot an occurrence touches two edges crossing the
        // same board boundary. Collect them before claiming any endpoints:
        // causal array order would let a passive edge steal a moving piece.
        for (snapshot, engine) in [after, before].into_iter().enumerate() {
            for (wid, line) in engine.history().worldlines().iter().enumerate() {
                for (age, pair) in line.windows(2).enumerate() {
                    if line
                        .gaps()
                        .iter()
                        .any(|gap| gap.after == MoveId::from_state(&pair[0]))
                    {
                        continue;
                    }
                    let a = PieceView::from_state(&pair[0], wid, age);
                    let b = PieceView::from_state(&pair[1], wid, age + 1);
                    let (from, to) = if a.t == from_t && b.t == to_t {
                        (a, b)
                    } else if b.t == from_t && a.t == to_t {
                        (b, a)
                    } else {
                        continue;
                    };
                    let from_id = identity(from);
                    let to_id = identity(to);
                    let visible_from = source.iter().find(|p| identity(**p) == from_id);
                    let visible_to = target.iter().find(|p| identity(**p) == to_id);
                    if visible_from.is_none() && visible_to.is_none() {
                        continue;
                    }
                    let command = recorded_edge(engine, a, b);
                    let rank = if played.is_some() && command == played {
                        0
                    } else if command.is_some() {
                        1
                    } else {
                        2
                    };
                    let reversed_move = a.t == to_t && command.is_some();
                    edges.push((
                        (
                            snapshot,
                            rank,
                            visible_from.is_none() || visible_to.is_none(),
                        ),
                        PieceMotion {
                            from: visible_from.copied().unwrap_or(from),
                            to: visible_to.copied().unwrap_or(to),
                            visible_from: visible_from.is_some(),
                            visible_to: visible_to.is_some(),
                            reversed_move,
                        },
                    ));
                }
            }
        }
        // The played move and other recorded moves (also reversed ones) win
        // over passive continuations. Previous-history edges are only fallback.
        edges.sort_by_key(|(priority, _)| *priority);
        for (_, track) in edges {
            let from_id = identity(track.from);
            let to_id = identity(track.to);
            if used_source.contains(&from_id) || used_target.contains(&to_id) {
                continue;
            }
            used_source.insert(from_id);
            used_target.insert(to_id);
            tracks.push(track);
        }
        // Unmatched incarnations genuinely appear/disappear after history edits.
        // Do not connect two different incarnations merely because they share wid.
        for from in source
            .iter()
            .copied()
            .filter(|p| !used_source.contains(&identity(*p)))
        {
            tracks.push(PieceMotion {
                from,
                to: from,
                visible_from: true,
                visible_to: false,
                reversed_move: false,
            });
        }
        for to in target
            .iter()
            .copied()
            .filter(|p| !used_target.contains(&identity(*p)))
        {
            tracks.push(PieceMotion {
                from: to,
                to,
                visible_from: false,
                visible_to: true,
                reversed_move: false,
            });
        }
        Some(Self {
            from_t,
            to_t,
            source_horizon: before.history().horizon(),
            target_horizon: after.history().horizon(),
            tracks,
            source,
            target,
            started: None,
            camera_from_rows: 0.0,
        })
    }

    pub fn start(&mut self, camera_from_rows: f32) {
        if self.started.is_none() {
            self.started = Some(Instant::now());
            self.camera_from_rows = camera_from_rows;
        }
    }

    pub fn progress(&self) -> f32 {
        self.started
            .map_or(0.0, |start| (start.elapsed().as_secs_f32() / 0.80).min(1.0))
    }
}

pub struct SmoothScroll {
    pub target: usize,
    pub from_rows: f32,
    started: Instant,
    duration: f32,
}

impl SmoothScroll {
    pub fn new(target: usize, from_rows: f32, target_row: usize) -> Self {
        Self {
            target,
            from_rows,
            started: Instant::now(),
            duration: (0.38 + (target_row as f32 - from_rows).abs() * 0.045).min(1.1),
        }
    }

    pub fn progress(&self) -> f32 {
        (self.started.elapsed().as_secs_f32() / self.duration).min(1.0)
    }
}

/// All boards share the side-to-move orientation. Finish spatial travel before
/// fading out, switch square mapping at full transparency, then fade back in.
/// Cache once per egui frame so layout retries cannot change hit testing.
pub struct BoardFacing {
    pub black_bottom: bool,
    pub opacity: f32,
    started: Option<Instant>,
    target_black: bool,
    frame: Option<u64>,
}

impl Default for BoardFacing {
    fn default() -> Self {
        Self {
            black_bottom: false,
            opacity: 1.0,
            started: None,
            target_black: false,
            frame: None,
        }
    }
}

impl BoardFacing {
    pub fn animating(&self) -> bool {
        self.started.is_some()
    }

    pub fn update(&mut self, black_turn: bool, travelling: bool, frame: u64) {
        if self.frame == Some(frame) {
            return;
        }
        self.frame = Some(frame);
        if travelling {
            return;
        }
        if self.started.is_none() && self.black_bottom != black_turn {
            self.target_black = black_turn;
            self.started = Some(Instant::now());
        }
        if let Some(started) = self.started {
            let progress = (started.elapsed().as_secs_f32() / 0.32).clamp(0.0, 1.0);
            if progress >= 0.5 {
                self.black_bottom = self.target_black;
            }
            self.opacity = if progress < 0.5 {
                1.0 - ease(progress * 2.0)
            } else {
                ease(progress * 2.0 - 1.0)
            };
            if progress >= 1.0 {
                self.started = None;
                self.opacity = 1.0;
            }
        }
    }
}
