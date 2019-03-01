use libp2p::{secio,NetworkBehaviour};
use libp2p::core::transport::Transport;

#[derive(NetworkBehaviour)]
struct CustomBehaviour<TSubstream: libp2p::tokio_io::AsyncRead + libp2p::tokio_io::AsyncWrite> {
    #[behaviour(handler = "on_floodsub")]
    floodsub: libp2p::floodsub::Floodsub<TSubstream>,
    mdns: libp2p::mdns::Mdns<TSubstream>,
}

fn main() {

    let local_key = secio::SecioKeyPair::ed25519_generated().unwrap();
    let local_pub_key = local_key.to_public_key();

    // Create TCP transport with secio message encryption
    let transport = libp2p::tcp::TcpConfig::new()
        .with_upgrade(secio::SecioConfig::new(local_key));

    // Create a floodsub for CRDTs
    let floodsub_topic = libp2p::floodsub::TopicBuilder::new("tmp").build();

    let mut behaviour = CustomBehaviour {
            floodsub: libp2p::floodsub::Floodsub::new(local_pub_key.clone().into_peer_id()),
            mdns: libp2p::mdns::Mdns::new().expect("Failed to create mDNS service"),
    };

    // Swarm is the libp2p state management structure
    let mut swarm = libp2p::core::swarm::SwarmBuilder::new(
        transport,
        behaviour,
        local_pub_key.clone().into_peer_id()).build();
}
