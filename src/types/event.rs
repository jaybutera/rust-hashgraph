use serde::{Serialize,Deserialize};
use crypto::sha3::Sha3;
use crypto::digest::Digest;
use super::Transaction;

#[derive(Serialize, Deserialize, Clone)] // Clone is temporary for graph unit tests
pub enum Event {
    Update {
        creator: String, // TODO: Change to a signature
        self_parent: String,
        other_parent: Option<String>,
        txs: Vec<Transaction>,
    },
    Genesis{creator: String},
}

impl Event {
    pub fn hash(&self) -> String {
        let mut hasher = Sha3::sha3_256();
        let serialized = serde_json::to_string(self).unwrap();
        hasher.input_str(&serialized[..]);
        hasher.result_str()
    }
}

