use thiserror::Error;

use crate::{algorithm::event, Timestamp};

/// Stores finalized/ordered events. We know the order of events that a
/// decided round "sees" (not actually sees as definition says but has
/// the round as ancestor, it's mostly a formality though). Such round
/// is also called `round received`
pub struct OrderedEvents {
    // None - no rounds were ordered (because first round is 0)
    latest_ordered_round: Option<usize>,
    // Events ordered according to algorithm
    events: Vec<OrderedEventsEntry>,
    // For iteration
    next_element_to_access: usize,
}

// Ordering-related data about event
struct OrderedEventsEntry {
    hash: event::Hash,
    // Do we need to store these fields??

    // round_received: usize,
    // consensus_timestamp: u64,        // ???
    // whitened_signature: event::Hash, // TODO: use actual signature
}

impl OrderedEvents {
    pub fn new() -> Self {
        Self {
            latest_ordered_round: None,
            events: vec![],
            next_element_to_access: 0,
        }
    }

    /// We need to supply events by each subsequent `round received`
    ///
    /// `events` contains `(event hash, consensus timestamp, event signature)`
    /// `unique_famous_witness_sigs` is needed for sig whitening
    pub fn add_received_round(
        &mut self,
        round: usize,
        events: impl Iterator<Item = (event::Hash, Timestamp, event::Signature)>, // TODO: use newtypes where applicable
        unique_famous_witness_sigs: Vec<event::Signature>,
    ) -> Result<(), RoundAddError> {
        self.verify_round_number(round)?;

        // XOR is associative + commutative, so we can combine ufw sigs
        // to not recompute it each time
        let ufw_sigs_combined = Self::combine_sigs_xor(unique_famous_witness_sigs.into_iter());
        let mut events: Vec<_> = events
            .map(|(a, b, sig)| (a, b, sig ^ &ufw_sigs_combined))
            .collect();

        // Sort according to paper: first by timestamp then by whitened signature
        events.sort_by(
            |(_, timestamp1, whitened_sig1), (_, timestamp2, whitened_sig2)| {
                timestamp1
                    .cmp(timestamp2)
                    .then(whitened_sig1.cmp(whitened_sig2))
            },
        );

        // Update state
        let mut events: Vec<_> = events
            .into_iter()
            .map(
                |(hash, _consensus_timestamp, _whitened_signature)| OrderedEventsEntry {
                    hash,
                    // round_received: round,
                    // consensus_timestamp,
                    // whitened_signature,
                },
            )
            .collect();
        self.events.append(&mut events);
        self.latest_ordered_round = Some(self.latest_ordered_round.map_or(0, |r| r + 1));
        Ok(())
    }

    pub fn next_round_to_order(&self) -> usize {
        match self.latest_ordered_round {
            Some(ordered) => ordered + 1,
            None => 0,
        }
    }

    pub fn next_event(&mut self) -> Option<&event::Hash> {
        let event_data = self.events.get(self.next_element_to_access)?;
        self.next_element_to_access += 1;
        Some(&event_data.hash)
    }

    fn verify_round_number(&self, r: usize) -> Result<(), RoundAddError> {
        (r == self.next_round_to_order())
            .then_some(())
            .ok_or(RoundAddError::IncorrectRoundNumber)
    }

    fn combine_sigs_xor(sigs: impl Iterator<Item = event::Signature>) -> event::Signature {
        sigs.into_iter().fold(
            event::Signature(event::Hash::from_array([0u8; 64])),
            |acc, next| acc ^ &next,
        )
    }
}

#[derive(Error, Debug)]
pub enum RoundAddError {
    #[error("Round right after previously supplied should be provided")]
    IncorrectRoundNumber,
}
