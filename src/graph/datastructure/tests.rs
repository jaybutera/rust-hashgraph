use std::{iter::repeat, ops::Deref};

use super::*;

/// `run_tests!(name, tested_function, name_lookup, peer_literal, cases)`
/// # Description
/// Test a property of a graph according to test cases.
///
/// The macro requires the name of property (for logging in case of failure),
/// function to test, and function for human readable name lookup (details
/// see in [`test_cases`]).
///
/// Also you should specify identifier for hashmap of events by each peer
/// that can be used in cases declaration. See examples for more clarity
///
/// # Usage
///
/// Upon calling the macro, first specify property name, function to test, and
/// lookup. Then list test cases for each graph.
///
/// ## Examples
///
/// Some kind of template to use
/// ```no_run
/// run_tests!(
///     tested_function_name => "fame",
///     tested_function => |g, event| g.kek(&event),
///     name_lookup => |names, event| names.get(event).unwrap().to_owned(),
///     peers_literal => peers,
///     tests => [
///         (
///             setup => build_graph(),
///             test_case => (
///                 expect: false,
///                 arguments: vec![
///                     &peers.get("g1").unwrap().events[2],
///                     &peers.get("g1").unwrap().events[1],
///                 ]
///             ),
///             test_case => (
///                 expect: true,
///                 arguments: [
///                     &peers.get("g1").unwrap().events[0..3],
///                     &peers.get("g2").unwrap().events[0..2],
///                 ].concat(),
///             )
///         ),
///     ]
/// );
/// ```
///
/// example of tests used for graph
/// ```no_run
/// run_tests!(
///     tested_function_name => "fame",
///     tested_function => |g, event| g.is_famous_witness(&event),
///     name_lookup => |event, names| names.get(event).unwrap().to_owned(),
///     peers_literal => peers,
///     tests => [
///         (
///             setup => build_graph_some_chain((), 999).unwrap(),
///             test_case => (
///                 expect: Ok(WitnessFamousness::Undecided),
///                 arguments: vec![
///                     peers.get("g1").unwrap().events[0].clone(),
///                     peers.get("g1").unwrap().events[2].clone(),
///                     peers.get("g2").unwrap().events[0].clone(),
///                     peers.get("g2").unwrap().events[3].clone(),
///                     peers.get("g3").unwrap().events[0].clone(),
///                     peers.get("g3").unwrap().events[2].clone(),
///                 ],
///             ),
///             test_case => (
///                 expect: Err(NotWitness),
///                 arguments: [
///                     &peers.get("g1").unwrap().events[1..2],
///                     &peers.get("g2").unwrap().events[1..3],
///                     &peers.get("g3").unwrap().events[1..2],
///                 ]
///                 .concat(),
///             ),
///         )
///     ]
/// );
/// ```
macro_rules! run_tests {
        (
            tested_function_name => $property_name:expr,
            tested_function => $tested_function:expr,
            name_lookup => $name_lookup:expr,
            peers_literal => $peers_literal:ident,
            tests => [
                $((
                    setup => $setup:expr,
                    $(test_case => (
                        expect: $expect:expr,
                        arguments: $arguments:expr $(,)?
                    )),* $(,)?
                )),* $(,)?
            ]
        ) => {
            let mut cases = vec![];
            $(
                let TestSetup { peers_events: $peers_literal, .. } = $setup;
                let graph_cases = vec![
                    $((
                        $expect,
                        $arguments
                    )),*
                ];
                cases.push(Test { setup: ($setup), results_args: graph_cases });
            )*
            test_cases(cases, $property_name, $tested_function, $name_lookup);
        };
    }

struct PeerEvents {
    id: PeerId,
    events: Vec<event::Hash>,
}

// Graph, Events by each peer, Test event names (for easier reading), Graph name
struct TestSetup<T> {
    /// Graph state
    graph: Graph<T>,
    /// For getting hashes for events
    peers_events: HashMap<String, PeerEvents>,
    /// For lookup of readable event name
    names: HashMap<event::Hash, String>,
    setup_name: String,
}

struct Test<TPayload, TResult, TArg> {
    setup: TestSetup<TPayload>,
    results_args: Vec<(TResult, Vec<TArg>)>,
}

/// # Description
/// Run tests on multiple cases, compare the results, and report if needed.
///
/// # Arguments
/// * `cases`: List of test cases. Each list entry consists of graph (with
/// helper data structures, see [`TestGraph`](TestGraph<T>)) and test cases
/// for the graph. The graph cases are grouped by result expected. For each
/// result there is a list of arguments to be supplied to `tested_function`.
///
/// * `tested_function_name`: name of the function, used for assert messages.
///
/// * `tested_function`: function to test, takes 2 arguments: graph itself and
/// argument specified in each test case.
///
/// * `name_lookup`: function for obtaining event name based on corresponding
/// `HashMap` and argument of the test case, used for better readable assert messages.
///
/// # Example
/// Suppose we want to check correctness of round calculation:
/// ```no_run
///
/// let mut cases = vec![];
/// let (graph, peers, names, graph_name) = build_graph_some_chain((), 999).unwrap();
/// let graph_cases = vec![
///     (
///         0,
///         [
///             &peers.get("g1").unwrap().events[0..2],
///             &peers.get("g2").unwrap().events[0..3],
///             &peers.get("g3").unwrap().events[0..2],
///         ]
///         .concat(),
///     ),
///     (
///         1,
///         [
///             &peers.get("g1").unwrap().events[2..3],
///             &peers.get("g2").unwrap().events[3..4],
///             &peers.get("g3").unwrap().events[2..3],
///         ]
///         .concat(),
///     ),
/// ];
/// cases.push(((graph, peers, names, graph_name), graph_cases));
/// test_cases(
///     cases,
///     "round",
///     |g, args| g.round_of(&args),
///     |event, names| names.get(event).unwrap().to_owned(),
/// );
/// ```
fn test_cases<TPayload, TArg, TResult, F, FNameLookup>(
    cases: Vec<Test<TPayload, TResult, TArg>>,
    tested_function_name: &str,
    tested_function: F,
    name_lookup: FNameLookup,
) where
    F: Fn(&Graph<TPayload>, &TArg) -> TResult,
    TResult: PartialEq + std::fmt::Debug,
    FNameLookup: Fn(&HashMap<event::Hash, String>, &TArg) -> String,
{
    for Test {
        setup,
        results_args: graph_cases,
    } in cases
    {
        let TestSetup {
            graph,
            peers_events: _,
            names,
            setup_name,
        } = setup;
        for (expected_result, result_cases) in graph_cases {
            for case in result_cases {
                let result = tested_function(&graph, &case);
                assert_eq!(
                    result,
                    expected_result,
                    "Event(-s) '{}' of graph '{}' expected '{}' to be {:?}, but got {:?}",
                    name_lookup(&names, &case),
                    setup_name,
                    tested_function_name,
                    expected_result,
                    result
                );
            }
        }
    }
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
fn add_events<T, TIter>(
    graph: &mut Graph<T>,
    events: &[(&'static str, &'static str, &'static str)],
    author_ids: HashMap<&'static str, PeerId>,
    payload: &mut TIter,
) -> Result<
    (
        HashMap<String, PeerEvents>,
        HashMap<event::Hash, String>, // hash -> event_name
    ),
    PushError,
>
where
    T: Serialize + Copy + Default,
    TIter: Iterator<Item = T>,
{
    let mut inserted_events = HashMap::with_capacity(events.len());
    let mut peers_events: HashMap<String, PeerEvents> = author_ids
        .keys()
        .map(|&name| {
            let id = *author_ids
                .get(name)
                .expect(&format!("Unknown author name '{}'", name));
            let genesis = graph.peer_genesis(&id).expect(&format!(
                "Unknown author id to graph '{}' (name {})",
                id, name
            ));
            (
                name.to_owned(),
                PeerEvents {
                    id,
                    events: vec![genesis.clone()],
                },
            )
        })
        .collect();

    for &(event_name, creator, other_parent_event) in events {
        let other_parent_event_hash = match author_ids.get(other_parent_event) {
            Some(h) => graph.peer_genesis(h).expect(&format!(
                "Unknown peer id {} to graph (name '{}')",
                h, creator
            )),
            None => inserted_events
                .get(other_parent_event)
                .expect(&format!("Unknown `other_parent` '{}'", other_parent_event)),
        };
        let genesis_prefix = "GENESIS_";
        let (self_parent_event_hash, author_id, author) = match (
            creator.starts_with(genesis_prefix),
            inserted_events.get(creator),
            author_ids.get(creator),
        ) {
            (true, _, _) => {
                let author_name = creator.trim_start_matches(genesis_prefix);
                let author_id = author_ids
                    .get(author_name)
                    .expect(&format!("Unknown author name '{}'", author_name));
                let self_parent = graph.peer_genesis(author_id).expect(&format!(
                    "Unknown author id of '{}': {}",
                    author_name, author_id
                ));
                let author = creator.trim_start_matches(genesis_prefix).to_owned();
                (self_parent, author_id, author)
            }
            (false, Some(event), _) => {
                let event = graph.event(event).expect("Just inserted the event");
                let author = author_ids
                    .iter()
                    .find(|(_name, id)| id == &event.author())
                    .expect("Just inserted the event, should be tracked")
                    .0
                    .deref()
                    .to_owned();
                (event.hash(), event.author(), author)
            }
            (false, None, Some(author_id)) => (
                graph
                    .peer_latest_event(author_id)
                    .expect(&format!("Unknown event author {}", creator)),
                author_id,
                creator.to_owned(),
            ),
            (false, None, None) => panic!("Could not recognize creator '{}'", creator),
        };
        let parents = Parents {
            self_parent: self_parent_event_hash.clone(),
            other_parent: other_parent_event_hash.clone(),
        };
        let new_event_hash = graph.push_node(
            payload.next().expect("Iterator finished"),
            PushKind::Regular(parents),
            *author_id,
        )?;
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

fn add_geneses<T: Serialize + Copy>(
    graph: &mut Graph<T>,
    this_author: &str,
    author_ids: &HashMap<&'static str, PeerId>,
    payload: T,
) -> Result<HashMap<event::Hash, String>, PushError> {
    let mut names = HashMap::with_capacity(author_ids.len());

    for (&name, id) in author_ids {
        let hash = if name == this_author {
            graph
                .peer_genesis(id)
                .expect("Mush have own genesis")
                .clone()
        } else {
            graph.push_node(payload, PushKind::Genesis, *id)?
        };
        names.insert(hash, name.to_owned());
    }
    Ok(names)
}

fn build_graph_from_paper<T: Serialize + Copy + Default>(
    payload: T,
    coin_frequency: usize,
) -> Result<TestSetup<T>, PushError> {
    let author_ids = HashMap::from([("a", 0), ("b", 1), ("c", 2), ("d", 3), ("e", 4)]);
    let mut graph = Graph::new(*author_ids.get("a").unwrap(), payload, coin_frequency);
    let mut names = add_geneses(&mut graph, "a", &author_ids, payload)?;
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
        ("c5", "c", "e2"),
        ("c6", "c", "a3"),
    ];
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

fn build_graph_some_chain<T: Serialize + Copy + Default>(
    payload: T,
    coin_frequency: usize,
) -> Result<TestSetup<T>, PushError> {
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
    let mut graph = Graph::new(*author_ids.get("g1").unwrap(), payload, coin_frequency);
    let mut names = add_geneses(&mut graph, "g1", &author_ids, payload)?;
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

fn build_graph_detailed_example<T: Serialize + Copy + Default>(
    payload: T,
    coin_frequency: usize,
) -> Result<TestSetup<T>, PushError> {
    // Defines graph from paper HASHGRAPH CONSENSUS: DETAILED EXAMPLES
    // https://www.swirlds.com/downloads/SWIRLDS-TR-2016-02.pdf
    // also in resources/graph_example.png

    let author_ids = HashMap::from([("a", 0), ("b", 1), ("c", 2), ("d", 3)]);
    let mut graph = Graph::new(*author_ids.get("a").unwrap(), payload, coin_frequency);
    let mut names = add_geneses(&mut graph, "a", &author_ids, payload)?;
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
    let (peers_events, new_names) =
        add_events(&mut graph, &events, author_ids, &mut repeat(payload))?;
    names.extend(new_names);
    Ok(TestSetup {
        graph,
        peers_events,
        names,
        setup_name: "Detailed examples tech report".to_owned(),
    })
}

fn build_graph_fork<T, TIter>(
    mut payload: TIter,
    coin_frequency: usize,
) -> Result<TestSetup<T>, PushError>
where
    T: Serialize + Copy + Default,
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
    );
    let mut names = add_geneses(
        &mut graph,
        "a",
        &author_ids,
        payload.next().expect("Iterator finished"),
    )?;
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
        setup_name: "Detailed examples tech report".to_owned(),
    })
}

fn build_graph_index_test<T: Serialize + Copy + Default>(
    payload: T,
    coin_frequency: usize,
) -> Result<TestSetup<T>, PushError> {
    // Graph to test round_index assignment. It seems that the logic is broken slightly,
    // this should fail with existing impl.
    let author_ids = HashMap::from([("a", 0), ("b", 1), ("c", 2), ("d", 3)]);
    let mut graph = Graph::new(*author_ids.get("b").unwrap(), payload, coin_frequency);
    let mut names = add_geneses(&mut graph, "b", &author_ids, payload)?;
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

// Test simple work + errors

#[test]
fn graph_builds() {
    build_graph_from_paper((), 999).unwrap();
    build_graph_some_chain((), 999).unwrap();
    build_graph_detailed_example((), 999).unwrap();
    // To make hashes of forks different
    build_graph_fork([42, 1337, 80085].into_iter().cycle(), 999).unwrap();
}

#[test]
fn duplicate_push_fails() {
    let TestSetup {
        mut graph,
        peers_events: peers,
        names: _,
        setup_name: _,
    } = build_graph_from_paper((), 999).unwrap();
    let a_id = peers.get("a").unwrap().id;
    assert!(matches!(
        graph.push_node((), PushKind::Genesis, a_id),
        Err(PushError::NodeAlreadyExists(hash)) if &hash == graph.peer_genesis(&a_id).unwrap()
    ));
}

#[test]
fn double_genesis_fails() {
    let TestSetup {
        mut graph,
        peers_events: peers,
        names: _,
        setup_name: _,
    } = build_graph_from_paper(0, 999).unwrap();
    assert!(matches!(
        graph.push_node(1, PushKind::Genesis, peers.get("a").unwrap().id),
        Err(PushError::GenesisAlreadyExists)
    ))
}

#[test]
fn missing_parent_fails() {
    let TestSetup {
        mut graph,
        peers_events: peers,
        names: _,
        setup_name: _,
    } = build_graph_from_paper((), 999).unwrap();
    let fake_node = Event::new((), event::Kind::Genesis, 1232423).unwrap();
    let legit_node_hash = graph.peer_latest_event(&0).unwrap().clone();

    let fake_parents_1 = Parents {
        self_parent: fake_node.hash().clone(),
        other_parent: legit_node_hash.clone(),
    };
    assert!(matches!(
        graph.push_node((), PushKind::Regular(fake_parents_1), peers.get("a").unwrap().id),
        Err(PushError::NoParent(fake_hash)) if &fake_hash == fake_node.hash()
    ));

    let fake_parents_2 = Parents {
        self_parent: legit_node_hash.clone(),
        other_parent: fake_node.hash().clone(),
    };
    assert!(matches!(
        graph.push_node((), PushKind::Regular(fake_parents_2), peers.get("a").unwrap().id),
        Err(PushError::NoParent(fake_hash)) if &fake_hash == fake_node.hash()
    ));
}

// Test graph properties

#[test]
fn test_ancestor() {
    run_tests!(
        tested_function_name => "ancestor",
        tested_function => |g, (e1, e2)| g.ancestor(&e1, &e2),
        name_lookup => |names, (e1, e2)| format!("({}, {})", names.get(e1).unwrap(), names.get(e2).unwrap()),
        peers_literal => peers,
        tests => [
            (
                setup => build_graph_some_chain((), 999).unwrap(),
                test_case => (
                    expect: true,
                    arguments: vec![(
                        &peers.get("g1").unwrap().events[1],
                        &peers.get("g1").unwrap().events[0]
                    )]
                ),
            ),
            (
                setup => build_graph_from_paper((), 999).unwrap(),
                test_case => (
                    expect: true,
                    arguments: vec![
                        (
                            &peers.get("c").unwrap().events[5],
                            &peers.get("b").unwrap().events[0],
                        ),
                        (
                            &peers.get("a").unwrap().events[2],
                            &peers.get("e").unwrap().events[1],
                        )
                    ]
                ),
            ),
            (
                setup => build_graph_detailed_example((), 999).unwrap(),
                test_case => (
                    expect: false,
                    arguments: vec![
                        (
                            &peers.get("c").unwrap().events[0],
                            &peers.get("c").unwrap().events[1],
                        ),
                        (
                            &peers.get("c").unwrap().events[0],
                            &peers.get("c").unwrap().events[3],
                        ),
                        (
                            &peers.get("c").unwrap().events[0],
                            &peers.get("b").unwrap().events[2],
                        ),
                        (
                            &peers.get("c").unwrap().events[1],
                            &peers.get("d").unwrap().events[3],
                        ),
                        (
                            &peers.get("a").unwrap().events[2],
                            &peers.get("c").unwrap().events[1],
                        ),
                    ]
                ),
                test_case => (
                    expect: true,
                    arguments: vec![
                        (
                            // Self parent
                            &peers.get("d").unwrap().events[1],
                            &peers.get("d").unwrap().events[0],
                        ),
                        (
                            // Self ancestor
                            &peers.get("d").unwrap().events[4],
                            &peers.get("d").unwrap().events[0],
                        ),
                        (
                            // Ancestry is reflective
                            &peers.get("c").unwrap().events[1],
                            &peers.get("c").unwrap().events[1],
                        ),
                        (
                            // Other parent
                            &peers.get("b").unwrap().events[3],
                            &peers.get("d").unwrap().events[3],
                        ),
                        (
                            &peers.get("c").unwrap().events[2],
                            &peers.get("a").unwrap().events[2],
                        ),
                        (
                            &peers.get("b").unwrap().events[3],
                            &peers.get("c").unwrap().events[0],
                        ),
                        (
                            &peers.get("d").unwrap().events[3],
                            &peers.get("c").unwrap().events[0],
                        ),
                        (
                            // Debugging b2 not being witness
                            &peers.get("d").unwrap().events[6],
                            &peers.get("a").unwrap().events[2],
                        ),
                        (
                            // Debugging b2 not being witness
                            &peers.get("b").unwrap().events[6],
                            &peers.get("a").unwrap().events[2],
                        ),
                        (
                            // Debugging b2 not being witness
                            &peers.get("a").unwrap().events[4],
                            &peers.get("a").unwrap().events[2],
                        ),
                    ]
                ),
            ),
        ]
    );
}

#[test]
fn test_ancestor_iter() {
    // Complicated case to use macro
    let TestSetup {
        graph,
        peers_events: peers,
        names,
        setup_name: _,
    } = build_graph_detailed_example((), 999).unwrap();
    // (Iterator, Actual ancestors to compare with)
    let cases = vec![
        (
            graph.iter(&peers.get("b").unwrap().events[3]).unwrap(),
            HashSet::<_>::from_iter(
                [
                    &peers.get("b").unwrap().events[0..4],
                    &peers.get("c").unwrap().events[0..1],
                    &peers.get("d").unwrap().events[0..4],
                ]
                .concat()
                .into_iter(),
            ),
        ),
        (
            // debugging b3 not being witness
            graph.iter(&peers.get("b").unwrap().events[6]).unwrap(),
            HashSet::<_>::from_iter(
                [
                    &peers.get("a").unwrap().events[0..5],
                    &peers.get("b").unwrap().events[0..7],
                    &peers.get("c").unwrap().events[0..2],
                    &peers.get("d").unwrap().events[0..7],
                ]
                .concat()
                .into_iter(),
            ),
        ),
    ];
    for (iter, ancestors) in cases {
        let ancestors_from_iter = HashSet::<_>::from_iter(iter.map(|e| e.hash().clone()));
        assert_eq!(
            ancestors,
            ancestors_from_iter,
            "Iterator did not find ancestors {:?}\n and it went through excess events: {:?}",
            ancestors
                .difference(&ancestors_from_iter)
                .map(|h| names.get(h).unwrap())
                .collect::<Vec<_>>(),
            ancestors_from_iter
                .difference(&ancestors)
                .map(|h| names.get(h).unwrap())
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn test_strongly_see() {
    run_tests!(
        tested_function_name => "strongly_see",
        tested_function => |graph, (e1, e2)| graph.strongly_see(e1, e2),
        name_lookup => |names, (e1, e2)| format!("({}, {})", names.get(e1).unwrap(), names.get(e2).unwrap()),
        peers_literal => peers,
        tests => [
            (
                setup => build_graph_some_chain((), 999).unwrap(),
                test_case => (
                    expect: false,
                    arguments: vec![
                        (
                            &peers.get("g1").unwrap().events[1],
                            &peers.get("g1").unwrap().events[0]
                        )
                    ]
                ),
                test_case => (
                    expect: true,
                    arguments: vec![
                        (
                            &peers.get("g2").unwrap().events[2],
                            &peers.get("g1").unwrap().events[0]
                        ),
                    ]
                )
            ),
            (
                setup => build_graph_from_paper((), 999).unwrap(),
                test_case => (
                    expect: true,
                    arguments: vec![(
                        &peers.get("c").unwrap().events[5],
                        &peers.get("d").unwrap().events[0]
                    ),]
                ),
                test_case => (
                    expect: false,
                    arguments: vec![(
                        &peers.get("c").unwrap().events[4],
                        &peers.get("d").unwrap().events[0]
                    ),]
                )
            ),
            (
                setup => build_graph_detailed_example((), 999).unwrap(),
                test_case => (
                    expect: false,
                    arguments: vec![
                        (
                            &peers.get("d").unwrap().events[0],
                            &peers.get("d").unwrap().events[0],
                        ),
                        (
                            &peers.get("d").unwrap().events[3],
                            &peers.get("d").unwrap().events[0],
                        ),
                        (
                            &peers.get("d").unwrap().events[3],
                            &peers.get("b").unwrap().events[0],
                        ),
                        (
                            &peers.get("b").unwrap().events[2],
                            &peers.get("c").unwrap().events[0],
                        ),
                        (
                            &peers.get("a").unwrap().events[0],
                            &peers.get("b").unwrap().events[0],
                        ),
                        (
                            &peers.get("a").unwrap().events[1],
                            &peers.get("c").unwrap().events[0],
                        ),
                    ],
                ),
                test_case => (
                    expect: true,
                    arguments: vec![
                        (
                            &peers.get("d").unwrap().events[4],
                            &peers.get("d").unwrap().events[0],
                        ),
                        (
                            &peers.get("d").unwrap().events[4],
                            &peers.get("b").unwrap().events[0],
                        ),
                        (
                            &peers.get("b").unwrap().events[3],
                            &peers.get("c").unwrap().events[0],
                        ),
                        (
                            &peers.get("a").unwrap().events[1],
                            &peers.get("b").unwrap().events[0],
                        ),
                        (
                            &peers.get("a").unwrap().events[3],
                            &peers.get("c").unwrap().events[0],
                        ),
                        (
                            // Did not find for round calculation once
                            &peers.get("b").unwrap().events[6],
                            &peers.get("a").unwrap().events[2],
                        ),
                    ],
                ),
            )
        ]
    );
}

#[test]
fn test_determine_round() {
    run_tests!(
        tested_function_name => "round",
        tested_function => |g, args| g.round_of(&args),
        name_lookup => |names, event| names.get(event).unwrap().to_owned(),
        peers_literal => peers,
        tests => [
            (
                setup => build_graph_some_chain((), 999).unwrap(),
                test_case => (
                    expect: 0,
                    arguments: [
                        &peers.get("g1").unwrap().events[0..2],
                        &peers.get("g2").unwrap().events[0..3],
                        &peers.get("g3").unwrap().events[0..2],
                    ]
                    .concat()
                ),
                test_case => (
                    expect: 1,
                    arguments: [
                        &peers.get("g1").unwrap().events[2..3],
                        &peers.get("g2").unwrap().events[3..4],
                        &peers.get("g3").unwrap().events[2..3],
                    ]
                    .concat()
                )
            ),
            (
                setup => build_graph_from_paper((), 999).unwrap(),
                test_case => (
                    expect: 0,
                    arguments: [
                        &peers.get("a").unwrap().events[1..],
                        &peers.get("b").unwrap().events[1..],
                        &peers.get("c").unwrap().events[1..5],
                        &peers.get("d").unwrap().events[1..],
                        &peers.get("e").unwrap().events[1..]
                    ]
                    .concat()
                ),
                test_case => (
                    expect: 1,
                    arguments: [
                        &peers.get("c").unwrap().events[5..],
                    ]
                    .concat()
                )
            ),
            (
                setup => build_graph_detailed_example((), 999).unwrap(),
                test_case => (
                    expect: 0usize,
                    arguments: [
                        &peers.get("a").unwrap().events[0..2],
                        &peers.get("b").unwrap().events[0..4],
                        &peers.get("c").unwrap().events[0..2],
                        &peers.get("d").unwrap().events[0..4],
                    ]
                    .concat()
                ),
                test_case => (
                    expect: 1,
                    arguments: [
                        &peers.get("a").unwrap().events[2..5],
                        &peers.get("b").unwrap().events[4..6],
                        &peers.get("c").unwrap().events[2..3],
                        &peers.get("d").unwrap().events[4..7],
                    ]
                    .concat()
                ),
                test_case => (
                    expect: 2,
                    arguments: [
                        &peers.get("a").unwrap().events[5..8],
                        &peers.get("b").unwrap().events[6..11],
                        &peers.get("c").unwrap().events[3..4],
                        &peers.get("d").unwrap().events[7..10],
                    ]
                    .concat()
                ),
                test_case => (
                    expect: 3,
                    arguments: [
                        &peers.get("b").unwrap().events[11..12],
                        &peers.get("d").unwrap().events[10..11],
                    ]
                    .concat()
                )
            ),
            (
                setup => build_graph_index_test((), 999).unwrap(),
                test_case => (
                    expect: 0usize,
                    arguments: [
                        &peers.get("a").unwrap().events[0..3],
                        &peers.get("b").unwrap().events[0..2],
                        &peers.get("c").unwrap().events[0..2],
                        &peers.get("d").unwrap().events[0..5],
                    ]
                    .concat()
                ),
                test_case => (
                    expect: 1,
                    arguments: [
                        &peers.get("c").unwrap().events[2..3],
                    ]
                    .concat()
                ),
            ),
        ]
    );
}

#[test]
fn test_round_indices_consistent() {
    // Uses internal state, yes. Want to make sure it's consistent to avoid future problems.
    fn round_index_consistent<T>(graph: &Graph<T>, hash: &event::Hash) -> Result<usize, String> {
        let round_of_num = graph.round_of(hash);
        let round_index = graph
            .round_index
            .get(round_of_num)
            .ok_or(format!("No round {} index found", round_of_num))?;
        if round_index.contains(hash) {
            Ok(round_of_num)
        } else {
            Err(format!(
                "round_index of round {} does not have the round",
                round_of_num
            ))
        }
    }
    run_tests!(
        tested_function_name => "round_index_consistency",
        tested_function => |g, args| round_index_consistent(g, &args),
        name_lookup => |names, event| names.get(event).unwrap().to_owned(),
        peers_literal => peers,
        tests => [
            (
                setup => build_graph_some_chain((), 999).unwrap(),
                test_case => (
                    expect: Ok(0),
                    arguments: [
                        &peers.get("g1").unwrap().events[0..2],
                        &peers.get("g2").unwrap().events[0..3],
                        &peers.get("g3").unwrap().events[0..2],
                    ]
                    .concat()
                ),
                test_case => (
                    expect: Ok(1),
                    arguments: [
                        &peers.get("g1").unwrap().events[2..3],
                        &peers.get("g2").unwrap().events[3..4],
                        &peers.get("g3").unwrap().events[2..3],
                    ]
                    .concat()
                )
            ),
            (
                setup => build_graph_from_paper((), 999).unwrap(),
                test_case => (
                    expect: Ok(0),
                    arguments: [
                        &peers.get("a").unwrap().events[1..],
                        &peers.get("b").unwrap().events[1..],
                        &peers.get("c").unwrap().events[1..5],
                        &peers.get("d").unwrap().events[1..],
                        &peers.get("e").unwrap().events[1..]
                    ]
                    .concat()
                ),
                test_case => (
                    expect: Ok(1),
                    arguments: [
                        &peers.get("c").unwrap().events[5..],
                    ]
                    .concat()
                )
            ),
            (
                setup => build_graph_detailed_example((), 999).unwrap(),
                test_case => (
                    expect: Ok(0usize),
                    arguments: [
                        &peers.get("a").unwrap().events[0..2],
                        &peers.get("b").unwrap().events[0..4],
                        &peers.get("c").unwrap().events[0..2],
                        &peers.get("d").unwrap().events[0..4],
                    ]
                    .concat()
                ),
                test_case => (
                    expect: Ok(1),
                    arguments: [
                        &peers.get("a").unwrap().events[2..5],
                        &peers.get("b").unwrap().events[4..6],
                        &peers.get("c").unwrap().events[2..3],
                        &peers.get("d").unwrap().events[4..7],
                    ]
                    .concat()
                ),
                test_case => (
                    expect: Ok(2),
                    arguments: [
                        &peers.get("a").unwrap().events[5..8],
                        &peers.get("b").unwrap().events[6..11],
                        &peers.get("c").unwrap().events[3..4],
                        &peers.get("d").unwrap().events[7..10],
                    ]
                    .concat()
                ),
                test_case => (
                    expect: Ok(3),
                    arguments: [
                        &peers.get("b").unwrap().events[11..12],
                        &peers.get("d").unwrap().events[10..11],
                    ]
                    .concat()
                )
            ),
            ( // YESSS IT FAILS!1.1.1.1..
                setup => build_graph_index_test((), 999).unwrap(),
                test_case => (
                    expect: Ok(0usize),
                    arguments: [
                        &peers.get("a").unwrap().events[0..3],
                        &peers.get("b").unwrap().events[0..2],
                        &peers.get("c").unwrap().events[0..2],
                        &peers.get("d").unwrap().events[0..5],
                    ]
                    .concat()
                ),
                test_case => (
                    expect: Ok(1),
                    arguments: [
                        &peers.get("c").unwrap().events[2..3],
                    ]
                    .concat()
                ),
            ),
        ]
    );
}

#[test]
fn test_determine_witness() {
    run_tests!(
        tested_function_name => "determine_witness",
        tested_function => |graph, event| graph.determine_witness(&event).expect(&format!("Can't find event {:?}", event)),
        name_lookup => |names, event| names.get(event).unwrap().to_owned(),
        peers_literal => peers,
        tests => [
            (
                setup => build_graph_some_chain((), 999).unwrap(),
                test_case => (
                    expect: false,
                    arguments: vec![
                        &peers.get("g1").unwrap().events[1..2],
                        &peers.get("g2").unwrap().events[1..3],
                        &peers.get("g3").unwrap().events[1..2]
                    ].iter()
                        .flat_map(|s| s.iter().collect::<Vec<&_>>())
                        .collect()
                ),
                test_case => (
                    expect: true,
                    arguments: vec![
                        &peers.get("g1").unwrap().events[0],
                        &peers.get("g2").unwrap().events[0],
                        &peers.get("g3").unwrap().events[0],
                        &peers.get("g1").unwrap().events[2],
                        &peers.get("g2").unwrap().events[3],
                        &peers.get("g3").unwrap().events[2]
                    ],
                )
            ),
            (
                setup => build_graph_from_paper((), 999).unwrap(),
                test_case => (
                    expect: false,
                    arguments: vec![
                        &peers.get("a").unwrap().events[1..],
                        &peers.get("b").unwrap().events[1..],
                        &peers.get("c").unwrap().events[1..5],
                        &peers.get("d").unwrap().events[1..],
                        &peers.get("e").unwrap().events[1..]
                    ].iter()
                        .flat_map(|s| s.iter().collect::<Vec<&_>>())
                        .collect()
                ),
                test_case => (
                    expect: true,
                    arguments: vec![
                        &peers.get("a").unwrap().events[0],
                        &peers.get("b").unwrap().events[0],
                        &peers.get("c").unwrap().events[0],
                        &peers.get("c").unwrap().events[5],
                        &peers.get("d").unwrap().events[0],
                        &peers.get("e").unwrap().events[0]
                    ],
                )
            ),
            (
                setup => build_graph_detailed_example((), 999).unwrap(),
                test_case => (
                    expect: false,
                    arguments: [
                        &peers.get("a").unwrap().events[1..2],
                        &peers.get("a").unwrap().events[3..5],
                        &peers.get("a").unwrap().events[6..8],
                        &peers.get("b").unwrap().events[1..4],
                        &peers.get("b").unwrap().events[5..6],
                        &peers.get("b").unwrap().events[7..11],
                        &peers.get("c").unwrap().events[1..2],
                        &peers.get("d").unwrap().events[1..4],
                        &peers.get("d").unwrap().events[5..7],
                        &peers.get("d").unwrap().events[8..10]
                    ].iter()
                        .flat_map(|s| s.iter().collect::<Vec<&_>>())
                        .collect()
                ),
                test_case => (
                    expect: true,
                    arguments: vec![
                        &peers.get("a").unwrap().events[0],
                        &peers.get("a").unwrap().events[2],
                        &peers.get("a").unwrap().events[5],
                        &peers.get("b").unwrap().events[0],
                        &peers.get("b").unwrap().events[4],
                        &peers.get("b").unwrap().events[6],
                        &peers.get("b").unwrap().events[11],
                        &peers.get("c").unwrap().events[0],
                        &peers.get("c").unwrap().events[2],
                        &peers.get("c").unwrap().events[3],
                        &peers.get("d").unwrap().events[0],
                        &peers.get("d").unwrap().events[4],
                        &peers.get("d").unwrap().events[7],
                        &peers.get("d").unwrap().events[10]
                    ],
                )
            ),
        ]
    );
}

#[test]
fn test_is_famous_witness() {
    run_tests!(
        tested_function_name => "fame",
        tested_function => |g, event| g.is_famous_witness(&event),
        name_lookup => |names, event| names.get(event).unwrap().to_owned(),
        peers_literal => peers,
        tests => [
            (
                setup => build_graph_some_chain((), 999).unwrap(),
                test_case => (
                    expect: Ok(WitnessFamousness::Undecided),
                    arguments: vec![
                        &peers.get("g1").unwrap().events[0],
                        &peers.get("g1").unwrap().events[2],
                        &peers.get("g2").unwrap().events[0],
                        &peers.get("g2").unwrap().events[3],
                        &peers.get("g3").unwrap().events[0],
                        &peers.get("g3").unwrap().events[2],
                    ],
                ),
                test_case => (
                    expect: Err(WitnessCheckError::NotWitness),
                    arguments: [
                        &peers.get("g1").unwrap().events[1..2],
                        &peers.get("g2").unwrap().events[1..3],
                        &peers.get("g3").unwrap().events[1..2],
                    ].iter()
                        .flat_map(|s| s.iter().collect::<Vec<&_>>())
                        .collect(),
                ),
            ),
            (
                setup => build_graph_from_paper((), 999).unwrap(),
                test_case => (
                    expect: Ok(WitnessFamousness::Undecided),
                    arguments: vec![
                        &peers.get("a").unwrap().events[0],
                        &peers.get("b").unwrap().events[0],
                        &peers.get("c").unwrap().events[0],
                        &peers.get("c").unwrap().events[5],
                        &peers.get("d").unwrap().events[0],
                        &peers.get("e").unwrap().events[0]
                    ],
                ),
                test_case => (
                    expect: Err(WitnessCheckError::NotWitness),
                    arguments: vec![
                        &peers.get("a").unwrap().events[1..],
                        &peers.get("b").unwrap().events[1..],
                        &peers.get("c").unwrap().events[1..5],
                        &peers.get("d").unwrap().events[1..],
                        &peers.get("e").unwrap().events[1..]
                    ].iter()
                        .flat_map(|s| s.iter().collect::<Vec<&_>>())
                        .collect(),
                ),
            ),
            (
                setup => build_graph_detailed_example((), 999).unwrap(),
                test_case => (
                    expect: Ok(WitnessFamousness::Yes),
                    arguments: vec![
                        &peers.get("a").unwrap().events[0],
                        &peers.get("a").unwrap().events[2],
                        &peers.get("b").unwrap().events[0],
                        &peers.get("b").unwrap().events[4],
                        &peers.get("c").unwrap().events[0],
                        &peers.get("d").unwrap().events[0],
                        &peers.get("d").unwrap().events[4],
                    ],
                ),
                test_case => (
                    expect: Ok(WitnessFamousness::No),
                    arguments: vec![
                        &peers.get("c").unwrap().events[2],
                    ],
                ),
                test_case => (
                    expect: Ok(WitnessFamousness::Undecided),
                    arguments: vec![
                        &peers.get("a").unwrap().events[5],
                        &peers.get("b").unwrap().events[6],
                        &peers.get("b").unwrap().events[11],
                        &peers.get("c").unwrap().events[3],
                        &peers.get("d").unwrap().events[7],
                        &peers.get("d").unwrap().events[10]
                    ],
                ),
                test_case => (
                    expect: Err(WitnessCheckError::NotWitness),
                    arguments: [
                        &peers.get("a").unwrap().events[1..2],
                        &peers.get("a").unwrap().events[3..5],
                        &peers.get("a").unwrap().events[6..8],
                        &peers.get("b").unwrap().events[1..4],
                        &peers.get("b").unwrap().events[5..6],
                        &peers.get("b").unwrap().events[7..11],
                        &peers.get("c").unwrap().events[1..2],
                        &peers.get("d").unwrap().events[1..4],
                        &peers.get("d").unwrap().events[5..7],
                        &peers.get("d").unwrap().events[8..10]
                    ].iter()
                        .flat_map(|s| s.iter().collect::<Vec<&_>>())
                        .collect(),
                ),
            )
        ]
    );
}

#[test]
fn test_is_unique_famous_witness() {
    run_tests!(
        tested_function_name => "uniqueness + fame",
        tested_function => |g, event| g.is_unique_famous_witness(&event),
        name_lookup => |names, event| names.get(event).unwrap().to_owned(),
        peers_literal => peers,
        tests => [
            (
                setup => build_graph_some_chain(0, 999).unwrap(),
                test_case => (
                    expect: Ok(WitnessUniqueFamousness::Undecided),
                    arguments: vec![
                        &peers.get("g1").unwrap().events[0],
                        &peers.get("g1").unwrap().events[2],
                        &peers.get("g2").unwrap().events[0],
                        &peers.get("g2").unwrap().events[3],
                        &peers.get("g3").unwrap().events[0],
                        &peers.get("g3").unwrap().events[2],
                    ],
                ),
                test_case => (
                    expect: Err(WitnessCheckError::NotWitness),
                    arguments: [
                        &peers.get("g1").unwrap().events[1..2],
                        &peers.get("g2").unwrap().events[1..3],
                        &peers.get("g3").unwrap().events[1..2],
                    ].iter()
                        .flat_map(|s| s.iter().collect::<Vec<&_>>())
                        .collect(),
                ),
            ),
            (
                setup => build_graph_from_paper(0, 999).unwrap(),
                test_case => (
                    expect: Ok(WitnessUniqueFamousness::Undecided),
                    arguments: vec![
                        &peers.get("a").unwrap().events[0],
                        &peers.get("b").unwrap().events[0],
                        &peers.get("c").unwrap().events[0],
                        &peers.get("c").unwrap().events[5],
                        &peers.get("d").unwrap().events[0],
                        &peers.get("e").unwrap().events[0]
                    ],
                ),
                test_case => (
                    expect: Err(WitnessCheckError::NotWitness),
                    arguments: vec![
                        &peers.get("a").unwrap().events[1..],
                        &peers.get("b").unwrap().events[1..],
                        &peers.get("c").unwrap().events[1..5],
                        &peers.get("d").unwrap().events[1..],
                        &peers.get("e").unwrap().events[1..]
                    ].iter()
                        .flat_map(|s| s.iter().collect::<Vec<&_>>())
                        .collect(),
                ),
            ),
            (
                setup => build_graph_detailed_example(0, 999).unwrap(),
                test_case => (
                    expect: Ok(WitnessUniqueFamousness::FamousUnique),
                    arguments: vec![
                        &peers.get("a").unwrap().events[0],
                        &peers.get("a").unwrap().events[2],
                        &peers.get("b").unwrap().events[0],
                        &peers.get("b").unwrap().events[4],
                        &peers.get("c").unwrap().events[0],
                        &peers.get("d").unwrap().events[0],
                        &peers.get("d").unwrap().events[4],
                    ],
                ),
                test_case => (
                    expect: Ok(WitnessUniqueFamousness::NotFamous),
                    arguments: vec![
                        &peers.get("c").unwrap().events[2],
                    ],
                ),
                test_case => (
                    expect: Ok(WitnessUniqueFamousness::Undecided),
                    arguments: vec![
                        &peers.get("a").unwrap().events[5],
                        &peers.get("b").unwrap().events[6],
                        &peers.get("b").unwrap().events[11],
                        &peers.get("c").unwrap().events[3],
                        &peers.get("d").unwrap().events[7],
                        &peers.get("d").unwrap().events[10]
                    ],
                ),
                test_case => (
                    expect: Err(WitnessCheckError::NotWitness),
                    arguments: [
                        &peers.get("a").unwrap().events[1..2],
                        &peers.get("a").unwrap().events[3..5],
                        &peers.get("a").unwrap().events[6..8],
                        &peers.get("b").unwrap().events[1..4],
                        &peers.get("b").unwrap().events[5..6],
                        &peers.get("b").unwrap().events[7..11],
                        &peers.get("c").unwrap().events[1..2],
                        &peers.get("d").unwrap().events[1..4],
                        &peers.get("d").unwrap().events[5..7],
                        &peers.get("d").unwrap().events[8..10]
                    ].iter()
                        .flat_map(|s| s.iter().collect::<Vec<&_>>())
                        .collect(),
                ),
            ),
            (
                setup => build_graph_fork([42, 1337, 80085].into_iter().cycle(), 999).unwrap(),
                test_case => (
                    expect: Ok(WitnessUniqueFamousness::FamousUnique),
                    arguments: vec![
                        &peers.get("a").unwrap().events[0],
                        &peers.get("a").unwrap().events[2],
                        &peers.get("m").unwrap().events[0],
                    ],
                ),
                test_case => (
                    expect: Ok(WitnessUniqueFamousness::FamousNotUnique),
                    arguments: vec![
                        &peers.get("m").unwrap().events[1],
                        &peers.get("m").unwrap().events[2],
                    ],
                ),
                test_case => (
                    expect: Ok(WitnessUniqueFamousness::Undecided),
                    arguments: vec![
                        &peers.get("a").unwrap().events[3],
                        &peers.get("a").unwrap().events[4],
                        &peers.get("m").unwrap().events[4],
                        &peers.get("m").unwrap().events[5],
                    ],
                ),
                test_case => (
                    expect: Err(WitnessCheckError::NotWitness),
                    arguments: vec![
                        &peers.get("a").unwrap().events[1],
                        &peers.get("m").unwrap().events[3]
                    ],
                ),
            ),
        ]
    );
}