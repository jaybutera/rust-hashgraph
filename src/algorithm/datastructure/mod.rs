use itertools::izip;
use serde::Serialize;
use thiserror::Error;
use tracing::{debug, error, instrument, trace, warn};

use std::collections::{HashMap, HashSet, VecDeque};

use self::ordering::OrderedEvents;
use self::peer_index::{EventIndex, PeerIndex, PeerIndexEntry};
use self::slice::SliceIterator;
use super::event::{self, Event, Parents};
use super::{EventKind, PushError, RoundNum};
use crate::{PeerId, Timestamp};

mod ordering;
mod peer_index;
mod slice;
mod sync;

#[derive(Debug, PartialEq, Clone)]
enum WitnessFamousness {
    Yes,
    No,
    Undecided,
}

#[derive(Debug, PartialEq, Clone)]
enum WitnessUniqueFamousness {
    FamousUnique,
    FamousNotUnique,
    NotFamous,
    Undecided,
}

#[derive(Debug, PartialEq, Clone)]
struct UniquenessNotProvided;

impl TryFrom<WitnessFamousness> for WitnessUniqueFamousness {
    type Error = UniquenessNotProvided;

    fn try_from(value: WitnessFamousness) -> Result<Self, Self::Error> {
        match value {
            WitnessFamousness::Yes => Err(UniquenessNotProvided),
            WitnessFamousness::No => Ok(WitnessUniqueFamousness::NotFamous),
            WitnessFamousness::Undecided => Ok(WitnessUniqueFamousness::Undecided),
        }
    }
}

impl WitnessUniqueFamousness {
    fn from_famousness(value: WitnessFamousness, unique: bool) -> Self {
        match (value, unique) {
            (WitnessFamousness::Yes, true) => WitnessUniqueFamousness::FamousUnique,
            (WitnessFamousness::Yes, false) => WitnessUniqueFamousness::FamousNotUnique,
            (WitnessFamousness::No, _) => WitnessUniqueFamousness::NotFamous,
            (WitnessFamousness::Undecided, _) => WitnessUniqueFamousness::Undecided,
        }
    }
}

impl From<WitnessUniqueFamousness> for WitnessFamousness {
    fn from(value: WitnessUniqueFamousness) -> Self {
        match value {
            WitnessUniqueFamousness::FamousUnique | WitnessUniqueFamousness::FamousNotUnique => {
                WitnessFamousness::Yes
            }
            WitnessUniqueFamousness::NotFamous => WitnessFamousness::No,
            WitnessUniqueFamousness::Undecided => WitnessFamousness::Undecided,
        }
    }
}

#[derive(Error, Debug, PartialEq)]
#[error("Event with such hash is unknown to the graph (hash {0})")]
pub struct UnknownEvent(event::Hash);

#[derive(Error, Debug, PartialEq)]
pub enum WitnessCheckError {
    #[error("This event is not a witness")]
    NotWitness,
    #[error(transparent)]
    Unknown(#[from] UnknownEvent),
}

#[derive(Error, Debug, PartialEq)]
pub enum RoundUfwListError {
    #[error("Round with this number is unknown yet")]
    UnknownRound,
    #[error("Fame of some witnesses in the round is undecided")]
    RoundUndecided,
}

#[derive(Error, Debug, PartialEq)]
pub enum OrderingDataError {
    #[error("Ordering for the event is undecided")]
    Undecided,
    #[error(transparent)]
    UnknownEvent(#[from] UnknownEvent),
}

#[derive(Error, Debug, PartialEq)]
pub enum OrderedEventsError {
    #[error("Provided round number is not present in the graph")]
    UnknownRound,
    #[error("Given round is undecided")]
    UndecidedRound,
}

pub struct Graph<TPayload> {
    all_events: EventIndex<Event<TPayload>>,
    peer_index: PeerIndex,
    /// Consistent and reliable index (should be)
    round_index: Vec<HashSet<event::Hash>>,
    /// Some(false) means unfamous witness
    witnesses: HashMap<event::Hash, WitnessFamousness>,
    /// Cache, shouldn't be relied upon (however seems as reliable as `round_index`)
    round_of: HashMap<event::Hash, RoundNum>,
    ordering_data_cache: HashMap<event::Hash, (usize, Timestamp, event::Hash)>,
    /// The latest round known to have its fame decided. All previous rounds
    /// must be decided as well.
    ///
    /// If `None` - no rounds decided yet
    last_known_decided_round: Option<usize>,
    ordering: OrderedEvents,

    // probably move to config later
    self_id: PeerId,
    /// Coin round frequency
    coin_frequency: usize,
}

impl<TPayload> Graph<TPayload>
where
    TPayload: Serialize + Eq + std::hash::Hash,
{
    pub fn new(
        self_id: PeerId,
        genesis_payload: TPayload,
        genesis_timestamp: Timestamp,
        coin_frequency: usize,
    ) -> Self {
        let mut graph = Self {
            all_events: HashMap::new(),
            peer_index: HashMap::new(),
            self_id,
            round_index: vec![HashSet::new()],
            witnesses: HashMap::new(),
            round_of: HashMap::new(),
            ordering_data_cache: HashMap::new(),
            last_known_decided_round: None,
            ordering: OrderedEvents::new(),
            coin_frequency,
        };

        graph
            .push_event(
                genesis_payload,
                EventKind::Genesis,
                self_id,
                genesis_timestamp,
            )
            .expect("Genesis events should be valid");
        graph
    }

    /// Create and push event to the graph, adding it at the end of `author`'s lane
    /// (i.e. the event becomes the latest one of the peer).
    #[instrument(level = "error", skip_all)]
    #[instrument(level = "trace", skip(self, payload))]
    pub fn push_event(
        &mut self,
        payload: TPayload,
        event_type: EventKind,
        author: PeerId,
        time_created: Timestamp,
    ) -> Result<event::Hash, PushError> {
        // Verification first, no changing state
        debug!("Validating the event");

        trace!("Creating an event");
        let new_event = match event_type {
            EventKind::Genesis => Event::new(payload, event::Kind::Genesis, author, time_created)?,
            EventKind::Regular(Parents {
                self_parent,
                other_parent,
            }) => Event::new(
                payload,
                event::Kind::Regular(Parents {
                    self_parent,
                    other_parent,
                }),
                author,
                time_created,
            )?,
        };
        trace!("Event hash: {}", new_event.hash());

        trace!("Testing if event is already known");
        if self.all_events.contains_key(new_event.hash()) {
            return Err(PushError::EventAlreadyExists(new_event.hash().clone()));
        }

        trace!("Performing checks or updates specific to genesis or regular events");
        match new_event.parents() {
            event::Kind::Genesis => {
                trace!("It is a genesis event");
                if self.peer_index.contains_key(&author) {
                    return Err(PushError::GenesisAlreadyExists);
                }
                debug!("The event is valid, updating state to include it");
                let new_peer_index = PeerIndexEntry::new(
                    new_event.hash().clone(),
                    std::num::NonZeroU8::new(10u8).unwrap(),
                );
                self.peer_index.insert(author, new_peer_index);
            }
            event::Kind::Regular(parents) => {
                trace!("It is a regular event");
                trace!("Checking presence of parents");
                if !self.all_events.contains_key(&parents.self_parent) {
                    return Err(PushError::NoParent(parents.self_parent.clone()));
                }
                if !self.all_events.contains_key(&parents.other_parent) {
                    return Err(PushError::NoParent(parents.other_parent.clone()));
                }

                // taking mutable for update later
                let self_parent_event = self
                    .all_events
                    .get_mut(&parents.self_parent) // TODO: use get_many_mut when stabilized
                    .expect("Just checked self parent presence");

                // self parent must have the same author by definition
                trace!("Author validation with self parent");
                if self_parent_event.author() != &author {
                    debug!(
                        "Specified self parent author ({}) differs from provided one ({})",
                        self_parent_event.author(),
                        author
                    );
                    return Err(PushError::IncorrectAuthor(
                        self_parent_event.author().clone(),
                        author,
                    ));
                }

                // taking mutable for update later
                let author_index = self
                    .peer_index
                    .get_mut(&author)
                    .ok_or(PushError::PeerNotFound(author))?;

                // Insertion, should be valid at this point so that we don't leave in inconsistent state on error.
                debug!("The event is valid, updating state to include it");
                // TODO: move validation in a diff function

                trace!("Updating pointers of parents");
                self_parent_event
                    .children
                    .self_child
                    .add_child(new_event.hash().clone());
                if let Err(e) = author_index.add_event(
                    self_parent_event.hash().clone(),
                    new_event.hash().clone(),
                    |h| self.all_events.get(h),
                ) {
                    warn!("Peer index insertion error: {}", e);
                }
                let other_parent_event = self
                    .all_events
                    .get_mut(&parents.other_parent)
                    .expect("Just checked other parent presence");
                other_parent_event
                    .children
                    .other_children
                    .push(new_event.hash().clone());
            }
        };

        // Index the event and save
        trace!("Tracking the event");
        let hash = new_event.hash().clone();
        self.all_events.insert(hash.clone(), new_event);

        // Set round
        trace!("Calculating round");
        let last_idx = self.round_index.len() - 1;
        let r = self
            .determine_round(&hash)
            .expect("The event was just added to tracking");
        // Cache result
        trace!("Caching the result");
        self.round_of.insert(hash.clone(), r);
        if r > last_idx {
            // Create a new round
            trace!("Creating new round in index");
            let mut round_hs = HashSet::new();
            round_hs.insert(hash.clone());
            self.round_index.push(round_hs);
        } else {
            // Otherwise push onto appropriate round
            trace!("Inserting event into existing round index");
            self.round_index[r].insert(hash.clone());
        }

        // Set witness status
        trace!("Checking if the event is witness");
        if self
            .determine_witness(&hash)
            .expect("Just inserted to `all_events`")
        {
            debug!("Event is a witness, performing additional checks");
            trace!("Adding event to witness index");
            self.witnesses
                .insert(hash.clone(), WitnessFamousness::Undecided);

            // Update fame of previous rounds, if changed
            trace!("Updating fame and adding events to ordering");
            self.handle_ordering();
        } else {
            debug!("Event is not a witness");
        }
        Ok(hash)
    }

    pub fn next_event(&mut self) -> Option<&Event<TPayload>> {
        self.ordering.next_event().map(|hash| {
            self.all_events
                .get(hash)
                .expect("ordered events must be tracked")
        })
    }
}

/// Synchronization-related stuff.
/// details in [sync]
impl<TPayload> Graph<TPayload> {
    /// 1st sync step - (at reciever) generate compressed state.
    pub fn graph_known_state(
        &self,
    ) -> Result<sync::state::CompressedKnownState, sync::state::SubmultiplierMismatch> {
        let res = sync::state::CompressedKnownState::try_from(&self.peer_index);
        match &res {
            Ok(_state) => debug!("Successfully constructed compressed graph state"),
            Err(sync::state::SubmultiplierMismatch) => error!(
                "Fork index is inconsistent (submultipliers vary). Very strange, should not happen"
            ),
        };
        res
    }

    /// 2nd sync step - (at sender) find which updates the reciever must perform to get to the same
    /// (or higher) level of graph knowledge as we have.
    pub fn generate_sync_jobs(
        &self,
        peer_state: sync::state::CompressedKnownState,
    ) -> sync::jobs::Jobs<TPayload> {
        let events_unknown_by_receiver = {};
        todo!()
    }

    /// 3rd sync step - (at reciever) try to apply the sync steps provided by the sender.
    pub fn try_apply_sync(&mut self, jobs: sync::jobs::Jobs<TPayload>) -> Result<(), ()> {
        todo!()
    }
}

impl<TPayload> Graph<TPayload>
where
    TPayload: Eq + std::hash::Hash,
{
    #[instrument(level = "debug", skip_all)]
    /// Process stuff related to event ordering.
    ///
    /// In particular, checks if new events can be ordered and orders them.
    fn handle_ordering(&mut self) {
        if self.advance_rounds_decided() {
            let last_known_decided_round = match self.last_known_decided_round {
                Some(v) => v,
                None => return,
            };
            // We insert only events ordered by rounds with their fame decided.
            let new_rounds_that_order =
                self.ordering.next_round_to_order()..last_known_decided_round;
            debug!(
                "Sorting events ordered by rounds in range [{}, {})",
                new_rounds_that_order.start, new_rounds_that_order.end
            );
            for decided_round in new_rounds_that_order {
                match self.add_new_ordered_events(decided_round) {
                    Ok(()) => (),
                    Err(OrderedEventsError::UndecidedRound) => break,
                    Err(OrderedEventsError::UnknownRound) => {
                        panic!("Round marked as `last_known_decided_round` must be known")
                    }
                }
            }
        }
    }

    /// Round is decided if all known witnesses had their fame decided, for both
    /// round r and all earlier rounds (from the paper). Therefore it makes
    /// sense to check rounds for it one by one.
    ///
    /// This method checks if any new rounds satisfy this property and updates
    /// counter for event ordering.
    ///
    /// Returns true if some advancements were made
    #[instrument(level = "debug", skip(self))]
    fn advance_rounds_decided(&mut self) -> bool {
        let mut progress_made = false;
        let next_round_to_decide = self.last_known_decided_round.map(|a| a + 1).unwrap_or(0);
        debug!(
            "Starting from round {}, the previous one is known to be decided",
            next_round_to_decide
        );
        for checked_round in next_round_to_decide..self.round_index.len() {
            trace!("Checking round {}", checked_round);
            let round_witnesses = self
                .round_witnesses(checked_round)
                .expect("Round number is bounded by `round_index` size");
            for event_hash in round_witnesses {
                match self.is_famous_witness(event_hash) {
                    Ok(WitnessFamousness::Undecided) => {
                        debug!(
                            "Some witness of round {} is not decided, so {} is the next to be decided",
                            checked_round, self.last_known_decided_round.map(|a| a + 1).unwrap_or(0)
                        );
                        return progress_made;
                    }
                    Ok(_) => {
                        continue;
                    }
                    Err(WitnessCheckError::NotWitness) => {
                        error!("Witnesses index or witness check is broken, inconsistent state");
                        panic!("Events given by `round_witnesses` must be witnesses");
                    }
                    Err(WitnessCheckError::Unknown(_)) => {
                        error!("Witnesses index or something else is broken, inconsistent state");
                        panic!("Events given by `round_witnesses` must be known");
                    }
                }
            }
            debug!("Round {} is decided", checked_round);
            // At this point we know that round `checked_round` is decided.
            // So we update the stored value.
            self.last_known_decided_round = Some(
                self.last_known_decided_round
                    .map_or(checked_round, |d| d.max(checked_round)),
            );
            progress_made = true;
        }
        progress_made
    }

    /// # Description
    ///
    /// Update `ordering` index/tracker to include newly ordered events. These are events
    /// that have `round_received` <= `last_known_decided_round`. It is used to later
    /// provide events one by one.
    ///
    /// # Explanation
    ///
    /// Since events are sorted firstly by `round_received` and this number is set to `x`
    /// only when the round's ufws (unique famous witnesses) all have seen the `x`, it makes
    /// sense to order the events in batches after each round has its fame decided (all its
    /// ufws have fame value).
    ///
    /// Otherwise we cannot guarantee that some event unknown to ufws will not appear (which
    /// would mean that we might skipped it already).
    ///
    /// In other words, an event is finalized  when ufws of some round all see it. It implies
    /// (at least it seems so) that we can univocally find its place in order of all events.
    fn add_new_ordered_events(&mut self, decided_round: usize) -> Result<(), OrderedEventsError> {
        trace!("Handling events sorted by round {}", decided_round);
        match self.ordered_events(decided_round) {
            Ok(events) => {
                let ufw = self
                    .round_unique_famous_witnesses(decided_round)
                    .expect("round that orders events was already checked if its fame was decided");
                let unique_famous_witness_sigs = ufw
                    .into_iter()
                    .map(|e| {
                        self.all_events
                            .get(e)
                            .expect("witnesses must be tracked")
                            .signature()
                            .clone()
                    })
                    .collect();
                self.ordering
                    .add_received_round(
                        decided_round,
                        events.into_iter(),
                        unique_famous_witness_sigs,
                    )
                    .expect("just got round # from ordering, must be correct");
                Ok(())
            }
            Err(e) => {
                match e {
                    OrderedEventsError::UndecidedRound => warn!(
                        "No events ordered by round {} were found for some reason",
                        decided_round
                    ),
                    OrderedEventsError::UnknownRound => {
                        error!("Round {} is handled, but it is unknown!", decided_round);
                    }
                };
                Err(e)
            }
        }
    }

    // TODO: check if works properly with forks
    /// Get events to be ordered by `round` (in no particular order yet).
    #[instrument(level = "trace", skip(self))]
    fn ordered_events(
        &mut self,
        target_round_received: usize,
    ) -> Result<Vec<(event::Hash, Timestamp, event::Hash)>, OrderedEventsError> {
        // We want to find all events with `round_received` == `target_round_received`.
        // To do it we start from witnesses of round `target_round_received`, since
        // no later rounds can have their `round_received` <= than our value of interest.
        // Some peers might not have events (and thus witnesses) at the round at all,
        // so for such peers we take the last round (this case should be pretty rare, so
        // we will settle on this).
        //
        // Then we traverse the graph. We go to the self-ancestors only to avoid duplicate visits.
        // Therefore, we needed to have a starting event from each peer. This way we find the
        // events we're interested in.

        // We create a "slice" of the network at the round witnesses and go down to find events
        // ordered by it.
        trace!("Creating initial slice");
        let mut init_slice = self
            .round_witnesses(target_round_received)
            .ok_or(OrderedEventsError::UnknownRound)?;

        // Some peers might not have witnesses in this round, so we start from the end for them
        let slice_extension = {
            trace!("Extending the slice to include all peers");
            let peers_hit: HashSet<_> = init_slice
                .iter()
                .map(|h| {
                    self.all_events
                        .get(h)
                        .expect("witnesses must be tracked")
                        .author()
                })
                .collect();
            let mut extension = vec![];
            for (peer_id, index) in &self.peer_index {
                if !peers_hit.contains(&peer_id) {
                    extension.push(index.latest_events()[0]);
                }
            }
            trace!(
                "{} peers did not have witnesses in the round, adding the latest events for them",
                extension.len()
            );
            extension
        };
        init_slice.extend(slice_extension);

        trace!("Creating iterator");

        // Iterate from the witnesses or from the end until we reach events
        // with `round_received` less than desired.
        let iter = SliceIterator::new(
            &init_slice,
            |event: &Event<TPayload>| match self.ordering_data(event.hash()) {
                Ok((round_received, _, _)) => round_received < target_round_received,
                Err(OrderingDataError::Undecided) => false,
                Err(OrderingDataError::UnknownEvent(UnknownEvent(e))) => {
                    panic!("events referenced in events must be tracked {e} is unknown.")
                }
            },
            &self.all_events,
        )
        .expect("witnesses must be tracked (2)");

        let mut result = vec![];

        trace!("Getting ordered events");
        for event in iter {
            match self.ordering_data(event.hash()) {
                Ok((round_received, consensus_timestamp, event_signature)) => {
                    if round_received == target_round_received {
                        trace!("Found event with target round received");
                        result.push((event.hash().clone(), consensus_timestamp, event_signature))
                    } else {
                        trace!("Not expected round received, skipping");
                        continue;
                    }
                }
                Err(OrderingDataError::UnknownEvent(UnknownEvent(e))) => {
                    panic!("iterator must iterate on existing events. {e} is unknown.")
                }
                Err(OrderingDataError::Undecided) =>
                    trace!("Event does not have ordering data yet, assuming its round_received is higher than needed"),
            }
        }
        if !init_slice.is_empty() && result.is_empty() {
            // not necessary undecided, should be fixed
            return Err(OrderedEventsError::UndecidedRound);
        }
        Ok(result)
    }
}

impl<TPayload> Graph<TPayload> {
    fn members_count(&self) -> usize {
        self.peer_index.keys().len()
    }

    // for navigating the graph state externally
    pub fn peer_latest_event(&self, peer: &PeerId) -> Option<&event::Hash> {
        self.peer_index.get(peer).map(|e| {
            *e.latest_events()
                .get(0)
                .expect("At least genesis is present")
        })
    }

    // for navigating the graph state externally
    pub fn peer_genesis(&self, peer: &PeerId) -> Option<&event::Hash> {
        self.peer_index.get(peer).map(|e| e.origin())
    }

    // for navigating the graph state externally
    pub fn event(&self, id: &event::Hash) -> Option<&event::Event<TPayload>> {
        self.all_events.get(id)
    }

    /// Iterator over ancestors of the event
    fn ancestor_iter<'a>(&'a self, event_hash: &'a event::Hash) -> Option<AncestorIter<TPayload>> {
        let event = self.all_events.get(event_hash)?;
        let mut e_iter = AncestorIter::new(&self.all_events, event_hash);

        if let event::Kind::Regular(_) = event.parents() {
            e_iter.push_self_ancestors(event_hash)
        }
        Some(e_iter)
    }

    /// Iterator over self ancestors of the event
    fn self_ancestor_iter<'a>(
        &'a self,
        event_hash: &'a event::Hash,
    ) -> Option<SelfAncestorIter<TPayload>> {
        let iter = SelfAncestorIter::new(&self.all_events, event_hash);
        iter
    }

    /// Determine the round an event belongs to, which is the max of its parents' rounds +1 if it
    /// is a witness.
    ///
    /// Actually calculates the number according to needed properties.
    #[instrument(level = "debug", skip(self))]
    fn determine_round(&self, event_hash: &event::Hash) -> Result<RoundNum, UnknownEvent> {
        let event = self
            .all_events
            .get(event_hash)
            .ok_or(UnknownEvent(event_hash.clone()))?;
        match event.parents() {
            event::Kind::Genesis => Ok(0),
            event::Kind::Regular(Parents {
                self_parent,
                other_parent,
            }) => {
                trace!("Finding round # of regular event");
                // Check if it is cached
                if let Some(r) = self.round_of.get(event_hash) {
                    trace!("Result was cached, returning it");
                    return Ok(*r);
                }
                let r = std::cmp::max(
                    self.determine_round(self_parent)
                        .expect("Parents of known events must be known"),
                    self.determine_round(other_parent)
                        .expect("Parents of known events must be known"),
                );

                // Get witnesses from round r
                trace!("Fetching parents' round witnesses to check if it is a witness");
                let round_witnesses = self
                    .round_witnesses(r)
                    .expect("Round of known events must be known")
                    .into_iter()
                    .filter(|eh| *eh != event_hash)
                    .map(|e_hash| {
                        self.all_events
                            .get(e_hash)
                            .expect("Witnesses must be known")
                    })
                    .collect::<Vec<_>>();

                // Find out how many witnesses by unique members the event can strongly see
                trace!("Find which of them are strongly seen by the event");
                let round_witnesses_strongly_seen =
                    round_witnesses
                        .iter()
                        .fold(HashSet::new(), |mut set, witness| {
                            if self.strongly_see(event_hash, &witness.hash()) {
                                let author = witness.author();
                                set.insert(author.clone());
                            }
                            set
                        });

                // n is number of members in hashgraph
                let n = self.members_count();

                let event_round = if round_witnesses_strongly_seen.len() > (2 * n / 3) {
                    trace!("Supermajority achieved, it is a witness");
                    r + 1
                } else {
                    trace!("No supermajority, it is not a witness");
                    r
                };
                Ok(event_round)
            }
        }
    }

    /// None if this round is unknown
    fn round_witnesses(&self, r: usize) -> Option<HashSet<&event::Hash>> {
        let all_round_events = self.round_index.get(r)?.iter();
        let witnesses = all_round_events
            .filter(|e| self.witnesses.contains_key(e))
            .collect();
        Some(witnesses)
    }

    fn round_unique_famous_witnesses(
        &self,
        r: usize,
    ) -> Result<HashSet<&event::Hash>, RoundUfwListError> {
        let round_index = self
            .round_index
            .get(r)
            .ok_or(RoundUfwListError::UnknownRound)?;
        if round_index
            .iter()
            .map(|e| self.is_unique_famous_witness(e))
            .any(|ufw_res| matches!(ufw_res, Ok(WitnessUniqueFamousness::Undecided)))
        {
            return Err(RoundUfwListError::RoundUndecided);
        }
        Ok(round_index
            .iter()
            .filter(|e| {
                matches!(
                    self.is_unique_famous_witness(e),
                    Ok(WitnessUniqueFamousness::FamousUnique)
                )
            })
            .collect())
    }
}

impl<TPayload> Graph<TPayload> {
    // TODO: probably move to round field in event to avoid panics and stuff
    fn round_of(&self, event_hash: &event::Hash) -> RoundNum {
        match self.round_of.get(event_hash) {
            Some(r) => *r,
            None => {
                self.round_index
                    .iter()
                    .enumerate()
                    .find(|(_, round)| round.contains(&event_hash))
                    .expect("Failed to find a round for event")
                    .0 // add to `round_of` in this case maybe??
            }
        }
    }

    /// Determines if the event is a witness
    fn determine_witness(&self, event_hash: &event::Hash) -> Result<bool, UnknownEvent> {
        let r = match self
            .all_events
            .get(&event_hash)
            .ok_or(UnknownEvent(event_hash.clone()))?
            .parents()
        {
            event::Kind::Genesis => true,
            event::Kind::Regular(Parents { self_parent, .. }) => {
                self.round_of(event_hash) > self.round_of(self_parent)
            }
        };
        Ok(r)
    }

    fn decide_fame_for_witness(
        &mut self,
        event_hash: &event::Hash,
    ) -> Result<(), WitnessCheckError> {
        let fame = self.is_famous_witness(event_hash)?;
        self.witnesses.insert(event_hash.clone(), fame);
        Ok(())
    }

    /// Determine if the event is famous.
    /// An event is famous if it is a witness and 2/3 of future witnesses strongly see it.
    ///
    /// None if the event is not witness, otherwise reports famousness
    fn is_famous_witness(
        &self,
        event_hash: &event::Hash,
    ) -> Result<WitnessFamousness, WitnessCheckError> {
        // Event must be a witness
        if !self.determine_witness(event_hash)? {
            return Err(WitnessCheckError::NotWitness);
        }

        // TODO: use `witnesses` as cache (return famous if so, recalculate if not)

        let r = self.round_of(event_hash);

        // first round of the election
        let this_round_index = match self.round_index.get(r + 1) {
            Some(i) => i,
            None => return Ok(WitnessFamousness::Undecided),
        };
        let mut prev_round_votes = HashMap::new();
        for y_hash in this_round_index {
            if self.witnesses.contains_key(y_hash) {
                prev_round_votes.insert(y_hash, self.see(y_hash, &event_hash));
            }
        }

        // TODO: consider dynamic number of nodes
        // (i.e. need to count members at particular round and not at the end)
        let n = self.members_count();

        let next_rounds_indices = match self.round_index.get(r + 2..) {
            Some(i) => i,
            None => return Ok(WitnessFamousness::Undecided),
        };
        for (d, this_round_index) in izip!((2..), next_rounds_indices) {
            let mut this_round_votes = HashMap::new();
            let voter_round = r + d;
            let round_witnesses = this_round_index
                .iter()
                .filter(|e| self.witnesses.contains_key(e));
            for y_hash in round_witnesses {
                // The set of witness events in round (y.round-1) that y can strongly see
                let s = self
                    .round_witnesses(voter_round - 1)
                    .unwrap() // TODO: handle/check if ok?
                    .into_iter()
                    .filter(|h| self.strongly_see(y_hash, h));
                // count votes
                let (votes_for, votes_against) = s.fold((0, 0), |(yes, no), prev_round_witness| {
                    let vote = prev_round_votes.get(prev_round_witness);
                    match vote {
                        Some(true) => (yes + 1, no),
                        Some(false) => (yes, no + 1),
                        None => {
                            // Should not happen but don't just panic, maybe return error later
                            // TODO: warn on inconsistent state
                            (yes, no)
                        }
                    }
                });
                // majority vote in s ( is TRUE for a tie )
                let v = votes_for >= votes_against;
                // number of events in s with a vote of v
                let t = std::cmp::max(votes_for, votes_against);

                if d % self.coin_frequency > 0 {
                    // Normal round
                    if t > (2 * n / 3) {
                        // TODO: move supermajority cond to func
                        // if supermajority, then decide
                        let fame = match v {
                            // TODO: Maybe save the result?? shouldn't change if decided, right?
                            true => WitnessFamousness::Yes,
                            false => WitnessFamousness::No,
                        };
                        return Ok(fame);
                    } else {
                        this_round_votes.insert(y_hash, v);
                    }
                } else {
                    // Coin round
                    if t > (2 * n / 3) {
                        // TODO: move supermajority cond to func
                        // if supermajority, then vote
                        this_round_votes.insert(y_hash, v);
                    } else {
                        let middle_bit = {
                            // TODO: use actual signature, not sure if makes a diff tho
                            let y_sig = self
                                .all_events
                                .get(y_hash)
                                .expect("Inconsistent graph state") //TODO: turn to error
                                .signature()
                                .as_ref();
                            let middle_bit_index = y_sig.len() * 8 / 2;
                            let middle_byte_index = middle_bit_index / 8;
                            let middle_byte = y_sig[middle_byte_index];
                            let middle_bit_index = middle_bit_index % 8;
                            (middle_byte >> middle_bit_index & 1) != 0
                        };
                        this_round_votes.insert(y_hash, middle_bit);
                    }
                }
            }
            prev_round_votes = this_round_votes;
        }
        Ok(WitnessFamousness::Undecided)
    }

    fn is_unique_famous_witness(
        &self,
        event_hash: &event::Hash,
    ) -> Result<WitnessUniqueFamousness, WitnessCheckError> {
        // Get famousness
        let fame = self.is_famous_witness(event_hash)?;

        // If it's undecided or not famous, we don't need to/can't check uniqueness
        if let Ok(result) = fame.clone().try_into() {
            return Ok(result);
        }

        // Determine uniqueness
        let r = self.round_of(event_hash);
        let round_index = match self.round_index.get(r) {
            Some(index) => index,
            None => return Ok(WitnessUniqueFamousness::Undecided),
        };
        let author = self
            .all_events
            .get(event_hash)
            .ok_or(WitnessCheckError::Unknown(UnknownEvent(event_hash.clone())))?
            .author();

        // Events created by the same author in the same round (except for this event)
        let same_creator_round_index = round_index.iter().filter(|hash| {
            let other_author = self
                .all_events
                .get(hash)
                .expect("Inconsistent graph state")
                .author();
            other_author == author && hash != &event_hash
        });

        // Fame of /events created by the same author in the same round (except for this event)/
        let same_creator_round_fame: Vec<_> = same_creator_round_index
            .map(|hash| self.is_famous_witness(hash))
            .collect();

        // If some events in the round are undecided, wait for them
        // Not sure if should work like that, but seems logical
        if same_creator_round_fame
            .iter()
            .any(|fame| matches!(fame, Ok(WitnessFamousness::Undecided)))
        {
            return Ok(WitnessUniqueFamousness::Undecided);
        }

        let unique = !same_creator_round_fame
            .iter()
            .any(|fame| matches!(fame, Ok(WitnessFamousness::Yes)));

        // TODO: cache?
        Ok(WitnessUniqueFamousness::from_famousness(fame, unique))
    }

    /// # Description
    ///
    /// Data needed for ordering an event.
    ///
    /// The value is decided when the target event is an ancestor of (basically, seen by)
    /// all unique famous witnesses of some round.
    ///
    /// Therefore, if there is no such round yet, it is considered to be undecided.
    ///
    /// # Return value
    ///
    /// It consists of
    /// `(round_received, consensus_timestamp, event_signature)`.
    /// `round_received` can be understood as the round this event is finalized.
    #[instrument(level = "trace", skip_all)]
    fn ordering_data(
        &self,
        event_hash: &event::Hash,
    ) -> Result<(usize, Timestamp, event::Hash), OrderingDataError> {
        // is result cached?
        trace!("Checking cache if ordering data is already present there");
        if let Some(cached) = self.ordering_data_cache.get(event_hash) {
            trace!("Cache hit!");
            return Ok(cached.clone());
        }
        trace!("No luck in cache, calculating..");

        // check that the event is known in advance
        let event_signature = self
            .all_events
            .get(event_hash)
            .ok_or(UnknownEvent(event_hash.clone()))?
            .signature();

        // "x is an ancestor of every round r unique famous witness", where `r` is
        // `checked_round`. `r` is also the earliest such round.

        // Since `x` is ancestor of round `r` witnesses and we search for the earliest
        // round that satisfies the condition, we start from round of `x` forward to
        // get `r`.
        for checked_round in self.round_of(event_hash)..self.round_index.len() {
            trace!("Checking round {}", checked_round);
            let unique_famous_witnesses = match self.round_unique_famous_witnesses(checked_round) {
                Ok(list) => list,
                // round before this did not satisfy our condition and later ones
                // are still undecided
                Err(RoundUfwListError::RoundUndecided) => return Err(OrderingDataError::Undecided),
                Err(RoundUfwListError::UnknownRound) => {
                    panic!("`checked_round` range boundary must not allow this")
                }
            };
            trace!("Found {} ufw in the round", unique_famous_witnesses.len());
            // Is `x` an ancestor of every round `r` unique famous witness?
            if unique_famous_witnesses
                .iter()
                .all(|ufw| self.is_ancestor(ufw, event_hash))
            {
                trace!("The event of interest is an ancestor of them all");
                // set of each event z such that z is
                // a self-ancestor of a round r unique famous
                // witness, and x is an ancestor of z but not
                // of the self-parent of z
                let s = unique_famous_witnesses.iter().map(|ufw| {
                    let mut self_ancestors = self
                        .self_ancestor_iter(ufw)
                        .expect("all self ancestors of unique famous witness must be known");
                    // we want to keep track of possible z event
                    let mut first_descendant_event_candidate = self_ancestors
                        .next()
                        .expect("at least 1 self-ancestor must be present - the event itself");
                    for next_ufw_ancestor in self_ancestors {
                        if !self.is_ancestor(next_ufw_ancestor.hash(), event_hash) {
                            break;
                        }
                        first_descendant_event_candidate = next_ufw_ancestor
                    }
                    first_descendant_event_candidate
                });
                let mut timestamps: Vec<_> = s.map(|event| event.timestamp()).collect();
                timestamps.sort();
                trace!("Found {} corresponding events-receivers", timestamps.len());
                trace!("Their timestamps (sorted): {:?}", timestamps);
                // Note that we assume a supermajority of honest members and
                // the median is taken here. Thus this median value will always be
                // in range of honest timestamps.
                let consensus_timestamp = **timestamps
                    .get(timestamps.len() / 2)
                    .expect("there must be some unique famous witnesses in a round");
                trace!("Median is {}", consensus_timestamp);
                return Ok((checked_round, consensus_timestamp, event_signature.clone()));
            }
            trace!("The event of interest is NOT an ancestor of them all, continuing..");
        }
        trace!("Couldn't find set of unique famous witnesses that will order the event.");
        Err(OrderingDataError::Undecided)
    }

    fn is_ancestor(&self, target: &event::Hash, potential_ancestor: &event::Hash) -> bool {
        // TODO: check in other way and return error???
        let _x = self.all_events.get(target).unwrap();
        let _y = self.all_events.get(potential_ancestor).unwrap();

        self.ancestor_iter(target)
            .unwrap()
            .any(|e| e.hash() == potential_ancestor)
    }

    /// True if target(y) is an ancestor of observer(x), but no fork of target is an
    /// ancestor of observer.
    fn see(&self, observer: &event::Hash, target: &event::Hash) -> bool {
        // TODO: add fork check
        return self.is_ancestor(observer, target);
    }

    /// Event `observer` strongly sees `target` through more than 2n/3 members.
    ///
    /// Target is ancestor of observer, for reference
    fn strongly_see(&self, observer: &event::Hash, target: &event::Hash) -> bool {
        // TODO: Check fork conditions
        let authors_seen = self
            .ancestor_iter(observer)
            .unwrap()
            .filter(|e| self.see(&e.hash(), target))
            .fold(HashSet::new(), |mut set, event| {
                let author = event.author();
                set.insert(author.clone());
                set
            });
        let n = self.members_count();
        authors_seen.len() > (2 * n / 3)
    }
}

struct AncestorIter<'a, T> {
    event_list: Vec<&'a Event<T>>,
    all_events: &'a HashMap<event::Hash, Event<T>>,
    visited_events: HashSet<&'a event::Hash>,
}

impl<'a, T> AncestorIter<'a, T> {
    fn new(all_events: &'a HashMap<event::Hash, Event<T>>, ancestors_of: &'a event::Hash) -> Self {
        let mut iter = AncestorIter {
            event_list: vec![],
            all_events: all_events,
            visited_events: HashSet::new(),
        };
        iter.push_self_ancestors(ancestors_of);
        iter
    }

    fn push_self_ancestors(&mut self, event_hash: &'a event::Hash) {
        if self.visited_events.contains(event_hash) {
            return;
        }
        let mut event = self.all_events.get(event_hash).unwrap();

        loop {
            self.event_list.push(event);
            self.visited_events.insert(event_hash);

            if let event::Kind::Regular(Parents { self_parent, .. }) = event.parents() {
                if self.visited_events.contains(self_parent) {
                    // We've already visited all of its self ancestors
                    break;
                }
                event = self.all_events.get(self_parent).unwrap();
            } else {
                break;
            }
        }
    }
}

impl<'a, T> Iterator for AncestorIter<'a, T> {
    type Item = &'a Event<T>;

    fn next(&mut self) -> Option<Self::Item> {
        let event = self.event_list.pop()?;

        if let event::Kind::Regular(Parents { other_parent, .. }) = event.parents() {
            self.push_self_ancestors(other_parent);
        }
        Some(event)
    }
}

struct SelfAncestorIter<'a, T> {
    event_list: VecDeque<&'a Event<T>>,
}

impl<'a, T> SelfAncestorIter<'a, T> {
    fn new(
        all_events: &'a HashMap<event::Hash, Event<T>>,
        ancestors_of: &'a event::Hash,
    ) -> Option<Self> {
        let mut event_list = VecDeque::new();
        let mut next_event_hash = ancestors_of;
        loop {
            let next_event = all_events.get(next_event_hash)?;
            event_list.push_back(next_event);
            if let event::Kind::Regular(Parents { self_parent, .. }) = next_event.parents() {
                next_event_hash = self_parent
            } else {
                break;
            }
        }
        Some(Self { event_list })
    }
}

impl<'a, T> Iterator for SelfAncestorIter<'a, T> {
    type Item = &'a Event<T>;

    fn next(&mut self) -> Option<Self::Item> {
        self.event_list.pop_front()
    }
}

impl<'a, T> DoubleEndedIterator for SelfAncestorIter<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.event_list.pop_back()
    }
}

impl<TPayload> crate::common::Graph for Graph<TPayload> {
    type NodeIdentifier = event::Hash;
    type NodeIdentifiers =
        std::iter::Flatten<std::option::IntoIter<std::vec::IntoIter<event::Hash>>>;

    fn neighbors(&self, node: &Self::NodeIdentifier) -> Self::NodeIdentifiers {
        self.all_events
            .get(node)
            .map(|some_event| {
                let children: Vec<_> = some_event.children.clone().into();
                children.into_iter()
            })
            .into_iter()
            .flatten()
    }
}

// Tests became larger than the code, so for easier navigation I've moved them
#[cfg(test)]
mod tests;
