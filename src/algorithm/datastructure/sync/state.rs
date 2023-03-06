//! Generation of data that allows more efficient sync.
//!
//! In particular, generates compressed version of known state.
//! It means state with only some event hashes present. In best case of
//! honest peers (no forking) it includes only log(N) of event hashes,
//! where N is number of events created by the peer.
use std::collections::HashMap;

use derive_getters::Getters;
use itertools::unfold;
use thiserror::Error;

use crate::{algorithm::event, PeerId};

use crate::algorithm::datastructure::peer_index::{
    fork_tracking::{Extension, ForkIndexEntry},
    PeerIndex, PeerIndexEntry,
};

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

impl CompressedKnownState {
    pub fn as_vec(&self) -> &Vec<(PeerId, CompressedPeerState)> {
        &self.peer_states
    }
}

#[derive(Getters)]
pub struct CompressedPeerState {
    origin: event::Hash,
    // Indexed by starting event
    entries: HashMap<event::Hash, CompressedStateEntry>,
    // Start of entries in `data` by ends of corresponding
    // sections
    entries_starts: HashMap<event::Hash, event::Hash>,
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
        let data_starts: HashMap<_, _> = data
            .iter()
            .map(|(start, entry)| (entry.section().last.clone(), start.clone()))
            .collect();
        Ok(Self {
            origin: value.origin().clone(),
            entries: data,
            entries_starts: data_starts,
        })
    }
}

pub enum CompressedStateEntry {
    /// Sequence of events with fork at the end (in case of dishonest author)
    Fork {
        events: EventsSection,
        /// First elements of each child
        forks: Vec<event::Hash>,
    },
    /// Just a sequence of events
    Leaf(EventsSection),
}

impl CompressedStateEntry {
    pub fn section(&self) -> &EventsSection {
        match self {
            CompressedStateEntry::Fork { events, .. } => events,
            CompressedStateEntry::Leaf(events) => events,
        }
    }
}

/// Helps to roughly figure out how many events are known in a chain (and to send)
/// without transferring all of the events.
#[derive(Debug, Clone, PartialEq)]
pub struct EventsSection {
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
    // TODO: reverse this, I wanted H-S, H-2S, H-4S, H-8S... until the start of the section
    intermediates: Vec<(usize, event::Hash)>,
}

impl EventsSection {
    // temp to update tests later??
    pub fn intermediates(&self) -> &Vec<(usize, event::Hash)> {
        &self.intermediates
    }

    pub fn submultiple(&self) -> u32 {
        self.submultiple
    }
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
                .map(|i| value.multiples().get_ith(i))
                .take_while(|x| x.is_some())
                .filter_map(|x| x)
                .collect()
        } else {
            vec![]
        };
        let intermediates = intermediates
            .into_iter()
            .map(|(a, b)| (a.clone(), b.clone()))
            .collect();
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
    use std::{collections::HashSet, num::NonZeroU8, time::Duration};

    use hex_literal::hex;

    use super::*;

    const FAKE_HASHES: [[u8; 64]; 15] = [
        hex!["411f1010e94a282a038a3fa1d18a6956361d9918be2b827308042764299ea995ccfdcb6f2d49187c869b9ed87560e136dc7b6671a37909617432aa8595106a8b"],
        hex!["0aca064a733c3d97be52cb47fb8a336823da4ad7dff114352f17f78d1a16c6b0fa67260a84b4913fbcad55357599fc3d943172b400b6551692857004bda4ff87"],
        hex!["ec10b0e7a15b1beb12e87ebe1c461b2ee276f5e151545d8992e1daaa8f61e1d46c913baa90854a34e755bd2545631ff3f300bffe541bff6adb240ab43d0ffbb7"],
        hex!["43632b90d55f060ee55943350d324dce6d3c40d826f9279ef5a5a688380a3227660ce295b4c6c3e1d50c918b7e2eab18613c7ed5cd521c172b7cc20db5a9e847"],
        hex!["e3ba6dbad32d3ca4c186aa40cbe294181c4cdb77ba95204ac97d7d79635a139a967ef7f09e72737eb10bdd0776784726efcd07e0b7143966990c22d5d97083e0"],
        hex!["fba6f9b0a0ea6785abc7a368227604c668a7e3ca2dca6c95998729b867489509247a27786a26c7ab33b13ef1e020eb4caa66780d8caa03e47e2238f9f0eff69d"],
        hex!["051207ac1bdb6fca1f0f784609ffd213c7a95730ee9ef209fb7828d94eb6898d09c99c0a0c2e90fc23f19554ad9c5f2fa00aedc430fc54b8aa7e16fd7cc47830"],
        hex!["62c8b3617313924ff319a39706524797eb8bfe70232b10f21aee8b1a5ca62d120de06b09d0facb4ad2ebc0462342e2e8497514ffc51345b50d26558a2dde53f9"],
        hex!["819f8d3a5781660b111982f33cab86c3384148c3887654181b0fbe4fc25b59f005b5c8142d3c476954745ade577b34dd68c194c696ff6adad8aaded9341fdc44"],
        hex!["2d28a3f1a77c924fe860f481f9b1388cb3dcc20486b0177299371c7463495e869d1f27721753f84ad59371cfa592860c7a75bae69c28dae7e49a5927b9135440"],
        hex!["4a75eb97c22ec64b3659eb65f1f6350d39edafdb59a7966cc8c5553972ee7e61ba7d124e78b9cbacf3a76e151349352bec36e214ebc17df4221c4a3a01a09e4d"],
        hex!["1530b07b6edf74fc9dea6929d2c801b3e9b197e365bd349db4ec7eec08927e0e537179e8028281851eeb8632203a42e86138aabe1710d9d16f6807bb09c1b000"],
        hex!["55571800a2bcadebeba16b2cdd39f6e390482ee1cfb4550a4b21818173f875697ae841ad78ed8b94e3363a793f3e7a1f4b4f5b3d821b67f81869413b306d2a2d"],
        hex!["75305afe9bf50e38c97e53d01ed69e6b596815dee321c26d1f12f5ec51e9c3009e303bdafa88e2f5887290c0f15f69e29888a70145632b6df100f3cabbbc23a6"],
        hex!["e88814dfb0055827c5159f2b16d57f72c74294d1128dc47827ea613c48cabb44cd03d9d4443b384c4b675771a20e4a25e522a235cd36b6398268b09d3836e7c4"],
    ];

    #[test]
    fn section_constructs_correctly() {
        let fake_event_hashes_init = FAKE_HASHES.iter().map(|h| event::Hash::from_array(*h));
        // For 15 events and submultiple 3, the section should save hashes of
        // events at heights 0, 3, 6, 12.
        let section_1 = {
            let mut fake_event_hashes = fake_event_hashes_init.clone();
            let mut ext = Extension::from_event_with_submultiple(
                fake_event_hashes.next().unwrap().clone(),
                0,
                3,
            )
            .unwrap();
            for h in fake_event_hashes {
                ext.push_event(h);
            }
            EventsSection::from(&ext)
        };
        // test for 13 events (at height 12 is the last one)
        let section_2 = {
            let mut fake_event_hashes = fake_event_hashes_init.clone().take(13);
            let mut ext = Extension::from_event_with_submultiple(
                fake_event_hashes.next().unwrap().clone(),
                0,
                3,
            )
            .unwrap();
            for h in fake_event_hashes {
                ext.push_event(h);
            }
            EventsSection::from(&ext)
        };

        let mut fake_event_hashes = fake_event_hashes_init.clone();
        let expected = vec![
            (0, fake_event_hashes.nth(0).unwrap()),
            (3, fake_event_hashes.nth(2).unwrap()),
            (6, fake_event_hashes.nth(2).unwrap()),
            (12, fake_event_hashes.nth(5).unwrap()),
        ];

        assert_eq!(section_1.intermediates(), &expected);

        assert_eq!(section_2.intermediates(), &expected);

        // Start height is not necessary 0
        let section_3 = {
            let mut fake_event_hashes = fake_event_hashes_init.clone().take(15);
            let mut ext = Extension::from_event_with_submultiple(
                fake_event_hashes.next().unwrap().clone(),
                11,
                3,
            )
            .unwrap();
            for h in fake_event_hashes {
                ext.push_event(h);
            }
            EventsSection::from(&ext)
        };

        let mut fake_event_hashes = fake_event_hashes_init.clone();
        let expected = vec![
            (12, fake_event_hashes.nth(1).unwrap()),
            (15, fake_event_hashes.nth(2).unwrap()),
            (18, fake_event_hashes.nth(2).unwrap()),
            (24, fake_event_hashes.nth(5).unwrap()),
        ];
        assert_eq!(section_3.intermediates(), &expected);
    }

    #[test]
    fn peer_state_constructs_correctly() {
        // Let's do something like
        //    1-2-3-4-5-6-7-8-9-10-11-12-13-14-15
        //   /
        //  0
        //   \
        //    16

        // arbitrary values
        let peer = 3u64;
        let start_time = Duration::from_secs(1674549572);
        let mut all_events = HashMap::new();

        // Since we work with actual events, we can't use our sample hashes.

        // init
        let genesis_event =
            event::Event::new((), event::Kind::Genesis, peer, start_time.as_secs().into()).unwrap();
        let genesis_hash = genesis_event.hash().clone();
        all_events.insert(genesis_hash.clone(), genesis_event.clone());
        let mut index = PeerIndexEntry::new(genesis_hash.clone(), NonZeroU8::new(3u8).unwrap());
        let mut tracked_events = vec![(0, genesis_hash.clone())];

        // events 1..=13
        let mut prev_hash = genesis_hash.clone();
        for i in 1usize..=15 {
            let event = event::Event::new(
                (),
                event::Kind::Regular(event::Parents {
                    self_parent: prev_hash.clone(),
                    // doesn't matter what to put here, we don't test it at all
                    other_parent: prev_hash.clone(),
                }),
                peer,
                (start_time + Duration::from_secs(i.try_into().unwrap()))
                    .as_secs()
                    .into(),
            )
            .unwrap();
            all_events
                .get_mut(&prev_hash)
                .unwrap()
                .children
                .self_child
                .add_child(event.hash().clone());
            index
                .add_event(prev_hash, event.hash().clone(), |h| all_events.get(h))
                .unwrap();
            prev_hash = event.hash().clone();
            if i % 3 == 0 {
                tracked_events.push((i, event.hash().clone()));
            }
            all_events.insert(event.hash().clone(), event);
        }

        // at this point it should track 0, 3, 6, 12
        // (as in `section_constructs_correctly`)

        let peer_state = CompressedPeerState::try_from(&index).unwrap();
        assert_eq!(peer_state.origin, genesis_hash);
        let mut entries = peer_state.entries.iter();

        let Some((start_hash, entry)) = entries.next()
        else {
            panic!("expected at least one entry to be produced");
        };
        assert_eq!(start_hash, &genesis_hash);

        let CompressedStateEntry::Leaf(ext) = entry
        else {
            panic!("no forks were inserted yet");
        };
        assert_eq!(
            ext.intermediates(),
            &vec![
                tracked_events[0].clone(),
                tracked_events[1].clone(),
                tracked_events[2].clone(),
                tracked_events[4].clone()
            ]
        );

        // the last 16'th event

        let event_16 = event::Event::new(
            (),
            event::Kind::Regular(event::Parents {
                self_parent: genesis_hash.clone(),
                // doesn't matter what to put here, we don't test it at all
                other_parent: genesis_hash.clone(),
            }),
            peer,
            (start_time + Duration::from_secs(16)).as_secs().into(),
        )
        .unwrap();
        all_events
            .get_mut(&genesis_hash)
            .unwrap()
            .children
            .self_child
            .add_child(event_16.hash().clone());
        index
            .add_event(genesis_hash.clone(), event_16.hash().clone(), |h| {
                all_events.get(h)
            })
            .unwrap();
        all_events.insert(event_16.hash().clone(), event_16.clone());

        // Now the largest section should track only 3, 6, and 12
        // (since 0 is in another section now)

        let peer_state = CompressedPeerState::try_from(&index).unwrap();
        assert_eq!(peer_state.origin, genesis_hash);

        let Some(entry) = peer_state.entries.get(&genesis_hash)
        else {
            panic!("should produce corresponding structure");
        };

        let CompressedStateEntry::Fork { events: ext, forks } = entry
        else {
            panic!("we inserted a fork, so it should be tracked");
        };
        // less than 15, so none are tracked
        assert_eq!(&ext.intermediates()[..], &vec![]);

        // Test the smaller leaf first (with event 16)
        let mut forks = HashSet::<_>::from_iter(forks.into_iter());
        assert!(forks.remove(event_16.hash()));

        let Some(entry) = peer_state.entries.get(&event_16.hash())
        else {
            panic!("should produce corresponding structure");
        };

        let CompressedStateEntry::Leaf(ext) = entry
        else {
            panic!("no more forks on the branch");
        };
        assert_eq!(ext.intermediates(), &vec![]);

        // Now the larger one
        let mut forks = forks.into_iter();
        let last_fork_hash = forks
            .next()
            .expect("forks must branch to at least 2 sections");
        assert!(matches!(forks.next(), None));
        let gen_children_without_16: Vec<_> = all_events
            .get(&genesis_hash)
            .unwrap()
            .children
            .self_child
            .clone()
            .with_child_removed(event_16.hash())
            .into();
        let mut gen_children_without_16 = gen_children_without_16.into_iter();
        let Some(event_1_hash) = gen_children_without_16.next()
        else {
            panic!("only inserted 2 children to genesis");
        };
        assert_eq!(gen_children_without_16.next(), None);
        assert_eq!(last_fork_hash, &event_1_hash);

        let Some(entry) = peer_state.entries.get(&last_fork_hash)
        else {
            panic!("should produce corresponding structure");
        };

        let CompressedStateEntry::Leaf(ext) = entry
        else {
            panic!("no more forks on the branch");
        };
        assert_eq!(
            &ext.intermediates()[..],
            &vec![
                tracked_events[1].clone(),
                tracked_events[2].clone(),
                tracked_events[3].clone(),
                tracked_events[5].clone()
            ]
        );
    }
}
