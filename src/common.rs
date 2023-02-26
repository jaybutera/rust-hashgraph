use std::marker::PhantomData;

use petgraph::visit::{GraphBase, VisitMap, Visitable};

struct FilteredVisitMap<'a, TNode, TFilter, TInnerMap> {
    visited: TInnerMap,
    filter: &'a TFilter,
    _node: PhantomData<TNode>,
}

impl<'a, TNode, TFilter, TInnerMap> FilteredVisitMap<'a, TNode, TFilter, TInnerMap> {
    /// `filter` should return `true` if this node can be visited,
    /// false otherwise.
    fn new(inner_map: TInnerMap, filter: &'a TFilter) -> Self {
        Self {
            visited: inner_map,
            filter,
            _node: PhantomData,
        }
    }
}

impl<'a, TNode, TFilter, TInnerMap> VisitMap<TNode>
    for FilteredVisitMap<'a, TNode, TFilter, TInnerMap>
where
    TNode: std::hash::Hash + Eq + Clone,
    TFilter: Fn(&TNode) -> bool,
    TInnerMap: VisitMap<TNode>,
{
    fn visit(&mut self, a: TNode) -> bool {
        let self_visited = self.is_visited(&a);
        let inner_visited = self.visited.visit(a);
        self_visited || inner_visited
    }

    fn is_visited(&self, a: &TNode) -> bool {
        self.visited.is_visited(a) || !(self.filter)(&a)
    }
}

pub struct GraphVisitableFilter<TGraph, TFilter> {
    graph: TGraph,
    filter: TFilter,
}

impl<TGraph, TFilter> GraphVisitableFilter<TGraph, TFilter> {
    pub fn new(graph: TGraph, filter: TFilter) -> Self {
        Self { graph, filter }
    }
}

impl<TGraph, TFilter> GraphBase for GraphVisitableFilter<TGraph, TFilter>
where
    TGraph: GraphBase,
{
    type EdgeId = TGraph::EdgeId;
    type NodeId = TGraph::NodeId;
}

impl<'a, TGraph, TFilter> Visitable for &'a GraphVisitableFilter<TGraph, TFilter>
where
    TGraph: Visitable,
    TGraph::NodeId: std::hash::Hash + Eq,
    TFilter: Fn(&TGraph::NodeId) -> bool,
{
    type Map = FilteredVisitMap<'a, TGraph::NodeId, TFilter, TGraph::Map>;

    fn visit_map(self: &Self) -> Self::Map {
        FilteredVisitMap::new(self.graph.visit_map(), &self.filter)
    }

    fn reset_map(self: &Self, map: &mut Self::Map) {
        self.graph.reset_map(&mut map.visited)
    }
}
