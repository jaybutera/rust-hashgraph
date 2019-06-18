use serde::Serialize;
use crypto::sha3::Sha3;
use crypto::digest::Digest;
use std::collections::{HashMap,HashSet};
use wasm_bindgen::prelude::*;

use super::event::Event::{self,*};
use super::{Transaction, RoundNum};

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
    latest_event: String,
    #[serde(skip)]
    round_of: HashMap<String, RoundNum>, // Just testing a caching system for now
}

#[wasm_bindgen]
impl Graph {
    pub fn new(peer_id: String) -> Self {
        let genesis = Event::Genesis { creator: peer_id.clone() };

        let mut g = Graph {
            events: HashMap::new(),
            peer_id: peer_id,
            round_index: vec![HashSet::new()],
            creators: HashSet::new(),
            is_famous: HashMap::new(),
            latest_event: genesis.hash(), // Gets reset in add_event
            round_of: HashMap::new(),
        };

        g.add_event(genesis);
        g
    }

    pub fn add(
        &mut self,
        other_parent: Option<String>, // Events can be reactionary or independent of an "other"
        js_txs: Box<[JsValue]>) -> Option<String>
    {
        //let txs: Vec<Transaction> = Vec::from(js_txs);
        // TODO: This is complicated so ignoring txs for now
        let txs = vec![];
        let event = self.create_event(other_parent, txs);

        let hash = event.hash();
        if self.add_event(event).is_ok() { Some(hash) }
        else { None }
    }

    pub fn get_graph(&self) -> JsValue {
        let self_str = serde_json::to_string(self).expect("Graph should always be serializable");
        wasm_bindgen::JsValue::from(self_str)
    }
}

impl Graph {
    pub fn create_event(
        &self,
        other_parent: Option<String>, // Events can be reactionary or independent of an "other"
        txs: Vec<Transaction>) -> Event
    {
        Event::Update {
            creator: self.peer_id.clone(),
            self_parent: self.events.get( &self.latest_event ).expect("Should always have a self parent from genesis").hash(),
            other_parent: other_parent,
            txs: txs,
        }
    }

    pub fn add_event(&mut self, event: Event) -> Result<(),()> {
        // Only accept an event with a valid self_parent and other_parent
        // TODO: Make this prettier
        if let Update{ ref self_parent, ref other_parent, .. } = event {
            if self.events.get(self_parent).is_none() {
                return Err(())
            }
            if let Some(ref op) = other_parent {
                if self.events.get(op).is_none() {
                    return Err(())
                }
            }
        }

        let event_hash = event.hash();
        self.events.insert(event_hash.clone(), event);

        { // Add event to creators list if it isn't already there
            let event = self.events.get(&event_hash).unwrap();
            match event {
                Genesis{ creator, .. } => self.creators.insert(creator.clone()),
                Update { creator, .. } => {
                    // Set this event as latest received
                    if *creator == self.peer_id {
                        self.latest_event = event_hash.clone();
                    }
                    self.creators.insert(creator.clone())
                },
            };
        }

        //-- Set event's round
        let last_idx = self.round_index.len()-1;
        let r = self.determine_round(&event_hash);
        // Cache result
        self.round_of.insert(event_hash.clone(), r);

        if r > last_idx {
            // Create a new round
            let mut hs = HashSet::new();
            hs.insert(event_hash.clone());
            self.round_index.push(hs);
        }
        else {
            // Otherwise push onto current round
            self.round_index[last_idx].insert(event_hash.clone());
        }
        //--

        // Set witness status
        if self.determine_witness(&event_hash) {
            self.is_famous.insert(event_hash, false);
        }

        Ok(())
    }

    pub fn iter(&self, event_hash: &String) -> EventIter {
        let event = self.events.get(event_hash).unwrap();
        let mut e = EventIter { node_list: vec![], events: &self.events };

        match *event {
            Update { ref self_parent, .. } => e.push_self_parents(event_hash),
            _ => (),
        }

        e
    }

    /// Determine the round an event belongs to, which is the max of its parents' rounds +1 if it
    /// is a witness.
    pub fn determine_round(&self, event_hash: &String) -> RoundNum {
        let event = self.events.get(event_hash).unwrap();
        match event {
            Event::Genesis{ .. } => 0,
            Event::Update{ self_parent, other_parent, .. } => {
                // Check if it is cached
                if let Some(r) = self.round_of.get(event_hash) {
                    return *r
                } else {
                let r = match other_parent {
                    Some(op) => std::cmp::max(
                        self.determine_round(self_parent),
                        self.determine_round(op),
                    ),
                    None => self.determine_round(self_parent),
                };

                // Get events from round r
                let round = self.round_index[r].iter()
                    .filter(|eh| *eh != event_hash)
                    .map(|e_hash| self.events.get(e_hash).unwrap())
                    .collect::<Vec<_>>();

                // Find out how many witnesses by unique members the event can strongly see
                let witnesses_strongly_seen = round.iter()
                    .filter(|e| if let Some(_) = self.is_famous.get(&e.hash()) { true } else { false })
                    .fold(HashSet::new(), |mut set, e| {
                        if self.strongly_see(event_hash, &e.hash()) {
                            let creator = match *e {
                                Update{ ref creator, .. } => creator,
                                Genesis{ ref creator } => creator,
                            };
                            set.insert(creator.clone());
                        }
                        set
                    });

                // n is number of members in hashgraph
                let n = self.creators.len();

                if witnesses_strongly_seen.len() > (2*n/3) { r+1 } else { r }
                }
            },
        }
    }

    fn round_of(&self, event_hash: &String) -> RoundNum {
        match self.round_of.get(event_hash) {
            Some(r) => *r,
            None => self.round_index.iter().enumerate()
                        .find(|(_,round)| round.contains(event_hash))
                        .expect("Failed to find a round for event").0
        }
    }

    /// Determines if the event is a witness
    pub fn determine_witness(&self, event_hash: &String) -> bool {
        match self.events.get(event_hash).unwrap() {
            Genesis{ .. } => true,
            Update{ self_parent, .. } =>
                self.round_of(event_hash) > self.round_of(self_parent)
        }
    }

    /// Determine if the event is famous.
    /// An event is famous if 2/3 future witnesses strongly see it.
    fn is_famous(&self, event_hash: &String) -> bool {
        let event = self.events.get(event_hash).unwrap();

        match event {
            Genesis{ .. } => true,
            Update{ .. } => {
                // Event must be a witness
                if !self.determine_witness(event_hash) {
                    return false
                }

                let witnesses = self.events.values()
                    .filter(|e| if let Some(_) = self.is_famous.get(&e.hash()) { true } else { false })
                    .fold(HashSet::new(), |mut set, e| {
                        if self.strongly_see(&e.hash(), event_hash) {
                            let creator = match *e {
                                Update{ ref creator, .. } => creator,
                                Genesis{ ref creator } => creator,
                            };
                            set.insert(creator.clone());
                        }
                        set
                    });

                let n = self.creators.len();
                witnesses.len() > (2*n/3)
            },
        }
    }

    fn ancestor(&self, x_hash: &String, y_hash: &String) -> bool {
        let x = self.events.get(x_hash).unwrap();
        let y = self.events.get(y_hash).unwrap();

        match self.iter(x_hash).find(|e| e.hash() == *y_hash) {
            Some(_) => true,
            None => false,
        }
    }

    fn strongly_see(&self, x_hash: &String, y_hash: &String) -> bool {
        let mut creators_seen = self.iter(x_hash)
            .filter(|e| self.ancestor(&e.hash(),y_hash))
            .fold(HashSet::new(), |mut set, event| {
                let creator = match *event {
                    Update{ ref creator, .. } => creator,
                    Genesis{ ref creator } => creator,
                };
                set.insert(creator.clone());
                set
            });

        // Add self to seen set incase it wasn't traversed above
        match self.events.get(x_hash).unwrap() {
            Genesis{ .. } => true,
            Update{ .. } => creators_seen.insert(self.peer_id.clone()),
        };

        let n = self.creators.len();
        creators_seen.len() > (2*n/3)
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

            if let Update{ ref self_parent, .. } = *e {
                e = self.events.get(self_parent).unwrap();
            }
            else { break; }
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

        if let Update{ ref other_parent, .. } = *event {
            if let Some(ref e) = *other_parent {
                self.push_self_parents(e);
            }
        }
        Some(event)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use super::*;

    fn generate() -> ((Graph,Graph,Graph), [String;10]) {
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

        let g1_hash = peer1.events.get(&peer1.latest_event).unwrap().hash();
        let g2_hash = peer2.events.get(&peer2.latest_event).unwrap().hash();
        let g3_hash = peer3.events.get(&peer3.latest_event).unwrap().hash();

        // Share genesis events
        peer1.add_event(peer2.events.get(&g2_hash).unwrap().clone());
        peer1.add_event(peer3.events.get(&g3_hash).unwrap().clone());
        peer2.add_event(peer3.events.get(&g3_hash).unwrap().clone());
        peer2.add_event(peer1.events.get(&g1_hash).unwrap().clone());
        peer3.add_event(peer1.events.get(&g1_hash).unwrap().clone());
        peer3.add_event(peer2.events.get(&g2_hash).unwrap().clone());

        // Peer1 receives an update from peer1, and creates an event for it
        let e1 = peer1.create_event(
            Some(g2_hash.clone()),
            peer2.peer_id.clone(),
            vec![]);
        let e1_hash = e1.hash();
        peer1.add_event(e1.clone());
        peer2.add_event(e1.clone());
        peer3.add_event(e1.clone());

        let e2 = peer2.create_event(
            Some(e1_hash.clone()),
            peer3.peer_id.clone(),
            vec![]);
        let e2_hash = e2.hash();
        peer1.add_event(e2.clone());
        peer2.add_event(e2.clone());
        peer3.add_event(e2.clone());

        let e3 = peer3.create_event(
            Some(e2_hash.clone()),
            peer2.peer_id.clone(),
            vec![]);
        let e3_hash = e3.hash();
        peer1.add_event(e3.clone());
        peer2.add_event(e3.clone());
        peer3.add_event(e3.clone());

        let e4 = peer2.create_event(
            Some(e3_hash.clone()),
            peer1.peer_id.clone(),
            vec![]);
        let e4_hash = e4.hash();
        peer1.add_event(e4.clone());
        peer2.add_event(e4.clone());
        peer3.add_event(e4.clone());

        let e5 = peer1.create_event(
            Some(e4_hash.clone()),
            peer3.peer_id.clone(),
            vec![]);
        let e5_hash = e5.hash();
        peer1.add_event(e5.clone());
        peer2.add_event(e5.clone());
        peer3.add_event(e5.clone());

        let e6 = peer3.create_event(
            Some(e5_hash.clone()),
            peer2.peer_id.clone(),
            vec![]);
        let e6_hash = e6.hash();
        peer1.add_event(e6.clone());
        peer2.add_event(e6.clone());
        peer3.add_event(e6.clone());

        let e7 = peer2.create_event(
            Some(e6_hash.clone()),
            peer1.peer_id.clone(), // Outgoing doesn't matter
            vec![]);
        let e7_hash = e7.hash();
        peer1.add_event(e7.clone());
        peer2.add_event(e7.clone());
        peer3.add_event(e7.clone());

        ((peer1, peer2, peer3), [g1_hash, g2_hash, g3_hash, e1_hash, e2_hash, e3_hash, e4_hash, e5_hash, e6_hash, e7_hash])
    }

    #[test]
    fn test_ancestor() {
        let (graph, event_hashes) = generate();

        assert_eq!(
            true,
            graph.0.ancestor(
                &event_hashes[3],
                &event_hashes[0]))
    }

    #[test]
    fn test_strongly_see() {
        let (graph, event_hashes) = generate();

        assert_eq!(
            false,
            graph.0.strongly_see(
                &event_hashes[4],
                &event_hashes[0]));
        assert_eq!(
            true,
            graph.0.strongly_see(
                &event_hashes[6],
                &event_hashes[0]));
    }

    #[test]
    fn test_determine_round() {
        let (graph, event_hashes) = generate();

        assert_eq!(
            true,
            graph.0.round_index[0].contains(&event_hashes[5])
            );
        assert_eq!(
            true,
            graph.0.round_index[1].contains(&event_hashes[6])
            );
    }

    #[test]
    fn test_determine_witness() {
        let (graph, event_hashes) = generate();

        assert_eq!(
            false,
            graph.0.determine_witness(&event_hashes[5]));
        assert_eq!(
            true,
            graph.0.determine_witness(&event_hashes[6]));
        assert_eq!(
            true,
            graph.0.determine_witness(&event_hashes[7]));
    }

    #[test]
    fn test_is_famous() {
        let (graph, event_hashes) = generate();

        assert_eq!(
            true,
            graph.0.is_famous(&event_hashes[0]));
        assert_eq!(
            false,
            graph.0.is_famous(&event_hashes[3]));
    }
}
