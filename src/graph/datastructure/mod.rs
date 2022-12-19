use itertools::izip;
use serde::Serialize;
use thiserror::Error;

use std::collections::{HashMap, HashSet, VecDeque};

use super::event::{self, Event, Parents};
use super::index::{NodeIndex, PeerIndexEntry};
use super::{PushError, PushKind, RoundNum};
use crate::PeerId;

#[derive(Debug, PartialEq, Clone)]
pub enum WitnessFamousness {
    Yes,
    No,
    Undecided,
}

#[derive(Debug, PartialEq, Clone)]
pub enum WitnessUniqueFamousness {
    FamousUnique,
    FamousNotUnique,
    NotFamous,
    Undecided,
}

#[derive(Debug, PartialEq, Clone)]
pub struct UniquenessNotProvided;

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
pub enum WitnessCheckError {
    #[error("This event is not a witness")]
    NotWitness,
    #[error(transparent)]
    Unknown(#[from] UnknownEvent),
}

#[derive(Error, Debug, PartialEq)]
#[error("Event with such hash is unknown to the graph")]
pub struct UnknownEvent;

pub struct Graph<TPayload> {
    all_events: NodeIndex<Event<TPayload>>,
    peer_index: HashMap<PeerId, PeerIndexEntry>,
    /// Consistent and reliable index (should be)
    round_index: Vec<HashSet<event::Hash>>,
    /// Some(false) means unfamous witness
    witnesses: HashMap<event::Hash, WitnessFamousness>,
    /// Cache, shouldn't be relied upon (however seems as reliable as `round_index`)
    round_of: HashMap<event::Hash, RoundNum>,
    /// If round # is in the set - it's decided
    rounds_decided_cache: HashSet<usize>,

    // probably move to config later
    self_id: PeerId,
    /// Coin round frequency
    coin_frequency: usize,
}

impl<T: Serialize> Graph<T> {
    pub fn new(self_id: PeerId, genesis_payload: T, coin_frequency: usize) -> Self {
        let mut graph = Self {
            all_events: HashMap::new(),
            peer_index: HashMap::new(),
            self_id,
            round_index: vec![HashSet::new()],
            witnesses: HashMap::new(),
            round_of: HashMap::new(),
            rounds_decided_cache: HashSet::new(),
            coin_frequency,
        };

        graph
            .push_node(genesis_payload, PushKind::Genesis, self_id)
            .expect("Genesis events should be valid");
        graph
    }
}

impl<TPayload: Serialize> Graph<TPayload> {
    /// Create and push node to the graph, adding it at the end of `author`'s lane
    /// (i.e. the node becomes the latest event of the peer).
    pub fn push_node(
        &mut self,
        payload: TPayload,
        node_type: PushKind,
        author: PeerId,
    ) -> Result<event::Hash, PushError> {
        // Verification first, no changing state

        let new_node = match node_type {
            PushKind::Genesis => Event::new(payload, event::Kind::Genesis, author)?,
            PushKind::Regular(Parents {
                self_parent,
                other_parent,
            }) => Event::new(
                payload,
                event::Kind::Regular(Parents {
                    self_parent,
                    other_parent,
                }),
                author,
            )?,
        };

        if self.all_events.contains_key(new_node.hash()) {
            return Err(PushError::NodeAlreadyExists(new_node.hash().clone()));
        }

        match new_node.parents() {
            event::Kind::Genesis => {
                if self.peer_index.contains_key(&author) {
                    return Err(PushError::GenesisAlreadyExists);
                }
                let new_peer_index = PeerIndexEntry::new(new_node.hash().clone());
                self.peer_index.insert(author, new_peer_index);
            }
            event::Kind::Regular(parents) => {
                if !self.all_events.contains_key(&parents.self_parent) {
                    return Err(PushError::NoParent(parents.self_parent.clone()));
                }
                if !self.all_events.contains_key(&parents.other_parent) {
                    return Err(PushError::NoParent(parents.other_parent.clone()));
                }

                // taking mutable for update later
                let self_parent_node = self
                    .all_events
                    .get_mut(&parents.self_parent) // TODO: use get_many_mut when stabilized
                    .expect("Just checked self parent presence");

                if self_parent_node.author() != &author {
                    return Err(PushError::IncorrectAuthor(
                        self_parent_node.author().clone(),
                        author,
                    ));
                }

                // if let Some(existing_child) = &self_parent_node.children.self_child {
                //     // Should not happen since latest events should not have self children

                //     // TODO: insert with self_parent as hash and handle fork insertion.
                //     return Err(PushError::SelfChildAlreadyExists(existing_child.clone()));
                // }

                // taking mutable for update later
                let author_index = self
                    .peer_index
                    .get_mut(&author)
                    .ok_or(PushError::PeerNotFound(author))?;

                // Insertion, should be valid at this point so that we don't leave in inconsistent state on error.

                // update pointers of parents
                if let Some(sibling) = self_parent_node.children.self_child.get(0) {
                    // TODO: handle fork insertion somehow or check if it's handled?.
                    // Track the fork
                    author_index.add_fork(self_parent_node.hash().clone(), sibling.clone());
                    author_index.add_fork(self_parent_node.hash().clone(), new_node.hash().clone());
                }
                self_parent_node
                    .children
                    .self_child
                    .push(new_node.hash().clone());
                let other_parent_node = self
                    .all_events
                    .get_mut(&parents.other_parent)
                    .expect("Just checked other parent presence");
                other_parent_node
                    .children
                    .other_children
                    .push(new_node.hash().clone());
                if let Some(_) = author_index.add_latest(new_node.hash().clone()) {
                    // TODO: warn, inconsistent state between `all_events` and `author_index`
                    panic!()
                }
            }
        };

        // Index the node and save
        let hash = new_node.hash().clone();
        self.all_events.insert(new_node.hash().clone(), new_node);

        // Set round

        let last_idx = self.round_index.len() - 1;
        let r = self.determine_round(&hash);
        // Cache result
        self.round_of.insert(hash.clone(), r);
        if r > last_idx {
            // Create a new round
            let mut round_hs = HashSet::new();
            round_hs.insert(hash.clone());
            self.round_index.push(round_hs);
        } else {
            // Otherwise push onto appropriate round
            self.round_index[r].insert(hash.clone());
        }

        // Set witness status
        if self
            .determine_witness(&hash)
            .expect("Just inserted to `all_events`")
        {
            self.witnesses
                .insert(hash.clone(), WitnessFamousness::Undecided);
        }
        Ok(hash)
    }

    pub fn next_node(&mut self) {}
}

impl<TPayload> Graph<TPayload> {
    pub fn members_count(&self) -> usize {
        self.peer_index.keys().len()
    }

    pub fn peer_latest_event(&self, peer: &PeerId) -> Option<&event::Hash> {
        self.peer_index.get(peer).map(|e| e.latest_event())
    }

    pub fn peer_genesis(&self, peer: &PeerId) -> Option<&event::Hash> {
        self.peer_index.get(peer).map(|e| e.genesis())
    }

    pub fn event(&self, id: &event::Hash) -> Option<&event::Event<TPayload>> {
        self.all_events.get(id)
    }

    /// Iterator over ancestors of the event
    pub fn ancestor_iter<'a>(
        &'a self,
        event_hash: &'a event::Hash,
    ) -> Option<AncestorIter<TPayload>> {
        let event = self.all_events.get(event_hash)?;
        let mut e_iter = AncestorIter::new(&self.all_events, event_hash);

        if let event::Kind::Regular(_) = event.parents() {
            e_iter.push_self_ancestors(event_hash)
        }
        Some(e_iter)
    }

    /// Iterator over self ancestors of the event
    pub fn self_ancestor_iter<'a>(
        &'a self,
        event_hash: &'a event::Hash,
    ) -> Option<SelfAncestorIter<TPayload>> {
        let iter = SelfAncestorIter::new(&self.all_events, event_hash);
        iter
    }

    /// Determine the round an event belongs to, which is the max of its parents' rounds +1 if it
    /// is a witness.
    fn determine_round(&self, event_hash: &event::Hash) -> RoundNum {
        let event = self.all_events.get(event_hash).unwrap();
        match event.parents() {
            event::Kind::Genesis => 0,
            event::Kind::Regular(Parents {
                self_parent,
                other_parent,
            }) => {
                // Check if it is cached
                if let Some(r) = self.round_of.get(event_hash) {
                    return *r;
                }
                let r = std::cmp::max(
                    self.determine_round(self_parent),
                    self.determine_round(other_parent),
                );

                // Get witnesses from round r
                let round_witnesses = self
                    .round_witnesses(r)
                    .unwrap()
                    .into_iter()
                    .filter(|eh| *eh != event_hash)
                    .map(|e_hash| self.all_events.get(e_hash).unwrap())
                    .collect::<Vec<_>>();

                // Find out how many witnesses by unique members the event can strongly see
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

                if round_witnesses_strongly_seen.len() > (2 * n / 3) {
                    r + 1
                } else {
                    r
                }
            }
        }
    }

    /// None if this round is unknown
    fn round_witnesses(&self, r: usize) -> Option<HashSet<&event::Hash>> {
        Some(
            self.round_index
                .get(r)?
                .iter()
                .filter(|e| self.witnesses.contains_key(e))
                .collect(),
        )
    }
}

impl<TPayload> Graph<TPayload> {
    // TODO: probably move to round field in event to avoid panics and stuff
    pub fn round_of(&self, event_hash: &event::Hash) -> RoundNum {
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
    pub fn determine_witness(&self, event_hash: &event::Hash) -> Result<bool, UnknownEvent> {
        let r = match self
            .all_events
            .get(&event_hash)
            .ok_or(UnknownEvent)?
            .parents()
        {
            event::Kind::Genesis => true,
            event::Kind::Regular(Parents { self_parent, .. }) => {
                self.round_of(event_hash) > self.round_of(self_parent)
            }
        };
        Ok(r)
    }

    pub fn decide_fame_for_witness(
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
    pub fn is_famous_witness(
        &self,
        event_hash: &event::Hash,
    ) -> Result<WitnessFamousness, WitnessCheckError> {
        // Event must be a witness
        if !self.determine_witness(event_hash)? {
            return Err(WitnessCheckError::NotWitness);
        }

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
                            // Maybe save the result?? shouldn't change if decided, right?
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
                                .hash()
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

    pub fn is_unique_famous_witness(
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
            .ok_or(WitnessCheckError::Unknown(UnknownEvent))?
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
        Ok(WitnessUniqueFamousness::from_famousness(fame, unique))
    }

    /// true if all known witnesses had their fame decided, for both
    /// round r and all earlier rounds (from the paper)
    fn is_round_decided(&mut self, r: usize) -> bool {
        // TODO: check that the rounds can't become undecided for some reason
        // (shouldn't happen, right??). I assume this when saving that

        // Usually the last round should fail first, however not sure if it's
        // cheaper to compute from the end.
        for checked_round in (0..=r).rev() {
            if self.rounds_decided_cache.contains(&checked_round) {
                continue;
            }
            let round_witnesses = if let Some(i) = self.round_witnesses(r) {
                i
            } else {
                return false;
            };
            for event_hash in round_witnesses {
                match self.is_famous_witness(event_hash) {
                    Ok(WitnessFamousness::Undecided) => {
                        return false;
                    }
                    Ok(_) => {
                        continue;
                    }
                    Err(WitnessCheckError::NotWitness) => {
                        // TODO: warn?? or smth, maybe separate error
                        panic!("Witnesses index or witness check is broken, inconsistent state");
                    }
                    Err(WitnessCheckError::Unknown(_)) => {
                        // TODO: warn?? or smth, maybe separate error
                        panic!("Witnesses index or something else is broken, inconsistent state");
                    }
                }
            }
            // At this point we know that round `checked_round` is decided
            // we should be safe to save this not to recompute it later
            self.rounds_decided_cache.insert(checked_round);
        }
        // This and all previous rounds were decided
        true
    }

    /// Since events are sorted firstly by `round_received` and this number is set to `x`
    /// only when the round's ufws (unique famous witnesses) all have seen the `x`, it makes
    /// sense to order the events in batches after each round is decided.
    ///
    /// Otherwise we cannot guarantee that some event unknown to ufws will not appear (which
    /// would mean that we might skipped it already).
    ///
    /// It is similar to finalization notion. Basically, in such case an event is finalized
    /// when ufws of some round all see it. It implies (at least it seems to me) that we can
    /// univocally find its place in order of all events.

    /// If false, it's undecided
    /// Returns data needed for ordering??
    fn ordering_data(&mut self, event_hash: &event::Hash) -> Option<usize> {
        // "x is an ancestor of every round r unique famous witness", where `r` is
        // `checked_round`. `r` is also the earliest such round.

        // Since `x` is ancestor of round `r` witnesses and we search for the earliest
        // round that satisfies the condition, we start from round of `x` forward to
        // get `r`.
        for checked_round in self.round_of(event_hash)..self.round_index.len() {
            // TODO: Move iteration on unique famous witnesses in a separate func (+ do
            // smth with panics)
            let mut unique_famous_witnesses = vec![];
            for witness in self
                .round_witnesses(checked_round)
                .expect("Inconsistent state between `round_of` and `round_index`")
            {
                match self.is_famous_witness(witness) {
                    Ok(WitnessFamousness::Undecided) => {
                        // round before this did not satisfy our condition and later ones
                        // are still undecided
                        return None;
                    }
                    Ok(WitnessFamousness::Yes) => {
                        unique_famous_witnesses.push(witness);
                    }
                    Ok(WitnessFamousness::No) => {
                        continue;
                    }
                    Err(WitnessCheckError::NotWitness) => {
                        // TODO: warn?? or smth, maybe separate error
                        panic!("Witnesses index or witness check is broken, inconsistent state");
                    }
                    Err(WitnessCheckError::Unknown(_)) => {
                        // TODO: warn?? or smth, maybe separate error
                        panic!("Witnesses index or something else is broken, inconsistent state");
                    }
                }
            }
            // Is `x` an ancestor of every round `r` unique famous witness?
            if unique_famous_witnesses
                .iter()
                .all(|ufw| self.is_ancestor(ufw, event_hash))
            {
                let round_received = checked_round;

                // set of each event z such that z is
                // a self-ancestor of a round r unique famous
                // witness, and x is an ancestor of z but not
                // of the self-parent of z
                let s = unique_famous_witnesses.iter().filter_map(|ufw| {
                    let mut self_ancestors = self
                        .self_ancestor_iter(ufw)
                        .expect("all self ancestors of unique famous witness must be known");
                    // we want to keep track of possible z event
                    let mut first_descendant_event_candidate = self_ancestors
                        .next()
                        .expect("at least 1 self-ancestor must be present - the event itself");
                    for next_ufw_ancestor in self_ancestors {
                        if !self.is_ancestor(next_ufw_ancestor.hash(), event_hash) {
                            return Some(first_descendant_event_candidate);
                        }
                        first_descendant_event_candidate = next_ufw_ancestor
                    }
                    // all self-ancestors of `ufw` (unique famous witness) are descendants of `x`
                    // so it should mean that x is genesis and the ufw is self-descendant.

                    // just ignore it, since it doesn't satisfy our condition
                    None
                });
                let mut timestamps: Vec<_> = s.map(|event| event.timestamp()).collect();
                timestamps.sort();
                timestamps[timestamps.len() / 2]
            }
        }
        None
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

pub struct AncestorIter<'a, T> {
    node_list: Vec<&'a Event<T>>,
    all_events: &'a HashMap<event::Hash, Event<T>>,
    visited_events: HashSet<&'a event::Hash>,
}

impl<'a, T> AncestorIter<'a, T> {
    pub fn new(
        all_events: &'a HashMap<event::Hash, Event<T>>,
        ancestors_of: &'a event::Hash,
    ) -> Self {
        let mut iter = AncestorIter {
            node_list: vec![],
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
            self.node_list.push(event);
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
        let event = self.node_list.pop()?;

        if let event::Kind::Regular(Parents { other_parent, .. }) = event.parents() {
            self.push_self_ancestors(other_parent);
        }
        Some(event)
    }
}

pub struct SelfAncestorIter<'a, T> {
    node_list: VecDeque<&'a Event<T>>,
}

impl<'a, T> SelfAncestorIter<'a, T> {
    pub fn new(
        all_events: &'a HashMap<event::Hash, Event<T>>,
        ancestors_of: &'a event::Hash,
    ) -> Option<Self> {
        let mut node_list = VecDeque::new();
        let mut next_event_hash = ancestors_of;
        loop {
            let next_event = all_events.get(next_event_hash)?;
            node_list.push_back(next_event);
            if let event::Kind::Regular(Parents { self_parent, .. }) = next_event.parents() {
                next_event_hash = self_parent
            } else {
                break;
            }
        }
        Some(Self { node_list })
    }
}

impl<'a, T> Iterator for SelfAncestorIter<'a, T> {
    type Item = &'a Event<T>;

    fn next(&mut self) -> Option<Self::Item> {
        self.node_list.pop_front()
    }
}

impl<'a, T> DoubleEndedIterator for SelfAncestorIter<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.node_list.pop_back()
    }
}

// Tests became larger than the code, so for easier navigation I've moved them
#[cfg(test)]
mod tests;
