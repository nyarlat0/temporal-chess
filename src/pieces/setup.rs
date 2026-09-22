use super::{Color, PieceKind, Sq};

/// Stable worldline order: white back rank, white pawns, black back rank, black pawns.
pub(crate) fn initial_position() -> Vec<(PieceKind, Color, Sq)> {
    use PieceKind::*;
    let back_rank = [
        R { moved: false },
        N,
        B,
        Q,
        K { moved: false },
        B,
        N,
        R { moved: false },
    ];
    let mut pieces = Vec::with_capacity(32);
    for (color, back_y, pawn_y) in [(Color::White, 0, 1), (Color::Black, 7, 6)] {
        pieces.extend(
            back_rank
                .into_iter()
                .enumerate()
                .map(|(x, kind)| (kind, color, Sq(x, back_y))),
        );
        pieces.extend((0..8).map(|x| (P { moved: false }, color, Sq(x, pawn_y))));
    }
    pieces
}
