//! The lemmas the proofs of the walk's invariant call.

use super::logic::{
    discovered_prefix,
    none,
    stacked,
};
use creusot_std::prelude::{
    Int,
    Seq,
    ensures,
    logic,
    requires,
    variant,
};

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
