//! Presentation-only transitions. Worldline edges, not worldline IDs alone,
//! connect occurrences across a board boundary (including reversed moves).
use std::collections::HashSet;
use temporal_chess_engine::{
    engine::Engine,
    model::{Castling, Move, MoveId, PieceAt, PieceId, PieceKind, PieceView},
};
use web_time::Instant;

pub const BASE_ANIMATION_SPEED: f32 = 1.3;

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

#[derive(Clone, Default)]
struct AnimationClock {
    last: Option<Instant>,
    elapsed: f32,
}
impl AnimationClock {
    fn advance(&mut self, seconds: f32, speed: f32) {
        self.elapsed += seconds * speed.clamp(0.25, 3.0) * BASE_ANIMATION_SPEED;
    }
    fn progress(&mut self, duration: f32, speed: f32) -> f32 {
        let now = Instant::now();
        if let Some(last) = self.last.replace(now) {
            self.advance((now - last).as_secs_f32(), speed);
        }
        (self.elapsed / duration).clamp(0.0, 1.0)
    }
    fn start(&mut self) {
        self.elapsed = 0.0;
        self.last = Some(Instant::now());
    }
}

#[derive(Clone)]
pub struct PieceMotion {
    pub from: PieceView,
    pub to: PieceView,
    pub visible_from: bool,
    pub visible_to: bool,
    pub reversed_move: bool,
    pub move_direction: Option<temporal_chess_engine::model::TimeDirection>,
    pub dying: bool,
    pub fall_sign: f32,
}

#[derive(Clone)]
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
    clock: AnimationClock,
}

impl BoardTransition {
    pub fn command_path(
        after: &Engine,
        command: temporal_chess_engine::record::Command,
    ) -> Vec<usize> {
        use temporal_chess_engine::record::Command;
        let (source, minimum, until) = match command {
            Command::Move { mv, .. } => (mv.source(), 2, None),
            Command::MoveAndWait { mv, until, .. } => (mv.source(), 2, Some(until)),
            Command::Advance { source, to_t, .. } => (source, 1, Some(to_t)),
            _ => return vec![],
        };
        if let Some(line) = after.history().worldlines().get(source.piece.wid)
            && let Some(age) = line.iter().position(|s| {
                s.t == source.t && PieceId::from_state(s, source.piece.wid) == source.piece
            })
        {
            let mut path = vec![source.t];
            let mut reached = until.is_none();
            for (offset, state) in line.iter().skip(age + 1).enumerate() {
                path.push(state.t);
                reached |= until == Some(state.t);
                if reached
                    && offset + 1 >= minimum
                    && !state.is_loop_exit()
                    && temporal_chess_engine::model::Color::from_board(state.t) == state.color
                    && (state.rights.ordinary
                        || line
                            .get(age + offset + 2)
                            .is_none_or(|s| s.x != state.x || s.y != state.y))
                {
                    break;
                }
            }
            return path;
        }
        // A legal command can erase its own source. Still show its original
        // adjacent-board motion, without inventing a new route in the engine.
        if let Some(mv) = command.spatial_move() {
            let mut path = vec![mv.t];
            if let Some(t) = mv
                .direction
                .step(mv.t)
                .filter(|t| after.preboard(*t).is_some())
            {
                path.push(t);
                if let Some(t) = mv
                    .direction
                    .step(t)
                    .filter(|t| after.preboard(*t).is_some())
                {
                    path.push(t);
                }
            }
            return path;
        }
        vec![]
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
                    let (from, to, dead_to) = if a.t == from_t && b.t == to_t {
                        (a, b, !pair[1].alive)
                    } else if b.t == from_t && a.t == to_t {
                        (b, a, !pair[0].alive)
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
                            move_direction: command.map(|mv| mv.direction),
                            dying: visible_from.is_some() && dead_to,
                            fall_sign: fall_sign(),
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
                move_direction: None,
                dying: after.piece_state(identity(from)).is_some_and(|s| !s.alive),
                fall_sign: fall_sign(),
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
                move_direction: None,
                dying: false,
                fall_sign: fall_sign(),
            });
        }
        // Captures are checked on the adjacent board. A victim can exist only
        // there, with no occurrence on the source board (e.g. Time War).
        for victim in pieces(before, to_t) {
            if after
                .piece_state(identity(victim))
                .is_some_and(|s| !s.alive)
                && !tracks
                    .iter()
                    .any(|track| track.dying && identity(track.to) == identity(victim))
            {
                tracks.push(PieceMotion {
                    from: victim,
                    to: victim,
                    visible_from: true,
                    visible_to: false,
                    reversed_move: false,
                    move_direction: None,
                    dying: true,
                    fall_sign: fall_sign(),
                });
            }
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
            clock: Default::default(),
        })
    }

    pub fn start(&mut self, camera_from_rows: f32) {
        if self.started.is_none() {
            self.started = Some(Instant::now());
            self.clock.start();
            self.camera_from_rows = camera_from_rows;
        }
    }

    pub fn travel_fraction(&self) -> f32 {
        if self.tracks.iter().any(|t| t.dying) {
            1.6 / 2.6
        } else {
            0.88
        }
    }
    fn duration(&self) -> f32 {
        if self.tracks.iter().any(|t| t.dying) {
            2.6
        } else {
            1.8
        }
    }

    pub fn progress(&mut self, speed: f32) -> f32 {
        let duration = self.duration();
        if self.started.is_none() {
            0.0
        } else {
            self.clock.progress(duration, speed)
        }
    }
    pub fn replay_sounds(
        &self,
    ) -> impl Iterator<Item = temporal_chess_engine::model::TimeDirection> + '_ {
        self.tracks
            .iter()
            .filter(|t| t.from.sq != t.to.sq)
            .filter_map(|t| t.move_direction)
    }
}

/// A short critically damped coast for mouse-wheel navigation.
#[derive(Default)]
pub struct WheelInertia {
    target: Option<f32>,
    velocity: f32,
}
impl WheelInertia {
    pub fn active(&self) -> bool {
        self.target.is_some()
    }
    pub fn stop(&mut self) {
        self.target = None;
        self.velocity = 0.0;
    }
    pub fn update(&mut self, offset: f32, delta: f32, dt: f32, maximum: f32) -> f32 {
        if delta != 0.0 {
            let reversing = self.velocity * delta < 0.0;
            if reversing {
                self.stop();
            }
            let gain = 1.0 + (self.velocity.abs() / 1800.0).min(0.45);
            self.target = Some((self.target.unwrap_or(offset) + delta * gain).clamp(0.0, maximum));
        }
        let Some(target) = self.target else {
            return offset;
        };
        let target = target.clamp(0.0, maximum);
        self.target = Some(target);
        let dt = dt.clamp(0.0, 0.05);
        let omega = 18.0;
        let difference = offset - target;
        let decay = (-omega * dt).exp();
        let blend = self.velocity + omega * difference;
        let next = (target + (difference + blend * dt) * decay).clamp(0.0, maximum);
        self.velocity = (self.velocity - omega * blend * dt) * decay;
        if (next - target).abs() < 0.01 && self.velocity.abs() < 0.1 {
            self.stop();
            // Preserve the last integrated position instead of snapping the
            // whole board to the target on the frame when repainting stops.
            next
        } else {
            next
        }
    }
}

pub struct SmoothScroll {
    pub target: usize,
    pub from_rows: f32,
    clock: AnimationClock,
    duration: f32,
}

impl SmoothScroll {
    pub fn new(target: usize, from_rows: f32, target_row: usize) -> Self {
        Self {
            target,
            from_rows,
            clock: {
                let mut clock = AnimationClock::default();
                clock.start();
                clock
            },
            duration: (0.75 + (target_row as f32 - from_rows).abs() * 0.08).min(2.0),
        }
    }

    pub fn progress(&mut self, speed: f32) -> f32 {
        self.clock.progress(self.duration, speed)
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
    clock: AnimationClock,
}

impl Default for BoardFacing {
    fn default() -> Self {
        Self {
            black_bottom: false,
            opacity: 1.0,
            started: None,
            target_black: false,
            frame: None,
            clock: Default::default(),
        }
    }
}

impl BoardFacing {
    pub fn animating(&self) -> bool {
        self.started.is_some()
    }

    pub fn update(&mut self, black_turn: bool, travelling: bool, frame: u64, speed: f32) {
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
            self.clock.start();
        }
        if self.started.is_some() {
            let progress = self.clock.progress(0.65, speed);
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

fn fall_sign() -> f32 {
    #[cfg(not(target_arch = "wasm32"))]
    let right = fastrand::bool();
    #[cfg(target_arch = "wasm32")]
    let right = {
        use wasm_bindgen::prelude::*;
        #[wasm_bindgen]
        unsafe extern "C" {
            #[wasm_bindgen(js_namespace = Math)]
            fn random() -> f64;
        }
        random() >= 0.5
    };
    if right { 1.0 } else { -1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use temporal_chess_engine::{
        model::{Color, GameRules, Sq, StartingPosition, State},
        record::{Command, GameRecord},
    };

    #[test]
    fn wheel_coasts_without_overshoot_reverses_promptly_and_stops_at_bounds() {
        let mut wheel = WheelInertia::default();
        let mut offset = wheel.update(200.0, 120.0, 0.0, 1000.0);
        assert!(wheel.active());
        for _ in 0..5 {
            let next = wheel.update(offset, 0.0, 1.0 / 60.0, 1000.0);
            assert!(next >= offset && next <= 320.0);
            offset = next;
        }
        let reverse = wheel.update(offset, -100.0, 1.0 / 60.0, 1000.0);
        assert!(reverse < offset);
        offset = reverse;
        for _ in 0..120 {
            offset = wheel.update(offset, 0.0, 1.0 / 60.0, 1000.0);
        }
        assert!(!wheel.active());
        offset = wheel.update(offset, 10000.0, 1.0 / 60.0, 1000.0);
        // Shrinking the scrollable range while coasting must not leave a
        // permanently active target beyond the new end of the ribbon.
        for _ in 0..120 {
            offset = wheel.update(offset.min(300.0), 0.0, 1.0 / 60.0, 300.0);
        }
        assert_eq!(offset, 300.0);
        assert!(!wheel.active());
    }

    #[test]
    fn wheel_response_is_independent_of_refresh_rate_and_accelerates_repeated_input() {
        let simulate = |steps| {
            let mut wheel = WheelInertia::default();
            let mut offset = wheel.update(0.0, 100.0, 0.0, 1000.0);
            for _ in 0..steps {
                offset = wheel.update(offset, 0.0, 0.1 / steps as f32, 1000.0);
            }
            (offset, wheel)
        };
        let (offset, mut fast) = simulate(12);
        let (other, _) = simulate(6);
        assert!((offset - other).abs() < 0.001);
        fast.update(offset, 100.0, 0.0, 1000.0);
        assert!(fast.target.unwrap() > 200.0);
        fast.stop();
        assert!(!fast.active());
    }

    #[test]
    fn replay_sounds_count_moving_pieces_and_preserve_inverse_direction() {
        let king = State::new(PieceKind::K { moved: false }, Color::White, Sq(4, 0));
        let rook = State::new(PieceKind::R { moved: false }, Color::White, Sq(7, 0));
        let engine = Engine::from_worldlines(vec![vec![king], vec![rook]], Color::White).unwrap();
        let mv = engine
            .possible_moves(0, 0)
            .unwrap()
            .into_iter()
            .find(|m| m.castling().is_some() && !m.to_past())
            .unwrap();
        let after = engine.after_move(mv).unwrap();
        let transition = BoardTransition::between(&after, &after, 0, 1, None).unwrap();
        assert_eq!(
            transition.replay_sounds().collect::<Vec<_>>(),
            [temporal_chess_engine::model::TimeDirection::Future; 2]
        );
        let passive = BoardTransition::between(&after, &after, 1, 2, None);
        if let Some(passive) = passive {
            assert_eq!(passive.replay_sounds().count(), 0);
        }
        let (after, _) = GameRecord::load("NOLAN-CHESS 6\nchain_info_activation off\nreact_multiple off\nfree_wait off\nstart time_war 5\nmove 0 12:0:0 e2 e4 + normal\nmove 5 17:5:0 b8 c6 - normal\n").unwrap();
        let reversed = BoardTransition::between(&after, &after, 4, 5, None).unwrap();
        assert_eq!(
            reversed.replay_sounds().collect::<Vec<_>>(),
            [temporal_chess_engine::model::TimeDirection::Past]
        );
    }

    #[test]
    fn changing_speed_preserves_accumulated_animation_time() {
        let mut clock = AnimationClock::default();
        clock.advance(0.5, 1.0);
        clock.advance(0.0, 3.0);
        assert!((clock.elapsed - 0.65).abs() < 1e-6);
        clock.advance(0.25, 2.0);
        assert!((clock.elapsed - 1.3).abs() < 1e-6);
        clock.advance(1.0, 0.25);
        assert!((clock.elapsed - 1.625).abs() < 1e-6);
    }

    #[test]
    fn move_and_wait_and_free_wait_visit_every_intermediate_board_in_both_directions() {
        let rules = GameRules {
            wait_before_move: true,
            ..Default::default()
        };
        let start = StartingPosition::TimeWar { separation: 9 };
        let mut engine = Engine::with_start(rules, start)
            .unwrap()
            .with_interactive_calculation()
            .unwrap();
        let mut record = GameRecord::with_start(rules, start);
        for (text, path) in [
            (
                "move 0 12:0:0 e2 e4 + normal wait 6",
                vec![0, 1, 2, 3, 4, 5, 6],
            ),
            ("advance 9 28:9:0 e7 3", vec![9, 8, 7, 6, 5, 4, 3]),
        ] {
            let before = engine.clone();
            let command: Command = text.parse().unwrap();
            record.apply(&mut engine, command).unwrap();
            assert_eq!(BoardTransition::command_path(&engine, command), path);
            assert!(path.windows(2).all(|p| {
                BoardTransition::between(&before, &engine, p[0], p[1], command.spatial_move())
                    .is_some()
            }));
        }
    }

    #[test]
    fn a_victim_present_only_on_the_capture_board_gets_a_death_track() {
        let knight = State::new(PieceKind::N, Color::White, Sq(2, 2));
        let mut queen = State::new(PieceKind::Q, Color::Black, Sq(1, 4));
        queen.t = 1;
        queen.fut_seen = 1;
        queen.rights.ordinary = true;
        let before =
            Engine::from_worldlines(vec![vec![knight], vec![queen]], Color::White).unwrap();
        let mv = before
            .possible_moves(0, 0)
            .unwrap()
            .into_iter()
            .find(|m| !m.to_past() && m.sq == Sq(1, 4))
            .unwrap();
        let after = before.after_move(mv).unwrap();
        assert!(!after.history().worldlines()[1].last().unwrap().alive);
        let transition = BoardTransition::between(&before, &after, 0, 1, Some(mv)).unwrap();
        let victim = transition
            .tracks
            .iter()
            .find(|t| t.to.origin == 1 && t.dying)
            .unwrap();
        assert_eq!(victim.to.sq, Sq(1, 4));
        assert!(victim.visible_from && !victim.visible_to);
        assert!(transition.travel_fraction() < 0.7);
    }
}
