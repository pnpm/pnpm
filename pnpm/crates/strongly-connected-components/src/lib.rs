//! Strongly connected components of a graph given as adjacency lists, by an
//! iterative Tarjan walk.
//!
//! Creusot proves this crate, see `pnpm/VERIFICATION.md`:
//!
//! - No call panics or overflows when the graph is well formed. That covers
//!   every index, and the `expect`s on the walk's frames and on its stack.
//! - [`StronglyConnectedComponents::compute`] gives every included node a
//!   component below [`StronglyConnectedComponents::component_count`], and
//!   every removed node [`usize::MAX`].
//! - [`StronglyConnectedComponents`] keeps that labelling as a type
//!   invariant, so each method that indexes by a component relies on it.
//!
//! That a component is strongly connected, and the order
//! [`StronglyConnectedComponents::components`] lists components and members
//! in, are tested, not proved.

#[cfg(creusot)]
use creusot_std::prelude::{
    ensures,
    invariant,
    proof_assert,
    requires,
    snapshot,
};
#[cfg(creusot)]
use lemmas::{
    discovered_prefix_bounds,
    discovered_prefix_discover,
    discovered_prefix_missing,
    discovered_prefix_of_undiscovered,
    stacked_prefix_pop,
    stacked_push,
};
#[cfg(creusot)]
use logic::{
    discovered_prefix,
    none,
    stacked,
    well_formed,
};
#[cfg(creusot)]
use pnpm_creusot_models as _;

#[cfg(creusot)]
pub mod lemmas;
#[cfg(creusot)]
pub mod logic;

/// The strongly connected components of the not-yet-removed subgraph,
/// computed with an iterative Tarjan walk (recursion would overflow on a
/// workspace-deep chain). Removed nodes belong to no component.
pub struct StronglyConnectedComponents {
    component_of: Vec<usize>,
    components: Vec<Vec<usize>>,
}

impl StronglyConnectedComponents {
    const NONE: usize = usize::MAX;

    /// Partitions the graph, excluding nodes marked in `removed`.
    ///
    /// Edges must contain valid node indices and `removed` must have one entry per node.
    #[cfg_attr(creusot, requires(well_formed(adjacency@, removed@)))]
    #[cfg_attr(creusot, ensures(result.ids().len() == adjacency@.len()))]
    #[cfg_attr(creusot, ensures(forall<node> 0 <= node && node < adjacency@.len() ==>
        (removed@[node] ==> result.ids()[node]@ == none())
        && (!removed@[node] ==> result.ids()[node]@ < result.count())))]
    #[cfg_attr(creusot, ensures(result.count() <= adjacency@.len()))]
    #[must_use]
    pub fn compute(adjacency: &[Vec<usize>], removed: &[bool]) -> Self {
        let mut walk = TarjanWalk::new(adjacency.len());
        #[cfg(creusot)]
        proof_assert! {
            discovered_prefix_of_undiscovered(walk.discovery@, adjacency@.len());
            walk.core() && walk.respects(removed@) && walk.frames_within(adjacency@, 0)
        };
        #[cfg_attr(creusot, invariant(walk.core()))]
        #[cfg_attr(creusot, invariant(walk.discovery@.len() == adjacency@.len()))]
        #[cfg_attr(creusot, invariant(walk.respects(removed@)))]
        #[cfg_attr(creusot, invariant(walk.frames@.len() == 0))]
        #[cfg_attr(creusot, invariant(forall<node> 0 <= node && node < produced.len() ==>
            removed@[node] || walk.discovery@[node]@ != none()))]
        for root in 0..adjacency.len() {
            if removed[root] || walk.discovery[root] != Self::NONE {
                continue;
            }
            walk.run(root, adjacency, removed);
        }
        StronglyConnectedComponents { component_of: walk.component_of, components: walk.components }
    }

    /// Component indices for each node. Removed nodes use [`usize::MAX`].
    #[cfg_attr(creusot, ensures(result@ == self.ids()))]
    #[must_use]
    pub fn component_ids(&self) -> &[usize] {
        &self.component_of
    }

    /// The members of each component, indexed as [`Self::component_ids`]
    /// numbers the components. The walk closes a component only after every
    /// component it has an edge to, so each comes after those. A
    /// component's members are in the order the walk popped them off its
    /// stack, the reverse of the order it discovered them in.
    #[must_use]
    pub fn components(&self) -> &[Vec<usize>] {
        &self.components
    }

    /// The number of components in the included subgraph.
    #[cfg_attr(creusot, ensures(result@ == self.count()))]
    #[must_use]
    pub fn component_count(&self) -> usize {
        self.components.len()
    }

    /// Whether a cycle through `node` can exist: it shares a non-trivial
    /// component with another node, or loops onto itself. Removals since
    /// [`Self::compute`] can make this a false positive (the search then
    /// comes back empty, exactly as it would have without the filter)
    /// but never a false negative, because removals only take cycles away.
    ///
    /// `node` must not have been removed when the components were computed.
    #[cfg_attr(creusot, requires(node@ < self.ids().len() && self.ids()[node@]@ != none()))]
    #[cfg_attr(creusot, requires(node@ < adjacency@.len()))]
    #[cfg_attr(creusot, ensures(result == (self.size(self.ids()[node@]@) >= 2
        || adjacency@[node@]@.contains(node))))]
    #[must_use]
    pub fn may_lie_on_cycle(&self, node: usize, adjacency: &[Vec<usize>]) -> bool {
        self.components[self.component_of[node]].len() >= 2 || adjacency[node].contains(&node)
    }

    /// Whether `left` and `right` share a component.
    #[cfg_attr(creusot, requires(left@ < self.ids().len() && right@ < self.ids().len()))]
    #[cfg_attr(creusot, ensures(result == (self.ids()[left@] == self.ids()[right@])))]
    #[must_use]
    pub fn shares_component(&self, left: usize, right: usize) -> bool {
        self.component_of[left] == self.component_of[right]
    }
}

/// One iterative Tarjan pass. Recursion would overflow on a workspace-deep
/// chain, so the DFS keeps its own frames.
struct TarjanWalk {
    discovery: Vec<usize>,
    low_link: Vec<usize>,
    on_stack: Vec<bool>,
    stack: Vec<usize>,
    component_of: Vec<usize>,
    components: Vec<Vec<usize>>,
    next_discovery: usize,
    /// Explicit DFS frames of (node, next edge position).
    frames: Vec<(usize, usize)>,
}

impl TarjanWalk {
    #[cfg_attr(creusot, ensures(result.node_count() == node_count@ && result.sized()))]
    #[cfg_attr(creusot, ensures(forall<node> 0 <= node && node < node_count@ ==>
        result.discovery@[node]@ == none()
        && !result.on_stack@[node]
        && result.component_of@[node]@ == none()))]
    #[cfg_attr(creusot, ensures(result.stack@.len() == 0 && result.frames@.len() == 0))]
    #[cfg_attr(creusot, ensures(result.components@.len() == 0 && result.next_discovery@ == 0))]
    fn new(node_count: usize) -> Self {
        TarjanWalk {
            discovery: vec![StronglyConnectedComponents::NONE; node_count],
            low_link: vec![0; node_count],
            on_stack: vec![false; node_count],
            stack: Vec::new(),
            component_of: vec![StronglyConnectedComponents::NONE; node_count],
            components: Vec::new(),
            next_discovery: 0,
            frames: Vec::new(),
        }
    }

    /// Walk everything reachable from `root` that is neither removed nor
    /// already discovered, assigning a component to each node it closes.
    #[cfg_attr(creusot, requires(well_formed(adjacency@, removed@)))]
    #[cfg_attr(creusot, requires(self.core() && self.node_count() == adjacency@.len()))]
    #[cfg_attr(creusot, requires(self.respects(removed@) && self.frames@.len() == 0))]
    #[cfg_attr(creusot, requires(root@ < adjacency@.len() && !removed@[root@]))]
    #[cfg_attr(creusot, requires(self.discovery@[root@]@ == none()))]
    #[cfg_attr(creusot, ensures((^self).core() && (^self).node_count() == adjacency@.len()))]
    #[cfg_attr(creusot, ensures((^self).respects(removed@) && (^self).frames@.len() == 0))]
    #[cfg_attr(creusot, ensures(self.discovered_within(^self)))]
    #[cfg_attr(creusot, ensures((^self).discovery@[root@]@ != none()))]
    fn run(&mut self, root: usize, adjacency: &[Vec<usize>], removed: &[bool]) {
        #[cfg(creusot)]
        let before = snapshot!(*self);
        self.discover(root);
        #[cfg_attr(creusot, invariant(self.core() && self.node_count() == adjacency@.len()))]
        #[cfg_attr(creusot, invariant(self.respects(removed@)))]
        #[cfg_attr(creusot, invariant(self.frames_within(adjacency@, 0)))]
        #[cfg_attr(creusot, invariant(before.discovered_within(*self)))]
        #[cfg_attr(creusot, invariant(self.discovery@[root@]@ != none()))]
        while !self.frames.is_empty() {
            let (node, edge_index) = {
                let frame = self.frames.last_mut().expect("the loop guard holds a frame");
                let step = (frame.0, frame.1);
                frame.1 += 1;
                step
            };
            match adjacency[node].get(edge_index) {
                Some(&to) => self.follow_edge(node, to, removed),
                None => self.finish_node(node),
            }
        }
    }

    /// Stamp `node` with the next discovery index and open a frame for it.
    #[cfg_attr(creusot, requires(self.core()))]
    #[cfg_attr(creusot, requires(node@ < self.node_count() && self.discovery@[node@]@ == none()))]
    #[cfg_attr(creusot, ensures((^self).core()))]
    #[cfg_attr(creusot, ensures((^self).discovery@ == self.discovery@.set(node@, self.next_discovery)))]
    #[cfg_attr(creusot, ensures((^self).frames@ == self.frames@.snoc((node, 0usize))))]
    fn discover(&mut self, node: usize) {
        #[cfg(creusot)]
        proof_assert! {
            discovered_prefix_missing(self.discovery@, node@, self.node_count());
            self.next_discovery@ < self.node_count()
        };
        #[cfg(creusot)]
        proof_assert! {
            discovered_prefix_discover(self.discovery@, node@, self.next_discovery, self.node_count());
            discovered_prefix_bounds(self.discovery@.set(node@, self.next_discovery), self.node_count());
            discovered_prefix(self.discovery@.set(node@, self.next_discovery), self.node_count())
                == self.next_discovery@ + 1
        };
        #[cfg(creusot)]
        let before = snapshot!(*self);
        self.discovery[node] = self.next_discovery;
        #[cfg(creusot)]
        proof_assert!(self.discovery@.ext_eq(before.discovery@.set(node@, before.next_discovery)));
        self.low_link[node] = self.next_discovery;
        self.next_discovery += 1;
        self.stack.push(node);
        #[cfg(creusot)]
        proof_assert!(self.stack@.ext_eq(before.stack@.snoc(node)));
        #[cfg(creusot)]
        proof_assert! {
            stacked_push(before.stack@, node);
            forall<other> 0 <= other && other < before.node_count() && before.on_stack@[other] ==>
                stacked(self.stack@, other)
        };
        self.on_stack[node] = true;
        #[cfg(creusot)]
        proof_assert!(self.on_stack@.ext_eq(before.on_stack@.set(node@, true)));
        self.frames.push((node, 0));
    }

    #[cfg_attr(creusot, requires(self.core() && self.respects(removed@)))]
    #[cfg_attr(creusot, requires(node@ < self.node_count() && self.on_stack@[node@]))]
    #[cfg_attr(creusot, requires(to@ < self.node_count()))]
    #[cfg_attr(creusot, ensures((^self).core() && (^self).respects(removed@)))]
    #[cfg_attr(creusot, ensures(self.discovered_within(^self)))]
    #[cfg_attr(creusot, ensures((^self).frames@ == self.frames@
        || (^self).frames@ == self.frames@.snoc((to, 0usize))))]
    fn follow_edge(&mut self, node: usize, to: usize, removed: &[bool]) {
        if removed[to] {
            return;
        }
        if self.discovery[to] == StronglyConnectedComponents::NONE {
            self.discover(to);
        } else if self.on_stack[to] {
            self.low_link[node] = self.low_link[node].min(self.discovery[to]);
        }
    }

    /// Close `node`'s frame, carry its low link up to its parent, and pop the
    /// component off the stack when `node` is a component root.
    #[cfg_attr(creusot, requires(self.core()))]
    #[cfg_attr(creusot, requires(self.frames@.len() > 0))]
    #[cfg_attr(creusot, requires(self.frames@[self.frames@.len() - 1].0 == node))]
    #[cfg_attr(creusot, ensures((^self).core()))]
    #[cfg_attr(creusot, ensures((^self).discovery@ == self.discovery@))]
    #[cfg_attr(creusot, ensures((^self).frames@ == self.frames@.subsequence(0, self.frames@.len() - 1)))]
    fn finish_node(&mut self, node: usize) {
        self.frames.pop();
        if let Some(&(parent, _)) = self.frames.last() {
            self.low_link[parent] = self.low_link[parent].min(self.low_link[node]);
        }
        if self.low_link[node] == self.discovery[node] {
            self.pop_component(node);
        }
    }

    /// Pop every stacked node discovered no earlier than `root` into one
    /// new component.
    #[cfg_attr(creusot, requires(self.core_unrooted()))]
    #[cfg_attr(creusot, requires(root@ < self.node_count() && self.on_stack@[root@]))]
    #[cfg_attr(creusot, requires(forall<depth> 0 <= depth && depth < self.frames@.len() ==>
        self.discovery@[self.frames@[depth].0@] < self.discovery@[root@]))]
    #[cfg_attr(creusot, ensures((^self).core_unrooted()))]
    #[cfg_attr(creusot, ensures(forall<node> 0 <= node && node < self.node_count() ==>
        (^self).on_stack@[node]
            == (self.on_stack@[node] && self.discovery@[node] < self.discovery@[root@])))]
    #[cfg_attr(creusot, ensures((^self).stack@ == self.stack@.subsequence(0, (^self).stack@.len())))]
    #[cfg_attr(creusot, ensures((^self).discovery@ == self.discovery@))]
    #[cfg_attr(creusot, ensures((^self).low_link@ == self.low_link@))]
    #[cfg_attr(creusot, ensures((^self).frames@ == self.frames@))]
    fn pop_component(&mut self, root: usize) {
        #[cfg(creusot)]
        let before = snapshot!(*self);
        let component = self.components.len();
        let mut members = Vec::new();
        #[cfg_attr(creusot, invariant(self.popping(*before, root@, component@, members@.len())))]
        loop {
            let member = self.stack.pop().expect("Tarjan stack holds the component");
            #[cfg(creusot)]
            proof_assert! {
                stacked_prefix_pop(before.stack@, self.stack@.len() + 1);
                self.stack@.ext_eq(before.stack@.subsequence(0, self.stack@.len()))
                    && forall<node> 0 <= node && node < before.node_count()
                        && self.on_stack@[node] && node != member@ ==> stacked(self.stack@, node)
            };
            self.on_stack[member] = false;
            self.component_of[member] = component;
            members.push(member);
            if member == root {
                break;
            }
        }
        self.components.push(members);
    }
}

#[cfg(test)]
mod tests;
#[cfg(creusot)]
#[expect(dead_code, reason = "a theorem is proved, never called")]
mod theorems;
