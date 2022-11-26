use serde::{Deserialize, Serialize};
use thiserror::Error;

use std::collections::HashMap;

pub mod event;
pub mod graph;

// u64 must be enough, if new round each 0.1 second
// then we'll be supplied for >5*10^10 years lol
//
// For 10 round/sec and u32 it's 27 years, so
// TODO put a warning for such case or drop program.
type RoundNum = usize;

use crate::PeerId;

type NodeIndex<TIndexPayload> = HashMap<event::Hash, TIndexPayload>;

struct PeerIndexEntry {
    genesis: event::Hash,
    /// Use `add_latest` for insertion
    authored_events: NodeIndex<()>,
    /// Forks authored by the peer that we've observed. Forks are events that have the same
    /// `self_parent`
    ///
    /// Represented by a mapping from `self_parent` to the forked events
    forks: HashMap<event::Hash, Vec<event::Hash>>,
    // Genesis at start
    latest_event: event::Hash,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum PushKind {
    Genesis,
    Regular(event::Parents),
}

impl PeerIndexEntry {
    fn new(genesis: event::Hash) -> Self {
        let latest_event = genesis.clone();
        Self {
            genesis,
            authored_events: HashMap::new(),
            forks: HashMap::new(),
            latest_event,
        }
    }

    /// Returns `Some(())` if entry already had event with this hash
    fn add_latest(&mut self, event: event::Hash) -> Option<()> {
        match self.authored_events.insert(event.clone(), ()) {
            Some(a) => Some(a),
            None => {
                self.latest_event = event;
                None
            }
        }
    }
}

#[derive(Error, Debug)]
pub enum PushError {
    #[error("Each peer can have only one genesis")]
    GenesisAlreadyExists,
    #[error("Could not find specified parent in the graph. Parent hash: `{0}`")]
    NoParent(event::Hash),
    #[error("Pushed node is already present in the graph. Hash: `{0}`. Can be triggered if hashes collide for actually different nodes (although extremely unlikely for 512 bit hash)")]
    NodeAlreadyExists(event::Hash),
    #[error("Given peer in not known ")]
    PeerNotFound(PeerId),
    /// (expected, provided)
    #[error("Provided author is different from author of self parent (expected {0}, provided {1}")]
    IncorrectAuthor(PeerId, PeerId),
    #[error("Serialization failed")]
    SerializationFailure(#[from] bincode::Error),
}
