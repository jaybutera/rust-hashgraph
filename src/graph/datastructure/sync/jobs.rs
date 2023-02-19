use std::collections::HashMap;

use crate::{
    graph::{EventKind, Signature},
    PeerId, Timestamp,
};

/// Sync jobs that need to be applied in order to achieve (at least)
/// the same knowledge as sender.
///
/// "at least" - because the receiver might know some more data (mostly
/// technicality)
pub struct Jobs<TPayload> {
    additions: Vec<AddEvent<TPayload>>,
}

pub struct AddEvent<TPayload> {
    pub signature: Signature,
    pub payload: TPayload,
    pub event_type: EventKind,
    pub author: PeerId,
    pub time_created: Timestamp,
}
