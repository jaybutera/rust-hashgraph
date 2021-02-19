An implementation of the SWIRLDS HashGraph Consensus Algorithm as specified in [the
paper](https://www.swirlds.com/downloads/SWIRLDS-TR-2016-01.pdf). This is a
library that provides all the functions needed to reach consensus.

*This code uses iterators to traverse commitments, and although it is a pretty abstraction, it can be slow. I re-implemented the library using conventional graph search algorithms and matrix math [here](https://github.com/jaybutera/fast-hashgraph). Performance is improved dramatically.*
## Interact in the browser
![Force directed hashgraph](https://github.com/jaybutera/rust-hashgraph/blob/master/web_screenshot.png)


The library compiles to [WebAssembly](https://webassembly.org/) and can be
invoked like a normal ES6 module. The ```www``` directory contains a javascript
webapp to interactively build and visualize a hashgraph as a force directed
graph in [D3.js](https://d3js.org/).

## Tests
*Right now tests aren't work as the interface has changed to support wasm*

Run the tests with ```cargo test```. The unit tests in the graph module are the necessary steps to
order all events topologically according to the HashGraph algorithm.

![Tests snapshot](https://i.imgur.com/eilv4Vk.png)

---
The main.rs file contains the project binary. This is effectively an event loop that will
1. Connect and discover peers over the mDNS protocol
2. Upon receiving an event from a peer, update the in-memory graph, and pass the event onto another random peer
3. Host an HTTP service that a user can upload new transactions to, which will be encoded into an event and passed to a
   random peer

## TODO
- Nodes send and receive events on gossip network with rust-libp2p
- Graph traversal optimizations
- Right now cloning strings is abundant to support the js/wasm cross-boundary.
  There should be workarounds for this, or at least a macro system to generate
  an optimized version for native.
