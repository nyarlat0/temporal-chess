mod actions;
mod boards;
mod collision_rules;
mod collisions;
mod history;
mod initialization;
mod move_generation;
mod temporal;
mod timeline;
mod types;
mod validation;

pub use crate::model::{
    Castling, Move, MoveEffect, MoveId, MoveRights, PieceAt, PieceId, Promotion, ShortPieceId,
    State, TimeDirection,
};
pub use types::{BoardConflict, BoardErr, Engine, EngineErr, History};
use types::{PieceCollision, Projection, ValidRes};

#[cfg(test)]
mod temporal_tests;
#[cfg(test)]
mod tests;
