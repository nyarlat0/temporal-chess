use std::collections::{BTreeMap, HashMap};
use std::fmt;

use crate::board::{Board, PreBoard};
use crate::model::{Move, MoveId, PieceAt, ShortPieceId, Worldline};
use crate::pieces::{Color, PieceView};

/// Result of resolving a candidate encounter on an isolated history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureOutcome {
    Captured,
    AttackerLost,
    Unresolved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WinReason {
    Checkmate,
    KingCaptured,
    Paradox,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GameOutcome {
    Victory { winner: Color, reason: WinReason },
    Stalemate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WaitingReason {
    NoActivePieces,
    OpponentPastMoves,
    KingUnderAttack,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GameStatus {
    #[default]
    Ongoing,
    Waiting {
        player: Color,
        reason: WaitingReason,
    },
    Finished(GameOutcome),
}

/// Visible/spatial threats and backward capture candidates are reported separately.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CheckStatus {
    pub ordinary: Vec<PieceAt>,
    pub inverted: Vec<PieceAt>,
}

impl CheckStatus {
    pub fn is_check(&self) -> bool {
        !self.ordinary.is_empty() || !self.inverted.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalculationStatus {
    Complete,
    Paused { t: usize, side: Color },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(super) struct Calculation {
    pub interactive: bool,
    pub acknowledged: Vec<PieceAt>,
    pub responsible: Option<Color>,
    pub cancellations: Vec<Cancellation>,
}

/// Physical worldline edits caused by one cancellation, including a castling rook.
/// Reaction flags alone are not evidence that a continuation affects history.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct Cancellation {
    pub source: PieceAt,
    pub lines: Vec<(usize, Worldline, Worldline)>,
}

/// Authoritative state. Board projections and lookup tables are never stored here.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct History {
    pub(super) rules: crate::model::GameRules,
    pub(super) starting_position: crate::model::StartingPosition,
    pub(super) worldlines: Vec<Worldline>,
    pub(super) moves: BTreeMap<MoveId, Move>,
    /// Targets observed when a move was played. Dead targets are restored only
    /// in its legality view, so a recorded pawn capture can be revalidated.
    pub(super) targets: BTreeMap<MoveId, Vec<PieceAt>>,
    /// The timeline still advances when its newest board contains no pieces.
    pub(super) horizon: usize,
    pub(super) status: GameStatus,
    pub(super) calculation: Calculation,
}

impl History {
    pub fn starting_position(&self) -> crate::model::StartingPosition {
        self.starting_position
    }
    pub fn rules(&self) -> crate::model::GameRules {
        self.rules
    }
    pub fn worldlines(&self) -> &[Worldline] {
        &self.worldlines
    }
    pub fn moves(&self) -> &BTreeMap<MoveId, Move> {
        &self.moves
    }
    pub fn horizon(&self) -> usize {
        self.horizon
    }
}

/// Entirely derived from History. Rebuilt together after every history mutation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Projection {
    pub preboards: BTreeMap<usize, PreBoard>,
    pub boards: BTreeMap<usize, Board>,
    pub pieces: HashMap<PieceAt, ShortPieceId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Engine {
    pub(super) history: History,
    pub(super) projection: Projection,
    pub(super) turn: Color,
}

impl Engine {
    /// Bucket key for analysis caches. Callers must also compare the full Engine
    /// before reusing a result: hashes are not identity proofs, and projections
    /// may expose different calculation frontiers for the same history.
    pub fn analysis_key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hash = std::hash::DefaultHasher::new();
        self.history.hash(&mut hash);
        self.turn.hash(&mut hash);
        hash.finish()
    }

    /// Conservative cache weight, counting states, remembered identities,
    /// cancellation snapshots and projected board cells. Not game logic.
    pub fn analysis_weight(&self) -> usize {
        fn line_weight(line: &Worldline) -> usize {
            line.iter()
                .map(|s| 1 + s.informed.len() + s.remembered.len() + s.loop_pivots.len())
                .sum::<usize>()
                + line.gaps().len()
        }
        self.history
            .worldlines
            .iter()
            .map(line_weight)
            .sum::<usize>()
            + self
                .history
                .calculation
                .cancellations
                .iter()
                .flat_map(|c| &c.lines)
                .map(|(_, a, b)| line_weight(a) + line_weight(b))
                .sum::<usize>()
            + 64 * (self.projection.boards.len() + self.projection.preboards.len())
            + self.history.moves.len() * 4
            + self.history.targets.values().map(Vec::len).sum::<usize>()
    }

    pub fn status(&self) -> GameStatus {
        self.history.status
    }

    pub fn outcome(&self) -> Option<GameOutcome> {
        match self.status() {
            GameStatus::Finished(outcome) => Some(outcome),
            _ => None,
        }
    }
    pub fn history(&self) -> &History {
        &self.history
    }
    pub fn turn(&self) -> Color {
        self.turn
    }
    pub fn board(&self, t: usize) -> Option<&Board> {
        self.projection.boards.get(&t)
    }
    pub fn preboard(&self, t: usize) -> Option<&PreBoard> {
        self.projection.preboards.get(&t)
    }

    /// All currently playable occurrences, in board-time order. No board takes
    /// priority: ordinary moves and reactions are both optional choices.
    pub fn move_sources(&self) -> impl Iterator<Item = PieceView> + '_ {
        self.active_occurrences(self.turn)
    }

    /// Active occurrences of either side, without changing the current turn.
    pub fn active_occurrences(
        &self,
        side: crate::model::Color,
    ) -> impl Iterator<Item = PieceView> + '_ {
        self.projection
            .boards
            .values()
            .flat_map(|board| board.pieces().iter().copied())
            .filter(move |piece| {
                self.outcome().is_none()
                    && piece.color == side
                    && piece.can_move()
                    && piece.is_movable()
            })
    }

    /// Publish a coherent command result. A validation cycle keeps the previous
    /// boards and commits only a paradox loss; other errors change nothing.
    pub(super) fn transaction(
        &mut self,
        command: impl FnOnce(&mut Self) -> Result<(), EngineErr>,
    ) -> Result<(), EngineErr> {
        *self = self.fork_transaction(command)?;
        Ok(())
    }

    pub(super) fn fork_transaction(
        &self,
        command: impl FnOnce(&mut Self) -> Result<(), EngineErr>,
    ) -> Result<Self, EngineErr> {
        if self.outcome().is_some() {
            return Err(EngineErr::GameOver);
        }
        let actor = self.turn;
        let mut candidate = self.clone();
        candidate.history.calculation.acknowledged.clear();
        candidate.history.calculation.responsible = Some(actor);
        match command(&mut candidate).and_then(|()| {
            candidate
                .finish_command(actor)
                .map_err(|e| e.during("adjudicating the resulting position"))
        }) {
            Ok(()) => Ok(candidate),
            Err(EngineErr::InfLoop) => {
                // Keep the last coherent timeline, but publish the loss. This is
                // a completed game command, not a retryable invalid move.
                let mut previous = self.clone();
                previous.history.status = GameStatus::Finished(GameOutcome::Victory {
                    winner: actor.opposite(),
                    reason: WinReason::Paradox,
                });
                Ok(previous)
            }
            Err(error) => Err(error),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ValidRes {
    HistChanged,
    Unchanged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineErr {
    GameOver,
    WrongPiece,
    AbsentBoard,
    WrongMove,
    WrongTurn,
    InfLoop,
    InvalidHistory(&'static str),
    UnsupportedMove(&'static str),
    BoardErr(BoardErr),
    /// An admitted command failed while updating the engine, not because the
    /// player chose an illegal move. Preserve the stage and the original cause.
    Internal {
        operation: String,
        cause: Box<EngineErr>,
    },
}

impl EngineErr {
    pub(super) fn during(self, operation: impl Into<String>) -> Self {
        // A causal cycle is a game outcome, never an implementation failure.
        if matches!(self, Self::InfLoop) {
            self
        } else {
            Self::Internal {
                operation: operation.into(),
                cause: Box::new(self),
            }
        }
    }

    pub fn is_internal(&self) -> bool {
        matches!(self, Self::Internal { .. })
    }
}

impl fmt::Display for EngineErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GameOver => f.write_str("the game has already finished"),
            Self::WrongPiece => f.write_str("piece occurrence is absent or cannot act"),
            Self::AbsentBoard => f.write_str("board is unavailable"),
            Self::WrongMove => f.write_str("move is not legal in this position"),
            Self::WrongTurn => f.write_str("piece cannot act on this turn"),
            Self::InfLoop => f.write_str("history reconciliation entered a cycle"),
            Self::InvalidHistory(reason) => write!(f, "invalid history: {reason}"),
            Self::UnsupportedMove(reason) => write!(f, "unsupported move: {reason}"),
            Self::BoardErr(error) => write!(f, "{error}"),
            Self::Internal { operation, cause } => {
                write!(f, "engine error at {operation}: {cause}")
            }
        }
    }
}

impl std::error::Error for EngineErr {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::BoardErr(error) => Some(error),
            Self::Internal { cause, .. } => Some(cause),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoardErr {
    InvalidTime,
    InvalidBoard(BoardConflict),
}

impl fmt::Display for BoardErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTime => f.write_str("no board at this time"),
            Self::InvalidBoard(conflict) => {
                write!(f, "{} pieces occupy one square", conflict.pieces().len())
            }
        }
    }
}

impl std::error::Error for BoardErr {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoardConflict {
    pub(super) pivs: Vec<PieceView>,
}

impl BoardConflict {
    pub fn pieces(&self) -> &[PieceView] {
        &self.pivs
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PieceCollision {
    pub bf_collider: PieceView,
    pub collider: PieceView,
    pub collided: PieceView,
}
