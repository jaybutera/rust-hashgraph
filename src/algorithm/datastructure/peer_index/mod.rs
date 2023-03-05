use std::{collections::HashMap, num::NonZeroU8};

use derive_getters::Getters;
use thiserror::Error;

use crate::PeerId;

use self::fork_tracking::{ForkIndex, ForkIndexIter, LeafPush, LookupIndexInconsistency};
use super::event;

pub mod fork_tracking;

pub type EventIndex<TIndexPayload> = HashMap<event::Hash, TIndexPayload>;

pub type PeerIndex = HashMap<PeerId, PeerIndexEntry>;

#[derive(Getters)]
pub struct PeerIndexEntry {
    origin: event::Hash,
    /// Use `add_latest` for insertion.
    ///
    /// Value is height of the event from genesis if considering self parent/self
    /// child relationship. Genesis (should) have height 0.
    ///
    /// In other words, height is a distance from genesis through `self_parent`s
    authored_events: EventIndex<usize>,
    /// Forks authored by the peer that we've observed. Forks are events
    /// that have the same `self_parent`.
    ///
    /// Represented by event hashes that have multiple self children
    /// (children authored by the same peer)
    fork_index: ForkIndex,
}

#[derive(Debug, Error, PartialEq)]
pub enum Error {
    #[error("Provided parent hash cannot be looked up with given method")]
    UnknownParent,
    #[error(
        "Provided parent does not have self children, however it is not \
        tracked by fork index. Likely due to inconsistent state of the index with event contents."
    )]
    InvalidParent,
    #[error("Attempted to add an event that is already present in the index")]
    EventAlreadyKnown,
    #[error("Events found via the lookup contain reference unknown events")]
    InconsistentLookup,
    #[error("State of the index and lookup state are inconsistent")]
    LookupIndexInconsistency(#[from] LookupIndexInconsistency),
    #[error("Pushing leaf (non-forking) event failed")]
    LeafPush(#[from] LeafPush),
}

impl PeerIndexEntry {
    pub fn new(genesis: event::Hash, fork_index_submultiple: NonZeroU8) -> Self {
        Self {
            origin: genesis.clone(),
            authored_events: HashMap::from([(genesis.clone(), 0)]),
            fork_index: ForkIndex::new(genesis, fork_index_submultiple),
        }
    }

    pub fn add_event<'a, F, TPayload>(
        &mut self,
        self_parent: event::Hash,
        event: event::Hash,
        event_lookup: F,
    ) -> Result<(), Error>
    where
        F: Fn(&event::Hash) -> Option<&'a event::Event<TPayload>>,
        TPayload: 'a,
    {
        // First do all checks, only then apply changes, to keep the state consistent

        // TODO: represent it in some way in the code. E.g. by creating 2 functions
        // where the first one may fail but works with immutable reference and the
        // second one does not fail but makes changes.
        // Otherwise it needs to be carefully reviewed by hand :(

        if self.authored_events.contains_key(&event) {
            return Err(Error::EventAlreadyKnown);
        }
        let parent_event = event_lookup(&self_parent).ok_or(Error::UnknownParent)?;
        let parent_height = self
            .authored_events
            .get(&self_parent)
            .ok_or(Error::UnknownParent)?;
        // Consider self children without the newly added event (just in case)
        let parent_self_children: event::SelfChild = parent_event
            .children
            .self_child
            .clone()
            .with_child_removed(&event);
        match parent_self_children {
            event::SelfChild::HonestParent(None) => {
                // It is a "leaf" in terms of forking, so we just add it to the index
                // Makes the final check and starts updating the state
                self.fork_index.push_event(event.clone(), &self_parent)?;
            }
            event::SelfChild::HonestParent(Some(firstborn)) => {
                // It is the second self child, so we creating a new fork
                let identifier = Self::find_fork_identifier(&self_parent, event_lookup)?;
                // completing the checks and starting to update the state
                self.fork_index.add_new_fork(
                    &identifier,
                    self_parent,
                    *parent_height,
                    event.clone(),
                    firstborn,
                )?;
            }
            event::SelfChild::ForkingParent(_) => {
                // Its parent already has forks, so we just add another one
                let identifier = Self::find_fork_identifier(&self_parent, event_lookup)?;
                // completing the checks and starting to update the state
                self.fork_index
                    .add_branch_to_fork(&identifier, event.clone())
                    .map_err(|e| <LookupIndexInconsistency>::from(e))?;
            }
        }
        self.authored_events.insert(event, parent_height + 1);
        Ok(())
    }

    fn find_fork_identifier<'a, F, TPayload>(
        target: &event::Hash,
        event_lookup: F,
    ) -> Result<event::Hash, Error>
    where
        F: Fn(&event::Hash) -> Option<&'a event::Event<TPayload>>,
        TPayload: 'a,
    {
        let mut this_event = event_lookup(target).ok_or(Error::InconsistentLookup)?;
        let mut prev_event = match this_event.parents() {
            event::Kind::Genesis => return Ok(target.clone()),
            event::Kind::Regular(parents) => {
                event_lookup(&parents.self_parent).ok_or(Error::InconsistentLookup)?
            }
        };
        while let event::Kind::Regular(parents) = prev_event.parents() {
            let prev_self_children: event::SelfChild = prev_event
                .children
                .self_child
                .clone()
                .with_child_removed(this_event.hash());
            if let event::SelfChild::ForkingParent(_) = prev_self_children {
                // `prev_event` has forking children, thus the child is the identifier we want
                return Ok(this_event.hash().clone());
            }
            this_event = prev_event;
            prev_event = event_lookup(&parents.self_parent).ok_or(Error::InconsistentLookup)?
        }
        // `prev_event` is genesis and thus it's the identifier
        Ok(prev_event.hash().clone())
    }

    /// Latest events across all known forks of the peer.
    /// If peer is honest, it will contain a single event
    pub fn latest_events(&self) -> Vec<&event::Hash> {
        self.fork_index.leaf_events()
    }

    pub fn forks<'a>(&'a self) -> ForkIndexIter<'a> {
        self.fork_index.iter()
    }

    pub fn forks_submultiple(&self) -> NonZeroU8 {
        *self.fork_index.submultiple()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    #[test]
    fn test_peer_index_constructs() {
        // Resulting layout; events are added in alphabetical order
        //   F E
        //  / /
        // A-B-C
        //    \
        //     D

        let peer = 3u64;
        let start_time = Duration::from_secs(1674549572);
        let mut all_events = HashMap::new();

        // Since we work with actual events, we can't use our sample hashes.
        let event_a =
            event::Event::new((), event::Kind::Genesis, peer, start_time.as_secs().into()).unwrap();
        let a_hash = event_a.hash().clone();
        all_events.insert(a_hash.clone(), event_a);
        let mut index = PeerIndexEntry::new(a_hash.clone(), NonZeroU8::new(3u8).unwrap());

        let event_b = event::Event::new(
            (),
            event::Kind::Regular(event::Parents {
                self_parent: a_hash.clone(),
                // doesn't matter what to put here, we don't test it at all
                other_parent: a_hash.clone(),
            }),
            peer,
            (start_time + Duration::from_secs(1)).as_secs().into(),
        )
        .unwrap();
        let b_hash = event_b.hash().clone();
        all_events.insert(b_hash.clone(), event_b);
        all_events
            .get_mut(&a_hash)
            .unwrap()
            .children
            .self_child
            .add_child(b_hash.clone());
        index
            .add_event(a_hash.clone(), b_hash.clone(), |h| all_events.get(h))
            .unwrap();

        let event_c = event::Event::new(
            (),
            event::Kind::Regular(event::Parents {
                self_parent: b_hash.clone(),
                // doesn't matter what to put here, we don't test it at all
                other_parent: a_hash.clone(),
            }),
            peer,
            (start_time + Duration::from_secs(3)).as_secs().into(),
        )
        .unwrap();
        let c_hash = event_c.hash().clone();
        all_events.insert(c_hash.clone(), event_c);
        all_events
            .get_mut(&b_hash)
            .unwrap()
            .children
            .self_child
            .add_child(c_hash.clone());
        index
            .add_event(b_hash.clone(), c_hash.clone(), |h| all_events.get(h))
            .unwrap();

        let event_d = event::Event::new(
            (),
            event::Kind::Regular(event::Parents {
                self_parent: b_hash.clone(),
                // doesn't matter what to put here, we don't test it at all
                other_parent: a_hash.clone(),
            }),
            peer,
            (start_time + Duration::from_secs(4)).as_secs().into(),
        )
        .unwrap();
        let d_hash = event_d.hash().clone();
        all_events.insert(d_hash.clone(), event_d);
        all_events
            .get_mut(&b_hash)
            .unwrap()
            .children
            .self_child
            .add_child(d_hash.clone());
        index
            .add_event(b_hash.clone(), d_hash.clone(), |h| all_events.get(h))
            .unwrap();

        let event_e = event::Event::new(
            (),
            event::Kind::Regular(event::Parents {
                self_parent: b_hash.clone(),
                // doesn't matter what to put here, we don't test it at all
                other_parent: a_hash.clone(),
            }),
            peer,
            (start_time + Duration::from_secs(5)).as_secs().into(),
        )
        .unwrap();
        let e_hash = event_e.hash().clone();
        all_events.insert(e_hash.clone(), event_e);
        all_events
            .get_mut(&b_hash)
            .unwrap()
            .children
            .self_child
            .add_child(e_hash.clone());
        index
            .add_event(b_hash.clone(), e_hash.clone(), |h| all_events.get(h))
            .unwrap();

        let event_f = event::Event::new(
            (),
            event::Kind::Regular(event::Parents {
                self_parent: a_hash.clone(),
                // doesn't matter what to put here, we don't test it at all
                other_parent: a_hash.clone(),
            }),
            peer,
            (start_time + Duration::from_secs(6)).as_secs().into(),
        )
        .unwrap();
        let f_hash = event_f.hash().clone();
        all_events.insert(f_hash.clone(), event_f);
        all_events
            .get_mut(&a_hash)
            .unwrap()
            .children
            .self_child
            .add_child(f_hash.clone());
        index
            .add_event(a_hash.clone(), f_hash.clone(), |h| all_events.get(h))
            .unwrap();
    }
}
