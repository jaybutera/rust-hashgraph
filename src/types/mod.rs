use serde::{Deserialize, Serialize};

pub mod event;
pub mod graph;

#[derive(Serialize, Deserialize, Clone)] // TODO: Does this need clone?
pub struct Transaction;

pub type RoundNum = usize;
