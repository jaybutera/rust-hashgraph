use std::collections::VecDeque;

use tracing::warn;

use crate::{
    algorithm::{
        datastructure::peer_index::{PeerIndex, PeerIndexEntry},
        event, EventKind, Signature,
    },
    common::Graph,
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
    additions: Vec<AddEvent<TPayload>>,
}

impl<TPayload> Jobs<TPayload> {
    /// the result is topologically sorted, as we
    /// require all parents to be known at the time
    /// of event addition
    pub fn generate(
        self_graph: &crate::algorithm::datastructure::Graph<TPayload>,
        reciever_state: &CompressedKnownState,
    ) -> Self {
        // Since what we need is topsort, we can construct a graph of all missing
        // events (in reciever), with all relevant edges present (both for "parent" and
        // "other parent" relations).
        //
        // This graph would be DAG, because it's a subgraph of another DAG.
        //
        // Then we can unleash all power of topological ordering at the graph.

        // 1-2. Get a graph of (potentially) events that are potentially missing on reciever
        let missing_events = Self::missing_events_graph(
            self_graph,
            &self_graph.peer_index,
            reciever_state,
            |hash| self_graph.all_events.get(hash),
        );

        // 3. topologically sort them, send as jobs to perform

        todo!()
    }

    fn missing_events_graph<'a, 'b, TGraphIn, TLookup>(
        self_state: &'a TGraphIn,
        self_index: &PeerIndex,
        reciever_state: &CompressedKnownState,
        event_lookup: TLookup,
    ) -> Subgraph<'a, TGraphIn>
    where
        TGraphIn: Graph<NodeIdentifier = event::Hash>,
        TLookup: Fn(&event::Hash) -> Option<&'b event::Event<TPayload>>,
        TPayload: 'b,
    {
        // 1. find first unknown events by the reciever (first events right after latest common events).
        let starting_slice = Self::first_unknown_events(self_index, reciever_state, event_lookup);
        // 2. construct a sub graph that starts from events found in 1.
        Subgraph::new(self_state, starting_slice.into())
    }

    /// Earliest events that might be unknown by the reciever
    fn first_unknown_events<'a, TLookup>(
        self_index: &PeerIndex,
        reciever_known_state: &CompressedKnownState,
        event_lookup: TLookup,
    ) -> Vec<event::Hash>
    where
        TLookup: Fn(&event::Hash) -> Option<&'a event::Event<TPayload>>,
        TPayload: 'a,
    {
        let mut result = Vec::with_capacity(self_index.capacity());
        for (id_known_by_reciever, compressed_state) in reciever_known_state.as_vec() {
            let Some(peer_index) = self_index.get(id_known_by_reciever) else {
                continue;
            };
            result.extend(Self::first_unknown_peer_events(
                peer_index,
                compressed_state,
                &&event_lookup,
            ));
        }
        result
    }

    /// Earliest events created by a certain peer that might be unknown by the reciever.
    /// Might be a singleton set of origin event.
    fn first_unknown_peer_events<'a, TLookup>(
        self_index: &PeerIndexEntry,
        reciever_state: &CompressedPeerState,
        event_lookup: TLookup,
    ) -> Vec<event::Hash>
    where
        TLookup: Fn(&event::Hash) -> Option<&'a event::Event<TPayload>>,
        TPayload: 'a,
    {
        let origin = self_index.fork_index().origin();
        if self_index.fork_index().origin() != reciever_state.origin() {
            return vec![origin.clone()];
        }
        // Now BFS/DFS but it stops when an event is not known by the reciever
        // (and store it)
        Self::first_unknown_search(
            self_index,
            reciever_state,
            event_lookup,
            vec![origin.clone()],
        )
    }

    /// Find first unknown (to reciever) events
    fn first_unknown_search<'a, TLookup>(
        self_index: &PeerIndexEntry,
        reciever_state: &CompressedPeerState,
        event_lookup: TLookup,
        start_events: Vec<event::Hash>,
    ) -> Vec<event::Hash>
    where
        TLookup: Fn(&event::Hash) -> Option<&'a event::Event<TPayload>>,
        TPayload: 'a,
    {
        fn next_events_after<'a, TLookup, TPayload>(
            event: &event::Hash,
            event_lookup: TLookup,
        ) -> Vec<event::Hash>
        where
            TLookup: Fn(&event::Hash) -> Option<&'a event::Event<TPayload>>,
            TPayload: 'a,
        {
            warn!("Recieved submultiple does not equal to ours. This leads to inefficient synchronization.");
            event_lookup(&event)
                .expect("Could not find events tracked in fork index. It is inconsistent with general graph state.")
                .children
                .self_child
                .clone()
                .into()
        }

        let mut to_visit = start_events;
        let mut first_unknown = vec![];
        while let Some(next) = to_visit.pop() {
            // let branching_events = self_index
            //     .fork_index()
            //     .forks()
            //     .get(&next_identifier)
            //     .map(|fork| fork.forks().clone())
            //     .unwrap_or(vec![]);
            let self_extension = self_index
                .fork_index()
                .find_extension(&next)
                .expect("Inconsistent fork_index state (or bfs is broken)");
            let Some(reciever_entry) = reciever_state.entries().get(&next) else {
                // Reciever doesn't know the extension start.
                // 
                // Since the event is in `to_visit`, its self-parent is known
                // to the receiver. Thus it's one of the first unknowns
                first_unknown.push(next);
                // no need to go further here
                continue;
            };
            // at this point we checked that the reciever knows `next`
            let reciever_knows_extension_end = reciever_state
                .entries_starts()
                .contains_key(self_extension.last_event());
            if !reciever_knows_extension_end {
                if reciever_entry.section().submultiple() == self_extension.submultiple().into() {
                    // find latest known event in the extension
                    let mut latest_known = &next;
                    for (height, reciever_intermediate) in reciever_entry.section().intermediates()
                    {
                        if let Some(self_intermediate) =
                            self_extension.multiples().get_by_height(*height)
                        {
                            if reciever_intermediate == self_intermediate {
                                latest_known = reciever_intermediate;
                                continue;
                            }
                        }
                        break;
                    }
                    // add its child(ren?); should be only one child btw
                    let mut after_known = next_events_after(&latest_known, &event_lookup);
                    first_unknown.append(&mut after_known);
                } else {
                    let mut next_next = next_events_after(&next, &event_lookup);
                    first_unknown.append(&mut next_next);
                }
                continue;
            }
            // events in the extension are fully present in the peer
            if let Some(next_extension_forks) = self_index.fork_index().forks().get(&next) {
                // there are more events after this extension, need to visit them
                to_visit.extend_from_slice(next_extension_forks.forks())
            }
            // if it was the leaf, just move on
        }
        first_unknown
    }
}

pub struct Subgraph<'a, G: Graph> {
    super_graph: &'a G,
    to_visit: VecDeque<G::NodeIdentifier>,
}

impl<'a, G: Graph> Subgraph<'a, G> {
    pub fn new(super_graph: &'a G, starting_nodes: VecDeque<G::NodeIdentifier>) -> Self {
        Self {
            super_graph,
            to_visit: starting_nodes,
        }
    }
}

impl<'a, G> Graph for Subgraph<'a, G>
where
    G: Graph,
{
    type NodeIdentifier = G::NodeIdentifier;
    type NodeIdentifiers = G::NodeIdentifiers;

    fn neighbors(&self, node: &Self::NodeIdentifier) -> Self::NodeIdentifiers {
        self.super_graph.neighbors(node)
    }
}
