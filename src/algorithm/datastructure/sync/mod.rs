//! Synchronization with another peer.
//!
//! We call a "reciever" a peer that recieves the graph updates
//! (and updates its state accordingly) and a "sender" - a peer
//! that shares its state to reciever.
//!
//! The sync is done in 3 steps:
//! 1. reciever sends compressed version of its state
//! 2. sender figures out what is missing and sends the missing
//! stuff to reciever
//! 3. reciever recieves (lol), verifies, and applies needed changes.
//!
// isn't it the same as https://github.com/bcpierce00/unison ?

pub mod jobs;
pub mod state;
