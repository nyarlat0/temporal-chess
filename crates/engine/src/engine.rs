mod actions;
mod boards;
mod calculation;
mod collision_rules;
mod collisions;
mod history;
mod initialization;
mod move_generation;
mod outcome;
mod skipping;
mod temporal;
mod timeline;
mod types;
mod validation;

pub use crate::model::{
    Castling, Move, MoveEffect, MoveId, MoveRights, PieceAt, PieceId, Promotion, ShortPieceId,
    State, TimeDirection,
};
pub use types::{
    BoardConflict, BoardErr, CalculationStatus, CaptureOutcome, CheckStatus, Engine, EngineErr,
    GameOutcome, GameStatus, History, WaitingReason, WinReason,
};
use types::{PieceCollision, Projection, ValidRes};

#[cfg(test)]
mod calculation_tests;
#[cfg(test)]
mod outcome_tests;
#[cfg(test)]
mod temporal_tests;
#[cfg(test)]
mod tests;
