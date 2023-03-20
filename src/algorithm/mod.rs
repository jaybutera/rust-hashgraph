use blake2::{Blake2b512, Digest};
use thiserror::Error;

use crate::PeerId;

pub mod datastructure;
pub mod event;

// u64 must be enough, if new round each 0.1 second
// then we'll be supplied for >5*10^10 years lol
//
// For 10 round/sec and u32 it's 27 years, so
// TODO put a warning for such case or drop program.
type RoundNum = usize;

// TODO: change to actual signature
type Signature = event::Hash;

pub trait Signer {
    fn sign(&self, message: &[u8]) -> Signature;
}

impl Signer for () {
    fn sign(&self, message: &[u8]) -> Signature {
        let mut hasher = Blake2b512::new();
        hasher.update(message);
        let hash_slice = &hasher.finalize()[..];
        Signature::from_array(hash_slice.try_into().unwrap())
    }
}

#[derive(Error, Debug)]
pub enum PushError {
    #[error("Each peer can have only one genesis")]
    GenesisAlreadyExists,
    #[error("Could not find specified parent in the graph. Parent hash: `{0}`")]
    NoParent(event::Hash),
    #[error("Pushed event is already present in the graph. Hash: `{0}`. Can be triggered if hashes collide for actually different events (although extremely unlikely for 512 bit hash)")]
    EventAlreadyExists(event::Hash),
    #[error("Given peer in not known ")]
    PeerNotFound(PeerId),
    /// `(expected, provided)`
    #[error("Provided author is different from author of self parent (expected {0}, provided {1}")]
    IncorrectAuthor(PeerId, PeerId),
    #[error("Serialization failed")]
    SerializationFailure(#[from] bincode::Error),
}
