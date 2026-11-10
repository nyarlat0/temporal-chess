use crate::pieces::{Color, PieceKind};

use super::{
    CheckStatus, Engine, EngineErr, GameOutcome, GameStatus, TimeDirection, WaitingReason,
    WinReason,
};

impl Engine {
    fn has_active_pieces(&self, side: Color) -> bool {
        self.projection
            .boards
            .values()
            .flat_map(|b| b.pieces())
            .any(|p| p.color == side && p.can_move() && p.is_movable())
    }
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
                let mut ordinary = self.spatially_attacked(board, side.opposite(), king.sq);
                for (source_t, direction) in [
                    (t.checked_sub(1), TimeDirection::Future),
                    (t.checked_add(1), TimeDirection::Past),
                ] {
                    let Some(source) = source_t.and_then(|time| self.board(time)) else {
                        continue;
                    };
                    for enemy in source
                        .pieces()
                        .iter()
                        .filter(|p| p.color != side && p.can_move() && p.is_movable())
                    {
                        if !self.time_direction_available(*enemy, direction) {
                            continue;
                        }
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
        // In a finite two-front game the remote army's starting board is not
        // this side's present. Its natural frontier advances from its own end.
        let fixed = self.history.starting_position.fixed_end().is_some();
        let times = self
            .history
            .worldlines
            .iter()
            .flat_map(|line| line.iter())
            .filter(|state| {
                state.alive && state.color == side && Color::from_board(state.t) == side
            })
            .map(|state| state.t);
        let frontier = if fixed && side == Color::Black {
            times.min().unwrap_or(self.history.horizon)
        } else if fixed {
            times.max().unwrap_or(0)
        } else {
            self.history.horizon
        };
        self.history
            .worldlines
            .iter()
            .flat_map(|line| line.iter())
            .any(|state| {
                state.alive
                    && state.color == side
                    && state.rights.ordinary
                    && Color::from_board(state.t) == side
                    && if fixed && side == Color::Black {
                        state.t > frontier
                    } else {
                        state.t < frontier
                    }
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
    /// Only surviving, non-paradoxical continuations count as a defence.
    /// The initiating move may still cancel itself during reconciliation.
    fn can_act(&self, side: Color) -> Result<bool, EngineErr> {
        let mut prepared = self.clone();
        prepared.turn = side;
        if prepared.can_decline_reactions() {
            return Ok(true);
        }
        prepared.history.calculation.interactive = false;
        prepared.history.status = GameStatus::Ongoing;
        prepared.reconcile()?;
        let sources: Vec<_> = prepared.move_sources().collect();
        for piece in sources {
            if piece.rights.ordinary {
                for t in prepared.possible_skips(Self::piece_at(piece)) {
                    let mut probe = prepared.clone();
                    let result = probe.skip_with_information(Self::piece_at(piece), t);
                    if result.is_ok() && !probe.king_died(side) {
                        return Ok(true);
                    }
                }
            }
            for mv in prepared.candidate_moves(piece.origin, piece.age)? {
                let mut probe = prepared.clone();
                let result = probe.apply_move_inner(mv).and_then(|()| {
                    probe.turn = side.opposite();
                    probe.reconcile()
                });
                if result.is_ok() && !probe.king_died(side) {
                    // A paradox is still a playable losing command, but it
                    // cannot serve as a defence against an otherwise final mate.
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
        // Neither king loss nor stalemate is final while the future is unresolved.
        if self.calculation_barrier().is_some() {
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

        // No reserves is a pass, not stalemate. Adjudicate only a side which
        // actually has active pieces, and never recurse between empty sides.
        if !self.has_active_pieces(self.turn) {
            let absent = self.turn;
            if !self.has_active_pieces(absent.opposite()) {
                self.history.status = GameStatus::Waiting {
                    player: absent,
                    reason: WaitingReason::NoActivePieces,
                };
                return Ok(());
            }
            self.turn = absent.opposite();
            self.history.status = GameStatus::Ongoing;
            self.reconcile()?;
            if self.calculation_barrier().is_some() {
                return Ok(());
            }
        }

        // A free wait by the opponent can close the last debt. Re-evaluate the
        // waiting player without consuming the opponent's still-current turn.
        if let GameStatus::Waiting { player, .. } = self.status()
            && player != self.turn
            && self.has_active_pieces(player)
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
        // A purely future-origin threat is a warning, not a visible check
        // that restricts the player's commands or turns stalemate into mate.
        let threatened = !self.checks(player)?.ordinary.is_empty();
        let debt = self.has_past_debt(player.opposite());
        if self.has_active_pieces(player) && !debt {
            self.history.status = GameStatus::Finished(if threatened {
                GameOutcome::Victory {
                    winner: player.opposite(),
                    reason: WinReason::Checkmate,
                }
            } else {
                GameOutcome::Stalemate
            });
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
            self.reconcile()?;
        }
        // No recursive handovers: two blocked sides never busy-loop.
        Ok(())
    }
}
