use libp2p::{secio,NetworkBehaviour};
use libp2p::core::transport::Transport;
use substrate_network_libp2p::{CustomMessage, Protocol, ServiceEvent, build_multiaddr};
use futures::{future, stream, prelude::*, try_ready};
use rand::seq::SliceRandom;
use std::{io, iter};

fn build_nodes<TMsg>(num: usize) -> Vec<substrate_network_libp2p::Service<TMsg>>
	where TMsg: CustomMessage + Send + 'static
{
	let mut result: Vec<substrate_network_libp2p::Service<_>> = Vec::with_capacity(num);

	for _ in 0 .. num {
		let mut boot_nodes = Vec::new();
		if !result.is_empty() {
			let mut bootnode = result[0].listeners().next().unwrap().clone();
			bootnode.append(Protocol::P2p(result[0].peer_id().clone().into()));
			boot_nodes.push(bootnode.to_string());
		}

		let config = substrate_network_libp2p::NetworkConfiguration {
			listen_addresses: vec![build_multiaddr![Ip4([127, 0, 0, 1]), Tcp(0u16)]],
			boot_nodes,
			..substrate_network_libp2p::NetworkConfiguration::default()
		};

		let proto = substrate_network_libp2p::RegisteredProtocol::new(*b"tst", &[1]);
		result.push(substrate_network_libp2p::start_service(config, iter::once(proto)).unwrap());
	}

	result
}

fn basic_two_nodes_connectivity() {
	let (mut service1, mut service2) = {
		let mut l = build_nodes::<Vec<u8>>(2).into_iter();
		let a = l.next().unwrap();
		let b = l.next().unwrap();
		(a, b)
	};

	let fut1 = future::poll_fn(move || -> io::Result<_> {
		match try_ready!(service1.poll()) {
			Some(ServiceEvent::OpenedCustomProtocol { protocol, version, .. }) => {
				assert_eq!(protocol, *b"tst");
				assert_eq!(version, 1);
				Ok(Async::Ready(()))
			},
			_ => panic!(),
		}
	});

	let fut2 = future::poll_fn(move || -> io::Result<_> {
		match try_ready!(service2.poll()) {
			Some(ServiceEvent::OpenedCustomProtocol { protocol, version, .. }) => {
				assert_eq!(protocol, *b"tst");
				assert_eq!(version, 1);
				Ok(Async::Ready(()))
			},
			_ => panic!(),
		}
	});

	let combined = fut1.select(fut2).map_err(|(err, _)| err);
	tokio::runtime::Runtime::new().unwrap().block_on_all(combined).unwrap();
}

fn main() {
    basic_two_nodes_connectivity();
}
