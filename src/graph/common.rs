trait Graph {
    type Identifier;
    type Node;

    fn get_node(identifier: Identifier) -> Node;
    fn neighbors(node: Node) -> Vec<Identifier>;
}

trait Visitable: Graph {
    
}