use super::{Engine, EngineErr, GameStatus, History, PieceAt};
use crate::model::{
    BoardTime, Color, GameRules, MoveId, PieceId, PieceKind, Sq, StartingPosition, State,
    TimeDirection,
};
use std::collections::{BTreeMap, HashSet};

/// A complete initial causal setting, independent of the later game journal.
/// Projections are rebuilt and validated when restoring; no cached boards travel.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LabSetup {
    version: u16,
    history: History,
    turn: Color,
}

#[derive(Clone, Copy, Debug)]
pub enum LabEdit {
    Boards {
        beginning: BoardTime,
        horizon: BoardTime,
    },
    Place {
        t: BoardTime,
        square: Sq,
        kind: PieceKind,
        color: Color,
        inverted: bool,
    },
    Remove {
        wid: usize,
    },
    RemoveState {
        state: PieceAt,
    },
    RemoveBoard {
        t: BoardTime,
    },
    Prolong {
        wid: usize,
        direction: crate::model::TimeDirection,
        count: usize,
    },
    Turn(Color),
}

impl LabSetup {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("Lab values are serializable")
    }
    pub fn from_json(text: &str) -> Result<Self, String> {
        let setup: Self =
            serde_json::from_str(text).map_err(|e| format!("Invalid Lab setting: {e}"))?;
        if setup.version != 1 || setup.history.starting_position != StartingPosition::Lab {
            return Err("Expected a Lab setting".into());
        }
        Ok(setup)
    }
    pub fn rules(&self) -> GameRules {
        self.history.rules
    }
    pub fn restore(&self) -> Result<Engine, EngineErr> {
        let mut history = self.history.clone();
        history.calculation.interactive = true;
        Engine::restore_history(history, self.turn, false, false)
    }
}

impl Engine {
    fn grow_lab_boards(
        &mut self,
        lower: BoardTime,
        upper: BoardTime,
        extra: Option<(usize, BoardTime)>,
    ) -> Result<(), EngineErr> {
        let mut floors = BTreeMap::new();
        if upper > self.history.horizon {
            for (wid, line) in self.history.worldlines.iter().enumerate() {
                if line
                    .first()
                    .is_some_and(|s| s.inverted && s.t == self.history.horizon)
                {
                    floors.insert(wid, upper);
                }
            }
        }
        if let Some((wid, floor)) = extra {
            floors
                .entry(wid)
                .and_modify(|t| *t = (*t).max(floor))
                .or_insert(floor);
        }
        let mut slots = std::collections::BTreeSet::new();
        for mv in self.history.moves.values() {
            let mut renamed = *mv;
            if let Some(&floor) = floors.get(&mv.pid.wid) {
                renamed.pid.fut_seen = renamed.pid.fut_seen.max(floor);
            }
            if !slots.insert(renamed.id()) {
                return Err(EngineErr::InvalidHistory(
                    "Lab prolongation merges two recorded move slots",
                ));
            }
        }
        self.extend_timeline(lower, upper, None)
    }
    /// Only causal endpoints can grow passively. At a temporal turn both
    /// endpoints may face the same board-time direction; do not invent a turn.
    pub fn lab_prolongation_source(&self, wid: usize, direction: TimeDirection) -> Option<PieceAt> {
        let line = self.history.worldlines.get(wid)?;
        let first = line.first()?;
        let last = line.last()?;
        let mut ends = Vec::new();
        if first.inverted != direction.is_inverted() {
            ends.push(first);
        }
        if last.inverted == direction.is_inverted() {
            ends.push(last);
        }
        let state = match direction {
            TimeDirection::Past => ends.into_iter().min_by_key(|s| s.t),
            TimeDirection::Future => ends.into_iter().max_by_key(|s| s.t),
        }?;
        Some(PieceAt {
            piece: PieceId::from_state(state, wid),
            t: state.t,
        })
    }

    fn prolong_lab_line(
        &mut self,
        wid: usize,
        direction: TimeDirection,
        count: usize,
    ) -> Result<(), EngineErr> {
        if count == 0 {
            return Err(EngineErr::WrongMove);
        }
        let source = self
            .lab_prolongation_source(wid, direction)
            .ok_or(EngineErr::WrongPiece)?;
        let count = i64::try_from(count).map_err(|_| EngineErr::WrongMove)?;
        let target = match direction {
            TimeDirection::Past => source.t.checked_sub(count),
            TimeDirection::Future => source.t.checked_add(count),
        }
        .ok_or(EngineErr::WrongMove)?;
        let first = self.history.worldlines[wid].first().unwrap();
        let prepend = PieceId::from_state(first, wid) == source.piece
            && first.t == source.t
            && first.inverted != direction.is_inverted();
        let extra = (prepend && first.inverted).then_some((wid, target));
        self.grow_lab_boards(
            self.history.beginning.min(target),
            self.history.horizon.max(target),
            extra,
        )?;
        loop {
            let line = &mut self.history.worldlines[wid];
            let end = if prepend {
                &line[0]
            } else {
                line.last().unwrap()
            };
            let done = match direction {
                TimeDirection::Past => end.t <= target,
                TimeDirection::Future => end.t >= target,
            };
            if done {
                break;
            }
            if prepend {
                let t = direction.step(end.t).ok_or(EngineErr::WrongMove)?;
                let state = Self::ancestral_state(end, t);
                let inverted = state.inverted;
                line.states.insert(0, state);
                if inverted {
                    self.raise_ancestral_knowledge(&[wid], t);
                }
            } else {
                let state = end.prolongate().ok_or(EngineErr::WrongMove)?;
                line.states.push(state);
            }
        }
        Ok(())
    }

    /// Removing a state removes its subjective suffix, even across temporal
    /// turns. Compound commands must first restore the other participant.
    fn cut_lab_line(&mut self, wid: usize, age: usize) -> Result<(), EngineErr> {
        let line = self
            .history
            .worldlines
            .get(wid)
            .ok_or(EngineErr::WrongPiece)?;
        if age >= line.len() {
            return Err(EngineErr::WrongPiece);
        }
        let kept: Vec<_> = line[..age].iter().map(MoveId::from_state).collect();
        let affected: Vec<_> = self
            .history
            .moves
            .values()
            .copied()
            .filter(|mv| {
                let source = self.occurrence(mv.source());
                source.is_some_and(|p| p.origin == wid && p.age + 1 >= age)
                    || source
                        .and_then(|king| self.historical_castling_rook(*mv, king))
                        .is_some_and(|rook| rook.origin == wid && rook.age + 1 >= age)
            })
            .collect();
        for mv in &affected {
            if mv.castling().is_some() && self.history.moves.contains_key(&mv.id()) {
                self.undo_mv(mv.id())?;
            }
        }
        for mv in affected {
            self.history.moves.remove(&mv.id());
            self.history.targets.remove(&mv.id());
        }
        let line = &mut self.history.worldlines[wid];
        let keep = kept
            .last()
            .and_then(|id| line.locate(*id))
            .map_or(0, |age| age + 1);
        line.states.truncate(keep);
        line.gaps.clear();
        self.clean_lab_references();
        Ok(())
    }

    /// Clip ancestry at the removed outer boundary, then cut the first route
    /// which leaves the remaining interval. Internal holes are never introduced.
    fn trim_lab_boards(
        &mut self,
        beginning: BoardTime,
        horizon: BoardTime,
    ) -> Result<(), EngineErr> {
        if beginning > 0 || horizon < 0 {
            return Err(EngineErr::InvalidHistory(
                "the zero board cannot be deleted",
            ));
        }
        let ranges = |engine: &Engine| {
            engine
                .history
                .worldlines
                .iter()
                .map(|line| {
                    let start = line
                        .iter()
                        .position(|s| s.t >= beginning && s.t <= horizon)
                        .unwrap_or(line.len());
                    let end = start
                        + line[start..]
                            .iter()
                            .take_while(|s| s.t >= beginning && s.t <= horizon)
                            .count();
                    start..end
                })
                .collect::<Vec<_>>()
        };
        loop {
            let kept = ranges(self);
            let invalid = self.history.moves.values().copied().find(|mv| {
                if mv.castling().is_none() {
                    return false;
                }
                let Some(king) = self.occurrence(mv.source()) else {
                    return false;
                };
                let rook = self.historical_castling_rook(*mv, king);
                let source_kept = kept[king.origin].contains(&king.age)
                    || rook.is_some_and(|p| kept[p.origin].contains(&p.age));
                source_kept
                    && (!kept[king.origin].contains(&(king.age + 1))
                        || rook.is_none_or(|p| !kept[p.origin].contains(&(p.age + 1))))
            });
            let Some(mv) = invalid else {
                break;
            };
            self.undo_mv(mv.id())?;
        }
        let kept = ranges(self);
        for (line, kept) in self.history.worldlines.iter_mut().zip(kept) {
            line.states = line.states[kept].to_vec();
            line.gaps.clear();
        }
        self.history.beginning = beginning;
        self.history.horizon = horizon;
        self.clean_lab_references();
        Ok(())
    }

    /// Editor cuts rebase the setting. Keep subjective IDs and current
    /// future knowledge; only vector indices and references to erased states change.
    fn clean_lab_references(&mut self) {
        let mut remap = Vec::with_capacity(self.history.worldlines.len());
        let mut next = 0;
        for line in &self.history.worldlines {
            remap.push(if line.is_empty() {
                None
            } else {
                let id = next;
                next += 1;
                Some(id)
            });
        }
        self.history.worldlines.retain(|l| !l.is_empty());
        let at = |mut p: PieceAt| {
            p.piece.wid = *remap.get(p.piece.wid)?.as_ref()?;
            Some(p)
        };
        let existing: HashSet<_> = self
            .history
            .worldlines
            .iter()
            .enumerate()
            .flat_map(|(wid, line)| {
                line.iter().map(move |s| PieceAt {
                    piece: PieceId::from_state(s, wid),
                    t: s.t,
                })
            })
            .collect();
        self.history.moves.retain(|_, mv| {
            let Some(source) = at(mv.source()).filter(|p| existing.contains(p)) else {
                return false;
            };
            mv.pid = source.piece;
            let line = &self.history.worldlines[mv.pid.wid];
            line.locate(mv.id())
                .and_then(|age| line.get(age + 1))
                .is_some_and(|s| Some(s.t) == mv.direction.step(mv.t))
        });
        self.history
            .targets
            .retain(|id, _| self.history.moves.contains_key(id));
        for targets in self.history.targets.values_mut() {
            *targets = targets
                .iter()
                .copied()
                .filter_map(at)
                .filter(|p| existing.contains(p))
                .collect();
        }
        self.history.calculation.cancellations.clear();
        self.history.calculation.acknowledged.clear();
    }

    pub fn new_lab(rules: GameRules) -> Result<Self, EngineErr> {
        Self::from_history(
            History {
                rules,
                starting_position: StartingPosition::Lab,
                worldlines: Vec::new(),
                moves: BTreeMap::new(),
                targets: BTreeMap::new(),
                beginning: 0,
                horizon: 0,
                status: GameStatus::Ongoing,
                turn_streak: None,
                calculation: Default::default(),
            },
            Color::White,
        )
    }

    pub fn lab_setup(&self) -> LabSetup {
        let mut history = self.history.clone();
        history.starting_position = StartingPosition::Lab;
        LabSetup {
            version: 1,
            history,
            turn: self.turn,
        }
    }

    /// Start a game from the editor with a fresh count of consecutive turns.
    pub fn reset_lab_turn_streak(&mut self) {
        if self.history.starting_position == StartingPosition::Lab {
            self.history.turn_streak = None;
            if matches!(
                self.outcome(),
                Some(super::GameOutcome::Victory {
                    reason: super::WinReason::TurnHogging,
                    ..
                })
            ) {
                self.history.status = GameStatus::Ongoing;
            }
        }
    }

    /// Editor mutations are atomic. Trial spatial moves still use the ordinary
    /// public move API and all of its legality/collision/history checks.
    pub fn edit_lab(&mut self, edit: LabEdit) -> Result<(), EngineErr> {
        if self.history.starting_position != StartingPosition::Lab {
            return Err(EngineErr::InvalidHistory(
                "Lab editing requires a Lab setting",
            ));
        }
        let mut next = self.clone();
        next.history.status = GameStatus::Ongoing;
        next.history.turn_streak = None;
        match edit {
            LabEdit::Boards { beginning, horizon } => {
                if beginning > next.history.beginning
                    || horizon < next.history.horizon
                    || horizon
                        .checked_sub(beginning)
                        .and_then(|v| v.checked_add(1))
                        .is_none()
                {
                    return Err(EngineErr::InvalidHistory("invalid Lab board range"));
                }
                next.grow_lab_boards(beginning, horizon, None)?;
            }
            LabEdit::Place {
                t,
                square,
                kind,
                color,
                inverted,
            } => {
                if !square.on_board() || t < next.history.beginning || t > next.history.horizon {
                    return Err(EngineErr::AbsentBoard);
                }
                if next
                    .preboard(t)
                    .is_some_and(|b| !b.space[square.0][square.1].is_empty())
                {
                    return Err(EngineErr::InvalidHistory("Lab square is occupied"));
                }
                let home = if color == Color::White { 0 } else { 7 };
                let unmoved = match kind {
                    PieceKind::K { .. } => square == Sq(4, home),
                    PieceKind::R { .. } => square.1 == home && [0, 7].contains(&square.0),
                    PieceKind::P { .. } => square.1 == if color == Color::White { 1 } else { 6 },
                    _ => false,
                };
                let mut state = State::new(kind.set_moved(!unmoved), color, square);
                state.t = t;
                state.fut_seen = t;
                state.inverted = inverted;
                state.rights.ordinary = Color::from_board(t) == color;
                let mut line = vec![state];
                // An appearance on the other side's parity needs its passive
                // tick to an actionable board, otherwise it would be stranded.
                if Color::from_board(t) != color {
                    let own = line[0].prolongate().ok_or(EngineErr::WrongMove)?;
                    next.grow_lab_boards(
                        next.history.beginning.min(own.t),
                        next.history.horizon.max(own.t),
                        None,
                    )?;
                    line.push(own);
                }
                next.history.worldlines.push(line.into());
            }
            LabEdit::Remove { wid } => next.cut_lab_line(wid, 0)?,
            LabEdit::RemoveState { state } => {
                let piece = next.occurrence(state).ok_or(EngineErr::WrongPiece)?;
                next.cut_lab_line(piece.origin, piece.age)?;
            }
            LabEdit::RemoveBoard { t } => {
                if t == 0 || t < next.history.beginning || t > next.history.horizon {
                    return Err(EngineErr::InvalidHistory(
                        "the zero board cannot be deleted",
                    ));
                }
                let beginning = if t < 0 {
                    t.checked_add(1).ok_or(EngineErr::WrongMove)?
                } else {
                    next.history.beginning
                };
                let horizon = if t > 0 {
                    t.checked_sub(1).ok_or(EngineErr::WrongMove)?
                } else {
                    next.history.horizon
                };
                next.trim_lab_boards(beginning, horizon)?;
            }
            LabEdit::Prolong {
                wid,
                direction,
                count,
            } => next.prolong_lab_line(wid, direction, count)?,
            LabEdit::Turn(side) => next.turn = side,
        }
        next.update_bufs()?;
        next.reconcile_with_auto_wait(false)?;
        *self = next;
        Ok(())
    }
}

/// JSON objects cannot have structured keys. Encode ordered maps as entry lists
/// and reject duplicates instead of silently overwriting causal commands.
pub(super) mod map_entries {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;
    pub fn serialize<K: Serialize, V: Serialize, S: Serializer>(
        map: &BTreeMap<K, V>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        map.iter().collect::<Vec<_>>().serialize(s)
    }
    pub fn deserialize<
        'de,
        K: Deserialize<'de> + Ord,
        V: Deserialize<'de>,
        D: Deserializer<'de>,
    >(
        d: D,
    ) -> Result<BTreeMap<K, V>, D::Error> {
        let entries = Vec::<(K, V)>::deserialize(d)?;
        let len = entries.len();
        let map: BTreeMap<_, _> = entries.into_iter().collect();
        if map.len() != len {
            return Err(serde::de::Error::custom("duplicate history key"));
        }
        Ok(map)
    }
}
