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
    /// Choose immutable rules for a fresh standard game. Default is destructive
    /// temporal turns; opt in to borrowing a preserved future across causal gaps.
    pub fn new(causal_gaps: bool) -> Self {
        let mut engine = Self::default();
        engine.history.causal_gaps = causal_gaps;
        engine
    }

    /// Build a scenario from worldlines, validating and projecting it immediately.
    pub fn from_worldlines(worldlines: Vec<Vec<State>>, turn: Color) -> Result<Self, EngineErr> {
        let horizon = worldlines
            .iter()
            .flatten()
            .map(|state| state.t)
            .max()
            .unwrap_or(0);
        Self::from_history(
            History {
                causal_gaps: false,
                worldlines: worldlines.into_iter().map(Into::into).collect(),
                moves: BTreeMap::new(),
                targets: BTreeMap::new(),
                horizon,
                status: super::GameStatus::Ongoing,
                calculation: Default::default(),
            },
            turn,
        )
    }

    /// Restore an owned snapshot without trusting any cached boards or lookups.
    pub fn from_history(history: History, turn: Color) -> Result<Self, EngineErr> {
        for line in &history.worldlines {
            if !history.causal_gaps && !line.gaps.is_empty() {
                return Err(EngineErr::InvalidHistory("causal gaps disabled"));
            }
            let Some(first) = line.first() else {
                return Err(EngineErr::InvalidHistory("empty worldline"));
            };
            for state in line.iter() {
                if !Sq(state.x, state.y).on_board() {
                    return Err(EngineErr::InvalidHistory("square outside the board"));
                }
                if state.fut_seen < state.t || state.t > history.horizon {
                    return Err(EngineErr::InvalidHistory("invalid temporal coordinates"));
                }
                if state.color != first.color {
                    return Err(EngineErr::InvalidHistory("a worldline changes color"));
                }
            }
            for (age, pair) in line.windows(2).enumerate() {
                if let Some(gap) = line.gap_after(age) {
                    if line.gaps[gap].resume != Some(super::MoveId::from_state(&pair[1])) {
                        return Err(EngineErr::InvalidHistory(
                            "causal gap does not meet its preserved future",
                        ));
                    }
                    continue;
                }
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
                let step = pair[0]
                    .step(super::TimeDirection::from_inverted(pair[1].inverted))
                    .ok_or(EngineErr::InvalidHistory(
                        "temporal counter overflow or underflow",
                    ))?;
                if step.loop_turn != pair[1].loop_turn
                    || step.loop_pivots != pair[1].loop_pivots
                    || step.fut_seen != pair[1].fut_seen
                {
                    return Err(EngineErr::InvalidHistory(
                        "inconsistent temporal transition",
                    ));
                }
            }
            for gap in &line.gaps {
                let Some(entry) = line.locate(gap.entry) else {
                    return Err(EngineErr::InvalidHistory("missing loop entry"));
                };
                let Some(after) = line.locate(gap.after) else {
                    return Err(EngineErr::InvalidHistory("missing loop endpoint"));
                };
                if entry > after
                    || gap
                        .resume
                        .is_some_and(|key| line.locate(key) != Some(after + 1))
                    || (gap.resume.is_none() && after + 1 != line.len())
                {
                    return Err(EngineErr::InvalidHistory("invalid causal gap"));
                }
            }
        }
        let mut engine = Self {
            history,
            projection: Projection::default(),
            turn,
        };
        engine.reconcile()?;
        if engine.status() == super::GameStatus::Ongoing {
            engine.finish_command(turn.opposite())?;
        }
        Ok(engine)
    }
}
