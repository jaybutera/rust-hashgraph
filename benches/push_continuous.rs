use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use rand::seq::SliceRandom;
use rand_chacha::{rand_core::SeedableRng, ChaCha8Rng};
use rust_hashgraph::algorithm::{
    datastructure::Graph,
    event::{self, SignedEvent},
    Clock, IncrementalClock, MockSigner, Signer,
};

/// n_events does not count genesis events
fn push_events_into_empty_graph(n_peers: usize, n_events: usize) {
    let author_ids: Vec<_> = (0..n_peers).collect();
    // for reproducibility use seed
    let mut pseudo_rng = ChaCha8Rng::seed_from_u64(1);
    let mock_signer = MockSigner::new();
    let mut clock = IncrementalClock::new();

    let mut g = Graph::new(
        author_ids[0],
        (),
        (),
        999,
        mock_signer.clone(),
        IncrementalClock::new(),
    );
    for i in 1..n_peers {
        let next_genesis = SignedEvent::new(
            (),
            event::Kind::Genesis(()),
            author_ids[i],
            clock.current_timestamp(),
            |h| mock_signer.sign(h),
        )
        .expect("Failed to create event");
        let (unsigned, signature) = next_genesis.into_parts();
        g.push_event(unsigned, signature).unwrap();
    }
    for _ in 0..n_events {
        let author = author_ids.choose(&mut pseudo_rng).unwrap();
        let from_author = author_ids.choose(&mut pseudo_rng).unwrap();

        let parents = event::Parents {
            self_parent: g.peer_latest_event(author).unwrap().clone(),
            other_parent: g.peer_latest_event(from_author).unwrap().clone(),
        };
        let new_event = SignedEvent::new(
            (),
            event::Kind::Regular(parents),
            author.clone(),
            clock.current_timestamp(),
            |h| mock_signer.sign(h),
        )
        .expect("Failed to create event");
        let (unsigned, signature) = new_event.into_parts();
        g.push_event(unsigned, signature).unwrap();
    }
}

fn criterion_benchmark(c: &mut Criterion) {
    for n_peers in 3usize..=5 {
        let mut group = c.benchmark_group(format!("{} peers", n_peers));
        for i in 0usize..=5 {
            let n_events = i * 20;
            group.bench_with_input(
                BenchmarkId::new("push_event", format!("{} events", n_events)),
                &(n_peers, n_events),
                |b, (n_peers, n_events)| {
                    b.iter(|| {
                        push_events_into_empty_graph(black_box(*n_peers), black_box(*n_events))
                    })
                },
            );
        }
    }
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
