use std::ops::{Deref, DerefMut};

use super::{MoveId, State};

/// A causal discontinuity while an unfinished journey owes its existing future.
/// Keys, unlike ages, survive insertion of states into this or an outer loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CausalGap {
    pub entry: MoveId,
    pub after: MoveId,
    pub resume: Option<MoveId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worldline {
    pub(crate) states: Vec<State>,
    pub(crate) gaps: Vec<CausalGap>,
}

impl From<Vec<State>> for Worldline {
    fn from(states: Vec<State>) -> Self {
        Self {
            states,
            gaps: Vec::new(),
        }
    }
}

impl Deref for Worldline {
    type Target = [State];
    fn deref(&self) -> &Self::Target {
        &self.states
    }
}

impl DerefMut for Worldline {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.states
    }
}

impl Worldline {
    pub fn gaps(&self) -> &[CausalGap] {
        &self.gaps
    }

    pub(crate) fn locate(&self, key: MoveId) -> Option<usize> {
        self.states
            .iter()
            .position(|state| MoveId::from_state(state) == key)
    }

    pub(crate) fn gap_after(&self, age: usize) -> Option<usize> {
        let key = MoveId::from_state(self.get(age)?);
        self.gaps.iter().position(|gap| gap.after == key)
    }

    /// Mutations cannot propagate through a debt into the preserved future.
    pub(crate) fn segment_end(&self, age: usize) -> usize {
        (age..self.len())
            .find(|&i| self.gap_after(i).is_some())
            .map_or(self.len(), |i| i + 1)
    }

    pub(crate) fn is_borrowed(&self, age: usize) -> bool {
        self.gaps.iter().any(|gap| {
            gap.resume
                .and_then(|key| self.locate(key))
                .is_some_and(|i| i <= age)
        })
    }
}
