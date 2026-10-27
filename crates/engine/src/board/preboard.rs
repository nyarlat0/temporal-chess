use crate::pieces::PieceView;

use super::Board;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreBoard {
    pub t: usize,
    pub space: [[Vec<PieceView>; 8]; 8],
}

impl PreBoard {
    pub fn new(t: usize) -> Self {
        let space = std::array::from_fn(|_| std::array::from_fn(|_| Vec::new()));

        Self { t, space }
    }

    pub fn is_valid(&self) -> bool {
        for row in self.space.iter() {
            for pieces in row.iter() {
                if pieces.len() > 1 {
                    return false;
                }
            }
        }

        true
    }

    pub fn try_into_board(&self) -> Option<Board> {
        if !self.is_valid() {
            return None;
        }

        let mut board = Board::empty(self.t);
        for (x, column) in self.space.iter().enumerate() {
            for (y, cell) in column.iter().enumerate() {
                if let Some(piece) = cell.first() {
                    if piece.sq != crate::pieces::Sq(x, y) || piece.t != self.t {
                        return None;
                    }
                    board.add_piece(*piece);
                }
            }
        }

        Some(board)
    }

    pub fn is_empty(&self) -> bool {
        for row in self.space.iter() {
            for pieces in row.iter() {
                if !pieces.is_empty() {
                    return false;
                }
            }
        }

        true
    }

    pub fn find_piv(&self, origin: usize, age: usize) -> Option<PieceView> {
        for row in self.space.iter() {
            for pieces in row.iter() {
                let mb_p = pieces
                    .iter()
                    .find(|p| p.origin == origin && p.age == age)
                    .cloned();
                if mb_p.is_some() {
                    return mb_p;
                }
            }
        }

        None
    }

    pub fn get_mut_piv(&mut self, origin: usize, age: usize) -> Option<&mut PieceView> {
        for row in self.space.iter_mut() {
            for pieces in row.iter_mut() {
                let mb_p = pieces
                    .iter_mut()
                    .find(|p| p.origin == origin && p.age == age);
                if mb_p.is_some() {
                    return mb_p;
                }
            }
        }

        None
    }
}
