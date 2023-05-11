use std::{hash::Hash, iter::repeat};

use crate::algorithm::MockSigner;

use super::*;

pub type MockPeerId = u64;

#[derive(Clone)]
pub struct PeerEvents<TPeerId> {
    pub id: TPeerId,
    pub events: Vec<event::Hash>,
}

// Graph, Events by each peer, Test event names (for easier reading), Graph name
pub struct TestSetup<TPayload, TPeerId> {
    /// Graph state
    pub graph: Graph<TPayload, TPeerId, MockSigner<TPeerId>, IncrementalClock>,
    /// For getting hashes for events
    pub peers_events: HashMap<String, PeerEvents<TPeerId>>,
    /// For lookup of readable event name
    pub names: HashMap<event::Hash, String>,
    pub setup_name: String,
}

/// See [`add_events_with_timestamps`]
fn add_events<TPayload, TPeerId, TIter>(
    graph: &mut Graph<TPayload, TPeerId, MockSigner<TPeerId>, IncrementalClock>,
    events: &[(&'static str, &'static str, &'static str)],
    author_ids: HashMap<&'static str, TPeerId>,
    payload: &mut TIter,
) -> Result<
    (
        HashMap<String, PeerEvents<TPeerId>>,
        HashMap<event::Hash, String>, // hash -> event_name
    ),
    String,
>
where
    TPayload: Serialize + Copy + Default + Eq + Hash + Debug,
    TPeerId: Serialize + Eq + std::hash::Hash + Debug + Clone,
    TIter: Iterator<Item = TPayload>,
{
    let timestamps = events.iter().map(|&(name, _, _)| (name, 0)).collect();
    add_events_with_timestamps(graph, events, author_ids, payload, timestamps)
}

/// ## Description
/// Add multiple events in the graph (for easier test case creation and
/// concise and more intuitive writing).
///
/// ## Arguments
/// ### `events`
/// is list of tuples (`event_name`, `creator`, `other_parent_name`)
///
/// ### `creator`
/// is either
/// * `"GENESIS_<peer_name>"` for genesis event of the peer (intended for fork testing)
/// * `event_name` of a previous event (intended for fork testing)
/// * name of peer for **its latest event** (in `author_ids`, to insert at the end of the graph)
///
/// first match is chosen
///
/// ### `other_parent_name`
/// is either
/// * `event_name` of a previous event
/// * name of peer for **its genesis**
///
/// first match is chosen
fn add_events_with_timestamps<T, TPeerId, TIter>(
    graph: &mut Graph<T, TPeerId, MockSigner<TPeerId>, IncrementalClock>,
    events: &[(&'static str, &'static str, &'static str)],
    author_ids: HashMap<&'static str, TPeerId>,
    payload: &mut TIter,
    timestamps: HashMap<&'static str, Timestamp>,
) -> Result<
    (
        HashMap<String, PeerEvents<TPeerId>>,
        HashMap<event::Hash, String>, // hash -> event_name
    ),
    String,
>
where
    T: Serialize + Copy + Default + Eq + Hash + Debug,
    TPeerId: Serialize + Eq + std::hash::Hash + Debug + Clone,
    TIter: Iterator<Item = T>,
{
    let mut inserted_events = HashMap::with_capacity(events.len());
    for (peer_name, peer_id) in &author_ids {
        let key = format!("GENESIS_{}", peer_name).to_owned();
        inserted_events.insert(
            key,
            graph
                .peer_genesis(&peer_id)
                .expect(&format!("Incorrect peer id: {:?}", peer_id))
                .clone(),
        );
    }
    let mut peers_events: HashMap<String, PeerEvents<TPeerId>> = author_ids
        .keys()
        .map(|&name| {
            let id = author_ids
                .get(name)
                .expect(&format!("Unknown author name '{}'", name))
                .clone();
            let genesis = graph
                .peer_genesis(&id)
                .expect(&format!("Unknown author id '{:?}' (name {})", id, name));
            (
                name.to_owned(),
                PeerEvents {
                    id,
                    events: vec![genesis.clone()],
                },
            )
        })
        .collect();

    for &(event_name, self_parent_str, other_parent_event) in events {
        let other_parent_event_hash = match author_ids.get(other_parent_event) {
            Some(h) => graph.peer_genesis(h).expect(&format!(
                "Unknown peer id {:?} to graph (name '{}')",
                h, self_parent_str
            )),
            None => inserted_events
                .get(other_parent_event)
                .expect(&format!("Unknown `other_parent` '{}'", other_parent_event)),
        };
        let genesis_prefix = "GENESIS_";
        let (self_parent_event_hash, author_id, author) = match (
            self_parent_str.starts_with(genesis_prefix),
            inserted_events.get(self_parent_str),
            author_ids.get(self_parent_str),
        ) {
            // `self_parent_str` has form `GENESIS_{author_name}`; genesis is self_parent.
            (true, _, _) => {
                let author_name = self_parent_str.trim_start_matches(genesis_prefix);
                let author_id = author_ids
                    .get(author_name)
                    .expect(&format!("Unknown author name '{}'", author_name));
                let self_parent = graph.peer_genesis(author_id).expect(&format!(
                    "Unknown author id of '{}': {:?}",
                    author_name, author_id
                ));
                let author_name = self_parent_str
                    .trim_start_matches(genesis_prefix)
                    .to_owned();
                (self_parent, author_id, author_name)
            }
            // `self_parent_str` is a name of previously inserted event; it is the self parent.
            (false, Some(self_parent_hash), _) => {
                let self_parent_event = graph
                    .event(self_parent_hash)
                    .expect("Just inserted the event");
                let author_name = author_ids
                    .iter()
                    .find(|(_name, id)| id == &self_parent_event.author())
                    .expect("Just inserted the event, should be tracked")
                    .0
                    .deref()
                    .to_owned();
                (self_parent_hash, self_parent_event.author(), author_name)
            }
            // `self_parent_str` is a name of peer; latest event of the peer is self parent.
            (false, None, Some(self_parent_author_id)) => (
                graph
                    .peer_latest_event(self_parent_author_id)
                    .expect(&format!("Unknown event author {}", self_parent_str)),
                self_parent_author_id,
                self_parent_str.to_owned(),
            ),
            (false, None, None) => panic!("Could not recognize creator '{}'", self_parent_str),
        };
        let parents = Parents {
            self_parent: self_parent_event_hash.clone(),
            other_parent: other_parent_event_hash.clone(),
        };
        let new_event = SignedEvent::new_fakely_signed(
            payload.next().expect("Iterator finished"),
            event::Kind::Regular(parents),
            author_id.clone(),
            *timestamps
                .get(event_name)
                .expect(&format!("No timestamp for event {}", event_name)),
        )
        .expect("Failed to create event");
        let new_event_hash = new_event.hash().clone();
        let (unsigned, signature) = new_event.into_parts();
        graph
            .push_event(unsigned, signature)
            .map_err(|e| format!("Failer to push event {}: {:?}", event_name, e))?;
        peers_events
            .get_mut(&author)
            .expect(&format!("Author '{}' should be in the index", author))
            .events
            .push(new_event_hash.clone());
        let clashed_event = inserted_events.insert(event_name.to_owned(), new_event_hash);
        if clashed_event.is_some() {
            panic!("Event name clash '{}'", event_name)
        }
    }
    let names = inserted_events.into_iter().map(|(a, b)| (b, a)).collect();
    Ok((peers_events, names))
}

fn add_geneses<TPayload, TPeerId>(
    graph: &mut Graph<TPayload, TPeerId, MockSigner<TPeerId>, IncrementalClock>,
    this_author: &str,
    author_ids: &HashMap<&'static str, TPeerId>,
    payload: TPayload,
) -> Result<HashMap<event::Hash, String>, PushError<TPeerId>>
where
    TPayload: Serialize + Copy + Eq + Hash + Debug,
    TPeerId: Serialize + Eq + std::hash::Hash + Debug + Clone,
{
    let mut names = HashMap::with_capacity(author_ids.len());

    for (&name, id) in author_ids {
        let hash = if name == this_author {
            graph
                .peer_genesis(id)
                .expect("Mush have own genesis")
                .clone()
        } else {
            // Geneses should not have timestamp 0 (?), but why not do it for testing other components
            let next_genesis =
                SignedEvent::new_fakely_signed(payload, event::Kind::Genesis, id.clone(), 0)
                    .expect("Failed to create event");
            let gen_id = next_genesis.hash().clone();
            let (unsigned, signature) = next_genesis.into_parts();
            graph.push_event(unsigned, signature)?;
            gen_id
        };
        names.insert(hash, name.to_owned());
    }
    Ok(names)
}

pub fn build_graph_from_paper<T>(
    payload: T,
    coin_frequency: usize,
) -> Result<TestSetup<T, MockPeerId>, String>
where
    T: Serialize + Copy + Default + Eq + Hash + Debug,
{
    let author_ids = HashMap::from([("a", 0), ("b", 1), ("c", 2), ("d", 3), ("e", 4)]);
    let mut graph = Graph::new(
        *author_ids.get("a").unwrap(),
        payload,
        coin_frequency,
        MockSigner::new(),
        IncrementalClock::new(),
    );
    let mut names =
        add_geneses(&mut graph, "a", &author_ids, payload).map_err(|e| format!("{}", e))?;
    let events = [
        //  (name, peer, other_parent)
        ("c2", "c", "d"),
        ("e2", "e", "b"),
        ("b2", "b", "c2"),
        ("c3", "c", "e2"),
        ("d2", "d", "c3"),
        ("a2", "a", "b2"),
        ("b3", "b", "c3"),
        ("c4", "c", "d2"),
        ("a3", "a", "b3"),
        ("c5", "c", "e2"), // ???
        ("c6", "c", "a3"),
    ];
    // let timestamps = events.iter().zip(0..).map(|(a, b)| (a.0, b)).collect();
    let (peers_events, new_names) =
        add_events(&mut graph, &events, author_ids, &mut repeat(payload))?;
    names.extend(new_names);
    Ok(TestSetup {
        graph,
        peers_events,
        names,
        setup_name: "Whitepaper example".to_owned(),
    })
}

pub fn build_graph_some_chain<T>(
    payload: T,
    coin_frequency: usize,
) -> Result<TestSetup<T, MockPeerId>, String>
where
    T: Serialize + Copy + Default + Eq + Hash + Debug,
{
    /* Generates the following graph for each member (c1,c2,c3)
     *
        |  o__|  -- e7
        |__|__o  -- e6
        o__|  |  -- e5 ~ new round
        |  o__|  -- e4
        |  |__o  -- e3
        |__o  |  -- e2
        o__|  |  -- e1
        o  o  o  -- (g1,g2,g3)
    */
    let author_ids = HashMap::from([("g1", 0), ("g2", 1), ("g3", 2)]);
    let mut graph = Graph::new(
        *author_ids.get("g1").unwrap(),
        payload,
        coin_frequency,
        MockSigner::new(),
        IncrementalClock::new(),
    );
    let mut names =
        add_geneses(&mut graph, "g1", &author_ids, payload).map_err(|e| format!("{}", e))?;
    let events = [
        //  (name, peer, other_parent)
        ("e1", "g1", "g2"),
        ("e2", "g2", "e1"),
        ("e3", "g3", "e2"),
        ("e4", "g2", "e3"),
        ("e5", "g1", "e4"),
        ("e6", "g3", "e5"),
        ("e7", "g2", "e6"),
    ];
    let (peers_events, new_names) =
        add_events(&mut graph, &events, author_ids, &mut repeat(payload))?;
    names.extend(new_names);
    Ok(TestSetup {
        graph,
        peers_events,
        names,
        setup_name: "Chain events".to_owned(),
    })
}

pub fn build_graph_detailed_example<T>(
    payload: T,
    coin_frequency: usize,
) -> Result<TestSetup<T, MockPeerId>, String>
where
    T: Serialize + Copy + Default + Eq + Hash + Debug,
{
    build_graph_detailed_example_with_timestamps(payload, coin_frequency, repeat(0))
}

pub fn build_graph_detailed_example_with_timestamps<T, TIter>(
    payload: T,
    coin_frequency: usize,
    mut timestamp_generator: TIter,
) -> Result<TestSetup<T, MockPeerId>, String>
where
    T: Serialize + Copy + Default + Eq + Hash + Debug,
    TIter: Iterator<Item = Timestamp>,
{
    // Defines graph from paper HASHGRAPH CONSENSUS: DETAILED EXAMPLES
    // https://www.swirlds.com/downloads/SWIRLDS-TR-2016-02.pdf
    // also in resources/graph_example.png

    let author_ids = HashMap::from([("a", 0), ("b", 1), ("c", 2), ("d", 3)]);
    let mut graph = Graph::new(
        *author_ids.get("a").unwrap(),
        payload,
        coin_frequency,
        MockSigner::new(),
        IncrementalClock::new(),
    );
    let mut names =
        add_geneses(&mut graph, "a", &author_ids, payload).map_err(|e| format!("{}", e))?;
    // resources/graph_example.png for reference
    let events = [
        //  (name,  peer, other_parent)
        // round 1
        ("d1_1", "d", "b"),
        ("b1_1", "b", "d1_1"),
        ("d1_2", "d", "b1_1"),
        ("b1_2", "b", "c"),
        ("a1_1", "a", "b1_1"),
        ("d1_3", "d", "b1_2"),
        ("c1_1", "c", "b1_2"),
        ("b1_3", "b", "d1_3"),
        // round 2
        ("d2", "d", "a1_1"),
        ("a2", "a", "d2"),
        ("b2", "b", "d2"),
        ("a2_1", "a", "c1_1"),
        ("c2", "c", "a2_1"),
        ("d2_1", "d", "b2"),
        ("a2_2", "a", "b2"),
        ("d2_2", "d", "a2_2"),
        ("b2_1", "b", "a2_2"),
        // round 3
        ("b3", "b", "d2_2"),
        ("a3", "a", "b3"),
        ("d3", "d", "b3"),
        ("d3_1", "d", "c2"),
        ("c3", "c", "d3_1"),
        ("b3_1", "b", "a3"),
        ("b3_2", "b", "a3"),
        ("a3_1", "a", "b3_2"),
        ("b3_3", "b", "d3_1"),
        ("a3_2", "a", "b3_3"),
        ("b3_4", "b", "a3_2"),
        ("d3_2", "d", "b3_3"),
        // round 4
        ("d4", "d", "c3"),
        ("b4", "b", "d4"),
    ];
    let timestamps = events
        .iter()
        .map(|&(event, _, _)| {
            (
                event,
                timestamp_generator.next().expect("No timestamps left"),
            )
        })
        .collect();
    let (peers_events, new_names) = add_events_with_timestamps(
        &mut graph,
        &events,
        author_ids,
        &mut repeat(payload),
        timestamps,
    )?;
    names.extend(new_names);
    Ok(TestSetup {
        graph,
        peers_events,
        names,
        setup_name: "Detailed examples tech report".to_owned(),
    })
}

pub fn build_graph_fork<T, TIter>(
    mut payload: TIter,
    coin_frequency: usize,
) -> Result<TestSetup<T, MockPeerId>, String>
where
    T: Serialize + Copy + Default + Eq + Hash + Debug,
    TIter: Iterator<Item = T>,
{
    // Graph to test fork handling
    // In peers_events the "_forked" event goes before non-fork (they're simmetric, so we refer
    // to the names)
    let author_ids = HashMap::from([("a", 0), ("m", 1)]);
    let mut graph = Graph::new(
        *author_ids.get("a").unwrap(),
        payload.next().expect("Iterator finished"),
        coin_frequency,
        MockSigner::new(),
        IncrementalClock::new(),
    );
    let mut names = add_geneses(
        &mut graph,
        "a",
        &author_ids,
        payload.next().expect("Iterator finished"),
    )
    .map_err(|e| format!("{}", e))?;
    let events = [
        //  (name,  peer, other_parent)
        // round 1
        ("a1_1", "a", "m"),
        // round 2
        ("m2", "GENESIS_m", "a1_1"),
        ("m2_fork", "GENESIS_m", "a1_1"),
        ("m2_1", "m2_fork", "m2"),
        ("a2", "a", "m2_1"),
        // round 3
        ("m3", "m", "a2"),
        ("a3", "a", "m3"),
        // round 4
        ("m4", "m", "a3"),
        ("a4", "a", "m4"),
    ];
    let (peers_events, new_names) = add_events(&mut graph, &events, author_ids, &mut payload)?;
    names.extend(new_names);
    Ok(TestSetup {
        graph,
        peers_events,
        names,
        setup_name: "Fork graph".to_owned(),
    })
}

pub fn build_graph_index_test<T>(
    payload: T,
    coin_frequency: usize,
) -> Result<TestSetup<T, MockPeerId>, String>
where
    T: Serialize + Copy + Default + Eq + Hash + Debug,
{
    // Graph to test round_index assignment. It seems that the logic is broken slightly,
    // this should fail with existing impl.
    let author_ids = HashMap::from([("a", 0), ("b", 1), ("c", 2), ("d", 3)]);
    let mut graph = Graph::new(
        *author_ids.get("b").unwrap(),
        payload,
        coin_frequency,
        MockSigner::new(),
        IncrementalClock::new(),
    );
    let mut names =
        add_geneses(&mut graph, "b", &author_ids, payload).map_err(|e| format!("{}", e))?;
    // resources/graph_example.png for reference
    let events = [
        //  (name,  peer, other_parent)
        // round 0
        ("d1", "d", "c"),
        ("c1", "c", "b"),
        ("b1", "b", "d1"),
        ("d2", "d", "c1"),
        ("d3", "d", "b1"),
        ("d4", "d", "c1"),
        // round 1
        ("c2", "c", "d4"),
        // round 0 again, this order should be the problem
        ("a1", "a", "b"),
        ("a2", "a", "b1"),
    ];
    let (peers_events, new_names) =
        add_events(&mut graph, &events, author_ids, &mut repeat(payload))?;
    names.extend(new_names);
    Ok(TestSetup {
        graph,
        peers_events,
        names,
        setup_name: "`round_index` test".to_owned(),
    })
}
