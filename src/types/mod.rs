use serde::Serialize;

pub mod graph;
pub mod event;

#[derive(Serialize)]
pub struct Transaction;

pub type RoundNum = usize;
