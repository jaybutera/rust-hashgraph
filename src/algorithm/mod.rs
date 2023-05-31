use std::marker::PhantomData;

use blake2::{Blake2b512, Digest};
use thiserror::Error;

use crate::Timestamp;

use self::event::{Hash, Signature, WithSignatureCreationError};

pub mod datastructure;
pub mod event;

// u64 must be enough, if new round each 0.1 second
// then we'll be supplied for >5*10^10 years lol
//
// For 10 round/sec and u32 it's 27 years, so
// TODO put a warning for such case or drop program.
type RoundNum = usize;

pub trait Signer<TGenesisPayload> {
    type SignerIdentity;

    fn sign(&self, event_hash: &Hash) -> Signature;
    fn verify(
        &self,
        event_hash: &Hash,
        signature: &Signature,
        identity: &Self::SignerIdentity,
        genesis_payload: &TGenesisPayload,
    ) -> bool;
}

/// Signer that just hashes the hash again
#[derive(Clone)]
pub struct MockSigner<I, G>(PhantomData<(I, G)>);

impl<I, G> MockSigner<I, G> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

impl<I, G> Signer<G> for MockSigner<I, G> {
    fn sign(&self, message: &Hash) -> Signature {
        let mut hasher = Blake2b512::new();
        hasher.update(message.as_ref());
        let hash_slice = &hasher.finalize()[..];
        Signature(Hash::from_array(hash_slice.try_into().unwrap()))
    }

    type SignerIdentity = I;

    fn verify(
        &self,
        message: &Hash,
        signature: &Signature,
        _identity: &Self::SignerIdentity,
        _genesis_payload: &G,
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
    #[error("The author of a regular event is unkown")]
    PeerNotFound(TPeerId),
    /// `(expected, provided)`
    #[error("Provided author is different from author of self parent (expected {0}, provided {1}")]
    IncorrectAuthor(TPeerId, TPeerId),
    #[error("Serialization failed")]
    SerializationFailure(#[from] bincode::Error),
    #[error(transparent)]
    InvalidSignature(#[from] WithSignatureCreationError),
}

#[cfg(test)]
mod tests {
    use blake2::Blake2b512;
    use blake2::Digest;

    use super::event::Hash;
    use super::MockSigner;
    use super::Signer;

    #[test]
    fn mock_signer_works() {
        let some_data = [0, 1, 2, 3];
        let hash = {
            let mut hasher = Blake2b512::new();
            hasher.update(some_data);
            let hash_slice = &hasher.finalize()[..];
            let hash_arr: [u8; 64] = hash_slice.try_into().expect("event hashing failure");
            Hash::from_array(hash_arr)
        };

        let signer = MockSigner::<(), ()>::new();
        let signature = signer.sign(&hash);
        assert!(signer.verify(&hash, &signature, &(), &()));
    }
}
