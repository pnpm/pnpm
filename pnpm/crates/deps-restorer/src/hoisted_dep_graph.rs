//! Type skeleton for the directory-keyed dependency graph that
//! `nodeLinker: hoisted` installs produce.
//!
//! The walker [`lockfile_to_hoisted_dep_graph`] takes a wanted
//! lockfile plus an optional *current* lockfile and runs
//! `pnpm_real_hoist::hoist` to get the directory shape, then
//! assembles a [`LockfileToDepGraphResult`] keyed by the computed
//! absolute directory of every node. Store I/O (`fetching` /
//! `files_index_file`) is still deferred — those fields are
//! populated by the linker, which kicks off store fetches when it
//! has a real consumer for the handles.
//!
//! Unlike the depPath-keyed [`crate::deps_graph`] module (which is
//! a hashing-side adapter for the build cache), the graph defined
//! here is keyed by *absolute directory path* — that's the
//! identity hoisted-linker nodes have, because the same package
//! can occupy several directories when a name conflict forces it
//! to nest. Hoisting decisions are made at directory granularity,
//! not depPath granularity.

use crate::{
    HoistedLocations,
    safe_join_modules_dir::{InvalidDependencyAliasError, safe_join_modules_dir},
};
use derive_more::{Display, Error, From};
use indexmap::IndexSet;
use miette::Diagnostic;
use pnpm_deps_path::get_pkg_id_with_patch_hash;
use pnpm_lockfile::{
    Lockfile, LockfileResolution, PackageKey, ParsePkgNameVerPeerError, PkgIdWithPatchHash,
};
use pnpm_modules_yaml::DepPath;
use pnpm_package_is_installable::{
    InstallabilityError, InstallabilityOptions, InstallabilityVerdict,
    PackageInstallabilityManifest, SupportedArchitectures, WantedEngine, package_is_installable,
};
use pnpm_patching::PatchInfo;
use pnpm_real_hoist::{HoistError, HoistOpts, HoisterResult, RcByPtr, hoist};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

/// One node in a hoisted-linker dependency graph. Keyed in the
/// outer [`DependenciesGraph`] by the node's absolute `dir`.
///
/// Omits the store-controller-bound fields (`fetching`,
/// `files_index_file`) that the walker only learns about once it
/// fetches the package. Those land in the follow-up sub-slice that
/// wires the store in; today, this type pins the shape of every
/// other field so the walker can fill them without churning the
/// call sites.
#[derive(Debug, Clone, PartialEq)]
pub struct DependenciesGraphNode {
    /// The alias this node was placed under in its parent's
    /// `node_modules`. Optional — only populated when the node is
    /// reached via the hoist walk.
    pub alias: Option<String>,
    /// The depPath that produced this node, used as the key for
    /// `hoistedLocations` and the join key for `hoistedDependencies`.
    pub dep_path: DepPath,
    /// `pkgIdWithPatchHash`: the patch-aware ident key
    /// the side-effects cache uses. Modelled by
    /// [`pnpm_lockfile::PkgIdWithPatchHash`] — a non-validating
    /// branded newtype around `String`.
    pub pkg_id_with_patch_hash: PkgIdWithPatchHash,
    /// Absolute path of the package's directory on disk. The
    /// outer [`DependenciesGraph`]'s key is this same value;
    /// it is stored on the node too so consumers don't need
    /// to walk the map by reverse lookup.
    pub dir: PathBuf,
    /// Absolute path of the `node_modules/` directory the package
    /// lives in (i.e. `dir.parent()`). Used by the bin-linker
    /// pass: every hoist location needs `<modules>/.bin` populated.
    pub modules: PathBuf,
    /// Alias → child `dir` of this node's listed dependencies, as
    /// computed from the lockfile snapshot's `dependencies` and
    /// (when included) `optionalDependencies`. The walker resolves
    /// each child to the directory the alias was hoisted to —
    /// which may be the root, a sibling, or this node's own
    /// `node_modules`, depending on the hoister's decision.
    pub children: BTreeMap<String, PathBuf>,
    pub name: String,
    pub version: String,
    pub optional: bool,
    pub optional_dependencies: BTreeSet<String>,
    pub has_bin: bool,
    pub has_bundled_dependencies: bool,
    pub patch: Option<PatchInfo>,
    pub resolution: LockfileResolution,
    /// `true` when the previous install recorded this package at `dir`
    /// (`.modules.yaml` `hoistedLocations`) and the directory still
    /// holds a `package.json` of the recorded version. The linker
    /// neither re-imports such a directory nor hands it to the build
    /// phase, as pnpm's walker does through `skipFetch` and
    /// `isBuilt`. Always `false` on a `force` walk, for a directory
    /// (`file:`) dependency, whose source is mutable, and for a patched
    /// package, whose patch is applied on a fresh copy.
    pub present: bool,
}

/// Directory-keyed graph of every hoisted-linker node the walker
/// emitted.
pub type DependenciesGraph = BTreeMap<PathBuf, DependenciesGraphNode>;

/// Recursive directory hierarchy: each `node_modules` directory
/// maps to its children, which in turn map to their own
/// children's `node_modules`. The linker walks this to know which
/// directories to populate (and in what order) and which
/// `<dir>/node_modules/.bin` to wire up.
///
/// Wrapped in a newtype rather than typedef'd to a recursive
/// `BTreeMap` because Rust doesn't allow recursive type aliases.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DepHierarchy(pub BTreeMap<PathBuf, DepHierarchy>);

/// Per-importer alias → direct-dependency directory. For the
/// single-importer case the only key is `"."`; workspace support
/// will add per-importer entries keyed by the importer's
/// project id.
pub type DirectDependenciesByImporterId = BTreeMap<String, BTreeMap<String, PathBuf>>;

/// Everything the walker hands back to the install pipeline.
///
/// All fields are populated for the hoisted-linker path; the
/// isolated linker uses the same struct with `hierarchy`,
/// `hoisted_locations`, and `symlinked_direct_dependencies_by_importer_id`
/// left empty.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct LockfileToDepGraphResult {
    pub graph: DependenciesGraph,
    pub direct_dependencies_by_importer_id: DirectDependenciesByImporterId,
    /// Outer key is the project root that owns the inner
    /// hierarchy (the workspace root for single-importer
    /// lockfiles, plus per-project roots once Slice 9 lands).
    pub hierarchy: BTreeMap<PathBuf, DepHierarchy>,
    /// Per-depPath list of lockfile-relative directory paths
    /// where the package landed. Round-trips through
    /// [`pnpm_modules_yaml::Modules::hoisted_locations`].
    ///
    /// The values are typed as raw `String` lists (not `DepPath`
    /// lists), even though the strings are populated from depPaths
    /// internally, to keep the on-disk shape identical. The
    /// same choice was made for the `Modules` schema field this
    /// round-trips through (see its doc-comment in
    /// `pnpm-modules-yaml`).
    pub hoisted_locations: BTreeMap<String, Vec<String>>,
    pub symlinked_direct_dependencies_by_importer_id: DirectDependenciesByImporterId,
    /// Diffed against `graph` by the linker's orphan-removal pass
    /// to know which directories the previous install owned that
    /// the new install does not. `None` on a fresh install (no
    /// prior lockfile).
    pub prev_graph: Option<DependenciesGraph>,
    /// Per-depPath list of directories where the package is
    /// expected to live as an *injected* workspace package. Used
    /// by the post-install re-mirror step. The keys are typed as raw
    /// `String`, not `DepPath`, to keep the on-disk shape identical.
    pub injection_targets_by_dep_path: BTreeMap<String, Vec<PathBuf>>,
    /// Packages the walker decided to skip — the input
    /// `opts.skipped` extended with any depPaths whose
    /// installability check failed (optional + unsupported
    /// platform/engine). pacquet returns the augmented set on the
    /// result so the caller can persist it into `.modules.yaml.skipped`
    /// without sharing mutable state.
    pub skipped: BTreeSet<String>,
}

/// Inputs the walker reads from. Carries the subset pacquet needs
/// for the hoisted-linker path that's actually implemented today.
/// Fields tied to the still-unported store controller, fetch
/// concurrency, or workspace project list will be added when their
/// consumers land.
#[derive(Debug, Clone)]
pub struct LockfileToHoistedDepGraphOptions<'a> {
    /// Project / workspace root. Used as the base for relativizing
    /// `hoisted_locations` entries and for placing the root's
    /// `node_modules/` directory.
    pub lockfile_dir: PathBuf,
    /// `autoInstallPeers` from `.npmrc`. Passed through to the
    /// hoister, which zeroes every node's `peer_names` when this
    /// is `true` so peer-constrained packages float freely.
    pub auto_install_peers: bool,
    /// Packages the previous install decided not to fetch
    /// (installability check failed; the package was added here).
    /// The walker skips any depPath in this set without consulting
    /// the snapshot. Cloned + extended on the way out. The
    /// hoisted-specific typing is a set of raw `String`s (rather than
    /// `DepPath`s), so the wrapper here is `BTreeSet<String>`.
    pub skipped: BTreeSet<String>,
    /// When true, suppress the installability check and emit every
    /// dep into the graph regardless of cpu / os / libc / engines.
    /// Used by the `prev_graph` walk (Slice 4d) where the previous
    /// lockfile is replayed wholesale to compute orphans — that walk
    /// passes `force: true` with an empty skip set so
    /// the diff catches packages that previously installed but
    /// would now be filtered.
    pub force: bool,
    /// `engineStrict` from config. When true, an engine mismatch on
    /// a *required* (non-optional) package becomes a hard error
    /// instead of a warning.
    pub engine_strict: bool,
    /// Current host's node version, used as the `engines.node`
    /// satisfiability target. See `InstallabilityOptions::current_node_version`.
    pub current_node_version: String,
    /// Current host's OS (`linux`, `darwin`, `win32`, ...).
    pub current_os: String,
    /// Current host's CPU architecture (`x64`, `arm64`, ...).
    pub current_cpu: String,
    /// Current host's libc variant (`glibc`, `musl`, or empty when
    /// the host is not Linux).
    pub current_libc: String,
    /// `supportedArchitectures` override from `pnpm-workspace.yaml`,
    /// widening the host-derived axes so a Linux host can prepare
    /// `node_modules` for a Windows / macOS target. `None` means use
    /// only the current-host axes.
    pub supported_architectures: Option<SupportedArchitectures>,
    /// Mirrors [`pnpm_real_hoist::HoistOpts::hoist_workspace_packages`].
    /// When `true` (the default), every non-root workspace importer
    /// becomes a `Workspace`-kind child of the virtual `.` root in
    /// the hoist tree, and the walker emits per-importer subtrees
    /// under `<lockfile_dir>/<importer_id>/node_modules`. When
    /// `false`, only the root importer's subtree is emitted (the
    /// hoister also skips adding the workspace children to its
    /// shared tree). Pacquet's `Config::hoist_workspace_packages`
    /// (in `pnpm-config`) drives this from the install pipeline.
    pub hoist_workspace_packages: bool,

    /// Per-importer block-list passed straight through to
    /// [`pnpm_real_hoist::HoistOpts::hoisting_limits`]. See the
    /// hoister's doc-comment for the locator-keyed shape and
    /// `Config::hoisting_limits` in `pnpm-config` for how the
    /// install pipeline derives this from `pnpm-workspace.yaml`.
    pub hoisting_limits: pnpm_real_hoist::HoistingLimits,

    /// Reserved-name list passed straight through to
    /// [`pnpm_real_hoist::HoistOpts::external_dependencies`].
    /// See the hoister's doc-comment for the strip semantics and
    /// `Config::external_dependencies` in `pnpm-config` for how
    /// the install pipeline derives this from
    /// `pnpm-workspace.yaml`.
    pub external_dependencies: BTreeSet<String>,

    /// `hoistedLocations` recorded by the previous install's
    /// `.modules.yaml`. A package the walker places at a directory
    /// listed here, which still holds a `package.json` of the expected
    /// version, is marked [`DependenciesGraphNode::present`] so the
    /// linker skips it. `None` on a first install, and ignored when
    /// `force` is set.
    pub current_hoisted_locations: Option<&'a HoistedLocations>,
}

impl Default for LockfileToHoistedDepGraphOptions<'_> {
    fn default() -> Self {
        Self {
            lockfile_dir: PathBuf::new(),
            auto_install_peers: false,
            skipped: BTreeSet::new(),
            force: false,
            engine_strict: false,
            current_node_version: String::new(),
            current_os: String::new(),
            current_cpu: String::new(),
            current_libc: String::new(),
            supported_architectures: None,
            // Match the hoister's default-on behavior so a
            // `..Default::default()`-style construction at the call
            // site doesn't silently disable workspace hoisting.
            hoist_workspace_packages: true,
            hoisting_limits: pnpm_real_hoist::HoistingLimits::new(),
            external_dependencies: BTreeSet::new(),
            current_hoisted_locations: None,
        }
    }
}

/// Failure modes of [`lockfile_to_hoisted_dep_graph`]. Marked
/// `#[non_exhaustive]` so adding variants in later sub-slices (the
/// installability filter, the store-fetch integration) isn't a
/// breaking API change.
#[derive(Debug, Display, Error, Diagnostic, From)]
#[non_exhaustive]
pub enum HoistedDepGraphError {
    /// The hoister refused the lockfile (broken snapshot,
    /// unsupported workspace, etc.). Surfaced verbatim so callers
    /// see the underlying error code.
    Hoist(#[error(source)] HoistError),
    /// A `HoisterResult` node carried a reference string that
    /// doesn't parse as a `name@version[(peers)]` package key.
    /// Should never happen for hoister output produced from a
    /// valid lockfile — the hoister only emits references it
    /// already validated — but the conversion is fallible at the
    /// type level, so a typed error is the honest surface.
    #[display("Unparsable snapshot reference {reference:?} on hoisted node")]
    #[diagnostic(code(ERR_PNPM_HOISTED_GRAPH_BAD_REFERENCE))]
    BadReference {
        reference: String,
        #[error(source)]
        source: ParsePkgNameVerPeerError,
    },
    /// A required (non-optional) package failed the
    /// installability check. `engineStrict` + an engine mismatch
    /// surfaces as `ERR_PNPM_UNSUPPORTED_ENGINE`; the inner
    /// `InstallabilityError` is propagated transparently so
    /// callers see the same diagnostic code
    /// (`ERR_PNPM_UNSUPPORTED_ENGINE` /
    /// `ERR_PNPM_UNSUPPORTED_PLATFORM` /
    /// `ERR_PNPM_INVALID_NODE_VERSION`), and the inner error
    /// already carries the package id for context.
    #[diagnostic(transparent)]
    Installability(#[error(source)] Box<InstallabilityError>),
    /// A hoisted node's alias was rejected by `safe_join_modules_dir`
    /// before the join. Surfaces `ERR_PNPM_INVALID_DEPENDENCY_NAME`.
    #[diagnostic(transparent)]
    InvalidDependencyAlias(#[error(source)] InvalidDependencyAliasError),
}

/// Build a directory-keyed [`LockfileToDepGraphResult`] from a
/// wanted lockfile, plus an optional *current* lockfile to diff
/// against.
///
/// The store-controller-bound `fetching` / `files_index_file`
/// fields on each graph node remain default-valued — those are
/// populated by Slice 5's linker, which kicks off the actual
/// store fetches when it has a real consumer for the handles.
///
/// Multi-importer (workspace) lockfiles are supported: the hoister
/// ([`pnpm_real_hoist::hoist`]) attaches each non-root importer as
/// a workspace child of the virtual `.` root when
/// `hoist_workspace_packages` is enabled. Per-importer hoisting roots
/// (a multi-level output shape) are not modelled yet.
pub fn lockfile_to_hoisted_dep_graph(
    lockfile: &Lockfile,
    current_lockfile: Option<&Lockfile>,
    opts: &LockfileToHoistedDepGraphOptions<'_>,
) -> Result<LockfileToDepGraphResult, HoistedDepGraphError> {
    // Prev-graph walk: forced (every snapshot in the current
    // lockfile must surface so the diff catches packages that
    // would now fail installability) and unskipped (the previous
    // install's `skipped` is irrelevant — we want the full
    // previous layout to compute orphans against).
    let prev_graph = match current_lockfile {
        // Require a non-empty `packages` map. For an empty `packages:
        // {}` the inner walk produces an empty graph too, which
        // is observationally equivalent to "no orphans to
        // consider". Pacquet collapses both absent and empty into
        // `prev_graph: None` so the API contract is unambiguous
        // and the empty case skips the (no-op) second walk.
        Some(current) if current.packages.as_ref().is_some_and(|packages| !packages.is_empty()) => {
            let prev_opts = LockfileToHoistedDepGraphOptions {
                force: true,
                skipped: BTreeSet::new(),
                ..opts.clone()
            };
            Some(build_dep_graph(current, &prev_opts, None)?.graph)
        }
        _ => None,
    };

    let mut result = build_dep_graph(lockfile, opts, prev_graph.as_ref())?;
    result.prev_graph = prev_graph;
    Ok(result)
}

/// Inner builder: runs the hoister + walker for one lockfile and
/// returns the per-walk subset of [`LockfileToDepGraphResult`]
/// (everything except `prev_graph`, which only the outer wrapper
/// sets).
fn build_dep_graph<'a>(
    lockfile: &'a Lockfile,
    opts: &'a LockfileToHoistedDepGraphOptions<'a>,
    prev_graph: Option<&'a DependenciesGraph>,
) -> Result<LockfileToDepGraphResult, HoistedDepGraphError> {
    let hoist_opts = HoistOpts {
        auto_install_peers: opts.auto_install_peers,
        hoist_workspace_packages: opts.hoist_workspace_packages,
        hoisting_limits: opts.hoisting_limits.clone(),
        external_dependencies: opts.external_dependencies.clone(),
    };
    let hoister_result = hoist(lockfile, &hoist_opts)?;

    let modules_dir = opts.lockfile_dir.join("node_modules");
    let mut state = WalkState {
        lockfile,
        lockfile_dir: &opts.lockfile_dir,
        opts,
        prev_graph,
        skipped: opts.skipped.clone(),
        graph: DependenciesGraph::new(),
        pkg_locations_by_pkg_id: BTreeMap::new(),
        hoisted_locations: BTreeMap::new(),
        injection_targets_by_dep_path: BTreeMap::new(),
        per_importer_hierarchies: BTreeMap::new(),
        per_importer_direct_deps: BTreeMap::new(),
    };
    let root_deps = hoister_result.dependencies.borrow();
    let root_hierarchy = walk_deps(&mut state, &modules_dir, &root_deps)?;
    drop(root_deps);
    state.into_result(root_hierarchy)
}

impl WalkState<'_> {
    /// Pass 2 — fill in each node's `children` map from the
    /// now-complete `pkg_locations_by_pkg_id`, then assemble the result.
    ///
    /// The walk intentionally leaves `children` empty: every sibling and
    /// descendant of a node must have its directory recorded in
    /// `pkg_locations_by_pkg_id` before any node resolves its children,
    /// so the location index is complete by the time children are
    /// computed. The simplest way to preserve that invariant is to
    /// insert everything first and resolve children second.
    fn into_result(
        mut self,
        root_hierarchy: DepHierarchy,
    ) -> Result<LockfileToDepGraphResult, HoistedDepGraphError> {
        fill_children(&mut self.graph, &self.pkg_locations_by_pkg_id, self.lockfile)?;

        // The hoister produced a children order; the directory keys in
        // `root_hierarchy` follow it, and
        // `direct_dependencies_by_importer_id["."]` is built from that
        // order.
        let mut direct_dependencies_by_importer_id: DirectDependenciesByImporterId =
            BTreeMap::new();
        direct_dependencies_by_importer_id.insert(
            Lockfile::ROOT_IMPORTER_KEY.to_string(),
            root_direct_deps(&root_hierarchy, &self.graph),
        );

        // `link:` entries are skipped — they don't enter the hoist tree
        // and have no `pkg_locations` entry. The install pipeline handles
        // them via [`crate::SymlinkDirectDependencies`]'s `link_only` pass
        // after the hoisted linker runs.
        for importer_id in self.per_importer_direct_deps.keys() {
            let Some(importer) = self.lockfile.importers.get(importer_id) else { continue };
            direct_dependencies_by_importer_id.insert(
                importer_id.clone(),
                importer_direct_deps(importer, &self.pkg_locations_by_pkg_id),
            );
        }

        // Hierarchy: one entry per importer root. Root importer gets
        // `lockfile_dir`; non-root workspace importers get
        // `<lockfile_dir>/<importer_id>` — the linker walks each
        // importer's subtree under its own root.
        let mut hierarchy = BTreeMap::new();
        hierarchy.insert(self.opts.lockfile_dir.clone(), root_hierarchy);
        hierarchy.extend(self.per_importer_hierarchies);

        Ok(LockfileToDepGraphResult {
            graph: self.graph,
            direct_dependencies_by_importer_id,
            hierarchy,
            hoisted_locations: self.hoisted_locations,
            symlinked_direct_dependencies_by_importer_id: DirectDependenciesByImporterId::new(),
            prev_graph: None,
            injection_targets_by_dep_path: self.injection_targets_by_dep_path,
            skipped: self.skipped,
        })
    }
}

/// The root importer's direct dependencies, in the children order the
/// hoister produced.
fn root_direct_deps(
    root_hierarchy: &DepHierarchy,
    graph: &DependenciesGraph,
) -> BTreeMap<String, PathBuf> {
    let mut direct_deps = BTreeMap::new();
    for child_dir in root_hierarchy.0.keys() {
        if let Some(alias) = graph.get(child_dir).and_then(|node| node.alias.as_deref()) {
            direct_deps.insert(alias.to_string(), child_dir.clone());
        }
    }
    direct_deps
}

/// One non-root importer's direct dependencies, read off its declared
/// lockfile entries rather than off its hoist-tree node: the hoister
/// moves dedupe-able deps up to root, leaving the workspace node's
/// children empty even when the importer declared those deps. The first
/// recorded location of each resolved snapshot wins.
fn importer_direct_deps(
    importer: &pnpm_lockfile::ProjectSnapshot,
    pkg_locations_by_pkg_id: &BTreeMap<String, Vec<PathBuf>>,
) -> BTreeMap<String, PathBuf> {
    let mut direct_deps = BTreeMap::new();
    for dep_map in [
        importer.dependencies.as_ref(),
        importer.dev_dependencies.as_ref(),
        importer.optional_dependencies.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        for (alias, spec) in dep_map {
            // For an aliased dep the snapshot key uses the alias's own
            // (name, suffix); for a regular dep it's `(alias, version)`.
            let Some(dep_key) = spec.version.resolved_key(alias) else { continue };
            if let Some(first) = pkg_locations_by_pkg_id
                .get(&pnpm_real_hoist::pkg_id(&dep_key))
                .and_then(|locations| locations.first())
            {
                direct_deps.insert(alias.to_string(), first.clone());
            }
        }
    }
    direct_deps
}

/// Second walker pass: with every node's directory already in
/// `pkg_locations`, resolve each graph node's `children: alias →
/// dir` map by looking up the node's snapshot in the lockfile.
fn fill_children(
    graph: &mut DependenciesGraph,
    pkg_locations: &BTreeMap<String, Vec<PathBuf>>,
    lockfile: &Lockfile,
) -> Result<(), HoistedDepGraphError> {
    let dirs: Vec<PathBuf> = graph.keys().cloned().collect();
    for dir in dirs {
        let reference = graph[&dir].dep_path.as_str().to_string();
        let pkg_key: PackageKey = match reference.parse() {
            Ok(key) => key,
            Err(source) => {
                return Err(HoistedDepGraphError::BadReference { reference, source });
            }
        };
        let snapshot = lockfile.snapshots.as_ref().and_then(|m| m.get(&pkg_key));
        let children = compute_children(snapshot, pkg_locations);
        if let Some(node) = graph.get_mut(&dir) {
            node.children = children;
        }
    }
    Ok(())
}

/// Mutable scratch space the recursive walker threads through
/// every level. Borrowing the lockfile + `lockfile_dir` + opts up
/// front avoids passing four separate arguments. `skipped` is
/// owned (cloned from `opts.skipped`) because the walker mutates
/// it — every dep that fails the installability check gets added.
struct WalkState<'a> {
    lockfile: &'a Lockfile,
    lockfile_dir: &'a Path,
    opts: &'a LockfileToHoistedDepGraphOptions<'a>,
    /// The graph the current lockfile produces, when there is one. Only
    /// [`walk_dep`]'s presence check reads it, to see whether the
    /// package the previous install put at a directory resolves the same
    /// way as the one going there now. `None` on the fresh-lockfile
    /// path, which has no current lockfile to walk.
    prev_graph: Option<&'a DependenciesGraph>,
    skipped: BTreeSet<String>,
    graph: DependenciesGraph,
    /// Records every directory each package landed in, in visit
    /// order. The first entry wins for parent → child wiring.
    ///
    /// Keyed by [`pnpm_real_hoist::pkg_id`], not the snapshot key:
    /// the hoister collapses every peer variant of one package
    /// version onto a single node, so only the first-seen variant's
    /// key reaches this walk. Sharing the hoister's own identity
    /// function is what lets an edge declared against any other
    /// variant still find that node's directory.
    pkg_locations_by_pkg_id: BTreeMap<String, Vec<PathBuf>>,
    hoisted_locations: BTreeMap<String, Vec<String>>,
    injection_targets_by_dep_path: BTreeMap<String, Vec<PathBuf>>,
    /// Per-non-root-importer hierarchy emitted while walking
    /// `Workspace`-kind nodes. Outer key is the importer's root
    /// directory (`<lockfile_dir>/<importer_id>`). Folded into
    /// [`LockfileToDepGraphResult::hierarchy`] alongside the root
    /// importer's hierarchy by [`build_dep_graph`].
    per_importer_hierarchies: BTreeMap<PathBuf, DepHierarchy>,
    /// Per-non-root-importer direct dependencies emitted while
    /// walking `Workspace`-kind nodes. Outer key is the importer
    /// id from the lockfile (e.g. `packages/foo`). Folded into
    /// [`LockfileToDepGraphResult::direct_dependencies_by_importer_id`]
    /// alongside the root importer's entry by [`build_dep_graph`].
    per_importer_direct_deps: DirectDependenciesByImporterId,
}

/// Recursive walker over `HoisterResult.dependencies`. Skips the
/// store-fetch / installability path; here the walker only computes
/// node identity, location, children, and hoisted-location records.
///
/// No cycle detection — the walk trusts the hoister to produce a
/// DAG. The hoister's own cyclic-input tests pin that property.
fn walk_deps(
    state: &mut WalkState<'_>,
    modules: &Path,
    deps: &IndexSet<RcByPtr<HoisterResult>>,
) -> Result<DepHierarchy, HoistedDepGraphError> {
    let mut hierarchy: BTreeMap<PathBuf, DepHierarchy> = BTreeMap::new();
    for dep in deps {
        if let Some((dir, inner_hierarchy)) = walk_dep(state, modules, dep)? {
            hierarchy.insert(dir, inner_hierarchy);
        }
    }
    Ok(DepHierarchy(hierarchy))
}

/// One node of [`walk_deps`]. `None` when the node contributes nothing
/// to the parent's hierarchy: a workspace importer, a link placeholder,
/// or a package the installability filter skipped.
fn walk_dep(
    state: &mut WalkState<'_>,
    modules: &Path,
    dep: &RcByPtr<HoisterResult>,
) -> Result<Option<(PathBuf, DepHierarchy)>, HoistedDepGraphError> {
    // The hoister keeps every absorbed reference; the first
    // (alphabetically smallest) is the canonical depPath for this
    // node's location.
    let Some(reference) = dep.0.references.borrow().iter().next().cloned() else {
        return Ok(None);
    };

    if state.skipped.contains(&reference) {
        return Ok(None);
    }

    if let Some(importer_id) = reference.strip_prefix("workspace:") {
        walk_workspace_importer(state, dep, importer_id)?;
        return Ok(None);
    }

    let Some(resolved) = resolve_reference(state, &reference)? else {
        return Ok(None);
    };
    let optional = resolved.snapshot.is_some_and(|snapshot| snapshot.optional);

    if installability_skip(state, &resolved.pkg_key, resolved.metadata, optional)? {
        state.skipped.insert(reference);
        return Ok(None);
    }

    let dir = safe_join_modules_dir(modules, &dep.0.name)?;
    let dep_location = path_relative_to_lockfile_dir(&dir, state.lockfile_dir);
    // The previous install's record says the package is at this
    // directory, and the directory agrees. pnpm checks the disk too
    // ("there is no guarantee the modules manifest and current lockfile
    // were successfully saved after node_modules was changed"). A
    // directory (`file:`) dependency is never present: its source can
    // change without its version changing, so it is re-copied on every
    // install, as the isolated linker does for mutable sources.
    // The version to expect on disk is the recorded manifest version
    // when the lockfile carries one (tarball, git and other non-semver
    // dep paths), else the version in the dep path, as pnpm's
    // `nameVerFromPkgSnapshot` reads it.
    let expected_version = resolved
        .metadata
        .version
        .clone()
        .unwrap_or_else(|| resolved.pkg_key.suffix.version().to_string());
    // A patched package is not present either: the build phase applies
    // its patch, and applying a patch over an already patched directory
    // does not produce the same file, so it needs a fresh copy.
    let present = !state.opts.force
        && !matches!(resolved.metadata.resolution, LockfileResolution::Directory(_))
        && !reference.contains("(patch_hash=")
        && state.opts.current_hoisted_locations.is_some_and(|locations| {
            locations.get(&reference).is_some_and(|dirs| dirs.contains(&dep_location))
        })
        && !resolution_changed_at(state.prev_graph, &dir, &resolved.metadata.resolution)
        && package_present_at(&dir, &expected_version);

    // Insert *before* recursing (insert + push to `pkg_locations`, then
    // recurse) so every node's location is recorded ahead of any child
    // that needs to resolve to it. `children` is filled in by
    // `fill_children` after the whole walk is done.
    state.graph.insert(
        dir.clone(),
        graph_node(dep, &reference, &resolved, optional, present, &dir, modules),
    );
    state
        .pkg_locations_by_pkg_id
        .entry(pnpm_real_hoist::pkg_id(&resolved.pkg_key))
        .or_default()
        .push(dir.clone());

    // Directory resolutions are injected workspace packages. Record
    // every dir an injected dep lands in for the post-install re-mirror
    // step, so a future re-mirror pass has the input it needs.
    if let LockfileResolution::Directory(_) = &resolved.metadata.resolution {
        state.injection_targets_by_dep_path.entry(reference.clone()).or_default().push(dir.clone());
    }

    let hierarchy = walk_deps(state, &dir.join("node_modules"), &dep.0.dependencies.borrow())?;

    // `hoistedLocations` is pushed AFTER the recursion. The
    // pre-recursion sites that mutate state are for graph/index
    // identity; this one is the user-visible location list that the
    // linker consumes.
    state.hoisted_locations.entry(reference).or_default().push(dep_location);
    Ok(Some((dir, hierarchy)))
}

/// Workspace-kind hoister children are non-root workspace importers.
/// Recurse into their (post-hoist, often-empty) dependencies under
/// `<lockfile_dir>/<importer_id>/node_modules` to capture any deps the
/// hoister couldn't move up — those become nested entries in the
/// per-importer hierarchy. The workspace node itself is *not* added to
/// the graph or to the parent's hierarchy: it has no package contents
/// to import. Per-importer `direct_dependencies_by_importer_id` is
/// computed in [`WalkState::into_result`] from the lockfile (not from
/// the hoister tree) because hoisted siblings don't appear in the
/// workspace node's children.
fn walk_workspace_importer(
    state: &mut WalkState<'_>,
    dep: &RcByPtr<HoisterResult>,
    importer_id: &str,
) -> Result<(), HoistedDepGraphError> {
    let importer_root = state.lockfile_dir.join(importer_id);
    let importer_hierarchy =
        walk_deps(state, &importer_root.join("node_modules"), &dep.0.dependencies.borrow())?;
    state.per_importer_hierarchies.insert(importer_root, importer_hierarchy);
    // Reserve the importer's slot so [`WalkState::into_result`]'s
    // post-walk loop knows the importer was visited, even when it ends
    // up with zero direct deps.
    state.per_importer_direct_deps.entry(importer_id.to_string()).or_default();
    Ok(())
}

/// The lockfile's metadata and snapshot for a hoister reference. `None`
/// for a link / external placeholder the wrapper strips, which the
/// walker skips.
struct ResolvedReference<'l> {
    pkg_key: PackageKey,
    metadata: &'l pnpm_lockfile::PackageMetadata,
    snapshot: Option<&'l pnpm_lockfile::SnapshotEntry>,
}

fn resolve_reference<'l>(
    state: &WalkState<'l>,
    reference: &str,
) -> Result<Option<ResolvedReference<'l>>, HoistedDepGraphError> {
    let pkg_key: PackageKey = match reference.parse() {
        Ok(key) => key,
        Err(source) => {
            return Err(HoistedDepGraphError::BadReference {
                reference: reference.to_string(),
                source,
            });
        }
    };
    let Some(metadata) = lookup_package_metadata(state.lockfile, &pkg_key) else {
        return Ok(None);
    };
    let snapshot = state.lockfile.snapshots.as_ref().and_then(|snapshots| snapshots.get(&pkg_key));
    Ok(Some(ResolvedReference { pkg_key, metadata, snapshot }))
}

fn graph_node(
    dep: &RcByPtr<HoisterResult>,
    reference: &str,
    resolved: &ResolvedReference<'_>,
    optional: bool,
    present: bool,
    dir: &Path,
    modules: &Path,
) -> DependenciesGraphNode {
    DependenciesGraphNode {
        alias: Some(dep.0.name.clone()),
        dep_path: DepPath::from(reference.to_string()),
        // `pkgIdWithPatchHash` strips peer-graph hashes but keeps
        // `(patch_hash=...)`.
        pkg_id_with_patch_hash: PkgIdWithPatchHash::from(
            get_pkg_id_with_patch_hash(&resolved.pkg_key.to_string()).to_string(),
        ),
        dir: dir.to_path_buf(),
        modules: modules.to_path_buf(),
        children: BTreeMap::new(),
        name: resolved.pkg_key.name.to_string(),
        version: resolved.pkg_key.suffix.version().to_string(),
        optional,
        optional_dependencies: resolved
            .snapshot
            .and_then(|snap| snap.optional_dependencies.as_ref())
            .map(|map| map.keys().map(std::string::ToString::to_string).collect())
            .unwrap_or_default(),
        has_bin: resolved.metadata.has_bin.unwrap_or(false),
        has_bundled_dependencies: resolved.metadata.bundled_dependencies.is_some(),
        patch: None,
        resolution: resolved.metadata.resolution.clone(),
        present,
    }
}

/// Whether a previous install left this package at `dir`: a real
/// directory holding a regular `package.json` whose `version` is
/// `version`.
///
/// Mirrors pnpm's `dirHasPackageJsonWithVersion`, minus its fallback
/// that trusts a directory whose manifest cannot be read, so an
/// interrupted import is repaired rather than skipped. The link checks
/// keep the same promise: [`crate::import_indexed_dir()`] removes a
/// symlink standing where a package directory belongs, so one here is
/// not what a previous install left and the import has to replace it
/// rather than read a manifest through it.
fn package_present_at(dir: &Path, version: &str) -> bool {
    if !fs::symlink_metadata(dir).is_ok_and(|entry| entry.is_dir()) {
        return false;
    }
    let manifest_path = dir.join("package.json");
    if !fs::symlink_metadata(&manifest_path).is_ok_and(|entry| entry.is_file()) {
        return false;
    }
    let Ok(raw) = fs::read(&manifest_path) else {
        return false;
    };
    serde_json::from_slice::<serde_json::Value>(&raw).is_ok_and(|manifest| {
        manifest.get("version").and_then(serde_json::Value::as_str) == Some(version)
    })
}

/// Whether the current lockfile resolves the package at `dir`
/// differently from the wanted one.
///
/// A dep path carries the package's name and version, so the same key
/// can survive a change of tarball URL, integrity or revision, and the
/// manifest version on disk still matches. The contents are meant to
/// change, so the directory has to be imported again. `false` when
/// there is no previous graph to compare against, which is the
/// fresh-lockfile path: the recorded location and version stay the only
/// evidence there, as they are for pnpm's `skipFetch`.
fn resolution_changed_at(
    prev_graph: Option<&DependenciesGraph>,
    dir: &Path,
    wanted: &LockfileResolution,
) -> bool {
    prev_graph.is_some_and(|graph| graph.get(dir).is_some_and(|node| &node.resolution != wanted))
}

/// Whether the installability filter rules this package out on this
/// host. Applied only when `!opts.force`. An optional dep on an
/// unsupported platform is silently skipped; a required one is an
/// error.
fn installability_skip(
    state: &WalkState<'_>,
    pkg_key: &PackageKey,
    metadata: &pnpm_lockfile::PackageMetadata,
    optional: bool,
) -> Result<bool, HoistedDepGraphError> {
    if state.opts.force {
        return Ok(false);
    }
    let manifest = manifest_for_installability(pkg_key, metadata);
    let install_opts = InstallabilityOptions {
        engine_strict: state.opts.engine_strict,
        optional,
        current_node_version: &state.opts.current_node_version,
        pnpm_version: None,
        current_os: &state.opts.current_os,
        current_cpu: &state.opts.current_cpu,
        current_libc: &state.opts.current_libc,
        supported_architectures: state.opts.supported_architectures.as_ref(),
    };
    match package_is_installable(&pkg_key.to_string(), &manifest, &install_opts) {
        Ok(
            InstallabilityVerdict::Installable | InstallabilityVerdict::ProceedWithWarning { .. },
        ) => Ok(false),
        Ok(InstallabilityVerdict::SkipOptional { .. }) => Ok(true),
        Err(source) => Err(HoistedDepGraphError::Installability(source)),
    }
}

/// Look up the metadata side of a snapshot. Pacquet stores
/// `packages` and `snapshots` separately; the walker needs the
/// metadata for resolution / `has_bin` / bundledDependencies.
fn lookup_package_metadata<'a>(
    lockfile: &'a Lockfile,
    key: &PackageKey,
) -> Option<&'a pnpm_lockfile::PackageMetadata> {
    let packages = lockfile.packages.as_ref()?;
    // `packages:` keys are peer-stripped (`react-dom@19.2.7`), while a
    // hoister reference carries the full peer suffix
    // (`react-dom@19.2.7(react@19.2.7)`). Try the exact key first
    // (peerless references — the common case — hit immediately), then
    // fall back to the stripped key so peered snapshots resolve their
    // metadata instead of being silently dropped from the graph along
    // with their whole subtree.
    packages.get(key).or_else(|| {
        if key.suffix.peer().is_empty() {
            return None;
        }
        packages.get(&key.without_peer())
    })
}

/// Project the platform / engines axes from a `PackageMetadata`
/// onto the [`PackageInstallabilityManifest`] shape
/// [`package_is_installable`] consumes. Extracted into its own
/// helper so the walker body stays small.
fn manifest_for_installability(
    pkg_key: &PackageKey,
    metadata: &pnpm_lockfile::PackageMetadata,
) -> PackageInstallabilityManifest {
    let engines = metadata.engines.as_ref().map(|engines| WantedEngine {
        node: engines.get("node").cloned(),
        pnpm: engines.get("pnpm").cloned(),
    });
    PackageInstallabilityManifest {
        name: pkg_key.name.to_string(),
        engines,
        cpu: metadata.cpu.clone(),
        os: metadata.os.clone(),
        libc: metadata.libc.as_deref().map(<[String]>::to_vec),
    }
}

/// Lockfile-relative path string (`dir` relative to `lockfile_dir`).
/// Returns an empty string when `dir == lockfile_dir`.
///
/// Backslashes are normalized to forward slashes so the value is
/// portable across platforms — `.modules.yaml.hoistedLocations`
/// is read on whatever OS the next install runs on, and
/// `pnpm-lock.yaml` already uses forward slashes for the same
/// reason. pacquet normalizes here for cross-platform consistency
/// with the rest of pnpm's serialised formats.
fn path_relative_to_lockfile_dir(dir: &Path, lockfile_dir: &Path) -> String {
    dir.strip_prefix(lockfile_dir).map_or_else(
        |_| dir.to_string_lossy().replace('\\', "/"),
        |rel| rel.to_string_lossy().replace('\\', "/"),
    )
}

/// Compute the `children: alias → dir` map for a node: look up
/// every direct (and optional, with `include` always on here) dep
/// of the snapshot, resolve it to its snapshot key via
/// `SnapshotDepRef::resolve`, and take the first recorded
/// location.
fn compute_children(
    snapshot: Option<&pnpm_lockfile::SnapshotEntry>,
    pkg_locations: &BTreeMap<String, Vec<PathBuf>>,
) -> BTreeMap<String, PathBuf> {
    let mut children: BTreeMap<String, PathBuf> = BTreeMap::new();
    let Some(snapshot) = snapshot else { return children };

    let dep_iter = snapshot
        .dependencies
        .iter()
        .flatten()
        .chain(snapshot.optional_dependencies.iter().flatten());
    for (alias_name, dep_ref) in dep_iter {
        // `link:` deps return `None` here — they live outside the
        // virtual store and don't show up in `pkg_locations`.
        let Some(child_key) = dep_ref.resolve(alias_name) else {
            continue;
        };
        if let Some(locations) = pkg_locations.get(&pnpm_real_hoist::pkg_id(&child_key))
            && let Some(first) = locations.first()
        {
            children.insert(alias_name.to_string(), first.clone());
        }
    }
    children
}

#[cfg(test)]
mod tests;
