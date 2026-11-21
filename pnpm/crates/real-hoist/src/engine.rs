//! A port of the hoisting algorithm in upstream's
//! [`hoist.ts`][upstream], kept close to its structure so the two can be
//! compared function by function. Each item names the upstream function
//! it ports.
//!
//! Two departures, both forced by Rust:
//!
//! * Traversals that follow the whole graph (cloning, shrinking, the
//!   preference map, the used-dependency maps) run on explicit stacks in
//!   the same visit order, so a deep dependency chain cannot overflow the
//!   thread stack. The per-path recursion of `hoistTo` and
//!   `hoistNodeDependencies` stays recursive, as upstream's does.
//! * A node is identified by [`WorkNode::ident`], the package with its
//!   peer suffix collapsed (see [`ident_reference`]), and its own snapshot
//!   key travels in [`WorkNode::references`]. Upstream gets the same
//!   split from Yarn's virtual references, whose ident strips the
//!   virtual part.
//!
//! [upstream]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts

pub(crate) mod graph;
mod hoist_graph;
mod hoist_info;
mod hoist_to;
mod preference;

use crate::{
    HoistOpts,
    HoisterDependencyKind,
    HoisterResult,
    HoisterTree,
};
use indexmap::{
    IndexMap,
    IndexSet,
};
use std::{
    cell::{
        Cell,
        RefCell,
    },
    rc::Rc,
};

/// Children keyed by the name they are installed under, in insertion
/// order, as upstream's `Map<PackageName, HoisterWorkTree>`.
type Children = RefCell<IndexMap<String, Rc<WorkNode>>>;

/// Upstream's `HoisterWorkTree`, without the fields that only feed its
/// debug output (`reasons`, `hoistedFrom`, `hoistedTo`).
pub(crate) struct WorkNode {
    name: String,
    ident_name: String,
    /// Every snapshot key served by this directory. The first is the
    /// node's own; dedups append the keys of the copies they absorb.
    references: RefCell<IndexSet<String>>,
    /// `ident_name@<collapsed reference>`. Upstream's `ident`, and also
    /// its `locator`, which pnpm v11's wrapper makes equal by collapsing
    /// the reference itself.
    ident: Rc<str>,
    dependencies: Children,
    original_dependencies: Children,
    hoisted_dependencies: Children,
    peer_names: Rc<IndexSet<String>>,
    decoupled: Cell<bool>,
    is_hoist_border: Cell<bool>,
    hoist_priority: u32,
    dependency_kind: HoisterDependencyKind,
}

impl WorkNode {
    fn dependency(&self, name: &str) -> Option<Rc<WorkNode>> {
        self.dependencies
            .borrow()
            .get(name)
            .map(Rc::clone)
    }

    /// The current children, detached from the cell so the caller can
    /// mutate the graph while it iterates.
    fn dependency_list(&self) -> Vec<Rc<WorkNode>> {
        self.dependencies
            .borrow()
            .values()
            .map(Rc::clone)
            .collect()
    }

    /// Upstream's `getAliasedLocator`.
    fn aliased_locator(&self) -> String {
        format!("{}@{}", self.name, self.ident)
    }
}

/// The reference a node is identified by. A snapshot key loses its peer
/// suffix, so the peer variants of one package version share an ident
/// and can dedup into one directory, as pnpm v11's `depPathByPkgId`
/// collapse allows. An injected directory keeps its suffix (see
/// [`crate::pkg_id`]). References that are not snapshot keys (the root,
/// `workspace:` and `link:`) stay as they are.
pub(crate) fn ident_reference(reference: &str) -> String {
    reference
        .parse::<pnpm_lockfile::PkgNameVerPeer>()
        .map_or_else(|_| reference.to_string(), |key| crate::pkg_id(&key))
}

/// Per-run options. Upstream's `options` object minus the debug fields.
pub(crate) struct EngineOptions<'opts> {
    hoisting_limits: &'opts crate::HoistingLimits,
    fast_lookup_possible: Cell<bool>,
    /// Every work node the run creates. Copies made for one path often
    /// point at each other or at themselves, so reference counting alone
    /// never frees them; [`graph::tear_down`] empties them all.
    nodes: Arena,
}

/// See [`EngineOptions::nodes`].
#[derive(Default)]
pub(crate) struct Arena(RefCell<Vec<Rc<WorkNode>>>);

impl Arena {
    fn track(&self, node: &Rc<WorkNode>) {
        self.0
            .borrow_mut()
            .push(Rc::clone(node));
    }
}

/// Upstream's `hoist`: clone the input, run `hoistTo` rounds until one
/// changes nothing, and shrink the result.
pub(crate) fn hoist(tree: &HoisterTree, opts: &HoistOpts) -> HoisterResult {
    let options = EngineOptions {
        hoisting_limits: &opts.hoisting_limits,
        fast_lookup_possible: Cell::new(true),
        nodes: Arena::default(),
    };
    let root = graph::clone_tree(tree, &options);
    loop {
        let round = hoist_to::hoist_to(&root, &options);
        options.fast_lookup_possible.set(false);
        if !round.another_round_needed && !round.is_graph_changed {
            break;
        }
    }
    let result = graph::shrink_tree(&root);
    graph::tear_down(&options.nodes);
    result
}
