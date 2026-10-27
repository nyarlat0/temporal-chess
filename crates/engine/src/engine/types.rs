use std::collections::{BTreeMap, HashMap};
use std::fmt;

use crate::board::{Board, PreBoard};
use crate::model::{Move, MoveId, PieceAt, ShortPieceId, Worldline};
use crate::pieces::{Color, PieceView};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinReason {
    Checkmate,
    KingCaptured,
    Paradox,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameOutcome {
    Victory { winner: Color, reason: WinReason },
    Stalemate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitingReason {
    NoActivePieces,
    OpponentPastMoves,
    KingUnderAttack,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Calculation {
    pub interactive: bool,
    pub acknowledged: Vec<PieceAt>,
    pub responsible: Option<Color>,
}

/// Authoritative state. Board projections and lookup tables are never stored here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct History {
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
        self.projection
            .boards
            .values()
            .flat_map(|board| board.pieces().iter().copied())
            .filter(|piece| {
                self.outcome().is_none()
                    && piece.color == self.turn
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
        if self.outcome().is_some() {
            return Err(EngineErr::GameOver);
        }
        let actor = self.turn;
        let mut candidate = self.clone();
        candidate.history.calculation.acknowledged.clear();
        candidate.history.calculation.responsible = Some(actor);
        match command(&mut candidate).and_then(|()| candidate.finish_command(actor)) {
            Ok(()) => {
                *self = candidate;
                Ok(())
            }
            Err(EngineErr::InfLoop) => {
                // Keep the last coherent timeline, but publish the loss. This is
                // a completed game command, not a retryable invalid move.
                self.history.status = GameStatus::Finished(GameOutcome::Victory {
                    winner: actor.opposite(),
                    reason: WinReason::Paradox,
                });
                Ok(())
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
        }
    }
}

impl std::error::Error for EngineErr {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::BoardErr(error) => Some(error),
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
