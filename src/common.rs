pub trait Graph {
    type NodeIdentifier;
    type NodeIdentifiers: Iterator<Item = Self::NodeIdentifier>;

    fn neighbors(&self, node: &Self::NodeIdentifier) -> Self::NodeIdentifiers;
}

pub trait GraphWithStart: Graph {
    fn starting_nodes(&self) -> Self::NodeIdentifiers;
}
