use std::collections::{HashSet, VecDeque};
use std::hash::Hash;

pub trait Graph {
    type NodeIdentifier;

    fn neighbors(&self, node: &Self::NodeIdentifier) -> HashSet<Self::NodeIdentifier>;
}

pub trait DirectedGraph: Graph {
    /// Nodes without any incoming edhe
    fn start_nodes(&self) -> HashSet<&Self::NodeIdentifier>;
}

pub struct BfsWithStop<'a, G: Graph, F> {
    visited: HashSet<G::NodeIdentifier>,
    graph: &'a G,
    /// `true` if needs to stop at this node
    stop_condition: F,
    to_visit: VecDeque<&'a G::NodeIdentifier>,
}

// impl<'a, G, F> BfsWithStop<'a, G, F>
// where
//     G: Graph,
// {
//     pub fn new(stop_condition: F, graph: &G, starting_nodes: &[G::NodeIdentifier]) -> Self {
//         Self {
//             visited: HashSet::new(),
//             stop_condition,
//             graph,
//             to_visit: VecDeque::from_iter(starting_nodes.into_iter()),
//         }
//     }
// }

enum BfsStopNode<'a, G: Graph> {
    /// BFS continues after this node
    Ordinary(&'a G::NodeIdentifier),
    /// BFS did not add neighbors of this node
    Stopped(&'a G::NodeIdentifier),
}

impl<'a, G, F> BfsWithStop<'a, G, F>
where
    G: Graph,
    G::NodeIdentifier: Hash + Eq,
    F: Fn(&G::NodeIdentifier) -> bool,
{
    fn next(&mut self) -> Option<BfsStopNode<G>> {
        let next_node = self.to_visit.pop_front();
        if let Some(node) = next_node {
            if (self.stop_condition)(node) {
                Some(BfsStopNode::Stopped(node))
            } else {
                for neighbor in self.graph.neighbors(node) {
                    if !self.visited.contains(&neighbor) {
                        // self.to_visit.push_back(&neighbor);
                    }
                }
                Some(BfsStopNode::Ordinary(node))
            }
        } else {
            None
        }
    }
}
