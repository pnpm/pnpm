//! The workspace-shared half of the walk: [`WorkspaceTreeCtx`], the
//! per-`pkgIdWithPatchHash` dedup maps every importer's walk
//! contributes to, and the children-ownership bookkeeping that settles
//! which occurrence of a package records its children.

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

/// Cache key for [`WorkspaceTreeCtx`]'s `resolved_by_wanted` map.
///
/// The npm-shaped slice pacquet exposes today calls
/// [`Resolver::resolve`] with four [`WantedDependency`] fields
/// populated — `alias`, `bare_specifier`, `optional`, and `injected` (see
/// the `WantedDependency` literals in [`extend_tree`] and the recursive
/// arm of [`fn@resolve_node`]). Anything else stays at `Default::default()`,
/// so a tuple over those four fields uniquely identifies a wanted
/// dep across revisits.
///
/// `optional` is part of the key because the npm resolver's
/// `pick_package` toggles between the abbreviated and full packument
/// based on `wanted.optional` — caching by `(alias, bare_specifier)`
/// alone would let an optional caller satisfy itself with a
/// non-optional caller's abbreviated result, losing the
/// `libc`/`cpu`/`os` filter inputs that mode supplies.
///
/// `injected` is part of the key because the workspace branch of the
/// npm resolver returns a `file:<path>` resolution when the dep is
/// injected and a `link:<path>` resolution otherwise (see
/// `resolve_from_local_package`). Two importers asking for the same
/// workspace dep with different `dependenciesMeta[*].injected` flags
/// must take different cache slots.
///
/// `pick_lowest_version` and `published_by` are part of the key because
/// `resolutionMode` makes the version pick depend on them: under
/// `time-based` / `lowest-direct` a direct dependency is resolved
/// lowest while a transitive one is resolved highest, and under
/// `time-based` transitive deps carry a publish-date cutoff a direct
/// dep does not. The same wanted spec (`react@^18`) can therefore
/// resolve to a different version as a direct vs. transitive dep, so
/// the two occurrences must take different cache slots. In `highest`
/// mode (the default) every occurrence shares the same pair, so the
/// dedup is unchanged.
///
/// `project_dir` is part of the key for any specifier that can produce
/// a project-relative resolution. This includes explicit local
/// specifiers (`link:` / `file:` / `workspace:`) and normal semver
/// specifiers in workspace mode, because `linkWorkspacePackages` can
/// replace the registry pick with a workspace package. A non-injected
/// workspace dep resolves through `resolve_from_local_package` to a
/// `link:<path>` whose `<path>` is computed *relative to the
/// consuming importer's directory*. Without `project_dir` in the key,
/// the first importer to resolve `(@scope/lib, ^1.0.0)` would
/// seed the workspace-wide cache with its own relative path and every
/// other importer would reuse it verbatim — e.g. a root resolving to
/// `link:packages/lib` would hand `packages/app` the same string,
/// which from `packages/app` points at the non-existent
/// `packages/app/packages/lib`.
///
/// The final two fields isolate importers with active update policies
/// and record whether this wanted dependency is an explicit update target.
/// Ordinary keep-all importers use no importer scope and retain the existing
/// cross-importer cache sharing.
///
/// [`Resolver::resolve`]: pnpm_resolving_resolver_base::Resolver::resolve
/// [`WantedDependency`]: pnpm_resolving_resolver_base::WantedDependency
/// [`extend_tree`]: super::extend_tree
/// [`fn@resolve_node`]: super::walk::resolve_node
pub(super) type WantedKeyFields = (
    Option<String>,
    Option<String>,
    Option<bool>,
    Option<bool>,
    bool,
    Option<DateTime<Utc>>,
    Option<PathKey>,
    Option<PkgNameVerPeer>,
    Vec<(String, Vec<String>)>,
    Option<String>,
    bool,
);

/// A [`WantedKeyFields`] tuple with its hashes fixed at construction —
/// the full one, and a consumer-scope-less one for
/// [`SharedWorkspaceWantedKey`] — so an edge's key is hashed once
/// however many maps and derived keys carry it. Cloning is cheap.
#[derive(Debug, Clone)]
pub(super) struct WantedKey(Arc<WantedKeyInner>);

#[derive(Debug)]
struct WantedKeyInner {
    full_hash: u64,
    scopeless_hash: u64,
    fields: WantedKeyFields,
}

impl WantedKey {
    pub(super) fn new(fields: WantedKeyFields) -> Self {
        // One pass over the fields serves both hashes: everything but
        // the consumer scope feeds a hasher whose intermediate state is
        // snapshotted for the scope-less hash before the scope joins.
        let mut hasher = rustc_hash::FxHasher::default();
        (
            &fields.0, &fields.1, &fields.2, &fields.3, &fields.4, &fields.5, &fields.7, &fields.8,
            &fields.9, &fields.10,
        )
            .hash(&mut hasher);
        let scopeless_hash = hasher.clone().finish();
        fields.6.hash(&mut hasher);
        WantedKey(Arc::new(WantedKeyInner { full_hash: hasher.finish(), scopeless_hash, fields }))
    }

    pub(super) fn fields(&self) -> &WantedKeyFields {
        &self.0.fields
    }

    /// Field-wise equality without the consumer-scope slot — the
    /// equality [`SharedWorkspaceWantedKey`] shares between importers.
    fn scopeless_eq(&self, other: &Self) -> bool {
        let left = &self.0.fields;
        let right = &other.0.fields;
        left.0 == right.0
            && left.1 == right.1
            && left.2 == right.2
            && left.3 == right.3
            && left.4 == right.4
            && left.5 == right.5
            && left.7 == right.7
            && left.8 == right.8
            && left.9 == right.9
            && left.10 == right.10
    }
}

impl PartialEq for WantedKey {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
            || (self.0.full_hash == other.0.full_hash && self.0.fields == other.0.fields)
    }
}

impl Eq for WantedKey {}

impl Hash for WantedKey {
    fn hash<State: Hasher>(&self, state: &mut State) {
        state.write_u64(self.0.full_hash);
    }
}

/// A path slot of a resolver cache key.
///
/// `Path`'s own `Hash` and `Eq` walk the path component by component,
/// and the per-edge key lookups made that walk one of the hottest
/// spots of a large workspace's resolution. The paths that reach these
/// keys come from one canonical config-derived source per importer, so
/// this wrapper compares and hashes the underlying `OsStr` — a plain
/// byte comparison. That is *stricter* than component equality
/// (`a//b` ≠ `a/b` here), which for a dedup cache can only cost an
/// extra identical resolution, never conflate two different paths.
#[derive(Debug, Clone, derive_more::From)]
pub(super) struct PathKey(pub(super) PathBuf);

impl PartialEq for PathKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_os_str() == other.0.as_os_str()
    }
}

impl Eq for PathKey {}

impl Hash for PathKey {
    fn hash<State: Hasher>(&self, state: &mut State) {
        self.0.as_os_str().hash(state);
    }
}

/// A wanted dependency key without its consumer directory, plus the resolver
/// inputs that may vary between importers.
///
/// Holds the edge's full [`WantedKey`]; `Hash`/`Eq` drop the consumer
/// directory, so keys built by different importers match by value.
#[derive(Debug, Clone)]
pub(super) struct SharedWorkspaceWantedKey {
    wanted: WantedKey,
    previous_specifier: Option<String>,
    // Behind an `Arc` because the fields are invariant per importer
    // (see [`WorkspaceResolutionOptionsKey`]) while a key is built per
    // dependency edge; `Hash`/`Eq` see through the `Arc`, so keys
    // built by different importers still match by value.
    resolve_options: Arc<WorkspaceResolutionOptionsKey>,
}

impl SharedWorkspaceWantedKey {
    pub(super) fn new(
        wanted: WantedKey,
        previous_specifier: Option<String>,
        resolve_options: &Arc<WorkspaceResolutionOptionsKey>,
    ) -> Self {
        Self { wanted, previous_specifier, resolve_options: Arc::clone(resolve_options) }
    }
}

impl PartialEq for SharedWorkspaceWantedKey {
    fn eq(&self, other: &Self) -> bool {
        self.wanted.scopeless_eq(&other.wanted)
            && self.previous_specifier == other.previous_specifier
            && self.resolve_options == other.resolve_options
    }
}

impl Eq for SharedWorkspaceWantedKey {}

impl Hash for SharedWorkspaceWantedKey {
    fn hash<State: Hasher>(&self, state: &mut State) {
        state.write_u64(self.wanted.0.scopeless_hash);
        self.previous_specifier.hash(state);
        self.resolve_options.hash(state);
    }
}

/// Resolver inputs that can change a named workspace resolution independently
/// of the consuming project directory.
///
/// Every field is invariant across the [`ResolveOptions`] variants one
/// importer's walk hands the resolver — the depth split changes only
/// the version pick, and the per-edge overrides change only
/// `project_dir` / `current_pkg` / the overlay — so [`TreeCtx`] builds
/// this once per importer and every edge shares it.
/// [`Self::matches_options`] backs the debug assertion pinning that
/// invariance.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct WorkspaceResolutionOptionsKey {
    workspace_packages: Option<WorkspacePackagesKey>,
    lockfile_dir: PathKey,
    default_tag: Option<String>,
    inject_workspace_packages: bool,
    calc_specifier: bool,
    range_spec_style_discriminant: Option<u8>,
    save_workspace_protocol_discriminant: u8,
}

impl WorkspaceResolutionOptionsKey {
    pub(super) fn new(options: &ResolveOptions) -> Self {
        Self {
            workspace_packages: options.workspace_packages.as_ref().map(WorkspacePackagesKey::new),
            lockfile_dir: PathKey(options.lockfile_dir.clone()),
            default_tag: options.default_tag.clone(),
            inject_workspace_packages: options.inject_workspace_packages,
            calc_specifier: options.calc_specifier,
            range_spec_style_discriminant: options.range_spec_style.map(|style| style as u8),
            save_workspace_protocol_discriminant: options.save_workspace_protocol as u8,
        }
    }

    /// Whether the importer-wide key still describes `options` — the
    /// per-importer invariance the shared cache relies on, asserted at
    /// the key's use site in debug builds.
    #[cfg(debug_assertions)]
    pub(super) fn matches_options(&self, options: &ResolveOptions) -> bool {
        *self == Self::new(options)
    }
}

/// Keeps the immutable workspace map alive.
/// The key uses pointer identity instead of hashing the map for every dependency edge.
/// Clones of the same [`Arc`] share a key. Separately allocated maps use separate keys.
#[derive(Clone)]
struct WorkspacePackagesKey(Arc<WorkspacePackages>);

impl WorkspacePackagesKey {
    fn new(packages: &Arc<WorkspacePackages>) -> Self {
        Self(Arc::clone(packages))
    }
}

impl std::fmt::Debug for WorkspacePackagesKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("WorkspacePackagesKey").field(&Arc::as_ptr(&self.0)).finish()
    }
}

impl PartialEq for WorkspacePackagesKey {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for WorkspacePackagesKey {}

impl Hash for WorkspacePackagesKey {
    fn hash<State: Hasher>(&self, state: &mut State) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

/// Cache key for a hook-processed workspace result.
#[derive(Debug, PartialEq, Eq, Hash)]
pub(super) struct WorkspaceFinalWantedKey {
    shared_wanted: SharedWorkspaceWantedKey,
    canonical_resolution_id: PkgResolutionId,
    rendered_resolution_id: PkgResolutionId,
}

impl WorkspaceFinalWantedKey {
    pub(super) fn new(
        shared_wanted: SharedWorkspaceWantedKey,
        canonical_resolution_id: &PkgResolutionId,
        rendered_resolution_id: &PkgResolutionId,
    ) -> Self {
        Self {
            shared_wanted,
            canonical_resolution_id: canonical_resolution_id.clone(),
            rendered_resolution_id: rendered_resolution_id.clone(),
        }
    }
}

type SubtreeReuseKey = (Option<String>, PkgNameVerPeer, i32);

/// An importer's resolved direct-dependency versions, keyed by package
/// name. See [`WorkspaceTreeCtx::direct_dep_versions`].
pub(super) type DirectDepVersions = HashMap<String, Vec<node_semver::Version>>;

/// One entry in [`WorkspaceTreeCtx`]'s `children_specs_by_id` map —
/// `(child_alias, child_range, child_optional, child_injected)` tuples extracted from
/// a resolved package's manifest's `dependencies` plus
/// `optionalDependencies` sections.
pub(super) type ChildSpec = (String, String, bool, bool);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ChildrenOwner {
    update_active: bool,
    depth: i32,
    pub(super) importer_order: usize,
    parent_path: Vec<String>,
    pub(super) importer_id: String,
}

impl ChildrenOwner {
    fn wins_over(&self, other: &Self) -> bool {
        if self.update_active != other.update_active {
            return self.update_active;
        }
        (&self.depth, &self.importer_order, &self.parent_path)
            < (&other.depth, &other.importer_order, &other.parent_path)
    }
}

/// What the current children owner of a `pkgIdWithPatchHash` decided
/// for it. Both halves are per-occurrence inputs the walk has to settle
/// on one answer for, so they are claimed together, under one lock.
#[derive(Debug, Clone)]
struct ChildrenOwnerEntry {
    pub(super) owner: ChildrenOwner,
    /// The owner occurrence's peer-shadowed `dependencies` (see
    /// [`peer_shadowed_dependencies`]). Which names are shadowed
    /// depends on the parent scope, which differs per occurrence;
    /// pnpm lets the first occurrence to resolve decide, which is
    /// arrival-ordered. The deterministic children owner decides here
    /// instead, so the same graph always yields the same lockfile.
    ///
    /// [`peer_shadowed_dependencies`]: crate::parent_pkg_aliases::peer_shadowed_dependencies
    pub(super) peer_shadowed: Arc<HashSet<String>>,
}

/// A package's recorded children together with what they were resolved
/// under. The two travel in one entry so a reader that accepts the
/// context cannot then expand edges another walk recorded.
#[derive(Debug, Clone)]
pub(super) struct RecordedChildren {
    pub(super) edges: Arc<Vec<crate::resolved_tree::ChildEdge>>,
    context: RecordedChildrenContext,
}

/// What a recorded `children_by_id` entry was resolved under. A later
/// occurrence may expand from that entry instead of walking the
/// package's manifest again only when its own context equals this one:
/// each field can change which children the walk produces.
#[derive(Debug, Clone)]
pub(super) struct RecordedChildrenContext {
    /// Dependencies the package's own `peerDependencies` shadow, which
    /// the walk drops from its children.
    pub(super) peer_shadowed: Arc<HashSet<String>>,
    /// The prior-lockfile key whose snapshot pinned the children, if the
    /// walk reused one.
    pub(super) prior_key: Option<PkgNameVerPeer>,
    /// Whether the resolving importer had an active update policy, which
    /// re-resolves what a keep-all importer reuses.
    pub(super) update_active: bool,
}

impl RecordedChildrenContext {
    /// Whether a walk under `other` would re-resolve what the prior
    /// lockfile pinned for this recording — the churn reuse exists to
    /// avoid, and which leaves the occurrences that realized the
    /// pinned subtree pointing at children the record no longer holds.
    ///
    /// An update policy is the one thing that re-resolves a pin on
    /// purpose, so a walk under one is never held to the pins.
    pub(super) fn pins_children_over(&self, other: &Self) -> bool {
        self.peer_shadowed == other.peer_shadowed
            && self.prior_key.is_some()
            && other.prior_key.is_none()
            && !other.update_active
    }

    /// Two contexts produce the same children.
    ///
    /// The preferred-versions overlay is deliberately not part of the
    /// test. `children_by_id` records one child list per package id,
    /// and every occurrence that does not own the children already
    /// expands from it whatever its own overlay says — the overlay
    /// only ever decides which occurrence's versions get *recorded*.
    /// Requiring identical overlays here would mean requiring identical
    /// parents, which the occurrences that race never have.
    pub(super) fn produces_same_children_as(&self, other: &Self) -> bool {
        self.peer_shadowed == other.peer_shadowed
            && self.prior_key == other.prior_key
            && self.update_active == other.update_active
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
    /// set; see [`update_parent_index`].
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

impl Default for WorkspaceTreeCtx {
    fn default() -> Self {
        WorkspaceTreeCtx {
            revision: std::sync::atomic::AtomicU64::new(0),
            children_rewrites: std::sync::atomic::AtomicU64::new(0),
            packages: Mutex::new(HashMap::default()),
            preferred_version_roots: Mutex::new(HashSet::default()),
            workspace_manifest_identities: Mutex::new(HashMap::default()),
            run_versions_cache: Mutex::new(RunVersionsCache {
                revision: 0,
                children_rewrites: 0,
                visited: HashSet::default(),
                awaiting_identity: HashSet::default(),
                versions: pnpm_resolving_resolver_base::PreferredVersions::new(),
            }),
            dependencies_tree: Mutex::new(HashMap::default()),
            all_peer_dep_names: Mutex::new(HashSet::default()),
            policy_violations: Mutex::new(Vec::new()),
            applied_patches: Mutex::new(HashSet::default()),
            resolved_by_wanted: Mutex::new(HashMap::default()),
            resolved_workspace_by_wanted: Mutex::new(HashMap::default()),
            resolved_workspace_final_by_wanted: Mutex::new(HashMap::default()),
            share_workspace_resolutions: false,
            children_specs_by_id: Mutex::new(HashMap::default()),
            warmed_children_by_id: Mutex::new(HashSet::default()),
            children_by_id: Mutex::new(HashMap::default()),
            children_owner_by_id: Mutex::new(HashMap::default()),
            node_parent_ids_by_id: Mutex::new(HashMap::default()),
            nodes_by_pkg_id: Mutex::new(HashMap::default()),
            sync_log: Mutex::new(SyncLog::default()),
            manifest_hook: None,
            overrides_hook: None,
            wanted_lockfile: None,
            reuse_lockfile_subtrees: true,
            update_reuse_scope: UpdateReuseScope::All,
            update_reuse_scopes_by_importer: BTreeMap::new(),
            update_depth: UpdateDepth::UNLIMITED,
            subtree_reusable: Mutex::new(HashMap::default()),
            pnpmfile_hook: None,
            read_package_log: None,
            skipped_optional_log: None,
            finalized_package: None,
            finalized_ids: Mutex::new(HashSet::default()),
            finalization_pending: Mutex::new(Vec::new()),
            parents_by_id: Mutex::new(HashMap::default()),
            allowed_deprecated_versions: BTreeMap::new(),
            deprecation_log: None,
            auto_install_peers: false,
            registry_context: RegistryContext::default(),
            first_importer_by_pkg: Mutex::new(SnapshotCell::default()),
            first_walk_missing_by_pkg: Mutex::new(SnapshotCell::default()),
            changed_direct_deps: Mutex::new(HashMap::default()),
            direct_dep_versions: Mutex::new(HashMap::default()),
        }
    }
}

impl WorkspaceTreeCtx {
    /// Sets [`crate::WorkspaceResolveOptions::share_workspace_resolutions`].
    #[must_use]
    pub fn with_shared_workspace_resolutions(mut self, share_workspace_resolutions: bool) -> Self {
        self.share_workspace_resolutions = share_workspace_resolutions;
        self
    }

    /// Snapshot the workspace context into a [`ResolvedTree`] without
    /// consuming `self`. `direct` carries the combined direct-dep
    /// envelopes the caller built up across importers; multi-importer
    /// orchestration usually leaves this empty and threads per-importer
    /// direct deps separately into [`fn@crate::resolve_peers_workspace`].
    pub fn snapshot(&self, direct: Vec<DirectDep>) -> ResolvedTree {
        ResolvedTree {
            direct,
            packages: lock_recoverable(&self.packages).clone(),
            dependencies_tree: lock_recoverable(&self.dependencies_tree).clone(),
            all_peer_dep_names: lock_recoverable(&self.all_peer_dep_names).clone(),
            policy_violations: lock_recoverable(&self.policy_violations).clone(),
            applied_patches: lock_recoverable(&self.applied_patches).clone(),
            children_by_id: lock_recoverable(&self.children_by_id)
                .iter()
                .map(|(pkg_id, recorded)| {
                    (std::sync::Arc::<str>::clone(pkg_id), Arc::clone(&recorded.edges))
                })
                .collect(),
        }
    }

    /// Snapshot only the part of the occurrence tree reachable from one
    /// importer's direct dependencies.
    ///
    /// A workspace resolve keeps every importer's occurrence nodes in
    /// this shared context, so a full [`Self::snapshot`] taken for one
    /// importer retains every other importer's nodes too. The
    /// reachable-only snapshot keeps per-importer consumers (the
    /// workspace-root hoistable-deps scan) proportional to that
    /// importer's own subtree.
    ///
    /// Realized edges provide the occurrence-node closure. Lazy edges are
    /// materialized by the peer walker from `children_by_id`, so their
    /// package closure is collected separately and included here too.
    #[must_use]
    pub fn snapshot_reachable_from(&self, direct: Vec<DirectDep>) -> ResolvedTree {
        let dependencies_tree = lock_recoverable(&self.dependencies_tree);
        let mut reachable_node_ids = HashSet::default();
        let mut reachable_pkg_ids = HashSet::default();
        let mut pending_node_ids: Vec<NodeId> =
            direct.iter().map(|dep| dep.node_id.clone()).collect();
        while let Some(node_id) = pending_node_ids.pop() {
            if !reachable_node_ids.insert(node_id.clone()) {
                continue;
            }
            let Some(node) = dependencies_tree.get(&node_id) else {
                continue;
            };
            reachable_pkg_ids.insert(std::sync::Arc::<str>::clone(&node.resolved_package_id));
            if let crate::resolved_tree::TreeChildren::Realized(children) = &node.children {
                pending_node_ids.extend(children.values().cloned());
            }
        }
        let reachable_dependencies_tree: HashMap<_, _> = reachable_node_ids
            .iter()
            .filter_map(|node_id| {
                dependencies_tree.get(node_id).cloned().map(|node| (node_id.clone(), node))
            })
            .collect();
        // Release before taking the next guard so this function never
        // holds two of the context's locks at once — `snapshot` takes
        // `packages` before `dependencies_tree`, so overlapping here
        // would create a reversed acquisition order.
        drop(dependencies_tree);
        let dependencies_tree = reachable_dependencies_tree;

        let all_children = lock_recoverable(&self.children_by_id);
        let mut pending_pkg_ids: Vec<Arc<str>> = reachable_pkg_ids.iter().cloned().collect();
        let mut children_by_id = HashMap::default();
        while let Some(pkg_id) = pending_pkg_ids.pop() {
            let Some(children) = all_children.get(&pkg_id) else {
                continue;
            };
            children_by_id.insert(pkg_id, Arc::clone(&children.edges));
            for child in children.edges.iter() {
                if reachable_pkg_ids.insert(std::sync::Arc::<str>::clone(&child.pkg_id)) {
                    pending_pkg_ids.push(std::sync::Arc::<str>::clone(&child.pkg_id));
                }
            }
        }
        drop(all_children);

        let packages = lock_recoverable(&self.packages);
        let packages = reachable_pkg_ids
            .into_iter()
            .filter_map(|pkg_id| packages.get(&*pkg_id).cloned().map(|pkg| (pkg_id, pkg)))
            .collect();

        ResolvedTree {
            direct,
            packages,
            dependencies_tree,
            all_peer_dep_names: lock_recoverable(&self.all_peer_dep_names).clone(),
            policy_violations: lock_recoverable(&self.policy_violations).clone(),
            applied_patches: lock_recoverable(&self.applied_patches).clone(),
            children_by_id,
        }
    }

    /// Attach a `readPackageHook` applied to every resolved manifest
    /// before it enters the wanted-dep cache. See [`ManifestHook`] for
    /// the signature.
    #[must_use]
    pub fn with_manifest_hook(mut self, manifest_hook: Option<ManifestHook>) -> Self {
        self.manifest_hook = manifest_hook;
        self
    }

    /// Attach the post-pnpmfile [`ManifestHook`] (overrides). See the
    /// `overrides_hook` field for the ordering contract.
    #[must_use]
    pub fn with_overrides_hook(mut self, overrides_hook: Option<ManifestHook>) -> Self {
        self.overrides_hook = overrides_hook;
        self
    }

    /// Attach the prior `pnpm-lock.yaml` so `resolve_node` can reuse
    /// already-resolved dependencies instead of re-resolving them. See
    /// the `wanted_lockfile` field.
    #[must_use]
    pub fn with_wanted_lockfile(
        mut self,
        wanted_lockfile: Option<Arc<pnpm_lockfile::Lockfile>>,
    ) -> Self {
        self.wanted_lockfile = wanted_lockfile;
        self
    }

    /// The prior `pnpm-lock.yaml` to reuse resolutions from, if any.
    pub fn wanted_lockfile(&self) -> Option<&Arc<pnpm_lockfile::Lockfile>> {
        self.wanted_lockfile.as_ref()
    }

    /// Restrict [`Self::wanted_lockfile`] to per-edge version pinning.
    /// See the `reuse_lockfile_subtrees` field.
    #[must_use]
    pub fn with_reuse_lockfile_subtrees(mut self, reuse_lockfile_subtrees: bool) -> Self {
        self.reuse_lockfile_subtrees = reuse_lockfile_subtrees;
        self
    }

    /// Snapshot of `pkg id → children-owner importer id`. See the field doc.
    #[must_use]
    pub fn first_importer_by_pkg(&self) -> Arc<HashMap<String, String>> {
        lock_recoverable(&self.first_importer_by_pkg).snapshot(Clone::clone)
    }

    /// Record a walk's per-package missing-peer names. The owning
    /// importer's report is written once per ownership generation —
    /// its own later hoist waves never refresh it — and replaces any
    /// provisional report a non-owner's earlier walk left behind. See
    /// the `first_walk_missing_by_pkg` field doc.
    pub(crate) fn record_first_walk_missing(
        &self,
        importer_id: &str,
        missing_by_pkg: &HashMap<&str, MissingNames<'_>>,
    ) {
        // Lock order: `children_owner_by_id` before
        // `first_walk_missing_by_pkg`, the only place both are held.
        let owners = lock_recoverable(&self.children_owner_by_id);
        let mut record = lock_recoverable(&self.first_walk_missing_by_pkg);
        for (pkg_id, ChildrenOwnerEntry { owner, .. }) in owners.iter() {
            if owner.importer_id != importer_id {
                continue;
            }
            let recorded_by_current_owner = record
                .map()
                .get(&**pkg_id)
                .is_some_and(|entry| entry.recorded_by.as_ref() == Some(owner));
            if !recorded_by_current_owner {
                let names = missing_by_pkg
                    .get(&**pkg_id)
                    .map(|names| names.iter().map(str::to_owned).collect())
                    .unwrap_or_default();
                record.map_mut().insert(
                    std::sync::Arc::<str>::clone(pkg_id).to_string(),
                    OwnerMissingRecord { recorded_by: Some(owner.clone()), names },
                );
            }
        }
        for (pkg_id, names) in missing_by_pkg {
            if record.map().contains_key(*pkg_id) {
                continue;
            }
            if owners.get(*pkg_id).is_none_or(|entry| entry.owner.importer_id != importer_id) {
                record.map_mut().insert(
                    (*pkg_id).to_owned(),
                    OwnerMissingRecord {
                        recorded_by: None,
                        names: names.iter().map(str::to_owned).collect(),
                    },
                );
            }
        }
    }

    /// Snapshot of the per-package owner-context missing-peer names.
    /// See the `first_walk_missing_by_pkg` field doc.
    #[must_use]
    pub fn first_walk_missing_by_pkg(&self) -> Arc<FirstWalkMissing> {
        lock_recoverable(&self.first_walk_missing_by_pkg).snapshot(|record| {
            record.iter().map(|(pkg_id, entry)| (pkg_id.clone(), entry.names.clone())).collect()
        })
    }

    /// Record importer-level direct-dependency package ids as roots for
    /// [`Self::run_preferred_versions`]. Called by [`fn@extend_tree`]
    /// after each importer-level wave resolves.
    ///
    /// [`fn@extend_tree`]: super::extend_tree
    pub(super) fn record_preferred_version_roots<'id>(
        &self,
        pkg_ids: impl Iterator<Item = &'id str>,
    ) {
        let mut roots = lock_recoverable(&self.preferred_version_roots);
        for pkg_id in pkg_ids {
            if !roots.contains(pkg_id) {
                roots.insert(pkg_id.to_string());
            }
        }
    }

    /// The `name → version` entries of every package reachable from any
    /// importer's recorded direct dependencies, shaped as the plain
    /// [`pnpm_resolving_resolver_base::PreferredVersions`] entries the
    /// peer-hoist pickers bias toward.
    ///
    /// Derived from the settled tree — the recorded roots plus the
    /// children edges the deterministic children owners recorded — not
    /// from resolution arrival order. Concurrent importer waves can
    /// transiently walk (and resolve versions inside) a subtree whose
    /// children ownership a better-placed occurrence later takes over;
    /// whether such a walk happens at all depends on thread
    /// interleaving, so an arrival-ordered fold gives the pickers
    /// run-to-run varying candidates and reshuffles peer bindings on
    /// every install. The reachable closure is interleaving-independent
    /// because both the roots and the surviving children records are.
    ///
    /// The closure is cached and extended incrementally: shared-map
    /// growth (`revision`) only ever hangs new subtrees under new
    /// roots, so previously visited packages keep their entries; a
    /// children-ownership rewrite (`children_rewrites`) can restructure
    /// existing subtrees, so it rebuilds the closure from scratch.
    pub(super) fn run_preferred_versions(&self) -> MutexGuard<'_, RunVersionsCache> {
        let revision = self.revision();
        let children_rewrites = self.children_rewrites();
        let mut cache = lock_recoverable(&self.run_versions_cache);
        if cache.revision == revision && cache.children_rewrites == children_rewrites {
            return cache;
        }
        if cache.children_rewrites != children_rewrites {
            cache.visited.clear();
            cache.awaiting_identity.clear();
            cache.versions.clear();
        }
        let mut queue: Vec<String> = lock_recoverable(&self.preferred_version_roots)
            .iter()
            .filter(|pkg_id| !cache.visited.contains(*pkg_id))
            .cloned()
            .collect();
        // Lock discipline: `run_versions_cache` is locked only by this
        // function, so holding it across the refresh cannot form an
        // acquisition cycle; the context's shared maps (roots,
        // `children_by_id`, `packages`, identities) are each taken on
        // their own, never two at a time. Contention is also not a
        // concern: refreshes run at the quiescent points between hoist
        // waves, not while walks hold the shared maps hot.
        let mut newly_visited: Vec<String> = Vec::new();
        {
            let children_by_id = lock_recoverable(&self.children_by_id);
            while let Some(pkg_id) = queue.pop() {
                if !cache.visited.insert(pkg_id.clone()) {
                    continue;
                }
                if let Some(children) = children_by_id.get(pkg_id.as_str()) {
                    for child in children.edges.iter() {
                        if !cache.visited.contains(&*child.pkg_id) {
                            queue.push(std::sync::Arc::<str>::clone(&child.pkg_id).to_string());
                        }
                    }
                }
                newly_visited.push(pkg_id);
            }
        }
        {
            let packages = lock_recoverable(&self.packages);
            for pkg_id in newly_visited {
                match packages.get(pkg_id.as_str()).and_then(|pkg| pkg.result.name_ver.as_ref()) {
                    Some(name_ver) => fold_version(
                        &mut cache.versions,
                        name_ver.name.to_string(),
                        name_ver.suffix.to_string(),
                    ),
                    None => {
                        cache.awaiting_identity.insert(pkg_id);
                    }
                }
            }
        }
        let identities = lock_recoverable(&self.workspace_manifest_identities);
        let RunVersionsCache { awaiting_identity, versions, .. } = &mut *cache;
        awaiting_identity.retain(|pkg_id| match identities.get(pkg_id) {
            Some((name, version)) => {
                fold_version(versions, name.clone(), version.clone());
                false
            }
            None => true,
        });
        drop(identities);
        cache.revision = revision;
        cache.children_rewrites = children_rewrites;
        cache
    }

    /// Record the manifest identity of a `name_ver`-less package wanted
    /// through a non-path `workspace:` specifier, for
    /// [`Self::run_preferred_versions`] to fold once the package is
    /// reachable.
    pub(super) fn record_workspace_manifest_identity(
        &self,
        pkg_id: &str,
        name: &str,
        version: &str,
    ) {
        lock_recoverable(&self.workspace_manifest_identities)
            .entry(pkg_id.to_string())
            .or_insert_with(|| (name.to_string(), version.to_string()));
    }

    /// See the `revision` field doc.
    pub(crate) fn revision(&self) -> u64 {
        self.revision.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn bump_revision(&self) {
        self.revision.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// See the `children_rewrites` field doc.
    pub(crate) fn children_rewrites(&self) -> u64 {
        self.children_rewrites.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn record_children_rewrite(&self) {
        self.children_rewrites.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// Record a write to one of the maps [`Self::sync_discovery_tree`]
    /// mirrors. Every write a sync has to observe must be recorded here,
    /// including an in-place mutation of an entry that already exists:
    /// the sync visits recorded keys only, so an unrecorded write stays
    /// invisible to the discovery engine's view.
    pub(super) fn record_package_write(&self, pkg_id: &str) {
        lock_recoverable(&self.sync_log).packages.push(pkg_id.to_string());
        self.note_finalization_candidate(pkg_id);
    }

    /// Queue `pkg_id` for the next finalization sweep. See
    /// [`Self::finalization_pending`].
    fn note_finalization_candidate(&self, pkg_id: &str) {
        if self.finalized_package.is_some() {
            lock_recoverable(&self.finalization_pending).push(Arc::from(pkg_id));
        }
    }

    /// See [`Self::record_package_write`].
    pub(super) fn record_children_by_id_write(&self, pkg_id: &str) {
        lock_recoverable(&self.sync_log).children_by_id.push(pkg_id.to_string());
    }

    /// See [`Self::record_package_write`].
    pub(super) fn record_tree_node_write(&self, node_id: &NodeId) {
        lock_recoverable(&self.sync_log).dependencies_tree.push(node_id.clone());
    }

    /// See [`Self::record_package_write`].
    pub(super) fn record_peer_dep_name(&self, name: &str) {
        lock_recoverable(&self.sync_log).peer_dep_names.push(name.to_string());
    }

    /// Fold the context's growth since the last sync into `tree`, the
    /// peer-hoist discovery engine's persistent view of the workspace.
    ///
    /// The shared maps grow monotonically during the hoist rounds, so
    /// the sync inserts entries `tree` doesn't have yet and lowers node
    /// depths that shrank. The exceptions are a children-owner change,
    /// which can rewrite an existing package's recorded child list or
    /// peer-dependency split: those invalidate walk results already
    /// derived from the old values, so the sync reports them as
    /// unmergeable (`false`) and the engine rebuilds its view from
    /// scratch. A replaced `children_by_id` `Arc` with equal contents
    /// (an ownership handover that re-recorded the same children) is
    /// re-pointed without invalidating.
    ///
    /// The sync visits the keys written since `cursor` rather than every
    /// entry of the shared maps, which is what keeps a hoist round
    /// proportional to what the round changed instead of to the size of
    /// the workspace. On `false` the cursor is left where it was: the
    /// caller discards the view and builds a fresh one with
    /// [`Self::rebuild_discovery_tree`].
    pub(crate) fn sync_discovery_tree(
        &self,
        tree: &mut ResolvedTree,
        cursor: &mut SyncCursor,
    ) -> bool {
        use std::collections::hash_map::Entry;
        let next = {
            let log = lock_recoverable(&self.sync_log);
            SyncCursor {
                packages: log.packages.len(),
                children_by_id: log.children_by_id.len(),
                dependencies_tree: log.dependencies_tree.len(),
                peer_dep_names: log.peer_dep_names.len(),
            }
        };
        {
            let written = self.written_since(cursor.children_by_id, next.children_by_id, |log| {
                &log.children_by_id
            });
            let children_by_id = lock_recoverable(&self.children_by_id);
            for pkg_id in &written {
                let Some(spec) =
                    children_by_id.get(pkg_id.as_str()).map(|recorded| &recorded.edges)
                else {
                    continue;
                };
                match tree.children_by_id.entry(Arc::from(pkg_id.clone())) {
                    Entry::Vacant(entry) => {
                        entry.insert(Arc::clone(spec));
                    }
                    Entry::Occupied(mut entry) => {
                        if Arc::ptr_eq(entry.get(), spec) {
                            continue;
                        }
                        if **entry.get() != **spec {
                            return false;
                        }
                        entry.insert(Arc::clone(spec));
                    }
                }
            }
        }
        {
            let written = self.written_since(cursor.packages, next.packages, |log| &log.packages);
            let packages = lock_recoverable(&self.packages);
            for pkg_id in &written {
                let Some(pkg) = packages.get(pkg_id.as_str()) else { continue };
                match tree.packages.entry(Arc::from(pkg_id.clone())) {
                    Entry::Vacant(entry) => {
                        entry.insert(pkg.clone());
                    }
                    Entry::Occupied(entry) => {
                        if entry.get().peer_dependencies != pkg.peer_dependencies {
                            return false;
                        }
                    }
                }
            }
        }
        {
            let written =
                self.written_since(cursor.dependencies_tree, next.dependencies_tree, |log| {
                    &log.dependencies_tree
                });
            let dependencies_tree = lock_recoverable(&self.dependencies_tree);
            for node_id in &written {
                let Some(node) = dependencies_tree.get(node_id) else { continue };
                match tree.dependencies_tree.entry(node_id.clone()) {
                    Entry::Vacant(entry) => {
                        entry.insert(node.clone());
                    }
                    Entry::Occupied(mut entry) => {
                        if entry.get().depth > node.depth {
                            entry.get_mut().depth = node.depth;
                        }
                    }
                }
            }
        }
        let peer_dep_names =
            self.written_since(cursor.peer_dep_names, next.peer_dep_names, |log| {
                &log.peer_dep_names
            });
        tree.all_peer_dep_names.extend(peer_dep_names);
        *cursor = next;
        true
    }

    /// Fill an empty `tree` from the shared maps, and set `cursor` to
    /// where the refilled view picks the write log up.
    ///
    /// Replaying the whole write log would reach the same view, but a
    /// scan copies each key once instead of once into the log snapshot
    /// and once into the view. The cursor is read *before* the scan, so
    /// a write that lands mid-scan is either picked up here and replayed
    /// harmlessly by the next sync, or missed here and applied by it.
    pub(crate) fn rebuild_discovery_tree(&self, tree: &mut ResolvedTree, cursor: &mut SyncCursor) {
        *cursor = {
            let log = lock_recoverable(&self.sync_log);
            SyncCursor {
                packages: log.packages.len(),
                children_by_id: log.children_by_id.len(),
                dependencies_tree: log.dependencies_tree.len(),
                peer_dep_names: log.peer_dep_names.len(),
            }
        };
        for (pkg_id, recorded) in lock_recoverable(&self.children_by_id).iter() {
            tree.children_by_id
                .entry(std::sync::Arc::<str>::clone(pkg_id))
                .or_insert_with(|| Arc::clone(&recorded.edges));
        }
        for (pkg_id, pkg) in lock_recoverable(&self.packages).iter() {
            tree.packages
                .entry(std::sync::Arc::<str>::clone(pkg_id))
                .or_insert_with(|| pkg.clone());
        }
        for (node_id, node) in lock_recoverable(&self.dependencies_tree).iter() {
            tree.dependencies_tree.entry(node_id.clone()).or_insert_with(|| node.clone());
        }
        tree.all_peer_dep_names.extend(lock_recoverable(&self.all_peer_dep_names).iter().cloned());
    }

    /// The keys written to one of [`SyncLog`]'s slots between two cursor
    /// positions, copied out so the sync can take the map's own lock
    /// without holding the log's. The range is what one hoist round
    /// wrote; a from-scratch view goes through
    /// [`Self::rebuild_discovery_tree`] instead of replaying the log.
    fn written_since<Key: Clone>(
        &self,
        from: usize,
        to: usize,
        slot: impl Fn(&SyncLog) -> &Vec<Key>,
    ) -> Vec<Key> {
        if from >= to {
            return Vec::new();
        }
        slot(&lock_recoverable(&self.sync_log))[from..to].to_vec()
    }

    /// `NodeId → pkgIdWithPatchHash` for the given peer-provider nodes,
    /// keeping only nodes the shared context resolved eagerly (a node
    /// the peer walker realized lazily has no context entry and is
    /// dropped, matching the hoist loop's providers-that-existed-before-
    /// the-pass filter).
    pub(crate) fn provider_pkg_ids<'node_ids>(
        &self,
        node_ids: impl Iterator<Item = &'node_ids NodeId>,
    ) -> HashMap<NodeId, String> {
        let dependencies_tree = lock_recoverable(&self.dependencies_tree);
        let packages = lock_recoverable(&self.packages);
        node_ids
            .filter_map(|node_id| {
                let pkg_id = &dependencies_tree.get(node_id)?.resolved_package_id;
                packages.contains_key(&**pkg_id).then(|| (node_id.clone(), pkg_id.to_string()))
            })
            .collect()
    }

    /// Set which dependencies `pacquet update` excludes from reuse. See
    /// [`UpdateReuseScope`].
    #[must_use]
    pub fn with_update_reuse_scope(mut self, scope: UpdateReuseScope) -> Self {
        self.update_reuse_scope = scope;
        self
    }

    #[must_use]
    pub fn with_update_reuse_scopes_by_importer(
        mut self,
        scopes: BTreeMap<String, UpdateReuseScope>,
    ) -> Self {
        self.update_reuse_scopes_by_importer = scopes;
        self
    }

    #[must_use]
    pub fn with_update_depth(mut self, update_depth: UpdateDepth) -> Self {
        self.update_depth = update_depth;
        self
    }

    pub(super) fn update_reuse_scope_for(&self, importer_id: &str) -> &UpdateReuseScope {
        if matches!(self.update_reuse_scope, UpdateReuseScope::None) {
            return &self.update_reuse_scope;
        }
        self.update_reuse_scopes_by_importer.get(importer_id).unwrap_or(&self.update_reuse_scope)
    }

    #[must_use]
    pub fn with_pnpmfile_hook(mut self, pnpmfile_hook: Option<Arc<dyn PnpmfileHooks>>) -> Self {
        self.pnpmfile_hook = pnpmfile_hook;
        self
    }

    /// Attach the `context.log(...)` sink the `pnpmfile_hook`'s
    /// `readPackage` calls forward to. The install layer pre-binds the
    /// reporter, project prefix, and pnpmfile path into the closure so the
    /// resolver stays reporter-agnostic.
    #[must_use]
    pub fn with_read_package_log(mut self, read_package_log: Option<pnpm_hooks::LogFn>) -> Self {
        self.read_package_log = read_package_log;
        self
    }

    /// Attach the sink skipped-optional-dependency notifications are
    /// forwarded to. See [`SkippedOptionalLogFn`].
    #[must_use]
    pub fn with_skipped_optional_log(
        mut self,
        skipped_optional_log: Option<SkippedOptionalLogFn>,
    ) -> Self {
        self.skipped_optional_log = skipped_optional_log;
        self
    }

    /// Attach the finalized-package sink. See [`FinalizedPackageFn`].
    #[must_use]
    pub fn with_finalized_package(mut self, finalized_package: Option<FinalizedPackageFn>) -> Self {
        self.finalized_package = finalized_package;
        self
    }

    /// Attach the `pnpm.allowedDeprecatedVersions` map. See
    /// [`crate::WorkspaceResolveOptions::allowed_deprecated_versions`].
    #[must_use]
    pub fn with_allowed_deprecated_versions(
        mut self,
        allowed_deprecated_versions: BTreeMap<String, String>,
    ) -> Self {
        self.allowed_deprecated_versions = allowed_deprecated_versions;
        self
    }

    /// Attach the sink deprecation notifications are forwarded to.
    /// See [`DeprecationLogFn`].
    #[must_use]
    pub fn with_deprecation_log(mut self, deprecation_log: Option<DeprecationLogFn>) -> Self {
        self.deprecation_log = deprecation_log;
        self
    }

    /// Set the install's `autoInstallPeers` flag. See the field doc.
    #[must_use]
    pub fn with_auto_install_peers(mut self, auto_install_peers: bool) -> Self {
        self.auto_install_peers = auto_install_peers;
        self
    }

    /// Attach the registry facts. See the `registry_context` field.
    #[must_use]
    pub fn with_registry_context(mut self, registry_context: RegistryContext) -> Self {
        self.registry_context = registry_context;
        self
    }

    /// Take ownership of `self` and emit the final [`ResolvedTree`].
    /// Pacquet's single-importer path consumes the context via
    /// [`TreeCtx::into_resolved_tree`], which routes through here once
    /// the last `Arc<WorkspaceTreeCtx>` reference is the [`TreeCtx`]'s
    /// own.
    pub fn into_resolved_tree(mut self, direct: Vec<DirectDep>) -> ResolvedTree {
        let tree = ResolvedTree {
            direct,
            packages: take_locked(&mut self.packages),
            dependencies_tree: take_locked(&mut self.dependencies_tree),
            all_peer_dep_names: take_locked(&mut self.all_peer_dep_names),
            policy_violations: take_locked(&mut self.policy_violations),
            applied_patches: take_locked(&mut self.applied_patches),
            children_by_id: take_locked(&mut self.children_by_id)
                .into_iter()
                .map(|(pkg_id, recorded)| (pkg_id, recorded.edges))
                .collect(),
        };
        // The per-edge dedup caches hold an entry per resolved wanted
        // dependency; freeing a workspace-scale map costs long enough
        // to show up in the install's tail, and nothing reads them
        // again, so a background thread takes the drop off the
        // critical path.
        let dedup_caches = (
            take_locked(&mut self.resolved_by_wanted),
            take_locked(&mut self.resolved_workspace_by_wanted),
            take_locked(&mut self.resolved_workspace_final_by_wanted),
            take_locked(&mut self.children_specs_by_id),
        );
        pnpm_fs::background_drop(dedup_caches);
        tree
    }
}

/// Fold one `name → version` pair into `versions` as a plain
/// `version` selector; the first fold of a pair wins over later ones.
fn fold_version(
    versions: &mut pnpm_resolving_resolver_base::PreferredVersions,
    name: String,
    version: String,
) {
    versions.entry(name).or_default().entry(version).or_insert(
        pnpm_resolving_resolver_base::VersionSelectorEntry::Plain(
            pnpm_resolving_resolver_base::VersionSelectorType::Version,
        ),
    );
}

pub(super) struct ChildrenOwnerClaim {
    pub(super) owner: ChildrenOwner,
    pub(super) owns_children: bool,
    /// The shadowed-dependency set in force for the package id — this
    /// occurrence's when it won the claim, the standing owner's when it
    /// lost. See [`ChildrenOwnerEntry::peer_shadowed`].
    pub(super) peer_shadowed: Arc<HashSet<String>>,
    /// A winning claim displaced an owner whose shadowed-dependency set
    /// equals this occurrence's, so the other occurrences' realized
    /// children — including a subtree reused from the prior lockfile —
    /// stay valid *as far as the claim can tell*, and the winner skips
    /// the lazy-flip (and the engine-invalidating rewrite signal) it
    /// would otherwise broadcast. The walk it then runs can still land
    /// on different edges, which
    /// [`ChildrenRecording::PublishedOverStale`] reports instead.
    ///
    /// This is a narrower question than whether the *recorded* children
    /// can be reused instead of walked; that one is
    /// [`fn@recorded_children_match`], which compares the full
    /// resolution context.
    pub(super) children_context_unchanged: bool,
}

/// `peer_shadowed` is this occurrence's own set; it is installed as the
/// package id's set only when the occurrence wins the claim, so the
/// losing occurrences of a concurrent claim all read back the winner's.
pub(super) fn claim_children_owner(
    ctx: &TreeCtx,
    pkg_id: &str,
    depth: i32,
    ancestor_ids: &[String],
    peer_shadowed: HashSet<String>,
) -> ChildrenOwnerClaim {
    let owner = ChildrenOwner {
        update_active: !matches!(ctx.update_reuse_scope(), UpdateReuseScope::All),
        depth,
        importer_order: ctx.importer_order,
        parent_path: ancestor_ids.to_vec(),
        importer_id: ctx.importer_id.clone(),
    };
    let (owns_children, peer_shadowed, children_context_unchanged) = {
        let mut owners = lock_recoverable(&ctx.workspace.children_owner_by_id);
        match owners.get(pkg_id) {
            Some(existing) if !owner.wins_over(&existing.owner) => {
                (false, Arc::clone(&existing.peer_shadowed), false)
            }
            existing => {
                let children_context_unchanged =
                    existing.is_some_and(|entry| *entry.peer_shadowed == peer_shadowed);
                let peer_shadowed = Arc::new(peer_shadowed);
                owners.insert(
                    Arc::from(pkg_id),
                    ChildrenOwnerEntry {
                        owner: owner.clone(),
                        peer_shadowed: Arc::clone(&peer_shadowed),
                    },
                );
                (true, peer_shadowed, children_context_unchanged)
            }
        }
    };
    if owns_children {
        let mut first_importer = lock_recoverable(&ctx.workspace.first_importer_by_pkg);
        if first_importer.map().get(pkg_id) != Some(&owner.importer_id) {
            first_importer.map_mut().insert(pkg_id.to_string(), owner.importer_id.clone());
        }
    }
    ChildrenOwnerClaim { owner, owns_children, peer_shadowed, children_context_unchanged }
}

/// Whether this occurrence is the first to offer to warm its package's
/// children, so the speculative resolutions run once per package
/// rather than once per occurrence of it.
pub(super) fn claim_children_warmup(ctx: &TreeCtx, pkg_id: &str) -> bool {
    let mut warmed = lock_recoverable(&ctx.workspace.warmed_children_by_id);
    // Every occurrence of a package offers, and all but the first are
    // turned away, so the owned key is built only for the one that
    // takes the warmup.
    !warmed.contains(pkg_id) && warmed.insert(Arc::from(pkg_id.to_string()))
}

/// Whether this package's recorded children were resolved under
/// `context`, and can therefore be expanded from instead of walked
/// again. The walk that recorded them need not be the one owning the
/// children now, which is why the comparison is against the recorded
/// context rather than against the standing claim.
pub(super) fn recorded_children_match(
    ctx: &TreeCtx,
    pkg_id: &str,
    context: &RecordedChildrenContext,
) -> bool {
    lock_recoverable(&ctx.workspace.children_by_id).get(pkg_id).is_some_and(|recorded| {
        recorded.context.produces_same_children_as(context)
            || recorded.context.pins_children_over(context)
    })
}

/// What [`fn@record_children`] did with a walk's child edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChildrenRecording {
    /// This walk's ownership lapsed before it could publish, so the
    /// standing owner's children stand. Its node stays lazy and expands
    /// from whatever that owner recorded.
    Declined,
    /// Published, and the realized children every other occurrence node
    /// holds still stand.
    Published,
    /// Published over edges the other occurrence nodes realized, whose
    /// children are now stale.
    PublishedOverStale,
}

impl ChildrenRecording {
    /// The children to hang on the recording walk's own node, plus
    /// whether the recording staled the children the package's other
    /// occurrence nodes realized — the flag that gates
    /// [`fn@make_non_owner_nodes_lazy`].
    pub(super) fn into_children(
        self,
        realized: BTreeMap<String, NodeId>,
        parent_ids: &Arc<Vec<String>>,
    ) -> (crate::resolved_tree::TreeChildren, bool) {
        match self {
            ChildrenRecording::Declined => (lazy_children(parent_ids), false),
            ChildrenRecording::Published => {
                (crate::resolved_tree::TreeChildren::Realized(std::sync::Arc::new(realized)), false)
            }
            ChildrenRecording::PublishedOverStale => {
                (crate::resolved_tree::TreeChildren::Realized(std::sync::Arc::new(realized)), true)
            }
        }
    }
}

/// Children a node expands from the standing owner's recording, under
/// its own `parent_ids` cycle break.
pub(super) fn lazy_children(parent_ids: &Arc<Vec<String>>) -> crate::resolved_tree::TreeChildren {
    crate::resolved_tree::TreeChildren::Lazy {
        parent_ids: AncestorIds::from(Arc::clone(parent_ids)),
    }
}

/// Publish a package's children together with the context that
/// produced them, and report what that did.
///
/// The ownership check and the comparison against the standing
/// recording happen under the same lock as the write: a claim that
/// landed while this walk ran has its own children to publish, and an
/// older walk finishing afterwards would otherwise overwrite them.
pub(super) fn record_children(
    ctx: &TreeCtx,
    pkg_id: &str,
    owner: &ChildrenOwner,
    edges: Vec<crate::resolved_tree::ChildEdge>,
    context: RecordedChildrenContext,
) -> ChildrenRecording {
    let recording = {
        let owners = lock_recoverable(&ctx.workspace.children_owner_by_id);
        if owners.get(pkg_id).is_none_or(|entry| entry.owner != *owner) {
            return ChildrenRecording::Declined;
        }
        let mut children = lock_recoverable(&ctx.workspace.children_by_id);
        let recording = match children.get(pkg_id) {
            // Nothing recorded yet, so no occurrence node can hold
            // realized children of this package to stale.
            None => ChildrenRecording::Published,
            // A recording the prior lockfile pinned outlives a fresh
            // walk's answer, so this walk publishes nothing and reads
            // the pinned children like every occurrence that reused the
            // subtree. Publishing over them would re-resolve the open
            // ranges reuse exists to hold still, and would leave those
            // occurrences realizing children the record no longer
            // holds. This comes before the equal-edge arm because
            // republishing even the same edges would carry this walk's
            // unpinned context onto the record, leaving the next fresh
            // walk to land on different edges nothing to hold it back.
            Some(recorded) if recorded.context.pins_children_over(&context) => {
                return ChildrenRecording::Declined;
            }
            Some(recorded) if *recorded.edges == edges => ChildrenRecording::Published,
            Some(_) => ChildrenRecording::PublishedOverStale,
        };
        let edges = Arc::new(edges);
        if ctx.workspace.finalized_package.is_some() {
            update_parent_index(
                &mut lock_recoverable(&ctx.workspace.parents_by_id),
                pkg_id,
                children.get(pkg_id).map(|recorded| recorded.edges.as_slice()),
                &edges,
            );
        }
        children.insert(Arc::from(pkg_id.to_string()), RecordedChildren { edges, context });
        recording
    };
    ctx.workspace.record_children_by_id_write(pkg_id);
    ctx.workspace.note_finalization_candidate(pkg_id);
    recording
}

/// Move `pkg_id` in the reverse parent index from the children it
/// recorded before (`previous`) to the ones it records now (`next`).
/// Recording the same edges again is a no-op, and a child dropped
/// from the record no longer lists `pkg_id` as a parent.
fn update_parent_index(
    parents_by_id: &mut HashMap<Arc<str>, HashSet<Arc<str>>>,
    pkg_id: &str,
    previous: Option<&[crate::resolved_tree::ChildEdge]>,
    next: &[crate::resolved_tree::ChildEdge],
) {
    if previous.is_some_and(|previous| previous == next) {
        return;
    }
    let kept: HashSet<&str> = next.iter().map(|edge| edge.pkg_id.as_ref()).collect();
    for edge in previous.into_iter().flatten() {
        if kept.contains(edge.pkg_id.as_ref()) {
            continue;
        }
        if let Some(parents) = parents_by_id.get_mut(&edge.pkg_id) {
            parents.remove(pkg_id);
            if parents.is_empty() {
                parents_by_id.remove(&edge.pkg_id);
            }
        }
    }
    for edge in next {
        parents_by_id.entry(Arc::clone(&edge.pkg_id)).or_default().insert(Arc::from(pkg_id));
    }
}

/// Seed the peer-walker's `parentPkgs` filter with the names a
/// resolved package declares as peers.
pub(super) fn register_peer_dep_names(
    ctx: &TreeCtx,
    peer_dependencies: &BTreeMap<String, PeerDep>,
) {
    let mut all_peers = lock_recoverable(&ctx.workspace.all_peer_dep_names);
    for name in peer_dependencies.keys() {
        if all_peers.insert(name.clone()) {
            ctx.workspace.record_peer_dep_name(name);
        }
    }
}

pub(super) fn is_current_children_owner(
    ctx: &TreeCtx,
    pkg_id: &str,
    owner: &ChildrenOwner,
) -> bool {
    lock_recoverable(&ctx.workspace.children_owner_by_id)
        .get(pkg_id)
        .is_some_and(|current| current.owner == *owner)
}

pub(super) fn remember_node_parent_ids(
    ctx: &TreeCtx,
    node_id: &NodeId,
    parent_ids: Arc<Vec<String>>,
) {
    lock_recoverable(&ctx.workspace.node_parent_ids_by_id).insert(node_id.clone(), parent_ids);
}

/// Record an occurrence node in the shared tree (lowering the depth of
/// a revisited leaf) and, on first insertion, in the per-package
/// reverse index [`fn@make_non_owner_nodes_lazy`] flips through.
pub(super) fn insert_tree_node(
    ctx: &TreeCtx,
    node_id: NodeId,
    pkg_id: &str,
    children: crate::resolved_tree::TreeChildren,
    depth: i32,
) {
    let mut written = true;
    let inserted = match lock_recoverable(&ctx.workspace.dependencies_tree).entry(node_id.clone()) {
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            written = entry.get().depth > depth;
            if written {
                entry.get_mut().depth = depth;
            }
            false
        }
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(DependenciesTreeNode::new(
                Arc::from(pkg_id.to_string()),
                children,
                depth,
                true,
            ));
            true
        }
    };
    if written {
        ctx.workspace.record_tree_node_write(&node_id);
    }
    if inserted {
        lock_recoverable(&ctx.workspace.nodes_by_pkg_id)
            .entry(Arc::from(pkg_id.to_string()))
            .or_default()
            .push(node_id);
    }
}

pub(super) fn make_non_owner_nodes_lazy(ctx: &TreeCtx, pkg_id: &str, owner_node_id: &NodeId) {
    let pkg_nodes = match lock_recoverable(&ctx.workspace.nodes_by_pkg_id).get(pkg_id) {
        Some(nodes) => nodes.clone(),
        None => return,
    };
    // Collect the parent chains first so the two locks are never held
    // together.
    let parent_ids_by_node: Vec<(NodeId, Arc<Vec<String>>)> = {
        let parent_ids = lock_recoverable(&ctx.workspace.node_parent_ids_by_id);
        pkg_nodes
            .into_iter()
            .filter(|node_id| node_id != owner_node_id)
            .filter_map(|node_id| {
                let ids = Arc::clone(parent_ids.get(&node_id)?);
                Some((node_id, ids))
            })
            .collect()
    };
    let mut tree = lock_recoverable(&ctx.workspace.dependencies_tree);
    let mut rewritten = Vec::new();
    for (node_id, parent_ids) in parent_ids_by_node {
        // An occurrence already reading the owner's children needs no
        // rewrite — and must not report one, since the signal makes the
        // discovery engine rebuild from scratch. In a peer-heavy graph
        // most occurrences of a package are already lazy.
        if let Some(node) = tree.get_mut(&node_id)
            && !matches!(node.children, crate::resolved_tree::TreeChildren::Lazy { .. })
        {
            node.children = crate::resolved_tree::TreeChildren::Lazy {
                parent_ids: AncestorIds::from(parent_ids),
            };
            rewritten.push(node_id);
        }
    }
    drop(tree);
    let rewrote_any = !rewritten.is_empty();
    for node_id in &rewritten {
        ctx.workspace.record_tree_node_write(node_id);
    }
    if rewrote_any {
        ctx.workspace.record_children_rewrite();
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
