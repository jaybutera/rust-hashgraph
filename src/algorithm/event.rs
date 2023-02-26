use blake2::{Blake2b512, Digest};
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;

use crate::{PeerId, Timestamp};

use super::Signature;

// smth like H256 ??? (some hash type)
#[derive(Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash, Clone)]
pub struct Hash {
    #[serde(with = "BigArray")]
    inner: [u8; 64],
}

impl std::fmt::Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:X?}", self.inner)
    }
}

impl std::fmt::Debug for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hash")
            .field("hex_value", &format!("{self}"))
            .finish()
    }
}

impl std::ops::BitXor for &Hash {
    type Output = Hash;

    fn bitxor(self, rhs: Self) -> Self::Output {
        let mut result = [0u8; 64];
        for (i, (b1, b2)) in self.inner.iter().zip(rhs.inner.iter()).enumerate() {
            result[i] = b1 ^ b2;
        }
        Hash::from_array(result)
    }
}

impl std::ops::BitXor<&Hash> for Hash {
    type Output = Hash;

    fn bitxor(mut self, rhs: &Self) -> Self::Output {
        for i in 0..self.inner.len() {
            self.inner[i] ^= rhs.inner[i];
        }
        self
    }
}

impl Hash {
    pub fn into_array(self) -> [u8; 64] {
        return self.inner;
    }

    pub fn as_ref(&self) -> &[u8; 64] {
        return &self.inner;
    }

    pub const fn from_array(inner: [u8; 64]) -> Self {
        return Hash { inner };
    }
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Hash, Clone, Debug)]
pub struct Event<TPayload> {
    // parents are inside `type_specific`, as geneses do not have ones
    pub children: Children,

    // Hash of user payload + parents + author data + timestamp (blockchain-like)
    // TODO: write why
    id: Hash,

    user_payload: TPayload,
    parents: Kind,
    author: PeerId,
    /// Timestamp set by author
    timestamp: Timestamp,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Hash, Clone, Debug)]
pub struct Children {
    // Child(-ren in case of forks) with the same author
    pub self_child: SelfChild,
    // Children created by different peers
    pub other_children: Vec<Hash>,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Hash, Clone, Debug)]
pub enum SelfChild {
    HonestParent(Option<Hash>),
    ForkingParent(Vec<Hash>),
}

impl SelfChild {
    /// Returns `true` if the parent became dishonest/forking
    pub fn add_child(&mut self, child: Hash) -> bool {
        // guilty until proven innocent lol :)
        let mut dishonesty = true;
        match self {
            SelfChild::HonestParent(self_child_entry) => {
                let new_val = match self_child_entry {
                    None => {
                        dishonesty = false;
                        Self::HonestParent(Some(child))
                    }
                    Some(child_2) => Self::ForkingParent(vec![child, child_2.clone()]),
                };
                *self = new_val;
            }
            SelfChild::ForkingParent(children) => children.push(child),
        };
        dishonesty
    }

    /// Returns `true` if the child was removed (thus was present before removal)
    pub fn with_child_removed(self, child: &Hash) -> Self {
        let self_children_vec: Vec<_> = self.into();
        self_children_vec
            .into_iter()
            .filter(|h| h != child)
            .collect::<Vec<_>>()
            .into()
    }
}

impl Into<Vec<Hash>> for SelfChild {
    fn into(self) -> Vec<Hash> {
        match self {
            SelfChild::HonestParent(child_opt) => child_opt.into_iter().collect(),
            SelfChild::ForkingParent(children_list) => children_list,
        }
    }
}

impl From<Vec<Hash>> for SelfChild {
    fn from(value: Vec<Hash>) -> Self {
        match &value[..] {
            [] => Self::HonestParent(None),
            [_] => Self::HonestParent(Some(value.into_iter().next().unwrap())),
            _ => Self::ForkingParent(value),
        }
    }
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Hash, Clone, Debug)]
pub struct Parents {
    pub self_parent: Hash,
    pub other_parent: Hash,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Hash, Clone, Debug)]
pub enum Kind {
    Genesis,
    Regular(Parents),
}

impl<TPayload: Serialize> Event<TPayload> {
    pub fn new(
        payload: TPayload,
        event_kind: Kind,
        author: PeerId,
        timestamp: Timestamp,
    ) -> bincode::Result<Self> {
        let hash = Self::calculate_hash(&payload, &event_kind, &author, &timestamp)?;
        Ok(Event {
            children: Children {
                self_child: SelfChild::HonestParent(None),
                other_children: vec![],
            },
            id: hash,
            user_payload: payload,
            parents: event_kind,
            author,
            timestamp,
        })
    }

    fn digest_from_parts(
        payload: &TPayload,
        event_kind: &Kind,
        author: &PeerId,
        timestamp: &Timestamp,
    ) -> bincode::Result<Vec<u8>> {
        let mut v = vec![];
        let payload_bytes = bincode::serialize(&payload)?;
        v.extend(payload_bytes);
        let kind_bytes = bincode::serialize(&event_kind)?;
        v.extend(kind_bytes);
        let author_bytes = bincode::serialize(&author)?;
        v.extend(author_bytes);
        let timestamp_bytes = bincode::serialize(&timestamp)?;
        v.extend(timestamp_bytes);
        Ok(v)
    }

    fn calculate_hash(
        payload: &TPayload,
        event_kind: &Kind,
        author: &PeerId,
        timestamp: &Timestamp,
    ) -> bincode::Result<Hash> {
        let mut hasher = Blake2b512::new();
        hasher.update(Self::digest_from_parts(
            payload, event_kind, author, timestamp,
        )?);
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

    // TODO: actually have signature
    pub fn signature(&self) -> &Signature {
        self.hash()
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

    pub fn timestamp(&self) -> &u128 {
        &self.timestamp
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use hex_literal::hex;

    use super::*;

    fn create_events() -> Result<Vec<Event<i32>>, bincode::Error> {
        let mock_parents_1 = Parents {
            self_parent: Hash {
                inner: hex!(
                    "021ced8799296ceca557832ab941a50b4a11f83478cf141f51f933f653ab9fbc
                c05a037cddbed06e309bf334942c4e58cdf1a46e237911ccd7fcf9787cbc7fd0"
                ),
            },
            other_parent: Hash {
                inner: hex!(
                    "a231788464c1d56aab39b098359eb00e2fd12622d85821d8bffe68fdb3044f24
                370e750986e6e4747f6ec0e051ae3e7d2558f7c4d3c4d5ab57362e572abecb36"
                ),
            },
        };
        let mock_parents_2 = Parents {
            self_parent: Hash {
                inner: hex!(
                    "8a64b55fcfa60235edf16cebbfb36364d6481c3c5ec4de987114ed86c8f252c22
                3fadfa820edd589d9c723f032fdf6c9ca95f2fd95c4ffc01808812d8c1bafea"
                ),
            },
            other_parent: Hash {
                inner: hex!(
                    "c3ea7982719e7197c63842e41427f358a747e96c7a849b28604569ea101b0bdc5
                6cba63e4a60b95cb29bce01c2e7e3f918d60fa35aa90586770dfc699da0361a"
                ),
            },
        };
        let results = vec![
            Event::new(0, Kind::Genesis, 0, 0)?,
            Event::new(0, Kind::Genesis, 1, 0)?,
            Event::new(0, Kind::Regular(mock_parents_1.clone()), 0, 0)?,
            Event::new(0, Kind::Regular(mock_parents_2.clone()), 0, 0)?,
            Event::new(
                0,
                Kind::Regular(Parents {
                    self_parent: mock_parents_1.self_parent.clone(),
                    other_parent: mock_parents_2.other_parent.clone(),
                }),
                0,
                0,
            )?,
            Event::new(
                0,
                Kind::Regular(Parents {
                    self_parent: mock_parents_2.self_parent.clone(),
                    other_parent: mock_parents_1.other_parent.clone(),
                }),
                0,
                0,
            )?,
            Event::new(1234567, Kind::Genesis, 0, 0)?,
            Event::new(1234567, Kind::Regular(mock_parents_1.clone()), 0, 1)?,
        ];
        Ok(results)
    }

    #[test]
    fn events_create() {
        create_events().unwrap();
        // also test on various payloads
        Event::new((), Kind::Genesis, 0, 0).unwrap();
        Event::new((0,), Kind::Genesis, 0, 0).unwrap();
        Event::new(vec![()], Kind::Genesis, 0, 0).unwrap();
        Event::new("asdassa", Kind::Genesis, 0, 0).unwrap();
        Event::new("asdassa".to_owned(), Kind::Genesis, 0, 0).unwrap();
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