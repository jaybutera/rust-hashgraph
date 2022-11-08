An implementation of the SWIRLDS HashGraph Consensus Algorithm as specified in [the
paper](https://www.swirlds.com/downloads/SWIRLDS-TR-2016-01.pdf). This is a
library that provides all the functions needed to reach consensus.

*This code uses iterators to traverse commitments, and although it is a pretty abstraction, it can be slow. There is a re-implementation of the library using conventional graph search algorithms and matrix math [here](https://github.com/jaybutera/fast-hashgraph). Performance is improved dramatically.*

## Tests
Run the tests with ```cargo test```. The unit tests in the graph module are the necessary steps to
order all events topologically according to the HashGraph algorithm.


## TODO
- Graph traversal optimizations
