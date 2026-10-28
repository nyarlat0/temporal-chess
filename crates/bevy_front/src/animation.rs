//! Presentation-only transitions. Worldline edges, not worldline IDs alone,
//! connect occurrences across a board boundary (including reversed moves).
use std::{collections::HashSet, time::Instant};
use temporal_chess_engine::{
    engine::Engine,
    model::{Castling, Move, MoveId, PieceAt, PieceId, PieceKind, PieceView},
};

pub fn ease(progress: f32) -> f32 {
    let t = progress.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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
    pub played: Move,
    pub tracks: Vec<PieceMotion>,
    pub started: Option<Instant>,
    pub camera_from_rows: f32,
}

impl BoardTransition {
    pub fn new(before: &Engine, after: &Engine, played: Move) -> Option<Self> {
        let from_t = played.t;
        let to_t = played.direction.step(from_t)?;
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
                    let rank = if command == Some(played) {
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
            .into_iter()
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
            .into_iter()
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
            played,
            tracks,
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
