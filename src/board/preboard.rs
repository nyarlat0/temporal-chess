use crate::pieces::PieceView;

use super::Board;

#[derive(Clone, PartialEq, Eq)]
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

        let preboard = self;
        let t = preboard.t;
        let mut pieces = Vec::new();
        let mut occ = [[None; 8]; 8];

        for (y, row) in preboard.space.iter().enumerate() {
            for (x, prep) in row.iter().enumerate() {
                if prep.len() > 1 {
                    return None;
                } else {
                    if let Some(p) = prep.first() {
                        let idx = pieces.len();
                        pieces.push(*p);
                        occ[x][y] = Some(idx);
                    }
                }
            }
        }

        Some(Board {
            pieces,
            occ,
            age: t,
        })
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
