An implementation of the SWIRLDS HashGraph Consensus Algorithm as specified in [the
paper](https://www.swirlds.com/downloads/SWIRLDS-TR-2016-01.pdf).

## Usage
For now, you can just run the tests with ```cargo test```. The unit tests in the graph module are the necessary steps to
order all events topologically according to the HashGraph algorithm.

![Tests snapshot](https://i.imgur.com/eilv4Vk.png)

### The main.rs file contains the project binary. This is effectively an event loop that will
1. Connect and discover peers over the mDNS protocol
2. Upon receiving an event from a peer, update the in-memory graph, and pass the event onto another random peer
3. Host an HTTP service that a user can upload new transactions to, which will be encoded into an event and passed to a
   random peer

## TODO
- Nodes send and receive events on gossip network with rust-libp2p
- Graph traversal optimizations
