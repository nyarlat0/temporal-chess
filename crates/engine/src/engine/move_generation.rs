use super::{
    Castling, Engine, EngineErr, Move, MoveEffect, MoveId, PieceId, Promotion, TimeDirection,
};
use crate::board::Board;
use crate::pieces::{Color, PieceKind, PieceView, Sq};

impl Engine {
    /// Necessary spatial conditions, independent of unresolved occupancy and
    /// attacks. Reuse movement rules on a permissive board: allow a pawn's
    /// potential capture and provide a rook when checking castling geometry.
    pub(super) fn recorded_move_has_geometry(&self, piece: PieceView, mv: Move) -> bool {
        let mut board = Board::empty(piece.t);
        if let Some(castling) = mv.castling() {
            let rook_x = if castling == Castling::Long { 0 } else { 7 };
            let home_y = if piece.color == Color::White { 0 } else { 7 };
            board.add_piece(PieceView {
                origin: usize::MAX,
                kind: PieceKind::R { moved: false },
                sq: Sq(rook_x, home_y),
                ..piece
            });
            return piece
                .castling_moves_with_attacks(&board, &board, |_| false)
                .contains(&(mv.sq, castling));
        }
        if matches!(piece.kind, PieceKind::P { .. }) && piece.sq.0 != mv.sq.0 {
            board.add_piece(PieceView {
                origin: usize::MAX,
                color: piece.color.opposite(),
                sq: mv.sq,
                ..piece
            });
        }
        piece.basic_moves(&board).contains(&mv.sq)
    }

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
        let computed = target_time.and_then(|t| self.projection.boards.get(&t));
        // A computed board always wins. Do not allocate an unused tentative
        // board on this hot path; fallback selection is otherwise identical.
        let tentative = if computed.is_none() {
            target_time
                .and_then(|t| self.projection.preboards.get(&t))
                .and_then(|preboard| preboard.try_into_board())
        } else {
            None
        };
        let target = computed.or(tentative.as_ref());
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
        // In Time War a frontier board already exists but is still empty.
        // Include the ordinary endpoints that this shared turn will continue,
        // using their own orientation (not the mover's chosen direction).
        if self.history.starting_position.fixed_end().is_some() {
            for (wid, line) in self.history.worldlines.iter().enumerate() {
                let age = line.len() - 1;
                let state = &line[age];
                let idle = PieceView::from_state(state, wid, age);
                if wid == piece.origin
                    || state.t != piece.t
                    || !state.alive
                    || !state.rights.ordinary
                    || !idle.same_order(piece)
                {
                    continue;
                }
                if let Some(next) = self.passive_successor(state)
                    && Some(next.t) == target_time
                    && board.is_empty(idle.sq)
                {
                    board.add_piece(PieceView::from_state(&next, wid, age + 1));
                }
            }
        }
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
        self.candidate_moves(wid, age)
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

    /// Resolve a legal candidate's encounter without modifying the live game.
    /// Unlike `move_capture_target`, occupancy alone does not imply a capture.
    /// Analysis runs past interactive barriers and examines the resulting states,
    /// even if the initiating move was cancelled. Errors/paradoxes stay unresolved.
    pub fn preview_capture(&self, mv: Move) -> Option<(PieceView, super::CaptureOutcome)> {
        use super::CaptureOutcome;
        let target = self.move_capture_target(mv)?;
        let mut probe = self.clone();
        probe.history.calculation.interactive = false;
        let result = (|| -> Result<CaptureOutcome, EngineErr> {
            let next = probe
                .piece_state(mv.source())
                .and_then(|state| state.step(mv.direction))
                .ok_or(EngineErr::WrongPiece)?;
            let attacker = super::PieceAt {
                piece: PieceId::from_state(&next, mv.pid.wid),
                t: next.t,
            };
            probe.apply_move_inner(mv)?;
            // The recorded target refers to the actual arrival occurrence,
            // including when a new board was created using source occupancy.
            let victim = *probe
                .history
                .targets
                .get(&mv.id())
                .and_then(|targets| targets.first())
                .ok_or(EngineErr::WrongPiece)?;
            probe.turn = probe.turn.opposite();
            probe.reconcile()?;
            match probe.piece_state(victim) {
                Some(state) if !state.alive => Ok(CaptureOutcome::Captured),
                Some(_)
                    if probe
                        .piece_state(attacker)
                        .is_some_and(|state| !state.alive) =>
                {
                    Ok(CaptureOutcome::AttackerLost)
                }
                _ => Ok(CaptureOutcome::Unresolved),
            }
        })();
        Some((target, result.unwrap_or(CaptureOutcome::Unresolved)))
    }

    /// Historical validation must not recursively apply the rules for a new
    /// command to recorded moves; future edits may have changed their threats.
    pub(super) fn candidate_moves(&self, wid: usize, age: usize) -> Result<Vec<Move>, EngineErr> {
        self.candidates_for(wid, age, None)
    }

    pub(super) fn recorded_move_candidates(
        &self,
        wid: usize,
        age: usize,
        recorded: Move,
    ) -> Result<Vec<Move>, EngineErr> {
        self.candidates_for(wid, age, Some(recorded))
    }

    /// A completed collision may kill the mover exactly at arrival. Its winner
    /// is a consequence of that old move, not a new obstacle that cancels it.
    /// This adjustment belongs only to revalidation, never to newly offered moves.
    fn restore_lost_arrival(&self, piece: PieceView, recorded: Move, board: &mut Board) {
        use super::collision_rules::CollisionEffect;
        let line = &self.history.worldlines[piece.origin];
        let Some(arrival) = line.get(piece.age + 1) else {
            return;
        };
        if line.gap_after(piece.age).is_some()
            || arrival.alive
            || recorded.direction.step(piece.t) != Some(arrival.t)
            || board.time() != arrival.t
            || Sq(arrival.x, arrival.y) != recorded.sq
        {
            return;
        }
        let victim = PieceView::from_state(arrival, piece.origin, piece.age + 1);
        let Some(winner) = board.get_piece(arrival.x, arrival.y).copied() else {
            return;
        };
        let Some(effects) = self.encounter_effects(victim, winner) else {
            return;
        };
        let killed_on_arrival = effects.iter().any(|effect| {
            matches!(effect, CollisionEffect::KillFrom(id)
                if id.wid == victim.origin && id.age <= victim.age)
        });
        if !killed_on_arrival {
            return;
        }
        board.remove_piece(recorded.sq);
        // Retain the original capture geometry (including diagonal pawn moves).
        // The ordinary move-board restoration could not restore a victim while
        // this collision's surviving piece occupied the destination.
        if let Some(targets) = self.history.targets.get(&recorded.id()) {
            for &target in targets {
                if let Some(target) = self.occurrence(target)
                    && target.t == board.time()
                    && board.is_empty(target.sq)
                {
                    board.add_piece(target);
                }
            }
        }
    }

    fn candidates_for(
        &self,
        wid: usize,
        age: usize,
        recorded: Option<Move>,
    ) -> Result<Vec<Move>, EngineErr> {
        let piece = self.get_piv(wid, age).ok_or(EngineErr::WrongPiece)?;
        let source = self.board(piece.t).ok_or(EngineErr::AbsentBoard)?;
        let candidate = |sq, direction, effect| Move {
            pid: PieceId::from_piv(piece),
            t: piece.t,
            sq,
            direction,
            effect,
        };
        let mut moves = Vec::new();
        for direction in [TimeDirection::Future, TimeDirection::Past] {
            if !self.time_direction_available(piece, direction) {
                continue;
            }
            let mut board = match self.move_board(piece, direction) {
                Ok(board) => board,
                Err(EngineErr::AbsentBoard) => continue,
                Err(error) => return Err(error),
            };
            if let Some(recorded) = recorded.filter(|mv| mv.direction == direction) {
                self.restore_lost_arrival(piece, recorded, &mut board);
            }
            let mut directed = Vec::new();
            for square in piece.basic_moves(&board) {
                directed.push(candidate(square, direction, MoveEffect::Normal));
            }
            // Backward castling requires both participants to already be inverted.
            if direction == TimeDirection::Future || piece.inverted {
                for (square, castling) in
                    piece.castling_moves_with_attacks(&board, source, |_| false)
                {
                    let rook_x = if castling == Castling::Long { 0 } else { 7 };
                    if direction == TimeDirection::Past
                        && !source
                            .get_piece(rook_x, piece.sq.1)
                            .is_some_and(|rook| rook.inverted)
                    {
                        continue;
                    }
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
            // Checks are informational: even a move leaving the king attacked
            // can repair its history after reconciliation. Never reject it here.
            moves.extend(directed);
        }
        Ok(moves)
    }

    /// A visible geometric attack is a threat only if this incarnation can
    /// actually act. Passive pieces must survive to their next own-color board
    /// in their temporal orientation and still have a move there.
    pub(super) fn spatially_attacked(&self, board: &Board, by: Color, sq: Sq) -> bool {
        board.is_attacked_by(by, sq, |piece| {
            if piece.can_move() {
                return piece.is_movable();
            }
            let Some(state) = self.piece_state(Self::piece_at(*piece)) else {
                return false;
            };
            let Some(next) = self.passive_successor(state) else {
                return false;
            };
            let next_id = super::PieceAt {
                piece: PieceId::from_state(&next, piece.origin),
                t: next.t,
            };
            if let Some(actual) = self.piece_state(next_id) {
                return actual.alive
                    && actual.is_movable()
                    && actual.x == piece.sq.0
                    && actual.y == piece.sq.1
                    && actual.kind == piece.kind
                    && actual.inverted == piece.inverted;
            }
            // A new future board has not been created yet: its ordinary enemy
            // replies still count. Absence/death inside existing history does
            // not invent a new opportunity to move.
            !piece.inverted && next.t > self.history.horizon() && next.alive && next.is_movable()
        })
    }
}
