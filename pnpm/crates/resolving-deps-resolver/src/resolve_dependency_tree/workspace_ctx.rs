//! The workspace-shared half of the walk: [`WorkspaceTreeCtx`], the
//! per-`pkgIdWithPatchHash` dedup maps every importer's walk
//! contributes to, and the children-ownership bookkeeping that settles
//! which occurrence of a package records its children.

pub(super) use children_ownership::{
    ChildrenOwner, ChildrenOwnerClaim, RecordedChildren, RecordedChildrenContext,
    claim_children_owner, claim_children_warmup, insert_tree_node, is_current_children_owner,
    lazy_children, make_non_owner_nodes_lazy, record_children, recorded_children_match,
    register_peer_dep_names, remember_node_parent_ids,
};

pub(super) use cache_keys::{
    PathKey, SharedWorkspaceWantedKey, WantedKey, WorkspaceFinalWantedKey,
    WorkspaceResolutionOptionsKey,
};

mod reachable_nodes;
use reachable_nodes::{
    collect_newly_visited, fold_version, fold_visited_versions, merge_synced_child_spec,
    walk_reachable_children, walk_reachable_nodes,
};

mod version_snapshot;

mod tree_sync;

mod children_ownership;
use children_ownership::ChildrenOwnerEntry;

mod cache_keys;

use chrono::{DateTime, Utc};
use pnpm_hooks::PnpmfileHooks;
use pnpm_lockfile::{PkgName, PkgNameVerPeer, RegistryContext};
use pnpm_resolving_resolver_base::{PkgResolutionId, ResolveOptions, WorkspacePackages};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
};

use crate::{
    node_id::NodeId,
    resolve_peers::MissingNames,
    resolved_tree::{
        AncestorIds, DependenciesTreeNode, DirectDep, PeerDep, ResolvedPackage, ResolvedTree,
    },
};

use super::{
    DeprecationLogFn, FinalizedPackageFn, ManifestHook, SkippedOptionalLogFn, UpdateDepth,
    UpdateReuseScope, lock_recoverable, tree_ctx::TreeCtx,
};

type SubtreeReuseKey = (Option<String>, PkgNameVerPeer, i32);

/// An importer's resolved direct-dependency versions, keyed by package
/// name. See [`WorkspaceTreeCtx::direct_dep_versions`].
pub(super) type DirectDepVersions = HashMap<String, Vec<node_semver::Version>>;

/// One entry in [`WorkspaceTreeCtx`]'s `children_specs_by_id` map —
/// `(child_alias, child_range, child_optional, child_injected)` tuples extracted from
/// a resolved package's manifest's `dependencies` plus
/// `optionalDependencies` sections.
pub(super) type ChildSpec = (String, String, bool, bool);

/// The install's manifest hooks: the two [`ManifestHook`]s applied
/// around the pnpmfile's own `readPackage`, the pnpmfile itself, and the
/// `context.log(...)` sink its calls forward to.
#[derive(Default)]
pub struct WorkspaceHooks {
    pub manifest_hook: Option<ManifestHook>,
    pub overrides_hook: Option<ManifestHook>,
    pub pnpmfile_hook: Option<Arc<dyn PnpmfileHooks>>,
    pub read_package_log: Option<pnpm_hooks::LogFn>,
}

/// The sinks the walk reports through. `None` keeps the behavior the
/// notification describes and drops the notification.
#[derive(Default)]
pub struct WorkspaceLogs {
    pub skipped_optional_log: Option<SkippedOptionalLogFn>,
    pub finalized_package: Option<FinalizedPackageFn>,
    pub deprecation_log: Option<DeprecationLogFn>,
}

/// What the walk may take from the install's previous `pnpm-lock.yaml`,
/// and how far an update suppresses that reuse.
pub struct LockfileReuse {
    pub wanted_lockfile: Option<Arc<pnpm_lockfile::Lockfile>>,
    pub reuse_lockfile_subtrees: bool,
    pub update_reuse_scope: UpdateReuseScope,
    pub update_reuse_scopes_by_importer: BTreeMap<String, UpdateReuseScope>,
    pub update_depth: UpdateDepth,
}

impl Default for LockfileReuse {
    fn default() -> Self {
        LockfileReuse {
            wanted_lockfile: None,
            reuse_lockfile_subtrees: true,
            update_reuse_scope: UpdateReuseScope::default(),
            update_reuse_scopes_by_importer: BTreeMap::new(),
            update_depth: UpdateDepth::default(),
        }
    }
}

/// The install-wide settings every importer's walk resolves under.
#[derive(Default)]
pub struct WorkspaceResolutionPolicy {
    pub share_workspace_resolutions: bool,
    pub auto_install_peers: bool,
    pub allowed_deprecated_versions: BTreeMap<String, String>,
    pub registry_context: RegistryContext,
}

/// Everything an install hands the context its importers share, as
/// [`WorkspaceTreeCtx::from_wiring`] takes it.
#[derive(Default)]
pub struct WorkspaceWiring {
    pub hooks: WorkspaceHooks,
    pub logs: WorkspaceLogs,
    pub lockfile_reuse: LockfileReuse,
    pub policy: WorkspaceResolutionPolicy,
}

impl WorkspaceWiring {
    /// The wiring of a single-importer install: the manifest hooks, and
    /// `autoInstallPeers` as the one install-wide setting such a walk
    /// takes beyond them.
    #[must_use]
    pub fn for_importer(hooks: WorkspaceHooks, auto_install_peers: bool) -> Self {
        WorkspaceWiring {
            hooks,
            policy: WorkspaceResolutionPolicy {
                auto_install_peers,
                ..WorkspaceResolutionPolicy::default()
            },
            ..WorkspaceWiring::default()
        }
    }
}

/// Workspace-shared maps. Every per-importer [`TreeCtx`] in a
/// multi-importer install holds an `Arc<WorkspaceTreeCtx>` so the
/// resolver's per-`pkgIdWithPatchHash` dedup (`packages`,
/// `children_specs_by_id`, `children_by_id`, `resolved_by_wanted`) and
/// the peer-walker's seed sets (`all_peer_dep_names`,
/// `applied_patches`, `policy_violations`) carry across importers. One
/// shared context is handed to every importer's hoist loop.
///
/// `dependencies_tree` (`NodeId → DependenciesTreeNode`) is keyed by
/// per-occurrence `NodeIds`, which are unique even across importers, so
/// every importer's walk contributes entries to one combined tree
/// without colliding.
#[derive(smart_default::SmartDefault)]
pub struct WorkspaceTreeCtx {
    /// Bumped whenever an [`fn@extend_tree`] call may mutate the shared
    /// maps. The peer-hoist discovery engine compares it against the
    /// revision of its last sync to skip re-syncing an unchanged
    /// context.
    ///
    /// [`fn@extend_tree`]: super::extend_tree
    revision: std::sync::atomic::AtomicU64,
    /// Bumped whenever a children-ownership change rewrites an existing
    /// occurrence node's children in `dependencies_tree` (see
    /// [`fn@make_non_owner_nodes_lazy`]). Such a rewrite invalidates
    /// walk state derived from the previous children, so the discovery
    /// engine rebuilds its view instead of merging when this advanced
    /// since its last sync.
    children_rewrites: std::sync::atomic::AtomicU64,
    pub(super) packages: Mutex<HashMap<Arc<str>, ResolvedPackage>>,
    /// `pkgIdWithPatchHash` of every importer-level direct dependency
    /// recorded so far (initial waves plus hoisted peers), across all
    /// importers. These are the roots [`Self::run_preferred_versions`]
    /// derives the run-resolved preferred versions from.
    preferred_version_roots: Mutex<HashSet<String>>,
    /// `pkgIdWithPatchHash → (name, version)` for packages whose
    /// resolution carries no `name_ver` but was wanted through a
    /// non-path `workspace:` specifier — the workspace-project versions
    /// [`Self::run_preferred_versions`] folds for such packages. Keyed
    /// by package id so the fold stays reachability-gated.
    workspace_manifest_identities: Mutex<HashMap<String, (String, String)>>,
    /// Memoised result of [`Self::run_preferred_versions`], keyed by the
    /// `(revision, children_rewrites)` pair it was computed at.
    run_versions_cache: Mutex<RunVersionsCache>,
    dependencies_tree: Mutex<HashMap<NodeId, DependenciesTreeNode>>,
    pub(super) all_peer_dep_names: Mutex<HashSet<String>>,
    pub(super) policy_violations:
        Mutex<Vec<pnpm_resolving_resolver_base::ResolutionPolicyViolation>>,
    pub(super) applied_patches: Mutex<HashSet<String>>,
    pub(super) resolved_by_wanted:
        Mutex<HashMap<WantedKey, Arc<pnpm_resolving_resolver_base::ResolveResult>>>,
    /// Resolver output for workspace directory resolutions before manifest
    /// hooks run. `link:` paths are canonicalised relative to the lockfile
    /// root, so one entry can be rendered for every consuming importer.
    pub(super) resolved_workspace_by_wanted:
        Mutex<HashMap<SharedWorkspaceWantedKey, Arc<pnpm_resolving_resolver_base::ResolveResult>>>,
    /// Hook-processed workspace results indexed by their canonical target and
    /// rendered consumer link, so importers that render the same `link:` reuse
    /// one hook pass. `resolved_by_wanted` keeps its project-scoped entry for
    /// these too — this map is what a *different* importer hits.
    pub(super) resolved_workspace_final_by_wanted:
        Mutex<HashMap<WorkspaceFinalWantedKey, Arc<pnpm_resolving_resolver_base::ResolveResult>>>,
    /// See [`crate::WorkspaceResolveOptions::share_workspace_resolutions`].
    pub(super) share_workspace_resolutions: bool,
    pub(super) children_specs_by_id: Mutex<HashMap<Arc<str>, Arc<Vec<ChildSpec>>>>,
    /// Package ids whose children have already been speculatively
    /// resolved. A package is warmed once, however many occurrences of
    /// it a level seeds — see [`fn@warm_children_resolutions`].
    ///
    /// [`fn@warm_children_resolutions`]: super::walk::warm_children_resolutions
    warmed_children_by_id: Mutex<HashSet<Arc<str>>>,
    pub(super) children_by_id: Mutex<HashMap<Arc<str>, RecordedChildren>>,
    children_owner_by_id: Mutex<HashMap<Arc<str>, ChildrenOwnerEntry>>,
    node_parent_ids_by_id: Mutex<HashMap<NodeId, Arc<Vec<String>>>>,
    /// Reverse index over `dependencies_tree`: every occurrence node
    /// recorded for a `pkgIdWithPatchHash`. Keeps
    /// [`fn@make_non_owner_nodes_lazy`] proportional to the package's
    /// own occurrences — scanning the whole tree per recorded package
    /// made lockfile-reuse walks quadratic in workspace size.
    nodes_by_pkg_id: Mutex<HashMap<Arc<str>, Vec<NodeId>>>,
    /// See [`SyncLog`].
    sync_log: Mutex<SyncLog>,
    pub(super) manifest_hook: Option<ManifestHook>,
    /// [`ManifestHook`] applied *after* [`Self::pnpmfile_hook`], where
    /// `manifest_hook` runs before it. pnpm's `createReadPackageHook`
    /// composes `packageExtensions → readPackage hooks → overrides`, so
    /// overrides land here: a hook that replaces the manifest (e.g. an
    /// embedder substituting a workspace project's raw manifest) must not
    /// erase the overrides.
    pub(super) overrides_hook: Option<ManifestHook>,
    /// The previous `pnpm-lock.yaml` the install started from, when one
    /// exists. Consulted by [`resolve_node`] to reuse an already-resolved
    /// dependency + its transitive subtree instead of re-resolving from
    /// the registry (see `pnpm/plans/LOCKFILE_RESOLUTION_REUSE.md`).
    /// `None` on a first install or when reuse is disabled.
    ///
    /// [`resolve_node`]: super::walk::resolve_node
    pub(super) wanted_lockfile: Option<Arc<pnpm_lockfile::Lockfile>>,
    /// Whether the walk may reuse whole already-resolved subtrees from
    /// [`Self::wanted_lockfile`]; `false` keeps it as a per-edge
    /// version-pin source only. See
    /// [`WorkspaceResolveOptions::reuse_lockfile_subtrees`] for the
    /// contract.
    ///
    /// [`WorkspaceResolveOptions::reuse_lockfile_subtrees`]: crate::WorkspaceResolveOptions::reuse_lockfile_subtrees
    #[default(true)]
    pub(super) reuse_lockfile_subtrees: bool,
    /// Lockfile-reuse suppression for `pacquet update`. `update`
    /// re-resolves its target deps to highest-in-range, so a reused
    /// resolution would defeat the bump. See [`UpdateReuseScope`].
    pub(super) update_reuse_scope: UpdateReuseScope,
    /// Importer overrides used by filtered workspace updates. IDs absent from
    /// this map keep the workspace default above.
    update_reuse_scopes_by_importer: BTreeMap<String, UpdateReuseScope>,
    /// `pacquet update --depth`: how deep the suppression above reaches.
    pub(super) update_depth: UpdateDepth,
    /// Memoises `reuse::subtree_fully_reusable` per update scope and snapshot
    /// key. Keep-all importers share one scope; update-active importers use
    /// isolated scopes so one importer's reuse answer cannot leak to another.
    /// `true` means the package and its entire transitive subtree can be
    /// synthesized from the prior lockfile.
    pub(super) subtree_reusable: Mutex<HashMap<SubtreeReuseKey, bool>>,
    pub(super) pnpmfile_hook: Option<Arc<dyn PnpmfileHooks>>,
    /// `context.log(...)` sink for the `pnpmfile_hook`'s `readPackage`
    /// calls, pre-bound to the install's reporter, project prefix, and
    /// pnpmfile path. `None` leaves hook logging a no-op. See
    /// [`WorkspaceTreeCtx::with_read_package_log`].
    pub(super) read_package_log: Option<pnpm_hooks::LogFn>,
    /// Sink for skipped-optional-dependency notifications. `None`
    /// keeps the skip behavior but drops the notification. See
    /// [`SkippedOptionalLogFn`].
    pub(super) skipped_optional_log: Option<SkippedOptionalLogFn>,
    /// Sink for finalized-package notifications. `None` skips the
    /// per-level subtree sweep entirely. See [`FinalizedPackageFn`].
    pub(super) finalized_package: Option<FinalizedPackageFn>,
    /// The package ids already handed to `finalized_package`, so every
    /// package is announced once across importers and hoist rounds.
    pub(super) finalized_ids: Mutex<HashSet<Arc<str>>>,
    /// Packages written or re-recorded since the last finalization
    /// sweep: the only ones whose verdict can have changed on their own.
    /// Maintained only while `finalized_package` is set.
    pub(super) finalization_pending: Mutex<Vec<Arc<str>>>,
    /// Every package whose recorded children include the key, so a
    /// package's finalization can be propagated to the packages
    /// depending on it. Maintained only while `finalized_package` is
    /// set; see [`update_parent_index`](children_ownership::update_parent_index).
    pub(super) parents_by_id: Mutex<HashMap<Arc<str>, HashSet<Arc<str>>>>,
    /// The `pnpm.allowedDeprecatedVersions` map. See
    /// [`crate::WorkspaceResolveOptions::allowed_deprecated_versions`].
    pub(super) allowed_deprecated_versions: BTreeMap<String, String>,
    /// Sink for deprecation notifications. `None` keeps the
    /// deprecation check but drops the notification. See
    /// [`DeprecationLogFn`].
    pub(super) deprecation_log: Option<DeprecationLogFn>,
    /// The install's `autoInstallPeers` setting. It widens which of a
    /// resolved package's `dependencies` its own `peerDependencies`
    /// shadow — see [`peer_shadowed_dependencies`].
    ///
    /// [`peer_shadowed_dependencies`]: crate::parent_pkg_aliases::peer_shadowed_dependencies
    pub(super) auto_install_peers: bool,
    /// Resolved registry map (`"default"` + per-scope) used to
    /// materialize a prior `Registry` lockfile resolution back into its
    /// tarball URL for the `currentPkg` payload. Empty when the entry
    /// point doesn't thread registries (then `currentPkg` is withheld
    /// for `Registry`-shaped entries rather than sent without a URL).
    /// Alias → URL map of named registries (built-ins merged with the
    /// user's setting), for materializing a prior registry-qualified
    /// `Registry` lockfile resolution back into its tarball URL.
    pub(super) registry_context: RegistryContext,
    /// `pkg id → importer id` of the importer whose occurrence owns
    /// that package's shared children context. Ownership is chosen by
    /// update-active status followed by `(depth, importer order, parent path)`:
    /// a package's subtree is recorded once per id, and a non-owner occurrence
    /// reuses the owner occurrence's children and missing-peer report. Consumed via
    /// [`crate::HoistMissingScope`].
    first_importer_by_pkg: Mutex<SnapshotCell<HashMap<String, String>, HashMap<String, String>>>,
    /// Per package: the missing-peer names reported by the *initial*
    /// peer walk of the current children-owner generation, plus the
    /// owner that recorded them (`None` while only a non-owner's
    /// provisional walk has been seen). The record is written once per
    /// generation: later hoist waves of the same owner never refresh it,
    /// so a peer the owner only satisfied by hoisting stays visible to
    /// every other importer's hoist. Consumed via
    /// [`crate::HoistMissingScope`].
    first_walk_missing_by_pkg: Mutex<FirstWalkMissingCell>,
    /// Per importer: direct-dep aliases whose manifest specifier differs
    /// from the prior lockfile (new deps included). Gates the stale-pin
    /// refresh's reuse-decline; only a changed direct dep can re-resolve
    /// away from a transitive occurrence's pin. Keyed by importer: this
    /// crate resolves importers sequentially (no workspace-wide
    /// directs-before-transitives barrier), so a shared map would refresh
    /// one importer's edges from another's direct deps order-dependently.
    /// (pnpm has that barrier and so converges cross-importer; pacquet
    /// stays per-importer to stay deterministic.)
    pub(super) changed_direct_deps: Mutex<HashMap<String, HashSet<PkgName>>>,
    /// Per importer: the parsed resolved versions of its direct
    /// dependencies, recorded once the direct-dep level finishes resolving.
    /// The direct-dep versions folded into the children's
    /// `preferredVersions`; consulted by [`fn@higher_direct_dep_version`].
    /// `Arc` so the hot child walk snapshots the importer's map with one
    /// lock + refcount bump instead of locking per edge.
    ///
    /// [`fn@higher_direct_dep_version`]: super::reuse::higher_direct_dep_version
    pub(super) direct_dep_versions: Mutex<HashMap<String, Arc<DirectDepVersions>>>,
}

/// The per-package missing-peer names
/// [`WorkspaceTreeCtx::first_walk_missing_by_pkg`] projects out of its
/// [`OwnerMissingRecord`] entries.
type FirstWalkMissing = HashMap<String, HashSet<String>>;

type FirstWalkMissingCell = SnapshotCell<HashMap<String, OwnerMissingRecord>, FirstWalkMissing>;

/// One [`WorkspaceTreeCtx::first_walk_missing_by_pkg`] entry: the
/// missing-peer names plus the owner generation that recorded them
/// (`None` for a non-owner's provisional report).
struct OwnerMissingRecord {
    recorded_by: Option<ChildrenOwner>,
    names: HashSet<String>,
}

/// A map whose readers want a whole-map snapshot rather than a lookup,
/// paired with the last snapshot handed out. Every hoist round of every
/// importer takes one of these, so rebuilding per read is quadratic in
/// workspace size — the cached `Arc` collapses a round of reads that
/// changed nothing into one projection.
///
/// Writers reach the map through [`Self::map_mut`], which drops the
/// snapshot; a writer that finds nothing to change must keep to
/// [`Self::map`] so the cache survives.
struct SnapshotCell<Map, Snapshot> {
    map: Map,
    snapshot: Option<Arc<Snapshot>>,
}

impl<Map: Default, Snapshot> Default for SnapshotCell<Map, Snapshot> {
    fn default() -> Self {
        SnapshotCell { map: Map::default(), snapshot: None }
    }
}

impl<Map, Snapshot> SnapshotCell<Map, Snapshot> {
    fn map(&self) -> &Map {
        &self.map
    }

    fn map_mut(&mut self) -> &mut Map {
        self.snapshot = None;
        &mut self.map
    }

    /// The current snapshot, projected through `project` when a write
    /// invalidated the last one. Snapshots already handed out keep the
    /// contents they were built from.
    fn snapshot(&mut self, project: impl FnOnce(&Map) -> Snapshot) -> Arc<Snapshot> {
        Arc::clone(self.snapshot.get_or_insert_with(|| Arc::new(project(&self.map))))
    }
}

/// State behind [`WorkspaceTreeCtx::run_preferred_versions`]: the
/// package ids already traversed and the `name → version` entries
/// their identities folded into, valid as of the recorded
/// `(revision, children_rewrites)` pair.
#[derive(Default)]
pub(super) struct RunVersionsCache {
    revision: u64,
    children_rewrites: u64,
    visited: HashSet<String>,
    /// Visited packages that carry no `name_ver` and whose
    /// workspace-manifest identity hasn't been recorded yet. Re-checked
    /// on every refresh: the identity is recorded per `workspace:` edge,
    /// and a later wave can add such an edge to an already-visited
    /// package.
    awaiting_identity: HashSet<String>,
    pub(super) versions: pnpm_resolving_resolver_base::PreferredVersions,
}

/// Append-only record of which keys of the shared maps have been
/// written since the context was created, so
/// [`WorkspaceTreeCtx::sync_discovery_tree`] can refresh a view by
/// visiting the writes instead of rescanning every map.
///
/// Only keys are recorded, never values: the sync reads each key's
/// current value, so a key logged several times, or logged by
/// concurrently-walking importers in either order, converges on the
/// same view.
#[derive(Default)]
struct SyncLog {
    packages: Vec<String>,
    children_by_id: Vec<String>,
    dependencies_tree: Vec<NodeId>,
    peer_dep_names: Vec<String>,
}

/// How much of a [`SyncLog`] a [`ResolvedTree`] view has already
/// absorbed. [`WorkspaceTreeCtx::rebuild_discovery_tree`] sets it for a
/// view built from scratch.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct SyncCursor {
    packages: usize,
    children_by_id: usize,
    dependencies_tree: usize,
    peer_dep_names: usize,
}

impl WorkspaceTreeCtx {
    /// The context an install's importers share, built from the
    /// install's own wiring. Every field the walk keeps for itself (the
    /// dedup maps, the peer seeds, the ownership bookkeeping) starts
    /// empty.
    #[must_use]
    pub fn from_wiring(wiring: WorkspaceWiring) -> Self {
        let WorkspaceWiring { hooks, logs, lockfile_reuse, policy } = wiring;
        WorkspaceTreeCtx {
            manifest_hook: hooks.manifest_hook,
            overrides_hook: hooks.overrides_hook,
            pnpmfile_hook: hooks.pnpmfile_hook,
            read_package_log: hooks.read_package_log,
            skipped_optional_log: logs.skipped_optional_log,
            finalized_package: logs.finalized_package,
            deprecation_log: logs.deprecation_log,
            wanted_lockfile: lockfile_reuse.wanted_lockfile,
            reuse_lockfile_subtrees: lockfile_reuse.reuse_lockfile_subtrees,
            update_reuse_scope: lockfile_reuse.update_reuse_scope,
            update_reuse_scopes_by_importer: lockfile_reuse.update_reuse_scopes_by_importer,
            update_depth: lockfile_reuse.update_depth,
            share_workspace_resolutions: policy.share_workspace_resolutions,
            auto_install_peers: policy.auto_install_peers,
            allowed_deprecated_versions: policy.allowed_deprecated_versions,
            registry_context: policy.registry_context,
            ..Self::default()
        }
    }
    /// The prior `pnpm-lock.yaml` to reuse resolutions from, if any.
    pub fn wanted_lockfile(&self) -> Option<&Arc<pnpm_lockfile::Lockfile>> {
        self.wanted_lockfile.as_ref()
    }

    /// Snapshot of `pkg id → children-owner importer id`. See the field doc.
    #[must_use]
    pub fn first_importer_by_pkg(&self) -> Arc<HashMap<String, String>> {
        lock_recoverable(&self.first_importer_by_pkg).snapshot(Clone::clone)
    }

    pub(super) fn update_reuse_scope_for(&self, importer_id: &str) -> &UpdateReuseScope {
        if matches!(self.update_reuse_scope, UpdateReuseScope::None) {
            return &self.update_reuse_scope;
        }
        self.update_reuse_scopes_by_importer.get(importer_id).unwrap_or(&self.update_reuse_scope)
    }
}

/// Take a mutex-held map out of a context this thread solely owns,
/// recovering from poisoning like every other read of these maps.
fn take_locked<Value: Default>(cell: &mut Mutex<Value>) -> Value {
    std::mem::take(cell.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod parent_index_tests;
