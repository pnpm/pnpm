//! Type skeleton for the directory-keyed dependency graph that
//! `nodeLinker: hoisted` installs produce. Ports the data shapes
//! from upstream's
//! [`installing/deps-restorer/src/lockfileToHoistedDepGraph.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/lockfileToHoistedDepGraph.ts)
//! and the supporting types factored into
//! [`deps/graph-builder/src/lockfileToDepGraph.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-builder/src/lockfileToDepGraph.ts).
//!
//! The walker [`lockfile_to_hoisted_dep_graph`] takes a wanted
//! lockfile plus an optional *current* lockfile and runs
//! `pacquet_real_hoist::hoist` to get the directory shape, then
//! assembles a [`LockfileToDepGraphResult`] keyed by the computed
//! absolute directory of every node. Optional packages whose
//! `cpu` / `os` / `libc` / `engines` don't fit the current host
//! are added to `result.skipped` rather than emitted into the
//! graph; required incompatible packages proceed with a
//! (yet-to-be-wired) warning, matching upstream
//! `package_is_installable`'s `null | true | false` shape. When a
//! current lockfile is supplied, the walker runs a second
//! `force: true, skipped: empty` pass over it to populate
//! `result.prev_graph` — the input Slice 5's linker diffs against
//! to identify orphaned directories. Store I/O (`fetching` /
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

use derive_more::{Display, Error, From};
use indexmap::IndexSet;
use miette::Diagnostic;
use pacquet_deps_path::get_pkg_id_with_patch_hash;
use pacquet_lockfile::{
    Lockfile, LockfileResolution, PackageKey, ParsePkgNameVerPeerError, PkgIdWithPatchHash,
};
use pacquet_modules_yaml::DepPath;
use pacquet_package_is_installable::{
    InstallabilityError, InstallabilityOptions, InstallabilityVerdict,
    PackageInstallabilityManifest, SupportedArchitectures, WantedEngine, package_is_installable,
};
use pacquet_patching::PatchInfo;
use pacquet_real_hoist::{HoistError, HoistOpts, HoisterResult, RcByPtr, hoist};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// One node in a hoisted-linker dependency graph. Keyed in the
/// outer [`DependenciesGraph`] by the node's absolute `dir`.
///
/// Mirrors upstream's
/// [`DependenciesGraphNode`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-builder/src/lockfileToDepGraph.ts#L38)
/// minus the store-controller-bound fields (`fetching`,
/// `files_index_file`) that the walker only learns about once it
/// calls `storeController.fetchPackage`. Those land in the
/// follow-up sub-slice that wires the store in; today, this type
/// pins the shape of every other field so the walker can fill
/// them without churning the call sites.
#[derive(Debug, Clone, PartialEq)]
pub struct DependenciesGraphNode {
    /// The alias this node was placed under in its parent's
    /// `node_modules`. Optional for parity with upstream — only
    /// populated when the node is reached via the hoist walk;
    /// upstream marks it `?` for the same reason.
    pub alias: Option<String>,
    /// The depPath that produced this node, used as the key for
    /// `hoistedLocations` and the join key for `hoistedDependencies`.
    pub dep_path: DepPath,
    /// Upstream's `pkgIdWithPatchHash`: the patch-aware ident key
    /// the side-effects cache uses. Ported as
    /// [`pacquet_lockfile::PkgIdWithPatchHash`] — a non-validating
    /// branded newtype around `String` matching upstream's
    /// `string & { __brand: 'PkgIdWithPatchHash' }`.
    pub pkg_id_with_patch_hash: PkgIdWithPatchHash,
    /// Absolute path of the package's directory on disk. The
    /// outer [`DependenciesGraph`]'s key is this same value;
    /// upstream stores it on the node too so consumers don't need
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
}

/// Directory-keyed graph of every hoisted-linker node the walker
/// emitted. Mirrors upstream's
/// [`DependenciesGraph`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-builder/src/lockfileToDepGraph.ts#L60-L62).
pub type DependenciesGraph = BTreeMap<PathBuf, DependenciesGraphNode>;

/// Recursive directory hierarchy: each `node_modules` directory
/// maps to its children, which in turn map to their own
/// children's `node_modules`. The linker walks this to know which
/// directories to populate (and in what order) and which
/// `<dir>/node_modules/.bin` to wire up. Mirrors upstream's
/// [`DepHierarchy`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-builder/src/lockfileToDepGraph.ts#L98).
///
/// Wrapped in a newtype rather than typedef'd to a recursive
/// `BTreeMap` because Rust doesn't allow recursive type aliases.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DepHierarchy(pub BTreeMap<PathBuf, DepHierarchy>);

/// Per-importer alias → direct-dependency directory. For the
/// single-importer case the only key is `"."`; workspace support
/// will add per-importer entries keyed by the importer's
/// project id. Mirrors upstream's
/// [`DirectDependenciesByImporterId`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-builder/src/lockfileToDepGraph.ts#L94-L96).
pub type DirectDependenciesByImporterId = BTreeMap<String, BTreeMap<String, PathBuf>>;

/// Everything the walker hands back to the install pipeline.
///
/// Mirrors upstream's
/// [`LockfileToDepGraphResult`](https://github.com/pnpm/pnpm/blob/94240bc046/deps/graph-builder/src/lockfileToDepGraph.ts#L100-L108).
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
    /// [`pacquet_modules_yaml::Modules::hoisted_locations`].
    ///
    /// Upstream literally types the values as `Record<string,
    /// string[]>` (not `Record<DepPath, string[]>`), even though
    /// the strings are populated from depPaths internally —
    /// mirrored here to keep the on-disk shape identical. The
    /// same choice was made for the `Modules` schema field this
    /// round-trips through (see its doc-comment in
    /// `pacquet-modules-yaml`).
    pub hoisted_locations: BTreeMap<String, Vec<String>>,
    pub symlinked_direct_dependencies_by_importer_id: DirectDependenciesByImporterId,
    /// Diffed against `graph` by the linker's orphan-removal pass
    /// to know which directories the previous install owned that
    /// the new install does not. `None` on a fresh install (no
    /// prior lockfile).
    pub prev_graph: Option<DependenciesGraph>,
    /// Per-depPath list of directories where the package is
    /// expected to live as an *injected* workspace package. Used
    /// by the post-install re-mirror step. Upstream is
    /// `Map<string, string[]>` (keys typed as raw `string`, not
    /// `DepPath`); mirrored here. See `injectionTargetsByDepPath`
    /// at
    /// [lockfileToHoistedDepGraph.ts:286](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/lockfileToHoistedDepGraph.ts#L286-L292).
    pub injection_targets_by_dep_path: BTreeMap<String, Vec<PathBuf>>,
    /// Packages the walker decided to skip — the input
    /// `opts.skipped` extended with any depPaths whose
    /// installability check failed (optional + unsupported
    /// platform/engine). Upstream mutates the input `Set<string>`
    /// in place; pacquet returns the augmented set on the result
    /// so the caller can persist it into `.modules.yaml.skipped`
    /// without sharing mutable state.
    pub skipped: BTreeSet<String>,
}

/// Inputs the walker reads from. Mirrors the subset of upstream's
/// [`LockfileToHoistedDepGraphOptions`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/lockfileToHoistedDepGraph.ts#L34-L63)
/// pacquet needs for the hoisted-linker path that's actually
/// implemented today. Fields tied to the still-unported store
/// controller, fetch concurrency, or workspace project list will
/// be added when their consumers land.
#[derive(Debug, Clone)]
pub struct LockfileToHoistedDepGraphOptions {
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
    /// the snapshot. Cloned + extended on the way out. Upstream's
    /// `LockfileToHoistedDepGraphOptions.skipped` is `Set<string>`
    /// (note: `Set<DepPath>` in the isolated-graph builder's
    /// options — pacquet matches the hoisted-specific typing
    /// here), so the wrapper here is `BTreeSet<String>`.
    pub skipped: BTreeSet<String>,
    /// When true, suppress the installability check and emit every
    /// dep into the graph regardless of cpu / os / libc / engines.
    /// Used by the `prev_graph` walk (Slice 4d) where the previous
    /// lockfile is replayed wholesale to compute orphans — upstream
    /// passes `force: true, skipped: new Set()` to that call so
    /// the diff catches packages that previously installed but
    /// would now be filtered. Mirrors upstream's `force` at
    /// [lockfileToHoistedDepGraph.ts:73](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/lockfileToHoistedDepGraph.ts#L73-L76).
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
    /// Mirrors [`pacquet_real_hoist::HoistOpts::hoist_workspace_packages`].
    /// When `true` (the default), every non-root workspace importer
    /// becomes a `Workspace`-kind child of the virtual `.` root in
    /// the hoist tree, and the walker emits per-importer subtrees
    /// under `<lockfile_dir>/<importer_id>/node_modules`. When
    /// `false`, only the root importer's subtree is emitted (the
    /// hoister also skips adding the workspace children to its
    /// shared tree). Pacquet's `Config::hoist_workspace_packages`
    /// (in `pacquet-config`) drives this from the install pipeline.
    pub hoist_workspace_packages: bool,

    /// Per-importer block-list passed straight through to
    /// [`pacquet_real_hoist::HoistOpts::hoisting_limits`]. See the
    /// hoister's doc-comment for the locator-keyed shape and
    /// `Config::hoisting_limits` in `pacquet-config` for how the
    /// install pipeline derives this from `pnpm-workspace.yaml`.
    pub hoisting_limits: pacquet_real_hoist::HoistingLimits,

    /// Reserved-name list passed straight through to
    /// [`pacquet_real_hoist::HoistOpts::external_dependencies`].
    /// See the hoister's doc-comment for the strip semantics and
    /// `Config::external_dependencies` in `pacquet-config` for how
    /// the install pipeline derives this from
    /// `pnpm-workspace.yaml`.
    pub external_dependencies: BTreeSet<String>,
}

impl Default for LockfileToHoistedDepGraphOptions {
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
            hoisting_limits: pacquet_real_hoist::HoistingLimits::new(),
            external_dependencies: BTreeSet::new(),
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
    /// see the same error code as upstream.
    #[display("{_0}")]
    Hoist(#[error(source)] HoistError),
    /// A `HoisterResult` node carried a reference string that
    /// doesn't parse as a `name@version[(peers)]` package key.
    /// Should never happen for hoister output produced from a
    /// valid lockfile — the hoister only emits references it
    /// already validated — but the conversion is fallible at the
    /// type level, so a typed error is the honest surface.
    #[display("Unparsable snapshot reference {reference:?} on hoisted node")]
    #[diagnostic(code(ERR_PACQUET_HOISTED_GRAPH_BAD_REFERENCE))]
    BadReference {
        reference: String,
        #[error(source)]
        source: ParsePkgNameVerPeerError,
    },
    /// A required (non-optional) package failed the
    /// installability check. Mirrors upstream's `throw` path
    /// where `engineStrict` + an engine mismatch surfaces as
    /// `ERR_PNPM_UNSUPPORTED_ENGINE`; the inner
    /// `InstallabilityError` is propagated transparently so
    /// callers see the same diagnostic code
    /// (`ERR_PNPM_UNSUPPORTED_ENGINE` /
    /// `ERR_PNPM_UNSUPPORTED_PLATFORM` /
    /// `ERR_PNPM_INVALID_NODE_VERSION`) as upstream, and the
    /// inner error already carries the package id for context.
    ///
    /// Optional packages on incompatible platforms do *not* take
    /// this path — they are added to `result.skipped` and
    /// silently skipped, matching upstream's
    /// `pnpm:skipped-optional-dependency` semantics.
    #[display("{_0}")]
    #[diagnostic(transparent)]
    Installability(#[error(source)] Box<InstallabilityError>),
}

/// Build a directory-keyed [`LockfileToDepGraphResult`] from a
/// wanted lockfile, plus an optional *current* lockfile to diff
/// against. Ports upstream's
/// [`lockfileToHoistedDepGraph`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/lockfileToHoistedDepGraph.ts#L65-L85).
///
/// When `current_lockfile` is `Some` and the lockfile has a
/// non-empty `packages:` map, the function runs an extra pass
/// over that lockfile with `force: true` and `skipped: empty` to
/// produce `prev_graph` — the graph the previous install
/// produced, used by Slice 5's linker to identify orphaned
/// directories. Mirrors upstream's pre-await branch at
/// [`lockfileToHoistedDepGraph.ts:70-79`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/lockfileToHoistedDepGraph.ts#L70-L79).
///
/// The store-controller-bound `fetching` / `files_index_file`
/// fields on each graph node remain default-valued — those are
/// populated by Slice 5's linker, which kicks off the actual
/// store fetches when it has a real consumer for the handles.
///
/// Multi-importer (workspace) lockfiles are supported: the hoister
/// ([`pacquet_real_hoist::hoist`]) attaches each non-root importer as
/// a workspace child of the virtual `.` root when
/// `hoist_workspace_packages` is enabled. Per-importer hoisting roots
/// (upstream's multi-level output shape) are not modelled yet.
pub fn lockfile_to_hoisted_dep_graph(
    lockfile: &Lockfile,
    current_lockfile: Option<&Lockfile>,
    opts: &LockfileToHoistedDepGraphOptions,
) -> Result<LockfileToDepGraphResult, HoistedDepGraphError> {
    // Prev-graph walk: forced (every snapshot in the current
    // lockfile must surface so the diff catches packages that
    // would now fail installability) and unskipped (the previous
    // install's `skipped` is irrelevant — we want the full
    // previous layout to compute orphans against).
    let prev_graph = match current_lockfile {
        // Require a non-empty `packages` map. Upstream's
        // `currentLockfile?.packages != null` guard only filters
        // out `null` / `undefined` — but for an empty `packages:
        // {}` the inner walk produces an empty graph too, which
        // is observationally equivalent to "no orphans to
        // consider". Pacquet collapses both null and empty into
        // `prev_graph: None` so the API contract is unambiguous
        // and the empty case skips the (no-op) second walk.
        Some(current) if current.packages.as_ref().is_some_and(|packages| !packages.is_empty()) => {
            let prev_opts = LockfileToHoistedDepGraphOptions {
                force: true,
                skipped: BTreeSet::new(),
                ..opts.clone()
            };
            Some(build_dep_graph(current, &prev_opts)?.graph)
        }
        _ => None,
    };

    let mut result = build_dep_graph(lockfile, opts)?;
    result.prev_graph = prev_graph;
    Ok(result)
}

/// Inner builder: runs the hoister + walker for one lockfile and
/// returns the per-walk subset of [`LockfileToDepGraphResult`]
/// (everything except `prev_graph`, which only the outer wrapper
/// sets). Mirrors upstream's private `_lockfileToHoistedDepGraph`
/// at [`lockfileToHoistedDepGraph.ts:91-127`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/lockfileToHoistedDepGraph.ts#L91-L127).
fn build_dep_graph(
    lockfile: &Lockfile,
    opts: &LockfileToHoistedDepGraphOptions,
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
        skipped: opts.skipped.clone(),
        graph: DependenciesGraph::new(),
        pkg_locations_by_dep_path: BTreeMap::new(),
        hoisted_locations: BTreeMap::new(),
        injection_targets_by_dep_path: BTreeMap::new(),
        per_importer_hierarchies: BTreeMap::new(),
        per_importer_direct_deps: BTreeMap::new(),
    };
    let root_deps = hoister_result.dependencies.borrow();
    let root_hierarchy = walk_deps(&mut state, &modules_dir, &root_deps)?;
    drop(root_deps);

    // Pass 2 — fill in each node's `children` map from the
    // now-complete `pkg_locations_by_dep_path`. Mirrors upstream's
    // post-await `graph[dir].children = getChildren(...)` line.
    //
    // The walk above intentionally leaves `children` empty: in
    // upstream's parallel-async walker, every sibling and
    // descendant of a node has its directory pushed to
    // `pkgLocationsByDepPath` during the sync prologue of its
    // `async (dep) => { ... }` body, *before* any continuation
    // (the post-recursion `getChildren` call) runs. So by the
    // time any node computes its children, the location index is
    // already complete. Pacquet runs synchronously, so the
    // simplest way to preserve that invariant is to insert
    // everything first and resolve children second.
    let WalkState {
        graph,
        pkg_locations_by_dep_path,
        hoisted_locations,
        injection_targets_by_dep_path,
        skipped,
        lockfile,
        per_importer_hierarchies,
        per_importer_direct_deps,
        ..
    } = state;
    let mut graph = graph;
    fill_children(&mut graph, &pkg_locations_by_dep_path, lockfile)?;

    // The hoister produced a children order; the directory keys in
    // `root_hierarchy` follow it. `direct_dependencies_by_importer_id["."]`
    // mirrors upstream's `directDepsMap` at
    // <https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/lockfileToHoistedDepGraph.ts#L139-L145>.
    let mut direct_deps_root: BTreeMap<String, PathBuf> = BTreeMap::new();
    for child_dir in root_hierarchy.0.keys() {
        if let Some(alias) = graph.get(child_dir).and_then(|node| node.alias.as_deref()) {
            direct_deps_root.insert(alias.to_string(), child_dir.clone());
        }
    }
    let mut direct_dependencies_by_importer_id: DirectDependenciesByImporterId = BTreeMap::new();
    direct_dependencies_by_importer_id
        .insert(Lockfile::ROOT_IMPORTER_KEY.to_string(), direct_deps_root);

    // Per-non-root importer direct deps: iterate each importer's
    // declared lockfile entries and look up the resolved depPath
    // in `pkg_locations_by_dep_path`. The first recorded location
    // wins (matches upstream's `pkgLocationsByDepPath[depPath][0]`
    // pick at
    // [`lockfileToHoistedDepGraph.ts:148`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/lockfileToHoistedDepGraph.ts#L148)).
    // We can't read this off the workspace node's tree-children
    // because the hoister moves dedupe-able deps up to root, leaving
    // the workspace node's children empty even when the importer
    // *declared* those deps.
    //
    // `link:` entries are skipped — they don't enter the hoist tree
    // and have no `pkg_locations` entry. The install pipeline
    // handles them via [`crate::SymlinkDirectDependencies`]'s
    // `link_only` pass after the hoisted linker runs.
    for importer_id in per_importer_direct_deps.keys() {
        let Some(importer) = lockfile.importers.get(importer_id) else { continue };
        let mut direct_deps: BTreeMap<String, PathBuf> = BTreeMap::new();
        for dep_map in [
            importer.dependencies.as_ref(),
            importer.dev_dependencies.as_ref(),
            importer.optional_dependencies.as_ref(),
        ] {
            let Some(dep_map) = dep_map else { continue };
            for (alias, spec) in dep_map {
                // For an aliased dep the snapshot key uses the
                // alias's own (name, suffix); for a regular dep it's
                // `(alias, version)`. `link:` deps are skipped — they
                // don't live in the virtual store.
                let Some(dep_key) = spec.version.resolved_key(alias) else { continue };
                let dep_path = dep_key.to_string();
                if let Some(locations) = pkg_locations_by_dep_path.get(&dep_path)
                    && let Some(first) = locations.first()
                {
                    direct_deps.insert(alias.to_string(), first.clone());
                }
            }
        }
        direct_dependencies_by_importer_id.insert(importer_id.clone(), direct_deps);
    }

    // Hierarchy: one entry per importer root. Root importer gets
    // `lockfile_dir`; non-root workspace importers get
    // `<lockfile_dir>/<importer_id>`. Mirrors upstream's
    // `hierarchy` shape at
    // [`lockfileToHoistedDepGraph.ts:170-180`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/lockfileToHoistedDepGraph.ts#L170-L180)
    // — the linker walks each importer's subtree under its own
    // root.
    let mut hierarchy = BTreeMap::new();
    hierarchy.insert(opts.lockfile_dir.clone(), root_hierarchy);
    hierarchy.extend(per_importer_hierarchies);

    Ok(LockfileToDepGraphResult {
        graph,
        direct_dependencies_by_importer_id,
        hierarchy,
        hoisted_locations,
        symlinked_direct_dependencies_by_importer_id: DirectDependenciesByImporterId::new(),
        prev_graph: None,
        injection_targets_by_dep_path,
        skipped,
    })
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
    opts: &'a LockfileToHoistedDepGraphOptions,
    skipped: BTreeSet<String>,
    graph: DependenciesGraph,
    /// Records every directory each depPath landed in, in visit
    /// order. The first entry wins for parent → child wiring (see
    /// upstream `getChildren`).
    pkg_locations_by_dep_path: BTreeMap<String, Vec<PathBuf>>,
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

/// Recursive walker over `HoisterResult.dependencies`. Mirrors
/// upstream's
/// [`fetchDeps`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/lockfileToHoistedDepGraph.ts#L168-L296)
/// minus the store-fetch / installability path; here the walker
/// only computes node identity, location, children, and
/// hoisted-location records.
///
/// No cycle detection — matches upstream's recursion shape and
/// trusts the hoister to produce a DAG. The hoister's own
/// cyclic-input tests pin that property.
fn walk_deps(
    state: &mut WalkState<'_>,
    modules: &Path,
    deps: &IndexSet<RcByPtr<HoisterResult>>,
) -> Result<DepHierarchy, HoistedDepGraphError> {
    let mut hierarchy: BTreeMap<PathBuf, DepHierarchy> = BTreeMap::new();
    for dep in deps {
        // The hoister keeps every absorbed reference; the first
        // (alphabetically smallest) is the canonical depPath for
        // this node's location. Mirrors upstream's
        // `Array.from(dep.references)[0]`.
        let Some(reference) = dep.0.references.borrow().iter().next().cloned() else {
            continue;
        };

        if state.skipped.contains(&reference) {
            continue;
        }

        // Workspace-kind hoister children are non-root workspace
        // importers. Recurse into their (post-hoist, often-empty)
        // dependencies under `<lockfile_dir>/<importer_id>/node_modules`
        // to capture any deps the hoister couldn't move up — those
        // become nested entries in the per-importer hierarchy. The
        // workspace node itself is *not* added to the graph or to
        // the parent's hierarchy: it has no package contents to
        // import. Per-importer `direct_dependencies_by_importer_id`
        // is computed in [`build_dep_graph`] from the lockfile
        // (not from the hoister tree) because hoisted siblings
        // don't appear in the workspace node's children. Mirrors
        // upstream's per-importer fan-out at
        // [`lockfileToHoistedDepGraph.ts:113-180`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/lockfileToHoistedDepGraph.ts#L113-L180).
        if let Some(importer_id) = reference.strip_prefix("workspace:") {
            let importer_id = importer_id.to_string();
            let importer_root = state.lockfile_dir.join(&importer_id);
            let importer_modules = importer_root.join("node_modules");
            let child_deps = dep.0.dependencies.borrow();
            let importer_hierarchy = walk_deps(state, &importer_modules, &child_deps)?;
            drop(child_deps);
            state.per_importer_hierarchies.insert(importer_root, importer_hierarchy);
            // Reserve the importer's slot so [`build_dep_graph`]'s
            // post-walk loop knows the importer was visited, even
            // when it ends up with zero direct deps.
            state.per_importer_direct_deps.entry(importer_id).or_default();
            continue;
        }

        let pkg_key: PackageKey = match reference.parse() {
            Ok(key) => key,
            Err(source) => {
                return Err(HoistedDepGraphError::BadReference { reference, source });
            }
        };

        // `packages[key]` is the metadata source; absent → this is
        // a link / external placeholder that the wrapper strips,
        // and the walker mirrors upstream's `if (!pkgSnapshot) return`
        // by skipping.
        let Some(metadata) = lookup_package_metadata(state.lockfile, &pkg_key) else {
            continue;
        };
        let snapshot =
            state.lockfile.snapshots.as_ref().and_then(|snapshots| snapshots.get(&pkg_key));

        // Installability filter. Mirrors upstream's
        // `if (!opts.force && packageIsInstallable(...) === false)`
        // at [lockfileToHoistedDepGraph.ts:200](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/lockfileToHoistedDepGraph.ts#L200-L210).
        // `optional` comes from the snapshot — an optional dep on
        // an unsupported platform is silently added to `skipped`;
        // a required dep takes the error path.
        if !state.opts.force {
            let manifest = manifest_for_installability(&pkg_key, metadata);
            let optional = snapshot.is_some_and(|s| s.optional);
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
                    InstallabilityVerdict::Installable
                    | InstallabilityVerdict::ProceedWithWarning { .. },
                ) => {}
                Ok(InstallabilityVerdict::SkipOptional { .. }) => {
                    state.skipped.insert(reference.clone());
                    continue;
                }
                Err(source) => {
                    return Err(HoistedDepGraphError::Installability(source));
                }
            }
        }

        let dir = modules.join(&dep.0.name);
        let dep_location = path_relative_to_lockfile_dir(&dir, state.lockfile_dir);

        // Insert *before* recursing — mirrors upstream's
        // `fetchDeps` body order (insert + push to pkgLocations,
        // then `await fetchDeps(...)`). `children` is filled in
        // by `fill_children` after the whole walk is done.
        let node = DependenciesGraphNode {
            alias: Some(dep.0.name.clone()),
            dep_path: DepPath::from(reference.clone()),
            // `pkgIdWithPatchHash` strips peer-graph hashes but
            // keeps `(patch_hash=...)`. Mirrors upstream's
            // [`getPkgIdWithPatchHash`](https://github.com/pnpm/pnpm/blob/cc4ff817aa/deps/path/src/index.ts#L63-L70).
            pkg_id_with_patch_hash: PkgIdWithPatchHash::from(
                get_pkg_id_with_patch_hash(&pkg_key.to_string()).to_string(),
            ),
            dir: dir.clone(),
            modules: modules.to_path_buf(),
            children: BTreeMap::new(),
            name: pkg_key.name.to_string(),
            version: pkg_key.suffix.version().to_string(),
            optional: snapshot.is_some_and(|s| s.optional),
            optional_dependencies: snapshot
                .and_then(|snap| snap.optional_dependencies.as_ref())
                .map(|map| map.keys().map(std::string::ToString::to_string).collect())
                .unwrap_or_default(),
            has_bin: metadata.has_bin.unwrap_or(false),
            has_bundled_dependencies: metadata.bundled_dependencies.is_some(),
            patch: None,
            resolution: metadata.resolution.clone(),
        };

        state.graph.insert(dir.clone(), node);
        state.pkg_locations_by_dep_path.entry(reference.clone()).or_default().push(dir.clone());

        // Directory resolutions are injected workspace packages.
        // Upstream records every dir an injected dep lands in for
        // the post-install re-mirror step; mirrored here so a
        // future re-mirror pass has the same input shape.
        if let LockfileResolution::Directory(_) = &metadata.resolution {
            state
                .injection_targets_by_dep_path
                .entry(reference.clone())
                .or_default()
                .push(dir.clone());
        }

        // Recurse into the children (records their pkg_locations
        // and produces their `DepHierarchy`).
        let inner_modules = dir.join("node_modules");
        let child_deps = dep.0.dependencies.borrow();
        let inner_hierarchy = walk_deps(state, &inner_modules, &child_deps)?;
        drop(child_deps);

        // `hoistedLocations` is pushed AFTER the recursion, matching
        // upstream. The pre-recursion sites that mutate state are
        // for graph/index identity; this one is the user-visible
        // location list that the linker consumes.
        state.hoisted_locations.entry(reference).or_default().push(dep_location);
        hierarchy.insert(dir, inner_hierarchy);
    }
    Ok(DepHierarchy(hierarchy))
}

/// Look up the metadata side of a snapshot. Pacquet stores
/// `packages` and `snapshots` separately; the walker needs the
/// metadata for resolution / `has_bin` / bundledDependencies (which
/// upstream pulls from `pkgSnapshot`).
fn lookup_package_metadata<'a>(
    lockfile: &'a Lockfile,
    key: &PackageKey,
) -> Option<&'a pacquet_lockfile::PackageMetadata> {
    lockfile.packages.as_ref()?.get(key)
}

/// Project the platform / engines axes from a `PackageMetadata`
/// onto the [`PackageInstallabilityManifest`] shape
/// [`package_is_installable`] consumes. Upstream builds this
/// inline at
/// [lockfileToHoistedDepGraph.ts:192-199](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/lockfileToHoistedDepGraph.ts#L192-L199);
/// extracted here so the walker body stays small.
fn manifest_for_installability(
    pkg_key: &PackageKey,
    metadata: &pacquet_lockfile::PackageMetadata,
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
        libc: metadata.libc.clone(),
    }
}

/// Lockfile-relative path string, matching upstream's
/// `path.relative(lockfileDir, dir)`. Returns an empty string when
/// `dir == lockfile_dir`.
///
/// Backslashes are normalized to forward slashes so the value is
/// portable across platforms — `.modules.yaml.hoistedLocations`
/// is read on whatever OS the next install runs on, and pnpm's
/// `pnpm-lock.yaml` already uses forward slashes for the same
/// reason. Upstream's `path.relative` produces OS-native
/// separators (so `.modules.yaml` written on Windows technically
/// holds backslashes), but pacquet normalizes here for
/// cross-platform consistency with the rest of pnpm's serialised
/// formats.
fn path_relative_to_lockfile_dir(dir: &Path, lockfile_dir: &Path) -> String {
    dir.strip_prefix(lockfile_dir).map_or_else(
        |_| dir.to_string_lossy().replace('\\', "/"),
        |rel| rel.to_string_lossy().replace('\\', "/"),
    )
}

/// Compute the `children: alias → dir` map for a node. Mirrors
/// upstream's
/// [`getChildren`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/lockfileToHoistedDepGraph.ts#L320-L334):
/// look up every direct (and optional, with `include` always on
/// here) dep of the snapshot, resolve it to its depPath via
/// `SnapshotDepRef::resolve`, and take the first recorded
/// location.
fn compute_children(
    snapshot: Option<&pacquet_lockfile::SnapshotEntry>,
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
        // Mirrors upstream's `if (childDepPath && pkgLocations...)`
        // guard in `getChildren`.
        let Some(child_key) = dep_ref.resolve(alias_name) else {
            continue;
        };
        let child_dep_path = child_key.to_string();
        if let Some(locations) = pkg_locations.get(&child_dep_path)
            && let Some(first) = locations.first()
        {
            children.insert(alias_name.to_string(), first.clone());
        }
    }
    children
}

#[cfg(test)]
mod tests;
