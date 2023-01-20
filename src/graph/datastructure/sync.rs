use std::collections::HashMap;

use crate::{graph::event, PeerId};

use super::{PeerIndex, PeerIndexEntry};

pub struct CompressedKnownState {
    peer_states: Vec<(PeerId, CompressedPeerState)>,
}

impl CompressedKnownState {
    pub fn generate(source: &PeerIndex) -> Self {
        let mut peer_states = Vec::with_capacity(source.len());
        for (id, entry) in source {
            let state = CompressedPeerState::generate(entry);
            peer_states.push((*id, state));
        }
        Self { peer_states }
    }
}

pub struct CompressedPeerState {
    data: HashMap<event::Hash, CompressedStateEntry>,
}

/// Helps to roughly figure out how many events are known in a chain (and to send)
/// without transferring all of the events.
struct EventsList {
    first: event::Hash,
    /// (index, hash)
    /// first would have index zero
    intermediate: (usize, event::Hash),
    last: event::Hash,
}

enum CompressedStateEntry {
    /// Sequence of events with fork at the end (in case of dishonest author)
    Fork {
        events: EventsList,
        forks: Vec<event::Hash>,
    },
    /// Just a sequence of events
    DeadEnd(EventsList),
}

impl CompressedPeerState {
    pub fn generate(source: &PeerIndexEntry) -> Self {
        todo!()
    }
}

pub struct StateUpdateData;
