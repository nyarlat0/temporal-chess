mod actions;
mod boards;
mod calculation;
mod collision_rules;
mod collisions;
mod expansion;
mod history;
mod initialization;
mod lab;
pub use lab::{LabEdit, LabSetup};
mod move_generation;
mod move_wait;
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
    BoardConflict, BoardErr, CalculationStatus, Cancellation, CaptureOutcome, CheckStatus, Engine,
    EngineErr, GameOutcome, GameStatus, History, TurnStreak, WaitingReason, WinReason,
};
use types::{PieceCollision, Projection, ValidRes};

#[cfg(test)]
mod calculation_tests;
#[cfg(test)]
mod castling_tests;
#[cfg(test)]
mod outcome_tests;
#[cfg(test)]
mod temporal_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
fn legacy_from_worldlines(
    worldlines: Vec<Vec<State>>,
    turn: crate::model::Color,
) -> Result<Engine, EngineErr> {
    let mut engine = Engine::from_worldlines_with_rules(
        worldlines,
        turn,
        crate::model::GameRules {
            wait_before_move: true,
            global_reaction_reset: false,
            ..Default::default()
        },
    )?;
    engine.history.beginning = engine.history.beginning.min(0);
    engine.update_bufs()?;
    engine.reconcile()?;
    Ok(engine)
}

#[cfg(test)]
mod mode_tests;

#[cfg(test)]
mod continuation_tests;

#[cfg(test)]
mod information_tests;

#[cfg(test)]
mod new_rules_tests;
