use blake2::{Blake2b512, Digest};
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;

use crate::PeerId;


// smth like H256 ??? (some hash type)
#[derive(Serialize, Deserialize, Eq, PartialEq, Hash, Clone, Debug)]
pub struct Hash {
    #[serde(with = "BigArray")]
    inner: [u8; 64],
}

impl std::fmt::Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:X?}", self.inner)
    }
}

pub struct Event<TPayload> {
    // parents are inside `type_specific`, as geneses do not have ones
    pub children: Children,

    // Hash of user payload + parents + author data (blockchain-like)
    id: Hash,

    user_payload: TPayload,
    parents: Kind,
    author: PeerId,
}

pub struct Children {
    pub self_child: Option<Hash>,
    pub other_children: Vec<Hash>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Parents {
    pub self_parent: Hash,
    pub other_parent: Hash,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum Kind {
    Genesis,
    Regular(Parents),
}

impl<TPayload: Serialize> Event<TPayload> {
    pub fn new(payload: TPayload, event_kind: Kind, author: PeerId) -> bincode::Result<Self> {
        let hash = Self::calculate_hash(&payload, &event_kind, &author)?;
        Ok(Event {
            children: Children { self_child: None, other_children: vec![] },
            id: hash,
            user_payload: payload,
            parents: event_kind,
            author,
        })
    }

    fn digest_from_parts(payload: &TPayload, event_kind: &Kind, author: &PeerId) -> bincode::Result<Vec<u8>> {
        let mut v = vec![];
        let payload_bytes = bincode::serialize(&payload)?;
        v.extend(payload_bytes);
        let kind_bytes = bincode::serialize(&event_kind)?;
        v.extend(kind_bytes);
        let author_bytes = bincode::serialize(&author)?;
        v.extend(author_bytes);
        Ok(v)
    }

    fn calculate_hash(payload: &TPayload, event_kind: &Kind, author: &PeerId) -> bincode::Result<Hash> {
        let mut hasher = Blake2b512::new();
        hasher.update(Self::digest_from_parts(payload, event_kind, author)?);
        let hash_slice = &hasher.finalize()[..];
        // Should be compiler checkable but the developers of the library
        // didn't use generic constants, which would allow to work with arrays right away
        Ok(Hash {
            inner: hash_slice
                .try_into()
                .expect("Fixed hash function must return same result length"),
        })
    }
}

impl<TPayload> Event<TPayload> {
    pub fn hash(&self) -> &Hash {
        &self.id
    }

    pub fn parents(&self) -> &Kind {
        &self.parents
    }

    pub fn payload(&self) -> &TPayload {
        &self.user_payload
    }

    pub fn author(&self) -> &PeerId {
        &self.author
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use hex_literal::hex;

    use super::*;

    fn create_events() -> Result<Vec<Event<i32>>, bincode::Error> {
        let mock_parents = Parents {
            self_parent: Hash{inner: hex!(
                "021ced8799296ceca557832ab941a50b4a11f83478cf141f51f933f653ab9fbc
                c05a037cddbed06e309bf334942c4e58cdf1a46e237911ccd7fcf9787cbc7fd0"
            )},
            other_parent: Hash{inner: hex!(
                "a231788464c1d56aab39b098359eb00e2fd12622d85821d8bffe68fdb3044f24
                370e750986e6e4747f6ec0e051ae3e7d2558f7c4d3c4d5ab57362e572abecb36"
            )},
        };
        let results = vec![
            Event::new(0, Kind::Genesis, 0)?,
            Event::new(0, Kind::Genesis, 1)?,
            Event::new(0, Kind::Regular(mock_parents.clone()), 0)?,
            Event::new(1234567, Kind::Genesis, 0)?,
            Event::new(1234567, Kind::Regular(mock_parents.clone()), 0)?,
        ];
        Ok(results)
    }

    #[test]
    fn events_create() {
        create_events().unwrap();
        // also test on various payloads
        Event::new((), Kind::Genesis, 0).unwrap();
        Event::new((0,), Kind::Genesis, 0).unwrap();
        Event::new(vec![()], Kind::Genesis, 0).unwrap();
        Event::new("asdassa", Kind::Genesis, 0).unwrap();
        Event::new("asdassa".to_owned(), Kind::Genesis, 0).unwrap();
    }

    #[test]
    fn hashes_unique() {
        let events = create_events().unwrap();
        let mut hashes = HashSet::with_capacity(events.len());
        for n in events {
            assert!(!hashes.contains(n.hash()));
            hashes.insert(n.hash().clone());
        }
    }
}