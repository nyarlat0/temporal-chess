use super::{
    Castling, Engine, EngineErr, Move, MoveEffect, MoveId, PieceId, Promotion, TimeDirection,
};
use crate::board::Board;
use crate::pieces::{Color, PieceKind, PieceView};

impl Engine {
    /// Board on which movement is tested, with this move's own old effects undone.
    /// Existing target boards take precedence; only forward moves may fall back.
    pub(super) fn move_board(
        &self,
        piece: PieceView,
        direction: TimeDirection,
    ) -> Result<Board, EngineErr> {
        let target_time = direction.step(piece.t);
        // A conflict-free preboard is usable as a tentative destination even
        // when reconciliation is paused before it. Never publish it as a Board.
        let tentative = target_time
            .and_then(|t| self.projection.preboards.get(&t))
            .and_then(|preboard| preboard.try_into_board());
        let target = target_time
            .and_then(|t| self.projection.boards.get(&t))
            .or(tentative.as_ref());
        if target.is_none() && target_time.is_some_and(|t| self.preboard(t).is_some()) {
            return Err(EngineErr::AbsentBoard);
        }
        let target_exists = target.is_some();
        let selected = match target {
            Some(board) => board,
            None if direction == TimeDirection::Future => self
                .projection
                .boards
                .get(&piece.t)
                .ok_or(EngineErr::AbsentBoard)?,
            None => return Err(EngineErr::AbsentBoard),
        };
        let mut board = selected.clone();
        if target_exists {
            let state = &self.history.worldlines[piece.origin][piece.age];
            if let Some(next) = state.step(direction) {
                let key = super::PieceAt {
                    piece: PieceId::from_state(&next, piece.origin),
                    t: next.t,
                };
                if let Some(own) = self.occurrence(key)
                    && board
                        .get_piece(own.sq.0, own.sq.1)
                        .is_some_and(|p| p.origin == own.origin && p.age == own.age)
                {
                    board.remove_piece(own.sq);
                }
            }
        } else if board.is_me(&piece, piece.sq) {
            board.remove_piece(piece.sq);
        }
        if let Some(old) = self
            .history
            .moves
            .get(&MoveId::from_piv(piece))
            .filter(|mv| mv.pid == PieceId::from_piv(piece) && mv.direction == direction)
        {
            if let Some(targets) = self.history.targets.get(&old.id()) {
                for &target in targets {
                    if let Some(victim) = self.occurrence(target)
                        && victim.t == board.time()
                        && board.is_empty(victim.sq)
                    {
                        board.add_piece(victim);
                    }
                }
            }
            if old.castling().is_some()
                && let Some(rook) = self.castling_rook(*old, piece)?
            {
                let successor = self.get_piv_necro(rook.origin, rook.age + 1);
                if let Some(successor) = successor
                    && board.is_me(&successor, successor.sq)
                {
                    board.remove_piece(successor.sq);
                }
                if board.is_empty(rook.sq) {
                    board.add_piece(rook);
                }
            }
        }
        Ok(board)
    }

    /// Candidate movement is evaluated on t+1 (or t when absent) / t-1.
    /// Turn ownership and the reason a piece may act are checked by the command.
    pub fn possible_moves(&self, wid: usize, age: usize) -> Result<Vec<Move>, EngineErr> {
        let mut moves = self.candidate_moves(wid, age)?;
        let piece = self.get_piv(wid, age).ok_or(EngineErr::WrongPiece)?;
        if piece.can_move()
            && piece.is_movable()
            && self
                .history
                .worldlines
                .iter()
                .flat_map(|line| line.iter())
                .any(|s| s.alive && s.color == piece.color && matches!(s.kind, PieceKind::K { .. }))
        {
            moves.retain(|mv| self.inverse_safe(*mv, piece.color));
        }
        Ok(moves)
    }

    /// Capture marker for a candidate, using exactly the same temporal target
    /// (including restored recorded targets) as the movement rules.
    pub fn move_captures(&self, mv: Move) -> bool {
        self.move_capture_target(mv).is_some()
    }

    /// Victim on the effective target board, including en passant and restored
    /// targets of a replaced move. The caller supplies a legal candidate move.
    pub fn move_capture_target(&self, mv: Move) -> Option<PieceView> {
        let piece = self.get_piv_from_mv(mv)?;
        let board = self.move_board(piece, mv.direction).ok()?;
        let sq = mv.en_passant().unwrap_or(mv.sq);
        board
            .get_piece(sq.0, sq.1)
            .filter(|p| p.color != piece.color)
            .copied()
    }

    /// Historical validation must not recursively apply the rules for a new
    /// command to recorded moves; future edits may have changed their threats.
    pub(super) fn candidate_moves(&self, wid: usize, age: usize) -> Result<Vec<Move>, EngineErr> {
        let piece = self.get_piv(wid, age).ok_or(EngineErr::WrongPiece)?;
        if self.board(piece.t).is_none() {
            return Err(EngineErr::AbsentBoard);
        }
        let candidate = |sq, direction, effect| Move {
            pid: PieceId::from_piv(piece),
            t: piece.t,
            sq,
            direction,
            effect,
        };
        let mut moves = Vec::new();
        for direction in [TimeDirection::Future, TimeDirection::Past] {
            if direction == TimeDirection::Past && piece.t <= 1 {
                continue;
            }
            let board = match self.move_board(piece, direction) {
                Ok(board) => board,
                Err(EngineErr::AbsentBoard) => continue,
                Err(error) => return Err(error),
            };
            let mut directed = Vec::new();
            for square in piece.basic_moves(&board) {
                directed.push(candidate(square, direction, MoveEffect::Normal));
            }
            // Preserve the prototype's forward-only castling rule.
            if direction == TimeDirection::Future {
                for (square, castling) in piece.castling_moves(&board) {
                    directed.push(candidate(square, direction, MoveEffect::Castle(castling)));
                }
            }
            let previous_time = match direction {
                TimeDirection::Future => piece.t.checked_sub(1),
                TimeDirection::Past => piece.t.checked_add(1),
            };
            if let Some(previous) = previous_time.and_then(|t| self.projection.preboards.get(&t)) {
                for (square, captured) in piece.en_passant_moves(&board, previous) {
                    directed.push(candidate(
                        square,
                        direction,
                        MoveEffect::EnPassant { captured },
                    ));
                }
            }
            if matches!(piece.kind, PieceKind::P { .. }) {
                let last_y = if piece.color == Color::White { 7 } else { 0 };
                directed = directed
                    .into_iter()
                    .flat_map(|mv| {
                        if mv.sq.1 == last_y {
                            Promotion::ALL
                                .into_iter()
                                .map(|kind| Move {
                                    effect: MoveEffect::Promote(kind),
                                    ..mv
                                })
                                .collect()
                        } else {
                            vec![mv]
                        }
                    })
                    .collect();
            }
            directed.retain(|mv| {
                // Royal capture ends the game. In particular an available
                // inverse capture cannot be suppressed by an unrelated check.
                if board.get_piece(mv.sq.0, mv.sq.1).is_some_and(|target| {
                    target.color != piece.color && matches!(target.kind, PieceKind::K { .. })
                }) {
                    return true;
                }
                let mut pretend = board.clone();
                // Source-square occupants of a different time are not the mover.
                pretend.remove_piece(mv.sq);
                if let Some(square) = mv.en_passant() {
                    pretend.remove_piece(square);
                }
                if let Some(castling) = mv.castling() {
                    let rook_x = match castling {
                        Castling::Long => 0,
                        Castling::Short => 7,
                    };
                    if let Some(rook) = pretend.get_piece(rook_x, piece.sq.1).copied() {
                        pretend.remove_piece(rook.sq);
                        pretend.add_piece(rook.apply_mv(*mv));
                    }
                }
                pretend.add_piece(piece.apply_mv(*mv));
                pretend
                    .pieces
                    .iter()
                    .filter(|p| p.color == piece.color && matches!(p.kind, PieceKind::K { .. }))
                    .all(|king| !pretend.is_attacked(piece.color.opposite(), king.sq))
            });
            moves.extend(directed);
        }
        Ok(moves)
    }

    pub(super) fn inverse_safe(&self, mv: Move, side: Color) -> bool {
        if !self
            .history
            .worldlines
            .iter()
            .flat_map(|line| line.iter())
            .any(|s| s.alive && s.color == side && matches!(s.kind, PieceKind::K { .. }))
        {
            return true;
        }
        let mut probe = self.clone();
        probe.turn = side;
        probe.history.calculation.interactive = false;
        let result = probe.apply_move_inner(mv).and_then(|()| {
            probe.turn = side.opposite();
            probe.reconcile()
        });
        match result {
            Err(EngineErr::InfLoop) => true, // The existing paradox loss still applies.
            Err(_) => false,
            Ok(()) => {
                if probe
                    .history
                    .worldlines
                    .iter()
                    .flat_map(|line| line.iter())
                    .any(|s| !s.alive && s.color != side && matches!(s.kind, PieceKind::K { .. }))
                {
                    return true; // Actual royal capture already ends the game.
                }
                probe
                    .checks(side)
                    .is_ok_and(|checks| checks.inverted.is_empty())
            }
        }
    }
}
