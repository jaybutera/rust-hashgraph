mod lib;

use crypto::sha3::Sha3;
use crypto::digest::Digest;
use serde_json;
use lib::{Event,roundNum};
use std::collections::HashMap;

fn main () {
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

    /*
    let mut hasher = Sha3::sha3_256();
    let serialized = serde_json::to_string(&genesis).unwrap();
    hasher.input_str(&serialized[..]);
    */
    println!("{}", genesis.hash());

    let mut events: HashMap<String,Event> = HashMap::new();
    let mut event_rounds: HashMap<String,roundNum> = HashMap::new();

    event_rounds.insert(genesis.hash(), 1);
    events.insert(genesis.hash(), genesis);

    event_rounds.insert(genesis1.hash(), 1);
    events.insert(genesis1.hash(), genesis1);

    event_rounds.insert(e1.hash(), e1.determine_round(&events,&event_rounds));
    events.insert(e1.hash(), e1);

    //event_rounds.insert(e1.hash(), e2.determine_round(&events,&event_rounds));
    //events.insert(e1.hash(), e2);

    println!("{}", e2.determine_round(&events,&event_rounds));
}
