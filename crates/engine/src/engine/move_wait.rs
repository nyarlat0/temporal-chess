//! A spatial move and its optional wait are one atomic turn.
use super::{Engine, EngineErr, Move, PieceAt, PieceId};
use crate::model::{Color, MoveId, MoveRights};

impl Engine {
    fn moved_endpoint(&self, mv: Move) -> Option<PieceAt> {
        let line = self.history.worldlines.get(mv.pid.wid)?;
        let age = line
            .iter()
            .position(|s| s.t == mv.t && PieceId::from_state(s, mv.pid.wid) == mv.pid)?;
        let arrival = line.get(age + 1)?;
        if !arrival.alive
            || crate::model::Sq(arrival.x, arrival.y) != mv.sq
            || mv.direction.step(mv.t) != Some(arrival.t)
        {
            return None;
        }
        let endpoint = line.get(age + 2)?;
        (endpoint.alive
            && endpoint.rights.ordinary
            && Color::from_board(endpoint.t) == endpoint.color
            && crate::model::Sq(endpoint.x, endpoint.y) == mv.sq)
            .then_some(PieceAt {
                piece: PieceId::from_state(endpoint, mv.pid.wid),
                t: endpoint.t,
            })
    }

    /// Existing own-color boards reachable after resolving the spatial capture.
    /// The captured incarnation is already dead when checking waiting blockers.
    pub fn possible_move_waits(&self, mv: Move) -> Result<Vec<usize>, EngineErr> {
        let mut probe = self.clone();
        probe.history.calculation.acknowledged.clear();
        probe.history.calculation.responsible = Some(self.turn);
        probe.apply_move_inner(mv)?;
        probe.turn = probe.turn.opposite();
        probe.reconcile_with_auto_wait(false)?;
        probe.turn = self.turn;
        let Some(source) = probe.moved_endpoint(mv) else {
            return Ok(Vec::new());
        };
        let mut result = vec![source.t];
        for t in probe.timeline_times() {
            if t == source.t {
                continue;
            }
            let mut waiting = probe.clone();
            if waiting.skip_inner(source, t).is_ok() {
                result.push(t);
            }
        }
        result.sort_unstable();
        Ok(result)
    }

    pub fn after_move_with_wait(&self, mv: Move, until: usize) -> Result<Self, EngineErr> {
        let source = self.occurrence(mv.source()).ok_or(EngineErr::WrongPiece)?;
        if !self
            .possible_moves(source.origin, source.age)?
            .contains(&mv)
        {
            return Err(EngineErr::WrongMove);
        }
        self.fork_transaction(|next| {
            next.apply_move_inner(mv)?;
            // Resolve captures before occupancy checks, without advancing a
            // temporarily immobile arrival past the player's chosen endpoint.
            let actor = next.turn;
            next.turn = actor.opposite();
            next.reconcile_with_auto_wait(false)?;
            next.turn = actor;
            let source = next.moved_endpoint(mv).ok_or(EngineErr::WrongMove)?;
            if source.t != until {
                next.skip_with_information_mode(source, until, false, actor.opposite())?;
            }
            next.turn = actor.opposite();
            next.reconcile()
        })
    }

    /// Advance one blocked terminal occurrence, then re-evaluate the coherent
    /// history before considering its next two ticks (or final forward tick).
    /// Collisions are intentional; the history horizon never grows.
    pub(super) fn auto_wait_blocked_endpoint(
        &mut self,
        waited: &mut Vec<PieceAt>,
    ) -> Result<bool, EngineErr> {
        if self.history.rules.wait_before_move {
            return Ok(false);
        }
        for wid in 0..self.history.worldlines.len() {
            let line = &self.history.worldlines[wid];
            let age = line.len() - 1;
            let state = &line[age];
            if line.is_borrowed(age)
                || !state.alive
                || !state.rights.is_movable()
                || Color::from_board(state.t) != state.color
                || self.board(state.t).is_none()
                || self
                    .history
                    .moves
                    .get(&MoveId::from_state(state))
                    .is_some_and(|mv| mv.pid.wid == wid)
            {
                continue;
            }
            // An occupied causal slot cannot provide an independent move for
            // this endpoint, even if its spatial geometry offers destinations.
            if !self.history.moves.contains_key(&MoveId::from_state(state))
                && !self.candidate_moves(wid, age)?.is_empty()
            {
                continue;
            }
            // Unlike a player-selected wait, automatic continuation may cause
            // collisions. Let the normal reconciliation loop resolve them.
            let Some(first) = state.prolongate().filter(|s| s.t <= self.history.horizon) else {
                continue;
            };
            let last = first.prolongate().filter(|s| s.t <= self.history.horizon);
            // Only forward continuation may finish on the opponent's parity.
            // Inverted passive lines still stop on their first own-color board.
            if state.inverted && last.is_none() {
                continue;
            }
            let source = &mut self.history.worldlines[wid][age];
            waited.push(PieceAt {
                piece: PieceId::from_state(source, wid),
                t: source.t,
            });
            source.remembered.extend(source.informed.iter().copied());
            source.rights = MoveRights::default();
            for next in std::iter::once(first).chain(last) {
                waited.push(PieceAt {
                    piece: PieceId::from_state(&next, wid),
                    t: next.t,
                });
                self.history.worldlines[wid].states.push(next);
            }
            self.update_bufs()?;
            return Ok(true);
        }
        Ok(false)
    }

    pub(super) fn acknowledge_auto_wait(&mut self, waited: &[PieceAt]) -> Result<bool, EngineErr> {
        if waited.is_empty() {
            return Ok(false);
        }
        let mut learned = false;
        for id in waited {
            if let Some(loc) = self.projection.pieces.get(id) {
                let state = &mut self.history.worldlines[loc.wid][loc.age];
                let before = state.remembered.len();
                state.remembered.extend(state.informed.iter().copied());
                learned |= state.remembered.len() != before;
            }
        }
        self.update_infos()?;
        self.update_bufs()?;
        // Learning can release a calculation barrier. Re-run validation before
        // publishing any newly exposed boards, even without a collision.
        Ok(self.build_boards(self.history.horizon)? == super::ValidRes::HistChanged || learned)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{GameStatus, History};
    use super::*;
    use crate::{
        model::{GameRules, PieceKind, Sq, StartingPosition, State},
        record::{Command, GameRecord},
    };
    use std::collections::BTreeMap;

    fn scenario(lines: Vec<Vec<State>>, wait_before_move: bool, turn: Color) -> Engine {
        let horizon = lines.iter().flatten().map(|s| s.t).max().unwrap_or(0);
        Engine::from_history(
            History {
                rules: GameRules {
                    wait_before_move,
                    ..Default::default()
                },
                starting_position: StartingPosition::Standard,
                worldlines: lines.into_iter().map(Into::into).collect(),
                moves: BTreeMap::new(),
                targets: BTreeMap::new(),
                horizon,
                status: GameStatus::Ongoing,
                calculation: Default::default(),
            },
            turn,
        )
        .unwrap()
    }
    fn line(kind: PieceKind, color: Color, sq: Sq, from: usize, until: usize) -> Vec<State> {
        (from..=until)
            .map(|t| {
                let mut state = State::new(kind, color, sq);
                state.t = t;
                state.fut_seen = t;
                state.rights.ordinary = Color::from_board(t) == color;
                state
            })
            .collect()
    }

    #[test]
    fn captured_piece_does_not_block_post_move_wait_and_turn_changes_once() {
        let engine = scenario(
            vec![
                line(PieceKind::R { moved: false }, Color::White, Sq(0, 0), 0, 0),
                line(PieceKind::N, Color::Black, Sq(0, 2), 1, 6),
            ],
            false,
            Color::White,
        );
        let mv = engine
            .possible_moves(0, 0)
            .unwrap()
            .into_iter()
            .find(|mv| mv.sq == Sq(0, 2) && !mv.to_past())
            .unwrap();
        assert!(engine.possible_move_waits(mv).unwrap().contains(&6));
        let next = engine.after_move_with_wait(mv, 6).unwrap();
        assert_eq!(next.turn(), Color::Black);
        assert_eq!(next.board(6).unwrap().get_piece(0, 2).unwrap().origin, 0);
        assert!(next.history.worldlines[1].iter().all(|s| !s.alive));
        assert!(next.history.worldlines[0].last().unwrap().rights.ordinary);
        assert!(
            next.history.worldlines[0]
                .iter()
                .all(|s| !s.rights.new_information)
        );
    }

    #[test]
    fn wait_stops_before_first_occupied_tick_even_between_own_boards() {
        let mut lines = vec![
            line(PieceKind::R { moved: false }, Color::White, Sq(0, 0), 0, 0),
            line(PieceKind::N, Color::Black, Sq(7, 7), 0, 6),
        ];
        lines.push(line(PieceKind::N, Color::Black, Sq(1, 0), 3, 3));
        let engine = scenario(lines, false, Color::White);
        let mv = engine
            .possible_moves(0, 0)
            .unwrap()
            .into_iter()
            .find(|mv| mv.sq == Sq(1, 0) && !mv.to_past())
            .unwrap();
        assert_eq!(engine.possible_move_waits(mv).unwrap(), vec![2]);
        assert!(engine.after_move_with_wait(mv, 4).is_err());
        assert!(engine.possible_skips(mv.source()).is_empty());
        let mut copy = engine.clone();
        assert!(copy.skip_ordinary(mv.source(), 2).is_err());
        assert_eq!(copy, engine);
    }

    #[test]
    fn auto_wait_resolves_collisions_and_reaches_the_absolute_future_boundary() {
        for wait_before in [false, true] {
            for end in [5, 6] {
                let engine = scenario(
                    vec![
                        line(PieceKind::P { moved: true }, Color::White, Sq(0, 7), 0, 0),
                        line(PieceKind::N, Color::Black, Sq(7, 7), 0, end),
                    ],
                    wait_before,
                    Color::White,
                );
                let last = engine.history.worldlines[0].last().unwrap();
                assert_eq!(last.t, if wait_before { 0 } else { end });
                assert_eq!(last.rights.ordinary, wait_before || end % 2 == 0);
                assert_eq!(engine.history.horizon, end);
            }
        }
        let setup = scenario(
            vec![
                line(PieceKind::P { moved: true }, Color::White, Sq(0, 7), 0, 0),
                line(PieceKind::N, Color::Black, Sq(7, 7), 0, 5),
            ],
            true,
            Color::White,
        );
        let mut history = setup.history;
        history.rules.wait_before_move = false;
        history.starting_position = StartingPosition::TimeWar { separation: 5 };
        let fixed = Engine::from_history(history, Color::White).unwrap();
        assert_eq!(fixed.history.worldlines[0].last().unwrap().t, 5);
        assert!(!fixed.history.worldlines[0].last().unwrap().inverted);
        assert_eq!(fixed.history.horizon, 5);

        // Meeting an inverted enemy on an intermediate tick is resolved as
        // an ordinary engine collision, rather than stopping before the square.
        let mut enemy = line(PieceKind::N, Color::Black, Sq(0, 7), 3, 3);
        enemy[0].inverted = true;
        enemy[0].fut_seen = 6;
        let engine = scenario(
            vec![
                line(PieceKind::P { moved: true }, Color::White, Sq(0, 7), 0, 0),
                enemy,
                line(PieceKind::N, Color::Black, Sq(7, 7), 0, 6),
            ],
            false,
            Color::White,
        );
        assert!(engine.history.worldlines[0].iter().any(|s| s.t == 3));
        assert!(
            engine
                .history
                .worldlines
                .iter()
                .take(2)
                .flat_map(|l| l.iter())
                .any(|s| !s.alive)
        );
        assert!(engine.make_board(3).is_ok());
        let mut stuck = State::new(PieceKind::P { moved: true }, Color::Black, Sq(0, 0));
        stuck.t = 1;
        stuck.fut_seen = 1;
        stuck.inverted = true;
        stuck.rights.ordinary = true;
        let engine = scenario(vec![vec![stuck]], false, Color::Black);
        assert_eq!(engine.history.worldlines[0].len(), 1);
        assert!(engine.history.worldlines[0][0].rights.ordinary);
    }

    #[test]
    fn auto_wait_stops_as_soon_as_a_move_exists_and_ignores_nonterminal_reactions() {
        for cancelled in [false, true] {
            let mut pawn = line(PieceKind::P { moved: true }, Color::White, Sq(0, 1), 0, 0);
            pawn[0].rights.ordinary = !cancelled;
            pawn[0].rights.cancelled_move = cancelled;
            let engine = scenario(
                vec![
                    pawn.clone(),
                    line(PieceKind::P { moved: true }, Color::White, Sq(0, 2), 1, 1),
                    line(PieceKind::N, Color::Black, Sq(7, 7), 0, 6),
                ],
                false,
                Color::White,
            );
            let endpoint = engine.history.worldlines[0].last().unwrap();
            assert_eq!(endpoint.t, 2);
            assert!(endpoint.rights.ordinary);
            assert!(!engine.possible_moves(0, 2).unwrap().is_empty());
            pawn.push(pawn[0].prolongate().unwrap());
            let engine = scenario(
                vec![
                    pawn,
                    line(PieceKind::P { moved: true }, Color::White, Sq(0, 2), 1, 1),
                    line(PieceKind::N, Color::Black, Sq(7, 7), 0, 6),
                ],
                false,
                Color::White,
            );
            assert_eq!(engine.history.worldlines[0].len(), 2);
        }
    }

    #[test]
    fn occupied_slot_auto_waits_a_mobile_endpoint_without_replacing_the_move() {
        for wait_before_move in [false, true] {
            let mut engine = Engine::new(GameRules {
                wait_before_move,
                ..Default::default()
            });
            let mv = engine
                .possible_moves(12, 0)
                .unwrap()
                .into_iter()
                .find(|mv| mv.sq == Sq(4, 3) && !mv.to_past())
                .unwrap();
            engine.apply_new_mv(mv).unwrap();
            // A terminal reaction left behind on a board whose causal slot
            // belongs to another piece, with perfectly usable knight geometry.
            engine.history.worldlines[6].states.truncate(1);
            engine.history.worldlines[6][0].rights.ordinary = true;
            engine.update_bufs().unwrap();
            engine.build_boards(engine.history.horizon).unwrap();
            assert!(!engine.candidate_moves(6, 0).unwrap().is_empty());
            let moves = engine.history.moves.clone();
            let mut waited = Vec::new();
            assert_eq!(
                engine.auto_wait_blocked_endpoint(&mut waited).unwrap(),
                !wait_before_move
            );
            let last = engine.history.worldlines[6].last().unwrap();
            assert_eq!(last.t, usize::from(!wait_before_move));
            assert_eq!((last.x, last.y), (6, 0));
            assert_eq!(engine.history.moves, moves);
            assert_eq!(engine.turn(), Color::Black);
            engine.reconcile().unwrap();
            assert_eq!(engine.history.moves, moves);
        }
    }

    #[test]
    fn move_and_wait_journal_roundtrip_and_undo_in_both_modes() {
        for wait_before_move in [false, true] {
            let rules = GameRules {
                wait_before_move,
                ..Default::default()
            };
            let start = StartingPosition::TimeWar { separation: 9 };
            let mut engine = Engine::with_start(rules, start)
                .unwrap()
                .with_interactive_calculation()
                .unwrap();
            let before = engine.clone();
            let mut record = GameRecord::with_start(rules, start);
            let mut found = None;
            for p in engine.move_sources() {
                for mv in engine.possible_moves(p.origin, p.age).unwrap() {
                    if let Ok(times) = engine.possible_move_waits(mv)
                        && let Some(&until) = times.iter().find(|&&t| t >= 4)
                    {
                        found = Some(Command::MoveAndWait {
                            mv,
                            from: p.sq,
                            until,
                        });
                        break;
                    }
                }
                if found.is_some() {
                    break;
                }
            }
            let command = found.expect("an opening move can wait");
            record.apply(&mut engine, command).unwrap();
            assert_eq!(record.commands(), &[command]);
            assert_eq!(command.to_string().parse::<Command>().unwrap(), command);
            assert_eq!(GameRecord::load(&record.to_text()).unwrap().0, engine);
            let (prefix, _) = record.without_last_move().unwrap();
            assert_eq!(GameRecord::load(&prefix.to_text()).unwrap().0, before);
        }
    }
}
