use std::{collections::HashMap};

use thiserror::Error;

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

#[derive(Debug, Error)]
pub enum AddForkError {
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
}

#[derive(Debug, Error)]
#[error("Provided fork identifier is unknown")]
pub struct InvalidForkIdentifier;

#[derive(Debug, Error)]
pub enum LookupIndexInconsistency {
    #[error(transparent)]
    LeafPush(#[from] LeafPush),
    #[error(transparent)]
    InvalidIdentifier(#[from] InvalidForkIdentifier),
    #[error("Fork height is out of bounds of the fork")]
    InvalidHeight,
}

#[derive(Debug, Error)]
pub enum LeafPush {
    #[error("Self parent is not known to be the latest event of some leaf")]
    InvalidSelfParent,
    #[error("Inconsistent fork tracking index")]
    InconsistentState,
}

impl PeerIndexEntry {
    pub fn new(genesis: event::Hash) -> Self {
        let latest_event = genesis.clone();
        Self {
            genesis: genesis.clone(),
            authored_events: HashMap::new(),
            fork_index: ForkIndex::new(genesis),
            latest_event,
        }
    }

    pub fn add_event<TPayload>(
        &mut self,
        self_parent: event::Hash,
        event: event::Hash,
        event_lookup: &HashMap<event::Hash, event::Event<TPayload>>,
    ) -> Result<(), AddForkError>
    // where
    //     F: Fn(&event::Hash) -> Option<&event::Event<TPayload>>,
    {
        // First do all checks, only then apply changes, to keep the state consistent

        // TODO: represent it in some way in the code. E.g. by creating 2 functions
        // where the first one may fail but works with immutable reference and the
        // second one does not fail but makes changes.

        if self.authored_events.contains_key(&event) {
            return Err(AddForkError::EventAlreadyKnown);
        }
        let parent_event = event_lookup.get(&self_parent).ok_or(AddForkError::UnknownParent)?;
        let parent_height = self
            .authored_events
            .get(&self_parent)
            .ok_or(AddForkError::UnknownParent)?;
        // Consider self children without the newly added event
        let parent_self_children: event::SelfChild = parent_event
            .children
            .self_child
            .clone()
            .with_child_removed(&event);
        match parent_self_children {
            event::SelfChild::HonestParent(None) => {
                // It is a "leaf" in terms of forking, so we just add it to the index
                // Makes the final check and starts updating the state
                self.fork_index
                    .push_event(event.clone(), &self_parent)
                    .map_err(|e| <LookupIndexInconsistency>::from(e))?;
            }
            event::SelfChild::HonestParent(Some(firstborn)) => {
                // It is the second self child, so we creating a new fork
                let identifier = Self::find_fork_identifier(&self_parent, event_lookup)?;
                // checks are completed, start updating the state
                self.fork_index.add_new_fork(
                    &identifier,
                    self_parent,
                    *parent_height,
                    event.clone(),
                    firstborn,
                );
            }
            event::SelfChild::ForkingParent(_) => {
                // Its parent already has forks, so we just add another one
                let identifier = Self::find_fork_identifier(&self_parent, event_lookup)?;
                // checks are completed, start updating the state
                self.fork_index
                    .add_branch_to_fork(&identifier, event.clone());
            }
        }
        self.authored_events.insert(event, parent_height + 1);
        Ok(())
    }

    fn find_fork_identifier<TPayload>(
        target: &event::Hash,
        event_lookup: &HashMap<event::Hash, event::Event<TPayload>>,
    ) -> Result<event::Hash, AddForkError>
    // where
    //     F: Fn(&event::Hash) -> Option<&event::Event<TPayload>>,
    {
        let mut this_event = event_lookup.get(target).ok_or(AddForkError::InconsistentLookup)?;
        let mut prev_event = match this_event.parents() {
            event::Kind::Genesis => return Ok(target.clone()),
            event::Kind::Regular(parents) => {
                event_lookup.get(&parents.self_parent).ok_or(AddForkError::InconsistentLookup)?
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
            prev_event =
                event_lookup.get(&parents.self_parent).ok_or(AddForkError::InconsistentLookup)?
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
#[derive(Debug)]
struct Extension {
    first: event::Hash,
    first_height: usize,
    last: event::Hash,
    length: usize,
}

impl Extension {
    /// Split the extension [A, D] into [A, B] and [C, D], where B is `parent` and C is `child`
    ///
    /// `None` if `parent_height` is out of bounds of current extension or is the last element
    /// (so the child will be out of bound).
    fn split(
        self,
        parent: event::Hash,
        parent_height: usize,
        child: event::Hash,
    ) -> Option<(Self, Self)> {
        if parent_height < self.first_height || parent_height >= self.first_height + self.length {
            return None;
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
        Some((first_part, second_part))
    }
}

/// Sequence of events without any forks at the end
#[derive(Debug)]
struct Leaf(Extension);

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
    ) -> Option<(Fork, Leaf, Leaf)> {
        let (parent_ext, firstborn_ext) =
            self.0
                .split(fork_parent, fork_parent_height, firstborn.clone())?;
        let parent_fork = Fork {
            events: parent_ext,
            forks: vec![firstborn, newborn.clone()],
        };
        let firstborn_fork = Leaf(firstborn_ext);
        let newborn_leaf = Leaf(Extension {
            first: newborn.clone(),
            first_height: fork_parent_height + 1,
            last: newborn,
            length: 1,
        });
        Some((parent_fork, firstborn_fork, newborn_leaf))
    }
}

/// Sequence of events with branching at the end
#[derive(Debug)]
struct Fork {
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
    ) -> Option<(Fork, Fork, Leaf)> {
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
        let newborn_leaf = Leaf(Extension {
            first: newborn.clone(),
            first_height: fork_parent_height + 1,
            last: newborn,
            length: 1,
        });
        Some((parent_fork, firstborn_fork, newborn_leaf))
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
        Self {
            leafs: HashMap::new(),
            leaf_ends: HashMap::new(),
            forks: HashMap::new(),
            origin: genesis,
        }
    }

    /// Add event to the end of a corresponding leaf.
    pub fn push_event(
        &mut self,
        event: event::Hash,
        self_parent: &event::Hash,
    ) -> Result<(), LeafPush> {
        let leaf_start = self
            .leaf_ends
            .get(self_parent)
            .ok_or(LeafPush::InvalidSelfParent)?;
        let mut leaf = self
            .leafs
            .get_mut(leaf_start)
            .ok_or(LeafPush::InconsistentState)?;
        if &leaf.0.last != self_parent {
            return Err(LeafPush::InconsistentState);
        }
        leaf.0.last = event;
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
    pub fn add_new_fork(
        &mut self,
        previous_fork_identifier: &event::Hash,
        forking_parent: event::Hash,
        forking_parent_height: usize,
        new_self_child: event::Hash,
        other_self_child: event::Hash,
    ) -> Result<(), LookupIndexInconsistency> {
        if let Some(fork) = self.forks.remove(previous_fork_identifier) {
            let (parent_fork, firstborn_fork, newborn_leaf) = fork
                .attach_leaf(
                    forking_parent,
                    forking_parent_height,
                    other_self_child,
                    new_self_child,
                )
                .ok_or(LookupIndexInconsistency::InvalidHeight)?;
            self.insert_fork(parent_fork);
            self.insert_fork(firstborn_fork);
            self.insert_leaf(newborn_leaf);
            Ok(())
        } else if let Some(leaf) = self.remove_leaf(previous_fork_identifier) {
            let (parent_fork, firstborn_leaf, newborn_leaf) = leaf
                .attach_leaf(
                    forking_parent,
                    forking_parent_height,
                    other_self_child,
                    new_self_child,
                )
                .ok_or(LookupIndexInconsistency::InvalidHeight)?;
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
        parent_fork.forks.push(new_self_child);
        Ok(())
    }
}

enum ForkIndexEntry<'a> {
    Fork(&'a Fork),
    Leaf(&'a Leaf),
}

struct ForkIndexIterator<'a> {
    index: &'a ForkIndex,
    queued_entries: Vec<event::Hash>,
}

/// Iterate over all forks/leafs
impl<'a> Iterator for ForkIndexIterator<'a> {
    type Item = ForkIndexEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let next_entry = self.queued_entries.pop()?;
        if let Some(fork) = self.index.forks.get(&next_entry) {
            Some(ForkIndexEntry::Fork(fork))
        } else if let Some(leaf) = self.index.leafs.get(&next_entry) {
            Some(ForkIndexEntry::Leaf(leaf))
        } else {
            None
        }
    }
}
