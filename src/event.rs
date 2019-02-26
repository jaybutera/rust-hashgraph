use serde::Serialize;
use crypto::sha3::Sha3;
use crypto::digest::Digest;

#[derive(Serialize)]
pub struct Transaction;

#[derive(Serialize)]
pub enum Event {
    Update {
        creator: String, // TODO: Change to a signature
        self_parent: String,
        other_parent: String,
        txs: Vec<Transaction>,
        to: String, // TODO: Temporary info just for simulation of multiple machines in one program
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

