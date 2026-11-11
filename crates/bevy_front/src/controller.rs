use crate::animation::{BoardTransition, SmoothScroll};
use bevy::{
    prelude::Resource,
    tasks::{AsyncComputeTaskPool, Task, TaskPool, futures::check_ready},
};
use temporal_chess_engine::{
    engine::{
        CalculationStatus, CheckStatus, Engine, GameOutcome, GameStatus, WaitingReason, WinReason,
    },
    model::{
        Color, GameRules, Move, PieceAt, PieceId, PieceView, Sq, StartingPosition, TimeDirection,
    },
    record::{Command, GameRecord},
};

pub fn identity(piece: PieceView) -> PieceAt {
    PieceAt {
        piece: PieceId::from_piv(piece),
        t: piece.t,
    }
}

pub fn side_name(side: Color) -> &'static str {
    match side {
        Color::White => crate::tr!("Белые", "White"),
        Color::Black => crate::tr!("Чёрные", "Black"),
    }
}

pub fn direction_name(direction: TimeDirection) -> &'static str {
    match direction {
        TimeDirection::Future => crate::tr!("В будущее", "Into the future"),
        TimeDirection::Past => crate::tr!("В прошлое", "Into the past"),
    }
}

pub struct Selection {
    pub piece: PieceView,
    pub direction: TimeDirection,
    pub moves: Vec<Move>,
    pub captures: Vec<(Move, PieceView)>,
    pub fatal_captures: Vec<(Move, PieceView)>,
    pub skips: Vec<usize>,
    pub waits: Vec<(Move, usize)>,
}

impl Selection {
    pub fn moves_to(&self, t: usize, square: Sq) -> Vec<Move> {
        self.moves
            .iter()
            .copied()
            .filter(|mv| {
                mv.direction == self.direction
                    && (self.piece.t == t || self.waits.contains(&(*mv, t)))
                    && mv.sq == square
            })
            .collect()
    }

    pub fn skip_to(&self, t: usize, square: Sq) -> bool {
        self.direction == TimeDirection::from_inverted(self.piece.inverted)
            && square == self.piece.sq
            && self.skips.contains(&t)
    }
}

#[derive(Resource)]
pub struct GameUi {
    pub online: crate::multiplayer::OnlineState,
    pub bot: crate::tempfish::BotState,
    pub new_game_bot: crate::tempfish::Settings,
    pub engine: Engine,
    pub checks: CheckStatus,
    pub selected: Option<Selection>,
    pub inspect_mode: bool,
    pub worldline_mode: bool,
    pub worldline_selected: Option<usize>,
    pub inspected: Option<PieceAt>,
    pub promotion: Vec<Move>,
    pub move_wait_until: Option<usize>,
    pub overlapping: Vec<PieceView>,
    pub notice: crate::i18n::Message,
    pub is_error: bool,
    pub cell_size: f32,
    pub jump_to: Option<usize>,
    pub confirm_restart: bool,
    pub new_game_rules: GameRules,
    pub new_game_start: StartingPosition,
    pub viewport: TimelineViewport,
    pub transition: Option<BoardTransition>,
    pub transition_queue: std::collections::VecDeque<BoardTransition>,
    pub remote_focus: Option<u64>,
    pub returning_camera: bool,
    pub playback_turn: Option<Color>,
    pub playback_destination: Option<usize>,
    pub facing: crate::animation::BoardFacing,
    pub local_auto_flip: bool,
    pub record: GameRecord,
    pub import_open: bool,
    pub import_text: String,
    pub import_error: Option<crate::i18n::Message>,
    pub replay_job: Option<ReplayJob>,
    pub pending_square: Option<(usize, Sq, bool)>,
    pub pending_move: Option<Move>,
}

#[derive(Clone, Copy)]
enum ReplayKind {
    Import,
    Network,
    Undo { source_t: usize },
}

pub struct ReplayJob {
    kind: ReplayKind,
    task: Task<Result<(Engine, GameRecord), String>>,
}

/// Retain the visible board when newer boards are inserted above it.
#[derive(Default)]
pub struct TimelineViewport {
    pub times: Vec<usize>,
    pub offset: f32,
    pub stride: f32,
    pub scroll: Option<SmoothScroll>,
    pub frame: Option<u64>,
    pub motion_progress: f32,
    pub focused: Option<usize>,
}

impl TimelineViewport {
    pub fn anchored_offset(&self, times: &[usize], stride: f32) -> f32 {
        if self.stride <= 0.0 || self.times.is_empty() {
            return 0.0;
        }
        let row = (self.offset / self.stride).floor() as usize;
        let row = row.min(self.times.len() - 1);
        let fraction = (self.offset - row as f32 * self.stride) / self.stride;
        times
            .iter()
            .position(|t| *t == self.times[row])
            .map_or(self.offset, |new_row| (new_row as f32 + fraction) * stride)
    }
}

impl Default for GameUi {
    fn default() -> Self {
        Self {
            online: Default::default(),
            bot: Default::default(),
            new_game_bot: Default::default(),
            engine: Engine::default()
                .with_interactive_calculation()
                .expect("valid initial game"),
            checks: CheckStatus::default(),
            selected: None,
            inspect_mode: false,
            worldline_mode: false,
            worldline_selected: None,
            inspected: None,
            promotion: Vec::new(),
            move_wait_until: None,
            overlapping: Vec::new(),
            notice: crate::message!(
                "Выберите фигуру. Здесь играют вдвоём за одним компьютером.",
                "Select a piece. Two players share this computer."
            ),
            is_error: false,
            cell_size: 64.0,
            jump_to: Some(0),
            confirm_restart: false,
            new_game_rules: GameRules::default(),
            new_game_start: StartingPosition::Standard,
            viewport: TimelineViewport::default(),
            transition: None,
            transition_queue: Default::default(),
            remote_focus: None,
            returning_camera: false,
            playback_turn: None,
            playback_destination: None,
            facing: Default::default(),
            local_auto_flip: true,
            record: GameRecord::default(),
            import_open: false,
            import_text: String::new(),
            import_error: None,
            replay_job: None,
            pending_square: None,
            pending_move: None,
        }
    }
}

impl GameUi {
    pub fn playback_busy(&self) -> bool {
        self.transition.is_some()
            || !self.transition_queue.is_empty()
            || self.remote_focus.is_some()
            || self.returning_camera
    }

    pub fn displayed_turn(&self) -> Color {
        self.playback_turn.unwrap_or_else(|| self.engine.turn())
    }

    pub fn finish_travel(&mut self, fallback: usize) {
        let side = if self.online.enabled {
            self.online.side.unwrap_or(self.engine.turn())
        } else if let Some(human) = self.bot.settings.human_side() {
            human
        } else {
            self.engine.turn()
        };
        let destination = self.playback_destination.take().unwrap_or(fallback);
        let target =
            self.times()
                .into_iter()
                .filter(|&t| {
                    self.engine.board(t).is_some()
                        && self.pieces(t).iter().any(|piece| {
                            piece.color == side && piece.can_move() && piece.is_movable()
                        })
                })
                // Equal distances prefer the newer board.
                .min_by_key(|&t| (t.abs_diff(destination), std::cmp::Reverse(t)));
        self.jump_to = Some(target.unwrap_or(destination));
        self.returning_camera = true;
    }

    pub fn reset_playback(&mut self) {
        self.transition = None;
        self.transition_queue.clear();
        self.remote_focus = None;
        self.returning_camera = false;
        self.playback_turn = None;
        self.playback_destination = None;
    }

    pub fn with_start(rules: GameRules, starting_position: StartingPosition) -> Self {
        Self {
            engine: Engine::with_start(rules, starting_position)
                .expect("validated starting position")
                .with_interactive_calculation()
                .expect("initial game"),
            record: GameRecord::with_start(rules, starting_position),
            new_game_start: starting_position,
            new_game_rules: rules,
            ..Self::default()
        }
    }
    pub fn clear_selection(&mut self) {
        self.selected = None;
        self.promotion.clear();
        self.move_wait_until = None;
        self.overlapping.clear();
        self.inspected = None;
    }

    pub fn toggle_direction(&mut self) {
        if let Some(selection) = &mut self.selected {
            selection.direction = match selection.direction {
                TimeDirection::Future => TimeDirection::Past,
                TimeDirection::Past => TimeDirection::Future,
            };
        }
    }

    pub fn times(&self) -> Vec<usize> {
        self.engine.timeline_times().rev().collect()
    }

    fn refresh_checks(&mut self) {
        let mut combined = CheckStatus::default();
        for side in [Color::White, Color::Black] {
            if let Ok(checks) = self.engine.checks(side) {
                combined.ordinary.extend(checks.ordinary);
                combined.inverted.extend(checks.inverted);
            }
        }
        self.checks = combined;
    }

    pub fn active_times(&self) -> Vec<usize> {
        let times: std::collections::BTreeSet<_> =
            self.engine.move_sources().map(|p| p.t).collect();
        times.into_iter().rev().collect()
    }

    pub fn navigate_active(&mut self, next: bool) {
        if self.playback_busy() {
            return;
        }
        let active = self.active_times();
        if active.is_empty() {
            return;
        }
        let current = self
            .jump_to
            .or_else(|| self.viewport.scroll.as_ref().map(|scroll| scroll.target))
            .or(self.viewport.focused)
            .or_else(|| {
                let row = if self.viewport.stride > 0.0 {
                    (self.viewport.offset / self.viewport.stride).round() as usize
                } else {
                    0
                };
                self.viewport
                    .times
                    .get(row.min(self.viewport.times.len().saturating_sub(1)))
                    .copied()
            });
        let index =
            if let Some(index) = current.and_then(|t| active.iter().position(|&time| time == t)) {
                if next {
                    (index + 1) % active.len()
                } else {
                    (index + active.len() - 1) % active.len()
                }
            } else if let Some(current) = current {
                if next {
                    active.iter().position(|&t| t < current).unwrap_or(0)
                } else {
                    active
                        .iter()
                        .rposition(|&t| t > current)
                        .unwrap_or(active.len() - 1)
                }
            } else {
                0
            };
        self.jump_to = Some(active[index]);
    }

    pub fn pieces(&self, t: usize) -> Vec<PieceView> {
        if let Some(preboard) = self.engine.preboard(t) {
            return preboard
                .space
                .iter()
                .flatten()
                .flat_map(|cell| cell.iter().copied())
                .collect();
        }
        Vec::new()
    }

    pub fn selectable(&self, piece: PieceView) -> bool {
        self.can_submit()
            && self.engine.board(piece.t).is_some()
            && piece.color == self.engine.turn()
            && self
                .engine
                .move_sources()
                .any(|p| identity(p) == identity(piece))
    }

    pub fn apply_pending_input(&mut self) {
        if let Some(mv) = self.pending_move.take() {
            self.play(mv);
        }
        if let Some((t, sq, reverse)) = self.pending_square.take() {
            if reverse {
                self.click_square_with_direction(t, sq, true);
            } else {
                self.click_square(t, sq);
            }
        }
    }

    pub fn choose(&mut self, piece: PieceView) {
        self.move_wait_until = None;
        if self.worldline_mode {
            self.worldline_selected = Some(piece.origin);
            self.overlapping.clear();
            return;
        }
        if !self.inspect_mode && !self.selectable(piece) {
            return;
        }
        self.overlapping.clear();
        if self.inspect_mode {
            self.inspected = Some(identity(piece));
            return;
        }
        match self.engine.possible_moves(piece.origin, piece.age) {
            Ok(moves) => {
                let skips = self.engine.possible_skips(identity(piece));
                let mut captures = Vec::new();
                let mut fatal_captures = Vec::new();
                for &mv in &moves {
                    if let Some((target, outcome)) = self.engine.preview_capture(mv) {
                        match outcome {
                            temporal_chess_engine::engine::CaptureOutcome::Captured => {
                                captures.push((mv, target))
                            }
                            temporal_chess_engine::engine::CaptureOutcome::AttackerLost => {
                                fatal_captures.push((mv, target))
                            }
                            temporal_chess_engine::engine::CaptureOutcome::Unresolved => {}
                        }
                    }
                }
                // Promotion choices may have different outcomes. Warn of an
                // unavoidable loss only when no variant can capture this target.
                fatal_captures.retain(|(mv, target)| {
                    !captures.iter().any(|(capture, victim)| {
                        capture.direction == mv.direction
                            && capture.sq == mv.sq
                            && identity(*victim) == identity(*target)
                    })
                });
                let waits = moves
                    .iter()
                    .flat_map(|&mv| {
                        self.engine
                            .possible_move_waits(mv)
                            .unwrap_or_default()
                            .into_iter()
                            .map(move |t| (mv, t))
                    })
                    .collect();
                self.selected = Some(Selection {
                    piece,
                    direction: TimeDirection::from_inverted(piece.inverted),
                    moves,
                    captures,
                    fatal_captures,
                    skips,
                    waits,
                });
                self.message(
                    crate::message!("Ход на исходной доске — без ожидания. На другой подсвеченной доске — ход с ожиданием до неё. СКМ — направление.", "Move on the source board without waiting, or choose a highlighted board to move and wait until then. Middle click: direction."),
                    false,
                );
            }
            Err(error) => self.message(
                crate::i18n::Message::with_error(
                    "Не удалось получить ходы: ",
                    "Could not get moves: ",
                    error.to_string(),
                ),
                true,
            ),
        }
    }

    pub fn click_square(&mut self, t: usize, sq: Sq) {
        self.click_square_with_direction(t, sq, false);
    }

    fn click_square_with_direction(&mut self, t: usize, sq: Sq, reverse: bool) {
        if !self.promotion.is_empty()
            || self.confirm_restart
            || !self.overlapping.is_empty()
            || self.import_open
            || self.replay_job.is_some()
            || self.playback_busy()
            || self.facing.animating()
        {
            return;
        }
        if reverse && !self.worldline_mode && !self.inspect_mode {
            let pieces: Vec<_> = self.pieces(t).into_iter().filter(|p| p.sq == sq).collect();
            if let Some(piece) = pieces.iter().find(|p| self.selectable(**p)) {
                self.choose(*piece);
                if let Some(selection) = &mut self.selected {
                    selection.direction = TimeDirection::from_inverted(!piece.inverted);
                }
            } else if pieces.is_empty() {
                self.toggle_direction();
            }
            return;
        }
        if self.worldline_mode {
            let inspected_wid = self.worldline_selected;
            let mut origins: Vec<_> = self
                .engine
                .history()
                .worldlines()
                .iter()
                .enumerate()
                .filter_map(|(wid, line)| {
                    line.iter()
                        .any(|state| {
                            state.t == t
                                && Sq(state.x, state.y) == sq
                                && (state.alive || inspected_wid == Some(wid))
                        })
                        .then_some(wid)
                })
                .collect();
            origins.sort_unstable();
            origins.dedup();
            if !origins.is_empty() {
                let next = self
                    .worldline_selected
                    .and_then(|wid| origins.iter().position(|&p| p == wid))
                    .map_or(0, |i| (i + 1) % origins.len());
                self.worldline_selected = Some(origins[next]);
            }
            return;
        }
        if !self.inspect_mode
            && let Some(selection) = &self.selected
        {
            let moves = selection.moves_to(t, sq);
            self.move_wait_until = (t != selection.piece.t && !moves.is_empty()).then_some(t);
            if moves.len() > 1 {
                self.promotion = moves;
                return;
            }
            if let Some(mv) = moves.first() {
                self.play(*mv);
                return;
            }
            if selection.skip_to(t, sq) {
                let source = identity(selection.piece);
                let command = Command::Advance {
                    source,
                    from: selection.piece.sq,
                    to_t: t,
                };
                self.submit(command);
                return;
            }
            let clicked_selected = selection.piece.t == t && selection.piece.sq == sq;
            self.clear_selection();
            if clicked_selected {
                return;
            }
        }
        if self.engine.preboard(t).is_none() {
            return;
        }
        let pieces: Vec<_> = self
            .pieces(t)
            .into_iter()
            .filter(|p| p.sq == sq && (self.inspect_mode || self.selectable(*p)))
            .collect();
        match pieces.len() {
            0 => {}
            1 => self.choose(pieces[0]),
            _ => self.overlapping = pieces,
        }
    }

    pub fn can_submit(&self) -> bool {
        !self.playback_busy()
            && !self.facing.animating()
            && !self.bot.owns_turn(self)
            && (!self.online.enabled
                || (self.online.ready
                    && !self.online.waiting
                    && !self.online.synchronizing
                    && self.online.opponent_connected()
                    && self.online.side == Some(self.engine.turn())))
    }

    fn submit(&mut self, command: Command) {
        if !self.can_submit() || self.replay_job.is_some() || self.playback_busy() {
            return;
        }
        if self.online.enabled {
            self.online.outgoing = Some(command);
            self.online.waiting = true;
            self.clear_selection();
        } else {
            let _ = self.apply_confirmed(command);
        }
    }

    pub fn play(&mut self, mv: Move) {
        if let Some(state) = self.engine.piece_state(mv.source()) {
            let from = Sq(state.x, state.y);
            let command = match self.move_wait_until.take() {
                Some(until) => Command::MoveAndWait { mv, from, until },
                None => Command::Move { mv, from },
            };
            self.submit(command);
        }
    }

    /// Shared local/server-confirmed application path. Network requests never
    /// mutate the game optimistically: only acknowledged commands reach here.
    pub fn apply_confirmed(&mut self, command: Command) -> Result<(), String> {
        let before = command
            .spatial_move()
            .is_some()
            .then(|| self.engine.clone());
        let result = self.record.apply(&mut self.engine, command);
        self.finish_confirmed(command, before, result)
    }

    /// Publish the isolated result already approved by this client, after both
    /// players have voted and the server has durably appended the command.
    pub fn apply_prepared(&mut self, command: Command, engine: Engine, record: GameRecord) {
        let before = command
            .spatial_move()
            .is_some()
            .then(|| self.engine.clone());
        self.engine = engine;
        self.record = record;
        let _ = self.finish_confirmed(command, before, Ok(()));
    }

    fn finish_confirmed(
        &mut self,
        command: Command,
        before: Option<Engine>,
        result: Result<(), temporal_chess_engine::engine::EngineErr>,
    ) -> Result<(), String> {
        let error = result.as_ref().err().map(ToString::to_string);
        if let (Ok(()), Some(mv), Some(before)) = (&result, command.spatial_move(), before) {
            self.transition = if matches!(
                self.engine.outcome(),
                Some(GameOutcome::Victory {
                    reason: WinReason::Paradox,
                    ..
                })
            ) {
                None
            } else {
                BoardTransition::new(&before, &self.engine, mv)
            };
            self.playback_turn = Some(before.turn());
            self.playback_destination = command.wait_until().or_else(|| mv.direction.step(mv.t));
            self.remote_focus = None;
            self.transition_queue.clear();
            if let Some(first) = &self.transition
                && let Some(next_t) = self
                    .engine
                    .history()
                    .worldlines()
                    .get(mv.pid.wid)
                    .and_then(|line| {
                        line.windows(3)
                            .find(|states| {
                                states[0].t == mv.t
                                    && temporal_chess_engine::model::PieceId::from_state(
                                        &states[0], mv.pid.wid,
                                    ) == mv.pid
                                    && states[1].t == first.to_t
                            })
                            .map(|states| states[2].t)
                    })
                    .or_else(|| mv.direction.step(first.to_t))
                && let Some(second) =
                    BoardTransition::between(&self.engine, &self.engine, first.to_t, next_t, None)
            {
                self.transition_queue.push_back(second);
            }
            self.viewport.scroll = None;
            self.jump_to = None;
            if self.transition.is_none() {
                self.finish_travel(mv.t);
            }
        }
        self.after_command(result, command.to_string());
        error.map_or(Ok(()), Err)
    }

    fn after_command(
        &mut self,
        result: Result<(), temporal_chess_engine::engine::EngineErr>,
        text: String,
    ) {
        match result {
            Ok(()) => {
                self.refresh_checks();
                self.clear_selection();
                self.message(text, false);
            }
            Err(error) => {
                self.promotion.clear();
                let message = if error.is_internal() {
                    crate::i18n::Message::from(error.to_string())
                } else {
                    crate::i18n::Message::with_error(
                        "Ход не принят: ",
                        "Move rejected: ",
                        error.to_string(),
                    )
                };
                self.message(message, true);
            }
        }
    }

    pub fn continue_calculation(&mut self) {
        self.submit(Command::Continue);
    }

    pub fn decline_reactions(&mut self) {
        self.submit(Command::DeclineReactions);
    }

    pub fn begin_network_replay(&mut self, history: &[String]) {
        self.clear_selection();
        self.pending_square = None;
        self.pending_move = None;
        self.import_open = false;
        self.confirm_restart = false;
        self.begin_replay(
            format!(
                "{}{}\n",
                GameRecord::with_start(self.online.rules, self.online.starting_position).to_text(),
                history.join("\n")
            ),
            ReplayKind::Network,
        );
    }

    pub fn begin_import(&mut self) {
        if self.online.enabled || self.replay_job.is_some() || self.playback_busy() {
            return;
        }
        self.import_error = None;
        self.begin_replay(self.import_text.clone(), ReplayKind::Import);
    }

    pub fn can_undo(&self) -> bool {
        !self.online.enabled
            && !self.facing.animating()
            && self.replay_job.is_none()
            && !self.playback_busy()
            && !self.import_open
            && !self.confirm_restart
            && self
                .record
                .commands()
                .iter()
                .any(|command| command.spatial_move().is_some())
    }

    pub fn undo_last_move(&mut self) {
        if !self.can_undo() {
            return;
        }
        let Some((prefix, removed)) = self.record.without_last_move() else {
            return;
        };
        self.clear_selection();
        // Do not immediately replay the just-undone bot move.
        if self.bot.settings.enabled {
            self.bot.paused = true;
        }
        self.begin_replay(
            prefix.to_text(),
            ReplayKind::Undo {
                source_t: removed.t,
            },
        );
    }

    pub fn undo_in_progress(&self) -> bool {
        self.replay_job
            .as_ref()
            .is_some_and(|job| matches!(job.kind, ReplayKind::Undo { .. }))
    }

    fn begin_replay(&mut self, text: String, kind: ReplayKind) {
        self.reset_playback();
        self.viewport.scroll = None;
        let task = AsyncComputeTaskPool::get_or_init(TaskPool::new).spawn(async move {
            // A failed replay must leave the live game intact, including a panic
            // in historical validation. The task owns only the imported text.
            std::panic::catch_unwind(|| GameRecord::load(&text))
                .map_err(|_| "Could not finish restoring the game".to_owned())?
                .map_err(|error| error.to_string())
        });
        self.replay_job = Some(ReplayJob { kind, task });
    }

    fn replay_failed(&mut self, kind: ReplayKind, error: String) {
        self.replay_job = None;
        match kind {
            ReplayKind::Network => {
                self.online.ready = false;
                self.online.fatal = true;
                self.online.status = crate::i18n::Message::with_error(
                    "Не удалось восстановить сетевую партию: ",
                    "Could not restore the online game: ",
                    error.to_string(),
                );
            }
            ReplayKind::Import => self.import_error = Some(error.into()),
            ReplayKind::Undo { .. } => self.message(
                crate::i18n::Message::with_error(
                    "Отмена не выполнена: ",
                    "Undo failed: ",
                    error.to_string(),
                ),
                true,
            ),
        }
    }

    pub fn poll_replay(&mut self) {
        let Some(job) = &mut self.replay_job else {
            return;
        };
        let kind = job.kind;
        let result = check_ready(&mut job.task);
        match result {
            Some(Ok((engine, record))) => {
                self.clear_selection();
                self.engine = engine;
                self.record = record;
                self.refresh_checks();
                self.jump_to = Some(match kind {
                    ReplayKind::Undo { source_t } => source_t.min(self.engine.history().horizon()),
                    ReplayKind::Import | ReplayKind::Network => {
                        match self.engine.calculation_status() {
                            CalculationStatus::Paused { t, .. } => t,
                            CalculationStatus::Complete => {
                                if self
                                    .engine
                                    .history()
                                    .starting_position()
                                    .fixed_end()
                                    .is_some()
                                {
                                    let side = self.online.side.unwrap_or(self.engine.turn());
                                    let times = self
                                        .engine
                                        .history()
                                        .worldlines()
                                        .iter()
                                        .flat_map(|l| l.iter())
                                        .filter(|s| {
                                            s.alive
                                                && s.color == side
                                                && Color::from_board(s.t) == side
                                                && s.rights.is_movable()
                                        })
                                        .map(|s| s.t);
                                    if side == Color::White {
                                        times.min().unwrap_or(0)
                                    } else {
                                        times.max().unwrap_or(self.engine.history().horizon())
                                    }
                                } else {
                                    self.engine.history().horizon()
                                }
                            }
                        }
                    }
                });
                self.viewport = TimelineViewport::default();
                self.replay_job = None;
                if matches!(kind, ReplayKind::Network) {
                    self.online.synchronizing = false;
                }
                self.import_open = false;
                self.inspect_mode = false;
                self.worldline_mode = false;
                self.worldline_selected = None;
                self.message(
                    match kind {
                        ReplayKind::Network => crate::message!(
                            "Сетевая партия синхронизирована.",
                            "Online game synchronized."
                        ),
                        ReplayKind::Import => crate::message!(
                            "Партия загружена. Можно продолжать игру.",
                            "Game loaded. You can continue playing."
                        ),
                        ReplayKind::Undo { .. } => crate::message!(
                            "Последний ход отменён. История восстановлена.",
                            "Last move undone. History restored."
                        ),
                    },
                    false,
                );
            }
            Some(Err(error)) => self.replay_failed(kind, error),
            None => {}
        }
    }

    pub fn message(&mut self, text: impl Into<crate::i18n::Message>, error: bool) {
        self.notice = text.into();
        self.is_error = error;
    }

    pub fn status_text(&self) -> String {
        if self.playback_busy() {
            return crate::tr_format!("Ход: {}", "To move: {}", side_name(self.displayed_turn()));
        }
        match self.engine.status() {
            GameStatus::Ongoing => {
                crate::tr_format!("Ход: {}", "To move: {}", side_name(self.engine.turn()))
            }
            GameStatus::Waiting {
                reason: WaitingReason::NoActivePieces,
                ..
            } => crate::tr!(
                "Ожидание · нет активных фигур",
                "Waiting · no active pieces"
            )
            .into(),
            GameStatus::Waiting { player, .. } => crate::tr_format!(
                "{} ожидают · ходят {}",
                "{} waiting · {} to move",
                side_name(player),
                side_name(self.engine.turn())
            ),
            GameStatus::Finished(GameOutcome::Stalemate) => {
                crate::tr!("Ничья — пат", "Draw — stalemate").into()
            }
            GameStatus::Finished(GameOutcome::Victory { winner, reason }) => crate::tr_format!(
                "Победа: {} · {}",
                "Winner: {} · {}",
                side_name(winner),
                match reason {
                    WinReason::Checkmate => crate::tr!("мат", "checkmate"),
                    WinReason::KingCaptured => crate::tr!("король взят", "king captured"),
                    WinReason::Paradox => crate::tr!("парадокс", "paradox"),
                }
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selecting_destination_board_submits_one_move_with_wait() {
        let mut game = GameUi::with_start(
            GameRules::default(),
            StartingPosition::TimeWar { separation: 9 },
        );
        let before = game.engine.clone();
        game.click_square(0, Sq(4, 1));
        let selection = game.selected.as_ref().unwrap();
        assert!(selection.skips.is_empty());
        assert!(
            selection
                .moves_to(6, Sq(4, 3))
                .iter()
                .any(|mv| !mv.to_past())
        );
        assert_eq!(game.engine, before);
        game.click_square(6, Sq(4, 3));
        assert!(matches!(
            game.record.commands(),
            [Command::MoveAndWait { until: 6, .. }]
        ));
        assert_eq!(game.engine.turn(), Color::Black);
        assert_eq!(game.playback_destination, Some(6));
        assert!(game.engine.board(6).unwrap().get_piece(4, 3).is_some());
        assert_eq!(
            GameRecord::load(&game.record.to_text()).unwrap().0,
            game.engine
        );
    }

    #[test]
    fn camera_prefers_active_move_arrival_after_both_transitions() {
        let (engine, record) = GameRecord::load("NOLAN-CHESS 2\ncausal_gaps off\nmove 0 12:0:0 e2 e4 + normal\nmove 1 27:1:0 d7 d5 + normal\nmove 2 6:2:0 g1 f3 + normal\nmove 3 18:3:0 c8 f5 - normal\nmove 4 6:4:0 f3 e5 - normal\nmove 1 18:3:1 f5 h3 + normal\nadvance 2 6:4:1 e5 0").unwrap();
        let mut game = GameUi {
            engine,
            record,
            ..Default::default()
        };
        let piece = *game.engine.board(0).unwrap().get_piece(4, 4).unwrap();
        let mv = game
            .engine
            .possible_moves(piece.origin, piece.age)
            .unwrap()
            .into_iter()
            .find(|mv| mv.sq == Sq(6, 3) && !mv.to_past())
            .unwrap();
        game.apply_confirmed(Command::Move { mv, from: Sq(4, 4) })
            .unwrap();
        assert_eq!(game.playback_destination, Some(1));
        assert!(game.active_times().contains(&1));
        assert!(game.active_times().iter().any(|&t| t > 1));
        game.finish_travel(2);
        assert_eq!(game.jump_to, Some(1));
        // An inactive arrival chooses the nearest active board, not the newest.
        game.playback_destination = Some(0);
        game.finish_travel(2);
        assert_eq!(game.jump_to, Some(1));
        game.playback_destination = game.active_times().first().copied();
        game.finish_travel(0);
        assert_eq!(game.jump_to, game.active_times().first().copied());
    }

    #[test]
    fn invalid_left_click_clears_or_selects_another_active_piece() {
        for square in [Sq(4, 1), Sq(3, 1), Sq(4, 6), Sq(0, 3)] {
            let mut game = GameUi::default();
            game.click_square(0, Sq(4, 1));
            game.click_square(0, square);
            if square == Sq(3, 1) {
                assert_eq!(game.selected.as_ref().unwrap().piece.sq, square);
            } else {
                assert!(game.selected.is_none());
            }
            assert!(game.record.commands().is_empty());
        }
    }

    #[test]
    fn middle_click_selects_opposite_to_piece_orientation_without_playing() {
        let mut game = GameUi::default();
        game.click_square(0, Sq(4, 1));
        game.click_square_with_direction(0, Sq(3, 1), true);
        assert_eq!(game.selected.as_ref().unwrap().piece.sq, Sq(3, 1));
        assert_eq!(
            game.selected.as_ref().unwrap().direction,
            TimeDirection::Past
        );
        game.click_square_with_direction(0, Sq(3, 1), true);
        assert_eq!(
            game.selected.as_ref().unwrap().direction,
            TimeDirection::Past
        );
        assert!(game.record.commands().is_empty());
        game.engine = Engine::with_start(
            Default::default(),
            temporal_chess_engine::model::StartingPosition::TimeWar { separation: 9 },
        )
        .unwrap();
        let pawn = *game.engine.board(0).unwrap().get_piece(4, 1).unwrap();
        let mv = game
            .engine
            .possible_moves(pawn.origin, pawn.age)
            .unwrap()
            .into_iter()
            .find(|mv| mv.sq == Sq(4, 3))
            .unwrap();
        game.engine.apply_new_mv(mv).unwrap();
        game.clear_selection();
        game.click_square_with_direction(9, Sq(4, 6), true);
        assert_eq!(
            game.selected.as_ref().unwrap().direction,
            TimeDirection::Future
        );
        assert!(game.selected.as_ref().unwrap().piece.inverted);
    }

    #[test]
    fn selecting_playing_and_cancelling_use_stable_occurrences() {
        let mut game = GameUi::default();
        game.click_square(0, Sq(4, 1));
        assert_eq!(game.times(), [0]);
        assert!(
            !game
                .selected
                .as_ref()
                .unwrap()
                .moves_to(0, Sq(4, 3))
                .is_empty()
        );
        assert!(
            game.selected
                .as_ref()
                .unwrap()
                .moves_to(1, Sq(4, 3))
                .is_empty()
        );
        game.toggle_direction();
        assert_eq!(game.times(), [0]);
        game.toggle_direction();
        game.click_square(0, Sq(4, 3));
        assert_eq!(game.engine.turn(), Color::Black);
        assert_eq!(game.engine.history().horizon(), 1);
        assert!(game.selected.is_none());
        game.inspect_mode = true;
        game.click_square(0, Sq(4, 1));
        let inspected = game.inspected.unwrap();
        assert!(game.engine.piece_state(inspected).is_some());
        game.clear_selection();
        assert!(game.inspected.is_none());
    }

    #[test]
    fn hotseat_inversion_defaults_to_backward_on_the_new_incarnation() {
        let mut game = GameUi::default();
        for (t, from, to) in [(0, Sq(4, 1), Sq(4, 3)), (1, Sq(4, 6), Sq(4, 4))] {
            game.click_square(t, from);
            game.click_square(t, to);
        }
        game.click_square(2, Sq(6, 0));
        game.toggle_direction();
        assert_eq!(
            game.selected.as_ref().unwrap().direction,
            TimeDirection::Past
        );
        game.click_square(2, Sq(7, 2));
        assert_eq!(game.jump_to, Some(1));
        assert_eq!(game.engine.turn(), Color::Black);
        game.click_square(3, Sq(1, 7));
        game.click_square(3, Sq(2, 5));
        assert_eq!(game.jump_to, Some(4));
        game.click_square(0, Sq(7, 2));
        assert!(game.selected.as_ref().unwrap().piece.inverted);
        assert_eq!(
            game.selected.as_ref().unwrap().direction,
            TimeDirection::Past
        );
    }

    #[test]
    fn waiting_on_another_board_still_has_no_turn_cost() {
        use temporal_chess_engine::model::{PieceKind, State};
        let mut line = vec![State::new(PieceKind::N, Color::White, Sq(1, 0))];
        for _ in 0..6 {
            line.push(line.last().unwrap().prolongate().unwrap());
        }
        let mut game = GameUi {
            engine: Engine::from_worldlines_with_rules(
                vec![line],
                Color::White,
                GameRules {
                    wait_before_move: true,
                    ..Default::default()
                },
            )
            .unwrap(),
            ..GameUi::default()
        };
        game.jump_to = None;
        game.click_square(2, Sq(1, 0));
        assert_eq!(game.selected.as_ref().unwrap().skips, [4, 6]);
        game.click_square(4, Sq(1, 0));
        assert_eq!(game.engine.turn(), Color::White);
        assert_eq!(game.engine.history().horizon(), 6);
        assert!(!game.engine.history().worldlines()[0][2].rights.ordinary);
        assert!(game.engine.history().worldlines()[0][4].rights.ordinary);
        assert!(game.jump_to.is_none());
    }
    fn finish_import(game: &mut GameUi) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while game.replay_job.is_some() {
            game.poll_replay();
            assert!(std::time::Instant::now() < deadline, "import timed out");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[test]
    fn import_is_atomic_and_successful_import_can_be_played_and_exported_again() {
        let mut game = GameUi::default();
        game.click_square(0, Sq(4, 1));
        game.click_square(0, Sq(4, 3));
        let before = game.engine.clone();
        let record = game.record.clone();
        game.import_open = true;
        game.import_text = "TEMPORAL-CHESS 1\nmove 0 12:0:0 e2 e5 + normal".into();
        game.begin_import();
        finish_import(&mut game);
        assert_eq!(game.engine, before);
        assert_eq!(game.record, record);
        assert!(game.import_error.as_ref().unwrap().text().contains("2"));
        assert!(game.import_open);
        game.import_text = record.to_text();
        game.begin_import();
        finish_import(&mut game);
        assert!(!game.import_open);
        assert_eq!(game.engine, before);
        game.click_square(1, Sq(4, 6));
        game.click_square(1, Sq(4, 4));
        assert_eq!(game.record.commands().len(), 2);
        assert_eq!(
            GameRecord::load(&game.record.to_text()).unwrap().0,
            game.engine
        );
    }
    #[test]
    fn time_war_selection_uses_inversion_and_bounds_without_growing_boards() {
        let mut game = GameUi::with_start(
            GameRules::default(),
            StartingPosition::TimeWar { separation: 9 },
        );
        assert_eq!(game.times(), (0..=9).rev().collect::<Vec<_>>());
        assert_eq!(game.jump_to, Some(0));
        game.choose(*game.engine.board(0).unwrap().get_piece(4, 1).unwrap());
        assert_eq!(
            game.selected.as_ref().unwrap().direction,
            TimeDirection::Future
        );
        let mv = *game
            .selected
            .as_ref()
            .unwrap()
            .moves
            .iter()
            .find(|m| m.sq == Sq(4, 3))
            .unwrap();
        game.record
            .apply(&mut game.engine, Command::Move { mv, from: Sq(4, 1) })
            .unwrap();
        game.clear_selection();
        game.choose(*game.engine.board(9).unwrap().get_piece(4, 6).unwrap());
        let selected = game.selected.as_ref().unwrap();
        assert_eq!(selected.direction, TimeDirection::Past);
        assert!(!selected.moves.is_empty());
        assert!(selected.moves.iter().all(|m| m.to_past()));
        assert_eq!(
            GameRecord::load(&game.record.to_text()).unwrap().0,
            game.engine
        );
    }

    #[test]
    fn time_war_network_replay_focuses_the_players_army() {
        for (side, t) in [(Color::White, 0), (Color::Black, 9)] {
            let mut game = GameUi::default();
            game.online.enabled = true;
            game.online.side = Some(side);
            game.online.starting_position = StartingPosition::TimeWar { separation: 9 };
            game.begin_network_replay(&[]);
            finish_import(&mut game);
            assert!(!game.online.fatal, "{}", game.online.status.text());
            assert_eq!(game.jump_to, Some(t));
            assert_eq!(
                game.engine.history().starting_position(),
                game.online.starting_position
            );
            assert_eq!(
                game.record.starting_position(),
                game.online.starting_position
            );
        }
    }
}
