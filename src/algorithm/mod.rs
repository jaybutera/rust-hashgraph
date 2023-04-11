use std::marker::PhantomData;

use blake2::{Blake2b512, Digest};
use thiserror::Error;

use crate::Timestamp;

use self::event::WithSignatureCreationError;

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
    type SignerIdentity;

    fn sign(&self, message: &[u8]) -> Signature;
    fn verify(
        &self,
        message: &[u8],
        signature: &Signature,
        identity: &Self::SignerIdentity,
    ) -> bool;
}

pub struct MockSigner<I>(PhantomData<I>);

impl<I> MockSigner<I> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

impl<I> Signer for MockSigner<I> {
    fn sign(&self, message: &[u8]) -> Signature {
        let mut hasher = Blake2b512::new();
        hasher.update(message);
        let hash_slice = &hasher.finalize()[..];
        Signature::from_array(hash_slice.try_into().unwrap())
    }

    type SignerIdentity = I;

    fn verify(
        &self,
        message: &[u8],
        signature: &Signature,
        _identity: &Self::SignerIdentity,
    ) -> bool {
        let calc_hash = self.sign(message);
        &calc_hash == signature
    }
}

pub trait Clock {
    fn current_timestamp(&mut self) -> Timestamp;
}

impl Clock for () {
    fn current_timestamp(&mut self) -> Timestamp {
        let start = std::time::SystemTime::now();
        start
            .duration_since(std::time::UNIX_EPOCH)
            .expect("Time went backwards")
            .as_nanos()
    }
}

pub struct IncrementalClock {
    next_time: u128,
}

impl IncrementalClock {
    pub fn new() -> Self {
        Self { next_time: 0 }
    }
}

impl Clock for IncrementalClock {
    fn current_timestamp(&mut self) -> Timestamp {
        let time = self.next_time;
        self.next_time += 1;
        time
    }
}

#[derive(Error, Debug)]
pub enum PushError<TPeerId> {
    #[error("Each peer can have only one genesis")]
    GenesisAlreadyExists,
    #[error("Could not find specified parent in the graph. Parent hash: `{0}`")]
    NoParent(event::Hash),
    #[error("Pushed event is already present in the graph. Hash: `{0}`. Can be triggered if hashes collide for actually different events (although extremely unlikely for 512 bit hash)")]
    EventAlreadyExists(event::Hash),
    #[error("Given peer in not known ")]
    PeerNotFound(TPeerId),
    /// `(expected, provided)`
    #[error("Provided author is different from author of self parent (expected {0}, provided {1}")]
    IncorrectAuthor(TPeerId, TPeerId),
    #[error("Serialization failed")]
    SerializationFailure(#[from] bincode::Error),
    #[error(transparent)]
    InvalidSignature(#[from] WithSignatureCreationError),
}
