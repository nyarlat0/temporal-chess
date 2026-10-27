use bevy::prelude::Resource;
use std::sync::{Mutex, mpsc};
use temporal_chess_engine::{
    engine::{CalculationStatus, Engine, GameOutcome, GameStatus, WaitingReason, WinReason},
    model::{Color, Move, PieceAt, PieceId, PieceView, Sq, TimeDirection},
    record::{Command, GameRecord, RecordError},
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
        TimeDirection::Future => "В будущее ↑",
        TimeDirection::Past => "В прошлое ↓",
    }
}

pub struct Selection {
    pub piece: PieceView,
    pub direction: TimeDirection,
    pub moves: Vec<Move>,
    pub captures: Vec<(Move, PieceView)>,
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
    pub engine: Engine,
    pub selected: Option<Selection>,
    pub inspect_mode: bool,
    pub inspected: Option<PieceAt>,
    pub promotion: Vec<Move>,
    pub overlapping: Vec<PieceView>,
    pub notice: String,
    pub is_error: bool,
    pub log: Vec<String>,
    pub cell_size: f32,
    pub jump_to: Option<usize>,
    pub confirm_restart: bool,
    pub fonts_ready: bool,
    pub art: Option<crate::piece_art::PieceArt>,
    pub viewport: TimelineViewport,
    pub record: GameRecord,
    pub import_open: bool,
    pub import_text: String,
    pub import_error: Option<String>,
    pub replay_job: Option<ReplayJob>,
}

#[derive(Clone, Copy)]
enum ReplayKind {
    Import,
    Undo { source_t: usize },
}

pub struct ReplayJob {
    kind: ReplayKind,
    receiver: Mutex<mpsc::Receiver<Result<(Engine, GameRecord), RecordError>>>,
}

/// Retain the visible board when newer boards are inserted above it.
#[derive(Default)]
pub struct TimelineViewport {
    pub times: Vec<usize>,
    pub offset: f32,
    pub stride: f32,
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
            engine: Engine::default()
                .with_interactive_calculation()
                .expect("valid initial game"),
            selected: None,
            inspect_mode: false,
            inspected: None,
            promotion: Vec::new(),
            overlapping: Vec::new(),
            notice: "Выберите фигуру. Здесь играют вдвоём за одним компьютером.".into(),
            is_error: false,
            log: Vec::new(),
            cell_size: 64.0,
            jump_to: Some(0),
            confirm_restart: false,
            fonts_ready: false,
            art: None,
            viewport: TimelineViewport::default(),
            record: GameRecord::default(),
            import_open: false,
            import_text: String::new(),
            import_error: None,
            replay_job: None,
        }
    }
}

impl GameUi {
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

    pub fn choose(&mut self, piece: PieceView) {
        self.overlapping.clear();
        if self.inspect_mode {
            self.inspected = Some(identity(piece));
            return;
        }
        if self.engine.board(piece.t).is_none() {
            self.message(
                "Это предоска: сначала продолжите просчёт. Инспекция доступна уже сейчас.",
                true,
            );
            return;
        }
        if !self
            .engine
            .move_sources()
            .any(|p| identity(p) == identity(piece))
        {
            self.message(
                "Фигура не активна для текущего игрока. Её можно посмотреть в инспекции.",
                true,
            );
            return;
        }
        match self.engine.possible_moves(piece.origin, piece.age) {
            Ok(moves) => {
                let skips = self.engine.possible_skips(identity(piece));
                let captures = moves
                    .iter()
                    .filter_map(|mv| {
                        self.engine
                            .move_capture_target(*mv)
                            .map(|target| (*mv, target))
                    })
                    .collect();
                self.selected = Some(Selection {
                    piece,
                    direction: TimeDirection::from_inverted(piece.inverted),
                    moves,
                    captures,
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
                let result = self.record.apply(&mut self.engine, command);
                self.after_command(result, format!("Ожидание: t={} → {t}", source.t));
                return;
            }
        }
        if self.engine.preboard(t).is_none() {
            return;
        }
        let pieces: Vec<_> = self.pieces(t).into_iter().filter(|p| p.sq == sq).collect();
        match pieces.len() {
            0 => {}
            1 => self.choose(pieces[0]),
            _ => self.overlapping = pieces,
        }
    }

    pub fn play(&mut self, mv: Move) {
        if self.replay_job.is_some() {
            return;
        }
        let text = format!(
            "{}: t={} → {} · {}{}",
            side_name(self.engine.turn()),
            mv.t,
            mv.direction.step(mv.t).unwrap_or(mv.t),
            char::from(b'a' + mv.sq.0 as u8),
            mv.sq.1 + 1
        );
        let result = match self
            .engine
            .piece_state(mv.source())
            .map(|state| Sq(state.x, state.y))
        {
            Some(from) => self
                .record
                .apply(&mut self.engine, Command::Move { mv, from }),
            None => Err(temporal_chess_engine::engine::EngineErr::WrongPiece),
        };
        if result.is_ok() {
            self.jump_to = mv.direction.step(mv.t);
        }
        self.after_command(result, text);
    }

    fn after_command(
        &mut self,
        result: Result<(), temporal_chess_engine::engine::EngineErr>,
        text: String,
    ) {
        match result {
            Ok(()) => {
                self.clear_selection();
                self.log.push(text.clone());
                if self.log.len() > 200 {
                    self.log.remove(0);
                }
                self.message(text, false);
            }
            Err(error) => {
                self.promotion.clear();
                self.message(format!("Ход не принят: {error}"), true);
            }
        }
    }

    pub fn continue_calculation(&mut self) {
        if self.replay_job.is_some() {
            return;
        }
        match self.record.apply(&mut self.engine, Command::Continue) {
            Ok(()) => {
                let status = self.engine.calculation_status();
                self.clear_selection();
                let text = match status {
                    CalculationStatus::Complete => "История просчитана полностью.".to_owned(),
                    CalculationStatus::Paused { t, .. } => {
                        format!("Просчёт остановлен у следующего отменённого хода: t={t}.")
                    }
                };
                self.message(text, false);
            }
            Err(error) => self.message(format!("Ошибка просчёта: {error}"), true),
        }
    }

    pub fn begin_import(&mut self) {
        if self.replay_job.is_some() {
            return;
        }
        self.import_error = None;
        self.begin_replay(self.import_text.clone(), ReplayKind::Import);
    }

    pub fn can_undo(&self) -> bool {
        self.replay_job.is_none()
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
        let (sender, receiver) = mpsc::channel();
        self.replay_job = Some(ReplayJob {
            kind,
            receiver: Mutex::new(receiver),
        });
        std::thread::spawn(move || {
            let _ = sender.send(GameRecord::load(&text));
        });
    }

    fn replay_failed(&mut self, kind: ReplayKind, error: String) {
        self.replay_job = None;
        match kind {
            ReplayKind::Import => self.import_error = Some(error),
            ReplayKind::Undo { .. } => self.message(format!("Отмена не выполнена: {error}"), true),
        }
    }

    pub fn poll_replay(&mut self) {
        let Some(job) = &self.replay_job else {
            return;
        };
        let kind = job.kind;
        let result = job.receiver.lock().expect("replay receiver").try_recv();
        match result {
            Ok(Ok((engine, record))) => {
                self.clear_selection();
                self.engine = engine;
                self.record = record;
                self.log = self
                    .record
                    .commands()
                    .iter()
                    .map(ToString::to_string)
                    .collect();
                self.jump_to = Some(match kind {
                    ReplayKind::Undo { source_t } => source_t.min(self.engine.history().horizon()),
                    ReplayKind::Import => match self.engine.calculation_status() {
                        CalculationStatus::Paused { t, .. } => t,
                        CalculationStatus::Complete => self.engine.history().horizon(),
                    },
                });
                self.viewport = TimelineViewport::default();
                self.replay_job = None;
                self.import_open = false;
                self.inspect_mode = false;
                self.message(
                    match kind {
                        ReplayKind::Import => "Партия загружена. Можно продолжать игру.",
                        ReplayKind::Undo { .. } => "Последний ход отменён. История восстановлена.",
                    },
                    false,
                );
            }
            Ok(Err(error)) => {
                self.replay_failed(kind, error.to_string());
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.replay_failed(kind, "Не удалось завершить восстановление партии".into());
            }
            Err(mpsc::TryRecvError::Empty) => {}
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
