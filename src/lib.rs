use serde::Serialize;
use crypto::sha3::Sha3;
use crypto::digest::Digest;
use std::collections::HashMap;

pub type roundNum = usize;

#[derive(Serialize)]
pub struct Transaction;

#[derive(Serialize)]
pub enum Event {
    Update {
        self_parent: String,
        other_parent: String,
        txs: Vec<Transaction>,
        witness: bool,
    },
    Genesis,
}

impl Event {
    pub fn determine_round(&self,
                           events: &HashMap<String,Event>,
                           event_rounds: &HashMap<String,roundNum>) -> roundNum {
        match self {
            Genesis => 1,
            Event::Update{self_parent,other_parent,txs,witness} => {
                let sp_event = events.get(self_parent).unwrap();
                let op_event = events.get(other_parent).unwrap();

                std::cmp::max(
                    sp_event.determine_round(events,event_rounds),
                    op_event.determine_round(events,event_rounds)
                )
            },
        }
    }

    pub fn hash(&self) -> String {
        let mut hasher = Sha3::sha3_256();
        let serialized = serde_json::to_string(self).unwrap();
        hasher.input_str(&serialized[..]);
        hasher.result_str()
    }

    fn ancestor(x: &Event, y: &Event, events: &HashMap<String,Event>) -> bool {
        if x.hash() == y.hash() { true }
        else {
            if let Event::Update{self_parent,other_parent,txs,witness} = x {
                if Event::ancestor(events.get(self_parent).unwrap(), y, &events)
                   || Event::ancestor(events.get(other_parent).unwrap(), y, &events)
                { true } else { false }
            } else { false }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    //use hg_test::{Event,roundNum};
    use super::*;

    fn generate() -> ([String; 4], HashMap<String,Event>, HashMap<String,roundNum>) {
        let genesis = Event::Genesis;
        let genesis1 = Event::Genesis;

        let e1 = Event::Update {
            self_parent: genesis.hash(),
            other_parent: genesis1.hash(),
            txs: vec![],
            witness: false,
        };
        let e2 = Event::Update {
            self_parent: genesis.hash(),
            other_parent: e1.hash(),
            txs: vec![],
            witness: false,
        };

        let mut events: HashMap<String,Event> = HashMap::new();
        let mut event_rounds: HashMap<String,roundNum> = HashMap::new();

        let g_hash = genesis.hash();
        event_rounds.insert(genesis.hash(), 1);
        events.insert(genesis.hash(), genesis);

        let g1_hash = genesis1.hash();
        event_rounds.insert(genesis1.hash(), 1);
        events.insert(genesis1.hash(), genesis1);

        let e1_hash = e1.hash();
        event_rounds.insert(e1.hash(), e1.determine_round(&events,&event_rounds));
        events.insert(e1.hash(), e1);

        let e2_hash = e2.hash();
        event_rounds.insert(e2.hash(), e2.determine_round(&events,&event_rounds));
        events.insert(e2.hash(), e2);

        ([g_hash, g1_hash, e1_hash, e2_hash], events, event_rounds)
    }

    #[test]
    fn test_ancestor() {
        let ([genesis, genesis1, e1, e2], events, event_rounds) = generate();

        assert!(
            true,
            Event::ancestor(
                events.get(&e1).unwrap(),
                events.get(&genesis).unwrap(),
                &events)
            )
    }
}
