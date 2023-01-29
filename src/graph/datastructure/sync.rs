use std::collections::HashMap;

use itertools::unfold;
use thiserror::Error;

use crate::{graph::event, PeerId};

use super::peer_index::fork_tracking::{Extension, ForkIndexEntry};
use super::{PeerIndex, PeerIndexEntry};

#[derive(Debug, Error, PartialEq)]
#[error("Submultipliers within fork index differ")]
pub struct SubmultiplierMismatch;

pub struct CompressedKnownState {
    peer_states: Vec<(PeerId, CompressedPeerState)>,
}

impl TryFrom<&PeerIndex> for CompressedKnownState {
    type Error = SubmultiplierMismatch;

    fn try_from(value: &PeerIndex) -> Result<Self, Self::Error> {
        let peer_states: Result<Vec<_>, _> = value
            .into_iter()
            .map(|(id, entry)| CompressedPeerState::try_from(entry).map(|s| (*id, s)))
            .collect();
        let peer_states = peer_states?;
        Ok(Self { peer_states })
    }
}

pub struct CompressedPeerState {
    origin: event::Hash,
    // Indexed by starting event
    data: HashMap<event::Hash, CompressedStateEntry>,
}

impl TryFrom<&PeerIndexEntry> for CompressedPeerState {
    type Error = SubmultiplierMismatch;

    fn try_from(value: &PeerIndexEntry) -> Result<Self, Self::Error> {
        let submul = value.forks_submultiple();
        let data_res: Result<HashMap<_, _>, _> = value
            .forks()
            .map(|entry| {
                let section = <EventsSection>::from(entry.extension());
                if section.submultiple != u8::from(submul).into() {
                    return Err(SubmultiplierMismatch);
                }
                let index = section.first.clone();
                let new_entry = match entry {
                    ForkIndexEntry::Fork(fork) => CompressedStateEntry::Fork {
                        events: section,
                        forks: fork.subsequent_forks().clone(),
                    },
                    ForkIndexEntry::Leaf(_) => CompressedStateEntry::Leaf(section),
                };
                Ok((index, new_entry))
            })
            .collect();
        let data = data_res?;
        Ok(Self {
            origin: value.origin().clone(),
            data,
        })
    }
}

enum CompressedStateEntry {
    /// Sequence of events with fork at the end (in case of dishonest author)
    Fork {
        events: EventsSection,
        /// First elements of each child
        forks: Vec<event::Hash>,
    },
    /// Just a sequence of events
    Leaf(EventsSection),
}

/// Helps to roughly figure out how many events are known in a chain (and to send)
/// without transferring all of the events.
#[derive(Debug, Clone, PartialEq)]
struct EventsSection {
    first: event::Hash,
    first_height: usize,
    last: event::Hash,
    length: usize,
    intermediate_threshold: u32,
    submultiple: u32,
    /// `(index, hash)`
    ///
    /// Empty if `length` < `intermediate_threshold`
    ///
    /// if `length` >= `intermediate_threshold`, the first element is the first
    /// whose height `H` is a multiple of `submultiple` (`S`), the next one is with height
    /// `H+S`, then `H+2S, H+4S, H+8S, ...` until the end of the section is reached
    intermediates: Vec<(usize, event::Hash)>,
}

impl From<&Extension> for EventsSection {
    fn from(value: &Extension) -> Self {
        let submul = value.submultiple();
        let threshold = submul.saturating_div(2).saturating_add(submul);
        // Take multiples at positions
        // 0, 1, 2, 4, 8, 16, 32, ... until none left
        let intermediates = if value.length() >= &(threshold as usize) {
            let powers_of_2 = unfold(1, |x| {
                let next = *x;
                *x *= 2;
                Some(next)
            });
            // also 0 at the start since we index from zero
            let indexes = [0].into_iter().chain(powers_of_2);
            indexes
                .map(|i| value.multiples().get(i))
                .take_while(|x| x.is_some())
                .filter_map(|x| x)
                .collect()
        } else {
            vec![]
        };
        Self {
            first: value.first_event().clone(),
            first_height: value.first_height().clone(),
            last: value.last_event().clone(),
            length: value.length().clone(),
            intermediate_threshold: threshold.into(),
            submultiple: submul.into(),
            intermediates,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_constructs_correctly() {
        // let Extension
    }
}
