use super::{Engine, EngineErr, GameStatus, History, PieceAt};
use crate::model::{BoardTime, Color, GameRules, PieceKind, Sq, StartingPosition, State};
use std::collections::BTreeMap;

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
                next.history.beginning = beginning;
                next.history.horizon = horizon;
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
                    next.history.beginning = next.history.beginning.min(own.t);
                    next.history.horizon = next.history.horizon.max(own.t);
                    line.push(own);
                }
                next.history.worldlines.push(line.into());
            }
            LabEdit::Remove { wid } => {
                if wid >= next.history.worldlines.len() {
                    return Err(EngineErr::WrongPiece);
                }
                next.history.worldlines.remove(wid);
                let remap = |mut at: PieceAt| {
                    if at.piece.wid == wid {
                        return None;
                    }
                    if at.piece.wid > wid {
                        at.piece.wid -= 1;
                    }
                    Some(at)
                };
                next.history.moves.retain(|_, mv| {
                    let Some(source) = remap(mv.source()) else {
                        return false;
                    };
                    mv.pid = source.piece;
                    true
                });
                for targets in next.history.targets.values_mut() {
                    *targets = targets.iter().copied().filter_map(remap).collect();
                }
                next.history
                    .targets
                    .retain(|id, _| next.history.moves.contains_key(id));
                // Editor deletion rebases the setting. Old cancellation
                // snapshots/acknowledgements carry obsolete worldline indices.
                next.history.calculation.cancellations.clear();
                next.history.calculation.acknowledged.clear();
            }
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
