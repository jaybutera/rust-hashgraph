use itertools::izip;
use serde::Serialize;

use std::collections::{HashMap, HashSet};

use super::event::{self, Event, Parents};
use super::{PeerIndexEntry, PushError, PushKind, RoundNum};
use crate::PeerId;

type NodeIndex<TIndexPayload> = HashMap<event::Hash, TIndexPayload>;

#[derive(Debug, PartialEq)]
pub enum WitnessFamousness {
    Undecided,
    Yes,
    No,
}

pub struct Graph<TPayload> {
    all_events: NodeIndex<Event<TPayload>>,
    peer_index: HashMap<PeerId, PeerIndexEntry>,
    round_index: Vec<HashSet<event::Hash>>,
    /// Some(false) means unfamous witness
    witnesses: HashMap<event::Hash, WitnessFamousness>,
    round_of: HashMap<event::Hash, RoundNum>, // Just testing a caching system for now

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
            PushKind::Regular(other_parent) => {
                let latest_author_event = &self
                    .peer_index
                    .get(&author)
                    .ok_or(PushError::PeerNotFound(author))?
                    .latest_event;
                Event::new(
                    payload,
                    event::Kind::Regular(Parents {
                        self_parent: latest_author_event.clone(),
                        other_parent,
                    }),
                    author,
                )?
            }
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
                    // Should not be triggered, since we check it above
                    return Err(PushError::NoParent(parents.self_parent.clone()));
                }
                if !self.all_events.contains_key(&parents.other_parent) {
                    return Err(PushError::NoParent(parents.other_parent.clone()));
                }

                // taking mutable for update later
                let self_parent_node = self
                    .all_events
                    .get_mut(&parents.self_parent) // TODO: use get_many_mut when stabilized
                    .expect("Just checked presence before");

                if self_parent_node.author() != &author {
                    return Err(PushError::IncorrectAuthor(
                        self_parent_node.author().clone(),
                        author,
                    ));
                }

                if let Some(existing_child) = &self_parent_node.children.self_child {
                    // Should not happen since latest events should not have self children
                    return Err(PushError::SelfChildAlreadyExists(existing_child.clone()));
                }

                // taking mutable for update later
                let author_index = self
                    .peer_index
                    .get_mut(&author)
                    .ok_or(PushError::PeerNotFound(author))?;

                // Insertion, should be valid at this point so that we don't leave in inconsistent state on error.

                // update pointers of parents
                self_parent_node.children.self_child = Some(new_node.hash().clone());
                let other_parent_node = self
                    .all_events
                    .get_mut(&parents.other_parent)
                    .expect("Just checked presence before");
                other_parent_node
                    .children
                    .other_children
                    .push(new_node.hash().clone());
                if let Some(_) = author_index.add_latest(new_node.hash().clone()) {
                    // TODO: warn
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
            // Otherwise push onto current round
            // (TODO: check why not to round `r`????)
            self.round_index[last_idx].insert(hash.clone());
        }

        // Set witness status
        if self.determine_witness(&hash) {
            self.witnesses
                .insert(hash.clone(), WitnessFamousness::Undecided);
        }
        Ok(hash)
    }
}

impl<TPayload> Graph<TPayload> {
    pub fn members_count(&self) -> usize {
        self.peer_index.keys().len()
    }

    pub fn peer_latest_event(&self, peer: &PeerId) -> Option<&event::Hash> {
        self.peer_index.get(peer).map(|e| &e.latest_event)
    }

    pub fn peer_genesis(&self, peer: &PeerId) -> Option<&event::Hash> {
        self.peer_index.get(peer).map(|e| &e.genesis)
    }

    pub fn event(&self, id: &event::Hash) -> Option<&TPayload> {
        self.all_events.get(id).map(|e| e.payload())
    }

    /// Iterator over ancestors of the event
    pub fn iter<'a>(&'a self, event_hash: &'a event::Hash) -> Option<EventIter<TPayload>> {
        let event = self.all_events.get(event_hash)?;
        let mut e_iter = EventIter::new(&self.all_events, event_hash);

        if let event::Kind::Regular(_) = event.parents() {
            e_iter.push_self_ancestors(event_hash)
        }
        Some(e_iter)
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

                // Get events from round r
                let round = self.round_index[r]
                    .iter()
                    .filter(|eh| *eh != event_hash)
                    .map(|e_hash| self.all_events.get(e_hash).unwrap())
                    .collect::<Vec<_>>();

                // Find out how many witnesses by unique members the event can strongly see
                let witnesses_strongly_seen = round
                    .iter()
                    .filter(|e| self.witnesses.contains_key(&e.hash()))
                    .fold(HashSet::new(), |mut set, witness| {
                        if self.strongly_see(event_hash, &witness.hash()) {
                            let author = witness.author();
                            set.insert(author.clone());
                        }
                        set
                    });

                // n is number of members in hashgraph
                let n = self.members_count();

                if witnesses_strongly_seen.len() > (2 * n / 3) {
                    r + 1
                } else {
                    r
                }
            }
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct NotWitness;

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
                    .0
            }
        }
    }

    /// Determines if the event is a witness
    pub fn determine_witness(&self, event_hash: &event::Hash) -> bool {
        match self.all_events.get(&event_hash).unwrap().parents() {
            event::Kind::Genesis => true,
            event::Kind::Regular(Parents { self_parent, .. }) => {
                self.round_of(event_hash) > self.round_of(self_parent)
            }
        }
    }

    pub fn decide_fame_for_witness(&mut self, event_hash: &event::Hash) -> Result<(), NotWitness> {
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
    ) -> Result<WitnessFamousness, NotWitness> {
        // Event must be a witness
        if !self.determine_witness(event_hash) {
            return Err(NotWitness);
        }

        let r = self.round_of(event_hash);

        // first round of the election
        let this_round_index = match self.round_index.get(r + 1) {
            Some(i) => i,
            None => return Ok(WitnessFamousness::Undecided),
        };
        let mut prev_round_votes = HashMap::new();
        for y_hash in this_round_index {
            prev_round_votes.insert(y_hash, self.see(y_hash, &event_hash));
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
                let s = self.round_index[voter_round - 1]
                    .iter()
                    .filter(|h| self.witnesses.contains_key(h) && self.strongly_see(y_hash, h));
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
                        return Ok(WitnessFamousness::Yes);
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

    fn ancestor(&self, target: &event::Hash, potential_ancestor: &event::Hash) -> bool {
        // TODO: check in other way and return error???
        let _x = self.all_events.get(target).unwrap();
        let _y = self.all_events.get(potential_ancestor).unwrap();

        self.iter(target)
            .unwrap()
            .any(|e| e.hash() == potential_ancestor)
    }

    /// True if y is an ancestor of x, but no fork of y is an ancestor of x
    ///
    /// Target is ancestor of observer, for reference
    fn see(&self, observer: &event::Hash, target: &event::Hash) -> bool {
        // TODO: add fork check
        return self.ancestor(observer, target);
    }

    /// Event `observer` strongly sees `target` through more than 2n/3 members.
    ///
    /// Target is ancestor of observer, for reference
    fn strongly_see(&self, observer: &event::Hash, target: &event::Hash) -> bool {
        // TODO: Check fork conditions
        let authors_seen = self
            .iter(observer)
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

pub struct EventIter<'a, T> {
    node_list: Vec<&'a Event<T>>,
    all_events: &'a HashMap<event::Hash, Event<T>>,
    visited_events: HashSet<&'a event::Hash>,
}

impl<'a, T> EventIter<'a, T> {
    pub fn new(
        all_events: &'a HashMap<event::Hash, Event<T>>,
        ancestors_of: &'a event::Hash,
    ) -> Self {
        let mut iter = EventIter {
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

impl<'a, T> Iterator for EventIter<'a, T> {
    type Item = &'a Event<T>;

    fn next(&mut self) -> Option<Self::Item> {
        let event = self.node_list.pop()?;

        if let event::Kind::Regular(Parents { other_parent, .. }) = event.parents() {
            self.push_self_ancestors(other_parent);
        }
        Some(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// # Description
    /// Test a property of a graph according to test cases.
    /// 
    /// The macro requires the name of property (for logging in case of failure),
    /// function to test, and function for human readable name lookup (details
    /// see in [`test_cases`]).
    /// 
    /// Also you should specify identifier for hashmap of events by each peer
    /// that can be used in cases declaration. See examples for more clarity
    /// 
    /// # Usage
    /// 
    /// Upon calling the macro, first specify property name, function to test, and
    /// lookup. Then list test cases for each graph.
    /// 
    /// ## Examples
    /// 
    /// ```no_run
    /// run_tests!(
    ///     "fame",
    ///     |g, event| g.is_famous_witness(&event),
    ///     |event, names| names.get(event).unwrap().to_owned(),
    ///     peers,
    ///     (
    ///         build_graph_some_chain((), 999).unwrap(),
    ///         vec![
    ///             (
    ///                 Ok(WitnessFamousness::Undecided),
    ///                 vec![
    ///                     peers.get("g1").unwrap().events[0].clone(),
    ///                     peers.get("g1").unwrap().events[2].clone(),
    ///                     peers.get("g2").unwrap().events[0].clone(),
    ///                     peers.get("g2").unwrap().events[3].clone(),
    ///                     peers.get("g3").unwrap().events[0].clone(),
    ///                     peers.get("g3").unwrap().events[2].clone(),
    ///                 ],
    ///             ),
    ///             (
    ///                 Err(NotWitness),
    ///                 [
    ///                     &peers.get("g1").unwrap().events[1..2],
    ///                     &peers.get("g2").unwrap().events[1..3],
    ///                     &peers.get("g3").unwrap().events[1..2],
    ///                 ]
    ///                 .concat(),
    ///             ),
    ///         ]
    ///     )
    /// );
    /// ```
    macro_rules! run_tests {
        ( $property_name:expr, $tested_function:expr, $name_lookup:expr, $peers_literal:ident, $(($graph:expr, $cases:expr)),* ) => {
            let mut cases = vec![];
            $(
                let (_graph, $peers_literal, _names, _graph_name) = $graph;
                let graph_cases = ($cases);
                cases.push(((_graph, $peers_literal, _names, _graph_name), graph_cases));
            )*
            test_cases(cases, $property_name, $tested_function, $name_lookup);
        };
    }

    struct PeerEvents {
        id: PeerId,
        events: Vec<event::Hash>,
    }

    // Graph, Events by each peer, Test event names (for easier reading), Graph name
    type TestGraph<T> = (
        Graph<T>,
        HashMap<String, PeerEvents>,  // For getting hashes for events
        HashMap<event::Hash, String>, // For lookup of readable name
        String,                       // Name of the test case
    );

    /// # Description
    /// Run tests on multiple cases, compare the results, and report if needed.
    /// 
    /// # Arguments
    /// * `cases`: List of test cases. Each list entry consists of graph (with
    /// helper data structures, see [`TestGraph`](TestGraph<T>)) and test cases
    /// for the graph. The graph cases are grouped by result expected. For each
    /// result there is a list of arguments to be supplied to `tested_function`.
    /// 
    /// * `tested_function_name`: name of the function, used for assert messages.
    /// 
    /// * `tested_function`: function to test, takes 2 arguments: graph itself and
    /// argument specified in each test case.
    /// 
    /// * `name_lookup`: function for obtaining event name based on corresponding
    /// `HashMap` and argument of the test case, used for better readable assert messages.
    /// 
    /// # Example
    /// 
    /// Suppose we want to check correctness of round calculation:
    /// ```no_run
    /// 
    /// let mut cases = vec![];
    /// let (graph, peers, names, graph_name) = build_graph_some_chain((), 999).unwrap();
    /// let graph_cases = vec![
    ///     (
    ///         0,
    ///         [
    ///             &peers.get("g1").unwrap().events[0..2],
    ///             &peers.get("g2").unwrap().events[0..3],
    ///             &peers.get("g3").unwrap().events[0..2],
    ///         ]
    ///         .concat(),
    ///     ),
    ///     (
    ///         1,
    ///         [
    ///             &peers.get("g1").unwrap().events[2..3],
    ///             &peers.get("g2").unwrap().events[3..4],
    ///             &peers.get("g3").unwrap().events[2..3],
    ///         ]
    ///         .concat(),
    ///     ),
    /// ];
    /// cases.push(((graph, peers, names, graph_name), graph_cases));
    /// test_cases(
    ///     cases,
    ///     "round",
    ///     |g, args| g.round_of(&args),
    ///     |event, names| names.get(event).unwrap().to_owned(),
    /// );
    /// ```
    fn test_cases<TPayload, TArg, TResult, F, FNameLookup>(
        cases: Vec<(TestGraph<TPayload>, Vec<(TResult, Vec<TArg>)>)>,
        tested_function_name: &str,
        tested_function: F,
        name_lookup: FNameLookup,
    ) where
        F: Fn(&Graph<TPayload>, &TArg) -> TResult,
        TResult: PartialEq + std::fmt::Debug,
        FNameLookup: Fn(&TArg, &HashMap<event::Hash, String>) -> String,
    {
        for ((graph, _peers, names, graph_name), graph_cases) in cases {
            for (expected_result, result_cases) in graph_cases {
                for case in result_cases {
                    let result = tested_function(&graph, &case);
                    assert_eq!(
                        result,
                        expected_result,
                        "Event(-s) {} of graph {} expected {} {:?}, but got {:?}",
                        name_lookup(&case, &names),
                        graph_name,
                        tested_function_name,
                        expected_result,
                        result
                    );
                }
            }
        }
    }

    /// Add multiple events in the graph (for easier test case creation and
    /// concise and more intuitive writing).
    /// `events` is list of tuples (event_name, author_name, other_parent_name)
    ///
    /// `other_parent_name` is either `event_name` of one of the previous entries
    /// or name of peer for its genesis
    fn add_events<T: Serialize + Copy>(
        graph: &mut Graph<T>,
        events: &[(&'static str, &'static str, &'static str)],
        author_ids: HashMap<&'static str, PeerId>,
        payload: T,
    ) -> Result<
        (
            HashMap<String, PeerEvents>,
            HashMap<event::Hash, String>, // hash -> event_name
        ),
        PushError,
    > {
        let mut inserted_events = HashMap::with_capacity(events.len());
        let mut peers_events: HashMap<String, PeerEvents> = author_ids
            .keys()
            .map(|&name| {
                let id = *author_ids
                    .get(name)
                    .expect(&format!("Unknown author name '{}'", name));
                let genesis = graph.peer_genesis(&id).expect(&format!(
                    "Unknown author id to graph '{}' (name {})",
                    id, name
                ));
                (
                    name.to_owned(),
                    PeerEvents {
                        id,
                        events: vec![genesis.clone()],
                    },
                )
            })
            .collect();

        for &(event_name, author, other_parent) in events {
            let other_parent_event_hash = match author_ids.get(other_parent) {
                Some(h) => graph.peer_genesis(h).expect(&format!(
                    "Unknown peer id {} to graph (name '{}')",
                    h, author
                )),
                None => inserted_events
                    .get(other_parent)
                    .expect(&format!("Unknown `other_parent` '{}'", other_parent)),
            };
            let author_id = author_ids
                .get(author)
                .expect(&format!("Unknown author name '{}'", author));
            let new_event_hash =
                graph.push_node(payload, PushKind::Regular(other_parent_event_hash.clone()), *author_id)?;
            let author = author.to_owned();
            peers_events
                .get_mut(&author)
                .expect("Just checked presence")
                .events
                .push(new_event_hash.clone());
            let clashed_event = inserted_events.insert(event_name.to_owned(), new_event_hash);
            if clashed_event.is_some() {
                panic!("Event name clash '{}'", event_name)
            }
        }
        let names = inserted_events.into_iter().map(|(a, b)| (b, a)).collect();
        Ok((peers_events, names))
    }

    fn add_geneses<T: Serialize + Copy>(
        graph: &mut Graph<T>,
        this_author: &str,
        author_ids: &HashMap<&'static str, PeerId>,
        payload: T,
    ) -> Result<HashMap<event::Hash, String>, PushError> {
        let mut names = HashMap::with_capacity(author_ids.len());

        for (&name, id) in author_ids {
            let hash = if name == this_author {
                graph
                    .peer_genesis(id)
                    .expect("Mush have own genesis")
                    .clone()
            } else {
                graph.push_node(payload, PushKind::Genesis, *id)?
            };
            names.insert(hash, name.to_owned());
        }
        Ok(names)
    }

    fn build_graph_from_paper<T: Serialize + Copy>(
        payload: T,
        coin_frequency: usize,
    ) -> Result<TestGraph<T>, PushError> {
        let author_ids = HashMap::from([("a", 0), ("b", 1), ("c", 2), ("d", 3), ("e", 4)]);
        let mut graph = Graph::new(*author_ids.get("a").unwrap(), payload, coin_frequency);
        let mut names = add_geneses(&mut graph, "a", &author_ids, payload)?;
        let events = [
            //  (name, peer, other_parent)
            ("c2", "c", "d"),
            ("e2", "e", "b"),
            ("b2", "b", "c2"),
            ("c3", "c", "e2"),
            ("d2", "d", "c3"),
            ("a2", "a", "b2"),
            ("b3", "b", "c3"),
            ("c4", "c", "d2"),
            ("a3", "a", "b3"),
            ("c5", "c", "e2"),
            ("c6", "c", "a3"),
        ];
        let (peers_events, new_names) = add_events(&mut graph, &events, author_ids, payload)?;
        names.extend(new_names);
        Ok((graph, peers_events, names, "Whitepaper example".to_owned()))
    }

    fn build_graph_some_chain<T: Serialize + Copy>(
        payload: T,
        coin_frequency: usize,
    ) -> Result<TestGraph<T>, PushError> {
        /* Generates the following graph for each member (c1,c2,c3)
         *
            |  o__|  -- e7
            |__|__o  -- e6
            o__|  |  -- e5
            |  o__|  -- e4
            |  |__o  -- e3
            |__o  |  -- e2
            o__|  |  -- e1
            o  o  o  -- (g1,g2,g3)
        */
        let author_ids = HashMap::from([("g1", 0), ("g2", 1), ("g3", 2)]);
        let mut graph = Graph::new(*author_ids.get("g1").unwrap(), payload, coin_frequency);
        let mut names = add_geneses(&mut graph, "g1", &author_ids, payload)?;
        let events = [
            //  (name, peer, other_parent)
            ("e1", "g1", "g2"),
            ("e2", "g2", "e1"),
            ("e3", "g3", "e2"),
            ("e4", "g2", "e3"),
            ("e5", "g1", "e4"),
            ("e6", "g3", "e5"),
            ("e7", "g2", "e6"),
        ];
        let (peers_events, new_names) = add_events(&mut graph, &events, author_ids, payload)?;
        names.extend(new_names);
        Ok((graph, peers_events, names, "Chain events".to_owned()))
    }

    fn build_graph_detailed_example<T: Serialize + Copy>(
        payload: T,
        coin_frequency: usize,
    ) -> Result<TestGraph<T>, PushError> {
        // Defines graph from paper HASHGRAPH CONSENSUS: DETAILED EXAMPLES
        // https://www.swirlds.com/downloads/SWIRLDS-TR-2016-02.pdf
        // also in resources/graph_example.png

        let author_ids = HashMap::from([("a", 0), ("b", 1), ("c", 2), ("d", 3)]);
        let mut graph = Graph::new(*author_ids.get("a").unwrap(), payload, coin_frequency);
        let mut names = add_geneses(&mut graph, "a", &author_ids, payload)?;
        // resources/graph_example.png for reference
        let events = [
            //  (name,  peer, other_parent)
            // round 1
            ("d1_1", "d", "b"),
            ("b1_1", "b", "d1_1"),
            ("d1_2", "d", "b1_1"),
            ("b1_2", "b", "c"),
            ("a1_1", "a", "b1_1"),
            ("d1_3", "d", "b1_2"),
            ("c1_1", "c", "b1_2"),
            ("b1_3", "b", "d1_3"),
            // round 2
            ("d2", "d", "a1_1"),
            ("a2", "a", "d2"),
            ("b2", "b", "d2"),
            ("a2_1", "a", "c1_1"),
            ("c2", "c", "a2_1"),
            ("d2_1", "d", "b2"),
            ("a2_2", "a", "b2"),
            ("d2_2", "d", "a2_2"),
            ("b2_1", "b", "a2_2"),
            // round 3
            ("b3", "b", "d2_2"),
            ("a3", "a", "b3"),
            ("d3", "d", "b3"),
            ("d3_1", "d", "c2"),
            ("c3", "c", "d3_1"),
            ("b3_1", "b", "a3"),
            ("b3_2", "b", "a3"),
            ("a3_1", "a", "b3_2"),
            ("b3_3", "b", "d3_1"),
            ("a3_2", "a", "b3_3"),
            ("b3_4", "b", "a3_2"),
            ("d3_2", "d", "b3_3"),
            // round 4
            ("d4", "d", "c3"),
            ("b4", "b", "d4"),
        ];
        let (peers_events, new_names) = add_events(&mut graph, &events, author_ids, payload)?;
        names.extend(new_names);
        Ok((
            graph,
            peers_events,
            names,
            "Detailed examples tech report".to_owned(),
        ))
    }

    // Test simple work + errors

    #[test]
    fn graph_builds() {
        build_graph_from_paper((), 999).unwrap();
        build_graph_some_chain((), 999).unwrap();
        build_graph_detailed_example((), 999).unwrap();
    }

    #[test]
    fn duplicate_push_fails() {
        let (mut graph, peers, _names, _graph_name) = build_graph_from_paper((), 999).unwrap();
        let a_id = peers.get("a").unwrap().id;
        assert!(matches!(
            graph.push_node((), PushKind::Genesis, a_id),
            Err(PushError::NodeAlreadyExists(hash)) if &hash == graph.peer_genesis(&a_id).unwrap()
        ));
    }

    #[test]
    fn double_genesis_fails() {
        let (mut graph, peers, _names, _graph_name) = build_graph_from_paper(0, 999).unwrap();
        assert!(matches!(
            graph.push_node(1, PushKind::Genesis, peers.get("a").unwrap().id),
            Err(PushError::GenesisAlreadyExists)
        ))
    }

    #[test]
    fn missing_parent_fails() {
        let (mut graph, peers, _names, _graph_name) = build_graph_from_paper((), 999).unwrap();
        let fake_node = Event::new((), event::Kind::Genesis, 1232423).unwrap();
        assert!(matches!(
            graph.push_node((), PushKind::Regular(fake_node.hash().clone()), peers.get("a").unwrap().id),
            Err(PushError::NoParent(fake_hash)) if &fake_hash == fake_node.hash()
        ))
    }

    // Test graph properties

    #[test]
    fn test_ancestor() {
        let (graph, peers, _names, graph_name) = build_graph_some_chain((), 999).unwrap();

        assert!(graph.ancestor(
            &peers.get("g1").unwrap().events[1],
            &peers.get("g1").unwrap().events[0]
        ));

        let (graph, peers, _names, graph_name) = build_graph_from_paper((), 999).unwrap();
        assert!(graph.ancestor(
            &peers.get("c").unwrap().events[5],
            &peers.get("b").unwrap().events[0],
        ));

        assert!(graph.ancestor(
            &peers.get("a").unwrap().events[2],
            &peers.get("e").unwrap().events[1],
        ));

        let (graph, peers, names, graph_name) = build_graph_detailed_example((), 999).unwrap();
        let test_cases = [
            (
                false,
                vec![
                    (
                        &peers.get("c").unwrap().events[0],
                        &peers.get("c").unwrap().events[1],
                    ),
                    (
                        &peers.get("c").unwrap().events[0],
                        &peers.get("c").unwrap().events[3],
                    ),
                    (
                        &peers.get("c").unwrap().events[0],
                        &peers.get("b").unwrap().events[2],
                    ),
                    (
                        &peers.get("c").unwrap().events[1],
                        &peers.get("d").unwrap().events[3],
                    ),
                    (
                        &peers.get("a").unwrap().events[2],
                        &peers.get("c").unwrap().events[1],
                    ),
                ],
            ),
            (
                true,
                vec![
                    (
                        // Self parent
                        &peers.get("d").unwrap().events[1],
                        &peers.get("d").unwrap().events[0],
                    ),
                    (
                        // Self ancestor
                        &peers.get("d").unwrap().events[4],
                        &peers.get("d").unwrap().events[0],
                    ),
                    (
                        // Ancestry is reflective
                        &peers.get("c").unwrap().events[1],
                        &peers.get("c").unwrap().events[1],
                    ),
                    (
                        // Other parent
                        &peers.get("b").unwrap().events[3],
                        &peers.get("d").unwrap().events[3],
                    ),
                    (
                        &peers.get("c").unwrap().events[2],
                        &peers.get("a").unwrap().events[2],
                    ),
                    (
                        &peers.get("b").unwrap().events[3],
                        &peers.get("c").unwrap().events[0],
                    ),
                    (
                        &peers.get("d").unwrap().events[3],
                        &peers.get("c").unwrap().events[0],
                    ),
                    (
                        // Debugging b2 not being witness
                        &peers.get("d").unwrap().events[6],
                        &peers.get("a").unwrap().events[2],
                    ),
                    (
                        // Debugging b2 not being witness
                        &peers.get("b").unwrap().events[6],
                        &peers.get("a").unwrap().events[2],
                    ),
                    (
                        // Debugging b2 not being witness
                        &peers.get("a").unwrap().events[4],
                        &peers.get("a").unwrap().events[2],
                    ),
                ],
            ),
        ];
        for (result, cases) in test_cases {
            for (e1, e2) in cases {
                let actual_result = graph.ancestor(e1, e2);
                let (e1_name, e2_name) = (names.get(e1).unwrap(), names.get(e2).unwrap());
                assert_eq!(
                    result, actual_result,
                    "expected ancestor({},{}) to be {}, but it is {}.",
                    e1_name, e2_name, result, actual_result
                )
            }
        }
    }

    #[test]
    fn test_ancestor_iter() {
        let (graph, peers, names, graph_name) = build_graph_detailed_example((), 999).unwrap();
        // (Iterator, Actual ancestors to compare with)
        let cases = vec![
            (
                graph.iter(&peers.get("b").unwrap().events[3]).unwrap(),
                HashSet::<_>::from_iter(
                    [
                        &peers.get("b").unwrap().events[0..4],
                        &peers.get("c").unwrap().events[0..1],
                        &peers.get("d").unwrap().events[0..4],
                    ]
                    .concat()
                    .into_iter(),
                ),
            ),
            (
                // debugging b3 not being witness
                graph.iter(&peers.get("b").unwrap().events[6]).unwrap(),
                HashSet::<_>::from_iter(
                    [
                        &peers.get("a").unwrap().events[0..5],
                        &peers.get("b").unwrap().events[0..7],
                        &peers.get("c").unwrap().events[0..2],
                        &peers.get("d").unwrap().events[0..7],
                    ]
                    .concat()
                    .into_iter(),
                ),
            ),
        ];
        for (iter, ancestors) in cases {
            let ancestors_from_iter = HashSet::<_>::from_iter(iter.map(|e| e.hash().clone()));
            assert_eq!(
                ancestors,
                ancestors_from_iter,
                "Iterator did not find ancestors {:?}\n and it went through excess events: {:?}",
                ancestors
                    .difference(&ancestors_from_iter)
                    .map(|h| names.get(h).unwrap())
                    .collect::<Vec<_>>(),
                ancestors_from_iter
                    .difference(&ancestors)
                    .map(|h| names.get(h).unwrap())
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn test_strongly_see() {
        let (graph, peers, _names, graph_name) = build_graph_some_chain((), 999).unwrap();

        assert!(!graph.strongly_see(
            &peers.get("g1").unwrap().events[1],
            &peers.get("g1").unwrap().events[0],
        ));
        assert!(graph.strongly_see(
            &peers.get("g2").unwrap().events[2],
            &peers.get("g1").unwrap().events[0],
        ));

        let (graph, peers, _names, graph_name) = build_graph_from_paper((), 999).unwrap();
        assert!(graph.strongly_see(
            &peers.get("c").unwrap().events[5],
            &peers.get("d").unwrap().events[0],
        ));

        let (graph, peers, names, graph_name) = build_graph_detailed_example((), 999).unwrap();
        let test_cases = [
            (
                false,
                vec![
                    (
                        &peers.get("d").unwrap().events[0],
                        &peers.get("d").unwrap().events[0],
                    ),
                    (
                        &peers.get("d").unwrap().events[3],
                        &peers.get("d").unwrap().events[0],
                    ),
                    (
                        &peers.get("d").unwrap().events[3],
                        &peers.get("b").unwrap().events[0],
                    ),
                    (
                        &peers.get("b").unwrap().events[2],
                        &peers.get("c").unwrap().events[0],
                    ),
                    (
                        &peers.get("a").unwrap().events[0],
                        &peers.get("b").unwrap().events[0],
                    ),
                    (
                        &peers.get("a").unwrap().events[1],
                        &peers.get("c").unwrap().events[0],
                    ),
                ],
            ),
            (
                true,
                vec![
                    (
                        &peers.get("d").unwrap().events[4],
                        &peers.get("d").unwrap().events[0],
                    ),
                    (
                        &peers.get("d").unwrap().events[4],
                        &peers.get("b").unwrap().events[0],
                    ),
                    (
                        &peers.get("b").unwrap().events[3],
                        &peers.get("c").unwrap().events[0],
                    ),
                    (
                        &peers.get("a").unwrap().events[1],
                        &peers.get("b").unwrap().events[0],
                    ),
                    (
                        &peers.get("a").unwrap().events[3],
                        &peers.get("c").unwrap().events[0],
                    ),
                    (
                        // Did not find for round calculation once
                        &peers.get("b").unwrap().events[6],
                        &peers.get("a").unwrap().events[2],
                    ),
                ],
            ),
        ];
        for (result, cases) in test_cases {
            for (e1, e2) in cases {
                let (e1_name, e2_name) = (names.get(e1).unwrap(), names.get(e2).unwrap());
                let actual_result = graph.strongly_see(e1, e2);
                assert_eq!(
                    result, actual_result,
                    "expected strongly_see({},{}) to be {}, but it is {}.",
                    e1_name, e2_name, result, actual_result
                )
            }
        }
    }

    #[test]
    fn test_determine_round() {
        run_tests!(
            "round",
            |g, args| g.round_of(&args),
            |event, names| names.get(event).unwrap().to_owned(),
            peers,
            (
                build_graph_some_chain((), 999).unwrap(),
                vec![
                    (
                        0,
                        [
                            &peers.get("g1").unwrap().events[0..2],
                            &peers.get("g2").unwrap().events[0..3],
                            &peers.get("g3").unwrap().events[0..2],
                        ]
                        .concat(),
                    ),
                    (
                        1,
                        [
                            &peers.get("g1").unwrap().events[2..3],
                            &peers.get("g2").unwrap().events[3..4],
                            &peers.get("g3").unwrap().events[2..3],
                        ]
                        .concat(),
                    ),
                ]
            ),
            (
                build_graph_detailed_example((), 999).unwrap(),
                vec![
                    (
                        0usize,
                        [
                            &peers.get("a").unwrap().events[0..2],
                            &peers.get("b").unwrap().events[0..4],
                            &peers.get("c").unwrap().events[0..2],
                            &peers.get("d").unwrap().events[0..4],
                        ]
                        .concat(),
                    ),
                    (
                        1,
                        [
                            &peers.get("a").unwrap().events[2..5],
                            &peers.get("b").unwrap().events[4..6],
                            &peers.get("c").unwrap().events[2..3],
                            &peers.get("d").unwrap().events[4..7],
                        ]
                        .concat(),
                    ),
                    (
                        2,
                        [
                            &peers.get("a").unwrap().events[5..8],
                            &peers.get("b").unwrap().events[6..11],
                            &peers.get("c").unwrap().events[3..4],
                            &peers.get("d").unwrap().events[7..10],
                        ]
                        .concat(),
                    ),
                    (
                        3,
                        [
                            &peers.get("b").unwrap().events[11..12],
                            &peers.get("d").unwrap().events[10..11],
                        ]
                        .concat(),
                    ),
                ]
            )
        );
    }

    #[test]
    fn test_determine_witness() {
        let (graph, peers, _names, graph_name) = build_graph_some_chain((), 999).unwrap();

        assert!(!graph.determine_witness(&peers.get("g3").unwrap().events[1]));
        assert!(graph.determine_witness(&peers.get("g2").unwrap().events[2]));
        assert!(graph.determine_witness(&peers.get("g1").unwrap().events[2]));
    }

    #[test]
    fn test_is_famous_witness() {
        run_tests!(
            "fame",
            |g, event| g.is_famous_witness(&event),
            |event, names| names.get(event).unwrap().to_owned(),
            peers,
            (
                build_graph_some_chain((), 999).unwrap(),
                vec![
                    (
                        Ok(WitnessFamousness::Undecided),
                        vec![
                            peers.get("g1").unwrap().events[0].clone(),
                            peers.get("g1").unwrap().events[2].clone(),
                            peers.get("g2").unwrap().events[0].clone(),
                            peers.get("g2").unwrap().events[3].clone(),
                            peers.get("g3").unwrap().events[0].clone(),
                            peers.get("g3").unwrap().events[2].clone(),
                        ],
                    ),
                    (
                        Err(NotWitness),
                        [
                            &peers.get("g1").unwrap().events[1..2],
                            &peers.get("g2").unwrap().events[1..3],
                            &peers.get("g3").unwrap().events[1..2],
                        ]
                        .concat(),
                    ),
                ]
            )
        );
    }
}
