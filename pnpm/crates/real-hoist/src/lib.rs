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

pub use tree::{
    percent_encode_path,
    pkg_id,
};

use derive_more::{
    Display,
    Error,
};
use indexmap::{
    IndexMap,
    IndexSet,
};
use miette::Diagnostic;
use pnpm_lockfile::{
    Lockfile,
    PkgName,
    PkgNameVerPeer,
    ProjectSnapshot,
    SnapshotEntry,
    VersionPart,
};
use std::{
    cell::RefCell,
    collections::{
        BTreeMap,
        BTreeSet,
        HashMap,
    },
    rc::Rc,
};

/// One of the three node categories the `@yarnpkg/nm` hoister
/// distinguishes. Mirrors [`HoisterDependencyKind`][yarn-kind] at the
/// [yarn source][yarn-kind].
///
/// [yarn-kind]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L12-L14
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HoisterDependencyKind {
    /// A normal package, eligible for hoisting.
    Regular,
    /// A workspace project. The root `.` node is one of these; each
    /// non-root importer is added under it as another `Workspace`
    /// node. Workspace nodes never hoist past their declared slot.
    Workspace,
    /// A package linked from outside the lockfile graph (e.g. a
    /// `link:` ref). Only hoists when *all* of its descendants
    /// hoist, and triggers another round when any do; see
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
/// via a back-edge sees the eventually-populated set, matching JS's
/// `Set<HoisterTree>` mutation semantics.
///
/// [yarn-tree]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L16-L19
#[derive(Debug)]
pub struct HoisterTree {
    /// The alias the package is exposed under at *this* parent,
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
    /// Aliases that this node refuses to hoist past; its parent
    /// must keep them in scope. The union of `peerDependencies` and
    /// `transitivePeerDependencies` from the lockfile, unless
    /// `autoInstallPeers` is set (which zeroes the set so the
    /// hoister moves freely). In declaration order, which breaks ties
    /// when peer providers are ordered before their dependents.
    pub peer_names: IndexSet<String>,
    /// What the node deduplicates under, when its `reference` does not
    /// say: two nodes of one package version share a directory only when
    /// their identities match. `None` uses the reference with its peer
    /// suffix collapsed (see [`pkg_id`]), so peer variants share one.
    pub identity: Option<String>,
    pub dependency_kind: HoisterDependencyKind,
    /// Tiebreaker used upstream when ranking competing hoist
    /// candidates. Carried through the type for parity with
    /// `@yarnpkg/nm`'s `HoisterTree.hoistPriority`, but pacquet
    /// builds every node with `0`, so the preference pass ranks
    /// purely by usage count, the `hoistPriority` tier stays inert
    /// until a producer sets it.
    pub hoist_priority: u32,
    /// Children of this node. Order matches insertion order, the
    /// hoister depends on it.
    pub dependencies: RefCell<IndexSet<RcByPtr<HoisterTree>>>,
}

/// Output node from the hoister: one directory of the layout. Mirrors
/// [`HoisterResult`][yarn-result].
///
/// [yarn-result]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L20-L23
#[derive(Debug, Clone)]
pub struct HoisterResult {
    pub name: String,
    pub ident_name: String,
    /// The snapshot keys this directory serves. The first is the key of
    /// the copy that holds the directory; the rest are peer variants of
    /// the same package version that deduplicated into it, in the order
    /// they did.
    pub references: RefCell<IndexSet<String>>,
    /// The peer names of the input node, in declaration order.
    pub peer_names: IndexSet<String>,
    pub dependencies: RefCell<IndexSet<RcByPtr<HoisterResult>>>,
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
    /// hoist-decisions pass, without this every project hoists
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
            // `nodeLinker: hoisted` in a workspace, opting out is a
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
/// This mirrors JS `Set<HoisterTree>` semantics, JS Sets hash by
/// object identity, so adding the same node via two parent paths
/// keeps one entry. Cloning a [`RcByPtr`] only bumps the refcount, so
/// the dedup property survives parent-to-child propagation.
///
/// Without this wrapper, [`IndexSet<Rc<HoisterTree>>`] would hash on
/// the tree contents, recursive and expensive for deep graphs,
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
/// `hoistingLimits` enforced as `Border` decisions).
pub fn hoist(lockfile: &Lockfile, opts: &HoistOpts) -> Result<HoisterResult, HoistError> {
    // See `repair`: each pass hands the peers the layout failed to resolve
    // to the next as regular dependencies. The set only grows, so the loop
    // ends.
    let mut regular_peers = repair::RegularPeers::new();
    let (tree, result) = loop {
        let tree = build_tree(lockfile, opts, &regular_peers)?;
        let result = engine::hoist(&tree, opts);
        let misresolved = repair::misresolved_peers(&result, lockfile, opts);
        let mut added = false;
        for (key, peer) in misresolved {
            added |= regular_peers
                .entry(key)
                .or_default()
                .insert(peer);
        }
        if !added {
            break (tree, result);
        }
        tree::tear_down(&tree);
    };
    #[cfg(test)]
    tests::resolution::assert_resolves_as_declared(&tree, &result);
    tree::tear_down(&tree);

    // Strip `externalDependencies` from the top-level result;
    // they exist only to reserve a name slot at the root.
    if !opts.external_dependencies.is_empty() {
        result.dependencies
            .borrow_mut()
            .retain(|dep| !opts.external_dependencies.contains(&dep.name));
    }

    Ok(result)
}

/// The hoister's input for `lockfile`: the root project's dependencies
/// under the `.` root, then one workspace node per other project.
fn build_tree(
    lockfile: &Lockfile,
    opts: &HoistOpts,
    regular_peers: &repair::RegularPeers,
) -> Result<Rc<HoisterTree>, HoistError> {
    let mut cache = TreeCache { regular_peers: regular_peers.clone(), ..TreeCache::default() };

    let mut root_children: IndexSet<RcByPtr<HoisterTree>> = IndexSet::new();

    if let Some(root) = lockfile.importers.get(Lockfile::ROOT_IMPORTER_KEY) {
        collect_importer_deps(root, lockfile, opts, &mut cache, &mut root_children)?;
    }

    // `externalDependencies` are added as `link:` placeholders at
    // the root so the hoister won't move anything else into those
    // slots; they're stripped from the result after hoisting.
    for dep in &opts.external_dependencies {
        root_children.insert(RcByPtr(external_placeholder(dep)));
    }

    // Non-root importers (workspace projects) become children of
    // the virtual `.` root, unconditionally, matching pnpm v11's
    // `hoist()`, which attaches every importer regardless of any
    // knob. The hoister sees the whole workspace as one tree, which
    // is what enables cross-project dedupe of conflicting versions
    // and gives the layout `node_modules/<dep>` →
    // `<lockfile_dir>/<importer>/node_modules/<dep>` shape the
    // hoisted linker expects. In pnpm, `hoist-workspace-packages`
    // only controls whether the workspace *packages themselves* get
    // name-links in the root's hoisted modules dir (v11's
    // `hoistedWorkspacePackages` in the headless linker); it never
    // decides tree membership.
    for (importer_id, importer) in sorted_non_root_importers(lockfile) {
        let mut importer_children: IndexSet<RcByPtr<HoisterTree>> = IndexSet::new();
        collect_importer_deps(importer, lockfile, opts, &mut cache, &mut importer_children)?;
        root_children.insert(RcByPtr(importer_node(importer_id, importer_children)));
    }

    let root_node = Rc::new(HoisterTree {
        identity: None,
        name: ".".to_string(),
        ident_name: ".".to_string(),
        reference: String::new(),
        peer_names: IndexSet::new(),
        dependency_kind: HoisterDependencyKind::Workspace,
        hoist_priority: 0,
        dependencies: RefCell::new(root_children),
    });

    Ok(root_node)
}

/// Run the hoist on a prepared input tree under the resolution oracle,
/// for tests that build the tree by hand. See [`engine`].
#[cfg(test)]
fn nm_hoist(tree: &HoisterTree, opts: &HoistOpts) -> HoisterResult {
    let result = engine::hoist(tree, opts);
    #[cfg(test)]
    tests::resolution::assert_resolves_as_declared(tree, &result);
    result
}

#[cfg(test)]
mod tests;

mod engine;

mod repair;

mod tree;
use tree::{
    TreeCache,
    collect_importer_deps,
    external_placeholder,
    importer_node,
    sorted_non_root_importers,
};
