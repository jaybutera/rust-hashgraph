use std::collections::{hash_map::Entry, HashMap, HashSet};

use derive_getters::Getters;

use crate::algorithm::event;

#[derive(Getters)]
pub struct ForkIndex {
    forks: HashMap<event::Hash, HashSet<event::Hash>>,
}

impl ForkIndex {
    pub fn new() -> Self {
        Self {
            forks: HashMap::new(),
        }
    }

    pub fn track_fork<TPayload, TPeerId>(
        &mut self,
        fork_parent: &event::EventWrapper<TPayload, TPeerId>,
        new_fork_child: event::Hash,
    ) {
        match self.forks.entry(fork_parent.inner().hash().clone()) {
            Entry::Occupied(mut fork) => {
                let fork = fork.get_mut();
                fork.insert(new_fork_child);
            }
            Entry::Vacant(place) => {
                let mut fork_children = fork_parent.children.self_child.clone();
                fork_children.add_child(new_fork_child);
                let fork_children: Vec<_> = fork_children.into();
                place.insert(HashSet::from_iter(fork_children.into_iter()));
            }
        }
    }
}
