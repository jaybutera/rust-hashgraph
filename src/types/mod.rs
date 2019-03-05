use serde::Serialize;

pub mod graph;
pub mod event;

#[derive(Serialize, Clone)] // TODO: Does this need clone?
pub struct Transaction;

pub type RoundNum = usize;
