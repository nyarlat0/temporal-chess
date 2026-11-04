#![doc = include_str!("../../../README.md")]

pub mod board;
pub mod engine;
pub mod model;
pub mod pieces;
pub mod record;

/// Content identity of the engine sources, independent of native/WASM target.
pub const ENGINE_REVISION: &str = env!("NOLAN_ENGINE_REVISION");
