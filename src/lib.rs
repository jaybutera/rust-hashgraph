use serde::Serialize;
use crypto::sha3::Sha3;
use crypto::digest::Digest;
use std::collections::{HashMap,HashSet};
use self::Event::*;

pub type RoundNum = usize;

#[derive(Serialize)]
pub struct Transaction;

pub struct Graph {
    pub events: HashMap<String, Event>,
    creators: HashSet<String>,
    round_index: Vec<HashSet<String>>,
    is_famous: HashMap<String, bool>, // Some(false) means unfamous witness
}

#[derive(Serialize)]
pub enum Event {
    Update {
        creator: String, // TODO: Change to a signature
        self_parent: String,
        other_parent: String,
        txs: Vec<Transaction>,
        to: String, // TODO: Temporary info just for simulation of multiple machines in one program
    },
    Genesis{creator: String},
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
            self.push_self_parents(other_parent);
        }
        Some(event)
    }
}


impl Event {
    pub fn hash(&self) -> String {
        let mut hasher = Sha3::sha3_256();
        let serialized = serde_json::to_string(self).unwrap();
        hasher.input_str(&serialized[..]);
        hasher.result_str()
    }
}

impl Graph {
    pub fn new() -> Self {
        Graph {
            events: HashMap::new(),
            round_index: vec![HashSet::new()],
            creators: HashSet::new(),
            is_famous: HashMap::new(),
        }
    }

    pub fn add_event(&mut self, event: Event) {
        let event_hash = event.hash();
        self.events.insert(event_hash.clone(), event);

        {
            let event = self.events.get(&event_hash).unwrap();

            // Add creator if not already
            match event {
                Genesis{ creator, .. } => {
                    self.is_famous.insert(event_hash.clone(), false);
                    self.creators.insert(creator.clone())
                },
                Update { creator, .. } => {
                    // Assign if witness
                    //*is_witness = witness_status;
                    let round_num = self.round_index.len()-1;
                    if self.determine_witness(&event_hash, round_num) {
                        self.is_famous.insert(event_hash.clone(), false);
                    }
                    self.creators.insert(creator.clone())
                },
            };
        }

        // Push onto events map
        let last_idx = self.round_index.len()-1;
        let r = self.determine_round(&event_hash);

        if r > last_idx {
            // Create a new round
            let mut hs = HashSet::new();
            hs.insert(event_hash);
            self.round_index.push(hs);
        }
        else {
            // Otherwise push onto current round
            self.round_index[last_idx].insert(event_hash);
        }
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

    /// Assumes witness status has been determined for the event in question
    pub fn determine_round(&self, event_hash: &String) -> RoundNum {
        let event = self.events.get(event_hash).unwrap();
        match event {
            Event::Genesis{ .. } => 0,
            Event::Update{ self_parent, other_parent, .. } => {
                let r = std::cmp::max(
                    self.determine_round(self_parent),
                    self.determine_round(other_parent),
                );

                if let Some(_) = self.is_famous.get(event_hash) { r+1 } else { r }
            },
        }
    }

    /// Determines if an event is a witness of the latest round
    pub fn determine_witness(&self, event_hash: &String, round_num: RoundNum) -> bool {
        let event = self.events.get(event_hash).unwrap();
        match *event { // TODO: Ugly temporary for use of "to"
            Genesis{ .. } => true,
            Update{ ref to, .. } => {

        //let round = self.round_index[self.round_index.len()-1].iter()
        let round = self.round_index[round_num].iter()
            .filter(|eh| *eh != event_hash)
            .map(|e_hash| self.events.get(e_hash).unwrap())
            .collect::<Vec<_>>();
        println!("How far");

        // First check if member has any events in this round
        if round.iter()
            .filter(|e| {
                let creator = match *e {
                    Update{ ref creator, .. } => creator,
                    Genesis{ ref creator } => creator,
                };
                creator == to
            }).collect::<Vec<_>>().is_empty() {
                return false
        }
        println!("Do I get");

        // Then find out how many witnesses by unique members strongly see the event
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
        println!("n:{}",witnesses_strongly_seen.len());

        witnesses_strongly_seen.len() > (2*n/3)
        }}
    }

    fn is_famous(&self, event_hash: &String) -> bool {
        let event = self.events.get(event_hash).unwrap();

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

        // TODO: This uses "to" as temporary, ultimately just use the member id
        match self.events.get(x_hash).unwrap() {
            Genesis{ .. } => true,
            Update{ to, .. } => creators_seen.insert(to.clone()),
        };

        let n = self.creators.len();
        creators_seen.len() > (2*n/3)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use super::*;

    fn generate() -> (Graph, [String;9]) {
        let c1 = "a".to_string();
        let c2 = "b".to_string();
        let c3 = "c".to_string();
        let genesis1 = Event::Genesis{ creator:c1.clone() };
        let genesis2 = Event::Genesis{ creator:c2.clone() };
        let genesis3 = Event::Genesis{ creator:c3.clone() };

        let e1 = Event::Update {
            creator: c2.clone(),
            self_parent: genesis1.hash(),
            other_parent: genesis2.hash(),
            txs: vec![],
            to: c1.clone()
        };
        let e2 = Event::Update {
            creator: c1.clone(), // TODO: Need to also consider the owner of a tx (whoever is running the program)
            self_parent: genesis2.hash(),
            other_parent: e1.hash(),
            txs: vec![],
            to: c2.clone()
        };
        let e3 = Event::Update {
            creator: c2.clone(),
            self_parent: genesis3.hash(),
            other_parent: e2.hash(),
            txs: vec![],
            to: c3.clone()
        };
        let e4 = Event::Update {
            creator: c3.clone(),
            self_parent: e2.hash(),
            other_parent: e3.hash(),
            txs: vec![],
            to: c2.clone()
        };
        let e5 = Event::Update {
            creator: c2.clone(),
            self_parent: e1.hash(),
            other_parent: e4.hash(),
            txs: vec![],
            to: c1.clone()
        };
        let e6 = Event::Update {
            creator: c1.clone(),
            self_parent: e3.hash(),
            other_parent: e5.hash(),
            txs: vec![],
            to: c3.clone()
        };

        let mut graph = Graph::new();

        let g1_hash = genesis1.hash();
        graph.add_event(genesis1);

        let g2_hash = genesis2.hash();
        graph.add_event(genesis2);

        let g3_hash = genesis3.hash();
        graph.add_event(genesis3);

        let e1_hash = e1.hash();
        graph.add_event(e1);

        let e2_hash = e2.hash();
        graph.add_event(e2);

        let e3_hash = e3.hash();
        graph.add_event(e3);

        let e4_hash = e4.hash();
        graph.add_event(e4);

        let e5_hash = e5.hash();
        graph.add_event(e5);

        let e6_hash = e6.hash();
        graph.add_event(e6);

        (graph, [g1_hash, g2_hash, g3_hash, e1_hash, e2_hash, e3_hash, e4_hash, e5_hash, e6_hash])
    }

    #[test]
    fn test_ancestor() {
        let (graph, event_hashes) = generate();

        assert_eq!(
            true,
            graph.ancestor(
                &event_hashes[3],
                &event_hashes[0]))
    }

    #[test]
    fn test_strongly_see() {
        let (graph, event_hashes) = generate();

        assert_eq!(
            true,
            graph.strongly_see(
                &event_hashes[6],
                &event_hashes[0]))
    }

    #[test]
    fn test_determine_round() {
        let (graph, event_hashes) = generate();

        for eh in &event_hashes {
            assert_eq!(
                true,
                graph.round_index[graph.determine_round(eh)].contains(eh));
        }
    }

    #[test]
    fn test_determine_witness() {
        let (graph, event_hashes) = generate();

        assert_eq!(
            false,
            graph.determine_witness(&event_hashes[6],0));
        assert_eq!(
            true,
            graph.determine_witness(&event_hashes[7],0));
        assert_eq!(
            true,
            graph.determine_witness(&event_hashes[8],0));
    }

    #[test]
    fn test_is_famous() {
        let (graph, event_hashes) = generate();

        println!("{}",event_hashes[5]);
        println!("{}",graph.determine_witness(&event_hashes[5],0));
        println!("all witnesses");
        for eh in &graph.is_famous {
            let r = graph.round_index[1].contains(eh.0);
            println!("{} in round {}",eh.0,r);
        }
        assert_eq!(
            true,
            graph.is_famous(&event_hashes[0]))
    }
}
