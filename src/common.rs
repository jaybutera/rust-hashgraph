pub trait Graph {
    type NodeIdentifier;
    type NodeIdentifiers;

    fn neighbors(&self, node: &Self::NodeIdentifier) -> Option<Self::NodeIdentifiers>;
}

impl<G: Graph> Graph for &G {
    type NodeIdentifier = G::NodeIdentifier;
    type NodeIdentifiers = G::NodeIdentifiers;

    fn neighbors(&self, node: &Self::NodeIdentifier) -> Option<Self::NodeIdentifiers> {
        (*self).neighbors(node)
    }
}

pub trait Directed: Graph {
    fn in_neighbors(&self, node: &Self::NodeIdentifier) -> Option<Self::NodeIdentifiers>;
    fn out_neighbors(&self, node: &Self::NodeIdentifier) -> Option<Self::NodeIdentifiers>;
}

impl<G: Directed> Directed for &G {
    fn in_neighbors(&self, node: &Self::NodeIdentifier) -> Option<Self::NodeIdentifiers> {
        (*self).in_neighbors(node)
    }

    fn out_neighbors(&self, node: &Self::NodeIdentifier) -> Option<Self::NodeIdentifiers> {
        (*self).out_neighbors(node)
    }
}

pub trait Reversable<'a, TReverse = ReversedGraph<'a, Self>>: Directed {
    fn reversed(&'a self) -> TReverse;
}

impl<'a, G> Reversable<'a, ReversedGraph<'a, G>> for G
where
    G: Directed,
{
    fn reversed(&'a self) -> ReversedGraph<'a, G> {
        self.into()
    }
}

pub struct ReversedGraph<'a, G> {
    inner: &'a G,
}

impl<'a, G> From<&'a G> for ReversedGraph<'a, G> {
    fn from(value: &'a G) -> Self {
        ReversedGraph { inner: value }
    }
}

impl<'a, G> Graph for ReversedGraph<'a, G>
where
    G: Graph,
{
    type NodeIdentifier = G::NodeIdentifier;
    type NodeIdentifiers = G::NodeIdentifiers;

    fn neighbors(&self, node: &Self::NodeIdentifier) -> Option<Self::NodeIdentifiers> {
        self.inner.neighbors(node)
    }
}

impl<'a, G> Directed for ReversedGraph<'a, G>
where
    G: Directed,
{
    fn in_neighbors(&self, node: &Self::NodeIdentifier) -> Option<Self::NodeIdentifiers> {
        self.inner.out_neighbors(node)
    }

    fn out_neighbors(&self, node: &Self::NodeIdentifier) -> Option<Self::NodeIdentifiers> {
        self.inner.in_neighbors(node)
    }
}

impl<'a, G> Reversable<'a, &'a G> for ReversedGraph<'a, G>
where
    G: Directed,
{
    fn reversed(&'a self) -> &G
    where
        Self: Sized,
    {
        self.inner
    }
}
