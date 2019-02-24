use serde::Serialize;
use crypto::sha3::Sha3;
use crypto::digest::Digest;
use std::collections::HashMap;
use self::Event::*;

pub type RoundNum = usize;
/*
pub type EventGraph = HashMap<String,Event>;

pub struct Context {
    pub events: EventGraph,
    pub num_nodes: usize,
}
*/

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
        witness: bool,
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
            } else { break; }
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
        println!("{}",event.hash());

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
    /*
    pub fn determine_round(&self,
                           events: &EventGraph,
                           event_rounds: &HashMap<String,roundNum>) -> roundNum {
        match self {
            Event::Genesis{ .. } => 1,
            Event::Update{creator,self_parent,other_parent,txs,witness} => {
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

    /// true if x can reach y by following 0 or more parent edges.
    /// Read "x is an ancestor of y"
    fn ancestor(x: &Event, y: &Event, events: &EventGraph) -> bool {
        if x.hash() == y.hash() { true }
        else {
            if let Event::Update{creator,self_parent,other_parent,txs,witness} = x {
                if Event::ancestor(events.get(self_parent).unwrap(), y, &events)
                   || Event::ancestor(events.get(other_parent).unwrap(), y, &events)
                { true } else { false }
            } else { false }
        }
    }

    /// true if y is an ancestor of x, but no fork of y is anancestor of x
    fn see(x: &Event, y: &Event, events: &EventGraph) -> bool {
        // no two events that are made by the same creator as y - the ancestor of x - and are also
        // ancestors of x, but not self ancestors of each other
        Event::ancestor(x,y,events)
    }

    /// true if x can see events by more than 2n/3 creators, each of which sees y
    fn strongly_see(x: &Event, y: &Event, context: &Context) -> bool {
        Event::strongly_see_aux(x,y,context,&mut HashMap::new())
    }
    fn strongly_see_aux(x: &Event, y: &Event, context: &Context, creators_seen: &mut HashMap<String,bool>) -> bool {
        if let Event::Update{creator,self_parent,other_parent,..} = x {
        if x.hash() != y.hash()
        {
            creators_seen.insert(x.hash(), true);
            Event::strongly_see_aux(context.events.get(self_parent).expect("failed to get sp"), y, &context, creators_seen);
            Event::strongly_see_aux(context.events.get(other_parent).expect("failed to get op"), y, &context, creators_seen);

            if creators_seen.len() >= (2*context.num_nodes/3) { true }
            else { false }
        } else { false }
        } else { false }
    }
    */

impl Graph {
    pub fn iter(&self, event: &Event) -> EventIter {
        //let event = self.events.get(event_hash).unwrap();
        let mut e = EventIter { node_list: vec![], events: &self.events };
        match *event {
            Update { ref self_parent, .. } => e.push_self_parents(self_parent),
            _ => (),
        }
        e
    }

    fn ancestor(&self, x_hash: &String, y_hash: &String) -> bool {
        let x = self.events.get(x_hash).unwrap();
        let y = self.events.get(y_hash).unwrap();
        println!("y hash: {}",y_hash);

        match self.iter(x).find(|e| e.hash() == *y_hash) {
            Some(_) => true,
            None => false,
    }
}
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use super::*;

    fn generate() -> (Graph, [String;5]) {//([String; 5], EventGraph, HashMap<String,roundNum>) {
        let c1 = "a".to_string();
        let c2 = "b".to_string();
        let c3 = "c".to_string();
        let genesis = Event::Genesis{ creator:c3.clone() };
        let genesis1 = Event::Genesis{ creator:c2.clone() };

        let e1 = Event::Update {
            creator: c1,
            self_parent: genesis.hash(),
            other_parent: genesis1.hash(),
            txs: vec![],
            witness: false,
        };
        let e2 = Event::Update {
            creator: c2,
            self_parent: genesis.hash(),
            other_parent: e1.hash(),
            txs: vec![],
            witness: false,
        };
        let e3 = Event::Update {
            creator: c3,
            self_parent: genesis.hash(),
            other_parent: e1.hash(),
            txs: vec![],
            witness: false,
        };

        let mut events = HashMap::new();
        //let mut event_rounds: HashMap<String,RoundNum> = HashMap::new();

        let g_hash = genesis.hash();
        //event_rounds.insert(genesis.hash(), 1);
        events.insert(genesis.hash(), genesis);

        let g1_hash = genesis1.hash();
        //event_rounds.insert(genesis1.hash(), 1);
        events.insert(genesis1.hash(), genesis1);

        let e1_hash = e1.hash();
        //event_rounds.insert(e1.hash(), e1.determine_round(&events,&event_rounds));
        events.insert(e1.hash(), e1);

        let e2_hash = e2.hash();
        //event_rounds.insert(e2.hash(), e2.determine_round(&events,&event_rounds));
        events.insert(e2.hash(), e2);

        let e3_hash = e3.hash();
        //event_rounds.insert(e3.hash(), e3.determine_round(&events,&event_rounds));
        events.insert(e3.hash(), e3);

        (Graph { events }, [g_hash, g1_hash, e1_hash, e2_hash, e3_hash])
        //([g_hash, g1_hash, e1_hash, e2_hash, e3_hash], events, event_rounds)
    }

    #[test]
    fn test_ancestor() {
        //let ([genesis, genesis1, e1, e2, e3], events, event_rounds) = generate();
        let (graph, event_hashes) = generate();

        assert_eq!(
            true,
            graph.ancestor(
                &event_hashes[2],
                &event_hashes[0]))
    }

    /*
    #[test]
    fn test_strongly_see() {
        let ([genesis, genesis1, e1, e2, e3], events, event_rounds) = generate();
        let context = Context {
            events: events,
            num_nodes: 3,
        };
        /*
        println!("{}",
            Event::strongly_see(
                context.events.get(&e3).unwrap(),
                context.events.get(&genesis).unwrap(),
                &context)
            );
        */

        assert_eq!(
            true,
            Event::strongly_see(
                context.events.get(&e2).unwrap(),
                context.events.get(&genesis).unwrap(),
                &context)
            );
    }
*/
}
