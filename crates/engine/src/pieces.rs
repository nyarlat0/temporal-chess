mod movement;
mod setup;

pub(crate) use setup::initial_position;

pub use crate::model::{Color, PieceKind, PieceView, Sq};
pub use movement::{BISHOP_DIRS, KING_DELTAS, KNIGHT_DELTAS, ROOK_DIRS};
