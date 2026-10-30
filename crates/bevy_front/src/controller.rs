use crate::animation::{BoardTransition, SmoothScroll};
use bevy::{
    prelude::Resource,
    tasks::{AsyncComputeTaskPool, Task, TaskPool, futures::check_ready},
};
use temporal_chess_engine::{
    engine::{
        CalculationStatus, CheckStatus, Engine, GameOutcome, GameStatus, WaitingReason, WinReason,
    },
    model::{Color, Move, PieceAt, PieceId, PieceView, Sq, TimeDirection},
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
        Color::White => "Белые",
        Color::Black => "Чёрные",
    }
}

pub fn direction_name(direction: TimeDirection) -> &'static str {
    match direction {
        TimeDirection::Future => "В будущее",
        TimeDirection::Past => "В прошлое",
    }
}

pub struct Selection {
    pub piece: PieceView,
    pub direction: TimeDirection,
    pub moves: Vec<Move>,
    pub captures: Vec<(Move, PieceView)>,
    pub fatal_captures: Vec<(Move, PieceView)>,
    pub skips: Vec<usize>,
}

impl Selection {
    pub fn moves_to(&self, t: usize, square: Sq) -> Vec<Move> {
        self.moves
            .iter()
            .copied()
            .filter(|mv| mv.direction == self.direction && self.piece.t == t && mv.sq == square)
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
    pub engine: Engine,
    pub checks: CheckStatus,
    pub selected: Option<Selection>,
    pub inspect_mode: bool,
    pub inspected: Option<PieceAt>,
    pub promotion: Vec<Move>,
    pub overlapping: Vec<PieceView>,
    pub notice: String,
    pub is_error: bool,
    pub cell_size: f32,
    pub jump_to: Option<usize>,
    pub confirm_restart: bool,
    pub new_game_causal_gaps: bool,
    pub viewport: TimelineViewport,
    pub transition: Option<BoardTransition>,
    pub facing: crate::animation::BoardFacing,
    pub record: GameRecord,
    pub import_open: bool,
    pub import_text: String,
    pub import_error: Option<String>,
    pub replay_job: Option<ReplayJob>,
    pub pending_square: Option<(usize, Sq)>,
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
            engine: Engine::default()
                .with_interactive_calculation()
                .expect("valid initial game"),
            checks: CheckStatus::default(),
            selected: None,
            inspect_mode: false,
            inspected: None,
            promotion: Vec::new(),
            overlapping: Vec::new(),
            notice: "Выберите фигуру. Здесь играют вдвоём за одним компьютером.".into(),
            is_error: false,
            cell_size: 64.0,
            jump_to: Some(0),
            confirm_restart: false,
            new_game_causal_gaps: false,
            viewport: TimelineViewport::default(),
            transition: None,
            facing: Default::default(),
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
    pub fn new(causal_gaps: bool) -> Self {
        Self {
            engine: Engine::new(causal_gaps)
                .with_interactive_calculation()
                .expect("initial game"),
            record: GameRecord::new(causal_gaps),
            new_game_causal_gaps: causal_gaps,
            ..Self::default()
        }
    }
    pub fn clear_selection(&mut self) {
        self.selected = None;
        self.promotion.clear();
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
        if self.transition.is_some() {
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
        if let Some((t, sq)) = self.pending_square.take() {
            self.click_square(t, sq);
        }
    }

    pub fn choose(&mut self, piece: PieceView) {
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
                self.selected = Some(Selection {
                    piece,
                    direction: TimeDirection::from_inverted(piece.inverted),
                    moves,
                    captures,
                    fatal_captures,
                    skips,
                });
                self.message(
                    "ЛКМ — ход или ожидание. СКМ — направление. ПКМ — снять выделение.",
                    false,
                );
            }
            Err(error) => self.message(format!("Не удалось получить ходы: {error}"), true),
        }
    }

    pub fn click_square(&mut self, t: usize, sq: Sq) {
        if !self.promotion.is_empty()
            || self.confirm_restart
            || !self.overlapping.is_empty()
            || self.import_open
            || self.replay_job.is_some()
            || self.transition.is_some()
            || self.facing.animating()
        {
            return;
        }
        if !self.inspect_mode
            && let Some(selection) = &self.selected
        {
            let moves = selection.moves_to(t, sq);
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
        !self.facing.animating()
            && (!self.online.enabled
                || (self.online.ready
                    && !self.online.waiting
                    && !self.online.synchronizing
                    && self.online.side == Some(self.engine.turn())))
    }

    fn submit(&mut self, command: Command) {
        if !self.can_submit() || self.replay_job.is_some() || self.transition.is_some() {
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
            self.submit(Command::Move {
                mv,
                from: Sq(state.x, state.y),
            });
        }
    }

    /// Shared local/server-confirmed application path. Network requests never
    /// mutate the game optimistically: only acknowledged commands reach here.
    pub fn apply_confirmed(&mut self, command: Command) -> Result<(), String> {
        let before = matches!(command, Command::Move { .. }).then(|| self.engine.clone());
        let result = self.record.apply(&mut self.engine, command);
        let error = result.as_ref().err().map(ToString::to_string);
        if let (Ok(()), Command::Move { mv, .. }, Some(before)) = (&result, command, before) {
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
            self.viewport.scroll = None;
            self.jump_to = if self.transition.is_none() {
                self.active_times().first().copied()
            } else {
                None
            };
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
                self.message(format!("Ход не принят: {error}"), true);
            }
        }
    }

    pub fn continue_calculation(&mut self) {
        self.submit(Command::Continue);
    }

    pub fn begin_network_replay(&mut self, history: &[String]) {
        self.clear_selection();
        self.pending_square = None;
        self.pending_move = None;
        self.import_open = false;
        self.confirm_restart = false;
        self.begin_replay(
            format!(
                "NOLAN-CHESS 2\ncausal_gaps {}\n{}\n",
                if self.online.causal_gaps { "on" } else { "off" },
                history.join("\n")
            ),
            ReplayKind::Network,
        );
    }

    pub fn begin_import(&mut self) {
        if self.online.enabled || self.replay_job.is_some() || self.transition.is_some() {
            return;
        }
        self.import_error = None;
        self.begin_replay(self.import_text.clone(), ReplayKind::Import);
    }

    pub fn can_undo(&self) -> bool {
        !self.online.enabled
            && !self.facing.animating()
            && self.replay_job.is_none()
            && self.transition.is_none()
            && !self.import_open
            && !self.confirm_restart
            && self
                .record
                .commands()
                .iter()
                .any(|command| matches!(command, Command::Move { .. }))
    }

    pub fn undo_last_move(&mut self) {
        if !self.can_undo() {
            return;
        }
        let Some((prefix, removed)) = self.record.without_last_move() else {
            return;
        };
        self.clear_selection();
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
        self.transition = None;
        self.viewport.scroll = None;
        let task = AsyncComputeTaskPool::get_or_init(TaskPool::new).spawn(async move {
            // A failed replay must leave the live game intact, including a panic
            // in historical validation. The task owns only the imported text.
            std::panic::catch_unwind(|| GameRecord::load(&text))
                .map_err(|_| "Не удалось завершить восстановление партии".to_owned())?
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
                self.online.status = format!("Не удалось восстановить сетевую партию: {error}");
            }
            ReplayKind::Import => self.import_error = Some(error),
            ReplayKind::Undo { .. } => self.message(format!("Отмена не выполнена: {error}"), true),
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
                            CalculationStatus::Complete => self.engine.history().horizon(),
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
                self.message(
                    match kind {
                        ReplayKind::Network => "Сетевая партия синхронизирована.",
                        ReplayKind::Import => "Партия загружена. Можно продолжать игру.",
                        ReplayKind::Undo { .. } => "Последний ход отменён. История восстановлена.",
                    },
                    false,
                );
            }
            Some(Err(error)) => self.replay_failed(kind, error),
            None => {}
        }
    }

    pub fn message(&mut self, text: impl Into<String>, error: bool) {
        self.notice = text.into();
        self.is_error = error;
    }

    pub fn status_text(&self) -> String {
        match self.engine.status() {
            GameStatus::Ongoing => format!("Ход: {}", side_name(self.engine.turn())),
            GameStatus::Waiting {
                reason: WaitingReason::NoActivePieces,
                ..
            } => "Ожидание · нет активных фигур".into(),
            GameStatus::Waiting { player, .. } => format!(
                "{} ожидают · ходят {}",
                side_name(player),
                side_name(self.engine.turn())
            ),
            GameStatus::Finished(GameOutcome::Stalemate) => "Ничья — пат".into(),
            GameStatus::Finished(GameOutcome::Victory { winner, reason }) => format!(
                "Победа: {} · {}",
                side_name(winner),
                match reason {
                    WinReason::Checkmate => "мат",
                    WinReason::KingCaptured => "король взят",
                    WinReason::Paradox => "парадокс",
                }
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            engine: Engine::from_worldlines(vec![line], Color::White).unwrap(),
            ..GameUi::default()
        };
        game.jump_to = None;
        game.click_square(2, Sq(1, 0));
        assert_eq!(game.selected.as_ref().unwrap().skips, [3, 4, 5, 6]);
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
        assert!(game.import_error.as_ref().unwrap().contains("2"));
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
}
