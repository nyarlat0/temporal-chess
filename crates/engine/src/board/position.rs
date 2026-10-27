use crate::pieces::{Color, PieceView, Sq};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Board {
    pub(crate) age: usize,
    pub(crate) pieces: Vec<PieceView>,
    occ: [[Option<usize>; 8]; 8],
}

impl Default for Board {
    fn default() -> Self {
        let mut board = Self::empty(0);
        for (origin, (kind, color, sq)) in crate::pieces::initial_position().into_iter().enumerate()
        {
            let state = crate::model::State::new(kind, color, sq);
            board.add_piece(PieceView::from_state(&state, origin, 0));
        }
        board
    }
}

impl Board {
    pub fn empty(t: usize) -> Self {
        Self {
            age: t,
            pieces: Vec::new(),
            occ: [[None; 8]; 8],
        }
    }

    pub fn time(&self) -> usize {
        self.age
    }

    pub fn pieces(&self) -> &[PieceView] {
        &self.pieces
    }

    pub fn is_empty(&self, sq: Sq) -> bool {
        sq.on_board() && self.get_piece(sq.0, sq.1).is_none()
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
        self.get_piece(sq.0, sq.1).map(|p| p.color)
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

    /// Remove a piece while maintaining the square-to-piece index.
    pub fn remove_piece(&mut self, sq: Sq) {
        if !sq.on_board() {
            return;
        }
        let Some(id) = self.occ[sq.0][sq.1].take() else {
            return;
        };
        self.pieces.swap_remove(id);
        if let Some(moved) = self.pieces.get(id) {
            self.occ[moved.sq.0][moved.sq.1] = Some(id);
        }
    }

    /// Insert or replace a piece on its square (use PreBoard for intersections).
    /// Panics if the square is outside the board.
    pub fn add_piece(&mut self, piv: PieceView) {
        assert!(piv.sq.on_board(), "piece square is outside the board");
        self.remove_piece(piv.sq);
        self.occ[piv.sq.0][piv.sq.1] = Some(self.pieces.len());
        self.pieces.push(piv);
    }
}
