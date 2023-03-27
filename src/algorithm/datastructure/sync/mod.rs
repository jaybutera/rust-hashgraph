use std::collections::{HashSet, VecDeque};

use thiserror::Error;

use crate::{
    algorithm::event,
    common::{Directed, Reversable},
};

pub struct Jobs<TPayload> {
    inner: Vec<event::Event<TPayload>>,
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("The provided tip is unknown in this state. Hash: {:?}.", 0)]
    IncorrectTip(event::Hash),
    #[error("Unknown event. Hash: {:?}.", 0)]
    UnknownEvent(event::Hash),
}

impl<TPayload> Jobs<TPayload> {
    fn generate<G, FKnows, FEvent>(
        known_state: G,
        peer_knows_event: FKnows,
        known_state_tips: impl Iterator<Item = event::Hash>,
        get_event: FEvent,
    ) -> Result<Self, Error>
    where
        G: Directed<NodeIdentifier = event::Hash, NodeIdentifiers = Vec<event::Hash>>,
        FKnows: Fn(&event::Hash) -> bool,
        FEvent: Fn(&event::Hash) -> Option<event::Event<TPayload>>,
    {
        // We need topologically sorted subgraph of known state, that is unknown
        // to the peer. The sorting must be from the oldest to the newest events.
        //
        // To find it, we do a trick: we find the reverse topsort.
        // 1. By definition of topological sorting,
        //      "for every directed edge u -> v from vertex u to vertex v, u comes before v in the ordering"
        //      We find topsort for such graph.
        // 2. Then we reverse each edge in the graph, so for each edge
        //      u <- v we will have u before v in the same ordering.
        // 3. Then we reverse/flip the ordering itself, we will have that for each
        //      u <- v, v is before u in the ordering.
        // Thus, it is a topological sort by defenition. Let's find it!

        // If we treat each parent -> child relationship as reverse (p <- c),
        // we need to start from the nodes without any children (thus without any
        // incoming edge p <- c). The nodes are tips.
        //
        // We can already filter out tips known to the peer, since all of its ancestors
        // are known to the peer.

        // nodes without incoming edges
        let sources: Vec<event::Hash> = known_state_tips
            .filter_map(|h| match known_state.in_neighbors(&h) {
                Some(in_neighbors) => {
                    if in_neighbors.is_empty() {
                        Some(Ok(h))
                    } else {
                        None
                    }
                }
                None => Some(Err(Error::IncorrectTip(h))),
            })
            .collect::<Result<_, _>>()?;
        let unknown_sources = sources.into_iter().filter(|h| !peer_knows_event(h));

        // Now do topsort with stop at known events

        // work with reversed events
        let reversed = known_state.reverse();
        let mut to_visit = VecDeque::from_iter(unknown_sources);
        // to check removed edges
        let mut visited = HashSet::with_capacity(to_visit.len());
        let mut sorted = Vec::with_capacity(to_visit.len());
        while let Some(next) = to_visit.pop_front() {
            for affected_neighbor in reversed
                .out_neighbors(&next)
                .ok_or_else(|| Error::UnknownEvent(next.clone()))?
            {
                if peer_knows_event(&affected_neighbor) {
                    continue;
                }
                if reversed
                    .in_neighbors(&affected_neighbor)
                    .ok_or_else(|| Error::UnknownEvent(next.clone()))?
                    .into_iter()
                    .all(|in_neighbor| visited.contains(&in_neighbor))
                {
                    // All in neighbors of `affected_neighbor` were visited before
                    to_visit.push_back(affected_neighbor)
                }
            }
            visited.insert(next.clone());
            sorted.push(next);
        }
        // note: no loop detection; we assume the graph already has no loops

        // Prepare the jobs
        sorted.reverse();
        let jobs: Vec<event::Event<TPayload>> = sorted
            .into_iter()
            .map(|hash| get_event(&hash).ok_or_else(|| Error::UnknownEvent(hash)))
            .collect::<Result<_, _>>()?;
        Ok(Jobs { inner: jobs })
    }
}
