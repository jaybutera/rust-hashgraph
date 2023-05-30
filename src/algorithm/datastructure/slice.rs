//! Slice of the graph and iteration over it. Details are in [`SliceIterator`].

use std::collections::{HashMap, HashSet};

use crate::algorithm::event::{self, EventWrapper};

use super::UnknownEvent;

/// Iterator over graph slice. It goes higher in ancestry (from the event to its
/// ancestors). Visits only events made by peers supplied in `starting_slice`. Does not
/// guarantee homogeneous (?) pass, i.e. might first go through only one peer and then
/// visit events of others.
///
/// Finishes when parents of all sliced events do not satisfy `continue_iterate_peer`.
///
/// `all_events` used for lookup of events since parents are stored in hashes (in the
/// events).
pub struct SliceIterator<'a, TPayload, TGenesisPayload, TPeerId, FContinue> {
    current_slice: HashSet<&'a EventWrapper<TPayload, TGenesisPayload, TPeerId>>,
    continue_iterate_peer: FContinue,
    all_events: &'a HashMap<event::Hash, EventWrapper<TPayload, TGenesisPayload, TPeerId>>,
}

impl<'a, TPayload, TGenesisPayload, TPeerId, FContinue>
    SliceIterator<'a, TPayload, TGenesisPayload, TPeerId, FContinue>
where
    TPayload: Eq + std::hash::Hash,
    TGenesisPayload: Eq + std::hash::Hash,
    TPeerId: Eq + std::hash::Hash,
{
    /// Create iterator over graph slice.
    ///
    /// - `starting_slice`: initial slice, the iterator will go only to their same-peer
    /// ancestors (created by the same peer).
    /// - `continue_condition`: predicate. When returns `false`, the event and its parents
    /// are not considered.
    /// - `all_events`: event lookup.
    pub fn new(
        starting_slice: &HashSet<&event::Hash>,
        continue_condition: FContinue,
        all_events: &'a HashMap<event::Hash, EventWrapper<TPayload, TGenesisPayload, TPeerId>>,
    ) -> Result<Self, UnknownEvent>
    where
        TPayload: Eq + std::hash::Hash,
        TGenesisPayload: Eq + std::hash::Hash,
    {
        let current_slice: Result<Vec<_>, _> = starting_slice
            .iter()
            .map(|hash| all_events.get(hash).ok_or(UnknownEvent((*hash).clone())))
            .collect();
        let current_slice = current_slice?;
        let current_slice = HashSet::<_>::from_iter(current_slice);
        Ok(Self {
            current_slice,
            continue_iterate_peer: continue_condition,
            all_events,
        })
    }

    fn add_parents(
        &mut self,
        event: &EventWrapper<TPayload, TGenesisPayload, TPeerId>,
    ) -> Result<(), UnknownEvent> {
        if let event::Kind::Regular(parents) = event.kind() {
            let self_parent = self
                .all_events
                .get(&parents.self_parent)
                .ok_or(UnknownEvent(parents.self_parent.clone()))?;
            self.current_slice.insert(self_parent);

            // We add only parents made by the same peer not to visit events multiple times
            let this_author = event.author();
            let other_parent = self
                .all_events
                .get(&parents.other_parent)
                .ok_or(UnknownEvent(parents.other_parent.clone()))?;
            if other_parent.author() == this_author {
                self.current_slice.insert(other_parent);
            }
        }
        Ok(())
    }
}

impl<'a, TPayload, TGenesisPayload, TPeerId, FContinue> Iterator
    for SliceIterator<'a, TPayload, TGenesisPayload, TPeerId, FContinue>
where
    FContinue: Fn(&EventWrapper<TPayload, TGenesisPayload, TPeerId>) -> bool,
    TPayload: Eq + std::hash::Hash,
    TGenesisPayload: Eq + std::hash::Hash,
    TPeerId: Eq + std::hash::Hash,
{
    type Item = &'a EventWrapper<TPayload, TGenesisPayload, TPeerId>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(next_event) = self.current_slice.iter().next().cloned() {
            self.current_slice.remove(next_event);
            if !(self.continue_iterate_peer)(next_event) {
                continue;
            } else {
                self.add_parents(next_event)
                    .expect("parents must be tracked");
                return Some(next_event);
            }
        }
        return None;
    }
}
