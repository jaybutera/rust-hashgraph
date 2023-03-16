use std::collections::{HashMap, HashSet, VecDeque};

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
    known_events: HashSet<event::Hash>,
    authored_events: EventIndex<()>,
    /// Forks authored by the peer that we've observed. Forks are events
    /// that have the same `self_parent`.
    ///
    /// Represented by event hashes that have multiple self children
    /// (children authored by the same peer)
    fork_index: ForkIndex,
    latest_events: HashSet<event::Hash>,
}

#[derive(Debug, Error, PartialEq)]
pub enum Error {
    #[error("Attempted to add an event that is already present in the index")]
    EventAlreadyKnown,
    #[error("Event {0} is unknown")]
    UnknownEvent(event::Hash),
}

impl PeerIndexEntry {
    pub fn new(genesis: event::Hash) -> Self {
        Self {
            origin: genesis.clone(),
            authored_events: HashMap::from([(genesis.clone(), ())]),
            known_events: HashSet::from([genesis.clone()]),
            fork_index: ForkIndex::new(),
            latest_events: HashSet::from_iter([genesis.clone()].into_iter()),
        }
    }

    /// Add event to the index. The added event should have all ancestors added
    /// before.
    ///
    /// `events_in_direct_sight` should return Some(_) when the queried hash is known
    /// and None if unknown (should not happen on consistent graph). It may not know
    /// about `event` yet though.
    pub fn add_event<'a, TPayload, F>(
        &mut self,
        events_in_direct_sight: F,
        self_parent: &event::Event<TPayload>,
        other_parent: event::Hash,
        event: event::Hash,
    ) -> Result<(), Error>
    where
        F: Fn(&event::Hash) -> Option<Vec<&'a event::Hash>>,
    {
        // First do all checks, only then apply changes, to keep the state consistent

        // TODO: represent it in some way in the code. E.g. by creating 2 functions
        // where the first one may fail but works with immutable reference and the
        // second one does not fail but makes changes.
        // Otherwise it needs to be carefully reviewed by hand :(

        if self.authored_events.contains_key(&event) {
            return Err(Error::EventAlreadyKnown);
        }
        // `event` itself is unknown to `events_in_direct_sight` yet, so we start from its
        // parents
        self.add_known_events(
            vec![self_parent.hash().clone(), other_parent],
            events_in_direct_sight,
        )?;
        // Consider self children without the newly added event (just in case)
        let parent_self_children: event::SelfChild = self_parent
            .children
            .self_child
            .clone()
            .with_child_removed(&event);
        match parent_self_children {
            event::SelfChild::HonestParent(None) => {
                self.latest_events.remove(self_parent.hash());
            }
            event::SelfChild::HonestParent(Some(_)) | event::SelfChild::ForkingParent(_) => {
                self.fork_index.track_fork(self_parent, event.clone());
            }
        }
        self.authored_events.insert(event.clone(), ());
        self.latest_events.insert(event);
        Ok(())
    }

    /// Update `known_events` index to include newly-seen events by the peer. It should
    /// help to always have the relevant list of events the peer sees and not recompute
    /// it on demand.
    ///
    /// `events_in_direct_sight` should return Some(_) when the queried hash is known
    /// and None if unknown (should not happen on consistent graph)
    fn add_known_events<'a, F>(
        &mut self,
        start: Vec<event::Hash>,
        events_in_direct_sight: F,
    ) -> Result<(), Error>
    where
        F: Fn(&event::Hash) -> Option<Vec<&'a event::Hash>>,
    {
        if start.iter().all(|s| self.known_events.contains(s)) {
            return Ok(());
        }
        let mut to_visit = VecDeque::from(start);
        // Add them after traversal. To not to leave the index in potentially incorrect
        // state in case error happens.
        let mut new_known_events = vec![];
        while let Some(next) = to_visit.pop_front() {
            let new_events =
                events_in_direct_sight(&next).ok_or(Error::UnknownEvent(next.clone()))?;
            let new_events = new_events
                .into_iter()
                .filter(|h| !self.known_events.contains(h))
                .cloned();
            to_visit.extend(new_events);
            new_known_events.push(next.clone());
        }
        self.known_events.extend(new_known_events);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashSet, time::Duration};

    // Returns the index and all_events tracker
    fn construct_peer_index<TPayload: Clone>(
        events: &[event::Event<TPayload>],
    ) -> (PeerIndexEntry, HashMap<event::Hash, event::Event<TPayload>>) {
        let mut events = events.into_iter();
        let mut all_events: HashMap<event::Hash, event::Event<TPayload>> = HashMap::new();
        let genesis = events.next().expect("event list must be nonempty");
        all_events.insert(genesis.hash().clone(), genesis.clone());
        let mut peer_index = PeerIndexEntry::new(genesis.hash().clone());
        for event in events {
            all_events.insert(event.hash().clone(), event.clone());
            let (self_parent, other_parent) = if let event::Kind::Regular(p) = event.parents() {
                (p.self_parent.clone(), p.other_parent.clone())
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
                    |_h| Some(vec![]),
                    all_events
                        .get(&self_parent)
                        .expect("unknown event listed as peer"),
                    other_parent,
                    event.hash().clone(),
                )
                .unwrap();
        }
        (peer_index, all_events)
    }

    #[test]
    fn test_peer_index_constructs() {
        // Resulting layout; events are added in alphabetical order
        //   F E
        //  / /
        // A-B-C
        //    \
        //     D

        let peer = 3u64;
        let start_time = Duration::from_secs(0);

        // Since we work with actual events, we can't use our sample hashes.
        let event_a =
            event::Event::new((), event::Kind::Genesis, peer, start_time.as_secs().into()).unwrap();
        let a_hash = event_a.hash().clone();

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

        // check test for correctness
        let events = vec![event_a, event_b, event_c, event_d, event_e, event_f];
        let (index_2, _all_events_2) = construct_peer_index(&events);

        let authored_events_expected =
            HashMap::<_, _>::from_iter(events.iter().map(|e| (e.hash().clone(), ())));
        assert_eq!(
            index_2.authored_events(),
            &authored_events_expected,
            "authored events not tracked correctly"
        );
        assert!(
            index_2
                .latest_events()
                .contains(events.last().unwrap().hash()),
            "Latest event is not tracked as tip"
        );
        assert_eq!(index_2.origin(), events.first().unwrap().hash());
        let forks_expected = HashMap::<_, _>::from_iter([
            (
                events[0].hash().clone(),
                HashSet::<_>::from_iter([events[1].hash(), events[5].hash()].into_iter().cloned()),
            ),
            (
                events[1].hash().clone(),
                HashSet::<_>::from_iter(
                    [events[2].hash(), events[3].hash(), events[4].hash()]
                        .into_iter()
                        .cloned(),
                ),
            ),
        ]);
        assert_eq!(index_2.fork_index().forks(), &forks_expected);
    }
}
