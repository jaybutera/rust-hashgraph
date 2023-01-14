use std::collections::{HashMap, HashSet};

use super::event;

pub type EventIndex<TIndexPayload> = HashMap<event::Hash, TIndexPayload>;

pub struct PeerIndexEntry {
    genesis: event::Hash,
    /// Use `add_latest` for insertion
    authored_events: EventIndex<()>,
    /// Forks authored by the peer that we've observed. Forks are events
    /// that have the same `self_parent`.
    ///
    /// Represented by event hashes that have multiple self children
    /// (childr authored by the same peer)
    forks: HashSet<event::Hash>,
    // Genesis at start
    latest_event: event::Hash,
}

impl PeerIndexEntry {
    pub fn new(genesis: event::Hash) -> Self {
        let latest_event = genesis.clone();
        Self {
            genesis,
            authored_events: HashMap::new(),
            forks: HashSet::new(),
            latest_event,
        }
    }

    /// Returns `Some(())` if entry already had event with this hash
    pub fn add_latest(&mut self, event: event::Hash) -> Option<()> {
        match self.authored_events.insert(event.clone(), ()) {
            Some(a) => Some(a),
            None => {
                self.latest_event = event;
                None
            }
        }
    }

    /// Idempotent, i.e. adding already tracked fork won't change anything
    ///
    /// Returns if the fork was already tracked
    pub fn add_fork(&mut self, parent: event::Hash) -> bool {
        self.forks.insert(parent)
    }

    pub fn latest_event(&self) -> &event::Hash {
        &self.latest_event
    }

    pub fn genesis(&self) -> &event::Hash {
        &self.genesis
    }
}
