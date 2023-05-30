use std::{iter::successors, ops::Deref};

use itertools::Itertools;
use mocks::{
    build_graph_detailed_example, build_graph_detailed_example_with_timestamps, build_graph_fork,
    build_graph_from_paper, build_graph_index_test, build_graph_some_chain, TestSetup,
};
use test_utils::{run_tests, test_cases, Test};

use crate::algorithm::{datastructure::tests::mocks::MockPeerId, IncrementalClock, MockSigner};

use super::*;

mod mocks;
mod test_utils;

// Test simple work + errors

#[test]
fn push_works() {
    let mut graph = Graph::new(0, (), (), 999, MockSigner::new(), IncrementalClock::new());

    // new event by the same author
    let new_event = SignedEvent::new(
        (),
        event::Kind::Regular(Parents {
            self_parent: graph.peer_latest_event(&graph.self_id).unwrap().clone(),
            other_parent: graph.peer_latest_event(&graph.self_id).unwrap().clone(),
        }),
        graph.self_id,
        1,
        |h| MockSigner::<i32, ()>::new().sign(h),
    )
    .unwrap();
    let (unsigned, signature) = new_event.into_parts();
    graph.push_event(unsigned, signature).unwrap();

    // new peer
    let new_event = SignedEvent::new((), event::Kind::Genesis(()), 1, 2, |h| {
        MockSigner::<i32, ()>::new().sign(h)
    })
    .unwrap();
    let (unsigned, signature) = new_event.into_parts();
    graph.push_event(unsigned, signature).unwrap();

    // new event by the new peer
    let new_event = SignedEvent::new(
        (),
        event::Kind::Regular(Parents {
            self_parent: graph.peer_latest_event(&1).unwrap().clone(),
            other_parent: graph.peer_latest_event(&graph.self_id).unwrap().clone(),
        }),
        1,
        3,
        |h| MockSigner::<i32, ()>::new().sign(h),
    )
    .unwrap();
    let (unsigned, signature) = new_event.into_parts();
    graph.push_event(unsigned, signature).unwrap();
}

#[test]
fn create_works() {
    let mut graph = Graph::new(
        0,
        (),
        (),
        999,
        MockSigner::<i32, ()>::new(),
        IncrementalClock::new(),
    );

    // new event by the same author
    graph
        .create_event((), graph.peer_latest_event(&graph.self_id).unwrap().clone())
        .unwrap();

    // new peer
    let new_event = SignedEvent::new((), event::Kind::Genesis(()), 1, 2, |h| {
        MockSigner::<i32, ()>::new().sign(h)
    })
    .unwrap();
    let (unsigned, signature) = new_event.into_parts();
    graph.push_event(unsigned, signature).unwrap();

    // new event by the new peer
    graph
        .create_event((), graph.peer_latest_event(&1).unwrap().clone())
        .unwrap();
}

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
    let new_event = SignedEvent::new((), event::Kind::Genesis(()), a_id, 0, |h| {
        MockSigner::<i32, ()>::new().sign(h)
    })
    .expect("Failed to create event");
    let (unsigned, signature) = new_event.into_parts();
    assert!(matches!(
        graph.push_event(unsigned, signature),
        Err(PushError::EventAlreadyExists(hash)) if &hash == graph.peer_genesis(&a_id).unwrap()
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
    let new_event = SignedEvent::new(
        1,
        event::Kind::Genesis(()),
        peers.get("a").unwrap().id,
        0,
        |h| MockSigner::<MockPeerId, ()>::new().sign(h),
    )
    .expect("Failed to create event");
    let (unsigned, signature) = new_event.into_parts();
    assert!(matches!(
        graph.push_event(unsigned, signature),
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
    let signer = MockSigner::<MockPeerId, ()>::new();
    let fake_event = SignedEvent::new((), event::Kind::Genesis(()), 1232423, 123, |h| {
        signer.sign(h)
    })
    .unwrap();
    let legit_event_hash = graph.peer_latest_event(&0).unwrap().clone();

    let fake_parents_1 = Parents {
        self_parent: fake_event.hash().clone(),
        other_parent: legit_event_hash.clone(),
    };
    let new_event = SignedEvent::new(
        (),
        event::Kind::Regular(fake_parents_1),
        peers.get("a").unwrap().id,
        0,
        |h| signer.sign(h),
    )
    .expect("Failed to create event");
    let (unsigned, signature) = new_event.into_parts();
    assert!(matches!(
        graph.push_event(unsigned, signature),
        Err(PushError::NoParent(fake_hash)) if &fake_hash == fake_event.hash()
    ));

    let fake_parents_2 = Parents {
        self_parent: legit_event_hash.clone(),
        other_parent: fake_event.hash().clone(),
    };
    let new_event = SignedEvent::new(
        (),
        event::Kind::Regular(fake_parents_2),
        peers.get("a").unwrap().id,
        0,
        |h| signer.sign(h),
    )
    .expect("Failed to create event");
    let (unsigned, signature) = new_event.into_parts();
    assert!(matches!(
        graph.push_event(unsigned, signature),
        Err(PushError::NoParent(fake_hash)) if &fake_hash == fake_event.hash()
    ));
}

// Test graph properties

#[test]
fn test_ancestor() {
    run_tests!(
        tested_function_name => "ancestor",
        tested_function => |g, (e1, e2)| g.is_ancestor(&e1, &e2),
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
            graph
                .ancestor_iter(&peers.get("b").unwrap().events[3])
                .unwrap(),
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
            graph
                .ancestor_iter(&peers.get("b").unwrap().events[6])
                .unwrap(),
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
        let ancestors_from_iter = HashSet::<_>::from_iter(iter.map(|e| e.inner().hash().clone()));
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
    fn round_index_consistent<TPayload, TGenesisPayload, TPeerId: Eq + std::hash::Hash>(
        graph: &Graph<
            TPayload,
            TGenesisPayload,
            TPeerId,
            MockSigner<TPeerId, TGenesisPayload>,
            IncrementalClock,
        >,
        hash: &event::Hash,
    ) -> Result<usize, String> {
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
        tested_function => |graph, event| graph.is_witness(&event).expect(&format!("Can't find event {:?}", event)),
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

#[test]
fn test_is_round_decided() {
    run_tests!(
        tested_function_name => "is_round_decided",
        tested_function => |g, _| g.last_known_decided_round,
        name_lookup => |_names, _| "".to_string(),
        peers_literal => _peers,
        tests => [
            (
                setup => build_graph_some_chain(0, 999).unwrap(),
                test_case => (
                    expect: None,
                    arguments: vec![()],
                ),
            ),
            (
                setup => build_graph_from_paper(0, 999).unwrap(),
                test_case => (
                    expect: None,
                    arguments: vec![()],
                ),
            ),
            (
                setup => build_graph_detailed_example(0, 999).unwrap(),
                test_case => (
                    expect: Some(1),
                    arguments: vec![()],
                ),
            ),
            (
                setup => build_graph_fork([42, 1337, 80085].into_iter().cycle(), 999).unwrap(),
                test_case => (
                    expect: Some(1),
                    arguments: vec![()],
                ),
            ),
        ]
    );
}

#[test]
fn test_ordering_decided() {
    run_tests!(
        tested_function_name => "ordering_data decided",
        tested_function => |g, event| matches!(g.ordering_data(*event), Ok(_)),
        name_lookup => |names, event| names.get(event).unwrap().to_owned(),
        peers_literal => peers,
        tests => [
            (
                setup => build_graph_detailed_example(0, 999).unwrap(),
                test_case => (
                    expect: true,
                    arguments: vec![
                        &peers.get("a").unwrap().events[0],
                        &peers.get("a").unwrap().events[1],
                        &peers.get("b").unwrap().events[0],
                        &peers.get("b").unwrap().events[1],
                        &peers.get("b").unwrap().events[2],
                        &peers.get("c").unwrap().events[0],
                        &peers.get("d").unwrap().events[0],
                        &peers.get("d").unwrap().events[1],
                        &peers.get("d").unwrap().events[2],
                        &peers.get("d").unwrap().events[3],
                        // For some reason they do not consider this event in "detailed examples"
                        // however it seems to fit the needed properties.
                        &peers.get("d").unwrap().events[4],
                    ],
                ),
                test_case => (
                    expect: false,
                    arguments: [
                        &peers.get("a").unwrap().events[2..],
                        &peers.get("b").unwrap().events[3..],
                        &peers.get("c").unwrap().events[1..],
                        &peers.get("d").unwrap().events[5..],
                    ].iter()
                        .flat_map(|s| s.iter().collect::<Vec<&_>>())
                        .collect(),
                ),
            ),
        ]
    );
}

#[test]
fn test_ordering_data_correct() {
    let signer = MockSigner::<u64, ()>::new();
    run_tests!(
        tested_function_name => "ordering_data correct values",
        tested_function => |g, event| g.ordering_data(*event),
        name_lookup => |names, event| names.get(event).unwrap().to_owned(),
        peers_literal => peers,
        tests => [
            (
                setup => build_graph_detailed_example(0, 999).unwrap(),
                test_case => (
                    expect: Ok((1, 0, signer.sign(&peers.get("a").unwrap().events[0].clone()))),
                    arguments: vec![&peers.get("a").unwrap().events[0]],
                ),
                test_case => (
                    expect: Ok((1, 0, signer.sign(&peers.get("a").unwrap().events[1].clone()))),
                    arguments: vec![&peers.get("a").unwrap().events[1]],
                ),
                test_case => (
                    expect: Ok((1, 0, signer.sign(&peers.get("b").unwrap().events[0].clone()))),
                    arguments: vec![&peers.get("b").unwrap().events[0]],
                ),
                test_case => (
                    expect: Ok((1, 0, signer.sign(&peers.get("b").unwrap().events[1].clone()))),
                    arguments: vec![&peers.get("b").unwrap().events[1]],
                ),
                test_case => (
                    expect: Ok((1, 0, signer.sign(&peers.get("b").unwrap().events[2].clone()))),
                    arguments: vec![&peers.get("b").unwrap().events[2]],
                ),
                test_case => (
                    expect: Ok((1, 0, signer.sign(&peers.get("c").unwrap().events[0].clone()))),
                    arguments: vec![&peers.get("c").unwrap().events[0]],
                ),
                test_case => (
                    expect: Ok((1, 0, signer.sign(&peers.get("d").unwrap().events[0].clone()))),
                    arguments: vec![&peers.get("d").unwrap().events[0]],
                ),
                test_case => (
                    expect: Ok((1, 0, signer.sign(&peers.get("d").unwrap().events[1].clone()))),
                    arguments: vec![&peers.get("d").unwrap().events[1]],
                ),
                test_case => (
                    expect: Ok((1, 0, signer.sign(&peers.get("d").unwrap().events[2].clone()))),
                    arguments: vec![&peers.get("d").unwrap().events[2]],
                ),
                test_case => (
                    expect: Ok((1, 0, signer.sign(&peers.get("d").unwrap().events[3].clone()))),
                    arguments: vec![&peers.get("d").unwrap().events[3]],
                ),
                // For some reason they do not consider this event in "detailed examples"
                // however it seems to fit the needed properties.
                test_case => (
                    expect: Ok((1, 0, signer.sign(&peers.get("d").unwrap().events[4].clone()))),
                    arguments: vec![&peers.get("d").unwrap().events[4]],
                ),
                test_case => (
                    expect: Err(OrderingDataError::Undecided),
                    arguments: [
                        &peers.get("a").unwrap().events[2..],
                        &peers.get("b").unwrap().events[3..],
                        &peers.get("c").unwrap().events[1..],
                        &peers.get("d").unwrap().events[5..],
                    ].iter()
                        .flat_map(|s| s.iter().collect::<Vec<&_>>())
                        .collect(),
                ),
            ),
            (
                setup => build_graph_detailed_example_with_timestamps(
                    0, 999, successors(Some(1), |x| Some(x+1))
                ).unwrap(),
                test_case => (
                    expect: Ok((1, 9, signer.sign(&peers.get("a").unwrap().events[0].clone()))),
                    arguments: vec![&peers.get("a").unwrap().events[0]],
                ),
                test_case => (
                    expect: Ok((1, 9, signer.sign(&peers.get("a").unwrap().events[1].clone()))),
                    arguments: vec![&peers.get("a").unwrap().events[1]],
                ),
                test_case => (
                    expect: Ok((1, 1, signer.sign(&peers.get("b").unwrap().events[0].clone()))),
                    arguments: vec![&peers.get("b").unwrap().events[0]],
                ),
                test_case => (
                    expect: Ok((1, 3, signer.sign(&peers.get("b").unwrap().events[1].clone()))),
                    arguments: vec![&peers.get("b").unwrap().events[1]],
                ),
                test_case => (
                    expect: Ok((1, 6, signer.sign(&peers.get("b").unwrap().events[2].clone()))),
                    arguments: vec![&peers.get("b").unwrap().events[2]],
                ),
                test_case => (
                    expect: Ok((1, 6, signer.sign(&peers.get("c").unwrap().events[0].clone()))),
                    arguments: vec![&peers.get("c").unwrap().events[0]],
                ),
                test_case => (
                    expect: Ok((1, 2, signer.sign(&peers.get("d").unwrap().events[0].clone()))),
                    arguments: vec![&peers.get("d").unwrap().events[0]],
                ),
                test_case => (
                    expect: Ok((1, 2, signer.sign(&peers.get("d").unwrap().events[1].clone()))),
                    arguments: vec![&peers.get("d").unwrap().events[1]],
                ),
                test_case => (
                    expect: Ok((1, 8, signer.sign(&peers.get("d").unwrap().events[2].clone()))),
                    arguments: vec![&peers.get("d").unwrap().events[2]],
                ),
                test_case => (
                    expect: Ok((1, 8, signer.sign(&peers.get("d").unwrap().events[3].clone()))),
                    arguments: vec![&peers.get("d").unwrap().events[3]],
                ),
                // For some reason they do not consider this event in "detailed examples"
                // however it seems to fit the needed properties.
                test_case => (
                    expect: Ok((1, 10, signer.sign(&peers.get("d").unwrap().events[4].clone()))),
                    arguments: vec![&peers.get("d").unwrap().events[4]],
                ),
                test_case => (
                    expect: Err(OrderingDataError::Undecided),
                    arguments: [
                        &peers.get("a").unwrap().events[2..],
                        &peers.get("b").unwrap().events[3..],
                        &peers.get("c").unwrap().events[1..],
                        &peers.get("d").unwrap().events[5..],
                    ].iter()
                        .flat_map(|s| s.iter().collect::<Vec<&_>>())
                        .collect(),
                ),
            ),
        ]
    );
}

fn check_recognized_events(setup: TestSetup<(), (), u64>) {
    let TestSetup {
        mut graph,
        peers_events: _,
        names,
        setup_name: _,
    } = setup;
    let expected_recognized: HashSet<&event::Hash> = names.keys().collect();
    let mut recognized_ordered = Vec::new();
    while let Some(next) = graph.next_recognized_event() {
        recognized_ordered.push(next.hash().clone());
    }
    let recognized: HashSet<&event::Hash> = recognized_ordered.iter().collect();
    assert_eq!(
        expected_recognized, recognized,
        "all pushed events must be recognized"
    );
    // check partial ordering
    // (ancestors are always recognized before)
    for (h, h_prev) in recognized_ordered.iter().enumerate().flat_map(|(i, h)| {
        recognized_ordered[..i]
            .iter()
            .map(move |h_prev| (h, h_prev))
    }) {
        // this event must not be an ancestor of all previous
        assert!(!graph.is_ancestor(h_prev, h), "recognized events must be partially ordered by ancestry relation; {} is an ancestor of {} but placed before in the order", names.get(h).unwrap(), names.get(h_prev).unwrap());
    }
}

#[test]
fn test_recognized_events_returned() {
    check_recognized_events(build_graph_some_chain((), 999).unwrap());
    check_recognized_events(build_graph_from_paper((), 999).unwrap());
    check_recognized_events(build_graph_detailed_example((), 999).unwrap());
    check_recognized_events(build_graph_index_test((), 999).unwrap());
    // todo: check fork case
}

#[test]
fn test_event_order_correct() {
    let setup =
        build_graph_detailed_example_with_timestamps(0, 999, successors(Some(1), |x| Some(x + 1)))
            .unwrap();
    let TestSetup {
        mut graph,
        peers_events: peers,
        names,
        setup_name: _,
    } = setup;

    // exp
    let expected_finalized = vec![
        (&peers.get("b").unwrap().events[0], (1, 1)),
        (&peers.get("d").unwrap().events[0], (1, 2)),
        (&peers.get("d").unwrap().events[1], (1, 2)),
        (&peers.get("b").unwrap().events[1], (1, 3)),
        (&peers.get("b").unwrap().events[2], (1, 6)),
        (&peers.get("c").unwrap().events[0], (1, 6)),
        (&peers.get("d").unwrap().events[2], (1, 8)),
        (&peers.get("d").unwrap().events[3], (1, 8)),
        (&peers.get("a").unwrap().events[0], (1, 9)),
        (&peers.get("a").unwrap().events[1], (1, 9)),
        (&peers.get("d").unwrap().events[4], (1, 10)),
    ];
    let expected_finalized = expected_finalized
        .into_iter()
        .map(|(hash, (_, _))| hash)
        .cloned()
        .collect_vec();

    let mut finalized = vec![];
    while let Some(event) = graph.next_finalized_event() {
        finalized.push(event.hash().clone());
    }

    let expected_finalized_names = expected_finalized
        .iter()
        .map(|h| names.get(h).unwrap())
        .collect_vec();
    let finalized_names = finalized
        .iter()
        .map(|h| names.get(h).unwrap())
        .collect_vec();

    println!("expected: {:?}", expected_finalized_names);
    println!("got: {:?}", finalized_names);

    assert_eq!(finalized, expected_finalized);
}

#[test]
fn test_sync_data_correct() {
    use test_utils::topsort::*;

    let setup = build_graph_from_paper((), 999).unwrap();
    let tests = vec![
        (
            "a",
            vec![
                PeerEventsSince::new("c", 3),
                PeerEventsSince::new("d", 1),
                PeerEventsSince::new("e", 2),
            ],
        ),
        (
            "b",
            vec![
                PeerEventsSince::new("a", 0),
                PeerEventsSince::new("c", 3),
                PeerEventsSince::new("d", 1),
                PeerEventsSince::new("e", 2),
            ],
        ),
        ("c", vec![]),
        (
            "d",
            vec![
                PeerEventsSince::new("a", 0),
                PeerEventsSince::new("b", 1),
                PeerEventsSince::new("c", 3),
                PeerEventsSince::new("e", 2),
            ],
        ),
        (
            "e",
            vec![
                PeerEventsSince::new("a", 0),
                PeerEventsSince::new("b", 1),
                PeerEventsSince::new("c", 0),
                PeerEventsSince::new("d", 0),
            ],
        ),
    ];
    for (peer_name, expected_events) in tests {
        test_topsort(&setup, peer_name, expected_events)
            .expect(&format!("sync for peer {} failed", peer_name));
    }

    let setup = build_graph_some_chain((), 999).unwrap();
    let tests = vec![
        (
            "g1",
            vec![PeerEventsSince::new("g2", 3), PeerEventsSince::new("g3", 2)],
        ),
        ("g2", vec![]),
        ("g3", vec![PeerEventsSince::new("g2", 3)]),
    ];
    for (peer_name, expected_events) in tests {
        test_topsort(&setup, peer_name, expected_events)
            .expect(&format!("sync for peer {} failed", peer_name));
    }

    let setup = build_graph_detailed_example((), 999).unwrap();
    let tests = vec![
        (
            "a",
            vec![
                PeerEventsSince::new("b", 10),
                PeerEventsSince::new("c", 3),
                PeerEventsSince::new("d", 9),
            ],
        ),
        ("b", vec![]),
        (
            "c",
            vec![
                PeerEventsSince::new("a", 5),
                PeerEventsSince::new("b", 7),
                PeerEventsSince::new("d", 9),
            ],
        ),
        (
            "d",
            vec![PeerEventsSince::new("a", 6), PeerEventsSince::new("b", 10)],
        ),
    ];
    for (peer_name, expected_events) in tests {
        test_topsort(&setup, peer_name, expected_events)
            .expect(&format!("sync for peer {} failed", peer_name));
    }

    let setup = build_graph_fork([42, 1337, 80085].into_iter().cycle(), 999).unwrap();
    let tests = vec![("a", vec![]), ("m", vec![PeerEventsSince::new("a", 4)])];
    for (peer_name, expected_events) in tests {
        test_topsort(&setup, peer_name, expected_events)
            .expect(&format!("sync for peer {} failed", peer_name));
    }
}
