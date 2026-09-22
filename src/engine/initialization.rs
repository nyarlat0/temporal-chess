use std::collections::BTreeMap;

use crate::pieces::{Color, Sq, initial_position};

use super::{Engine, EngineErr, History, Projection, State};

impl Default for Engine {
    fn default() -> Self {
        let worldlines = initial_position()
            .into_iter()
            .map(|(kind, color, sq)| vec![State::new(kind, color, sq)])
            .collect();
        Self::from_worldlines(worldlines, Color::White)
            .expect("the built-in starting position is valid")
    }
}

impl Engine {
    /// Build a scenario from worldlines, validating and projecting it immediately.
    pub fn from_worldlines(worldlines: Vec<Vec<State>>, turn: Color) -> Result<Self, EngineErr> {
        Self::from_history(
            History {
                worldlines,
                moves: BTreeMap::new(),
            },
            turn,
        )
    }

    /// Restore an owned snapshot without trusting any cached boards or lookups.
    pub fn from_history(history: History, turn: Color) -> Result<Self, EngineErr> {
        for line in &history.worldlines {
            let Some(first) = line.first() else {
                return Err(EngineErr::InvalidHistory("empty worldline"));
            };
            for state in line {
                if !Sq(state.x, state.y).on_board() {
                    return Err(EngineErr::InvalidHistory("square outside the board"));
                }
                if state.fut_seen < state.t {
                    return Err(EngineErr::InvalidHistory("invalid temporal coordinates"));
                }
                if state.color != first.color {
                    return Err(EngineErr::InvalidHistory("a worldline changes color"));
                }
            }
            for pair in line.windows(2) {
                if pair[1].fut_seen < pair[0].fut_seen {
                    return Err(EngineErr::InvalidHistory(
                        "future knowledge decreases along a worldline",
                    ));
                }
                if !pair[0].alive && pair[1].alive {
                    return Err(EngineErr::InvalidHistory(
                        "a dead worldline has a live suffix",
                    ));
                }
                let expected = if pair[1].inverted {
                    pair[0].t.checked_sub(1)
                } else {
                    pair[0].t.checked_add(1)
                };
                if expected != Some(pair[1].t) {
                    return Err(EngineErr::InvalidHistory("non-adjacent worldline states"));
                }
            }
        }
        let mut engine = Self {
            history,
            projection: Projection::default(),
            turn,
        };
        engine.reconcile()?;
        Ok(engine)
    }
}
