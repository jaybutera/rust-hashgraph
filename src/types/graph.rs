use serde::Serialize;

use std::collections::{HashMap, HashSet};
use wasm_bindgen::prelude::*;

use super::event::Event::{self, *};
use super::{RoundNum, Transaction};

#[wasm_bindgen]
#[derive(Serialize)]
pub struct Graph {
    events: HashMap<String, Event>,
    #[serde(skip)]
    peer_id: String,
    #[serde(skip)]
    creators: HashSet<String>,
    #[serde(skip)]
    round_index: Vec<HashSet<String>>,
    #[serde(skip)]
    is_famous: HashMap<String, bool>, // Some(false) means unfamous witness
    #[serde(skip)]
    latest_event: HashMap<String, String>, // Creator id to event hash
    #[serde(skip)]
    round_of: HashMap<String, RoundNum>, // Just testing a caching system for now
}

#[wasm_bindgen]
impl Graph {
    pub fn new(peer_id: String) -> Self {
        let genesis = Event::Genesis {
            creator: peer_id.clone(),
        };

        let mut g = Graph {
            events: HashMap::new(),
            peer_id,
            round_index: vec![HashSet::new()],
            creators: HashSet::new(),
            is_famous: HashMap::new(),
            latest_event: HashMap::new(), // Gets reset in add_event
            round_of: HashMap::new(),
        };

        g.add_event(genesis)
            .expect("Genesis events should be valid");
        g
    }

    pub fn add(
        &mut self,
        other_parent: Option<String>, // Events can be reactionary or independent of an "other"
        creator: String,
        // _js_txs: Box<[JsValue]>, // what is it for????
    ) -> Option<String> {
        //let txs: Vec<Transaction> = Vec::from(js_txs);
        // TODO: This is complicated so ignoring txs for now
        let txs = vec![];
        let creator_head_event = match self.latest_event.get(&creator) {
            Some(h) => h,
            None => return None,
        };

        let event = Event::Update {
            creator, // TODO: This is specifiable in the wasm api for now because the Event type can't be passed over JS
            self_parent: creator_head_event.clone(),
            other_parent,
            txs,
        };

        let hash = event.hash();
        if self.add_event(event).is_ok() {
            Some(hash)
        } else {
            None
        }
    }

    pub fn add_creator(&mut self, _creator: String) -> Option<String> {
        let event = Genesis { creator: _creator };
        let hash = event.hash();

        if self.add_event(event).is_ok() {
            Some(hash)
        } else {
            None
        }
    }

    pub fn get_graph(&self) -> JsValue {
        let self_str = serde_json::to_string(self).expect("Graph should always be serializable");
        wasm_bindgen::JsValue::from(self_str)
    }

    pub fn get_event(&self, hash: String) -> Option<String> {
        self.events
            .get(&hash)
            .map(|event| serde_json::to_string(event).expect("Event should be serializable"))
    }
}

impl Graph {
    pub fn create_event(
        &self,
        other_parent: Option<String>, // Events can be reactionary or independent of an "other"
        txs: Vec<Transaction>,
    ) -> Event {
        let latest_ev = self
            .latest_event
            .get(&self.peer_id)
            .expect("Peer id should always have an entry in latest_event");
        Event::Update {
            creator: self.peer_id.clone(),
            self_parent: self
                .events
                .get(latest_ev)
                .expect("Should always have a self parent from genesis")
                .hash(),
            other_parent,
            txs,
        }
    }

    pub fn add_event(&mut self, event: Event) -> Result<(), ()> {
        // Only accept an event with a valid self_parent and other_parent
        // TODO: Make this prettier
        if let Update {
            ref self_parent,
            ref other_parent,
            ..
        } = event
        {
            if self.events.get(self_parent).is_none() {
                return Err(());
            }
            if let Some(ref op) = other_parent {
                if self.events.get(op).is_none() {
                    return Err(());
                }
            }
        }

        let event_hash = event.hash();
        self.events.insert(event_hash.clone(), event);

        {
            // Add event to creators list if it isn't already there and update creator's latest event
            let event = self.events.get(&event_hash).unwrap();
            match event {
                Genesis { creator, .. } => {
                    self.latest_event
                        .insert(creator.clone(), event_hash.clone());
                    self.creators.insert(creator.clone())
                }
                Update { creator, .. } => {
                    self.latest_event
                        .insert(creator.clone(), event_hash.clone());
                    self.creators.insert(creator.clone())
                }
            };
        }

        //-- Set event's round
        let last_idx = self.round_index.len() - 1;
        let r = self.determine_round(&event_hash);
        // Cache result
        self.round_of.insert(event_hash.clone(), r);

        if r > last_idx {
            // Create a new round
            let mut hs = HashSet::new();
            hs.insert(event_hash.clone());
            self.round_index.push(hs);
        } else {
            // Otherwise push onto current round
            self.round_index[last_idx].insert(event_hash.clone());
        }
        //--

        // Set witness status
        if self.determine_witness(event_hash.clone()) {
            self.is_famous.insert(event_hash, false);
        }

        Ok(())
    }

    pub fn iter(&self, event_hash: &String) -> EventIter {
        let event = self.events.get(event_hash).unwrap();
        let mut e = EventIter {
            node_list: vec![],
            events: &self.events,
        };

        if let Update { self_parent: _, .. } = event {
            e.push_self_parents(event_hash)
        }
        e
    }

    /// Determine the round an event belongs to, which is the max of its parents' rounds +1 if it
    /// is a witness.
    fn determine_round(&self, event_hash: &String) -> RoundNum {
        let event = self.events.get(event_hash).unwrap();
        match event {
            Event::Genesis { .. } => 0,
            Event::Update {
                self_parent,
                other_parent,
                ..
            } => {
                // Check if it is cached
                if let Some(r) = self.round_of.get(event_hash) {
                    *r
                } else {
                    let r = match other_parent {
                        Some(op) => std::cmp::max(
                            self.determine_round(self_parent),
                            self.determine_round(op),
                        ),
                        None => self.determine_round(self_parent),
                    };

                    // Get events from round r
                    let round = self.round_index[r]
                        .iter()
                        .filter(|eh| *eh != event_hash)
                        .map(|e_hash| self.events.get(e_hash).unwrap())
                        .collect::<Vec<_>>();

                    // Find out how many witnesses by unique members the event can strongly see
                    let witnesses_strongly_seen = round
                        .iter()
                        .filter(|e| self.is_famous.get(&e.hash()).is_some())
                        .fold(HashSet::new(), |mut set, e| {
                            if self.strongly_see(event_hash.clone(), e.hash()) {
                                let creator = match *e {
                                    Update { ref creator, .. } => creator,
                                    Genesis { ref creator } => creator,
                                };
                                set.insert(creator.clone());
                            }
                            set
                        });

                    // n is number of members in hashgraph
                    let n = self.creators.len();

                    if witnesses_strongly_seen.len() > (2 * n / 3) {
                        r + 1
                    } else {
                        r
                    }
                }
            }
        }
    }
}

#[wasm_bindgen]
impl Graph {
    pub fn round_of(&self, event_hash: String) -> RoundNum {
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
    pub fn determine_witness(&self, event_hash: String) -> bool {
        match self.events.get(&event_hash).unwrap() {
            Genesis { .. } => true,
            Update { self_parent, .. } => {
                self.round_of(event_hash) > self.round_of(self_parent.clone())
            }
        }
    }

    /// Determine if the event is famous.
    /// An event is famous if 2/3 future witnesses strongly see it.
    fn is_famous(&self, event_hash: String) -> bool {
        let event = self.events.get(&event_hash).unwrap();

        match event {
            Genesis { .. } => true,
            Update { .. } => {
                // Event must be a witness
                if !self.determine_witness(event_hash.clone()) {
                    return false;
                }

                let witnesses = self
                    .events
                    .values()
                    .filter(|e| self.is_famous.get(&e.hash()).is_some())
                    .fold(HashSet::new(), |mut set, e| {
                        if self.strongly_see(e.hash(), event_hash.clone()) {
                            let creator = match *e {
                                Update { ref creator, .. } => creator,
                                Genesis { ref creator } => creator,
                            };
                            set.insert(creator.clone());
                        }
                        set
                    });

                let n = self.creators.len();
                witnesses.len() > (2 * n / 3)
            }
        }
    }

    fn ancestor(&self, x_hash: String, y_hash: String) -> bool {
        let _x = self.events.get(&x_hash).unwrap();
        let _y = self.events.get(&y_hash).unwrap();

        self.iter(&x_hash).any(|e| e.hash() == y_hash)
    }

    fn strongly_see(&self, x_hash: String, y_hash: String) -> bool {
        let mut creators_seen = self
            .iter(&x_hash)
            .filter(|e| self.ancestor(e.hash(), y_hash.clone()))
            .fold(HashSet::new(), |mut set, event| {
                let creator = match *event {
                    Update { ref creator, .. } => creator,
                    Genesis { ref creator } => creator,
                };
                set.insert(creator.clone());
                set
            });

        // Add self to seen set incase it wasn't traversed above
        match self.events.get(&x_hash).unwrap() {
            Genesis { .. } => true,
            Update { .. } => creators_seen.insert(self.peer_id.clone()),
        };

        let n = self.creators.len();
        creators_seen.len() > (2 * n / 3)
    }
}

pub struct EventIter<'a> {
    node_list: Vec<&'a Event>,
    events: &'a HashMap<String, Event>,
}

impl<'a> EventIter<'a> {
    fn push_self_parents(&mut self, event_hash: &String) {
        let mut e = self.events.get(event_hash).unwrap();

        loop {
            self.node_list.push(e);

            if let Update {
                ref self_parent, ..
            } = *e
            {
                e = self.events.get(self_parent).unwrap();
            } else {
                break;
            }
        }
    }
}

impl<'a> Iterator for EventIter<'a> {
    type Item = &'a Event;

    fn next(&mut self) -> Option<Self::Item> {
        let event = match self.node_list.pop() {
            None => return None,
            Some(e) => e,
        };

        if let Update {
            other_parent: Some(e),
            ..
        } = event
        {
            self.push_self_parents(e);
        }
        Some(event)
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    fn distribute_event<'a, I>(peers: I, event: Event)
    where
        I: IntoIterator<Item = &'a mut Graph>,
    {
        for peer in peers {
            peer.add_event(event.clone()).unwrap();
        }
    }

    fn generate() -> ((Graph, Graph, Graph), [String; 10]) {
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

        let c1 = "a".to_string();
        let c2 = "b".to_string();
        let c3 = "c".to_string();

        let mut peer1 = Graph::new(c1);
        let mut peer2 = Graph::new(c2);
        let mut peer3 = Graph::new(c3);

        let p1_g: &String = peer1.latest_event.get(&peer1.peer_id).unwrap();
        let p2_g: &String = peer2.latest_event.get(&peer2.peer_id).unwrap();
        let p3_g: &String = peer3.latest_event.get(&peer3.peer_id).unwrap();
        let g1_hash = peer1.events.get(p1_g).unwrap().hash();
        let g2_hash = peer2.events.get(p2_g).unwrap().hash();
        let g3_hash = peer3.events.get(p3_g).unwrap().hash();

        // Share genesis events
        peer1
            .add_event(peer2.events.get(&g2_hash).unwrap().clone())
            .unwrap();
        peer1
            .add_event(peer3.events.get(&g3_hash).unwrap().clone())
            .unwrap();
        peer2
            .add_event(peer3.events.get(&g3_hash).unwrap().clone())
            .unwrap();
        peer2
            .add_event(peer1.events.get(&g1_hash).unwrap().clone())
            .unwrap();
        peer3
            .add_event(peer1.events.get(&g1_hash).unwrap().clone())
            .unwrap();
        peer3
            .add_event(peer2.events.get(&g2_hash).unwrap().clone())
            .unwrap();

        let mut peers = vec![peer1, peer2, peer3];

        // Peer1 receives an update from peer2, and creates an event for it
        let e1 = peers[0].create_event(Some(g2_hash.clone()), vec![]);
        let e1_hash = e1.hash();
        distribute_event(&mut peers, e1);

        let e2 = peers[1].create_event(Some(e1_hash.clone()), vec![]);
        let e2_hash = e2.hash();
        distribute_event(&mut peers, e2);

        let e3 = peers[2].create_event(Some(e2_hash.clone()), vec![]);
        let e3_hash = e3.hash();
        distribute_event(&mut peers, e3);

        let e4 = peers[1].create_event(Some(e3_hash.clone()), vec![]);
        let e4_hash = e4.hash();
        distribute_event(&mut peers, e4);

        let e5 = peers[0].create_event(Some(e4_hash.clone()), vec![]);
        let e5_hash = e5.hash();
        distribute_event(&mut peers, e5);

        let e6 = peers[2].create_event(Some(e5_hash.clone()), vec![]);
        let e6_hash = e6.hash();
        distribute_event(&mut peers, e6);

        let e7 = peers[1].create_event(Some(e6_hash.clone()), vec![]);
        let e7_hash = e7.hash();
        distribute_event(&mut peers, e7);

        let mut peers = peers.into_iter();
        let peers = (
            peers.next().unwrap(),
            peers.next().unwrap(),
            peers.next().unwrap(),
        );
        (
            peers,
            [
                g1_hash, g2_hash, g3_hash, e1_hash, e2_hash, e3_hash, e4_hash, e5_hash, e6_hash,
                e7_hash,
            ],
        )
    }

    #[test]
    fn test_ancestor() {
        let (graph, event_hashes) = generate();

        assert!(graph
            .0
            .ancestor(event_hashes[3].clone(), event_hashes[0].clone()));
    }

    #[test]
    fn test_strongly_see() {
        let (graph, event_hashes) = generate();

        assert!(!graph
            .0
            .strongly_see(event_hashes[4].clone(), event_hashes[0].clone()));
        assert!(graph
            .0
            .strongly_see(event_hashes[6].clone(), event_hashes[0].clone()));
    }

    #[test]
    fn test_determine_round() {
        let (graph, event_hashes) = generate();

        assert!(graph.0.round_index[0].contains(&event_hashes[5]));
        assert!(graph.0.round_index[1].contains(&event_hashes[6]));
    }

    #[test]
    fn test_determine_witness() {
        let (graph, event_hashes) = generate();

        assert!(!graph.0.determine_witness(event_hashes[5].clone()));
        assert!(graph.0.determine_witness(event_hashes[6].clone()));
        assert!(graph.0.determine_witness(event_hashes[7].clone()));
    }

    #[test]
    fn test_is_famous() {
        let (graph, event_hashes) = generate();

        assert!(graph.0.is_famous(event_hashes[0].clone()));
        assert!(!graph.0.is_famous(event_hashes[3].clone()));
    }
}
