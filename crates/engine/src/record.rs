//! Portable, append-only commands starting from the standard initial position.
//! Recorded moves in History can be replaced or cancelled; they are not a replay log.
use crate::{
    engine::{Engine, EngineErr},
    model::{
        Castling, Color, Move, MoveEffect, PieceAt, PieceId, PieceKind, Promotion, Sq,
        TimeDirection,
    },
};
use std::fmt;

const HEADER: &str = "NOLAN-CHESS 2";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Move {
        mv: Move,
        from: Sq,
    },
    Advance {
        source: PieceAt,
        from: Sq,
        to_t: usize,
    },
    Continue,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GameRecord {
    causal_gaps: bool,
    commands: Vec<Command>,
    pieces: Vec<Option<RecordedPiece>>,
}

/// Historical command appearance, captured before later edits can remove or
/// transform its source occurrence. Reconstructed during import, not serialized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordedPiece {
    pub kind: PieceKind,
    pub color: Color,
    pub inverted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordError {
    pub line: usize,
    pub message: String,
}
impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Строка {}: {}", self.line, self.message)
    }
}
impl std::error::Error for RecordError {}

impl GameRecord {
    pub fn new(causal_gaps: bool) -> Self {
        Self {
            causal_gaps,
            ..Self::default()
        }
    }
    pub fn causal_gaps(&self) -> bool {
        self.causal_gaps
    }

    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    pub fn piece(&self, command_index: usize) -> Option<RecordedPiece> {
        self.pieces.get(command_index).copied().flatten()
    }

    /// The chronological prefix immediately before the last spatial move.
    /// Later free advances and calculation acknowledgements belong to the
    /// discarded continuation too; replaying them could reference erased states.
    /// The original journal stays intact until its replacement is replayed.
    pub fn without_last_move(&self) -> Option<(Self, Move)> {
        self.commands
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, command)| {
                if let Command::Move { mv, .. } = command {
                    Some((
                        Self {
                            causal_gaps: self.causal_gaps,
                            commands: self.commands[..index].to_vec(),
                            pieces: self.pieces[..index].to_vec(),
                        },
                        *mv,
                    ))
                } else {
                    None
                }
            })
    }

    /// A failed command leaves both the game and its replay record unchanged.
    /// Successful commands are retained even when reconciliation removes the
    /// initiating move from History: replay needs the command and its effects.
    pub fn apply(&mut self, engine: &mut Engine, command: Command) -> Result<(), EngineErr> {
        if !self.commands.is_empty() && self.causal_gaps != engine.history().causal_gaps() {
            return Err(EngineErr::InvalidHistory(
                "record rules differ from engine rules",
            ));
        }
        let piece = match command {
            Command::Move { mv, .. } => {
                engine.piece_state(mv.source()).map(|state| RecordedPiece {
                    kind: state.kind,
                    color: state.color,
                    inverted: mv.to_past(),
                })
            }
            Command::Advance { source, .. } => {
                engine.piece_state(source).map(|state| RecordedPiece {
                    kind: state.kind,
                    color: state.color,
                    inverted: state.inverted,
                })
            }
            Command::Continue => None,
        };
        match command {
            Command::Move { mv, from } => {
                check_source(engine, mv.source(), from)?;
                engine.apply_new_mv(mv)?;
            }
            Command::Advance { source, from, to_t } => {
                check_source(engine, source, from)?;
                engine.skip_ordinary(source, to_t)?;
            }
            Command::Continue => {
                // Older records can acknowledge a cancellation that is now
                // harmless and no longer pauses calculation. Keep replaying it
                // as a no-op, exactly like Engine::continue_calculation does.
                engine.continue_calculation()?;
            }
        }
        self.causal_gaps = engine.history().causal_gaps();
        self.commands.push(command);
        self.pieces.push(piece);
        Ok(())
    }

    pub fn to_text(&self) -> String {
        let mut text = format!(
            "{HEADER}\ncausal_gaps {}\n",
            if self.causal_gaps { "on" } else { "off" }
        );
        for command in &self.commands {
            text.push_str(&format!("{command}\n"));
        }
        text
    }

    /// Replay into an isolated game. Callers replace their game only on success.
    /// Blank lines and # comments are allowed; errors report original line numbers.
    pub fn load(text: &str) -> Result<(Engine, Self), RecordError> {
        let error = |line, message: String| RecordError { line, message };
        if text.len() > 2 * 1024 * 1024 {
            return Err(error(1, "Партия больше 2 МиБ".into()));
        }
        let mut lines = text
            .trim_start_matches('\u{feff}')
            .lines()
            .enumerate()
            .map(|(n, line)| (n + 1, line.split('#').next().unwrap_or("").trim()))
            .filter(|(_, line)| !line.is_empty());
        let (line, header) = lines
            .next()
            .ok_or_else(|| error(1, "Пустая запись партии".into()))?;
        let causal_gaps = match header {
            "NOLAN-CHESS 1" => true, // Existing records used preserved futures.
            HEADER => {
                let (line, mode) = lines
                    .next()
                    .ok_or_else(|| error(line, "Не указан режим causal_gaps".into()))?;
                match mode {
                    "causal_gaps on" => true,
                    "causal_gaps off" => false,
                    _ => {
                        return Err(error(
                            line,
                            "Ожидалось causal_gaps on или causal_gaps off".into(),
                        ));
                    }
                }
            }
            _ => {
                return Err(error(
                    line,
                    format!("Ожидался заголовок {HEADER} или NOLAN-CHESS 1"),
                ));
            }
        };
        let mut engine = Engine::new(causal_gaps)
            .with_interactive_calculation()
            .map_err(|e| error(line, e.to_string()))?;
        let mut record = Self::new(causal_gaps);
        for (line, text) in lines {
            if record.commands.len() >= 10_000 {
                return Err(error(line, "Больше 10000 команд".into()));
            }
            let command = parse(text).map_err(|message| error(line, message))?;
            record
                .apply(&mut engine, command)
                .map_err(|e| error(line, format!("Команда не принята: {e}")))?;
        }
        Ok((engine, record))
    }
}

fn check_source(engine: &Engine, source: PieceAt, from: Sq) -> Result<(), EngineErr> {
    let state = engine.piece_state(source).ok_or(EngineErr::WrongPiece)?;
    if !state.alive || Sq(state.x, state.y) != from {
        return Err(EngineErr::WrongPiece);
    }
    Ok(())
}

fn square(sq: Sq) -> String {
    format!("{}{}", char::from(b'a' + sq.0 as u8), sq.1 + 1)
}
fn piece(id: PieceId) -> String {
    format!("{}:{}:{}", id.wid, id.fut_seen, id.loop_turn)
}
impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Command::Move { mv, from } => {
                let effect = match mv.effect {
                    MoveEffect::Normal => "normal".into(),
                    MoveEffect::Castle(Castling::Short) => "O-O".into(),
                    MoveEffect::Castle(Castling::Long) => "O-O-O".into(),
                    MoveEffect::EnPassant { captured } => format!("ep:{}", square(captured)),
                    MoveEffect::Promote(p) => format!(
                        "={}",
                        match p {
                            Promotion::Queen => 'Q',
                            Promotion::Rook => 'R',
                            Promotion::Bishop => 'B',
                            Promotion::Knight => 'N',
                        }
                    ),
                };
                write!(
                    f,
                    "move {} {} {} {} {} {}",
                    mv.t,
                    piece(mv.pid),
                    square(from),
                    square(mv.sq),
                    if mv.to_past() { "-" } else { "+" },
                    effect
                )
            }
            Command::Advance { source, from, to_t } => write!(
                f,
                "advance {} {} {} {}",
                source.t,
                piece(source.piece),
                square(from),
                to_t
            ),
            Command::Continue => f.write_str("continue"),
        }
    }
}

impl std::str::FromStr for Command {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        parse(text)
    }
}

fn number(text: &str) -> Result<usize, String> {
    text.parse()
        .map_err(|_| format!("Некорректное число: {text}"))
}
fn parse_square(text: &str) -> Result<Sq, String> {
    let b = text.as_bytes();
    if b.len() == 2 && (b'a'..=b'h').contains(&b[0]) && (b'1'..=b'8').contains(&b[1]) {
        Ok(Sq((b[0] - b'a') as usize, (b[1] - b'1') as usize))
    } else {
        Err(format!("Некорректная клетка: {text}"))
    }
}
fn parse_piece(text: &str) -> Result<PieceId, String> {
    let fields: Vec<_> = text.split(':').collect();
    let [wid, future, turn] = fields.as_slice() else {
        return Err("Версия фигуры должна иметь вид wid:fut_seen:loop_turn".into());
    };
    Ok(PieceId {
        wid: number(wid)?,
        fut_seen: number(future)?,
        loop_turn: number(turn)?,
    })
}
fn parse(text: &str) -> Result<Command, String> {
    let fields: Vec<_> = text.split_whitespace().collect();
    match fields.as_slice() {
        ["move", t, pid, from, to, direction, effect] => {
            let direction = match *direction {
                "+" => TimeDirection::Future,
                "-" => TimeDirection::Past,
                _ => return Err("Направление: + или -".into()),
            };
            let effect = match *effect {
                "normal" => MoveEffect::Normal,
                "O-O" => MoveEffect::Castle(Castling::Short),
                "O-O-O" => MoveEffect::Castle(Castling::Long),
                "=Q" => MoveEffect::Promote(Promotion::Queen),
                "=R" => MoveEffect::Promote(Promotion::Rook),
                "=B" => MoveEffect::Promote(Promotion::Bishop),
                "=N" => MoveEffect::Promote(Promotion::Knight),
                s if s.starts_with("ep:") => MoveEffect::EnPassant {
                    captured: parse_square(&s[3..])?,
                },
                _ => return Err("Неизвестный вид хода".into()),
            };
            Ok(Command::Move {
                from: parse_square(from)?,
                mv: Move {
                    t: number(t)?,
                    pid: parse_piece(pid)?,
                    sq: parse_square(to)?,
                    direction,
                    effect,
                },
            })
        }
        ["advance", t, pid, from, to] => Ok(Command::Advance {
            source: PieceAt {
                t: number(t)?,
                piece: parse_piece(pid)?,
            },
            from: parse_square(from)?,
            to_t: number(to)?,
        }),
        ["continue"] => Ok(Command::Continue),
        _ => Err("Неизвестная команда или неверное количество полей".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::CalculationStatus;
    // Regression fixture: death on d4 at t=3 must not cancel d2-d4 at t=2.
    #[test]
    fn arrival_collision_does_not_grant_a_spurious_cancellation_reaction() {
        let text = "NOLAN-CHESS 1
move 0 6:0:0 g1 f3 + normal
move 1 17:1:0 b8 c6 + normal
move 2 11:2:0 d2 d4 + normal
move 3 31:3:0 h7 h6 + normal
move 4 15:4:0 h2 h3 + normal
move 5 30:5:0 g7 g5 + normal
move 6 14:6:0 g2 g4 + normal
move 7 17:7:0 c6 d4 - normal
move 6 6:6:0 f3 d4 - normal
move 5 17:5:0 c6 d4 - normal
";
        let (engine, record) = GameRecord::load(text).unwrap();
        let Command::Move { mv, .. } = record.commands()[2] else {
            unreachable!()
        };
        assert_eq!(engine.history().moves().get(&mv.id()), Some(&mv));
        assert!(
            !engine
                .piece_state(mv.source())
                .unwrap()
                .rights
                .cancelled_move
        );
        assert!(!engine.move_sources().any(|piece| piece.t == 2));
        let arrival = &engine.history().worldlines()[11][3];
        assert_eq!(
            (arrival.t, arrival.x, arrival.y, arrival.alive),
            (3, 3, 3, false)
        );
        // Revalidation's collision exception must not open a new straight pawn
        // capture against the actual occupant of the destination board.
        assert!(!engine.possible_moves(11, 2).unwrap().contains(&mv));
        assert_eq!(record.commands().len(), 10);
    }

    #[test]
    fn replay_inversion_and_then_continue_playing() {
        let mut engine = Engine::default().with_interactive_calculation().unwrap();
        let mut record = GameRecord::default();
        for (t, from, to, direction) in [
            (0, Sq(4, 1), Sq(4, 3), TimeDirection::Future),
            (1, Sq(4, 6), Sq(4, 4), TimeDirection::Future),
            (2, Sq(6, 0), Sq(7, 2), TimeDirection::Past),
        ] {
            let p = *engine.board(t).unwrap().get_piece(from.0, from.1).unwrap();
            let mv = engine
                .possible_moves(p.origin, p.age)
                .unwrap()
                .into_iter()
                .find(|mv| mv.sq == to && mv.direction == direction)
                .unwrap();
            record
                .apply(&mut engine, Command::Move { mv, from })
                .unwrap();
        }
        let (mut restored, mut imported) = GameRecord::load(&record.to_text()).unwrap();
        assert_eq!(restored, engine);
        assert_eq!(imported, record);
        let p = *restored.board(3).unwrap().get_piece(1, 7).unwrap();
        let mv = restored
            .possible_moves(p.origin, p.age)
            .unwrap()
            .into_iter()
            .find(|mv| mv.sq == Sq(2, 5) && !mv.to_past())
            .unwrap();
        imported
            .apply(&mut restored, Command::Move { mv, from: Sq(1, 7) })
            .unwrap();
        assert_eq!(GameRecord::load(&imported.to_text()).unwrap().0, restored);
    }
    #[test]
    fn malformed_or_illegal_commands_report_the_original_line() {
        for command in [
            "move 0 12:0:0 e2 e5 + normal",
            "move 0 12:0:0 e2 z9 + normal",
            "advance 0 12:0:0 e2 0",
            "continue",
            "move 0 12:0:0 e2 e4 ? normal",
        ] {
            let error =
                GameRecord::load(&format!("NOLAN-CHESS 1\n# comment\n\n{command}\n")).unwrap_err();
            assert_eq!(error.line, 4);
        }
        assert!(GameRecord::load("NOLAN-CHESS 2").is_err());
    }
    #[test]
    fn special_moves_and_incarnations_encode_without_loss() {
        for effect in [
            MoveEffect::Castle(Castling::Long),
            MoveEffect::Castle(Castling::Short),
            MoveEffect::EnPassant { captured: Sq(4, 4) },
            MoveEffect::Promote(Promotion::Knight),
            MoveEffect::Promote(Promotion::Queen),
        ] {
            let command = Command::Move {
                from: Sq(1, 6),
                mv: Move {
                    pid: PieceId {
                        wid: 3,
                        fut_seen: 12,
                        loop_turn: 2,
                    },
                    t: 8,
                    sq: Sq(1, 7),
                    direction: TimeDirection::Past,
                    effect,
                },
            };
            assert_eq!(parse(&command.to_string()).unwrap(), command);
        }
    }
    #[test]
    fn replay_free_advance_keeps_the_turn_and_temporal_worldlines() {
        let mut engine = Engine::default().with_interactive_calculation().unwrap();
        let mut record = GameRecord::default();
        for (t, from, to, direction) in [
            (0, Sq(4, 1), Sq(4, 3), TimeDirection::Future),
            (1, Sq(4, 6), Sq(4, 4), TimeDirection::Future),
            (2, Sq(6, 0), Sq(5, 2), TimeDirection::Future),
            (3, Sq(1, 7), Sq(2, 5), TimeDirection::Future),
            (4, Sq(5, 2), Sq(7, 3), TimeDirection::Past),
            (5, Sq(6, 7), Sq(5, 5), TimeDirection::Future),
        ] {
            let p = *engine.board(t).unwrap().get_piece(from.0, from.1).unwrap();
            let mv = engine
                .possible_moves(p.origin, p.age)
                .unwrap()
                .into_iter()
                .find(|mv| mv.sq == to && mv.direction == direction)
                .unwrap();
            record
                .apply(&mut engine, Command::Move { mv, from })
                .unwrap();
        }
        let p = engine
            .move_sources()
            .find(|p| p.inverted && p.t == 2)
            .unwrap();
        let source = PieceAt {
            piece: PieceId::from_piv(p),
            t: p.t,
        };
        assert!(engine.possible_skips(source).contains(&0));
        let turn = engine.turn();
        record
            .apply(
                &mut engine,
                Command::Advance {
                    source,
                    from: p.sq,
                    to_t: 0,
                },
            )
            .unwrap();
        assert_eq!(engine.turn(), turn);
        let (restored, imported) = GameRecord::load(&record.to_text()).unwrap();
        assert_eq!(restored, engine);
        assert_eq!(imported, record);
    }

    #[test]
    fn failed_commands_do_not_change_the_game_or_record() {
        let mut engine = Engine::default().with_interactive_calculation().unwrap();
        let original = engine.clone();
        let mut record = GameRecord::default();
        let command = parse("move 0 12:0:0 e2 e5 + normal").unwrap();
        assert!(record.apply(&mut engine, command).is_err());
        assert_eq!(engine, original);
        assert!(record.commands().is_empty());
    }

    #[test]
    fn calculation_acknowledgements_are_recorded_without_spending_a_turn() {
        use crate::model::{Color, PieceKind, State};
        let mut line = vec![State::new(PieceKind::N, Color::White, Sq(1, 0))];
        for _ in 0..6 {
            line.push(line.last().unwrap().prolongate().unwrap());
        }
        line[2].rights.cancelled_move = true;
        line[4].rights.cancelled_move = true;
        let mut engine = Engine::from_worldlines(vec![line], Color::White)
            .unwrap()
            .with_interactive_calculation()
            .unwrap();
        let mut record = GameRecord::default();
        record.apply(&mut engine, Command::Continue).unwrap();
        assert_eq!(
            engine.calculation_status(),
            CalculationStatus::Paused {
                t: 4,
                side: Color::White
            }
        );
        record.apply(&mut engine, Command::Continue).unwrap();
        assert_eq!(engine.calculation_status(), CalculationStatus::Complete);
        assert_eq!(engine.turn(), Color::White);
        assert_eq!(record.commands(), [Command::Continue, Command::Continue]);
        assert_eq!(parse("continue").unwrap(), Command::Continue);
        assert!(record.apply(&mut engine, Command::Continue).is_err());
        assert_eq!(record.commands().len(), 2);
    }
}
