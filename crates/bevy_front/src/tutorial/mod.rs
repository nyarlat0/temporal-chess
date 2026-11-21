//! Guided local games. Checkpoints identify a lesson step; the position is
//! rebuilt from its setup and the accepted scripted actions on resume.
mod lessons;
mod panel;
mod positions;

use crate::controller::{GameUi, TimelineViewport};
use bevy_egui::egui::{self, Rect};
use serde::{Deserialize, Serialize};
use temporal_chess_engine::{
    engine::Engine,
    model::{BoardTime, MoveEffect, Sq, TimeDirection},
    record::{Command, GameRecord},
};

use lessons::CHAPTERS;
pub use panel::{begin_pass, paint_spotlight, panel};

const STORAGE_KEY: &str = "temporal-chess.tutorial-v5";
const OLDER_STORAGE_KEYS: [&str; 4] = [
    "temporal-chess.tutorial-v1",
    "temporal-chess.tutorial-v2",
    "temporal-chess.tutorial-v3",
    "temporal-chess.tutorial-v4",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Cursor {
    chapter: usize,
    scene: usize,
    step: usize,
}

impl Cursor {
    const FIRST: Self = Self {
        chapter: 0,
        scene: 0,
        step: 0,
    };

    fn chapter_start(chapter: usize) -> Self {
        Self {
            chapter,
            ..Self::FIRST
        }
    }

    fn scene(self) -> Option<&'static Scene> {
        CHAPTERS.get(self.chapter)?.scenes.get(self.scene)
    }

    fn valid(self) -> bool {
        self.scene().is_some_and(|s| self.step < s.steps.len())
    }

    fn next(self) -> Option<Self> {
        let same_scene = Self {
            step: self.step + 1,
            ..self
        };
        let next_scene = Self {
            scene: self.scene + 1,
            step: 0,
            ..self
        };
        let next_chapter = Self {
            chapter: self.chapter + 1,
            scene: 0,
            step: 0,
        };
        [same_scene, next_scene, next_chapter]
            .into_iter()
            .find(|c| c.valid())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Checkpoint {
    Playing(Cursor),
    Paused(Cursor),
    Completed,
    Skipped,
}

fn checkpoint() -> Option<Checkpoint> {
    let mut value = crate::view::stored_setting(STORAGE_KEY)
        .and_then(|value| serde_json::from_str(&value).ok())
        .or_else(|| {
            let (version, old) =
                OLDER_STORAGE_KEYS
                    .iter()
                    .enumerate()
                    .rev()
                    .find_map(|(version, key)| {
                        let old = crate::view::stored_setting(key)?;
                        Some((version + 1, serde_json::from_str(&old).ok()?))
                    })?;
            let migrated = migrate_checkpoint(old, version);
            persist(migrated);
            Some(migrated)
        });
    if let Some(Checkpoint::Playing(c) | Checkpoint::Paused(c)) = value
        && !c.valid()
    {
        value = None;
    }
    value
}

fn migrate_checkpoint(checkpoint: Checkpoint, version: usize) -> Checkpoint {
    let migrate = |mut cursor: Cursor| {
        if version == 1 {
            match cursor.chapter {
                1 if cursor.scene > 0 || cursor.step > 13 => {
                    // Resume after the last retained action in the inversion chapter.
                    cursor.scene = 0;
                    cursor.step = 14;
                }
                5 => cursor = Cursor::chapter_start(5),
                6.. => cursor.chapter -= 1,
                _ => {}
            }
        }
        if (version < 4 && cursor.chapter == 14) || (version < 3 && cursor.chapter == 13) {
            Cursor::chapter_start(cursor.chapter)
        } else {
            if cursor.chapter == 15 && matches!(cursor.scene, 2 | 3) {
                cursor.step = 0;
            }
            cursor
        }
    };
    match checkpoint {
        Checkpoint::Playing(cursor) => Checkpoint::Playing(migrate(cursor)),
        Checkpoint::Paused(cursor) => Checkpoint::Paused(migrate(cursor)),
        Checkpoint::Completed | Checkpoint::Skipped => checkpoint,
    }
}

fn persist(checkpoint: Checkpoint) {
    if let Ok(text) = serde_json::to_string(&checkpoint) {
        crate::view::store_setting(STORAGE_KEY, &text);
    }
}

pub fn can_resume() -> bool {
    matches!(
        checkpoint(),
        Some(Checkpoint::Paused(_) | Checkpoint::Playing(_))
    )
}

pub fn on_first_draw(game: &mut GameUi) {
    if cfg!(target_arch = "wasm32") {
        match checkpoint() {
            None => start(game, Cursor::FIRST),
            Some(Checkpoint::Playing(cursor)) => start(game, cursor),
            Some(Checkpoint::Paused(_) | Checkpoint::Completed | Checkpoint::Skipped) => {}
        }
    }
}

pub fn replay(game: &mut GameUi, resume: bool) {
    if game.tutorial.is_some() {
        exit(game, Leave::Pause);
    }
    let cursor = if resume {
        match checkpoint() {
            Some(Checkpoint::Playing(cursor) | Checkpoint::Paused(cursor)) => cursor,
            _ => Cursor::FIRST,
        }
    } else {
        Cursor::FIRST
    };
    start(game, cursor);
}

fn start(game: &mut GameUi, cursor: Cursor) {
    game.reset_playback();
    game.clear_selection();
    game.pending_square = None;
    game.pending_move = None;
    let suspended = Box::new(std::mem::take(game));
    let session = Session {
        cursor,
        pending: None,
        completed: false,
        focus_pending: false,
        leave: None,
        suspended,
    };
    install_scene(game, session);
}

fn install_scene(game: &mut GameUi, session: Session) {
    let cursor = session.cursor;
    match build_scene(cursor) {
        Ok(mut scene) => {
            scene.local_auto_flip = session.suspended.local_auto_flip;
            scene.animation_speed = session.suspended.animation_speed;
            scene.cell_size = session.suspended.cell_size;
            scene.tutorial = Some(session);
            *game = scene;
            persist(Checkpoint::Playing(cursor));
        }
        Err(error) => {
            *game = *session.suspended;
            persist(Checkpoint::Paused(cursor));
            game.message(
                crate::i18n::Message::with_error(
                    "Не удалось открыть урок: ",
                    "Could not open the lesson: ",
                    error,
                ),
                true,
            );
        }
    }
}

fn build_scene(cursor: Cursor) -> Result<GameUi, String> {
    let scene = cursor.scene().ok_or("Unknown lesson")?;
    let (engine, record) = match scene.position {
        Position::Standard | Position::Opening(_) => (
            Engine::default()
                .with_interactive_calculation()
                .map_err(|err| err.to_string())?,
            GameRecord::default(),
        ),
        Position::Record(text) => GameRecord::load(text).map_err(|err| err.to_string())?,
        Position::Scenario(build) => build()?,
    };
    let mut game = GameUi {
        engine,
        record,
        ..GameUi::default()
    };
    if let Position::Opening(opening) = scene.position {
        for &action in opening {
            let command = action
                .command(&game.engine)?
                .ok_or("Expected setting command")?;
            game.record
                .apply(&mut game.engine, command)
                .map_err(|error| error.to_string())?;
        }
    }
    game.refresh_checks();
    for step in &scene.steps[..cursor.step] {
        if let Some(command) = step.action.command(&game.engine)? {
            game.apply_confirmed(command)?;
            game.reset_playback();
        } else {
            restore_control(&mut game, step.action);
        }
    }
    let step = scene.steps[cursor.step];
    game.jump_to = step
        .camera
        .or_else(|| step.action.source().map(|point| point.t))
        .or(Some(scene.camera));
    game.viewport = TimelineViewport::default();
    game.notice.clear();
    game.sound_events.clear();
    game.sound_events.push(crate::sound::Event::Reset);
    Ok(game)
}

fn restore_control(game: &mut GameUi, action: Action) {
    match action {
        Action::Button(Control::Worldline) => game.toggle_worldline(),
        Action::Button(Control::Inspect) => game.toggle_inspector(),
        Action::Button(Control::Up | Control::Down) if game.worldline_selected.is_some() => {
            game.navigate_worldline(action == Action::Button(Control::Up));
            game.worldline_transition = None;
        }
        Action::Inspect(point) => game.click_square(point.t, point.sq),
        _ => {}
    }
}

/// The real local/network match keeps its own state while the lessons run.
pub fn background_game(game: &mut GameUi) -> &mut GameUi {
    if game.tutorial.is_none() {
        return game;
    }
    &mut game.tutorial.as_mut().unwrap().suspended
}

#[derive(Clone, Copy)]
enum Leave {
    Pause,
    Skip,
}

fn exit(game: &mut GameUi, leave: Leave) {
    if let Some(session) = game.tutorial.take() {
        match leave {
            Leave::Skip => persist(Checkpoint::Skipped),
            Leave::Pause if !session.completed => persist(Checkpoint::Paused(
                session.pending.unwrap_or(session.cursor),
            )),
            Leave::Pause => {}
        }
        *game = *session.suspended;
        game.sound_events.clear();
        game.sound_events.push(crate::sound::Event::Reset);
    }
}

pub struct Session {
    cursor: Cursor,
    pending: Option<Cursor>,
    completed: bool,
    focus_pending: bool,
    leave: Option<Leave>,
    suspended: Box<GameUi>,
}

impl Session {
    fn step(&self) -> Step {
        self.cursor.scene().unwrap().steps[self.cursor.step]
    }

    fn advance(&mut self) {
        if self.completed || self.pending.is_some() {
            return;
        }
        match self.cursor.next() {
            Some(next) => {
                if next.chapter == self.cursor.chapter && next.scene == self.cursor.scene {
                    self.cursor = next;
                    self.focus_pending = true;
                } else {
                    self.pending = Some(next);
                }
                persist(Checkpoint::Playing(next));
            }
            None => {
                self.completed = true;
                persist(Checkpoint::Completed);
            }
        }
    }

    pub fn allows(&self, command: Command) -> bool {
        !self.completed && self.pending.is_none() && self.step().action.matches(command)
    }

    pub fn accepts_input(&self, command: Command) -> bool {
        !self.step().automatic && self.allows(command)
    }

    pub fn played(&mut self, command: Command) {
        if self.allows(command) {
            self.advance();
        }
    }

    pub fn clicked(&mut self, control: Control) {
        if !self.completed && self.step().action == Action::Button(control) {
            self.advance();
        }
    }

    pub fn inspected(&mut self, t: BoardTime, sq: Sq) {
        if self.step().action == Action::Inspect(Point { t, sq }) {
            self.advance();
        }
    }

    pub fn tool_enabled(&self, control: Control) -> bool {
        !self.completed && self.pending.is_none() && self.step().action == Action::Button(control)
    }

    pub fn inspect_target(&self, t: BoardTime, sq: Sq) -> bool {
        !self.completed
            && self.pending.is_none()
            && self.step().action == Action::Inspect(Point { t, sq })
    }
}

pub fn update_scene(game: &mut GameUi) {
    let busy = game.playback_busy() || game.facing.animating() || game.replay_job.is_some();
    let Some(session) = &mut game.tutorial else {
        return;
    };
    if let Some(leave) = session.leave {
        exit(game, leave);
        return;
    }
    if busy || game.worldline_transition.is_some() || game.viewport.scroll.is_some() {
        return;
    }
    if let Some(next) = session.pending.take() {
        session.cursor = next;
        let session = game.tutorial.take().unwrap();
        install_scene(game, session);
    } else {
        let step = session.step();
        if std::mem::take(&mut session.focus_pending)
            && let Some(target) = step
                .camera
                .or_else(|| step.action.source().map(|point| point.t))
        {
            game.jump_to = Some(target);
        }
        if step.automatic && !game.is_error {
            let result = step
                .action
                .command(&game.engine)
                .and_then(|command| command.ok_or_else(|| "Missing tutorial reply".to_owned()))
                .and_then(|command| game.apply_confirmed(command));
            if let Err(error) = result {
                game.message(
                    crate::i18n::Message::with_error(
                        "Не удалось сыграть ответный ход: ",
                        "Could not play the reply: ",
                        error,
                    ),
                    true,
                );
            }
        }
    }
}

fn select_chapter(game: &mut GameUi, chapter: usize) {
    let session = game.tutorial.as_mut().unwrap();
    session.pending = Some(Cursor::chapter_start(chapter));
    session.focus_pending = false;
    game.reset_playback();
    game.replay_job = None;
    game.viewport.scroll = None;
    game.worldline_transition = None;
    game.facing = Default::default();
}

#[derive(Clone, Copy)]
struct Text(&'static str, &'static str);

impl Text {
    fn get(self) -> &'static str {
        crate::i18n::Language::current().select(self.0, self.1)
    }
}

struct Chapter {
    title: Text,
    scenes: &'static [Scene],
}

#[derive(Clone, Copy)]
enum Position {
    Standard,
    Opening(&'static [Action]),
    Record(&'static str),
    Scenario(fn() -> Result<(Engine, GameRecord), String>),
}

struct Scene {
    position: Position,
    camera: BoardTime,
    steps: &'static [Step],
}

#[derive(Clone, Copy)]
struct Step {
    text: Text,
    action: Action,
    focus: [Option<Anchor>; 2],
    camera: Option<BoardTime>,
    automatic: bool,
}

impl Step {
    const fn new(text: Text, action: Action) -> Self {
        Self {
            text,
            action,
            focus: [None; 2],
            camera: None,
            automatic: false,
        }
    }

    const fn focus(mut self, focus: Anchor) -> Self {
        self.focus[0] = Some(focus);
        self
    }

    const fn also_focus(mut self, focus: Anchor) -> Self {
        self.focus[1] = Some(focus);
        self
    }

    const fn camera(mut self, t: BoardTime) -> Self {
        self.camera = Some(t);
        self
    }

    const fn automatic(mut self) -> Self {
        self.automatic = true;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Point {
    t: BoardTime,
    sq: Sq,
}

const fn at(t: BoardTime, square: &str) -> Point {
    let square = square.as_bytes();
    Point {
        t,
        sq: Sq((square[0] - b'a') as usize, (square[1] - b'1') as usize),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Anchor {
    Control(Control),
    Board(BoardTime),
    Square(BoardTime, Sq),
    Turn,
    Activity,
    Inspection,
    Streak,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Control {
    Replay,
    Inspect,
    Worldline,
    Up,
    Down,
    Direction,
    Continue,
    Decline,
    Check,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Move {
        from: Point,
        to: Sq,
        direction: TimeDirection,
        wait: Option<BoardTime>,
    },
    Wait {
        from: Point,
        until: BoardTime,
    },
    Inspect(Point),
    Button(Control),
    Next,
}

impl Action {
    const fn mv(t: BoardTime, from: &str, to: &str, direction: TimeDirection) -> Self {
        Self::Move {
            from: at(t, from),
            to: at(t, to).sq,
            direction,
            wait: None,
        }
    }

    const fn mv_wait(
        t: BoardTime,
        from: &str,
        to: &str,
        direction: TimeDirection,
        until: BoardTime,
    ) -> Self {
        Self::Move {
            from: at(t, from),
            to: at(t, to).sq,
            direction,
            wait: Some(until),
        }
    }

    fn source(self) -> Option<Point> {
        match self {
            Self::Move { from, .. } | Self::Wait { from, .. } | Self::Inspect(from) => Some(from),
            _ => None,
        }
    }

    fn matches(self, command: Command) -> bool {
        match (self, command) {
            (
                Self::Move {
                    from,
                    to,
                    direction,
                    wait,
                },
                command,
            ) => {
                let Some(mv) = command.spatial_move() else {
                    return false;
                };
                let origin = match command {
                    Command::Move { from, .. } | Command::MoveAndWait { from, .. } => from,
                    _ => return false,
                };
                Point {
                    t: mv.t,
                    sq: origin,
                } == from
                    && mv.sq == to
                    && mv.direction == direction
                    && command.wait_until() == wait
            }
            (
                Self::Wait { from, until },
                Command::Advance {
                    source,
                    from: sq,
                    to_t,
                },
            ) => from == Point { t: source.t, sq } && until == to_t,
            (Self::Button(Control::Continue), Command::Continue)
            | (Self::Button(Control::Decline), Command::DeclineReactions) => true,
            _ => false,
        }
    }

    fn command(self, engine: &Engine) -> Result<Option<Command>, String> {
        if self == Self::Button(Control::Continue) {
            return Ok(Some(Command::Continue));
        }
        if self == Self::Button(Control::Decline) {
            return Ok(Some(Command::DeclineReactions));
        }
        let Some(from) = self.source().filter(|_| !matches!(self, Self::Inspect(_))) else {
            return Ok(None);
        };
        let piece = engine
            .move_sources()
            .find(|p| p.t == from.t && p.sq == from.sq)
            .ok_or_else(|| {
                format!(
                    "No active piece for {self:?}; turn {:?}, status {:?}",
                    engine.turn(),
                    engine.status()
                )
            })?;
        match self {
            Self::Wait { until, .. } => Ok(Some(Command::Advance {
                source: crate::controller::identity(piece),
                from: from.sq,
                to_t: until,
            })),
            Self::Move {
                to,
                direction,
                wait,
                ..
            } => {
                let mv = engine
                    .possible_moves(piece.origin, piece.age)
                    .map_err(|err| err.to_string())?
                    .into_iter()
                    .find(|mv| {
                        mv.sq == to
                            && mv.direction == direction
                            && !matches!(mv.effect, MoveEffect::Promote(_))
                    })
                    .ok_or_else(|| format!("No available move for {self:?}"))?;
                Ok(Some(match wait {
                    Some(until) => Command::MoveAndWait {
                        mv,
                        from: from.sq,
                        until,
                    },
                    None => Command::Move { mv, from: from.sq },
                }))
            }
            _ => unreachable!(),
        }
    }

    fn spotlight(self, game: &GameUi) -> Option<Anchor> {
        let square = |Point { t, sq }| Some(Anchor::Square(t, sq));
        match self {
            Self::Move {
                from,
                to,
                direction,
                wait,
            } => match &game.selected {
                Some(selected) if selected.piece.t == from.t && selected.piece.sq == from.sq => {
                    if selected.direction == direction {
                        square(Point {
                            t: wait.unwrap_or(from.t),
                            sq: to,
                        })
                    } else {
                        Some(Anchor::Control(Control::Direction))
                    }
                }
                _ => square(from),
            },
            Self::Wait { from, until } => {
                if game
                    .selected
                    .as_ref()
                    .is_some_and(|s| s.piece.t == from.t && s.piece.sq == from.sq)
                {
                    square(Point {
                        t: until,
                        sq: from.sq,
                    })
                } else {
                    square(from)
                }
            }
            Self::Inspect(point) => square(point),
            Self::Button(Control::Check) => Some(Anchor::Turn),
            Self::Button(control) => Some(Anchor::Control(control)),
            Self::Next => None,
        }
    }
}

pub fn mark(ctx: &egui::Context, game: &GameUi, anchor: Anchor, rect: Rect) {
    if let Some(session) = &game.tutorial
        && !session.completed
        && ((!session.step().automatic && session.step().action.spotlight(game) == Some(anchor))
            || session.step().focus.contains(&Some(anchor)))
    {
        panel::register(ctx, rect);
    }
}

pub fn spotlight_square(game: &GameUi, t: BoardTime) -> Option<Sq> {
    let session = game.tutorial.as_ref()?;
    for anchor in std::iter::once(session.step().action.spotlight(game))
        .chain(session.step().focus)
        .flatten()
    {
        if let Anchor::Square(time, square) = anchor
            && time == t
        {
            return Some(square);
        }
    }
    None
}

#[cfg(test)]
mod tests;
