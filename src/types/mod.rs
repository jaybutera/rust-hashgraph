use serde::{Serialize,Deserialize};

pub mod graph;
pub mod event;

#[derive(Serialize, Deserialize, Clone)] // TODO: Does this need clone?
pub struct Transaction;

pub type RoundNum = usize;
