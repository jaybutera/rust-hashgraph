use serde::Serialize;
use crypto::sha3::Sha3;
use crypto::digest::Digest;
use std::collections::{HashMap,HashSet};
use self::Event::*;

pub type RoundNum = usize;

#[derive(Serialize)]
pub struct Transaction;

pub struct Graph {
    events: HashMap<String, Event>,
}

#[derive(Serialize)]
pub enum Event {
    Update {
        creator: String,
        self_parent: String,
        other_parent: String,
        txs: Vec<Transaction>,
        is_witness: bool,
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
    pub fn iter(&self, event_hash: &String) -> EventIter {
        let event = self.events.get(event_hash).unwrap();
        let mut e = EventIter { node_list: vec![], events: &self.events };

        match *event {
            Update { ref self_parent, .. } => e.push_self_parents(event_hash),
            _ => (),
        }

        e
    }

    pub fn determine_round(&self, event_hash: &String) -> RoundNum {
        let event = self.events.get(event_hash).unwrap();
        match event {
            Event::Genesis{ .. } => 1,
            Event::Update{ self_parent, other_parent, is_witness, .. } => {
                let r = std::cmp::max(
                    self.determine_round(self_parent),
                    self.determine_round(other_parent),
                );

                if *is_witness { r+1 } else { r }
            },
        }
    }

    /*
    pub fn determine_witness(&self, event_hash: &String) -> {
        let event = self.events.get(event_hash).unwrap();
    }
    */

    fn ancestor(&self, x_hash: &String, y_hash: &String) -> bool {
        let x = self.events.get(x_hash).unwrap();
        let y = self.events.get(y_hash).unwrap();

        match self.iter(x_hash).find(|e| e.hash() == *y_hash) {
            Some(_) => true,
            None => false,
        }
    }

    fn strongly_see(&self, x_hash: &String, y_hash: &String, n: usize) -> bool {
        let creators_seen = self.iter(x_hash)
            .filter(|e| self.ancestor(x_hash,y_hash))
            .fold(HashSet::new(), |mut set, event| {
                let creator = match *event {
                    Update{ ref creator, .. } => creator,
                    Genesis{ ref creator } => creator,
                };
                set.insert(creator.clone());
                set
            });
        creators_seen.len() >= (2*n/3)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use super::*;

    fn generate() -> (Graph, [String;6]) {
        let c1 = "a".to_string();
        let c2 = "b".to_string();
        let c3 = "c".to_string();
        let genesis1 = Event::Genesis{ creator:c1.clone() };
        let genesis2 = Event::Genesis{ creator:c2.clone() };
        let genesis3 = Event::Genesis{ creator:c3.clone() };

        let e1 = Event::Update {
            creator: c1,
            self_parent: genesis1.hash(),
            other_parent: genesis2.hash(),
            txs: vec![],
            is_witness: false,
        };
        let e2 = Event::Update {
            creator: c2,
            self_parent: genesis2.hash(),
            other_parent: e1.hash(),
            txs: vec![],
            is_witness: false,
        };
        let e3 = Event::Update {
            creator: c3,
            self_parent: genesis3.hash(),
            other_parent: e2.hash(),
            txs: vec![],
            is_witness: false,
        };

        let mut events = HashMap::new();

        let g1_hash = genesis1.hash();
        events.insert(genesis1.hash(), genesis1);

        let g2_hash = genesis2.hash();
        events.insert(genesis2.hash(), genesis2);

        let g3_hash = genesis3.hash();
        events.insert(genesis3.hash(), genesis3);

        let e1_hash = e1.hash();
        events.insert(e1.hash(), e1);

        let e2_hash = e2.hash();
        events.insert(e2.hash(), e2);

        let e3_hash = e3.hash();
        events.insert(e3.hash(), e3);

        (Graph { events }, [g1_hash, g2_hash, g3_hash, e1_hash, e2_hash, e3_hash])
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
                &event_hashes[5],
                &event_hashes[0],
                3))
    }
}
