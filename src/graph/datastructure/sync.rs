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
    origin: event::Hash,
    data: HashMap<event::Hash, CompressedStateEntry>,
}

/// Helps to roughly figure out how many events are known in a chain (and to send)
/// without transferring all of the events.
#[derive(Debug, Clone, PartialEq)]
struct EventsSection {
    first: event::Hash,
    first_height: usize,
    last: event::Hash,
    length: usize,
    intermediate_threshold: u32,
    submultiple: u32,
    /// `(index, hash)`
    /// 
    /// Empty if `length` < `intermediate_threshold`
    /// 
    /// if `length` >= `intermediate_threshold`, the first element is the first
    /// whose height `H` is a multiple of `submultiple` (`S`), the next one is with height
    /// `H+S`, then `H+2S, H+4S, H+8S, ...` until the end of the section is reached
    intermediates: Vec<(usize, event::Hash)>,
}

enum CompressedStateEntry {
    /// Sequence of events with fork at the end (in case of dishonest author)
    Fork {
        events: EventsSection,
        /// First elements of each child
        forks: Vec<event::Hash>,
    },
    /// Just a sequence of events
    Leaf(EventsSection),
}

impl CompressedPeerState {
    pub fn generate(source: &PeerIndexEntry) -> Result<Self, ()> {
        // if source.
        // for extension in source.forks() {

        // }
        todo!()
    }
}

pub struct StateUpdateData;
