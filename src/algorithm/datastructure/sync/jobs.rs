use std::collections::HashMap;

use petgraph::data::Build;

use crate::{
    algorithm::{datastructure::peer_index::fork_tracking::ForkIndex, event, EventKind, Signature},
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
    /// require all parents to be known at addition time
    pub fn generate(peer_state: &CompressedKnownState) -> Self {
        // Since what we need is topsort, we can construct a graph of all missing
        // events (in peer), with all relevant edges present (both for "parent" and
        // "other parent" relations).
        //
        // This graph would be DAG, because it's a subgraph of another DAG.
        //
        // Then we can unleash all power of topological ordering at the graph.

        todo!()
        // Self { peer_jobs }
    }

    fn missing_events_graph<TGraph, TLookup>(
        peer_state: &CompressedKnownState,
        event_lookup: &TLookup,
        fork_index: &ForkIndex,
    ) -> TGraph
    where
        TGraph: Build,
        TLookup: Fn(&event::Hash) -> event::Event<TPayload>,
    {
        let peer_jobs = peer_state.as_vec().iter().map(|(id, state)| {
            (
                *id,
                Self::latest_common_peer_events(state, event_lookup, fork_index),
            )
        });
        todo!()
    }

    fn latest_common_peer_events<TLookup>(
        peer_state: &CompressedPeerState,
        event_lookup: &TLookup,
        fork_index: &ForkIndex,
    ) -> Vec<event::Hash>
    where
        TLookup: Fn(&event::Hash) -> event::Event<TPayload>,
    {
        // probably compare with ForkIndex
        // let visitable_graph_until;
        todo!()
    }
}

pub struct PeerJobs<TPayload> {
    additions: Vec<AddEvent<TPayload>>,
}

impl<TPayload> PeerJobs<TPayload> {
    pub fn generate() -> Self {
        // find latest common events

        // going from them, get events missing in the peer

        // the result should be topologically sorted, as we
        // require all parents to be known at addition time
        todo!()
    }
}
