An implementation of the SWIRLDS HashGraph Consensus Algorithm as specified in [the
paper](https://www.swirlds.com/downloads/SWIRLDS-TR-2016-01.pdf). This is a
library that provides all the functions needed to reach consensus.

*This code uses iterators to traverse commitments, and although it is a pretty abstraction, it can be slow. There is a re-implementation of the library using conventional graph search algorithms and matrix math [here](https://github.com/jaybutera/fast-hashgraph). Performance is improved dramatically.*

## Tests
Run the tests with ```cargo test```.

## Usage
The algorithm is performed by `algorithm::datastructure::Graph` structure. See its documentation & implementation for details.
