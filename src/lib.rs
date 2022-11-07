//! Event data structures and operations
//! 

use serde::{Serialize, Deserialize};
use serde_big_array::BigArray;

mod graph;

// Some peer identity type? probably make generic
type PeerId = u64;
