use crate::pieces::{Color, PieceKind, PieceView, Sq};

#[derive(Clone)]
pub struct Board {
    pub age: usize,
    pub pieces: Vec<PieceView>,
    pub occ: [[Option<usize>; 8]; 8],
}

impl Default for Board {
    fn default() -> Self {
        let mut board = Self {
            age: 0,
            pieces: Vec::new(),
            occ: [[None; 8]; 8],
        };

        // helper to insert piece
        let mut next_id = 0;
        let mut add = |board: &mut Board, kind: PieceKind, color: Color, sq: Sq| {
            let id = next_id;
            next_id += 1;

            let piece = PieceView {
                origin: id,
                age: 0,
                t: 0,
                fut_seen: 0,
                loop_turn: 0,
                sq,
                kind,
                color,
                inverted: false,
                active: true,
                can_react: false,
                move_cancelled: false,
            };

            board.occ[sq.0][sq.1] = Some(id);
            board.pieces.insert(id, piece);
        };

        // --- WHITE ---
        let white = Color::White;

        // White major pieces (rank 0)
        add(&mut board, PieceKind::R { moved: false }, white, Sq(0, 0)); // a1
        add(&mut board, PieceKind::N, white, Sq(1, 0)); // b1
        add(&mut board, PieceKind::B, white, Sq(2, 0)); // c1
        add(&mut board, PieceKind::Q, white, Sq(3, 0)); // d1
        add(&mut board, PieceKind::K { moved: false }, white, Sq(4, 0)); // e1
        add(&mut board, PieceKind::B, white, Sq(5, 0)); // f1
        add(&mut board, PieceKind::N, white, Sq(6, 0)); // g1
        add(&mut board, PieceKind::R { moved: false }, white, Sq(7, 0)); // h1

        // White pawns (rank 1)
        for x in 0..8 {
            add(&mut board, PieceKind::P { moved: false }, white, Sq(x, 1));
        }

        // --- BLACK ---
        let black = Color::Black;

        // Black major pieces (rank 7)
        add(&mut board, PieceKind::R { moved: false }, black, Sq(0, 7)); // a8
        add(&mut board, PieceKind::N, black, Sq(1, 7)); // b8
        add(&mut board, PieceKind::B, black, Sq(2, 7)); // c8
        add(&mut board, PieceKind::Q, black, Sq(3, 7)); // d8
        add(&mut board, PieceKind::K { moved: false }, black, Sq(4, 7)); // e8
        add(&mut board, PieceKind::B, black, Sq(5, 7)); // f8
        add(&mut board, PieceKind::N, black, Sq(6, 7)); // g8
        add(&mut board, PieceKind::R { moved: false }, black, Sq(7, 7)); // h8

        // Black pawns (rank 6)
        for x in 0..8 {
            add(&mut board, PieceKind::P { moved: false }, black, Sq(x, 6));
        }

        board
    }
}

impl Board {
    pub fn is_empty(&self, sq: Sq) -> bool {
        let Sq(x, y) = sq;
        self.occ[x][y].is_none()
    }

    pub fn is_fut_me(&self, piece: &PieceView, sq: Sq) -> bool {
        if let Some(p) = self.get_piece(sq.0, sq.1) {
            (p.age == piece.age + 1) && (p.origin == piece.origin)
        } else {
            false
        }
    }

    pub fn is_me(&self, piece: &PieceView, sq: Sq) -> bool {
        if let Some(p) = self.get_piece(sq.0, sq.1) {
            (p.age == piece.age) && (p.origin == piece.origin)
        } else {
            false
        }
    }

    pub fn color_at(&self, sq: Sq) -> Option<Color> {
        let Sq(x, y) = sq;
        self.occ[x][y].and_then(|i| self.pieces.get(i).map(|p| p.color))
    }

    pub fn is_enemy(&self, who: Color, sq: Sq) -> bool {
        match self.color_at(sq) {
            Some(c) => c != who,
            None => false,
        }
    }

    pub fn is_inv(&self, sq: Sq) -> bool {
        if let Some(p) = self.get_piece(sq.0, sq.1) {
            p.inverted
        } else {
            false
        }
    }

    pub fn get_piece(&self, x: usize, y: usize) -> Option<&PieceView> {
        if x >= 8 || y >= 8 {
            return None;
        }

        self.occ[x][y].and_then(|key| self.pieces.get(key))
    }

    pub fn get_mut_piece(&mut self, x: usize, y: usize) -> Option<&mut PieceView> {
        if x >= 8 || y >= 8 {
            return None;
        }

        self.occ[x][y].and_then(|key| self.pieces.get_mut(key))
    }

    pub fn find_by_origin(&self, origin: usize) -> Vec<PieceView> {
        self.pieces
            .iter()
            .filter(|p| p.origin == origin)
            .cloned()
            .collect()
    }

    pub fn has_piece(&self, origin: usize, age: usize) -> bool {
        self.pieces
            .iter()
            .any(|p| p.origin == origin && p.age == age)
    }

    /// Only for pretend-tests on boards!!!
    pub fn remove_piece(&mut self, sq: Sq) {
        let Some(id) = self.occ[sq.0][sq.1].take() else {
            return;
        };
        self.pieces.remove(id);
    }

    /// Only for pretend-tests on boards!!!
    pub fn add_piece(&mut self, piv: PieceView) {
        self.occ[piv.sq.0][piv.sq.1] = Some(self.pieces.len());
        self.pieces.push(piv);
    }
}
