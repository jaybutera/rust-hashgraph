use std::collections::HashMap;

use thiserror::Error;
use tracing::{instrument, trace, warn};

use super::event;

pub type EventIndex<TIndexPayload> = HashMap<event::Hash, TIndexPayload>;

pub struct PeerIndexEntry {
    genesis: event::Hash,
    /// Use `add_latest` for insertion.
    ///
    /// Value is height of the event from genesis if considering self parent/self
    /// child relationship. Genesis (should) have height 0.
    ///
    /// In other words, height is a distance from genesis through `self_parent`s
    authored_events: EventIndex<usize>,
    /// Forks authored by the peer that we've observed. Forks are events
    /// that have the same `self_parent`.
    ///
    /// Represented by event hashes that have multiple self children
    /// (children authored by the same peer)
    fork_index: ForkIndex,
    /// Genesis at start
    latest_event: event::Hash,
}

#[derive(Debug, Error, PartialEq)]
pub enum Error {
    #[error("Provided parent hash cannot be looked up with given method")]
    UnknownParent,
    #[error(
        "Provided parent does not have self children, however it is not \
        tracked by fork index. Likely due to inconsistent state of the index with event contents."
    )]
    InvalidParent,
    #[error("Attempted to add an event that is already present in the index")]
    EventAlreadyKnown,
    #[error("Events found via the lookup contain reference unknown events")]
    InconsistentLookup,
    #[error("State of the index and lookup state are inconsistent")]
    LookupIndexInconsistency(#[from] LookupIndexInconsistency),
    #[error("Pushing leaf (non-forking) event failed")]
    LeafPush(#[from] LeafPush),
}

#[derive(Debug, Error, PartialEq)]
#[error("Provided fork identifier is unknown")]
pub struct InvalidForkIdentifier;

#[derive(Debug, Error, PartialEq)]
pub enum LookupIndexInconsistency {
    #[error(transparent)]
    InvalidIdentifier(#[from] InvalidForkIdentifier),
    #[error("Splitting extension failed")]
    SplitError(#[from] SplitError),
}

#[derive(Debug, Error, PartialEq)]
pub enum LeafPush {
    #[error("Self parent is not known to be the latest event of some leaf")]
    InvalidSelfParent,
    #[error("Inconsistent fork tracking index")]
    InconsistentState,
}

#[derive(Debug, Error, PartialEq)]
pub enum SplitError {
    // Also note implies that single-event extension is not splittable
    #[error(
        "Parent height is out of bounds for splitting the extension. \
        Note that parent cannot be the last event of the extension"
    )]
    HeightOutOfBounds,
    #[error(
        "Split occurs at the start of the extension, thus split \
        parent must be equal to the extension start."
    )]
    ParentStartMismatch,
    #[error(
        "Split occurs at the end of the extension, thus split \
        child must be equal to the extension end."
    )]
    ChildEndMismatch,
}

impl PeerIndexEntry {
    pub fn new(genesis: event::Hash) -> Self {
        let latest_event = genesis.clone();
        Self {
            genesis: genesis.clone(),
            authored_events: HashMap::from([(genesis.clone(), 0)]),
            fork_index: ForkIndex::new(genesis),
            latest_event,
        }
    }

    pub fn add_event<'a, F, TPayload>(
        &mut self,
        self_parent: event::Hash,
        event: event::Hash,
        event_lookup: F,
    ) -> Result<(), Error>
    where
        F: Fn(&event::Hash) -> Option<&'a event::Event<TPayload>>,
        TPayload: 'a,
    {
        // First do all checks, only then apply changes, to keep the state consistent

        // TODO: represent it in some way in the code. E.g. by creating 2 functions
        // where the first one may fail but works with immutable reference and the
        // second one does not fail but makes changes.
        // Otherwise it needs to be carefully reviewed by hand :(

        if self.authored_events.contains_key(&event) {
            return Err(Error::EventAlreadyKnown);
        }
        let parent_event = event_lookup(&self_parent).ok_or(Error::UnknownParent)?;
        let parent_height = self
            .authored_events
            .get(&self_parent)
            .ok_or(Error::UnknownParent)?;
        // Consider self children without the newly added event (just in case)
        let parent_self_children: event::SelfChild = parent_event
            .children
            .self_child
            .clone()
            .with_child_removed(&event);
        match parent_self_children {
            event::SelfChild::HonestParent(None) => {
                // It is a "leaf" in terms of forking, so we just add it to the index
                // Makes the final check and starts updating the state
                self.fork_index.push_event(event.clone(), &self_parent)?;
            }
            event::SelfChild::HonestParent(Some(firstborn)) => {
                // It is the second self child, so we creating a new fork
                let identifier = Self::find_fork_identifier(&self_parent, event_lookup)?;
                // completing the checks and starting to update the state
                self.fork_index.add_new_fork(
                    &identifier,
                    self_parent,
                    *parent_height,
                    event.clone(),
                    firstborn,
                )?;
            }
            event::SelfChild::ForkingParent(_) => {
                // Its parent already has forks, so we just add another one
                let identifier = Self::find_fork_identifier(&self_parent, event_lookup)?;
                // completing the checks and starting to update the state
                self.fork_index
                    .add_branch_to_fork(&identifier, event.clone())
                    .map_err(|e| <LookupIndexInconsistency>::from(e))?;
            }
        }
        self.authored_events.insert(event, parent_height + 1);
        Ok(())
    }

    fn find_fork_identifier<'a, F, TPayload>(
        target: &event::Hash,
        event_lookup: F,
    ) -> Result<event::Hash, Error>
    where
        F: Fn(&event::Hash) -> Option<&'a event::Event<TPayload>>,
        TPayload: 'a,
    {
        let mut this_event = event_lookup(target).ok_or(Error::InconsistentLookup)?;
        let mut prev_event = match this_event.parents() {
            event::Kind::Genesis => return Ok(target.clone()),
            event::Kind::Regular(parents) => {
                event_lookup(&parents.self_parent).ok_or(Error::InconsistentLookup)?
            }
        };
        while let event::Kind::Regular(parents) = prev_event.parents() {
            let prev_self_children: event::SelfChild = prev_event
                .children
                .self_child
                .clone()
                .with_child_removed(this_event.hash());
            if let event::SelfChild::ForkingParent(_) = prev_self_children {
                // `prev_event` has forking children, thus the child is the identifier we want
                return Ok(this_event.hash().clone());
            }
            this_event = prev_event;
            prev_event = event_lookup(&parents.self_parent).ok_or(Error::InconsistentLookup)?
        }
        // `prev_event` is genesis and thus it's the identifier
        Ok(prev_event.hash().clone())
    }

    pub fn latest_event(&self) -> &event::Hash {
        &self.latest_event
    }

    pub fn genesis(&self) -> &event::Hash {
        &self.genesis
    }
}

/// Node that represents a sequence of events without branching/forking
#[derive(Debug, Clone, PartialEq)]
struct Extension {
    first: event::Hash,
    first_height: usize,
    last: event::Hash,
    length: usize,
}

impl Extension {
    /// Construct extension consisting of a single event
    fn from_event(event: event::Hash, height: usize) -> Self {
        Self {
            first: event.clone(),
            first_height: height,
            last: event,
            length: 1,
        }
    }

    /// Add event to the end of extension.
    fn push_event(&mut self, event: event::Hash) {
        self.last = event;
        self.length += 1;
    }

    /// Split the extension [A, D] into [A, B] and [C, D], where B is `parent` and C is `child`
    fn split(
        self,
        parent: event::Hash,
        parent_height: usize,
        child: event::Hash,
    ) -> Result<(Self, Self), SplitError> {
        if parent_height < self.first_height || parent_height >= self.first_height + self.length - 1
        {
            return Err(SplitError::HeightOutOfBounds);
        }
        if parent_height == self.first_height && parent != self.first {
            return Err(SplitError::ParentStartMismatch);
        }
        // `self.first_height < parent_heigth < self.first_height + self.length`
        // is true from the first condition. These are all integers, so
        // length is at least 2. It means `length-2` won't panic
        if parent_height == self.first_height + (self.length - 2) && child != self.last {
            return Err(SplitError::ChildEndMismatch);
        }
        let first_part_length = parent_height - self.first_height + 1;
        let first_part = Extension {
            first: self.first,
            first_height: self.first_height,
            last: parent,
            length: first_part_length,
        };
        let second_part = Extension {
            first: child,
            first_height: parent_height + 1,
            last: self.last,
            length: self
                .length
                .checked_sub(first_part_length)
                .expect("Incorrect source length or some height"),
        };
        Ok((first_part, second_part))
    }
}

/// Sequence of events without any forks at the end
#[derive(Debug, Clone)]
pub struct Leaf(Extension);

impl Leaf {
    /// Insert a new leaf within this leaf, creating new fork.
    ///
    /// Returns `(parent_fork, first_child_leaf, new_child_leaf)` and
    /// `None` if `fork_parent_height` is incorrect (the parent or its child
    /// are out of bounds)
    fn attach_leaf(
        self,
        fork_parent: event::Hash,
        fork_parent_height: usize,
        firstborn: event::Hash,
        newborn: event::Hash,
    ) -> Result<(Fork, Leaf, Leaf), SplitError> {
        let (parent_ext, firstborn_ext) =
            self.0
                .split(fork_parent, fork_parent_height, firstborn.clone())?;
        let parent_fork = Fork {
            events: parent_ext,
            forks: vec![firstborn, newborn.clone()],
        };
        let firstborn_fork = Leaf(firstborn_ext);
        let newborn_leaf = Leaf(Extension::from_event(newborn, fork_parent_height + 1));
        Ok((parent_fork, firstborn_fork, newborn_leaf))
    }
}

/// Sequence of events with branching at the end
#[derive(Debug, Clone)]
pub struct Fork {
    events: Extension,
    /// First elements of each child
    forks: Vec<event::Hash>,
}

impl Fork {
    /// Insert a new leaf within this fork, creating new fork.
    ///
    /// Returns `(parent_fork, first_child_fork, new_child_leaf)` and
    /// `None` if `fork_parent_height` is incorrect (the parent or its child
    /// are out of bounds)
    fn attach_leaf(
        self,
        fork_parent: event::Hash,
        fork_parent_height: usize,
        firstborn: event::Hash,
        newborn: event::Hash,
    ) -> Result<(Fork, Fork, Leaf), SplitError> {
        let (parent_ext, firstborn_ext) =
            self.events
                .split(fork_parent, fork_parent_height, firstborn.clone())?;
        let parent_fork = Fork {
            events: parent_ext,
            forks: vec![firstborn, newborn.clone()],
        };
        let firstborn_fork = Fork {
            events: firstborn_ext,
            forks: self.forks,
        };
        let newborn_leaf = Leaf(Extension::from_event(newborn, fork_parent_height + 1));
        Ok((parent_fork, firstborn_fork, newborn_leaf))
    }
}

/// Tracks events created by a single peer with respect to self child/self parent
/// relation. Since by definition an event can only have a single self parent,
/// we can represent it as tree.
struct ForkIndex {
    // Sections of the peer graph that do not end with a fork. Indexed by their
    // first/starting element
    leafs: HashMap<event::Hash, Leaf>,
    // Index of the first element of a leaf by the last/ending element (for convenient
    // push)
    leaf_ends: HashMap<event::Hash, event::Hash>,
    forks: HashMap<event::Hash, Fork>,
    origin: event::Hash,
}

impl ForkIndex {
    pub fn new(genesis: event::Hash) -> Self {
        let mut new_index = Self {
            leafs: HashMap::new(),
            leaf_ends: HashMap::new(),
            forks: HashMap::new(),
            origin: genesis.clone(),
        };
        new_index.insert_leaf(Leaf(Extension::from_event(genesis, 0)));
        new_index
    }

    /// Add event to the end of a corresponding leaf.
    #[instrument(level = "trace", skip_all)]
    pub fn push_event(
        &mut self,
        event: event::Hash,
        self_parent: &event::Hash,
    ) -> Result<(), LeafPush> {
        let leaf_start = self
            .leaf_ends
            .get(self_parent)
            .ok_or(LeafPush::InvalidSelfParent)?
            .clone();
        let leaf = self
            .leafs
            .get_mut(&leaf_start)
            .ok_or(LeafPush::InconsistentState)?;
        if &leaf.0.last != self_parent {
            return Err(LeafPush::InconsistentState);
        }
        leaf.0.push_event(event.clone());
        self.leaf_ends.remove(self_parent);
        self.leaf_ends.insert(event, leaf_start);
        Ok(())
    }

    fn insert_fork(&mut self, fork: Fork) {
        self.forks.insert(fork.events.first.clone(), fork);
    }

    fn insert_leaf(&mut self, leaf: Leaf) {
        let first = leaf.0.first.clone();
        let last = leaf.0.last.clone();
        self.leaf_ends.insert(last, first.clone());
        self.leafs.insert(first, leaf);
    }

    fn remove_leaf(&mut self, leaf_identifier: &event::Hash) -> Option<Leaf> {
        // Separate block to ensure the borrow is dropped
        let leaf = self.leafs.remove(&leaf_identifier)?;
        let end_identifier = &leaf.0.last;
        self.leaf_ends.remove(&end_identifier);
        Some(leaf)
    }

    /// Add a new branch that starts with event `new_self_child` into the middle
    /// of [`Extension`] (thus creating a new fork).
    ///
    /// ## `previous_fork_identifier`
    /// Needs `previous_fork_identifier`; it is an event that starts the branch
    /// we insert the fork into
    ///
    /// In other words, it is a first self ancestor whose parent has multiple
    /// self children (or genesis event)
    ///
    /// ### Examples
    /// Let's say, we want to add new fork with event I after D:
    ///
    /// ```text
    ///           F
    ///          /
    ///     C-D-E
    ///    /     \
    /// A-B       G
    ///    \
    ///     H
    /// ```
    ///
    /// We use this procedure this way (assuming A has height 0 and thus C - 2)
    /// ```no_run
    /// add_new_fork(
    ///     previous_fork_identifier: "C",
    ///     forking_parent: "D",
    ///     forking_parent_height: 3,
    ///     new_self_child: "I",
    ///     other_self_child: "E",
    /// )
    /// ```
    /// It adds new leaf that consisting of single event "I" after event "D", which
    /// has another child - "E". Also, "D" belongs to [extension](Extension) identified
    /// by "C" at the height = 3.
    ///
    /// result:
    /// ```text
    ///           F
    ///          /
    ///     C-D-E
    ///    /   \ \
    /// A-B     I G
    ///    \
    ///     H
    /// ```
    #[instrument(level = "trace", skip_all)]
    pub fn add_new_fork(
        &mut self,
        extension_identifier: &event::Hash,
        forking_parent: event::Hash,
        forking_parent_height: usize,
        new_self_child: event::Hash,
        other_self_child: event::Hash,
    ) -> Result<(), LookupIndexInconsistency> {
        // Not `remove` because we don't want to arrive at
        // inconsistent state in case of error
        if let Some(fork) = self.forks.get(extension_identifier).cloned() {
            trace!("Found forking extension by ident, creating new fork within it");
            let (parent_fork, firstborn_fork, newborn_leaf) = fork.clone().attach_leaf(
                forking_parent,
                forking_parent_height,
                other_self_child,
                new_self_child,
            )?;
            // Now no errors can happen, so we're safe to remove
            self.forks.remove(extension_identifier);
            self.insert_fork(parent_fork);
            self.insert_fork(firstborn_fork);
            self.insert_leaf(newborn_leaf);
            Ok(())
        } else if let Some(leaf) = self.leafs.get(extension_identifier).cloned() {
            trace!("Found leaf extension by ident, creating new fork within it");
            let (parent_fork, firstborn_leaf, newborn_leaf) = leaf.attach_leaf(
                forking_parent,
                forking_parent_height,
                other_self_child,
                new_self_child,
            )?;
            // Now no errors can happen, so we're safe to remove
            self.remove_leaf(extension_identifier);
            self.insert_fork(parent_fork);
            self.insert_leaf(firstborn_leaf);
            self.insert_leaf(newborn_leaf);
            Ok(())
        } else {
            Err(LookupIndexInconsistency::InvalidIdentifier(
                InvalidForkIdentifier,
            ))
        }
    }

    /// Add a new branch that starts with event `new_self_child` to an existing
    /// fork OR at the leafs.
    ///
    /// To insert new fork use [`Self::add_new_fork()`].
    ///
    /// ## `previous_fork_identifier`
    /// Needs `previous_fork_identifier`; it is an event that starts the branch
    /// which resulted in the fork of interest.
    ///
    /// In other words, it is a first self ancestor whose parent has multiple
    /// self children (or genesis event)
    ///
    /// ### Examples
    /// Let's say, we want to add new forking event I in such fork tree:
    ///
    /// ```text
    ///           F
    ///          /
    ///     C-D-E
    ///    /     \
    /// A-B       G
    ///    \
    ///     H
    /// ```
    ///
    /// We use this procedure to add new forks after events B or E by calling
    /// #### B
    /// ```no_run
    /// add_branch_to_fork("A", "I")
    /// ```
    /// result:
    /// ```text
    ///           F
    ///          /
    ///     C-D-E
    ///    /     \
    /// A-B-I     G
    ///    \
    ///     H
    /// ```
    /// #### E
    /// ```no_run
    /// add_branch_to_fork("C", "I")
    /// ```
    /// result:
    /// ```text
    ///           F
    ///          /
    ///     C-D-E-I
    ///    /     \
    /// A-B       G
    ///    \
    ///     H
    /// ```
    #[instrument(level = "trace", skip_all)]
    pub fn add_branch_to_fork(
        &mut self,
        previous_fork_identifier: &event::Hash,
        new_self_child: event::Hash,
    ) -> Result<(), InvalidForkIdentifier> {
        let parent_fork = self
            .forks
            .get_mut(&previous_fork_identifier)
            .ok_or(InvalidForkIdentifier)?;
        // No need to add new `Fork`s
        parent_fork.forks.push(new_self_child.clone());
        let new_self_child_height = parent_fork.events.first_height + parent_fork.events.length;
        self.insert_leaf(Leaf(Extension::from_event(
            new_self_child,
            new_self_child_height,
        )));
        Ok(())
    }

    pub fn iter(&self) -> ForkIndexIter {
        ForkIndexIter {
            index: self,
            queued_entries: vec![&self.origin],
        }
    }
}

enum ForkIndexEntry<'a> {
    Fork(&'a Fork),
    Leaf(&'a Leaf),
}

impl<'a> ForkIndexEntry<'a> {
    fn extension(&self) -> &Extension {
        match self {
            ForkIndexEntry::Fork(f) => &f.events,
            ForkIndexEntry::Leaf(l) => &l.0,
        }
    }
}

struct ForkIndexIter<'a> {
    index: &'a ForkIndex,
    queued_entries: Vec<&'a event::Hash>,
}

/// Iterate over all forks/leafs. For a <parent> - <child> pair
/// in the index tree, never traverses child before parent
/// (basically DFS or BFS).
impl<'a> Iterator for ForkIndexIter<'a> {
    type Item = ForkIndexEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let next_entry = self.queued_entries.pop()?;
        if let Some(fork) = self.index.forks.get(next_entry) {
            let mut new_entries = fork.forks.iter().by_ref().clone().collect();
            self.queued_entries.append(&mut new_entries);
            Some(ForkIndexEntry::Fork(fork))
        } else if let Some(leaf) = self.index.leafs.get(next_entry) {
            Some(ForkIndexEntry::Leaf(leaf))
        } else {
            warn!(
                "Couldn't find fork {} in the index, likely the index is malformed",
                next_entry
            );
            None
        }
    }
}

// Tests became larger than the code, so for easier navigation I've moved them
#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        time::{Duration, Instant},
    };

    use super::*;
    use hex_literal::hex;

    const TEST_HASH_A: event::Hash = event::Hash::from_array(
        hex![
            "1c09ecaba3131425e5f04afb9e6ea029c363cdfbb17a04aff4946847d20bd85be6dbd9529a9b5bea3d63c967645ce28891e9994844fc6e0fdd0468d60fdf0300"
        ]
    );
    const TEST_HASH_B: event::Hash = event::Hash::from_array(
        hex![
            "66b4d625d5729f5a36fd918fbbda2cd38f636743708d489f9a35d0a62e7ca319b9db7939fbd129d0e8a3b4e00586acc88439e2bb7f9ba2beada06f1c34a6c065"
        ]
    );
    const TEST_HASH_C: event::Hash = event::Hash::from_array(
        hex![
            "65a3247180d90327a35f3662920336feb5e9487630294cdeb08ca25720998633ff95108c950200453ccb2ace1a4c774f4ae4887203900506576b38dd7fe93fd3"
        ]
    );
    const TEST_HASH_D: event::Hash = event::Hash::from_array(
        hex![
            "f537c5c9cf69000588d0bb69b835b7f3d062540e981acb82d748eeb102ad2a3b82cc228dba870fd0da5a9e7c5bf2669d3cb852520838599ecb52230ed15be1f4"
        ]
    );
    const TEST_HASH_E: event::Hash = event::Hash::from_array(
        hex![
            "f660614747149b2b9d324b503891d923bf96626f7cc8e0a5c2bc90e2803105d68e2710cd986b356626d067ef15b52af4caf29085e3ee8925104c3982020eb991"
        ]
    );
    const TEST_HASH_F: event::Hash = event::Hash::from_array(
        hex![
            "45d854c1bb52aa932940c6d80662961301f96f46d7f7fc9b5fc0a17d12d073fdc581dab54ee1e414a562ce354c74b2994935e4a8a843040336122add8e0a7086"
        ]
    );
    // For debugging
    #[allow(unused)]
    const NAMES: [(event::Hash, &str); 6] = [
        (TEST_HASH_A, "A"),
        (TEST_HASH_B, "B"),
        (TEST_HASH_C, "C"),
        (TEST_HASH_D, "D"),
        (TEST_HASH_E, "E"),
        (TEST_HASH_F, "F"),
    ];

    // For debugging
    #[allow(unused)]
    fn print_entry<F>(e: ForkIndexEntry, name_lookup: F)
    where
        F: Fn(&event::Hash) -> &str,
    {
        match e {
            ForkIndexEntry::Fork(_) => println!("Fork: "),
            ForkIndexEntry::Leaf(_) => println!("Leaf: "),
        }
        let ext = e.extension();
        print!(
            "events {}(at {})-({} events)->{}; then ",
            name_lookup(&ext.first),
            ext.first_height,
            ext.length,
            name_lookup(&ext.last)
        );
        match e {
            ForkIndexEntry::Fork(f) => {
                println!(
                    "forks to: {:?}",
                    f.forks
                        .iter()
                        .map(name_lookup)
                        .collect::<Vec<_>>()
                        .as_slice()
                );
            }
            ForkIndexEntry::Leaf(_) => println!("nothing."),
        }
    }

    // For debugging
    #[allow(unused)]
    fn print_fork_index<F>(index: &ForkIndex, name_lookup: F)
    where
        F: Fn(&event::Hash) -> &str,
    {
        for (_, fork) in &index.forks {
            let entry = ForkIndexEntry::Fork(&fork);
            print_entry(entry, &name_lookup);
        }
        for (_, leaf) in &index.leafs {
            let entry = ForkIndexEntry::Leaf(&leaf);
            print_entry(entry, &name_lookup);
        }
    }

    #[test]
    fn test_fork_index_constructs() {
        // Resulting layout; events are added in alphabetical order
        //   F E
        //  / /
        // A-B-C
        //    \
        //     D

        // A
        let mut index = ForkIndex::new(TEST_HASH_A);
        // B
        index.push_event(TEST_HASH_B, &TEST_HASH_A).unwrap();
        // C
        index.push_event(TEST_HASH_C, &TEST_HASH_B).unwrap();
        // D
        index
            .add_new_fork(&TEST_HASH_A, TEST_HASH_B, 1, TEST_HASH_D, TEST_HASH_C)
            .unwrap();
        // E
        index.add_branch_to_fork(&TEST_HASH_A, TEST_HASH_E).unwrap();
        // F
        index
            .add_new_fork(&TEST_HASH_A, TEST_HASH_A, 0, TEST_HASH_F, TEST_HASH_B)
            .unwrap();
    }

    #[test]
    fn test_fork_index_gives_errors() {
        // Resulting layout; events are added in alphabetical order
        //   F E
        //  / /
        // A-B-C
        //    \
        //     D

        // A
        let mut index = ForkIndex::new(TEST_HASH_A);
        // B
        assert_eq!(
            index.push_event(TEST_HASH_B, &TEST_HASH_B),
            Err(LeafPush::InvalidSelfParent)
        );
        index.push_event(TEST_HASH_B, &TEST_HASH_A).unwrap();
        // C
        index.push_event(TEST_HASH_C, &TEST_HASH_B).unwrap();
        // D
        assert_eq!(
            index.add_new_fork(&TEST_HASH_A, TEST_HASH_B, 2, TEST_HASH_D, TEST_HASH_C),
            Err(LookupIndexInconsistency::SplitError(
                SplitError::HeightOutOfBounds
            ))
        );
        assert_eq!(
            index.add_new_fork(&TEST_HASH_A, TEST_HASH_B, 1, TEST_HASH_D, TEST_HASH_A),
            Err(LookupIndexInconsistency::SplitError(
                SplitError::ChildEndMismatch
            ))
        );
        assert_eq!(
            index.add_new_fork(&TEST_HASH_D, TEST_HASH_B, 1, TEST_HASH_D, TEST_HASH_C),
            Err(LookupIndexInconsistency::InvalidIdentifier(
                InvalidForkIdentifier
            ))
        );
        // No need to test `forking_parent` as the index has no way of knowing if it is
        // correct in this case.
        // `new_self_child` as well, since it's just a new event.
        index
            .add_new_fork(&TEST_HASH_A, TEST_HASH_B, 1, TEST_HASH_D, TEST_HASH_C)
            .unwrap();
        // E
        assert_eq!(
            index.add_branch_to_fork(&TEST_HASH_B, TEST_HASH_E),
            Err(InvalidForkIdentifier)
        );
        index.add_branch_to_fork(&TEST_HASH_A, TEST_HASH_E).unwrap();
        // F
        assert_eq!(
            index.add_new_fork(&TEST_HASH_A, TEST_HASH_B, 0, TEST_HASH_F, TEST_HASH_B),
            Err(LookupIndexInconsistency::SplitError(
                SplitError::ParentStartMismatch
            ))
        );
        index
            .add_new_fork(&TEST_HASH_A, TEST_HASH_A, 0, TEST_HASH_F, TEST_HASH_B)
            .unwrap();
    }

    #[test]
    fn test_fork_index_iterates_correctly() {
        // Resulting layout; events are added in alphabetical order
        //   F E
        //  / /
        // A-B-C
        //    \
        //     D

        // We want to make sure we visit all events and don't visit
        // children before their parents.

        // Let's use these helpers to track it:

        enum ForkIndexEntryOwned {
            Fork(Fork),
            Leaf(Leaf),
        }

        impl ForkIndexEntryOwned {
            fn extension(&self) -> &Extension {
                match self {
                    ForkIndexEntryOwned::Fork(f) => &f.events,
                    ForkIndexEntryOwned::Leaf(l) => &l.0,
                }
            }
        }

        // key - identifier
        let mut entries_to_visit = HashMap::from([
            (
                TEST_HASH_A,
                ForkIndexEntryOwned::Fork(Fork {
                    events: Extension::from_event(TEST_HASH_A, 0),
                    forks: vec![TEST_HASH_B, TEST_HASH_F],
                }),
            ),
            (
                TEST_HASH_F,
                ForkIndexEntryOwned::Leaf(Leaf(Extension::from_event(TEST_HASH_F, 1))),
            ),
            (
                TEST_HASH_B,
                ForkIndexEntryOwned::Fork(Fork {
                    events: Extension::from_event(TEST_HASH_B, 1),
                    forks: vec![TEST_HASH_E, TEST_HASH_C, TEST_HASH_D],
                }),
            ),
            (
                TEST_HASH_E,
                ForkIndexEntryOwned::Leaf(Leaf(Extension::from_event(TEST_HASH_E, 2))),
            ),
            (
                TEST_HASH_C,
                ForkIndexEntryOwned::Leaf(Leaf(Extension::from_event(TEST_HASH_C, 2))),
            ),
            (
                TEST_HASH_D,
                ForkIndexEntryOwned::Leaf(Leaf(Extension::from_event(TEST_HASH_D, 2))),
            ),
        ]);
        let parent = HashMap::from([
            (TEST_HASH_B, TEST_HASH_A),
            (TEST_HASH_F, TEST_HASH_A),
            (TEST_HASH_C, TEST_HASH_B),
            (TEST_HASH_D, TEST_HASH_B),
            (TEST_HASH_E, TEST_HASH_B),
        ]);
        // let names = HashMap::from(NAMES);

        // A
        let mut index = ForkIndex::new(TEST_HASH_A);
        // println!("\nInserted A; state:");
        // print_fork_index(&index, |hash| names.get(hash).unwrap().clone());

        // B
        index.push_event(TEST_HASH_B, &TEST_HASH_A).unwrap();
        // println!("\nInserted B; state:");
        // print_fork_index(&index, |hash| names.get(hash).unwrap().clone());

        // C
        index.push_event(TEST_HASH_C, &TEST_HASH_B).unwrap();
        // println!("\nInserted C; state:");
        // print_fork_index(&index, |hash| names.get(hash).unwrap().clone());

        // D
        index
            .add_new_fork(&TEST_HASH_A, TEST_HASH_B, 1, TEST_HASH_D, TEST_HASH_C)
            .unwrap();
        // println!("\nInserted D; state:");
        // print_fork_index(&index, |hash| names.get(hash).unwrap().clone());

        // E
        index.add_branch_to_fork(&TEST_HASH_A, TEST_HASH_E).unwrap();
        // println!("\nInserted E; state:");
        // print_fork_index(&index, |hash| names.get(hash).unwrap().clone());

        // F
        index
            .add_new_fork(&TEST_HASH_A, TEST_HASH_A, 0, TEST_HASH_F, TEST_HASH_B)
            .unwrap();
        // println!("\nInserted F; state:");
        // print_fork_index(&index, |hash| names.get(hash).unwrap().clone());

        fn entries_equal(expected: ForkIndexEntryOwned, got: ForkIndexEntry) -> Result<(), String> {
            let ext_expected = expected.extension().clone();
            let ext_got = got.extension().clone();
            match (expected, got) {
                (ForkIndexEntryOwned::Fork(_), ForkIndexEntry::Leaf(_))
                | (ForkIndexEntryOwned::Leaf(_), ForkIndexEntry::Fork(_)) => {
                    return Err("Types mismatch".to_owned())
                }
                (ForkIndexEntryOwned::Fork(expected), ForkIndexEntry::Fork(got)) => {
                    let forks_expected = HashSet::<_>::from_iter(expected.forks);
                    let forks_got = HashSet::<_>::from_iter(got.forks.clone());
                    if forks_expected != forks_got {
                        return Err("Sets of forks are different".to_owned());
                    }
                }
                (ForkIndexEntryOwned::Leaf(_), ForkIndexEntry::Leaf(_)) => (),
            };
            if ext_expected != ext_got {
                return Err("Extensions are not equal".to_owned());
            }
            return Ok(());
        }

        for entry in index.iter() {
            let id = &entry.extension().first;
            if id != &TEST_HASH_A {
                let parent_id = parent.get(id).unwrap();

                if entries_to_visit.contains_key(parent_id) {
                    panic!("Visited child before parent ({} before {})", id, parent_id);
                }
            }
            let expected_entry = entries_to_visit.remove(id).unwrap();
            assert_eq!(Ok(()), entries_equal(expected_entry, entry));
        }
        assert!(entries_to_visit.is_empty());
    }

    #[test]
    fn test_peer_index_constructs() {
        // Resulting layout; events are added in alphabetical order
        //   F E
        //  / /
        // A-B-C
        //    \
        //     D

        let peer = 3u64;
        let start_time = Duration::from_secs(1674549572);
        let mut all_events = HashMap::new();

        // Since we work with actual events, we can't use our sample hashes.
        let event_a =
            event::Event::new((), event::Kind::Genesis, peer, start_time.as_secs().into()).unwrap();
        let a_hash = event_a.hash().clone();
        all_events.insert(a_hash.clone(), event_a);
        let mut index = PeerIndexEntry::new(a_hash.clone());

        let event_b = event::Event::new(
            (),
            event::Kind::Regular(event::Parents {
                self_parent: a_hash.clone(),
                // doesn't matter what to put here, we don't test it at all
                other_parent: a_hash.clone(),
            }),
            peer,
            (start_time + Duration::from_secs(1)).as_secs().into(),
        )
        .unwrap();
        let b_hash = event_b.hash().clone();
        all_events.insert(b_hash.clone(), event_b);
        all_events
            .get_mut(&a_hash)
            .unwrap()
            .children
            .self_child
            .add_child(b_hash.clone());
        index
            .add_event(a_hash.clone(), b_hash.clone(), |h| all_events.get(h))
            .unwrap();

        let event_c = event::Event::new(
            (),
            event::Kind::Regular(event::Parents {
                self_parent: b_hash.clone(),
                // doesn't matter what to put here, we don't test it at all
                other_parent: a_hash.clone(),
            }),
            peer,
            (start_time + Duration::from_secs(3)).as_secs().into(),
        )
        .unwrap();
        let c_hash = event_c.hash().clone();
        all_events.insert(c_hash.clone(), event_c);
        all_events
            .get_mut(&b_hash)
            .unwrap()
            .children
            .self_child
            .add_child(c_hash.clone());
        index
            .add_event(b_hash.clone(), c_hash.clone(), |h| all_events.get(h))
            .unwrap();

        let event_d = event::Event::new(
            (),
            event::Kind::Regular(event::Parents {
                self_parent: b_hash.clone(),
                // doesn't matter what to put here, we don't test it at all
                other_parent: a_hash.clone(),
            }),
            peer,
            (start_time + Duration::from_secs(4)).as_secs().into(),
        )
        .unwrap();
        let d_hash = event_d.hash().clone();
        all_events.insert(d_hash.clone(), event_d);
        all_events
            .get_mut(&b_hash)
            .unwrap()
            .children
            .self_child
            .add_child(d_hash.clone());
        index
            .add_event(b_hash.clone(), d_hash.clone(), |h| all_events.get(h))
            .unwrap();

        let event_e = event::Event::new(
            (),
            event::Kind::Regular(event::Parents {
                self_parent: b_hash.clone(),
                // doesn't matter what to put here, we don't test it at all
                other_parent: a_hash.clone(),
            }),
            peer,
            (start_time + Duration::from_secs(5)).as_secs().into(),
        )
        .unwrap();
        let e_hash = event_e.hash().clone();
        all_events.insert(e_hash.clone(), event_e);
        all_events
            .get_mut(&b_hash)
            .unwrap()
            .children
            .self_child
            .add_child(e_hash.clone());
        index
            .add_event(b_hash.clone(), e_hash.clone(), |h| all_events.get(h))
            .unwrap();

        let event_f = event::Event::new(
            (),
            event::Kind::Regular(event::Parents {
                self_parent: a_hash.clone(),
                // doesn't matter what to put here, we don't test it at all
                other_parent: a_hash.clone(),
            }),
            peer,
            (start_time + Duration::from_secs(6)).as_secs().into(),
        )
        .unwrap();
        let f_hash = event_f.hash().clone();
        all_events.insert(f_hash.clone(), event_f);
        all_events
            .get_mut(&a_hash)
            .unwrap()
            .children
            .self_child
            .add_child(f_hash.clone());
        index
            .add_event(a_hash.clone(), f_hash.clone(), |h| all_events.get(h))
            .unwrap();
    }
}
