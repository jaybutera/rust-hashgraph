use std::{collections::HashMap, num::NonZeroU8};

use derive_getters::Getters;
use thiserror::Error;
use tracing::{instrument, trace, warn};

pub use self::utils::Extension;
use self::utils::SplitError;
use crate::algorithm::event;

#[cfg(test)]
pub use self::utils::test_utils;

mod utils;

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
        let this_submul = self.0.submultiple();
        let (parent_ext, firstborn_ext) =
            self.0
                .split_at(fork_parent, fork_parent_height, firstborn.clone())?;
        let parent_fork = Fork {
            events: parent_ext,
            forks: vec![firstborn, newborn.clone()],
        };
        let firstborn_fork = Leaf(firstborn_ext);
        let newborn_leaf = Leaf(
            Extension::from_event_with_submultiple(newborn, fork_parent_height + 1, this_submul)
                .expect("Submultiple was correct before, so it must be here as well"),
        );
        Ok((parent_fork, firstborn_fork, newborn_leaf))
    }

    pub fn events(&self) -> &Extension {
        &self.0
    }
}

/// Sequence of events with branching at the end
#[derive(Debug, Clone, Getters)]
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
        let this_submul = self.events.submultiple();
        let (parent_ext, firstborn_ext) =
            self.events
                .split_at(fork_parent, fork_parent_height, firstborn.clone())?;
        let parent_fork = Fork {
            events: parent_ext,
            forks: vec![firstborn, newborn.clone()],
        };
        let firstborn_fork = Fork {
            events: firstborn_ext,
            forks: self.forks,
        };
        let newborn_leaf = Leaf(
            Extension::from_event_with_submultiple(newborn, fork_parent_height + 1, this_submul)
                .expect("Submultiple was correct before, so it must be here as well"),
        );
        Ok((parent_fork, firstborn_fork, newborn_leaf))
    }

    pub fn subsequent_forks(&self) -> &Vec<event::Hash> {
        &self.forks
    }
}

/// Tracks events created by a single peer with respect to self child/self parent
/// relation. Since by definition an event can only have a single self parent,
/// we can represent it as tree.
#[derive(Getters)]
pub struct ForkIndex {
    /// Sections of the peer graph that do not end with a fork. Indexed by their
    /// first/starting element
    leafs: HashMap<event::Hash, Leaf>,
    /// Index of the first element of a leaf by the last/ending element (for convenient
    /// push)
    leaf_starts: HashMap<event::Hash, event::Hash>,
    forks: HashMap<event::Hash, Fork>,
    /// Index of the first element of a fork by the last/ending element (for convenient
    /// sync)??? Todo: remove???
    fork_starts: HashMap<event::Hash, event::Hash>,
    origin: event::Hash,
    // Should be consistent with actual submultiples inside since they're propagated
    // on creation without changes
    //
    /// Submultiple for tracking intermediate events in [`Extension`]s
    submultiple: NonZeroU8,
}

impl ForkIndex {
    /// Create new fork index with origin event `genesis` and
    /// `submultiple` - base for multipliers of tracking long
    /// extensions.
    pub fn new(genesis: event::Hash, submultiple: NonZeroU8) -> Self {
        let mut new_index = Self {
            leafs: HashMap::new(),
            leaf_starts: HashMap::new(),
            forks: HashMap::new(),
            fork_starts: HashMap::new(),
            origin: genesis.clone(),
            submultiple,
        };
        new_index.insert_leaf(Leaf(
            Extension::from_event_with_submultiple(genesis, 0, submultiple.into())
                .expect("Nonzero static type"),
        ));
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
            .leaf_starts
            .get(self_parent)
            .ok_or(LeafPush::InvalidSelfParent)?
            .clone();
        let leaf = self
            .leafs
            .get_mut(&leaf_start)
            .ok_or(LeafPush::InconsistentState)?;
        if leaf.0.last_event() != self_parent {
            return Err(LeafPush::InconsistentState);
        }
        leaf.0.push_event(event.clone());
        self.leaf_starts.remove(self_parent);
        self.leaf_starts.insert(event, leaf_start);
        Ok(())
    }

    fn insert_fork(&mut self, fork: Fork) {
        self.fork_starts.insert(
            fork.events.last_event().clone(),
            fork.events.first_event().clone(),
        );
        self.forks.insert(fork.events.first_event().clone(), fork);
    }

    fn remove_fork(&mut self, fork_identifier: &event::Hash) -> Option<Fork> {
        // Separate block to ensure the borrow is dropped
        let fork = self.forks.remove(&fork_identifier)?;
        let end_identifier = &fork.events.last_event();
        self.fork_starts.remove(end_identifier);
        Some(fork)
    }

    fn insert_leaf(&mut self, leaf: Leaf) {
        let first = leaf.0.first_event().clone();
        let last = leaf.0.last_event().clone();
        self.leaf_starts.insert(last, first.clone());
        self.leafs.insert(first, leaf);
    }

    fn remove_leaf(&mut self, leaf_identifier: &event::Hash) -> Option<Leaf> {
        // Separate block to ensure the borrow is dropped
        let leaf = self.leafs.remove(&leaf_identifier)?;
        let end_identifier = &leaf.0.last_event();
        self.leaf_starts.remove(&end_identifier);
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
    /// ```rust,ignore
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
            self.remove_fork(extension_identifier);
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
    /// ```rust,ignore
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
    /// ```rust,ignore
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
        let new_self_child_height = parent_fork.events.first_height() + parent_fork.events.length();
        let parent_submul = parent_fork.events.submultiple();
        self.insert_leaf(Leaf(
            Extension::from_event_with_submultiple(
                new_self_child,
                new_self_child_height,
                parent_submul,
            )
            .expect("Submultiple was correct before, so it must be here as well"),
        ));
        Ok(())
    }

    pub fn iter(&self) -> ForkIndexIter {
        ForkIndexIter {
            index: self,
            queued_entries: vec![&self.origin],
        }
    }

    pub fn leaf_events(&self) -> Vec<&event::Hash> {
        self.leaf_starts.keys().collect()
    }

    // pub fn submultiple(&self) -> NonZeroU8 {
    //     self.submultiple
    // }

    /// Get fork either by its start or end.
    fn find_fork(&self, event: &event::Hash) -> Option<&Fork> {
        self.forks
            .get(event)
            .or_else(|| self.forks.get(self.fork_starts.get(event)?))
    }

    /// Get fork either by its start or end.
    fn find_leaf(&self, event: &event::Hash) -> Option<&Leaf> {
        self.leafs
            .get(event)
            .or_else(|| self.leafs.get(self.leaf_starts.get(event)?))
    }

    // /// Find extension by its start or end.
    // pub fn find_extension(&self, event: &event::Hash) -> Option<&Extension> {
    //     self.find_leaf(event)
    //         .map(|leaf| leaf.events())
    //         .or_else(|| self.find_fork(event).map(|fork| fork.events()))
    // }

    /// Find extension by its start.
    pub fn find_extension(&self, start: &event::Hash) -> Option<&Extension> {
        self.leafs
            .get(start)
            .map(|leaf| leaf.events())
            .or_else(|| self.forks.get(start).map(|fork| fork.events()))
    }
}

pub enum ForkIndexEntry<'a> {
    Fork(&'a Fork),
    Leaf(&'a Leaf),
}

impl<'a> ForkIndexEntry<'a> {
    pub fn extension(&self) -> &Extension {
        match self {
            ForkIndexEntry::Fork(f) => &f.events,
            ForkIndexEntry::Leaf(l) => &l.0,
        }
    }
}

pub struct ForkIndexIter<'a> {
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use utils::test_utils::*;

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
            name_lookup(ext.first_event()),
            ext.first_height(),
            ext.length(),
            name_lookup(&ext.last_event())
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
        let mut index = ForkIndex::new(TEST_HASH_A, NonZeroU8::new(3u8).unwrap());
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
        let mut index = ForkIndex::new(TEST_HASH_A, NonZeroU8::new(3u8).unwrap());
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
                    events: Extension::from_event_with_submultiple(TEST_HASH_A, 0, 3u8)
                        .expect("nonzero"),
                    forks: vec![TEST_HASH_B, TEST_HASH_F],
                }),
            ),
            (
                TEST_HASH_F,
                ForkIndexEntryOwned::Leaf(Leaf(
                    Extension::from_event_with_submultiple(TEST_HASH_F, 1, 3u8).expect("nonzero"),
                )),
            ),
            (
                TEST_HASH_B,
                ForkIndexEntryOwned::Fork(Fork {
                    events: Extension::from_event_with_submultiple(TEST_HASH_B, 1, 3u8)
                        .expect("nonzero"),
                    forks: vec![TEST_HASH_E, TEST_HASH_C, TEST_HASH_D],
                }),
            ),
            (
                TEST_HASH_E,
                ForkIndexEntryOwned::Leaf(Leaf(
                    Extension::from_event_with_submultiple(TEST_HASH_E, 2, 3u8).expect("nonzero"),
                )),
            ),
            (
                TEST_HASH_C,
                ForkIndexEntryOwned::Leaf(Leaf(
                    Extension::from_event_with_submultiple(TEST_HASH_C, 2, 3u8).expect("nonzero"),
                )),
            ),
            (
                TEST_HASH_D,
                ForkIndexEntryOwned::Leaf(Leaf(
                    Extension::from_event_with_submultiple(TEST_HASH_D, 2, 3u8).expect("nonzero"),
                )),
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
        let mut index = ForkIndex::new(TEST_HASH_A, NonZeroU8::new(3u8).unwrap());
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
            let id = entry.extension().first_event();
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
}
