//! The logic the contracts in the crate root are stated in: the type
//! invariant, the walk's invariant, and the lemmas their proofs call.

use crate::{
    StronglyConnectedComponents,
    TarjanWalk,
};
use creusot_std::prelude::{
    Int,
    Invariant,
    Seq,
    ensures,
    logic,
    requires,
    variant,
};

/// The value a node's discovery index or component holds while it has none.
#[logic(open)]
pub fn none() -> Int {
    pearlite! { usize::MAX@ }
}

/// What [`StronglyConnectedComponents::compute`](crate::StronglyConnectedComponents::compute)
/// requires: one `removed` flag per node, and every edge naming a node.
///
/// No edge list holds `usize::MAX` edges, since the largest allocation is
/// `isize::MAX` bytes. Creusot's model of `Vec` bounds a length by
/// `usize::MAX` alone, so the bound is spelled out.
#[logic(open)]
pub fn well_formed(adjacency: Seq<Vec<usize>>, removed: Seq<bool>) -> bool {
    pearlite! {
        removed.len() == adjacency.len()
            && forall<node> 0 <= node && node < adjacency.len() ==>
                adjacency[node]@.len() < usize::MAX@
                && forall<edge> 0 <= edge && edge < adjacency[node]@.len() ==>
                    adjacency[node]@[edge]@ < adjacency.len()
    }
}

/// How many of the first `len` entries of `discovery` are discovered.
#[logic(open)]
#[variant(len)]
pub fn discovered_prefix(discovery: Seq<usize>, len: Int) -> Int {
    pearlite! {
        if len <= 0 {
            0
        } else {
            discovered_prefix(discovery, len - 1)
                + if discovery[len - 1]@ == none() { 0 } else { 1 }
        }
    }
}

#[logic]
#[requires(0 <= len && len <= discovery.len())]
#[ensures(0 <= discovered_prefix(discovery, len) && discovered_prefix(discovery, len) <= len)]
#[variant(len)]
pub fn discovered_prefix_bounds(discovery: Seq<usize>, len: Int) {
    pearlite! {
        if len > 0 {
            discovered_prefix_bounds(discovery, len - 1)
        }
    }
}

#[logic]
#[requires(0 <= len && len <= discovery.len())]
#[requires(forall<node> 0 <= node && node < len ==> discovery[node]@ == none())]
#[ensures(discovered_prefix(discovery, len) == 0)]
#[variant(len)]
pub fn discovered_prefix_of_undiscovered(discovery: Seq<usize>, len: Int) {
    pearlite! {
        if len > 0 {
            discovered_prefix_of_undiscovered(discovery, len - 1)
        }
    }
}

#[logic]
#[requires(0 <= len && len <= node && node < discovery.len())]
#[ensures(discovered_prefix(discovery.set(node, index), len) == discovered_prefix(discovery, len))]
#[variant(len)]
pub fn discovered_prefix_before(discovery: Seq<usize>, node: Int, index: usize, len: Int) {
    pearlite! {
        if len > 0 {
            discovered_prefix_before(discovery, node, index, len - 1)
        }
    }
}

/// Discovering one node adds one to every prefix that holds it.
#[logic]
#[requires(0 <= node && node < len && len <= discovery.len())]
#[requires(discovery[node]@ == none() && index@ != none())]
#[ensures(discovered_prefix(discovery.set(node, index), len) == discovered_prefix(discovery, len) + 1)]
#[variant(len)]
pub fn discovered_prefix_discover(discovery: Seq<usize>, node: Int, index: usize, len: Int) {
    pearlite! {
        if len - 1 > node {
            discovered_prefix_discover(discovery, node, index, len - 1)
        } else {
            discovered_prefix_before(discovery, node, index, node)
        }
    }
}

/// A prefix holding an undiscovered node is not wholly discovered.
#[logic]
#[requires(0 <= node && node < len && len <= discovery.len())]
#[requires(discovery[node]@ == none())]
#[ensures(discovered_prefix(discovery, len) < len)]
#[variant(len)]
pub fn discovered_prefix_missing(discovery: Seq<usize>, node: Int, len: Int) {
    pearlite! {
        if len - 1 > node {
            discovered_prefix_missing(discovery, node, len - 1)
        } else {
            discovered_prefix_bounds(discovery, len - 1)
        }
    }
}

/// `node` is somewhere on `stack`.
#[logic(open)]
pub fn stacked(stack: Seq<usize>, node: Int) -> bool {
    pearlite! {
        exists<position> 0 <= position && position < stack.len() && stack[position]@ == node
    }
}

/// Pushing keeps every stacked node and stacks the pushed one.
#[logic]
#[ensures(forall<node> stacked(stack, node) ==> stacked(stack.snoc(item), node))]
#[ensures(stacked(stack.snoc(item), item@))]
pub fn stacked_push(stack: Seq<usize>, item: usize) {}

/// Shortening a prefix of `base` by one keeps every stacked node but the
/// one it drops.
#[logic]
#[requires(0 < len && len <= base.len())]
#[ensures(forall<node> stacked(base.subsequence(0, len), node) && base[len - 1]@ != node ==>
    stacked(base.subsequence(0, len - 1), node))]
pub fn stacked_prefix_pop(base: Seq<usize>, len: Int) {}

#[cfg(creusot)]
impl Invariant for StronglyConnectedComponents {
    #[logic(open(crate))]
    fn invariant(self) -> bool {
        pearlite! {
            forall<node> 0 <= node && node < self.component_of@.len() ==>
                self.component_of@[node]@ == none()
                    || self.component_of@[node]@ < self.component_size@.len()
        }
    }
}

#[cfg(creusot)]
impl StronglyConnectedComponents {
    /// [`Self::component_ids`] in the logic.
    #[logic(open(crate))]
    pub fn ids(self) -> Seq<usize> {
        pearlite! { self.component_of@ }
    }

    /// [`Self::component_count`] in the logic.
    #[logic(open(crate))]
    pub fn count(self) -> Int {
        pearlite! { self.component_size@.len() }
    }

    /// The number of nodes in each component.
    #[logic(open(crate))]
    pub fn sizes(self) -> Seq<usize> {
        pearlite! { self.component_size@ }
    }
}

/// The walk's invariant. Each predicate reads the node count off
/// `discovery`, so only the frame bounds need the graph itself.
#[cfg(creusot)]
impl TarjanWalk {
    #[logic(open(crate), inline)]
    pub(crate) fn node_count(self) -> Int {
        pearlite! { self.discovery@.len() }
    }

    /// Every per-node vector has one entry per node.
    #[logic(open(crate), inline)]
    pub(crate) fn sized(self) -> bool {
        pearlite! {
            self.low_link@.len() == self.node_count()
                && self.on_stack@.len() == self.node_count()
                && self.component_of@.len() == self.node_count()
        }
    }

    /// Every stacked node is a discovered node marked on the stack.
    #[logic(open(crate), inline)]
    pub(crate) fn stack_marked(self) -> bool {
        pearlite! {
            forall<position> 0 <= position && position < self.stack@.len() ==>
                self.stack@[position]@ < self.node_count()
                && self.on_stack@[self.stack@[position]@]
                && self.discovery@[self.stack@[position]@]@ != none()
        }
    }

    /// The stack is in discovery order.
    #[logic(open(crate), inline)]
    pub(crate) fn stack_sorted(self) -> bool {
        pearlite! {
            forall<lower, upper> 0 <= lower && lower < upper && upper < self.stack@.len() ==>
                self.discovery@[self.stack@[lower]@]@ < self.discovery@[self.stack@[upper]@]@
        }
    }

    /// Every node marked on the stack is on it.
    #[logic(open(crate), inline)]
    pub(crate) fn marks_stacked(self) -> bool {
        pearlite! {
            forall<node> 0 <= node && node < self.node_count() && self.on_stack@[node] ==>
                stacked(self.stack@, node)
        }
    }

    /// The stack holds exactly the nodes marked on it, in discovery order.
    #[logic(open(crate), inline)]
    pub(crate) fn stack_ordered(self) -> bool {
        pearlite! { self.stack_marked() && self.stack_sorted() && self.marks_stacked() }
    }

    /// The open frames are stacked nodes in discovery order.
    #[logic(open(crate), inline)]
    pub(crate) fn frames_ordered(self) -> bool {
        pearlite! {
            (forall<depth> 0 <= depth && depth < self.frames@.len() ==>
                self.frames@[depth].0@ < self.node_count()
                && self.on_stack@[self.frames@[depth].0@])
            && (forall<lower, upper> 0 <= lower && lower < upper && upper < self.frames@.len() ==>
                self.discovery@[self.frames@[lower].0@]@ < self.discovery@[self.frames@[upper].0@]@)
        }
    }

    /// The bottom frame is the bottom of the stack, and the stack empties
    /// with the frames.
    #[logic(open(crate), inline)]
    pub(crate) fn frames_rooted(self) -> bool {
        pearlite! {
            (self.frames@.len() > 0 ==>
                self.stack@.len() > 0 && self.frames@[0].0 == self.stack@[0])
            && (self.frames@.len() == 0 ==> self.stack@.len() == 0)
        }
    }

    /// `next_discovery` counts the discovered nodes, and every component
    /// took at least one of them off the stack.
    #[logic(open(crate), inline)]
    pub(crate) fn counted(self) -> bool {
        pearlite! {
            self.next_discovery@ == discovered_prefix(self.discovery@, self.node_count())
            && self.next_discovery@ <= self.node_count()
            && (forall<node> 0 <= node && node < self.node_count()
                && self.discovery@[node]@ != none() ==>
                self.discovery@[node]@ < self.next_discovery@)
            && self.component_size@.len() + self.stack@.len() <= self.next_discovery@
        }
    }

    /// A discovered node is on the stack or in a component; an undiscovered
    /// one is neither.
    #[logic(open(crate), inline)]
    pub(crate) fn labelled(self) -> bool {
        pearlite! {
            forall<node> 0 <= node && node < self.node_count() ==>
                (self.discovery@[node]@ == none() ==>
                    !self.on_stack@[node] && self.component_of@[node]@ == none())
                && (self.discovery@[node]@ != none() ==>
                    self.on_stack@[node] || self.component_of@[node]@ != none())
                && (self.component_of@[node]@ == none()
                    || self.component_of@[node]@ < self.component_size@.len())
        }
    }

    /// A stacked node's low link lies between the discovery index of the
    /// bottom of the stack and its own.
    #[logic(open(crate), inline)]
    pub(crate) fn low_links_bounded(self) -> bool {
        pearlite! {
            forall<node> 0 <= node && node < self.node_count() && self.on_stack@[node] ==>
                self.low_link@[node]@ <= self.discovery@[node]@
                && self.discovery@[self.stack@[0]@]@ <= self.low_link@[node]@
        }
    }

    /// Everything but the link between the frames and the stack, which
    /// `finish_node` breaks while it closes a component.
    #[logic(open(crate), inline)]
    pub(crate) fn core_unrooted(self) -> bool {
        pearlite! {
            self.sized()
                && self.stack_ordered()
                && self.frames_ordered()
                && self.counted()
                && self.labelled()
                && self.low_links_bounded()
        }
    }

    #[logic(open(crate), inline)]
    pub(crate) fn core(self) -> bool {
        pearlite! { self.core_unrooted() && self.frames_rooted() }
    }

    /// No removed node is discovered.
    #[logic(open(crate), inline)]
    pub(crate) fn respects(self, removed: Seq<bool>) -> bool {
        pearlite! {
            removed.len() == self.node_count()
                && forall<node> 0 <= node && node < removed.len() && removed[node] ==>
                    self.discovery@[node]@ == none()
        }
    }

    /// Each frame's edge position is within its edge list, the top one
    /// possibly `top_slack` past it, which is how `run` hands a finished
    /// frame to `finish_node`.
    #[logic(open(crate), inline)]
    pub(crate) fn frames_within(self, adjacency: Seq<Vec<usize>>, top_slack: Int) -> bool {
        pearlite! {
            forall<depth> 0 <= depth && depth < self.frames@.len() ==>
                self.frames@[depth].0@ < adjacency.len()
                && self.frames@[depth].1@ <= adjacency[self.frames@[depth].0@]@.len()
                    + if depth == self.frames@.len() - 1 { top_slack } else { 0 }
        }
    }

    /// Nothing discovered in `self` is undiscovered in `later`.
    #[logic(open(crate), inline)]
    pub(crate) fn discovered_within(self, later: Self) -> bool {
        pearlite! {
            forall<node> 0 <= node && node < self.node_count()
                && self.discovery@[node]@ != none() ==>
                later.discovery@[node]@ != none()
        }
    }

    /// `pop_component` part way: `size` members of `before`'s stack moved
    /// into `component`, and `root` still stacked.
    #[logic(open(crate), inline)]
    pub(crate) fn popping(self, before: Self, root: Int, component: Int, size: Int) -> bool {
        pearlite! {
            self.stack@ == before.stack@.subsequence(0, self.stack@.len())
            && size + self.stack@.len() == before.stack@.len()
            && stacked(self.stack@, root)
            && (forall<node> 0 <= node && node < before.node_count() && self.on_stack@[node] ==>
                stacked(self.stack@, node))
            && self.on_stack@.len() == before.on_stack@.len()
            && self.component_of@.len() == before.component_of@.len()
            && (forall<node> 0 <= node && node < before.node_count() ==>
                self.on_stack@[node] == (before.on_stack@[node]
                    && (self.stack@.len() == before.stack@.len()
                        || self.discovery@[node] < self.discovery@[before.stack@[self.stack@.len()]@])))
            && (forall<node> 0 <= node && node < before.node_count() ==>
                self.component_of@[node]@ == if before.on_stack@[node] && !self.on_stack@[node] {
                    component
                } else {
                    before.component_of@[node]@
                })
            && self.discovery@ == before.discovery@
            && self.low_link@ == before.low_link@
            && self.frames@ == before.frames@
            && self.next_discovery == before.next_discovery
            && self.component_size@ == before.component_size@
        }
    }
}
