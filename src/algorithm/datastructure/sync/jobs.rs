use std::collections::{HashMap, HashSet};

use petgraph::{data::Build, visit::Visitable};

use crate::{
    algorithm::{
        datastructure::peer_index::{fork_tracking::ForkIndex, PeerIndex, PeerIndexEntry},
        event, EventKind, Signature,
    },
    common::DirectedGraph,
    PeerId, Timestamp,
};

use super::state::{CompressedKnownState, CompressedPeerState};

pub struct AddEvent<TPayload> {
    pub signature: Signature,
    pub payload: TPayload,
    pub event_type: EventKind,
    pub author: PeerId,
    pub time_created: Timestamp,
}

/// Sync jobs that need to be applied in order to achieve (at least)
/// the same knowledge as sender.
///
/// "at least" - because the receiver might know some more data (mostly
/// technicality)
pub struct Jobs<TPayload> {
    peer_jobs: Vec<(PeerId, PeerJobs<TPayload>)>,
}

impl<TPayload> Jobs<TPayload> {
    /// the result is topologically sorted, as we
    /// require all parents to be known at the time
    /// of event addition
    pub fn generate(self_fork_index: &ForkIndex, reciever_state: &CompressedKnownState) -> Self {
        // Since what we need is topsort, we can construct a graph of all missing
        // events (in reciever), with all relevant edges present (both for "parent" and
        // "other parent" relations).
        //
        // This graph would be DAG, because it's a subgraph of another DAG.
        //
        // Then we can unleash all power of topological ordering at the graph.

        // 1-2. Get a graph of (potentially) events that are potentially missing on reciever
        // Self::missing_events_graph(self_state, reciever_state, event_lookup, fork_index);

        // 3. topologically sort them, send as jobs to perform

        todo!()
    }

    fn missing_events_graph<TGraphIn, TGraphOut, TLookup>(
        self_index: &PeerIndex,
        reciever_state: &CompressedKnownState,
        event_lookup: &TLookup,
        self_state: &TGraphIn,
    ) -> TGraphOut
    where
        TGraphIn: Visitable,
        TGraphOut: Build,
        TLookup: Fn(&event::Hash) -> event::Event<TPayload>,
    {
        // 1. find first unknown events by the reciever (first events right after latest common events).
        Self::first_unknown_events(self_index, reciever_state, event_lookup);
        // 2. perform bfs starting from events found in 1., remember visited events as a graph
        todo!()
    }

    /// Earliest events that might be unknown by the reciever
    fn first_unknown_events<TLookup>(
        self_index: &PeerIndex,
        reciever_known_state: &CompressedKnownState,
        event_lookup: &TLookup,
    ) -> Vec<event::Hash>
    where
        TLookup: Fn(&event::Hash) -> event::Event<TPayload>,
    {
        let mut result = Vec::with_capacity(self_index.capacity());
        for (id_known_by_reciever, compressed_state) in reciever_known_state.as_vec() {
            let Some(peer_index) = self_index.get(id_known_by_reciever) else {
                continue;
            };
            result.extend(Self::first_unknown_peer_events(
                peer_index,
                compressed_state,
                event_lookup,
            ));
        }
        todo!()
    }

    /// Earliest events created by a certain peer that might be unknown by the reciever.
    /// Might be a singleton set of origin event.
    fn first_unknown_peer_events<TLookup>(
        self_index: &PeerIndexEntry,
        reciever_state: &CompressedPeerState,
        event_lookup: &TLookup,
    ) -> HashSet<event::Hash>
    where
        TLookup: Fn(&event::Hash) -> event::Event<TPayload>,
    {
        let origin = self_index.fork_index().origin();
        if self_index.fork_index().origin() != reciever_state.origin() {
            return HashSet::from([origin.clone()]);
        }
        // Now BFS but it stops when an event is not known by the reciever
        // (and store it)
        todo!()
    }

    fn custom_bfs_aboba<TLookup>(
        self_index: &PeerIndexEntry,
        reciever_state: &CompressedPeerState,
        event_lookup: &TLookup,
        start_events: Vec<event::Hash>,
    ) -> HashSet<event::Hash>
    where
        TLookup: Fn(&event::Hash) -> event::Event<TPayload>,
    {
        let mut to_visit = start_events;
        let mut latest_known = vec![];
        while let Some(next) = to_visit.pop() {}
        todo!();
    }
}

pub struct PeerJobs<TPayload> {
    additions: Vec<AddEvent<TPayload>>,
}
