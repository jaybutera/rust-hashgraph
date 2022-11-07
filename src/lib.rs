//! Event data structures and operations
//! 

use serde::{Serialize, Deserialize};
use serde_big_array::BigArray;

mod graph;

// u64 must be enough, if new round each 0.1 second
// then we'll be supplied for >5*10^10 years lol
type RoundNum = u64;
// Some peer identity type? probably make generic
type PeerId = u64;
