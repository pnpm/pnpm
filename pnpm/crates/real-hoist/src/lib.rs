// This crate is a Rust port of the hoisting algorithm in `@yarnpkg/nm`,
// which is distributed under the following license:
//
//     BSD 2-Clause License
//
//     Copyright (c) 2016-present, Yarn Contributors.
//     All rights reserved.
//
//     Redistribution and use in source and binary forms, with or without
//     modification, are permitted provided that the following conditions are met:
//
//     1. Redistributions of source code must retain the above copyright notice, this
//        list of conditions and the following disclaimer.
//
//     2. Redistributions in binary form must reproduce the above copyright notice,
//        this list of conditions and the following disclaimer in the documentation
//        and/or other materials provided with the distribution.
//
//     THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
//     AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
//     IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//     DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
//     FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
//     DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//     SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
//     CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
//     OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
//     OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

//! Real-directory hoister for the `nodeLinker: hoisted` install layout.
//!
//! Implements pnpm's hoisted layout as a thin wrapper around the
//! [`@yarnpkg/nm/hoist`][yarn-hoist] algorithm. The wrapper translates a
//! pnpm lockfile into a [`HoisterTree`] (rooted at `.` with one child
//! per workspace importer), runs the algorithm, and post-filters
//! `externalDependencies` out of the top-level result.
//!
//! [yarn-hoist]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts

pub use tree::{percent_encode_path, pkg_id};

use derive_more::{Display, Error};
use indexmap::{IndexMap, IndexSet};
use miette::Diagnostic;
use pnpm_lockfile::{
    Lockfile, PkgName, PkgNameVerPeer, ProjectSnapshot, SnapshotEntry, VersionPart,
};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    rc::Rc,
};

/// One of the three node categories the `@yarnpkg/nm` hoister
/// distinguishes. Mirrors [`HoisterDependencyKind`][yarn-kind] at the
/// [yarn source][yarn-kind].
///
/// [yarn-kind]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L12-L14
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HoisterDependencyKind {
    /// A normal package — eligible for hoisting.
    Regular,
    /// A workspace project. The root `.` node is one of these; each
    /// non-root importer is added under it as another `Workspace`
    /// node. Workspace nodes never hoist past their declared slot.
    Workspace,
    /// A package linked from outside the lockfile graph (e.g. a
    /// `link:` ref). Only hoists when *all* of its descendants
    /// hoist, and triggers another round when any do — see
    /// [`hoist.ts:416`][soft-link].
    ///
    /// [soft-link]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L416
    ExternalSoftLink,
}

/// Input node for the hoister. Built by [`hoist`] from the lockfile.
///
/// Mirrors [`HoisterTree`][yarn-tree] at the [yarn source][yarn-tree]. Children
/// are stored in an [`IndexSet`] so insertion order is preserved (the
/// upstream hoister's traversal relies on declaration order to break
/// ties between equivalent candidates), and so that a node added via
/// two parent paths is shared by `Rc` identity the way JS's
/// `Set<HoisterTree>` shares by object identity.
///
/// `dependencies` is behind a [`RefCell`] so the construction phase
/// can stash a placeholder `Rc<HoisterTree>` for cycle short-circuit,
/// recurse, then populate the children in place. The placeholder Rc
/// and the populated one are the same allocation, so a node visited
/// via a back-edge sees the eventually-populated set — matching JS's
/// `Set<HoisterTree>` mutation semantics. The same interior
/// mutability is what the hoister algorithm will use to move children
/// between parents when it lands.
///
/// [yarn-tree]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L16-L19
#[derive(Debug)]
pub struct HoisterTree {
    /// The alias the package is exposed under at *this* parent —
    /// what would appear as the directory name in `node_modules`.
    /// For npm-alias deps (`"foo": "npm:bar@^1"`), this is `foo`.
    pub name: String,
    /// The package's underlying identity, independent of the alias.
    /// For npm-aliases this is the target package name (`bar`); for
    /// non-aliased deps it equals `name`.
    pub ident_name: String,
    /// Version-with-peer ref. For the root and workspace nodes this
    /// is `""` or `"workspace:<id>"`; for regular nodes it's the
    /// snapshot key (`name@version(peer)`).
    pub reference: String,
    /// Aliases that this node refuses to hoist past — its parent
    /// must keep them in scope. The union of `peerDependencies` and
    /// `transitivePeerDependencies` from the lockfile, unless
    /// `autoInstallPeers` is set (which zeroes the set so the
    /// hoister moves freely).
    pub peer_names: BTreeSet<String>,
    pub dependency_kind: HoisterDependencyKind,
    /// Tiebreaker used upstream when ranking competing hoist
    /// candidates. Carried through the type for parity with
    /// `@yarnpkg/nm`'s `HoisterTree.hoistPriority`, but pacquet
    /// builds every node with `0`, so the preference pass ranks
    /// purely by usage count — the `hoistPriority` tier stays inert
    /// until a producer sets it.
    pub hoist_priority: u32,
    /// Children of this node. Order matches insertion order — the
    /// hoister depends on it.
    pub dependencies: RefCell<IndexSet<RcByPtr<HoisterTree>>>,
}

/// Output node from the hoister. The shape mirrors [`HoisterTree`]
/// except that one [`HoisterResult`] can collect multiple references
/// (when several [`HoisterTree`] nodes with the same `ident_name`
/// converged onto the same hoist slot).
///
/// Both `references` and `dependencies` use [`RefCell`] for the same
/// reason [`HoisterTree::dependencies`] does: nodes are shared by
/// `Rc` identity across the result graph, and the algorithm
/// accumulates references / reorders children in place rather than
/// rebuilding `Rc`s (which would break the shared-by-identity
/// invariant for any earlier clone).
///
/// Mirrors [`HoisterResult`][yarn-result] at the [yarn source][yarn-result].
///
/// Pacquet extends upstream's [`HoisterResult`][yarn-result] with a `peer_names`
/// field copied through from [`HoisterTree::peer_names`]. The hoist
/// algorithm reads it while deciding whether a candidate can hoist
/// past parents that supply the peer; upstream resolves the same
/// information against its `HoisterWorkTree` instead, but since
/// pacquet runs the algorithm directly on [`HoisterResult`] (no
/// intermediate work tree), the peer set has to ride along.
///
/// [yarn-result]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L20-L23
#[derive(Debug, Clone)]
pub struct HoisterResult {
    pub name: String,
    pub ident_name: String,
    pub references: RefCell<BTreeSet<String>>,
    /// Peer-dependency names the upstream [`HoisterTree`][ts-HoisterTree] node
    /// declared. Read by the hoist algorithm to refuse hoists that
    /// would shadow a peer the candidate's ancestors satisfy with a
    /// different ident.
    ///
    /// [ts-HoisterTree]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L16-L19
    pub peer_names: BTreeSet<String>,
    pub dependencies: RefCell<IndexSet<RcByPtr<HoisterResult>>>,
    /// Names of dependencies that the hoist pass removed from this
    /// node (hoisted to a root or dedup'd against a root's copy),
    /// mapped to the removed node. Requires at this position resolve
    /// them through an ancestor directory, so a later nested-root
    /// pass must not shadow these names with a different version
    /// (see [`get_used_dependencies`]). Ports upstream's
    /// `hoistedDependencies`.
    hoisted_dependencies: RefCell<HashMap<String, Rc<HoisterResult>>>,
    /// Whether this node is a single-parent copy that the hoist
    /// algorithm may mutate. The converter builds a DAG in which a
    /// package reachable through several parents is one shared node;
    /// hoisting decisions are per parent path, so before a shared
    /// node's children are touched the node is cloned for the path
    /// being worked on (see [`decouple_child`]). Ports the
    /// `decoupled` flag consumed by upstream's
    /// [`decoupleGraphNode`][decouple].
    ///
    /// [decouple]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L670
    decoupled: Cell<bool>,
}

/// Per-importer hoisting borders. Outer key is the importer locator
/// (e.g. `.@`); the inner set lists package aliases that may not be
/// hoisted past that importer.
///
/// Mirrors yarn's [`hoistingLimits` option][yarn-hoisting-limits]
/// (`Map<Locator, Set<PackageName>>`). Pacquet uses `BTreeMap` /
/// `BTreeSet` so the order is deterministic for snapshot tests.
///
/// [yarn-hoisting-limits]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L100
pub type HoistingLimits = BTreeMap<String, BTreeSet<String>>;

/// Options accepted by [`hoist`]. Mirrors the `opts` object of the
/// pnpm wrapper.
#[derive(Debug, Clone)]
pub struct HoistOpts {
    pub hoisting_limits: HoistingLimits,
    pub external_dependencies: BTreeSet<String>,
    /// When `true`, every package's `peer_names` is zeroed before
    /// the hoister runs, so the hoister moves peers freely. Set from
    /// pnpm's `autoInstallPeers` config.
    pub auto_install_peers: bool,
    /// When `true` (the default), every non-root workspace importer
    /// is added to the hoister tree as a `Workspace`-kind child of
    /// the virtual `.` root. This is the only way under hoisted
    /// for workspace projects to participate in the shared
    /// hoist-decisions pass — without this every project hoists
    /// independently and conflicting versions don't dedupe across
    /// the workspace. Pacquet's `Config::hoist_workspace_packages`
    /// (in `pnpm-config`) drives this for the install pipeline.
    pub hoist_workspace_packages: bool,
}

impl Default for HoistOpts {
    fn default() -> Self {
        Self {
            hoisting_limits: HoistingLimits::new(),
            external_dependencies: BTreeSet::new(),
            auto_install_peers: false,
            // Workspace-aware hoisting is the whole point of
            // `nodeLinker: hoisted` in a workspace — opting out is a
            // niche knob, never the expected starting point, so it
            // defaults on.
            hoist_workspace_packages: true,
        }
    }
}

/// Failure modes of [`hoist`].
///
/// Marked `#[non_exhaustive]` so adding variants in later work
/// isn't a breaking API change.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum HoistError {
    /// A snapshot referenced by an importer is missing from
    /// `lockfile.snapshots`.
    #[display("Broken lockfile: missing snapshot for {pkg_key}")]
    #[diagnostic(
        code(ERR_PNPM_LOCKFILE_MISSING_DEPENDENCY),
        url("https://pnpm.io/errors#err_pnpm_lockfile_missing_dependency")
    )]
    LockfileMissingDependency {
        /// The depPath (snapshot key) the lockfile failed to
        /// resolve.
        pkg_key: String,
    },
}

/// Identity-hashed wrapper around `Rc<T>`. Two [`RcByPtr`] values are
/// equal iff their underlying `Rc`s point at the same allocation;
/// hashing uses the pointer address, not `T`'s `Hash` impl.
///
/// This mirrors JS `Set<HoisterTree>` semantics — JS Sets hash by
/// object identity, so adding the same node via two parent paths
/// keeps one entry. Cloning a [`RcByPtr`] only bumps the refcount, so
/// the dedup property survives parent-to-child propagation.
///
/// Without this wrapper, [`IndexSet<Rc<HoisterTree>>`] would hash on
/// the tree contents — recursive and expensive for deep graphs,
/// and wrong when two structurally-identical nodes come from
/// different sources and should stay distinct.
#[derive(Debug)]
pub struct RcByPtr<Inner>(pub Rc<Inner>);

impl<Inner> Clone for RcByPtr<Inner> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl<Inner> PartialEq for RcByPtr<Inner> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl<Inner> Eq for RcByPtr<Inner> {}

impl<Inner> std::hash::Hash for RcByPtr<Inner> {
    fn hash<Hasher: std::hash::Hasher>(&self, state: &mut Hasher) {
        (Rc::as_ptr(&self.0) as usize).hash(state);
    }
}

impl<Inner> std::ops::Deref for RcByPtr<Inner> {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

impl<Inner> From<Rc<Inner>> for RcByPtr<Inner> {
    fn from(rc: Rc<Inner>) -> Self {
        Self(rc)
    }
}

/// Build the [`HoisterTree`] for `lockfile`'s root importer and
/// run the `@yarnpkg/nm` hoister over it.
///
/// The inner hoist is a recursive DFS with multi-round
/// convergence over the result graph (peer-aware, with
/// `hoistingLimits` enforced as `Border` decisions). Gaps that
/// remain — popularity-based ident preference, multi-importer
/// workspace trees, and `ExternalSoftLink` descendants — are
/// documented on the private `nm_hoist` driver.
pub fn hoist(lockfile: &Lockfile, opts: &HoistOpts) -> Result<HoisterResult, HoistError> {
    let mut cache = TreeCache::default();

    let mut root_children: IndexSet<RcByPtr<HoisterTree>> = IndexSet::new();

    if let Some(root) = lockfile.importers.get(Lockfile::ROOT_IMPORTER_KEY) {
        collect_importer_deps(root, lockfile, opts, &mut cache, &mut root_children)?;
    }

    // `externalDependencies` are added as `link:` placeholders at
    // the root so the hoister won't move anything else into those
    // slots; they're stripped from the result after hoisting.
    // Pacquet has no consumer for this yet, but the wrapper handles
    // it so the signature is complete.
    for dep in &opts.external_dependencies {
        root_children.insert(RcByPtr(external_placeholder(dep)));
    }

    // Non-root importers (workspace projects) become children of
    // the virtual `.` root — unconditionally, matching pnpm v11's
    // `hoist()`, which attaches every importer regardless of any
    // knob. The hoister sees the whole workspace as one tree, which
    // is what enables cross-project dedupe of conflicting versions
    // and gives the layout `node_modules/<dep>` →
    // `<lockfile_dir>/<importer>/node_modules/<dep>` shape the
    // hoisted linker expects. In pnpm, `hoist-workspace-packages`
    // only controls whether the workspace *packages themselves* get
    // name-links in the root's hoisted modules dir (v11's
    // `hoistedWorkspacePackages` in the headless linker) — it never
    // decides tree membership; gating membership on it silently
    // dropped every importer-only dependency from the install.
    for (importer_id, importer) in sorted_non_root_importers(lockfile) {
        let mut importer_children: IndexSet<RcByPtr<HoisterTree>> = IndexSet::new();
        collect_importer_deps(importer, lockfile, opts, &mut cache, &mut importer_children)?;
        root_children.insert(RcByPtr(importer_node(importer_id, importer_children)));
    }

    let root_node = Rc::new(HoisterTree {
        name: ".".to_string(),
        ident_name: ".".to_string(),
        reference: String::new(),
        peer_names: BTreeSet::new(),
        dependency_kind: HoisterDependencyKind::Workspace,
        hoist_priority: 0,
        dependencies: RefCell::new(root_children),
    });

    let result = nm_hoist(&root_node, opts);

    // Strip `externalDependencies` from the top-level result —
    // they exist only to reserve a name slot at the root.
    if !opts.external_dependencies.is_empty() {
        result
            .dependencies
            .borrow_mut()
            .retain(|dep| !opts.external_dependencies.contains(&dep.name));
    }

    Ok(result)
}

/// Pacquet's port of the `@yarnpkg/nm` hoist algorithm. Walks the
/// input tree, deep-copies it into a [`HoisterResult`] shape, then
/// pulls eligible descendants up to the root via a depth-first
/// recursion run to a fixed point (see [`hoist_into_root`]) with
/// parent-wins conflict resolution. Models the common case
/// of pnpm's `nodeLinker: hoisted` install — every transitive
/// dependency that doesn't collide with an already-hoisted name
/// surfaces at the root, just like a flat `node_modules`.
///
/// Among competing versions of one name, the most-used version
/// wins the root slot — ported from upstream's `buildPreferenceMap`
/// / `getHoistIdentMap` (see [`build_hoist_ident_map`]) and the
/// per-pass ident shift in [`hoist_into_root`].
///
/// What this does *not* model yet:
///
/// * `ExternalSoftLink` descendants — pacquet creates soft-links
///   only as zero-children placeholders, so upstream's
///   "only-hoist-when-all-descendants-hoist" rule has nothing to
///   delay today.
///
/// Matches the structural intent of upstream `hoistTo` at
/// [hoist.ts:329][upstream] for the subset above.
///
/// [upstream]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L329
fn nm_hoist(tree: &HoisterTree, opts: &HoistOpts) -> HoisterResult {
    let mut context = ConvertContext::default();
    let root = convert(tree, &mut context);
    // The `.` root has no parents, so it is decoupled by
    // construction — mutating its children can't leak anywhere.
    root.decoupled.set(true);
    let mut path_locators = HashSet::from([node_locator(&root)]);
    hoist_to(&root, opts, &mut path_locators, true);
    // Returning an owned `HoisterResult` (rather than
    // `Rc<HoisterResult>`) keeps the wrapper's post-hoist
    // `external_dependencies` filter from mutating the shared graph.
    // Cloning the outer struct duplicates only the top-level fields —
    // the subtree children remain shared via the cloned `RcByPtr`
    // values, so deep deps stay deduplicated.
    (*root).clone()
}

/// Hoist eligible descendants onto `root`, then recurse into every
/// child that stayed (a conflict-nested loser, a border, ...) using it
/// as the next hoist root, so its own subtree flattens onto it.
/// `path_locators` carries the locators of every root on the current
/// recursion path; a child whose locator is already on it is a cycle
/// through the roots and is not descended into. Mirrors upstream's
/// `hoistTo` recursion (its `rootNodePathLocators` guard included) at
/// [hoist.ts:309][hoist-to].
///
/// Children are decoupled before they become roots: a hoist root's
/// dependency set is mutated (descendants are inserted), which is
/// only safe on a single-parent copy. In practice
/// [`hoist_subtree`] has already decoupled every remaining child
/// while walking, so the call here is a cheap no-op safety net.
///
/// [hoist-to]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L309
fn hoist_to(
    root: &Rc<HoisterResult>,
    opts: &HoistOpts,
    path_locators: &mut HashSet<String>,
    is_top_root: bool,
) {
    // Names this root's subtree already resolves from ancestor
    // directories (their providers were hoisted away earlier). A
    // candidate must not take such a name slot with a different
    // version. The top `.` root has nothing above it, so its map is
    // empty — matching upstream, which passes an empty map when
    // `tree == rootNode`.
    let used = if is_top_root { HashMap::new() } else { get_used_dependencies(root) };
    hoist_into_root(root, &node_locator(root), opts, &used);

    let children: Vec<RcByPtr<HoisterResult>> =
        root.dependencies.borrow().iter().cloned().collect();
    for child in children {
        if root.peer_names.contains(&child.0.name) {
            continue;
        }
        let locator = node_locator(&child.0);
        if !path_locators.insert(locator.clone()) {
            continue;
        }
        let child = decouple_child(root, &child);
        hoist_to(&child.0, opts, path_locators, false);
        path_locators.remove(&locator);
    }
}

/// Collects the dependencies that `root`'s subtree resolves through
/// ancestor directories: every name recorded in a subtree node's
/// `hoisted_dependencies` (the provider was hoisted above this root
/// earlier, so requires at that position reach it via parent
/// lookup). Hoisting a *different* version of such a name onto
/// `root` would shadow those resolutions, so [`hoist_into_root`]
/// refuses candidates that collide with this map. Ports upstream's
/// [`getZeroRoundUsedDependencies`][zero-round] — the variant yarn
/// always uses, since its `hoist()` hardcodes
/// `fastLookupPossible: true`.
///
/// [zero-round]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L161
fn get_used_dependencies(root: &Rc<HoisterResult>) -> HashMap<String, Rc<HoisterResult>> {
    let mut used: HashMap<String, Rc<HoisterResult>> = HashMap::new();
    let mut seen: HashSet<*const HoisterResult> = HashSet::new();
    let mut pending: Vec<Rc<HoisterResult>> = vec![Rc::clone(root)];
    while let Some(node) = pending.pop() {
        if !seen.insert(Rc::as_ptr(&node)) {
            continue;
        }
        for (name, dep) in node.hoisted_dependencies.borrow().iter() {
            used.insert(name.clone(), Rc::clone(dep));
        }
        for dep in node.dependencies.borrow().iter() {
            if !node.peer_names.contains(&dep.0.name) {
                pending.push(Rc::clone(&dep.0));
            }
        }
    }
    used
}

/// The node's locator: `ident@reference`, unique per package
/// identity and stable across decoupled copies. Matches the format
/// of the [`HoistingLimits`] keys and upstream's `node.locator`.
fn node_locator(node: &HoisterResult) -> String {
    format!("{}@{}", node.ident_name, node_ident(node))
}

/// Whether two nodes are the same package *version*, ignoring the
/// peer-resolution suffix — upstream's `ident` equality (its
/// `makeIdent` strips the virtual segment the way this drops the
/// `(...)` suffix). Peer-suffix variants of one version compare
/// equal: they merge in a hoisted layout, so they never shadow each
/// other.
fn same_ident(left: &HoisterResult, right: &HoisterResult) -> bool {
    fn ident_of(node: &HoisterResult) -> String {
        let references = node.references.borrow();
        let reference = references.iter().next().map_or("", String::as_str);
        match reference.find('(') {
            Some(idx) => reference[..idx].to_string(),
            None => reference.to_string(),
        }
    }
    left.ident_name == right.ident_name && ident_of(left) == ident_of(right)
}

/// Whether an ancestor strictly below the root carries a different
/// package version under `candidate`'s name (see
/// [`AbsorbDecision::PathShadow`](crate::absorption::AbsorbDecision::PathShadow)). `path[0]` is the hoist root —
/// its slot is judged by the root-index decision, not here.
fn path_shadowed(candidate: &HoisterResult, path: &[Rc<HoisterResult>]) -> bool {
    path.iter().skip(1).any(|ancestor| {
        ancestor
            .dependencies
            .borrow()
            .iter()
            .any(|dep| dep.0.name == candidate.name && !same_ident(&dep.0, candidate))
    })
}

/// Whether two nodes are the same package — equal locators — without
/// building the locator strings. Decoupled copies of one package
/// compare equal; different versions under one name do not.
fn same_locator(left: &HoisterResult, right: &HoisterResult) -> bool {
    left.ident_name == right.ident_name
        && left.references.borrow().iter().next() == right.references.borrow().iter().next()
}

/// Return a single-parent copy of `child` that is safe to mutate on
/// the current path, replacing `parent`'s edge with it (in place, so
/// sibling order is preserved). A node already decoupled has exactly
/// one parent and is returned as is. Ports upstream's
/// [`decoupleGraphNode`][decouple]: the clone shares the grandchild
/// `Rc`s — those are decoupled in turn if and when the walk reaches
/// them — so only the mutated spine of the graph is ever copied.
///
/// `parent` must itself be decoupled: replacing the edge mutates its
/// dependency set.
///
/// [decouple]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L670
fn decouple_child(
    parent: &Rc<HoisterResult>,
    child: &RcByPtr<HoisterResult>,
) -> RcByPtr<HoisterResult> {
    if child.0.decoupled.get() {
        return child.clone();
    }
    let clone = RcByPtr(Rc::new(HoisterResult {
        name: child.0.name.clone(),
        ident_name: child.0.ident_name.clone(),
        references: RefCell::new(child.0.references.borrow().clone()),
        peer_names: child.0.peer_names.clone(),
        dependencies: RefCell::new(child.0.dependencies.borrow().clone()),
        hoisted_dependencies: RefCell::new(child.0.hoisted_dependencies.borrow().clone()),
        decoupled: Cell::new(true),
    }));
    let mut deps = parent.dependencies.borrow_mut();
    let index = deps.get_index_of(child).expect("decoupled edge exists in its parent");
    deps.shift_remove_index(index);
    deps.shift_insert(index, clone.clone());
    clone
}

/// Immutable context shared across every [`hoist_subtree`] call in
/// one [`hoist_into_root`] pass: the hoisting root, the active
/// border-name set, and the per-name preferred-ident map. Bundled
/// into one struct so the recursive walker stays under the argument
/// limit; only `root_index` and the per-node position
/// (`node`, `ancestor_path`, `under_border`) vary per call.
struct HoistCtx<'a> {
    root: &'a Rc<HoisterResult>,
    border_names: &'a BTreeSet<String>,
    hoist_ident_map: &'a HashMap<String, VecDeque<String>>,
    /// See [`get_used_dependencies`]; empty for the top `.` root.
    used: &'a HashMap<String, Rc<HoisterResult>>,
}

/// Walk the result tree and hoist every eligible descendant of
/// `root` onto `root` itself, iterating until the graph reaches a
/// fixed point.
///
/// Maintains a side `HashMap<name, RcByPtr>` mirror of root's
/// direct deps so the per-edge "is this name taken at root?" check
/// stays O(1). Without the index a graph with `N` packages all
/// hoisting freely would do O(N²) `IndexSet` scans.
///
/// Each round is a recursive depth-first walk (see
/// [`hoist_subtree`]) whose `ancestor_path` reflects the current
/// result-graph position of each node — a freshly-hoisted child
/// recurses with `[root]`, a child that stayed nested recurses
/// with `parent_path + [parent]`. The outer loop re-runs the DFS
/// whenever a round made at least one move, because that move can
/// unlock further hoists: a previously-blocking peer ident may
/// have shifted out of the ancestor chain (a sibling's dep moved
/// to root), or a previously-empty root slot may now carry a
/// compatible ident.
///
/// Termination is bounded by O(N) rounds since each move is
/// one-way (parent → root) and the graph has finite size.
/// Mirrors upstream `hoistTo`'s
/// `do { hoistGraph(); } while (anotherRoundNeeded)` shape, just
/// with the DFS-by-round simplification described above.
///
/// The converter's result is a DAG (one shared node per package),
/// but the walk mutates only decoupled — single-parent — copies:
/// every edge the DFS crosses is decoupled first (see
/// [`decouple_child`]), so a package reachable through several
/// parents gets an independent hoist decision per path, exactly like
/// upstream's per-path work tree.
fn hoist_into_root(
    root: &Rc<HoisterResult>,
    root_locator: &str,
    opts: &HoistOpts,
    used: &HashMap<String, Rc<HoisterResult>>,
) {
    let mut root_index: HashMap<String, RcByPtr<HoisterResult>> =
        root.dependencies.borrow().iter().map(|dep| (dep.0.name.clone(), dep.clone())).collect();

    // Per-name candidate idents ordered most-preferred first. Only
    // the front ident of each name may claim the root slot; the
    // shift below promotes the next candidate when the preferred one
    // can't be placed. Built from the pre-hoist subtree, matching
    // yarn's `buildPreferenceMap(rootNode)` call at the top of
    // `hoistTo`.
    let mut hoist_ident_map = build_hoist_ident_map(root);

    // Look up the border names for *this* root locator: a node whose
    // name is in this set is a hoisting border, so its descendants
    // stay nested beneath it. Upstream stores the flag on each node
    // as `isHoistBorder` during `cloneTree`; pacquet stays DAG-shaped
    // and looks the names up by-name at decision time, which is
    // equivalent since there's only one root locator. An empty
    // fallback set means the check is a no-op when no limits are set.
    let empty_set: BTreeSet<String> = BTreeSet::new();
    let border_names: &BTreeSet<String> =
        opts.hoisting_limits.get(root_locator).unwrap_or(&empty_set);

    loop {
        let ctx = HoistCtx { root, border_names, hoist_ident_map: &hoist_ident_map, used };
        let changed = hoist_subtree(root, &[], &ctx, &mut root_index, false);

        // Per-pass ident shift: a name with more than one candidate
        // ident whose preferred ident still hasn't reached the root
        // drops that ident and promotes the next one, so a later pass
        // can place a less-preferred version when the most-preferred
        // is unreachable (nested under a conflict / border / peer).
        // Mirrors the `idents.shift()` loop in yarn's `hoistTo` at
        // <https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts>.
        let mut shifted = false;
        for (name, idents) in &mut hoist_ident_map {
            if idents.len() > 1 && !root_index.contains_key(name) {
                idents.pop_front();
                shifted = true;
            }
        }

        if !changed && !shifted {
            break;
        }
    }
}

/// The single canonical reference of a (pre-hoist) result node,
/// used as its "ident" in the preference map and the per-name
/// candidate lists. Pre-hoist nodes carry exactly one reference
/// (see [`convert`]).
fn node_ident(node: &HoisterResult) -> String {
    node.references.borrow().iter().next().cloned().unwrap_or_default()
}

/// Depth-first hoist driver. `ancestor_path` is the path from
/// `root` down to (but *excluding*) `node`, so for the root
/// itself it is empty and for a child of root it is `[root]`.
/// Returns whether this subtree moved at least one node in the
/// current round — the outer multi-round loop uses that to
/// decide whether another round can unlock further hoists.
///
/// `node` must be decoupled — the walk mutates its dependency set.
/// The recursion keeps that invariant: every child is decoupled
/// (relative to its post-decision parent) before it is descended
/// into, so a package shared by several parents is walked — and
/// decided — once per path, on that path's own copy. The walk tree
/// therefore has the shape of the final materialized layout, and
/// termination follows from the cycle cut below: no locator repeats
/// on a path, so paths (and the walk) are finite.
fn hoist_subtree(
    node: &Rc<HoisterResult>,
    ancestor_path: &[Rc<HoisterResult>],
    ctx: &HoistCtx<'_>,
    root_index: &mut HashMap<String, RcByPtr<HoisterResult>>,
    under_border: bool,
) -> bool {
    let mut changed_in_subtree = false;

    // A node whose name is in `border_names` is a hoisting border:
    // its descendants are kept nested beneath it rather than hoisted
    // to the root. `under_border` carries that boundary down the
    // recursion — once any proper ancestor of a node is a border,
    // the node (and everything below it) stays put. Mirrors
    // upstream's `isHoistBorder` flag, which blocks a bordered
    // node's *children* from hoisting past it, not the bordered
    // node itself.
    let children_blocked = under_border || ctx.border_names.contains(&node.name);

    // Snapshot the current children so we can mutate
    // `node.dependencies` mid-iteration without invalidating the
    // borrow. `RcByPtr::clone` just bumps refcounts.
    let children: Vec<RcByPtr<HoisterResult>> =
        node.dependencies.borrow().iter().cloned().collect();

    // Path from root down to and including `node` — i.e. the
    // ancestor path for `node`'s direct children. Used for the
    // peer-shadow checks, the cycle cut, and as the starting point
    // for the path passed into recursion when a child stays
    // nested.
    let mut path_for_children: Vec<Rc<HoisterResult>> = ancestor_path.to_vec();
    path_for_children.push(Rc::clone(node));

    for child in children {
        if is_cycle_edge(&child.0, &path_for_children) {
            node.dependencies.borrow_mut().shift_remove(&child);
            changed_in_subtree = true;
            continue;
        }

        let decision =
            absorb_decision(&child.0, &path_for_children, ctx, root_index, children_blocked);
        let child_recursion_path =
            match apply_decision(decision, &child, node, ctx, &path_for_children, root_index) {
                ChildStep::Dropped => {
                    changed_in_subtree = true;
                    continue;
                }
                ChildStep::Descend { path, moved } => {
                    changed_in_subtree |= moved;
                    path
                }
            };

        // Decouple before descending: the recursion mutates the
        // child's dependency set, which must not leak into other
        // paths that share the child. The child's current parent is
        // the last element of its recursion path — root for a
        // just-moved (or root-direct) child, `node` otherwise.
        let parent =
            child_recursion_path.last().expect("the recursion path ends at the child's parent");
        let child = decouple_child(parent, &child);
        if Rc::ptr_eq(parent, ctx.root) {
            root_index.insert(child.0.name.clone(), child.clone());
        }

        let child_changed =
            hoist_subtree(&child.0, &child_recursion_path, ctx, root_index, children_blocked);
        changed_in_subtree |= child_changed;
    }
    changed_in_subtree
}

/// Whether the edge to `child` closes a cycle (or is a self-reference): a
/// package with this alias *and* locator already materializes as an ancestor
/// of this position, so requiring the alias here resolves to that ancestor.
/// The edge carries no additional layout and would send the walkers into
/// unbounded recursion, so the caller cuts it.
///
/// The alias name must match too: an edge exposing the same package under a
/// *different* alias is the only `node_modules/<alias>` entry for that name
/// and stays, just like upstream, whose `aliasedLocatorPath` guard compares
/// `name@locator`. Upstream merely skips descending into cycle edges; pacquet
/// removes them outright because its layout walkers require the result to be
/// a DAG. The parent is decoupled, so the cut is per-path.
fn is_cycle_edge(child: &Rc<HoisterResult>, path: &[Rc<HoisterResult>]) -> bool {
    path.iter().any(|ancestor| ancestor.name == child.name && same_locator(ancestor, child))
}

#[cfg(test)]
mod tests;

mod tree;
use tree::{
    ConvertContext, TreeCache, collect_importer_deps, convert, external_placeholder, importer_node,
    sorted_non_root_importers,
};

mod preferences;
use preferences::{build_hoist_ident_map, is_preferred_ident};

mod absorption;
use absorption::{ChildStep, absorb_decision, apply_decision};
