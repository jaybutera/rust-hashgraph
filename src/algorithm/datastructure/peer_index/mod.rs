use std::collections::{HashMap, HashSet};

use derive_getters::Getters;
use thiserror::Error;

use crate::PeerId;

use self::fork_tracking::ForkIndex;
use super::{event, EventIndex};

pub mod fork_tracking;

pub type PeerIndex = HashMap<PeerId, PeerIndexEntry>;

#[derive(Getters)]
pub struct PeerIndexEntry {
    origin: event::Hash,
    authored_events: EventIndex<()>,
    /// Forks authored by the peer that we've observed. Forks are events
    /// that have the same `self_parent`.
    ///
    /// Represented by event hashes that have multiple self children
    /// (children authored by the same peer)
    fork_index: ForkIndex,
    latest_event: event::Hash,
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
}

impl PeerIndexEntry {
    pub fn new(genesis: event::Hash) -> Self {
        Self {
            origin: genesis.clone(),
            authored_events: HashMap::from([(genesis.clone(), ())]),
            fork_index: ForkIndex::new(),
            latest_event: genesis.clone(),
        }
    }

    pub fn add_event<TPayload>(
        &mut self,
        self_parent: &event::Event<TPayload>,
        event: event::Hash,
    ) -> Result<(), Error> {
        // First do all checks, only then apply changes, to keep the state consistent

        // TODO: represent it in some way in the code. E.g. by creating 2 functions
        // where the first one may fail but works with immutable reference and the
        // second one does not fail but makes changes.
        // Otherwise it needs to be carefully reviewed by hand :(

        if self.authored_events.contains_key(&event) {
            return Err(Error::EventAlreadyKnown);
        }
        // Consider self children without the newly added event (just in case)
        let parent_self_children: event::SelfChild = self_parent
            .children
            .self_child
            .clone()
            .with_child_removed(&event);
        match parent_self_children {
            event::SelfChild::HonestParent(None) => {}
            event::SelfChild::HonestParent(Some(_)) | event::SelfChild::ForkingParent(_) => {
                self.fork_index.track_fork(self_parent, event);
            }
        }
        self.authored_events.insert(event, ());
        self.latest_event = event;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    // Returns the index and all_events tracker
    fn construct_peer_index<TPayload>(
        events: &[(event::Event<TPayload>)],
    ) -> (PeerIndexEntry, HashMap<event::Hash, event::Event<TPayload>>) {
        let events = events.into_iter();
        let mut all_events = HashMap::new();
        let genesis = events.next().expect("event list must be nonempty");
        all_events.insert(genesis.hash().clone(), *genesis.clone());
        let mut peer_index = PeerIndexEntry::new(genesis.hash().clone());
        for event in events {
            all_events.insert(event.hash().clone(), *event.clone());
            let self_parent = if let event::Kind::Regular(p) = event.parents() {
                p.self_parent
            } else {
                panic!("2 geneses, can't add to the index");
            };
            all_events
                .get_mut(&self_parent)
                .unwrap()
                .children
                .self_child
                .add_child(event.hash().clone());
            peer_index
                .add_event(
                    all_events
                        .get(&self_parent)
                        .expect("unknown event listed as peer"),
                    event.hash().clone(),
                )
                .unwrap();
        }
        (peer_index, all_events)
    }

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
        let mut index = PeerIndexEntry::new(a_hash.clone());

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
            .add_event(all_events.get(&a_hash).expect("aboba"), b_hash.clone())
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
            .add_event(all_events.get(&b_hash).expect("akeke"), c_hash.clone())
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
            .add_event(all_events.get(&b_hash).expect("akeke"), d_hash.clone())
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
            .add_event(all_events.get(&b_hash).expect("akeke"), e_hash.clone())
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
            .add_event(all_events.get(&a_hash).expect("akeke"), f_hash.clone())
            .unwrap();

        // check test for correctness
        let events = vec![event_a, event_b, event_c, event_d, event_e, event_f];
        let (index_2, all_events_2) = construct_peer_index(&events);
    }
}
