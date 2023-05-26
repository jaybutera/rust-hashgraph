use std::collections::HashMap;

use crate::algorithm::{datastructure::Graph, event, IncrementalClock, MockSigner};

use super::mocks::TestSetup;

/// Filter for `tracing_subscriber` that passes all messages from the given function.
/// Should be useful for debugging test cases (actually was at least once).
///
/// # Usage
///
/// ```
/// use tracing_subscriber::prelude::*;
///
/// let my_filter = function_only_filter!("ordering_data");
/// tracing_subscriber::registry()
///     .with(tracing_subscriber::fmt::layer())
///     .with(my_filter)
///     .init();
/// ```
#[allow(unused)]
macro_rules! function_only_filter {
    ($function_name:literal) => {
        tracing_subscriber::filter::DynFilterFn::new(|metadata, cx| {
            if metadata.is_span() && metadata.name() == $function_name {
                return true;
            }
            if let Some(current_span) = cx.lookup_current() {
                return current_span.name() == $function_name;
            }
            false
        })
    };
}

/// `run_tests!(tested_function_name, tested_function, name_lookup, peer_literal, cases)`
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
/// Let's look in details:
///
/// ```no_run
/// run_tests!(
///     // Human-readable functionality name, it will only appear in test logs (panics)
///     tested_function_name => "cool property",
///     // Function that will be called on each argument provided (in `arguments`).
///     // Its output is compared to values in `expect`. First argument is always
///     // the graph
///     tested_function => |g, event| g.kek(&event),
///     // Function that finds name of argument given. Only used for more readable logs.
///     name_lookup => |names, event| names.get(event).unwrap().to_owned(),
///     // Literal for referring to index of events by peer in each test.
///     // Used to insert particular event hashes in `arguments`, because
///     // pasting hashes is completely unreadable.
///     peers_literal => peers,
///     // One test for each graph setup
///     tests => [
///         (
///             // Graph that we test here
///             setup => build_graph(),
///             // Test cases, one per expected return value
///             test_case => (
///                 // Value we expect to receive from `tested_funciton`
///                 expect: false,
///                 // Arguments we provide one by one to `tested_funciton`
///                 arguments: vec![
///                     &peers.get("g1").unwrap().events[2],
///                     &peers.get("g1").unwrap().events[1],
///                 ]
///             ),
///             // Another test case
///             test_case => (
///                 // Different expected value
///                 expect: true,
///                 // One can use slices like this not to insert each element separately
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
            let setup = $setup;
            let $peers_literal = setup.peers_events.clone();
            let graph_cases = vec![
                $((
                    $expect,
                    $arguments
                )),*
            ];
            cases.push(Test { setup, results_args: graph_cases });
        )*
        test_cases(cases, $property_name, $tested_function, $name_lookup);
    };
}

pub(crate) use run_tests;

pub(crate) struct Test<TPayload, TGenesisPayload, TPeerId, TResult, TArg> {
    pub setup: TestSetup<TPayload, TGenesisPayload, TPeerId>,
    pub results_args: Vec<(TResult, Vec<TArg>)>,
}

/// # Description
/// Run tests on multiple cases, compare the results, and report if needed.
///
/// # Arguments
/// * `cases`: List of test cases. Each list entry consists of graph (with
/// helper data structures, see [`TestGraph`](TestGraph<T, ()>)) and test cases
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
pub(crate) fn test_cases<TPayload, TGenesisPayload, TPeerId, TArg, TResult, F, FNameLookup>(
    cases: Vec<Test<TPayload, TGenesisPayload, TPeerId, TResult, TArg>>,
    tested_function_name: &str,
    tested_function: F,
    name_lookup: FNameLookup,
) where
    F: Fn(
        &mut Graph<
            TPayload,
            TGenesisPayload,
            TPeerId,
            MockSigner<TPeerId, TGenesisPayload>,
            IncrementalClock,
        >,
        &TArg,
    ) -> TResult,
    TResult: PartialEq + std::fmt::Debug,
    FNameLookup: Fn(&HashMap<event::Hash, String>, &TArg) -> String,
{
    for Test {
        setup,
        results_args: graph_cases,
    } in cases
    {
        let TestSetup {
            mut graph,
            peers_events: _,
            names,
            setup_name,
        } = setup;
        for (expected_result, result_cases) in graph_cases {
            for case in result_cases {
                let result = tested_function(&mut graph, &case);
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

pub mod topsort {
    use std::collections::{HashMap, HashSet};
    use std::fmt::Debug;

    use itertools::Itertools;

    use super::super::mocks::TestSetup;
    use crate::{algorithm::event, common::Directed};

    fn verify_topsort<G>(
        tested_topsort: Vec<event::Hash>,
        expected_events: HashSet<event::Hash>,
        event_relations: G,
        display_names: Option<&HashMap<event::Hash, String>>,
    ) -> Result<(), String>
    where
        G: Directed<NodeIdentifier = event::Hash>,
    {
        fn display_events<'a, E>(events: E, names: Option<&HashMap<event::Hash, String>>) -> String
        where
            E: IntoIterator<Item = &'a event::Hash> + Debug,
        {
            match names {
                Some(names) => {
                    format!(
                        "[{}]",
                        events
                            .into_iter()
                            .map(|h| names.get(h).expect("can't lookup event name"))
                            .format(", ")
                    )
                }
                None => format!("{:?}", events),
            }
        }

        // First we need to verify that the set of events is the same,
        // then we can check the ordering itself
        let tested_set = HashSet::<_>::from_iter(tested_topsort.clone().into_iter());
        if tested_set != expected_events {
            return Err(format!(
                "Events, missing in result: {}; unexpected events: {}",
                display_events(expected_events.difference(&tested_set), display_names),
                display_events(tested_set.difference(&expected_events), display_names)
            ));
        }
        // we test the definition of topsort.
        let mut events_before = HashSet::with_capacity(expected_events.len());
        for next in tested_topsort {
            for next_in in event_relations
                .in_neighbors(&next)
                .expect("Graph must be consistent, all tracked neighbors must be known")
            {
                if expected_events.contains(&next_in) && !events_before.contains(&next_in) {
                    return Err(format!(
                        "event {} expected to be before {} in topsort, but it is not",
                        next_in.clone(),
                        next.clone()
                    ));
                }
            }
            for next_out in event_relations
                .out_neighbors(&next)
                .expect("Graph must be consistent, all tracked neighbors must be known")
            {
                if expected_events.contains(&next_out) && events_before.contains(&next_out) {
                    return Err(format!(
                        "event {} expected to be after {} in topsort, but it is not",
                        next_out.clone(),
                        next.clone()
                    ));
                }
            }
            events_before.insert(next);
        }
        return Ok(());
    }

    pub struct PeerEventsSince {
        peer_name: &'static str,
        event_since_number: usize,
    }

    impl PeerEventsSince {
        pub fn new(peer_name: &'static str, event_since_number: usize) -> Self {
            PeerEventsSince {
                peer_name,
                event_since_number,
            }
        }
    }

    pub fn test_topsort<TPayload, TGenesisPayload, TPeerId>(
        setup: &TestSetup<TPayload, TGenesisPayload, TPeerId>,
        peer_name: &str,
        expected_events: Vec<PeerEventsSince>,
    ) -> Result<(), String>
    where
        TPayload: Clone,
        TGenesisPayload: Clone,
        TPeerId: Eq + std::hash::Hash + Clone + Debug,
    {
        let TestSetup {
            graph,
            peers_events: peers,
            names,
            setup_name: _,
        } = setup;
        let sync_for = graph
            .generate_sync_for(&peers.get(peer_name).unwrap().id)
            .expect("Graph must be correct and consistent");
        verify_topsort(
            sync_for
                .as_linear()
                .into_iter()
                .map(|e| e.hash().clone())
                .collect(),
            HashSet::<_>::from_iter(
                expected_events
                    .into_iter()
                    .map(|e| &peers.get(e.peer_name).unwrap().events[e.event_since_number..])
                    .flat_map(|s| s.iter().cloned().collect::<Vec<event::Hash>>()),
            ),
            &graph,
            Some(&names),
        )
    }
}
