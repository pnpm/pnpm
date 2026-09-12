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

mod presence;
use presence::{
    installability_skip, lookup_package_metadata, package_present_at,
    path_relative_to_lockfile_dir, resolution_changed_at,
};

mod walk;
use walk::{WalkState, walk_deps};

use crate::{HoistedLocations, safe_join_modules_dir::InvalidDependencyAliasError};
use derive_more::{Display, Error, From};
use miette::Diagnostic;
use pnpm_lockfile::{Lockfile, LockfileResolution, ParsePkgNameVerPeerError, PkgIdWithPatchHash};
use pnpm_modules_yaml::DepPath;
use pnpm_package_is_installable::{InstallabilityError, SupportedArchitectures};
use pnpm_patching::PatchInfo;
use pnpm_real_hoist::{HoistError, HoistOpts, hoist};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
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

#[cfg(test)]
mod tests;
