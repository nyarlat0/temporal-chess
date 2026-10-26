use crate::pieces::{Color, PieceKind};

use super::{
    CheckStatus, Engine, EngineErr, GameOutcome, GameStatus, TimeDirection, WaitingReason,
    WinReason,
};

impl Engine {
    /// Threats on every extant royal occurrence. An inverted threat comes from
    /// an active enemy on t+1 and uses blockers on t, irrespective of its current
    /// orientation: any piece may choose a backward move.
    pub fn checks(&self, side: Color) -> Result<CheckStatus, EngineErr> {
        let mut checks = CheckStatus::default();
        for (&t, board) in &self.projection.boards {
            for king in board
                .pieces()
                .iter()
                .filter(|p| p.color == side && matches!(p.kind, PieceKind::K { .. }))
            {
                let id = Self::piece_at(*king);
                let mut ordinary =
                    t == self.history.horizon && board.is_attacked(side.opposite(), king.sq);
                for (source_t, direction) in [
                    (t.checked_sub(1), TimeDirection::Future),
                    (t.checked_add(1), TimeDirection::Past),
                ] {
                    let Some(source) = source_t.and_then(|time| self.board(time)) else {
                        continue;
                    };
                    if direction == TimeDirection::Past && source.time() <= 1 {
                        continue;
                    }
                    for enemy in source
                        .pieces()
                        .iter()
                        .filter(|p| p.color != side && p.can_move() && p.is_movable())
                    {
                        let target = self.move_board(*enemy, direction)?;
                        if enemy.basic_moves(&target).contains(&king.sq) {
                            match direction {
                                TimeDirection::Future => ordinary = true,
                                TimeDirection::Past => checks.inverted.push(id),
                            }
                            break;
                        }
                    }
                }
                if ordinary {
                    checks.ordinary.push(id);
                }
            }
        }
        Ok(checks)
    }

    /// Whether an opponent still owes an ordinary move in existing past time.
    pub fn has_past_debt(&self, side: Color) -> bool {
        self.history
            .worldlines
            .iter()
            .flat_map(|line| line.iter())
            .any(|state| {
                state.alive
                    && state.color == side
                    && state.rights.ordinary
                    && Color::from_board(state.t) == side
                    && state.t < self.history.horizon
            })
    }

    /// Endgame adjudication needs both royal worldlines. Kingless setup fixtures
    /// remain usable as analysis positions, as they were before game outcomes.
    fn has_both_kings(&self) -> bool {
        [Color::White, Color::Black].into_iter().all(|side| {
            self.history.worldlines.iter().any(|line| {
                line.iter()
                    .any(|s| s.color == side && matches!(s.kind, PieceKind::K { .. }))
            })
        })
    }

    fn king_died(&self, side: Color) -> bool {
        self.history
            .worldlines
            .iter()
            .flat_map(|line| line.iter())
            .any(|s| s.color == side && !s.alive && matches!(s.kind, PieceKind::K { .. }))
    }

    /// Probe commands without recursively adjudicating their resulting turns.
    /// Geometric candidates rejected during reconciliation do not avert a draw.
    fn can_act(&self, side: Color) -> Result<bool, EngineErr> {
        if self.has_past_debt(side) {
            return Ok(true);
        }
        let mut prepared = self.clone();
        prepared.turn = side;
        prepared.history.status = GameStatus::Ongoing;
        prepared.reconcile()?;
        let sources: Vec<_> = prepared.move_sources().collect();
        for piece in sources {
            if piece.rights.ordinary && piece.inverted && piece.t > 0 {
                return Ok(true);
            }
            for mv in prepared.possible_moves(piece.origin, piece.age)? {
                let mut probe = prepared.clone();
                let result = probe.apply_move_inner(mv).and_then(|()| {
                    probe.turn = side.opposite();
                    probe.reconcile()
                });
                if (result.is_ok() && probe.history.moves.get(&mv.id()) == Some(&mv))
                    || result == Err(EngineErr::InfLoop)
                {
                    // A player may choose a paradox-causing command and lose;
                    // it must not be silently reclassified as stalemate.
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    pub(super) fn finish_command(&mut self, actor: Color) -> Result<(), EngineErr> {
        if self.outcome().is_some() {
            return Ok(());
        }
        // If both kings perish in a causal collision, the command's author
        // cannot claim victory by destroying their own royal worldline.
        for loser in [actor, actor.opposite()] {
            if self.king_died(loser) {
                self.history.status = GameStatus::Finished(GameOutcome::Victory {
                    winner: loser.opposite(),
                    reason: WinReason::KingCaptured,
                });
                return Ok(());
            }
        }
        if !self.has_both_kings() {
            return Ok(());
        }

        // A free wait by the opponent can close the last debt. Re-evaluate the
        // waiting player without consuming the opponent's still-current turn.
        if let GameStatus::Waiting { player, .. } = self.status()
            && player != self.turn
            && !self.can_act(player)?
        {
            return self.set_blocked(player, false);
        }
        self.history.status = GameStatus::Ongoing;
        if !self.can_act(self.turn)? {
            self.set_blocked(self.turn, true)?;
        }
        Ok(())
    }

    fn set_blocked(&mut self, player: Color, hand_over: bool) -> Result<(), EngineErr> {
        let threatened = self.checks(player)?.is_check();
        let debt = self.has_past_debt(player.opposite());
        if !threatened && !debt {
            self.history.status = GameStatus::Finished(GameOutcome::Stalemate);
            return Ok(());
        }
        self.history.status = GameStatus::Waiting {
            player,
            reason: if debt {
                WaitingReason::OpponentPastMoves
            } else {
                WaitingReason::KingUnderAttack
            },
        };
        if hand_over {
            self.turn = player.opposite();
            // A checked player with no commands must still allow an actual
            // royal capture. If the opponent has no turn-board yet, a forced
            // pass creates exactly one passive board (not a free player skip).
            if threatened && !debt && Color::from_board(self.history.horizon) == player {
                let end = self.history.horizon;
                let next = end.checked_add(1).ok_or(EngineErr::WrongMove)?;
                self.prolongate_wlines(end, None)?;
                self.history.horizon = next;
            }
            self.reconcile()?;
        }
        // No recursive handovers: two blocked sides never busy-loop.
        Ok(())
    }
}
