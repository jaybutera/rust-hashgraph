use std::collections::{HashMap, HashSet};

use super::event;

pub type NodeIndex<TIndexPayload> = HashMap<event::Hash, TIndexPayload>;

pub struct PeerIndexEntry {
    genesis: event::Hash,
    /// Use `add_latest` for insertion
    authored_events: NodeIndex<()>,
    /// Forks authored by the peer that we've observed. Forks are events that have the same
    /// `self_parent`
    ///
    /// Represented by a mapping from `self_parent` to the forked events
    forks: HashMap<event::Hash, HashSet<event::Hash>>,
    // Genesis at start
    latest_event: event::Hash,
}

impl PeerIndexEntry {
    pub fn new(genesis: event::Hash) -> Self {
        let latest_event = genesis.clone();
        Self {
            genesis,
            authored_events: HashMap::new(),
            forks: HashMap::new(),
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

    pub fn add_fork(&mut self, parent: event::Hash, child: event::Hash) {
        let fork_list = self.forks.entry(parent).or_default();
        fork_list.insert(child);
    }

    pub fn latest_event(&self) -> &event::Hash {
        &self.latest_event
    }

    pub fn genesis(&self) -> &event::Hash {
        &self.genesis
    }
}
