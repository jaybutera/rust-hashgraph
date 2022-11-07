use serde::Serialize;

use std::collections::{HashMap, HashSet};

use crate::PeerId;

use super::event::{self, Event, Parents};
use super::{RoundNum, PeerIndexEntry, PushKind, PushError};


type NodeIndex<TIndexPayload> = HashMap<event::Hash, TIndexPayload>;

pub struct Graph<TPayload> {
    all_events: NodeIndex<Event<TPayload>>,
    peer_index: HashMap<PeerId, PeerIndexEntry>,
    self_id: PeerId,
    round_index: Vec<HashSet<event::Hash>>,
    /// Some(false) means unfamous witness
    is_famous: HashMap<event::Hash, bool>,
    round_of: HashMap<event::Hash, RoundNum>, // Just testing a caching system for now
}

impl<T: Serialize> Graph<T> {
    pub fn new(self_id: PeerId, genesis_payload: T) -> Self {
        let mut graph = Self {
            all_events: HashMap::new(),
            peer_index: HashMap::new(),
            self_id,
            round_index: vec![HashSet::new()],
            is_famous: HashMap::new(),
            round_of: HashMap::new(),
        };

        graph.push_node(genesis_payload, PushKind::Genesis, self_id)
            .expect("Genesis events should be valid");
        graph
    }
}


impl<TPayload: Serialize> Graph<TPayload> {
    /// Create and push node to the graph, adding it at the end of `author`'s lane
    /// (i.e. the node becomes the latest event of the peer).
    pub fn push_node(
        &mut self,
        payload: TPayload,
        node_type: PushKind,
        author: PeerId,
    ) -> Result<event::Hash, PushError> {
        // Verification first, no changing state

        let new_node = match node_type {
            PushKind::Genesis => {
                Event::new(payload, event::Kind::Genesis, author)?
            }
            PushKind::Regular(other_parent) => {
                let latest_author_event = &self.peer_index.get(&author)
                    .ok_or(PushError::PeerNotFound(author))?
                    .latest_event;
                Event::new(payload, event::Kind::Regular(Parents { self_parent: latest_author_event.clone(), other_parent }), author)?
            }
        };

        if self.all_events.contains_key(new_node.hash()) {
            return Err(PushError::NodeAlreadyExists(new_node.hash().clone()));
        }
        
        match new_node.parents() {
            event::Kind::Genesis => {
                if self.peer_index.contains_key(&author) {
                    return Err(PushError::GenesisAlreadyExists);
                }
                let new_peer_index = PeerIndexEntry::new(new_node.hash().clone());
                self.peer_index.insert(author, new_peer_index);
            }
            event::Kind::Regular(parents) => {
                if !self.all_events.contains_key(&parents.self_parent) {
                    // Should not be triggered, since we check it above
                    return Err(PushError::NoParent(parents.self_parent.clone()));
                }
                if !self.all_events.contains_key(&parents.other_parent) {
                    return Err(PushError::NoParent(parents.other_parent.clone()));
                }

                // taking mutable for update later
                let self_parent_node = self
                    .all_events
                    .get_mut(&parents.self_parent) // TODO: use get_many_mut when stabilized
                    .expect("Just checked presence before");

                if self_parent_node.author() != &author {
                    return Err(PushError::IncorrectAuthor(
                        self_parent_node.author().clone(),
                        author,
                    ));
                }

                if let Some(existing_child) = &self_parent_node.children.self_child {
                    // Should not happen since latest events should not have self children
                    return Err(PushError::SelfChildAlreadyExists(existing_child.clone()));
                }

                // taking mutable for update later
                let author_index = self
                    .peer_index
                    .get_mut(&author)
                    .ok_or(PushError::PeerNotFound(author))?;

                // Insertion, should be valid at this point so that we don't leave in inconsistent state on error.

                // update pointers of parents
                self_parent_node.children.self_child = Some(new_node.hash().clone());
                let other_parent_node = self
                    .all_events
                    .get_mut(&parents.other_parent)
                    .expect("Just checked presence before");
                other_parent_node
                    .children
                    .other_children
                    .push(new_node.hash().clone());
                if let Some(_) = author_index
                    .add_latest(new_node.hash().clone())
                {
                    // TODO: warn
                    panic!()
                }
            }
        };

        // Index the node and save
        let hash = new_node.hash().clone();
        self.all_events.insert(new_node.hash().clone(), new_node);

        // Set round

        let last_idx = self.round_index.len() - 1;
        let r = self.determine_round(&hash);
        // Cache result
        self.round_of.insert(hash.clone(), r);
        if r > last_idx {
            // Create a new round
            let mut round_hs = HashSet::new();
            round_hs.insert(hash.clone());
            self.round_index.push(round_hs);
        } else {
            // Otherwise push onto current round
            // (TODO: check why not to round `r`????)
            self.round_index[last_idx].insert(hash.clone());
        }

        // Set witness status
        if self.determine_witness(hash.clone()) {
            self.is_famous.insert(hash.clone(), false);
        }
        Ok(hash)
    }
}

impl<TPayload> Graph<TPayload> {
    pub fn members_count(&self) -> usize {
        self.peer_index.keys().len()
    }

    pub fn peer_latest_event(&self, peer: &PeerId) -> Option<&event::Hash> {
        self.peer_index.get(peer)
            .map(|e| &e.latest_event)
    }

    pub fn peer_genesis(&self, peer: &PeerId) -> Option<&event::Hash> {
        self.peer_index.get(peer)
            .map(|e| &e.genesis)
    }

    pub fn event(&self, id: &event::Hash) -> Option<&TPayload> {
        self.all_events.get(id)
            .map(|e| e.payload())
    }


    /// Iterator over ancestors of the event
    pub fn iter(&self, event_hash: &event::Hash) -> Option<EventIter<TPayload>> {
        let event = self.all_events.get(event_hash)?;
        let mut e_iter = EventIter {
            node_list: vec![],
            all_events: &self.all_events,
        };

        if let event::Kind::Regular(Parents { self_parent, .. }) = event.parents() {
            e_iter.push_self_parents(event_hash)
        }
        Some(e_iter)
    }

    /// Determine the round an event belongs to, which is the max of its parents' rounds +1 if it
    /// is a witness.
    fn determine_round(&self, event_hash: &event::Hash) -> RoundNum {
        let event = self.all_events.get(event_hash).unwrap();
        match event.parents() {
            event::Kind::Genesis => 0,
            event::Kind::Regular(Parents {
                self_parent,
                other_parent,
            }) => {
                // Check if it is cached
                if let Some(r) = self.round_of.get(event_hash) {
                    return *r;
                }
                let r = std::cmp::max(
                    self.determine_round(self_parent),
                    self.determine_round(other_parent),
                );

                // Get events from round r
                let round = self.round_index[r]
                    .iter()
                    .filter(|eh| *eh != event_hash)
                    .map(|e_hash| self.all_events.get(e_hash).unwrap())
                    .collect::<Vec<_>>();

                // Find out how many witnesses by unique members the event can strongly see
                let witnesses_strongly_seen = round
                    .iter()
                    .filter(|e| self.is_famous.get(&e.hash()).is_some())
                    .fold(HashSet::new(), |mut set, e| {
                        if self.strongly_see(event_hash, &e.hash()) {
                            let creator = e.author();
                            set.insert(creator.clone());
                        }
                        set
                    });

                // n is number of members in hashgraph
                let n = self.members_count();

                if witnesses_strongly_seen.len() > (2 * n / 3) {
                    r + 1
                } else {
                    r
                }
            }
        }
    }
}

impl<TPayload> Graph<TPayload> {
    pub fn round_of(&self, event_hash: event::Hash) -> RoundNum {
        match self.round_of.get(&event_hash) {
            Some(r) => *r,
            None => {
                self.round_index
                    .iter()
                    .enumerate()
                    .find(|(_, round)| round.contains(&event_hash))
                    .expect("Failed to find a round for event")
                    .0
            }
        }
    }

    /// Determines if the event is a witness
    pub fn determine_witness(&self, event_hash: event::Hash) -> bool {
        match self.all_events.get(&event_hash).unwrap().parents() {
            event::Kind::Genesis => true,
            event::Kind::Regular(Parents{self_parent, .. }) => {
                self.round_of(event_hash) > self.round_of(self_parent.clone())
            }
        }
    }

    /// Determine if the event is famous.
    /// An event is famous if 2/3 future witnesses strongly see it.
    fn is_famous(&self, event_hash: event::Hash) -> bool {
        let event = self.all_events.get(&event_hash).unwrap();

        match event.parents() {
            event::Kind::Genesis => true,
            event::Kind::Regular(_) => {
                // Event must be a witness
                if !self.determine_witness(event_hash.clone()) {
                    return false;
                }

                let witnesses = self
                    .all_events
                    .values()
                    .filter(|e| self.is_famous.get(&e.hash()).is_some())
                    .fold(HashSet::new(), |mut set, e| {
                        if self.strongly_see(&e.hash(), &event_hash) {
                            let creator = e.author();
                            set.insert(creator.clone());
                        }
                        set
                    });

                let n = self.members_count();
                witnesses.len() > (2 * n / 3)
            }
        }
    }

    fn ancestor(&self, target: &event::Hash, potential_ancestor: &event::Hash) -> bool {
        let _x = self.all_events.get(target).unwrap();
        let _y = self.all_events.get(potential_ancestor).unwrap();

        self.iter(target).unwrap().any(|e| e.hash() == potential_ancestor)
    }

    fn strongly_see(&self, target: &event::Hash, observer: &event::Hash) -> bool {
        let mut creators_seen = self
            .iter(target).unwrap()
            .filter(|e| self.ancestor(&e.hash(), observer))
            .fold(HashSet::new(), |mut set, event| {
                let creator = event.author();
                set.insert(creator.clone());
                set
            });

        // Add self to seen set incase it wasn't traversed above
        match self.all_events.get(target).unwrap().parents() {
            event::Kind::Genesis => true,
            event::Kind::Regular(_) => creators_seen.insert(self.self_id.clone()),
        };

        let n = self.members_count();
        creators_seen.len() > (2 * n / 3)
    }
}


pub struct EventIter<'a, T> {
    node_list: Vec<&'a Event<T>>,
    all_events: &'a HashMap<event::Hash, Event<T>>,
}

impl<'a, T> EventIter<'a, T> {
    fn push_self_parents(&mut self, event_hash: &event::Hash) {
        let mut event = self.all_events.get(event_hash).unwrap();

        loop {
            self.node_list.push(event);

            if let event::Kind::Regular(Parents { self_parent, .. }) = event.parents() {
                event = self.all_events.get(self_parent).unwrap();
            } else {
                break;
            }
        }
    }
}

impl<'a, T> Iterator for EventIter<'a, T> {
    type Item = &'a Event<T>;

    fn next(&mut self) -> Option<Self::Item> {
        let event = self.node_list.pop()?;

        if let event::Kind::Regular(Parents { other_parent, .. }) = event.parents() {
            self.push_self_parents(other_parent);
        }
        Some(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // for more concise tests
    fn create_event_regul<T: Serialize>(
        graph: &mut Graph<T>,
        author: PeerId,
        other_parent: event::Hash,
        payload: T,
    ) -> Result<event::Hash, PushError> {
        graph.push_node(
            payload,
            PushKind::Regular(other_parent),
            author,
        )
    }

    struct PeerEvents {
        id: PeerId,
        events: Vec<event::Hash>,
    }

    fn build_graph1<T: Serialize + Copy>(payload: T) -> Result<(Graph<T>, [PeerEvents; 5]), PushError> {
        // Authors + Geneses
        //                                      a, b, c, d, e
        let peers = (0, 1, 2, 3, 4);
        let mut graph = Graph::new(peers.0, payload);
        let geneses = (
            graph.peer_genesis(&peers.0).expect("Just added genesis").clone(),
            graph.push_node(payload, PushKind::Genesis, peers.1)?,
            graph.push_node(payload, PushKind::Genesis, peers.2)?,
            graph.push_node(payload, PushKind::Genesis, peers.3)?,
            graph.push_node(payload, PushKind::Genesis, peers.4)?,
        );
        let c2 = create_event_regul(&mut graph, peers.2, geneses.3.clone(), payload)?;
        let e2 = create_event_regul(&mut graph, peers.4, geneses.1.clone(), payload)?;
        let b2 = create_event_regul(&mut graph, peers.1, c2.clone(), payload)?;
        let c3 = create_event_regul(&mut graph, peers.2, e2.clone(), payload)?;
        let d2 = create_event_regul(&mut graph, peers.3, c3.clone(), payload)?;
        let a2 = create_event_regul(&mut graph, peers.0, b2.clone(), payload)?;
        let b3 = create_event_regul(&mut graph, peers.1, c3.clone(), payload)?;
        let c4 = create_event_regul(&mut graph, peers.2, d2.clone(), payload)?;
        let a3 = create_event_regul(&mut graph, peers.0, b3.clone(), payload)?;
        let c5 = create_event_regul(&mut graph, peers.2, e2.clone(), payload)?;
        let c6 = create_event_regul(&mut graph, peers.2, a3.clone(), payload)?;

        let peers_events = [
            PeerEvents{ id: peers.0, events: vec![geneses.0, a2, a3] },
            PeerEvents{ id: peers.1, events: vec![geneses.1, b2, b3] },
            PeerEvents{ id: peers.2, events: vec![geneses.2, c2, c3, c4, c5, c6] },
            PeerEvents{ id: peers.3, events: vec![geneses.3, d2] },
            PeerEvents{ id: peers.4, events: vec![geneses.4, e2] },
        ];

        Ok((graph, peers_events))
    }

    fn build_graph2<T: Serialize + Copy>(payload: T) -> Result<(Graph<T>, [PeerEvents; 3]), PushError> {
        /* Generates the following graph for each member (c1,c2,c3)
         *
            |  o__|  -- e7
            |__|__o  -- e6
            o__|  |  -- e5
            |  o__|  -- e4
            |  |__o  -- e3
            |__o  |  -- e2
            o__|  |  -- e1
            o  o  o  -- (g1,g2,g3)
        */

        // Authors + Geneses
        //                            c1,c2,c3
        let peers = (0, 1, 2);
        let mut graph = Graph::new(peers.0, payload);
        let geneses = (
            graph.peer_genesis(&peers.0).expect("Just added genesis").clone(),
            graph.push_node(payload, PushKind::Genesis, peers.1)?,
            graph.push_node(payload, PushKind::Genesis, peers.2)?,
        );
        let e1 = create_event_regul(&mut graph, peers.0, geneses.1.clone(), payload).unwrap();
        let e2 = create_event_regul(&mut graph, peers.1, e1.clone(), payload).unwrap();
        let e3 = create_event_regul(&mut graph, peers.2, e2.clone(), payload).unwrap();
        let e4 = create_event_regul(&mut graph, peers.1, e3.clone(), payload).unwrap();
        let e5 = create_event_regul(&mut graph, peers.0, e4.clone(), payload).unwrap();
        let e6 = create_event_regul(&mut graph, peers.2, e5.clone(), payload).unwrap();
        let e7 = create_event_regul(&mut graph, peers.1, e6.clone(), payload).unwrap();

        let peers_events = [
            PeerEvents{ id: peers.0, events: vec![geneses.0, e1, e5] },
            PeerEvents{ id: peers.1, events: vec![geneses.1, e2, e4, e7] },
            PeerEvents{ id: peers.2, events: vec![geneses.2, e3, e6] },
        ];

        Ok((graph, peers_events))
    }

    // Test simple work + errors

    #[test]
    fn graph_builds() {
        build_graph1(()).unwrap();

        build_graph2(()).unwrap();
    }

    #[test]
    fn duplicate_push_fails() {
        let (mut graph, peers) = build_graph1(()).unwrap();
        assert!(matches!(
            graph.push_node((), PushKind::Genesis, peers[0].id),
            Err(PushError::NodeAlreadyExists(hash)) if &hash == graph.peer_genesis(&peers[0].id).unwrap()
        ));
    }

    #[test]
    fn double_genesis_fails() {
        let (mut graph, peers) = build_graph1(0).unwrap();
        assert!(matches!(
            graph.push_node(1, PushKind::Genesis, peers[0].id),
            Err(PushError::GenesisAlreadyExists)
        ))
    }

    #[test]
    fn missing_parent_fails() {
        let (mut graph, peers) = build_graph1(()).unwrap();
        let fake_node = Event::new((), event::Kind::Genesis, 1232423).unwrap();
        assert!(matches!(
            create_event_regul(&mut graph, peers[0].id, fake_node.hash().clone(), ()),
            Err(PushError::NoParent(fake_hash)) if &fake_hash == fake_node.hash()
        ))
    }

    // Test graph properties

    #[test]
    fn test_ancestor() {
        let (graph, peers_events) = build_graph2(()).unwrap();

        assert!(graph.ancestor(
            &peers_events[0].events[1],
            &peers_events[0].events[0]
        ));

        
        let (graph, peers_events) = build_graph1(()).unwrap();
        assert!(graph.ancestor(
            &peers_events[2].events[5],
            &peers_events[1].events[0],
        ));

        assert!(graph.ancestor(
            &peers_events[0].events[2],
            &peers_events[4].events[1],
        ));
    }
        
    #[test]
    fn test_strongly_see() {
        let (graph, peers_events) = build_graph2(()).unwrap();

        assert!(!graph.strongly_see(
            &peers_events[1].events[1],
            &peers_events[0].events[0],
        ));
        assert!(graph.strongly_see(
            &peers_events[1].events[2],
            &peers_events[0].events[0],
        ));

        let (graph, peers_events) = build_graph1(()).unwrap();
        assert!(graph.strongly_see(
            &peers_events[2].events[5],
            &peers_events[3].events[0],
        ))
    }

    #[test]
    fn test_determine_round() {
        let (graph, peers_events) = build_graph2(()).unwrap();

        assert!(graph.round_index[0].contains(&peers_events[2].events[1]));
        assert!(graph.round_index[1].contains(&peers_events[1].events[2]));
    }

    #[test]
    fn test_determine_witness() {
        let (graph, peers_events) = build_graph2(()).unwrap();
        
        assert!(!graph.determine_witness(peers_events[2].events[1].clone()));
        assert!(graph.determine_witness(peers_events[1].events[2].clone()));
        assert!(graph.determine_witness(peers_events[0].events[2].clone()));
    }

    #[test]
    fn test_is_famous() {
        let (graph, peers_events) = build_graph2(()).unwrap();
        
        assert!(graph.is_famous(peers_events[0].events[0].clone()));
        assert!(!graph.is_famous(peers_events[0].events[1].clone()));
    }
}
