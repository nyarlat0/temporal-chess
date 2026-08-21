use std::collections::HashMap;

use arrayvec::ArrayVec;

use crate::pieces::{Color, PieceKind};

use super::{Engine, State};

impl Default for Engine {
    fn default() -> Self {
        // Board coordinate system:
        // (x, y) with x=0..7 left->right, y=0..7 bottom->top.
        // White starts at y=0/1, Black at y=7/6.

        let mut wlines = Vec::new();
        let pbuf = HashMap::new();

        // Helper: create initial state at t=0, fut_seen=0, loop_turn=0.
        let mut spawn = |kind: PieceKind, color: Color, x: usize, y: usize| {
            let st = State {
                t: 0,
                x,
                y,
                fut_seen: 0,
                loop_turn: 0,
                alive: true,
                active: true,
                kind,
                color,
                inverted: false,
                informed: ArrayVec::new(),
            };

            // one worldline per piece, age 0 only
            wlines.push(vec![st]);
        };

        // --- WHITE ---
        let w = Color::White;

        // rank 0 (y=0)
        spawn(PieceKind::R { moved: false }, w, 0, 0);
        spawn(PieceKind::N, w, 1, 0);
        spawn(PieceKind::B, w, 2, 0);
        spawn(PieceKind::Q, w, 3, 0);
        spawn(PieceKind::K { moved: false }, w, 4, 0);
        spawn(PieceKind::B, w, 5, 0);
        spawn(PieceKind::N, w, 6, 0);
        spawn(PieceKind::R { moved: false }, w, 7, 0);

        // rank 1 (y=1)
        for x in 0..8 {
            spawn(PieceKind::P { moved: false }, w, x, 1);
        }

        // --- BLACK ---
        let b = Color::Black;

        // rank 7 (y=7)
        spawn(PieceKind::R { moved: false }, b, 0, 7);
        spawn(PieceKind::N, b, 1, 7);
        spawn(PieceKind::B, b, 2, 7);
        spawn(PieceKind::Q, b, 3, 7);
        spawn(PieceKind::K { moved: false }, b, 4, 7);
        spawn(PieceKind::B, b, 5, 7);
        spawn(PieceKind::N, b, 6, 7);
        spawn(PieceKind::R { moved: false }, b, 7, 7);

        // rank 6 (y=6)
        for x in 0..8 {
            spawn(PieceKind::P { moved: false }, b, x, 6);
        }

        Self {
            wlines,
            move_history: HashMap::new(),
            preboards: HashMap::new(),
            boards: HashMap::new(),
            pbuf,
            turn: Color::White,
            action_lines: (Some(0), None),
        }
    }
}
