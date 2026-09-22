use std::collections::{BTreeMap, HashMap};
use std::fmt;

use crate::board::{Board, PreBoard};
use crate::model::{Move, MoveId, PieceAt, ShortPieceId, State};
use crate::pieces::{Color, PieceView};

/// Authoritative state. Board projections and lookup tables are never stored here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct History {
    pub(super) worldlines: Vec<Vec<State>>,
    pub(super) moves: BTreeMap<MoveId, Move>,
}

impl History {
    pub fn worldlines(&self) -> &[Vec<State>] {
        &self.worldlines
    }
    pub fn moves(&self) -> &BTreeMap<MoveId, Move> {
        &self.moves
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
            .filter(|piece| piece.color == self.turn && piece.can_move() && piece.is_movable())
    }

    /// Publish a command's result only after all its checks have succeeded.
    pub(super) fn transaction<T>(
        &mut self,
        command: impl FnOnce(&mut Self) -> Result<T, EngineErr>,
    ) -> Result<T, EngineErr> {
        let mut candidate = self.clone();
        let result = command(&mut candidate)?;
        *self = candidate;
        Ok(result)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ValidRes {
    HistChanged,
    Unchanged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineErr {
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
