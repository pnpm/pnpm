//! Real-directory hoister for the `nodeLinker: hoisted` install layout.
//!
//! Ports pnpm v11's [`installing/linking/real-hoist`][upstream-wrapper]
//! package, which is itself a thin wrapper around the
//! [`@yarnpkg/nm/hoist`][yarn-hoist] algorithm. The wrapper translates a
//! pnpm lockfile into a [`HoisterTree`] (rooted at `.` with one child
//! per workspace importer), runs the algorithm, and post-filters
//! `externalDependencies` out of the top-level result.
//!
//! [upstream-wrapper]: https://github.com/pnpm/pnpm/blob/94240bc0464196bd52f7006b97f6d9a43df34633/installing/linking/real-hoist/src/index.ts
//! [yarn-hoist]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts

use derive_more::{Display, Error};
use indexmap::IndexSet;
use miette::Diagnostic;
use pacquet_lockfile::{Lockfile, PkgName, PkgNameVerPeer, ProjectSnapshot, SnapshotEntry};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fmt::Write as _,
    rc::Rc,
};

/// One of the three node categories the `@yarnpkg/nm` hoister
/// distinguishes. Mirrors `HoisterDependencyKind` at the
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
/// Mirrors `HoisterTree` at the [yarn source][yarn-tree]. Children
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
    /// `@yarnpkg/nm`'s `HoisterTree.hoistPriority` but currently
    /// unread by pacquet's algorithm — pacquet builds every node
    /// with `0` and the popularity-based preference pass that
    /// would consume it isn't ported yet (see the "popularity-
    /// based ident preference" gap on `nm_hoist`).
    pub hoist_priority: u32,
    /// Children of this node. Order matches insertion order — the
    /// hoister depends on it.
    pub dependencies: RefCell<IndexSet<RcByPtr<HoisterTree>>>,
}

/// Output node from the hoister. The shape mirrors `HoisterTree`
/// except that one `HoisterResult` can collect multiple references
/// (when several `HoisterTree` nodes with the same `ident_name`
/// converged onto the same hoist slot).
///
/// Both `references` and `dependencies` use [`RefCell`] for the same
/// reason [`HoisterTree::dependencies`] does: nodes are shared by
/// `Rc` identity across the result graph, and the algorithm
/// accumulates references / reorders children in place rather than
/// rebuilding `Rc`s (which would break the shared-by-identity
/// invariant for any earlier clone).
///
/// Mirrors `HoisterResult` at the [yarn source][yarn-result].
///
/// Pacquet extends upstream's `HoisterResult` with a `peer_names`
/// field copied through from [`HoisterTree::peer_names`]. The hoist
/// algorithm reads it while deciding whether a candidate can hoist
/// past parents that supply the peer; upstream resolves the same
/// information against its `HoisterWorkTree` instead, but since
/// pacquet runs the algorithm directly on `HoisterResult` (no
/// intermediate work tree), the peer set has to ride along.
///
/// [yarn-result]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L20-L23
#[derive(Debug, Clone)]
pub struct HoisterResult {
    pub name: String,
    pub ident_name: String,
    pub references: RefCell<BTreeSet<String>>,
    /// Peer-dependency names the upstream `HoisterTree` node
    /// declared. Read by the hoist algorithm to refuse hoists that
    /// would shadow a peer the candidate's ancestors satisfy with a
    /// different ident.
    pub peer_names: BTreeSet<String>,
    pub dependencies: RefCell<IndexSet<RcByPtr<HoisterResult>>>,
}

/// Per-importer hoisting borders. Outer key is the importer locator
/// (e.g. `.@`); the inner set lists package aliases that may not be
/// hoisted past that importer.
///
/// Upstream `HoistingLimits` is `Map<string, Set<string>>`. Pacquet
/// uses `BTreeMap` / `BTreeSet` so the order is deterministic for
/// snapshot tests.
pub type HoistingLimits = BTreeMap<String, BTreeSet<String>>;

/// Options accepted by [`hoist`]. Mirrors the `opts` object of the
/// pnpm wrapper.
#[derive(Debug, Clone)]
pub struct HoistOpts {
    pub hoisting_limits: HoistingLimits,
    pub external_dependencies: BTreeSet<String>,
    /// When `true`, every package's `peer_names` is zeroed before
    /// the hoister runs. Mirrors pnpm's `autoInstallPeers` short-
    /// circuit at [real-hoist:124][auto].
    ///
    /// [auto]: https://github.com/pnpm/pnpm/blob/94240bc0464196bd52f7006b97f6d9a43df34633/installing/linking/real-hoist/src/index.ts#L124-L129
    pub auto_install_peers: bool,
    /// When `true` (the default), every non-root workspace importer
    /// is added to the hoister tree as a `Workspace`-kind child of
    /// the virtual `.` root. This is the only way under hoisted
    /// for workspace projects to participate in the shared
    /// hoist-decisions pass — without this every project hoists
    /// independently and conflicting versions don't dedupe across
    /// the workspace. Mirrors pnpm's `hoistWorkspacePackages` at
    /// [`installing/linking/real-hoist/src/index.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/linking/real-hoist/src/index.ts#L51-L66).
    /// Pacquet's `Config::hoist_workspace_packages` (in
    /// `pacquet-config`) drives this for the install pipeline.
    pub hoist_workspace_packages: bool,
}

impl Default for HoistOpts {
    fn default() -> Self {
        Self {
            hoisting_limits: HoistingLimits::new(),
            external_dependencies: BTreeSet::new(),
            auto_install_peers: false,
            // Match upstream's default-on behavior. Workspace-aware
            // hoisting is the whole point of `nodeLinker: hoisted` in
            // a workspace — opting out is a niche knob, never the
            // expected starting point.
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
    /// `lockfile.snapshots`. Mirrors pnpm's
    /// `LockfileMissingDependencyError` raised at
    /// [real-hoist:111][missing-dep].
    ///
    /// [missing-dep]: https://github.com/pnpm/pnpm/blob/94240bc0464196bd52f7006b97f6d9a43df34633/installing/linking/real-hoist/src/index.ts#L109-L111
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

/// Identity-hashed wrapper around `Rc<T>`. Two `RcByPtr` values are
/// equal iff their underlying `Rc`s point at the same allocation;
/// hashing uses the pointer address, not `T`'s `Hash` impl.
///
/// This mirrors JS `Set<HoisterTree>` semantics — JS Sets hash by
/// object identity, so adding the same node via two parent paths
/// keeps one entry. Cloning a `RcByPtr` only bumps the refcount, so
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
/// run the `@yarnpkg/nm` hoister over it. Ports
/// [`installing/linking/real-hoist/src/index.ts`][upstream].
///
/// The inner hoist is a recursive DFS with multi-round
/// convergence over the result graph (peer-aware, with
/// `hoistingLimits` enforced as `Border` decisions). Gaps that
/// remain — popularity-based ident preference, multi-importer
/// workspace trees, and `ExternalSoftLink` descendants — are
/// documented on the private `nm_hoist` driver.
///
/// [upstream]: https://github.com/pnpm/pnpm/blob/94240bc0464196bd52f7006b97f6d9a43df34633/installing/linking/real-hoist/src/index.ts
pub fn hoist(lockfile: &Lockfile, opts: &HoistOpts) -> Result<HoisterResult, HoistError> {
    let mut nodes: HashMap<String, Rc<HoisterTree>> = HashMap::new();

    let mut root_children: IndexSet<RcByPtr<HoisterTree>> = IndexSet::new();

    if let Some(root) = lockfile.importers.get(Lockfile::ROOT_IMPORTER_KEY) {
        collect_importer_deps(root, lockfile, opts, &mut nodes, &mut root_children)?;
    }

    // `externalDependencies` are added as `link:` placeholders at
    // the root so the hoister won't move anything else into those
    // slots; they're stripped from the result after hoisting.
    // Pacquet has no consumer for this yet, but the wrapper handles
    // it for parity with upstream's signature.
    for dep in &opts.external_dependencies {
        let placeholder = Rc::new(HoisterTree {
            name: dep.clone(),
            ident_name: dep.clone(),
            reference: "link:".to_string(),
            peer_names: BTreeSet::new(),
            dependency_kind: HoisterDependencyKind::ExternalSoftLink,
            hoist_priority: 0,
            dependencies: RefCell::new(IndexSet::new()),
        });
        root_children.insert(RcByPtr(placeholder));
    }

    // Non-root importers (workspace projects) become children of
    // the virtual `.` root when `hoist_workspace_packages` is on
    // (the default). The hoister sees the whole workspace as one
    // tree, which is what enables cross-project dedupe of
    // conflicting versions and gives the layout `node_modules/<dep>`
    // → `<lockfile_dir>/<importer>/node_modules/<dep>` shape that
    // upstream's hoisted linker expects.
    //
    // When the knob is `false`, non-root importers don't enter the
    // shared tree — each project hoists independently in the walk
    // phase. Mirrors upstream's `hoistWorkspacePackages: false`
    // path at
    // <https://github.com/pnpm/pnpm/blob/94240bc046/installing/linking/real-hoist/src/index.ts#L51-L66>.
    if opts.hoist_workspace_packages {
        let mut non_root: Vec<(&String, &ProjectSnapshot)> = lockfile
            .importers
            .iter()
            .filter(|(id, _)| id.as_str() != Lockfile::ROOT_IMPORTER_KEY)
            .collect();
        // HashMap iteration order is non-deterministic; sort so the
        // output tree is stable across runs (matters for snapshot
        // tests).
        non_root.sort_by(|a, b| a.0.cmp(b.0));

        for (importer_id, importer) in non_root {
            let mut importer_children: IndexSet<RcByPtr<HoisterTree>> = IndexSet::new();
            collect_importer_deps(importer, lockfile, opts, &mut nodes, &mut importer_children)?;
            let importer_node = Rc::new(HoisterTree {
                name: percent_encode_path(importer_id),
                ident_name: percent_encode_path(importer_id),
                reference: format!("workspace:{importer_id}"),
                peer_names: BTreeSet::new(),
                dependency_kind: HoisterDependencyKind::Workspace,
                hoist_priority: 0,
                dependencies: RefCell::new(importer_children),
            });
            root_children.insert(RcByPtr(importer_node));
        }
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

fn collect_importer_deps(
    importer: &ProjectSnapshot,
    lockfile: &Lockfile,
    opts: &HoistOpts,
    nodes: &mut HashMap<String, Rc<HoisterTree>>,
    out: &mut IndexSet<RcByPtr<HoisterTree>>,
) -> Result<(), HoistError> {
    // Upstream merges `dependencies + devDependencies +
    // optionalDependencies` into one alias-keyed object. Later
    // entries (in declaration order) win on duplicate aliases —
    // which is the same as inserting in that order and keeping the
    // last write. Pacquet's `ResolvedDependencyMap` is a HashMap so
    // declaration order is lost; merge into a `HashMap` (last write
    // wins) and emit in alias-sorted order so the build is
    // deterministic regardless of map seed.
    let mut merged: HashMap<&PkgName, &pacquet_lockfile::ResolvedDependencySpec> = HashMap::new();
    for deps in
        [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies]
            .into_iter()
            .flatten()
    {
        for (alias, spec) in deps {
            merged.insert(alias, spec);
        }
    }
    let mut entries: Vec<_> = merged.into_iter().collect();
    entries.sort_by_key(|(alias, _)| alias.to_string());
    for (alias, spec) in entries {
        // For an aliased importer dep (`ImporterDepVersion::Alias`),
        // the snapshot key is the alias's own (name, suffix);
        // [`ImporterDepVersion::resolved_key`] returns that.
        // Transitive npm-aliases (modelled via `SnapshotDepRef::Alias`)
        // are handled in `collect_snapshot_deps`.
        //
        // `link:` deps (cross-importer `workspace:*` resolutions, see
        // [`ImporterDepVersion::Link`]) don't live in the virtual
        // store — they're directory symlinks materialised by
        // [`pacquet_package_manager::SymlinkDirectDependencies`] —
        // so they have no snapshot to hoist and we skip them here.
        let Some(dep_key) = spec.version.resolved_key(alias) else {
            continue;
        };
        let node = build_dep_node(alias, &dep_key, lockfile, opts, nodes)?;
        out.insert(RcByPtr(node));
    }
    Ok(())
}

fn build_dep_node(
    alias: &PkgName,
    dep_key: &PkgNameVerPeer,
    lockfile: &Lockfile,
    opts: &HoistOpts,
    nodes: &mut HashMap<String, Rc<HoisterTree>>,
) -> Result<Rc<HoisterTree>, HoistError> {
    // Cache key is `<alias>:<dep_key>` to match upstream — two
    // different aliases pointing at the same package are
    // intentionally different nodes (the node's `name` field
    // differs), so they shouldn't share a cache slot.
    let cache_key = format!("{alias}:{dep_key}");
    if let Some(existing) = nodes.get(&cache_key) {
        return Ok(Rc::clone(existing));
    }

    let snapshots = lockfile
        .snapshots
        .as_ref()
        .ok_or_else(|| HoistError::LockfileMissingDependency { pkg_key: dep_key.to_string() })?;
    let snapshot = snapshots
        .get(dep_key)
        .ok_or_else(|| HoistError::LockfileMissingDependency { pkg_key: dep_key.to_string() })?;

    // Peer-name set: peerDependencies (from the `packages:` map)
    // plus transitivePeerDependencies (from the `snapshots:` map).
    // Mirrors upstream's
    // `[...Object.keys(pkgSnapshot.peerDependencies), ...transitivePeerDependencies]`.
    // Zeroed when `auto_install_peers` is on, so the hoister moves
    // freely.
    let mut peer_names: BTreeSet<String> = BTreeSet::new();
    if !opts.auto_install_peers {
        if let Some(packages) = lockfile.packages.as_ref() {
            let packages_key = dep_key.without_peer();
            if let Some(meta) = packages.get(&packages_key)
                && let Some(peer_deps) = meta.peer_dependencies.as_ref()
            {
                for name in peer_deps.keys() {
                    peer_names.insert(name.clone());
                }
            }
        }
        if let Some(transitive) = snapshot.transitive_peer_dependencies.as_ref() {
            for name in transitive {
                peer_names.insert(name.clone());
            }
        }
    }

    // Construct the node with an empty `dependencies` cell, stash
    // it in the cache, then recurse and populate the cell in place.
    // A back-edge that hits the same `cache_key` during the
    // recursion gets the same `Rc<HoisterTree>` — by the time the
    // outer call returns the cell holds the populated set, and the
    // shared-by-identity invariant the hoister algorithm relies on
    // survives. Mirrors the in-place mutation of `node.dependencies`
    // at upstream's real-hoist:132.
    let node = Rc::new(HoisterTree {
        name: alias.to_string(),
        ident_name: dep_key.name.to_string(),
        reference: dep_key.to_string(),
        peer_names,
        dependency_kind: HoisterDependencyKind::Regular,
        hoist_priority: 0,
        dependencies: RefCell::new(IndexSet::new()),
    });
    nodes.insert(cache_key, Rc::clone(&node));

    let mut children: IndexSet<RcByPtr<HoisterTree>> = IndexSet::new();
    collect_snapshot_deps(snapshot, lockfile, opts, nodes, &mut children)?;
    *node.dependencies.borrow_mut() = children;
    Ok(node)
}

fn collect_snapshot_deps(
    snapshot: &SnapshotEntry,
    lockfile: &Lockfile,
    opts: &HoistOpts,
    nodes: &mut HashMap<String, Rc<HoisterTree>>,
    out: &mut IndexSet<RcByPtr<HoisterTree>>,
) -> Result<(), HoistError> {
    let mut merged: HashMap<&PkgName, &pacquet_lockfile::SnapshotDepRef> = HashMap::new();
    for deps in [&snapshot.dependencies, &snapshot.optional_dependencies].into_iter().flatten() {
        for (alias, dep_ref) in deps {
            merged.insert(alias, dep_ref);
        }
    }
    let mut entries: Vec<_> = merged.into_iter().collect();
    entries.sort_by_key(|(alias, _)| alias.to_string());
    for (alias, dep_ref) in entries {
        // `dep_ref.resolve(alias)` returns the *snapshot lookup
        // key*: `<alias>@<ver>` for `Plain`, `<target>@<ver>` for
        // an npm-alias `Alias`. Pass that as `dep_key` so the
        // snapshot lookup hits the right entry. The node's exposed
        // `name` stays `alias`; only the lookup uses the resolved
        // target name.
        //
        // `link:` deps return `None` — they have no snapshot to
        // hoist (the install layer materialises them as direct
        // directory symlinks), so we skip them here, mirroring
        // upstream's `if (childDepPath)` check in `getChildren`.
        let Some(dep_key) = dep_ref.resolve(alias) else {
            continue;
        };
        let node = build_dep_node(alias, &dep_key, lockfile, opts, nodes)?;
        out.insert(RcByPtr(node));
    }
    Ok(())
}

/// Encode an importer id for use as a child node's `name` (and in
/// the hoisting-limits locator keys built by
/// `pacquet_package_manager::get_hoisting_limits`). Upstream uses
/// `encodeURIComponent`, which percent-encodes everything except
/// `A-Z a-z 0-9 - _ . ! ~ * ' ( )`. Pacquet workspace importers are
/// filesystem-relative paths, so the common case is alphanumeric +
/// `/` + `-` + `_`. Encode `/` (since it would confuse
/// `node_modules` directory parsing) and pass the rest through; if a
/// richer set ever shows up the function can switch to a full
/// encoder without touching call sites.
#[must_use]
pub fn percent_encode_path(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            'A'..='Z'
            | 'a'..='z'
            | '0'..='9'
            | '-'
            | '_'
            | '.'
            | '!'
            | '~'
            | '*'
            | '\''
            | '('
            | ')' => out.push(ch),
            '/' => out.push_str("%2F"),
            other => {
                // Best-effort %xx encode for the ASCII subset we
                // expect in importer ids. Anything else is left
                // verbatim — pacquet's lockfile doesn't currently
                // hand the wrapper non-ASCII paths.
                if (other as u32) < 0x80 {
                    write!(out, "%{:02X}", other as u32).unwrap();
                } else {
                    out.push(other);
                }
            }
        }
    }
    out
}

/// Pacquet's port of the `@yarnpkg/nm` hoist algorithm. Walks the
/// input tree, deep-copies it into a `HoisterResult` shape, then
/// pulls eligible descendants up to the root via a depth-first
/// recursion run to a fixed point (see [`hoist_into_root`]) with
/// parent-wins conflict resolution. Models the common case
/// of pnpm's `nodeLinker: hoisted` install — every transitive
/// dependency that doesn't collide with an already-hoisted name
/// surfaces at the root, just like a flat `node_modules`.
///
/// What this models today:
///
/// * Free hoist: a transitive dep with no name collision at the
///   root surfaces at the root.
/// * Identity dedup: a dep reachable through multiple parents
///   (same `Rc` thanks to the wrapper's `nodes` cache) collapses
///   to one node at root.
/// * Parent-wins on version conflict: when two distinct deps
///   share an alias but resolve to different snapshot keys, the
///   first one the DFS reaches takes the root slot and the other
///   stays under its parent.
/// * Peer-shadow refusal: a candidate whose `peer_names` would
///   resolve against an ancestor's dep with a different ident
///   than the root carries stays nested under its parent. See
///   [`would_shadow_peer`] for the ancestor-path walk and how
///   pacquet's DAG-preserving model differs from upstream's
///   per-path tree clone in rare cross-path mismatch cases.
/// * Multi-round convergence: when a round refuses a hoist
///   because of a peer mismatch and a subsequent move shifts the
///   blocking ident out of the ancestor chain (or into a
///   compatible root slot), the next round reconsiders the
///   refused candidate. The outer loop in [`hoist_into_root`]
///   iterates until a round makes no moves. Bounded by O(N)
///   rounds since each move is one-way (parent → root).
///
/// What this models today (continued):
///
/// * `hoistingLimits` borders. Names in
///   `opts.hoisting_limits[root_locator]` mark hoisting borders: a
///   bordered node's descendants stay nested beneath it rather than
///   hoisting to the root (`AbsorbDecision::Border`). Mirrors
///   upstream's `isHoistBorder` flag.
/// * `externalDependencies` placeholders — the wrapper adds them
///   as zero-children `ExternalSoftLink` nodes at the root, and
///   strips them from the result post-hoist. Same observable
///   shape as upstream.
///
/// What this does *not* model yet:
///
/// * Popularity-based ident preference (upstream's
///   `buildPreferenceMap`). When two distinct deps share an
///   alias, pacquet picks the first-visited; upstream picks the
///   one with more incoming references. Outcome differs for the
///   handful of upstream test cases that exercise the tie-break.
/// * Per-importer roots and the multi-level output shape upstream
///   produces for workspaces. [`hoist`] does attach every non-root
///   importer as a `Workspace`-kind child of the virtual `.` root
///   when [`HoistOpts::hoist_workspace_packages`] is enabled, but the
///   algorithm still hoists into that single `.` root rather than
///   giving each importer its own hoisting root.
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
    // Compute the root locator from the input tree, where each
    // node carries a single unambiguous `reference`. The result
    // graph collects references into a `BTreeSet` (one
    // `HoisterResult` can absorb several `HoisterTree` nodes with
    // the same ident), so deriving the locator from a result
    // node would mean picking an arbitrary entry from the set;
    // doing it here keeps the lookup well-defined.
    let root_locator = format!("{}@{}", tree.ident_name, tree.reference);
    let mut memo: HashMap<*const HoisterTree, Rc<HoisterResult>> = HashMap::new();
    let root = convert(tree, &mut memo);
    hoist_into_root(&root, &root_locator, opts);
    // Returning an owned `HoisterResult` (rather than
    // `Rc<HoisterResult>`) keeps the wrapper's post-hoist
    // `external_dependencies` filter from mutating the shared graph.
    // Cloning the outer struct duplicates only the top-level fields —
    // the subtree children remain shared via the cloned `RcByPtr`
    // values, so deep deps stay deduplicated.
    (*root).clone()
}

/// Outcome of the per-child hoist decision at the root.
enum AbsorbDecision {
    /// Root's name slot is free; the child should be moved up to
    /// the root.
    Free,
    /// Root already holds *this exact `Rc`* (the same node was
    /// reachable through another parent path and got hoisted
    /// earlier). The duplicate reference in the current parent
    /// just needs to be removed.
    SameNode,
    /// Root's name slot is taken by a different `Rc` — a version
    /// conflict. The child stays under its current parent.
    Conflict,
    /// Hoisting would shadow a peer dependency one of the
    /// candidate's ancestors satisfies with a different ident than
    /// what the root provides. The child stays under its parent so
    /// the ancestor's peer resolution still finds the intended
    /// version. Mirrors upstream's `getNodeHoistInfo` peer checks
    /// at [hoist.ts:414][peer-shadow-root] and
    /// [hoist.ts:454-479][peer-path].
    ///
    /// [peer-shadow-root]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L414
    /// [peer-path]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L454-L479
    PeerShadow,
    /// The candidate sits beneath a hoisting border — its parent (or
    /// a higher ancestor) has a name listed in
    /// `opts.hoisting_limits` for the root locator. A bordered node's
    /// descendants stay nested beneath it rather than hoisting to the
    /// root, so the candidate stays under its parent. Mirrors
    /// upstream's `isHoistBorder` flag set during `cloneTree` from
    /// [`hoist.ts:707`][hoist-border], which blocks a bordered node's
    /// children from hoisting past it (not the bordered node itself).
    ///
    /// [hoist-border]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L707
    Border,
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
/// For a DAG where the same node is reachable through multiple
/// paths, only the first-arrived path is consulted; upstream's
/// `cloneTree` produces a strict tree (per-path duplication) and
/// gets a per-path peer decision for free, but pacquet preserves
/// DAG sharing and accepts a more conservative ruling in the
/// rare cross-path mismatch cases. The cost is layouts that are
/// sometimes more nested than pnpm's, never less.
fn hoist_into_root(root: &Rc<HoisterResult>, root_locator: &str, opts: &HoistOpts) {
    let mut root_index: HashMap<String, RcByPtr<HoisterResult>> =
        root.dependencies.borrow().iter().map(|dep| (dep.0.name.clone(), dep.clone())).collect();

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
        let mut visited: HashSet<*const HoisterResult> = HashSet::new();
        let changed =
            hoist_subtree(root, &[], root, &mut root_index, &mut visited, border_names, false);
        if !changed {
            break;
        }
    }
}

/// Depth-first hoist driver. `ancestor_path` is the path from
/// `root` down to (but *excluding*) `node`, so for the root
/// itself it is empty and for a child of root it is `[root]`.
/// Returns whether this subtree moved at least one node in the
/// current round — the outer multi-round loop uses that to
/// decide whether another round can unlock further hoists.
fn hoist_subtree(
    node: &Rc<HoisterResult>,
    ancestor_path: &[Rc<HoisterResult>],
    root: &Rc<HoisterResult>,
    root_index: &mut HashMap<String, RcByPtr<HoisterResult>>,
    visited: &mut HashSet<*const HoisterResult>,
    border_names: &BTreeSet<String>,
    under_border: bool,
) -> bool {
    let root_ptr = Rc::as_ptr(root);
    if !visited.insert(Rc::as_ptr(node)) {
        return false;
    }
    let mut changed_in_subtree = false;

    // A node whose name is in `border_names` is a hoisting border:
    // its descendants are kept nested beneath it rather than hoisted
    // to the root. `under_border` carries that boundary down the
    // recursion — once any proper ancestor of a node is a border,
    // the node (and everything below it) stays put. Mirrors
    // upstream's `isHoistBorder` flag, which blocks a bordered
    // node's *children* from hoisting past it, not the bordered
    // node itself.
    let children_blocked = under_border || border_names.contains(&node.name);

    // Snapshot the current children so we can mutate
    // `node.dependencies` mid-iteration without invalidating the
    // borrow. `RcByPtr::clone` just bumps refcounts.
    let children: Vec<RcByPtr<HoisterResult>> =
        node.dependencies.borrow().iter().cloned().collect();

    let is_root = Rc::ptr_eq(node, root);

    // Path from root down to and including `node` — i.e. the
    // ancestor path for `node`'s direct children. Used both for
    // peer-shadow checks (children) and as the starting point
    // for the path passed into recursion when a child stays
    // nested.
    let mut path_for_children: Vec<Rc<HoisterResult>> = ancestor_path.to_vec();
    path_for_children.push(Rc::clone(node));

    for child in children {
        if Rc::as_ptr(&child.0) == root_ptr {
            // Back-edge to root via a cycle. Nothing to hoist.
            continue;
        }

        // A hoisting border on this `node` (or any ancestor) keeps
        // every descendant nested, so the child stays under its
        // parent regardless of whether the root slot is free. Decided
        // before the free/dedup/conflict lookup because the border
        // wins outright.
        let mut decision = if children_blocked {
            AbsorbDecision::Border
        } else {
            match root_index.get(&child.0.name) {
                None => AbsorbDecision::Free,
                Some(existing) if Rc::ptr_eq(&existing.0, &child.0) => AbsorbDecision::SameNode,
                Some(_) => AbsorbDecision::Conflict,
            }
        };

        // Peer-aware refusal layered on top of the basic
        // free / dedup / conflict decision. `Conflict` already
        // leaves the candidate in place and `SameNode` dedups
        // an already-hoisted shared `Rc`, so the peer check
        // only matters when we'd otherwise hoist.
        if matches!(decision, AbsorbDecision::Free)
            && would_shadow_peer(&child.0, &path_for_children, root, root_index)
        {
            decision = AbsorbDecision::PeerShadow;
        }

        // Apply the decision, *then* compute the path to pass
        // into recursion based on the child's *new* position.
        // Computing post-decision is the load-bearing detail:
        // the recursion path always reflects the child's current
        // position in the result graph, so peer checks deeper
        // down see ancestors that are actually ancestors.
        let child_recursion_path: Vec<Rc<HoisterResult>> = if is_root {
            // Root's direct children are already at root — no
            // movement happens, and their ancestor path is
            // simply `[root]`.
            path_for_children.clone()
        } else {
            match decision {
                AbsorbDecision::Free => {
                    node.dependencies.borrow_mut().shift_remove(&child);
                    root.dependencies.borrow_mut().insert(child.clone());
                    root_index.insert(child.0.name.clone(), child.clone());
                    changed_in_subtree = true;
                    // Child is now a direct dep of root; its
                    // ancestor path collapses to `[root]`.
                    vec![Rc::clone(root)]
                }
                AbsorbDecision::SameNode => {
                    // The shared `Rc` is already at root; strip
                    // the duplicate reference at this parent so
                    // the deeper copy disappears. Child's actual
                    // ancestor path is `[root]`.
                    node.dependencies.borrow_mut().shift_remove(&child);
                    changed_in_subtree = true;
                    vec![Rc::clone(root)]
                }
                AbsorbDecision::Conflict | AbsorbDecision::PeerShadow | AbsorbDecision::Border => {
                    // Stays at the current parent. The version
                    // already at root wins the slot, hoisting would
                    // shadow a peer dependency, or the candidate sits
                    // beneath a `hoisting_limits` border. Child's
                    // ancestor path is the path through `node`; a
                    // later round may revisit this candidate with a
                    // different peer / conflict context (the border
                    // boundary is fixed so the Border verdict won't
                    // change).
                    path_for_children.clone()
                }
            }
        };

        let child_changed = hoist_subtree(
            &child.0,
            &child_recursion_path,
            root,
            root_index,
            visited,
            border_names,
            children_blocked,
        );
        changed_in_subtree |= child_changed;
    }
    changed_in_subtree
}

/// Return `true` when hoisting `candidate` onto the root would
/// shadow a peer dependency one of its ancestors already
/// satisfies with a different ident.
///
/// Implements two of the three peer guards upstream's
/// `getNodeHoistInfo` runs:
///
/// * **Root-shadow** — the candidate's own name appears in
///   `root.peer_names`. The root expects to *receive* this name as
///   a peer from its own parent, so promoting the candidate into
///   the root's name slot would change peer resolution for
///   anything that sees the root.
/// * **Ancestor-path mismatch** — for each peer name `P` in
///   `candidate.peer_names`, walk the candidate's ancestors from
///   deepest (immediate parent) toward the root. The first
///   ancestor that has a direct dep named `P` (and doesn't itself
///   peer-pass `P` through) is the one whose ident the candidate
///   resolves at runtime. If the root provides a *different*
///   ident for `P` (or none at all), promoting the candidate
///   would silently re-resolve its peer to the wrong package, so
///   we leave it nested.
///
/// Differs from upstream's check in one DAG case: upstream's
/// [`cloneTree`][clone] duplicates the work tree into a strict
/// tree per parent path, so each visit has a unique ancestor
/// chain. Pacquet preserves the DAG, and the DFS records only
/// the path it actually used to reach the candidate; if the same
/// candidate could be reached via a peer-compatible alternative
/// path, we still refuse to hoist. The result is at most
/// over-nested layouts, never under-nested ones.
///
/// [clone]: https://github.com/yarnpkg/berry/blob/4287909fa6a0a1ec976a55776bff606864b31990/packages/yarnpkg-nm/sources/hoist.ts#L670
fn would_shadow_peer(
    candidate: &HoisterResult,
    ancestor_path: &[Rc<HoisterResult>],
    root: &Rc<HoisterResult>,
    root_index: &HashMap<String, RcByPtr<HoisterResult>>,
) -> bool {
    // Root-shadow guard. Pacquet's wrapper builds the `.` root with
    // empty `peer_names` (it's a `Workspace`-kind node), so in
    // practice this check never fires today — kept for parity with
    // upstream and to stay correct if a future caller hands in a
    // root with declared peers.
    if root.peer_names.contains(&candidate.name) {
        return true;
    }

    'peer_loop: for peer_name in &candidate.peer_names {
        // Walk ancestors deepest-first so the closest provider
        // wins. An ancestor whose own `peer_names` includes this
        // name *and* doesn't carry it as a direct dep is just
        // passing the peer through — keep walking past it.
        for ancestor in ancestor_path.iter().rev() {
            // Clone before dropping the borrow so the Rc outlives
            // the `Ref` we'd otherwise hold.
            let provider_rc = ancestor
                .dependencies
                .borrow()
                .iter()
                .find(|dep| dep.0.name == *peer_name)
                .map(|dep| Rc::clone(&dep.0));

            if let Some(provider) = provider_rc {
                // Found a concrete provider in the ancestor
                // chain. Compare its identity against root's
                // current slot for the same name.
                match root_index.get(peer_name) {
                    Some(at_root) if Rc::ptr_eq(&at_root.0, &provider) => {
                        // Root already carries this exact
                        // provider — promoting the candidate
                        // doesn't change resolution. Move to
                        // the next peer.
                        continue 'peer_loop;
                    }
                    _ => {
                        // Root either has a different ident
                        // for this peer or doesn't have one
                        // at all. Either way, hoisting would
                        // shadow.
                        return true;
                    }
                }
            }
            // This ancestor doesn't supply the peer.
            // Walk further up — the actual provider may
            // be a parent of this ancestor (the common
            // shape is `ancestor` peer-passes the name
            // through to its own parent). If we exhaust
            // the path without finding any provider,
            // there's no ancestor-bound peer to shadow
            // and the candidate may hoist freely for
            // this peer.
        }
        // No ancestor (excluding root) provides the peer; the
        // candidate either resolves it at root or leaves it
        // unsatisfied. Either case is "no shadow" — keep going.
    }
    false
}

fn convert(
    tree: &HoisterTree,
    memo: &mut HashMap<*const HoisterTree, Rc<HoisterResult>>,
) -> Rc<HoisterResult> {
    let ptr = std::ptr::from_ref::<HoisterTree>(tree);
    if let Some(existing) = memo.get(&ptr) {
        return Rc::clone(existing);
    }
    // Stash a node with empty `dependencies`, then recurse and
    // populate the cell in place. Anyone reached via a back-edge
    // gets `Rc::clone` of the same allocation and reads the
    // (eventually-populated) cell — matches the in-place mutation
    // semantics the real hoist algorithm needs.
    let mut refs = BTreeSet::new();
    refs.insert(tree.reference.clone());
    let node = Rc::new(HoisterResult {
        name: tree.name.clone(),
        ident_name: tree.ident_name.clone(),
        references: RefCell::new(refs),
        peer_names: tree.peer_names.clone(),
        dependencies: RefCell::new(IndexSet::new()),
    });
    memo.insert(ptr, Rc::clone(&node));

    // Collect the children before recursing so we can drop the
    // `Ref<'_, IndexSet<...>>` borrow on `tree.dependencies`. The
    // recursion only reads (not mutates) `HoisterTree` cells, so
    // holding the borrow across recursive calls is technically
    // safe, but releasing it keeps the panic surface smaller if
    // the algorithm later grows a mutation pass over the input.
    let to_convert: Vec<RcByPtr<HoisterTree>> =
        tree.dependencies.borrow().iter().cloned().collect();
    let mut children: IndexSet<RcByPtr<HoisterResult>> = IndexSet::new();
    for child in to_convert {
        children.insert(RcByPtr(convert(&child.0, memo)));
    }
    *node.dependencies.borrow_mut() = children;
    node
}

#[cfg(test)]
mod tests;
