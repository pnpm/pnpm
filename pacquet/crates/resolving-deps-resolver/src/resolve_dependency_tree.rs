use async_recursion::async_recursion;
use chrono::{DateTime, Utc};
use derive_more::{Display, Error};
use futures_util::future;
use miette::Diagnostic;
use pacquet_catalogs_resolver::{
    CatalogResolutionError, CatalogResolutionResult, WantedDependency as CatalogWantedDependency,
    resolve_from_catalog,
};
use pacquet_catalogs_types::Catalogs;
use pacquet_hooks::PnpmfileHooks;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_patching::{PatchGroupRecord, PatchKeyConflictError, get_patch_info};
use pacquet_resolving_resolver_base::{ResolveError, ResolveOptions, Resolver, WantedDependency};
use pipe_trait::Pipe;
use serde_json::Value;
use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
};

/// Acquire a [`Mutex`] guard, recovering from poisoning the same way
/// the rest of pacquet does (`build_modules.rs`, `pick_package.rs`,
/// ...). The mutexes guarded by this helper hold short `HashMap` /
/// `HashSet` inserts with no invariants that survive a panic, so the
/// install can keep going after the unrelated panic that poisoned
/// the lock — better than escalating into a hard install-wide
/// failure.
fn lock_recoverable<Inner>(mutex: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

use crate::{
    lockfile_reuse::{reusable_importer_dep, synthesize_reused_result},
    node_id::NodeId,
    resolved_tree::{DependenciesTreeNode, DirectDep, PeerDep, ResolvedPackage, ResolvedTree},
};
use pacquet_lockfile::{PkgNameVerPeer, SnapshotEntry};

/// Which dependencies `pacquet update` excludes from lockfile-resolution
/// reuse. An excluded package re-resolves to highest-in-range, and its
/// whole subtree re-resolves with it (so the bump's new transitive deps
/// are picked up). Mirrors pnpm's `update` re-resolution scope.
#[derive(Default, Clone)]
pub enum UpdateReuseScope {
    /// Reuse every still-satisfied dependency. `install` / `add`.
    #[default]
    All,
    /// Reuse nothing — the whole graph re-resolves. `pacquet update`
    /// with no selectors.
    None,
    /// Reuse everything except the named packages (matched at any depth).
    /// `pacquet update <pattern>`.
    Except(std::collections::HashSet<String>),
}

/// How the current [`fn@resolve_node`] call may reuse the prior
/// lockfile's resolution instead of re-resolving from the registry.
///
/// Threaded down the recursion to faithfully port pnpm's
/// `resolvedDependencies` / `parentPkg.updated` mechanism
/// (`resolveChildren` / `getDepsToResolve` in
/// [`resolveDependencies.ts`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1000-L1248)).
#[derive(Clone)]
enum ReuseSource {
    /// A direct dependency of importer `importer_id`. Reuse matches the
    /// manifest specifier against the importer's recorded resolution via
    /// semver-satisfies ([`reusable_importer_dep`]).
    Importer { importer_id: String },
    /// A transitive dependency whose resolved snapshot key the parent's
    /// snapshot already pins. `Some` reuses that key directly (no semver
    /// check — the parent version pins it); `None` means an updated
    /// ancestor discarded its child-refs, forcing this subtree to
    /// re-resolve (pnpm's `parentPkg.updated ? undefined : refs`).
    Transitive { key: Option<PkgNameVerPeer> },
    /// Reuse disabled for this node (no prior lockfile).
    Off,
}

/// Options threaded into [`fn@resolve_dependency_tree`].
///
/// Mirrors upstream's per-importer options; pacquet's slice is single-
/// importer so the bag is smaller. `base_opts` is the [`ResolveOptions`]
/// every per-package `resolve()` call sees; the tree walker doesn't
/// mutate it.
///
/// Peer auto-installation lives one layer up in
/// [`fn@crate::resolve_importer`] — this entry point is a pure tree walker
/// over the manifest's explicit dependencies plus their transitive
/// children. The orchestrator extends the same tree with hoisted peers
/// via [`extend_tree`].
pub struct ResolveDependencyTreeOptions {
    pub base_opts: ResolveOptions,
    pub patched_dependencies: Option<Arc<PatchGroupRecord>>,
    pub manifest_hook: Option<ManifestHook>,
    pub pnpmfile_hook: Option<Arc<dyn PnpmfileHooks>>,
    /// `context.log(...)` sink for the `pnpmfile_hook`'s `readPackage`
    /// calls. `None` leaves hook logging a no-op. See
    /// [`WorkspaceTreeCtx::with_read_package_log`].
    pub read_package_log: Option<pacquet_hooks::LogFn>,
}

impl std::fmt::Debug for ResolveDependencyTreeOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolveDependencyTreeOptions")
            .field("base_opts", &self.base_opts)
            .field("patched_dependencies", &self.patched_dependencies)
            .field("manifest_hook", &self.manifest_hook.as_ref().map(|_| "<hook>"))
            .field("pnpmfile_hook", &self.pnpmfile_hook.as_ref().map(|_| "<hook>"))
            .field("read_package_log", &self.read_package_log.as_ref().map(|_| "<log>"))
            .finish()
    }
}

/// Per-manifest mutation applied to every resolved package's
/// manifest before downstream consumers (children walk, peer
/// extraction, lockfile build) see it. Takes the `Arc<Value>` the
/// resolver returned and yields either the same `Arc` (no-op) or a
/// fresh `Arc` carrying a deep-cloned + extended manifest.
///
/// Mirrors upstream's
/// [`ReadPackageHook`](https://github.com/pnpm/pnpm/blob/39101f5e37/hooks/types/src/index.ts)
/// signature collapsed to the only field pacquet currently touches
/// (the manifest). Threaded into [`TreeCtx`] so a single
/// `Arc::clone` reaches every recursive call.
pub type ManifestHook = Arc<dyn Fn(Arc<Value>) -> Arc<Value> + Send + Sync>;

/// Error envelope returned by the tree walker.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum ResolveDependencyTreeError {
    /// One of the resolver chain calls failed (network, parse, etc.).
    /// The inner error is the boxed type the resolver returned.
    #[display("Failed to resolve dependency: {_0}")]
    Resolve(#[error(not(source))] String),

    /// No resolver in the chain claimed the spec. Mirrors pnpm's
    /// [`SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`](https://github.com/pnpm/pnpm/blob/097983fbca/resolving/default-resolver/src/index.ts#L148-L156).
    #[display("\"{specifier}\" isn't supported by any available resolver.")]
    #[diagnostic(code(SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER))]
    SpecNotSupported {
        #[error(not(source))]
        specifier: String,
    },

    /// A `catalog:` specifier on a direct dependency referenced a
    /// missing entry, used a forbidden protocol, or was otherwise
    /// misconfigured. The inner error carries the upstream
    /// `ERR_PNPM_CATALOG_ENTRY_*` code and message.
    #[diagnostic(transparent)]
    CatalogMisconfiguration(#[error(source)] CatalogResolutionError),

    /// `patchedDependencies` configured more than one version range that
    /// satisfies the same `name@version` and the user did not break the
    /// tie with an exact-version entry. Propagated verbatim from
    /// [`pacquet_patching::get_patch_info`].
    #[display("{_0}")]
    #[diagnostic(transparent)]
    PatchKeyConflict(#[error(source)] PatchKeyConflictError),

    /// A transitive dependency was resolved through an exotic
    /// protocol (git, tarball, file, ...) while `block_exotic_subdeps`
    /// is on. Mirrors pnpm's
    /// [`EXOTIC_SUBDEP`](https://github.com/pnpm/pnpm/blob/df990fdb51/installing/deps-resolver/src/resolveDependencies.ts#L1420-L1434).
    #[display(
        "Exotic dependency \"{specifier}\" (resolved via {resolved_via}) is not allowed in subdependencies when blockExoticSubdeps is enabled"
    )]
    #[diagnostic(code(EXOTIC_SUBDEP))]
    ExoticSubdep {
        #[error(not(source))]
        specifier: String,
        resolved_via: String,
    },

    /// A dependency alias contained a path-separator segment that would
    /// escape the intended `node_modules` directory when joined onto a
    /// modules path. Mirrors pnpm's
    /// [`INVALID_DEPENDENCY_NAME`](https://github.com/pnpm/pnpm/blob/main/installing/deps-resolver/src/validateDependencyAlias.ts).
    #[display(
        "{parent} contains a dependency with an invalid name: {alias:?}. Dependency names must be a single package name or \"@scope/name\" — they cannot contain path-separator segments such as \"..\"."
    )]
    #[diagnostic(code(INVALID_DEPENDENCY_NAME))]
    InvalidDependencyName {
        #[error(not(source))]
        parent: String,
        alias: String,
    },

    /// A pnpmfile hook (`readPackage`) threw, timed out, or returned an
    /// invalid package manifest. Mirrors pnpm's `PNPMFILE_FAIL` /
    /// `BAD_READ_PACKAGE_HOOK_RESULT`: a bad hook aborts the install.
    #[display("{_0}")]
    #[diagnostic(code(PNPMFILE_FAIL))]
    PnpmfileHook(#[error(not(source))] pacquet_hooks::HookError),
}

impl From<PatchKeyConflictError> for ResolveDependencyTreeError {
    fn from(err: PatchKeyConflictError) -> Self {
        ResolveDependencyTreeError::PatchKeyConflict(err)
    }
}

/// Walk `manifest` plus the entries in `dependency_groups`, dispatch
/// each direct dep through `resolver`, recurse on each picked
/// package's own `dependencies`, and return a [`ResolvedTree`] that
/// carries both the flat dedup map (`packages`) and the per-occurrence
/// tree (`dependencies_tree`).
///
/// Mirrors upstream's
/// [`resolveDependencyTree`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencyTree.ts#L172-L357)
/// for the npm-shaped slice pacquet currently exposes.
///
/// Resolves siblings in parallel via `try_join_all` at every level.
/// The per-package dedupe gate is a shared `HashMap` behind a
/// [`std::sync::Mutex`]: a second visitor to the same resolved id `X`
/// AND-folds its `optional` flag into the existing
/// [`ResolvedPackage`] envelope and reuses it. It still allocates a
/// fresh [`DependenciesTreeNode`] for the current occurrence and
/// recurses on `X`'s children — only the resolver-side envelope is
/// shared. The critical sections are short `HashMap` inserts with no
/// `await` inside, so a sync mutex is the right tool — tokio's async
/// mutex adds per-acquire overhead that the resolve hot path was
/// paying once per visit per ctx field.
pub async fn resolve_dependency_tree<DependencyGroupList, Chain>(
    resolver: &Chain,
    manifest: &PackageManifest,
    dependency_groups: DependencyGroupList,
    opts: ResolveDependencyTreeOptions,
) -> Result<ResolvedTree, ResolveDependencyTreeError>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
    Chain: Resolver + ?Sized,
{
    let ctx = TreeCtx::new(opts.base_opts)
        .with_patched_dependencies(opts.patched_dependencies)
        .with_manifest_hook(opts.manifest_hook)
        .with_pnpmfile_hook(opts.pnpmfile_hook)
        .with_read_package_log(opts.read_package_log);
    let optional_names = importer_optional_dependency_names(manifest);
    let injected_names = importer_injected_dependency_names(manifest);
    let mut wanted: Vec<WantedSpec> = Vec::new();
    for (name, range) in manifest.dependencies(dependency_groups) {
        if !crate::is_valid_dependency_alias(name) {
            return Err(ResolveDependencyTreeError::InvalidDependencyName {
                parent: "The current package".to_string(),
                alias: name.to_string(),
            });
        }
        let optional = optional_names.contains(name);
        let injected = injected_names.contains(name);
        wanted.push((name.to_string(), range.to_string(), optional, injected));
    }
    let direct =
        extend_tree(&ctx, resolver, wanted, pacquet_lockfile::Lockfile::ROOT_IMPORTER_KEY).await?;
    Ok(ctx.into_resolved_tree(direct))
}

/// Collect the names of the importer manifest's `optionalDependencies`
/// entries so the walker can tag each direct dep with the right
/// `wanted.optional` flag. Mirrors upstream's per-alias classification
/// in [`getWantedDependenciesFromGivenSet`](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-resolver/src/getWantedDependencies.ts#L57-L72):
/// `optionalDependencies` wins over the other groups when an alias
/// appears in more than one. Pacquet builds the same set so the
/// `ResolvedPackage.optional` propagation starts from the right
/// per-direct-dep value.
pub(crate) fn importer_optional_dependency_names(manifest: &PackageManifest) -> HashSet<String> {
    manifest.dependencies([DependencyGroup::Optional]).map(|(name, _)| name.to_string()).collect()
}

/// Collect the names of the importer manifest's `dependenciesMeta` entries
/// whose `injected` flag is `true`. Mirrors upstream's per-alias
/// [`injected: opts.dependenciesMeta[alias]?.injected`](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-resolver/src/getWantedDependencies.ts#L73)
/// thread — the per-dep opt-in that flips a workspace dep onto the
/// hard-linked `file:` path even when the global
/// `injectWorkspacePackages` is off. Only importer-level deps are
/// consulted; the recursive walker does not inherit this from any
/// resolved package's own `dependenciesMeta`, matching
/// upstream's importer-only scope.
pub(crate) fn importer_injected_dependency_names(manifest: &PackageManifest) -> HashSet<String> {
    let Some(meta) =
        manifest.value().get("dependenciesMeta").and_then(serde_json::Value::as_object)
    else {
        return HashSet::new();
    };
    meta.iter()
        .filter(|(_, entry)| {
            entry.get("injected").and_then(serde_json::Value::as_bool).unwrap_or(false)
        })
        .map(|(name, _)| name.clone())
        .collect()
}

/// Build the importer's direct-dependency wanted specs: the manifest's
/// `dependencies` (plus, when `auto_install_peers`, its own
/// `peerDependencies`) tagged with the right `optional` / `injected`
/// flags and with `catalog:` specifiers resolved.
///
/// Shared by [`fn@crate::resolve_importer`] (which walks them) and the
/// `time-based` cutoff pre-pass in [`fn@crate::resolve_workspace`]
/// (which only needs the resolved direct-dep publish dates), so both
/// see the identical direct-dep set. Mirrors the importer-dep
/// computation pnpm runs once in
/// [`getAllDependenciesFromManifest`](https://github.com/pnpm/pnpm/blob/097983fbca/pkg-manifest/utils/src/getAllDependenciesFromManifest.ts)
/// before resolving an importer's deps.
pub(crate) fn importer_direct_wanted_specs<DependencyGroupList>(
    manifest: &PackageManifest,
    dependency_groups: DependencyGroupList,
    auto_install_peers: bool,
    catalogs: &Catalogs,
) -> Result<Vec<WantedSpec>, ResolveDependencyTreeError>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    let mut groups: Vec<DependencyGroup> = dependency_groups.into_iter().collect();
    if auto_install_peers && !groups.contains(&DependencyGroup::Peer) {
        groups.push(DependencyGroup::Peer);
    }
    let optional_names = importer_optional_dependency_names(manifest);
    let injected_names = importer_injected_dependency_names(manifest);
    let mut wanted: Vec<WantedSpec> = Vec::new();
    for (name, range) in manifest.dependencies(groups) {
        if !crate::is_valid_dependency_alias(name) {
            return Err(ResolveDependencyTreeError::InvalidDependencyName {
                parent: "The current package".to_string(),
                alias: name.to_string(),
            });
        }
        let optional = optional_names.contains(name);
        let injected = injected_names.contains(name);
        wanted.push((name.to_string(), range.to_string(), optional, injected));
    }
    resolve_catalog_specifiers(wanted, catalogs)
}

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
/// based on `wanted.optional` ([`pickPackage.ts:391`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L201)) —
/// caching by `(alias, bare_specifier)` alone would let an optional
/// caller satisfy itself with a non-optional caller's abbreviated
/// result, losing the `libc`/`cpu`/`os` filter inputs that mode
/// supplies.
///
/// `injected` is part of the key because the workspace branch of the
/// npm resolver returns a `file:<path>` resolution when the dep is
/// injected and a `link:<path>` resolution otherwise (see
/// [`resolve_from_local_package`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/npm-resolver/src/index.ts#L908-L951)).
/// Two importers asking for the same workspace dep with different
/// `dependenciesMeta[*].injected` flags must take different cache
/// slots.
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
/// `project_dir` is part of the key only for project-relative
/// specifiers (`link:` / `file:` / `workspace:`). A non-injected
/// workspace dep resolves through
/// [`resolve_from_local_package`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/npm-resolver/src/index.ts#L908-L951)
/// to a `link:<path>` whose `<path>` is computed *relative to the
/// consuming importer's directory*. Without `project_dir` in the key,
/// the first importer to resolve `(@scope/lib, workspace:*)` would
/// seed the workspace-wide cache with its own relative path and every
/// other importer would reuse it verbatim — e.g. a root resolving to
/// `link:packages/lib` would hand `packages/app` the same string,
/// which from `packages/app` points at the non-existent
/// `packages/app/packages/lib`. Registry specifiers are
/// importer-independent, so they keep `None` and stay shared across
/// importers (the cross-importer dedup the cache exists for).
type WantedKey = (
    Option<String>,
    Option<String>,
    Option<bool>,
    Option<bool>,
    bool,
    Option<DateTime<Utc>>,
    Option<PathBuf>,
);

/// Whether a wanted dep's resolution is computed relative to the
/// consuming importer's directory rather than being
/// importer-independent. True for the `link:` / `file:` / `workspace:`
/// protocols, whose resolved path
/// [`resolve_from_local_package`](https://github.com/pnpm/pnpm/blob/ef87f3ccff/resolving/npm-resolver/src/index.ts#L908-L951)
/// derives from `project_dir`. Such resolutions must not be shared
/// across importers in [`WantedKey`].
fn is_project_relative_specifier(bare_specifier: Option<&str>) -> bool {
    bare_specifier.is_some_and(|spec| {
        spec.starts_with("link:") || spec.starts_with("file:") || spec.starts_with("workspace:")
    })
}

/// One spec carried through [`extend_tree`] and the importer-side
/// orchestrator: `(alias, range, optional, injected)`. `injected`
/// reflects the importer manifest's `dependenciesMeta[alias].injected`
/// flag, threaded onto [`WantedDependency::injected`] so the workspace
/// resolver branch picks the `file:` resolution shape for that one
/// dep even when the global [`ResolveOptions::inject_workspace_packages`]
/// is off. Hoisted-peer arms in
/// [`fn@crate::resolve_importer::resolve_importer`] default this to
/// `false` — peers picked up via auto-install don't carry per-dep
/// meta from any manifest.
pub(crate) type WantedSpec = (String, String, bool, bool);

/// One entry in [`WorkspaceTreeCtx`]'s `children_specs_by_id` map —
/// `(child_alias, child_range, child_optional)` triples extracted from
/// a resolved package's manifest's `dependencies` plus
/// `optionalDependencies` sections.
type ChildSpec = (String, String, bool);

/// Workspace-shared maps. Every per-importer [`TreeCtx`] in a
/// multi-importer install holds an `Arc<WorkspaceTreeCtx>` so the
/// resolver's per-`pkgIdWithPatchHash` dedup (`packages`,
/// `children_specs_by_id`, `children_by_id`, `resolved_by_wanted`) and
/// the peer-walker's seed sets (`all_peer_dep_names`,
/// `applied_patches`, `policy_violations`) carry across importers.
/// Mirrors the single shared `ctx` pnpm's
/// [`resolveDependencyTree`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-resolver/src/resolveDependencyTree.ts#L180-L233)
/// hands to every importer's hoist loop.
///
/// `dependencies_tree` (`NodeId → DependenciesTreeNode`) is keyed by
/// per-occurrence `NodeIds`, which are unique even across importers, so
/// every importer's walk contributes entries to one combined tree
/// without colliding.
pub struct WorkspaceTreeCtx {
    packages: Mutex<HashMap<String, ResolvedPackage>>,
    dependencies_tree: Mutex<HashMap<NodeId, DependenciesTreeNode>>,
    all_peer_dep_names: Mutex<HashSet<String>>,
    policy_violations: Mutex<Vec<pacquet_resolving_resolver_base::ResolutionPolicyViolation>>,
    applied_patches: Mutex<HashSet<String>>,
    resolved_by_wanted:
        Mutex<HashMap<WantedKey, Arc<pacquet_resolving_resolver_base::ResolveResult>>>,
    children_specs_by_id: Mutex<HashMap<String, Arc<Vec<ChildSpec>>>>,
    children_by_id: Mutex<HashMap<String, Arc<Vec<crate::resolved_tree::ChildEdge>>>>,
    manifest_hook: Option<ManifestHook>,
    /// The previous `pnpm-lock.yaml` the install started from, when one
    /// exists. Consulted by `resolve_node` to reuse an already-resolved
    /// dependency + its transitive subtree instead of re-resolving from
    /// the registry (see `pacquet/plans/LOCKFILE_RESOLUTION_REUSE.md`).
    /// `None` on a first install or when reuse is disabled.
    wanted_lockfile: Option<Arc<pacquet_lockfile::Lockfile>>,
    /// Lockfile-reuse suppression for `pacquet update`. `update`
    /// re-resolves its target deps to highest-in-range, so a reused
    /// resolution would defeat the bump. Mirrors pnpm's `updateToLatest`
    /// / `updateMatching` propagation into `parentPkg.updated`. See
    /// [`UpdateReuseScope`].
    update_reuse_scope: UpdateReuseScope,
    /// Memoises [`fn@subtree_fully_reusable`] per snapshot key so the
    /// recursive reusability check runs once per package across the
    /// whole walk. `true` means the package and its entire transitive
    /// subtree can be synthesized from the prior lockfile.
    subtree_reusable: Mutex<HashMap<PkgNameVerPeer, bool>>,
    pnpmfile_hook: Option<Arc<dyn PnpmfileHooks>>,
    /// `context.log(...)` sink for the `pnpmfile_hook`'s `readPackage`
    /// calls, pre-bound to the install's reporter, project prefix, and
    /// pnpmfile path. `None` leaves hook logging a no-op. See
    /// [`WorkspaceTreeCtx::with_read_package_log`].
    read_package_log: Option<pacquet_hooks::LogFn>,
}

impl Default for WorkspaceTreeCtx {
    fn default() -> Self {
        WorkspaceTreeCtx {
            packages: Mutex::new(HashMap::new()),
            dependencies_tree: Mutex::new(HashMap::new()),
            all_peer_dep_names: Mutex::new(HashSet::new()),
            policy_violations: Mutex::new(Vec::new()),
            applied_patches: Mutex::new(HashSet::new()),
            resolved_by_wanted: Mutex::new(HashMap::new()),
            children_specs_by_id: Mutex::new(HashMap::new()),
            children_by_id: Mutex::new(HashMap::new()),
            manifest_hook: None,
            wanted_lockfile: None,
            update_reuse_scope: UpdateReuseScope::All,
            subtree_reusable: Mutex::new(HashMap::new()),
            pnpmfile_hook: None,
            read_package_log: None,
        }
    }
}

impl WorkspaceTreeCtx {
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
            children_by_id: lock_recoverable(&self.children_by_id).clone(),
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

    /// Attach the prior `pnpm-lock.yaml` so `resolve_node` can reuse
    /// already-resolved dependencies instead of re-resolving them. See
    /// the `wanted_lockfile` field.
    #[must_use]
    pub fn with_wanted_lockfile(
        mut self,
        wanted_lockfile: Option<Arc<pacquet_lockfile::Lockfile>>,
    ) -> Self {
        self.wanted_lockfile = wanted_lockfile;
        self
    }

    /// The prior `pnpm-lock.yaml` to reuse resolutions from, if any.
    pub fn wanted_lockfile(&self) -> Option<&Arc<pacquet_lockfile::Lockfile>> {
        self.wanted_lockfile.as_ref()
    }

    /// Set which dependencies `pacquet update` excludes from reuse. See
    /// [`UpdateReuseScope`].
    #[must_use]
    pub fn with_update_reuse_scope(mut self, scope: UpdateReuseScope) -> Self {
        self.update_reuse_scope = scope;
        self
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
    pub fn with_read_package_log(mut self, read_package_log: Option<pacquet_hooks::LogFn>) -> Self {
        self.read_package_log = read_package_log;
        self
    }

    /// Take ownership of `self` and emit the final [`ResolvedTree`].
    /// Pacquet's single-importer path consumes the context via
    /// [`TreeCtx::into_resolved_tree`], which routes through here once
    /// the last `Arc<WorkspaceTreeCtx>` reference is the [`TreeCtx`]'s
    /// own.
    pub fn into_resolved_tree(self, direct: Vec<DirectDep>) -> ResolvedTree {
        ResolvedTree {
            direct,
            packages: self.packages.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner),
            dependencies_tree: self
                .dependencies_tree
                .into_inner()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            all_peer_dep_names: self
                .all_peer_dep_names
                .into_inner()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            policy_violations: self
                .policy_violations
                .into_inner()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            applied_patches: self
                .applied_patches
                .into_inner()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            children_by_id: self
                .children_by_id
                .into_inner()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        }
    }
}

/// Mutable workspace for an in-flight tree walk. The orchestrator
/// (`resolve_importer`) holds one of these across hoist iterations and
/// extends it via [`extend_tree`] so newly-hoisted peer dependencies
/// reuse the existing per-id dedup map instead of restarting the walk.
///
/// The shared per-`pkgIdWithPatchHash` dedup maps live on
/// [`WorkspaceTreeCtx`] behind an `Arc`. In single-importer mode this
/// `Arc` is sole-owned by [`TreeCtx`]; in multi-importer mode
/// `Arc::clone(&workspace)` is handed to every per-importer
/// [`TreeCtx`] so importer N's tree walk reuses importer M's resolved
/// envelopes via the shared maps.
pub struct TreeCtx {
    base_opts: ResolveOptions,
    /// [`ResolveOptions`] handed to the resolver for importer-level
    /// (direct) dependencies — `depth == 0`. Differs from `base_opts`
    /// only in `pick_lowest_version`, which is set under
    /// `resolutionMode: time-based` / `lowest-direct`. Built once per
    /// importer by [`Self::with_resolution_mode`].
    direct_opts: ResolveOptions,
    /// [`ResolveOptions`] handed to the resolver for transitive
    /// dependencies — `depth > 0`. Always picks highest; carries the
    /// `time-based` publish-date cutoff in `published_by`. Built once
    /// per importer by [`Self::with_resolution_mode`].
    subdep_opts: ResolveOptions,
    workspace: Arc<WorkspaceTreeCtx>,
    /// Configured `patchedDependencies` (already grouped by name).
    /// Shared by `Arc` so the lookup table doesn't get cloned per
    /// recursive call. `None` when no patches are configured for this
    /// install.
    patched_dependencies: Option<Arc<PatchGroupRecord>>,
}

impl TreeCtx {
    /// Construct a single-importer context with a fresh
    /// [`WorkspaceTreeCtx`]. The multi-importer orchestrator uses
    /// [`Self::with_workspace`] instead so per-importer contexts share
    /// the same workspace ctx.
    #[must_use]
    pub fn new(base_opts: ResolveOptions) -> Self {
        TreeCtx {
            direct_opts: base_opts.clone(),
            subdep_opts: base_opts.clone(),
            base_opts,
            workspace: Arc::new(WorkspaceTreeCtx::default()),
            patched_dependencies: None,
        }
    }

    /// Construct a per-importer context that shares its dedup maps
    /// with `workspace`. The caller is responsible for keeping
    /// `workspace` alive across importers (typically via
    /// `Arc::clone(&workspace)`).
    pub fn with_workspace(workspace: Arc<WorkspaceTreeCtx>, base_opts: ResolveOptions) -> Self {
        TreeCtx {
            direct_opts: base_opts.clone(),
            subdep_opts: base_opts.clone(),
            base_opts,
            workspace,
            patched_dependencies: None,
        }
    }

    /// Derive the depth-specific resolve options from `resolutionMode`.
    ///
    /// - `pick_lowest_direct` — resolve direct dependencies to their
    ///   lowest satisfying version (`time-based` / `lowest-direct`).
    /// - `subdep_published_by` — the publish-date cutoff applied to
    ///   transitive dependencies. Under `time-based` this is the
    ///   workspace-wide cutoff computed from the resolved direct deps
    ///   (clamped by `minimumReleaseAge`); otherwise it is just
    ///   `base_opts.published_by` (the `minimumReleaseAge` cutoff),
    ///   leaving subdep resolution unchanged.
    ///
    /// Mirrors pnpm's split between the importer-dep pick
    /// ([`resolveDependenciesOfImporters`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/installing/deps-resolver/src/resolveDependencies.ts#L470))
    /// and the subdep pick (always highest, constrained by the computed
    /// `publishedBy`).
    #[must_use]
    pub fn with_resolution_mode(
        mut self,
        pick_lowest_direct: bool,
        subdep_published_by: Option<DateTime<Utc>>,
    ) -> Self {
        self.direct_opts.pick_lowest_version = pick_lowest_direct;
        self.subdep_opts.pick_lowest_version = false;
        self.subdep_opts.published_by = subdep_published_by;
        self
    }

    /// The [`ResolveOptions`] to hand the resolver for a node at the
    /// given `depth`: importer-level deps (`depth == 0`) use
    /// [`Self::direct_opts`]; everything below uses
    /// [`Self::subdep_opts`].
    fn opts_for_depth(&self, depth: i32) -> &ResolveOptions {
        if depth == 0 { &self.direct_opts } else { &self.subdep_opts }
    }

    /// Borrow the shared workspace ctx so callers can hand the same
    /// `Arc::clone` to the next per-importer [`TreeCtx`].
    #[must_use]
    pub fn workspace(&self) -> &Arc<WorkspaceTreeCtx> {
        &self.workspace
    }

    /// Attach the install's `patchedDependencies` map. When `Some`,
    /// the per-node walker looks every resolved `name@version` up via
    /// [`get_patch_info`] and appends `(patch_hash=<hash>)` to the
    /// `pkgIdWithPatchHash` on a match.
    #[must_use]
    pub fn with_patched_dependencies(
        mut self,
        patched_dependencies: Option<Arc<PatchGroupRecord>>,
    ) -> Self {
        self.patched_dependencies = patched_dependencies;
        self
    }

    /// Attach a `readPackageHook` to the underlying [`WorkspaceTreeCtx`].
    /// `manifest_hook` is workspace-wide (one hook per install), so this
    /// passthrough relies on the workspace ctx being sole-owned —
    /// `TreeCtx::new` always satisfies that, and the multi-importer
    /// orchestrator [`fn@crate::resolve_workspace`] hands the hook in via
    /// [`WorkspaceTreeCtx::with_manifest_hook`] before sharing the
    /// `Arc`. Panics if the workspace ctx has already been cloned —
    /// callers must set the hook before sharing the context.
    #[must_use]
    pub fn with_manifest_hook(mut self, manifest_hook: Option<ManifestHook>) -> Self {
        Arc::get_mut(&mut self.workspace)
            .expect("with_manifest_hook called after the workspace ctx was shared via Arc::clone")
            .manifest_hook = manifest_hook;
        self
    }

    #[must_use]
    pub fn with_pnpmfile_hook(mut self, pnpmfile_hook: Option<Arc<dyn PnpmfileHooks>>) -> Self {
        Arc::get_mut(&mut self.workspace)
            .expect("with_pnpmfile_hook called after the workspace ctx was shared via Arc::clone")
            .pnpmfile_hook = pnpmfile_hook;
        self
    }

    /// Attach the `context.log(...)` sink the `pnpmfile_hook`'s
    /// `readPackage` calls forward to. Like [`Self::with_pnpmfile_hook`],
    /// this targets the underlying [`WorkspaceTreeCtx`] and panics if it
    /// has already been shared via `Arc::clone`.
    #[must_use]
    pub fn with_read_package_log(mut self, read_package_log: Option<pacquet_hooks::LogFn>) -> Self {
        Arc::get_mut(&mut self.workspace)
            .expect(
                "with_read_package_log called after the workspace ctx was shared via Arc::clone",
            )
            .read_package_log = read_package_log;
        self
    }

    /// Take ownership of `self` and emit the final [`ResolvedTree`]
    /// the peer-resolution stage consumes. The orchestrator passes its
    /// cumulative [`DirectDep`] list (initial walk + each hoist
    /// iteration's contributions) as `direct`.
    ///
    /// When the [`WorkspaceTreeCtx`] is sole-owned by this context
    /// (single-importer install) the inner mutex contents move
    /// directly into the [`ResolvedTree`]; otherwise the maps are
    /// cloned out via [`WorkspaceTreeCtx::snapshot`].
    #[must_use]
    pub fn into_resolved_tree(self, direct: Vec<DirectDep>) -> ResolvedTree {
        match Arc::try_unwrap(self.workspace) {
            Ok(ws) => ws.into_resolved_tree(direct),
            Err(arc) => arc.snapshot(direct),
        }
    }

    /// Build a snapshot of the current tree state without consuming
    /// `self`. The orchestrator's hoist loop snapshots after each
    /// [`extend_tree`] call to run [`fn@crate::resolve_peers`] over the
    /// growing tree and find missing peers to hoist next.
    #[must_use]
    pub fn snapshot(&self, direct: Vec<DirectDep>) -> ResolvedTree {
        self.workspace.snapshot(direct)
    }

    /// Iterate over every `(name, version)` pair the walk has resolved
    /// so far. Used by the orchestrator to keep `allPreferredVersions`
    /// in sync — mirrors upstream's resolveDependency-time push at
    /// [`resolveDependencies.ts:1440`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1440).
    #[must_use]
    pub fn resolved_versions(&self) -> Vec<(String, String)> {
        lock_recoverable(&self.workspace.packages)
            .values()
            .filter_map(|pkg| {
                let name_ver = pkg.result.name_ver.as_ref()?;
                Some((name_ver.name.to_string(), name_ver.suffix.to_string()))
            })
            .collect()
    }
}

/// Walk an additional set of `(alias, range)` pairs as new direct
/// dependencies of the importer, extending `ctx` in place. Returns the
/// per-edge [`DirectDep`] envelopes for the freshly-walked deps; the
/// orchestrator concatenates these into the cumulative direct list it
/// hands to [`TreeCtx::into_resolved_tree`].
///
/// The per-id dedup gate in the per-node walker means already-resolved
/// packages reuse their existing [`ResolvedPackage`]; only the new
/// subtree is actually traversed. Top-level cycles can't occur (the
/// importer can't appear in its own ancestor chain), but the walker
/// may still return `None` for any spec the cycle break gated out;
/// those are filtered here.
pub async fn extend_tree<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    wanted: Vec<WantedSpec>,
    importer_id: &str,
) -> Result<Vec<DirectDep>, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    // Direct deps reuse via the importer's recorded resolution when a
    // prior lockfile exists; without one the gate is a no-op.
    let reuse = if ctx.workspace.wanted_lockfile.is_some() {
        ReuseSource::Importer { importer_id: importer_id.to_string() }
    } else {
        ReuseSource::Off
    };
    let results = wanted
        .into_iter()
        .map(|(name, range, optional, injected)| {
            let reuse = reuse.clone();
            async move {
                // `injected: Some(true)` only when the importer manifest's
                // `dependenciesMeta[name].injected = true` opted this dep
                // in. Otherwise leave it `None` — matches upstream's
                // `injected: opts.dependenciesMeta[alias]?.injected` shape
                // where an absent meta entry yields `undefined`, not
                // `false`. The resolver OR's this with the global
                // `inject_workspace_packages` flag, so `None` and
                // `Some(false)` would produce identical behavior — but
                // mirroring the upstream wire shape keeps the
                // [`WantedKey`] cache buckets aligned across the two
                // pacquet branches that surface `injected`.
                let wanted = WantedDependency {
                    alias: Some(name),
                    bare_specifier: Some(range),
                    optional: Some(optional),
                    injected: injected.then_some(true),
                    ..WantedDependency::default()
                };
                resolve_node(ctx, resolver, wanted, &[], 0, false, reuse).await
            }
        })
        .pipe(future::try_join_all)
        .await?;
    Ok(results.into_iter().flatten().collect())
}

/// Resolve one `(alias, range)` edge, register the resolved package in
/// the dedup map if absent, allocate a fresh [`NodeId`] for this
/// occurrence, and recurse into children.
///
/// `ancestor_ids` is the chain of `pkgIdWithPatchHash` values from the
/// root importer down to the current node's parent. Mirrors upstream's
/// `parentIds` / `parentDepPathsChain`. When the resolved id appears
/// in the chain, this call is a cycle re-entry: pacquet drops the
/// edge entirely (returns `Ok(None)`) so the parent's `children` map
/// omits the cycled child — same shape as upstream's
/// [`parentIdsContainSequence`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencyTree.ts#L378)
/// gate in `buildTree`. Without this, two nodes for the same id race
/// each other into `graph.insert`, and an empty-children entry for the
/// cycled occurrence can overwrite the real one.
#[async_recursion]
async fn resolve_node<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    wanted: WantedDependency,
    ancestor_ids: &[String],
    depth: i32,
    parent_optional: bool,
    reuse: ReuseSource,
) -> Result<Option<DirectDep>, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    let current_is_optional = wanted.optional.unwrap_or(false) || parent_optional;

    // **Lockfile-resolution reuse.** When the prior lockfile already
    // resolved this edge (and the recorded version still satisfies the
    // manifest range, for a direct dep), synthesize the resolution from
    // the lockfile and walk its transitive subtree from the snapshot
    // graph instead of re-resolving from the registry. Mirrors pnpm's
    // [`getInfoFromLockfile` reuse](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1199-L1248).
    // `synthesize_reused_result` is conservative: any shape it can't
    // faithfully reproduce (non-registry resolutions, missing metadata)
    // yields `None` here and the node falls through to a fresh resolve.
    if let Some(reused) = try_reuse_node(ctx, &wanted, reuse) {
        return resolve_reused_node(
            ctx,
            resolver,
            wanted,
            ancestor_ids,
            depth,
            current_is_optional,
            reused,
        )
        .await;
    }

    // Memoise the per-wanted resolve. The first caller for a given
    // `(alias, bare_specifier, optional)` runs the resolver chain and
    // stores the `Arc<ResolveResult>` on `ctx.resolved_by_wanted`;
    // every later caller for the same wanted dep clones the `Arc` and
    // skips the chain entirely. Concurrent first-callers can both miss
    // the cache and run `resolver.resolve` in parallel — the resolver's
    // own per-cache-key semaphore (`pick_package::fetch_locker`)
    // already coalesces those into a single network fetch, so the
    // doubled work is bounded to in-memory packument lookups + semver
    // matching, and the second to finish loses the `insert` race
    // harmlessly (the entry holds an `Arc` to an equivalent
    // `ResolveResult`).
    // `resolutionMode` makes the version pick depend on whether this is
    // a direct (`depth == 0`) or transitive dep, so the cache key and
    // the resolver call both key off the depth-specific options.
    let opts = ctx.opts_for_depth(depth);
    // Project-relative resolutions (`link:`/`file:`/`workspace:`) are
    // keyed by the consuming importer so one importer's relative path
    // is never reused by another. See [`WantedKey`].
    let project_scope = is_project_relative_specifier(wanted.bare_specifier.as_deref())
        .then(|| ctx.base_opts.project_dir.clone());
    let cache_key: WantedKey = (
        wanted.alias.clone(),
        wanted.bare_specifier.clone(),
        wanted.optional,
        wanted.injected,
        opts.pick_lowest_version,
        opts.published_by,
        project_scope,
    );
    let cached =
        lock_recoverable(&ctx.workspace.resolved_by_wanted).get(&cache_key).map(Arc::clone);
    let result = if let Some(result) = cached {
        result
    } else {
        let mut result = resolver
            .resolve(&wanted, opts)
            .await
            .map_err(|err: ResolveError| ResolveDependencyTreeError::Resolve(err.to_string()))?;
        let Some(result_inner) = result.as_mut() else {
            return Err(ResolveDependencyTreeError::SpecNotSupported {
                specifier: render_specifier(&wanted),
            });
        };
        // Apply the configured `readPackageHook` (today:
        // `packageExtensions`) to the manifest fragment before
        // anything downstream sees it. Mirrors upstream's
        // [`ctx.readPackageHook(pkg)`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-resolver/src/resolveDependencies.ts#L1481-L1483)
        // call at the resolveDependency seam. The hook clones the
        // inner `Value` only when it modifies it, so unrelated
        // manifests keep sharing the resolver's cached `Arc`.
        if let Some(hook) = ctx.workspace.manifest_hook.as_ref()
            && let Some(manifest) = result_inner.manifest.take()
        {
            result_inner.manifest = Some(hook(manifest));
        }

        if let Some(pnpmfile_hook) = ctx.workspace.pnpmfile_hook.as_ref()
            && let Some(manifest) = result_inner.manifest.take()
        {
            let log = ctx.workspace.read_package_log.clone().unwrap_or_else(|| Arc::new(|_| {}));
            let hook_ctx = pacquet_hooks::HookContext { log };

            let updated = pnpmfile_hook
                .read_package((*manifest).clone(), hook_ctx)
                .await
                .map_err(ResolveDependencyTreeError::PnpmfileHook)?;
            result_inner.manifest = Some(updated);
        }

        let result = result.expect("Some-guarded above");
        // Wrap in `Arc` once so the cache, the per-id
        // `ResolvedPackage` envelope, and the later peer-resolved
        // graph node share one heap-allocated `ResolveResult`
        // instead of cloning every `String` field per occurrence.
        let result = Arc::new(result);
        lock_recoverable(&ctx.workspace.resolved_by_wanted)
            .entry(cache_key)
            .or_insert_with(|| Arc::clone(&result));
        result
    };

    if let Some(violation) = result.policy_violation.clone() {
        lock_recoverable(&ctx.workspace.policy_violations).push(violation);
    }

    if ctx.base_opts.block_exotic_subdeps
        && depth > 0
        && is_exotic_resolved_via(&result.resolved_via)
    {
        return Err(ResolveDependencyTreeError::ExoticSubdep {
            specifier: wanted
                .alias
                .clone()
                .or_else(|| wanted.bare_specifier.clone())
                .unwrap_or_default(),
            resolved_via: result.resolved_via.clone(),
        });
    }

    let id = build_pkg_id_with_patch_hash(ctx, &result).await?;

    // Cycle break — see the doc comment above.
    if ancestor_ids.iter().any(|prev| prev == &id) {
        return Ok(None);
    }

    let alias = result
        .alias
        .clone()
        .or_else(|| wanted.alias.clone())
        .or_else(|| result.name_ver.as_ref().map(|nv| nv.name.to_string()))
        .unwrap_or_else(|| id.clone());

    // Build (or look up) the ResolvedPackage envelope. The first
    // visitor populates it; later visitors AND-fold the `optional`
    // flag so a single non-optional path flips it back to `false`.
    // Mirrors upstream's
    // [`resolvedPkgsById[...].optional = ... && currentIsOptional`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1630)
    // arm. The `is_revisit` flag flows into the children handling
    // below: a revisit can produce a [`TreeChildren::Lazy`] node
    // because the first visit already populated
    // [`WorkspaceTreeCtx`]'s `children_by_id` for this pkg, and revisits don't
    // discover new transitive packages.
    // Leaves (no deps / optional deps / peers / peerDependenciesMeta)
    // reuse the package id as their `NodeId`, collapsing every parent
    // edge onto one tree node. Non-leaves still get a fresh per-
    // occurrence id so the peer resolver can attach different peer
    // suffixes per call site. Mirrors upstream's
    // [`resolveDependencies.ts:1580`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1580).
    // Computed before the dedup insert so it can be persisted on
    // [`ResolvedPackage::is_leaf`] for the lazy realisation path to
    // reuse — matches upstream's
    // [`getResolvedPackage`](https://github.com/pnpm/pnpm/blob/b9de85dcb6/installing/deps-resolver/src/resolveDependencies.ts#L1771)
    // which sets `isLeaf` once on `resolvedPkgsById[id]` and lets
    // [`buildTree`](https://github.com/pnpm/pnpm/blob/b9de85dcb6/installing/deps-resolver/src/resolveDependencyTree.ts#L381)
    // read it back.
    // Workspace-link nodes follow upstream's
    // [`isLinkedDependency` arm](https://github.com/pnpm/pnpm/blob/cc4ff817aa/installing/deps-resolver/src/resolveDependencies.ts#L926-L937):
    // children are empty (the linked project resolves its own deps as
    // a separate importer), `depth = -1` flags the node for the
    // peer-resolution short-circuit, and the [`ResolvedPackage`] carries
    // no peer dependencies (peer matching is the linked importer's
    // responsibility, not the parent's). The node id is collapsed to a
    // leaf so every reference to the same workspace path shares one
    // [`NodeId`], matching upstream's `createNodeIdForLinkedLocalPkg`.
    let is_link = id.starts_with("link:");
    let is_leaf = is_link || pkg_is_leaf(&result);
    let node_id = if is_leaf { NodeId::leaf(&id) } else { NodeId::next() };

    let is_revisit;
    {
        let mut packages = lock_recoverable(&ctx.workspace.packages);
        if let Some(existing) = packages.get_mut(&id) {
            existing.optional = existing.optional && current_is_optional;
            is_revisit = true;
        } else {
            let peer_dependencies =
                if is_link { BTreeMap::new() } else { extract_peer_dependencies(&result) };
            // Collect peer names for the peer-resolution stage's
            // `parentPkgs` filter (only peers count as parents).
            {
                let mut all_peers = lock_recoverable(&ctx.workspace.all_peer_dep_names);
                for name in peer_dependencies.keys() {
                    all_peers.insert(name.clone());
                }
            }
            packages.insert(
                id.clone(),
                ResolvedPackage {
                    id: id.clone(),
                    result: Arc::clone(&result),
                    peer_dependencies,
                    optional: current_is_optional,
                    is_leaf,
                },
            );
            is_revisit = false;
        }
    }

    let next_ancestors: Vec<String> =
        ancestor_ids.iter().cloned().chain(std::iter::once(id.clone())).collect();

    // **Revisit short-circuit (lazy children).** The first walk of
    // this package populated `children_by_id[id]` with the resolved
    // child pkg_ids — revisits skip the per-child recursion and
    // emit a [`TreeChildren::Lazy`] entry instead. The peer-resolver's
    // `realize_children` walks the cached `children_by_id` to
    // allocate per-occurrence `NodeId`s on demand, applying the same
    // `parent_ids` cycle-break upstream's `buildTree` does. Mirrors
    // upstream's
    // [`isNew` skip](https://github.com/pnpm/pnpm/blob/c86c423bdc/installing/deps-resolver/src/resolveDependencies.ts#L1584):
    // a second visit doesn't rebuild the subtree.
    //
    // First visits still walk eagerly so `children_by_id`,
    // `packages`, `all_peer_dep_names`, and the per-pkg resolver
    // caches get populated for every transitive package — without
    // that pass, lazy revisits would have nothing to expand.
    let children = if is_link {
        // Linked nodes don't walk their manifest's deps — see the
        // `is_link` comment block above. Empty `Realized` map matches
        // upstream's `children: {}` for the `isLinkedDependency`
        // branch.
        crate::resolved_tree::TreeChildren::Realized(BTreeMap::new())
    } else if is_revisit {
        crate::resolved_tree::TreeChildren::Lazy { parent_ids: Arc::new(next_ancestors.clone()) }
    } else {
        // Look up cached children specs first; only walk the manifest on
        // a miss. The cache value is held by `Arc` so revisits clone the
        // refcount instead of the inner `Vec<(String, String, bool)>`.
        let child_specs = {
            let cache = lock_recoverable(&ctx.workspace.children_specs_by_id);
            cache.get(&id).map(Arc::clone)
        };
        let child_specs = if let Some(specs) = child_specs {
            specs
        } else {
            let specs = Arc::new(extract_children(&result)?);
            lock_recoverable(&ctx.workspace.children_specs_by_id)
                .entry(id.clone())
                .or_insert_with(|| Arc::clone(&specs));
            specs
        };
        let child_results = child_specs
            .iter()
            .map(|(child_name, child_range, child_optional)| {
                let child_wanted = WantedDependency {
                    alias: Some(child_name.clone()),
                    bare_specifier: Some(child_range.clone()),
                    optional: Some(*child_optional),
                    ..WantedDependency::default()
                };
                let next_ancestors = next_ancestors.clone();
                async move {
                    // A freshly-resolved node forces its whole subtree
                    // to re-resolve — pnpm's `resolvedDependencies =
                    // parentPkg.updated ? undefined`. `ReuseSource::Off`
                    // is the `undefined` arm.
                    resolve_node(
                        ctx,
                        resolver,
                        child_wanted,
                        &next_ancestors,
                        depth + 1,
                        current_is_optional,
                        ReuseSource::Off,
                    )
                    .await
                }
            })
            .pipe(future::try_join_all)
            .await?;
        // Build the realized `(alias → NodeId)` map for THIS
        // occurrence and the per-pkg `children_by_id` entry future
        // revisits will reuse. `children_by_id` records the resolved
        // child pkg ids (not NodeIds) plus the `optional` flag so
        // lazy realisation can thread `current_is_optional` correctly.
        let mut realized: BTreeMap<String, NodeId> = BTreeMap::new();
        let mut by_id: Vec<crate::resolved_tree::ChildEdge> = Vec::new();
        // Build a spec → optional map to look up each child's `optional` flag.
        let optional_by_alias: HashMap<&str, bool> =
            child_specs.iter().map(|(name, _, optional)| (name.as_str(), *optional)).collect();
        for dep in child_results.into_iter().flatten() {
            let optional = optional_by_alias.get(dep.alias.as_str()).copied().unwrap_or(false);
            by_id.push(crate::resolved_tree::ChildEdge {
                alias: dep.alias.clone(),
                pkg_id: dep.id.clone(),
                optional,
            });
            realized.insert(dep.alias, dep.node_id);
        }
        lock_recoverable(&ctx.workspace.children_by_id)
            .entry(id.clone())
            .or_insert_with(|| Arc::new(by_id));
        crate::resolved_tree::TreeChildren::Realized(realized)
    };

    // Repeat-visit leaves collapse onto one tree node; keep the
    // shallowest depth seen so downstream consumers that read
    // `tree_node.depth` (the peer pass folds it onto the graph node's
    // `depth`) match upstream's
    // [`Math.min(...)` arm](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1636-L1637).
    // Per-occurrence counter ids are unique by construction, so the
    // `and_modify` arm is dead for non-leaves.
    // Linked nodes carry `depth = -1` so the peer-resolution pass
    // short-circuits them in `resolve_node`. Mirrors upstream's
    // `depth: -1` on the `isLinkedDependency` arm.
    let node_depth = if is_link { -1 } else { depth };
    lock_recoverable(&ctx.workspace.dependencies_tree)
        .entry(node_id.clone())
        .and_modify(|node| {
            if node.depth > node_depth {
                node.depth = node_depth;
            }
        })
        .or_insert_with(|| DependenciesTreeNode {
            resolved_package_id: id.clone(),
            children,
            depth: node_depth,
            installable: true,
        });

    Ok(Some(DirectDep { alias, node_id, id }))
}

/// One reusable node: its prior-lockfile snapshot key plus the
/// `ResolveResult` synthesized from the lockfile metadata.
struct ReusedNode {
    key: PkgNameVerPeer,
    result: pacquet_resolving_resolver_base::ResolveResult,
}

/// Decide whether the current edge can reuse the prior lockfile's
/// resolution. Returns the synthesized node when the edge's whole
/// transitive subtree is reusable; `None` (fresh resolve) otherwise.
///
/// Conservative on every axis: no prior lockfile, an unsatisfied direct
/// range, a `link:` / non-registry shape anywhere in the subtree, or a
/// missing snapshot entry all yield `None`. See
/// [`fn@subtree_fully_reusable`] for the recursive subtree check.
fn try_reuse_node(
    ctx: &TreeCtx,
    wanted: &WantedDependency,
    reuse: ReuseSource,
) -> Option<ReusedNode> {
    let lockfile = ctx.workspace.wanted_lockfile.as_ref()?;
    if matches!(ctx.workspace.update_reuse_scope, UpdateReuseScope::None) {
        return None;
    }
    let alias = wanted.alias.as_deref()?;
    let key = match reuse {
        ReuseSource::Importer { importer_id } => {
            let bare_specifier = wanted.bare_specifier.as_deref()?;
            reusable_importer_dep(&lockfile.importers, &importer_id, alias, bare_specifier)?
        }
        ReuseSource::Transitive { key } => key?,
        ReuseSource::Off => return None,
    };
    if !subtree_fully_reusable(ctx, lockfile, &key) {
        return None;
    }
    let result = synthesize_reused_result(lockfile, &key, alias)?;
    Some(ReusedNode { key, result })
}

/// `true` when `name` is a `pacquet update` target excluded from reuse.
fn update_excludes(scope: &UpdateReuseScope, name: &pacquet_lockfile::PkgName) -> bool {
    match scope {
        UpdateReuseScope::All => false,
        // `None` is handled earlier in `try_reuse_node`; treat it the
        // same here for completeness.
        UpdateReuseScope::None => true,
        UpdateReuseScope::Except(names) => names.contains(&name.to_string()),
    }
}

/// `true` when `key` and its entire transitive subtree can be
/// synthesized from `lockfile` (every node a plain-semver registry
/// package present in `packages:`, every snapshot child non-`link:`).
/// Memoised on [`WorkspaceTreeCtx::subtree_reusable`] so each package is
/// checked once.
///
/// A snapshot cycle is treated as **non**-reusable at the back-edge: the
/// key is provisionally inserted as `false` before recursing, so a node
/// reached through a still-in-progress ancestor resolves to `false` and
/// any subtree containing a dependency cycle conservatively re-resolves.
/// This avoids the unsound alternative — a provisional `true` could cache
/// a cycle member as reusable based on an ancestor that later finalizes
/// `false` (e.g. an update-excluded target reachable only through the
/// cycle), wrongly reusing it. SCC-aware reuse of acyclic-equivalent
/// cycles is possible but not worth the complexity for an uncommon case.
fn subtree_fully_reusable(
    ctx: &TreeCtx,
    lockfile: &pacquet_lockfile::Lockfile,
    key: &PkgNameVerPeer,
) -> bool {
    if let Some(&cached) = lock_recoverable(&ctx.workspace.subtree_reusable).get(key) {
        return cached;
    }
    // Provisionally mark non-reusable so a cycle back to `key` resolves to
    // `false` (re-resolve) instead of recursing forever — see the doc above
    // for why `false` rather than `true`.
    lock_recoverable(&ctx.workspace.subtree_reusable).insert(key.clone(), false);
    // A `pacquet update` target anywhere in the subtree forces the whole
    // subtree to re-resolve so the bump's new transitive deps are picked
    // up — mirrors pnpm matching update names at any depth.
    let reusable = !update_excludes(&ctx.workspace.update_reuse_scope, &key.name)
        && synthesize_reused_result(lockfile, key, &key.name.to_string()).is_some()
        && subtree_children_reusable(ctx, lockfile, key);
    lock_recoverable(&ctx.workspace.subtree_reusable).insert(key.clone(), reusable);
    reusable
}

/// Recurse [`fn@subtree_fully_reusable`] across `key`'s snapshot
/// children. A `link:` child (no snapshot key) makes the subtree
/// non-reusable: the linked importer resolves its own deps, which this
/// reuse path doesn't model.
fn subtree_children_reusable(
    ctx: &TreeCtx,
    lockfile: &pacquet_lockfile::Lockfile,
    key: &PkgNameVerPeer,
) -> bool {
    let Some(snapshot) = lockfile.snapshots.as_ref().and_then(|snaps| snaps.get(key)) else {
        // No snapshot entry → the lockfile doesn't record this node's
        // children, so the reuse walk can't reproduce its subtree.
        // Force a fresh resolve rather than risk silently dropping
        // transitive deps. A genuine leaf has an empty-but-*present*
        // snapshot entry (`{}`); a missing one means an inconsistent
        // lockfile, which `try_reuse_node`'s contract sends to a fresh
        // resolve.
        return false;
    };
    let dep_maps = [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()];
    for dep_map in dep_maps.into_iter().flatten() {
        for (child_name, dep_ref) in dep_map {
            let Some(child_key) = dep_ref.resolve(child_name) else {
                return false;
            };
            if !subtree_fully_reusable(ctx, lockfile, &child_key) {
                return false;
            }
        }
    }
    true
}

/// Register a node whose resolution was reused from the prior lockfile,
/// then walk its transitive children from the snapshot graph instead of
/// re-resolving them. Mirrors the post-resolve half of
/// [`fn@resolve_node`], specialized for a node whose subtree
/// [`fn@try_reuse_node`] already confirmed reusable.
#[async_recursion]
async fn resolve_reused_node<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    wanted: WantedDependency,
    ancestor_ids: &[String],
    depth: i32,
    current_is_optional: bool,
    reused: ReusedNode,
) -> Result<Option<DirectDep>, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    let ReusedNode { key, result } = reused;
    let result = Arc::new(result);

    // A reused node carries the synthesized registry resolution into the
    // same per-wanted cache a fresh resolve would populate, so a later
    // fresh-resolve of the identical wanted dep short-circuits to it.
    // `try_reuse_node` only reproduces registry resolutions, which are
    // importer-independent, so the project scope is always `None` here —
    // computed through the same helper to stay in lockstep with the
    // fresh-resolve key shape.
    let opts = ctx.opts_for_depth(depth);
    let project_scope = is_project_relative_specifier(wanted.bare_specifier.as_deref())
        .then(|| ctx.base_opts.project_dir.clone());
    let cache_key: WantedKey = (
        wanted.alias.clone(),
        wanted.bare_specifier.clone(),
        wanted.optional,
        wanted.injected,
        opts.pick_lowest_version,
        opts.published_by,
        project_scope,
    );
    lock_recoverable(&ctx.workspace.resolved_by_wanted)
        .entry(cache_key)
        .or_insert_with(|| Arc::clone(&result));

    let id = build_pkg_id_with_patch_hash(ctx, &result).await?;

    // Cycle break — same as the fresh path.
    if ancestor_ids.iter().any(|prev| prev == &id) {
        return Ok(None);
    }

    let alias = result
        .alias
        .clone()
        .or_else(|| wanted.alias.clone())
        .or_else(|| result.name_ver.as_ref().map(|nv| nv.name.to_string()))
        .unwrap_or_else(|| id.clone());

    // Leaf classification reads the snapshot graph (the source of truth
    // for a reused node's children), not the synthesized manifest (whose
    // `dependencies` are deliberately omitted). A node with no recorded
    // children and no peers is a leaf, matching `pkg_is_leaf`.
    let snapshot = ctx
        .workspace
        .wanted_lockfile
        .as_ref()
        .and_then(|lockfile| lockfile.snapshots.as_ref())
        .and_then(|snaps| snaps.get(&key));
    let peer_dependencies = extract_peer_dependencies(&result);
    let child_refs = snapshot_child_refs(snapshot, &peer_dependencies);
    let is_leaf = child_refs.is_empty() && peer_dependencies.is_empty();
    let node_id = if is_leaf { NodeId::leaf(&id) } else { NodeId::next() };

    let is_revisit;
    {
        let mut packages = lock_recoverable(&ctx.workspace.packages);
        if let Some(existing) = packages.get_mut(&id) {
            existing.optional = existing.optional && current_is_optional;
            is_revisit = true;
        } else {
            {
                let mut all_peers = lock_recoverable(&ctx.workspace.all_peer_dep_names);
                for name in peer_dependencies.keys() {
                    all_peers.insert(name.clone());
                }
            }
            packages.insert(
                id.clone(),
                ResolvedPackage {
                    id: id.clone(),
                    result: Arc::clone(&result),
                    peer_dependencies,
                    optional: current_is_optional,
                    is_leaf,
                },
            );
            is_revisit = false;
        }
    }

    let next_ancestors: Vec<String> =
        ancestor_ids.iter().cloned().chain(std::iter::once(id.clone())).collect();

    let children = if is_revisit {
        crate::resolved_tree::TreeChildren::Lazy { parent_ids: Arc::new(next_ancestors.clone()) }
    } else {
        let child_results = child_refs
            .iter()
            .map(|(child_alias, child_key)| {
                let child_wanted = WantedDependency {
                    alias: Some(child_alias.clone()),
                    // The snapshot pins the exact version; carry it as
                    // the bare specifier so the per-wanted dedup cache
                    // key is stable and a fresh fallback (if reuse were
                    // ever disabled) would still target the right pin.
                    bare_specifier: Some(child_key.suffix.without_peer().to_string()),
                    ..WantedDependency::default()
                };
                let next_ancestors = next_ancestors.clone();
                let child_key = child_key.clone();
                async move {
                    resolve_node(
                        ctx,
                        resolver,
                        child_wanted,
                        &next_ancestors,
                        depth + 1,
                        current_is_optional,
                        ReuseSource::Transitive { key: Some(child_key) },
                    )
                    .await
                }
            })
            .pipe(future::try_join_all)
            .await?;
        let mut realized: BTreeMap<String, NodeId> = BTreeMap::new();
        let mut by_id: Vec<crate::resolved_tree::ChildEdge> = Vec::new();
        let optional_by_alias: HashMap<&str, bool> = child_refs
            .iter()
            .map(|(alias, _)| (alias.as_str(), is_optional_child(snapshot, alias)))
            .collect();
        for dep in child_results.into_iter().flatten() {
            let optional = optional_by_alias.get(dep.alias.as_str()).copied().unwrap_or(false);
            by_id.push(crate::resolved_tree::ChildEdge {
                alias: dep.alias.clone(),
                pkg_id: dep.id.clone(),
                optional,
            });
            realized.insert(dep.alias, dep.node_id);
        }
        lock_recoverable(&ctx.workspace.children_by_id)
            .entry(id.clone())
            .or_insert_with(|| Arc::new(by_id));
        crate::resolved_tree::TreeChildren::Realized(realized)
    };

    lock_recoverable(&ctx.workspace.dependencies_tree)
        .entry(node_id.clone())
        .and_modify(|node| {
            if node.depth > depth {
                node.depth = depth;
            }
        })
        .or_insert_with(|| DependenciesTreeNode {
            resolved_package_id: id.clone(),
            children,
            depth,
            installable: true,
        });

    Ok(Some(DirectDep { alias, node_id, id }))
}

/// `(install_alias, resolved_snapshot_key)` for every non-`link:` child
/// recorded on `snapshot`'s `dependencies` + `optionalDependencies`,
/// excluding resolved peers. Sorted by alias so the per-occurrence walk
/// order is deterministic.
///
/// A snapshot's `dependencies` map lists not only the package's real
/// dependencies but also every *resolved peer* — the node's own peers
/// (`peer_dependencies`) and the peers its descendants required and
/// resolved through this node (`transitivePeerDependencies`) — each
/// pinned to the version it matched in the recorded context. Those are
/// not real children: a fresh resolve walks only the package's manifest
/// `dependencies` and re-derives peers separately against the parent
/// context. Mirroring that, reuse must walk the manifest's deps too — so
/// peer-named entries are dropped here, matching pnpm's reuse path, which
/// builds children from
/// [`getNonDevWantedDependencies(parentPkg.pkg)`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1058)
/// and uses the snapshot's `dependencies` only as the locked-ref lookup,
/// not as the child set. Treating a resolved peer as a regular child
/// makes the peer pass satisfy the peer from the node's own subtree
/// instead of propagating it up, collapsing the peer-context suffix.
fn snapshot_child_refs(
    snapshot: Option<&SnapshotEntry>,
    peer_dependencies: &BTreeMap<String, PeerDep>,
) -> Vec<(String, PkgNameVerPeer)> {
    let Some(snapshot) = snapshot else { return Vec::new() };
    let transitive_peers: HashSet<&str> =
        snapshot.transitive_peer_dependencies.iter().flatten().map(String::as_str).collect();
    let mut out: Vec<(String, PkgNameVerPeer)> = Vec::new();
    for dep_map in [snapshot.dependencies.as_ref(), snapshot.optional_dependencies.as_ref()]
        .into_iter()
        .flatten()
    {
        for (alias, dep_ref) in dep_map {
            let alias_str = match &alias.scope {
                Some(scope) => Cow::Owned(format!("@{scope}/{}", alias.bare)),
                None => Cow::Borrowed(alias.bare.as_str()),
            };
            if peer_dependencies.contains_key(alias_str.as_ref())
                || transitive_peers.contains(alias_str.as_ref())
            {
                continue;
            }
            if let Some(key) = dep_ref.resolve(alias) {
                out.push((alias_str.into_owned(), key));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// `true` when `alias` is recorded under `snapshot.optionalDependencies`
/// (as opposed to `dependencies`). Threads the right `optional` flag onto
/// the reused child's [`crate::resolved_tree::ChildEdge`].
fn is_optional_child(snapshot: Option<&SnapshotEntry>, alias: &str) -> bool {
    let Some(snapshot) = snapshot else { return false };
    let Ok(name) = alias.parse::<pacquet_lockfile::PkgName>() else { return false };
    snapshot.optional_dependencies.as_ref().is_some_and(|deps| deps.contains_key(&name))
}

/// Replace `catalog:` bare specifiers on direct dependencies with the
/// version recorded in the catalogs map. Non-`catalog:` specifiers
/// pass through unchanged.
///
/// Catalog resolution runs only on importer-level deps, matching
/// upstream's
/// [importer-only scope](https://github.com/pnpm/pnpm/blob/a8a8cbce6d/installing/deps-resolver/src/resolveDependencies.ts#L592-L600).
/// A misconfigured entry surfaces immediately rather than masquerading
/// as a `SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER`.
pub(crate) fn resolve_catalog_specifiers(
    specs: Vec<WantedSpec>,
    catalogs: &Catalogs,
) -> Result<Vec<WantedSpec>, ResolveDependencyTreeError> {
    specs
        .into_iter()
        .map(|(name, range, optional, injected)| {
            let wanted =
                CatalogWantedDependency { alias: name.clone(), bare_specifier: range.clone() };
            match resolve_from_catalog(catalogs, &wanted) {
                CatalogResolutionResult::Found(found) => {
                    Ok((name, found.resolution.specifier, optional, injected))
                }
                CatalogResolutionResult::Unused => Ok((name, range, optional, injected)),
                CatalogResolutionResult::Misconfiguration(misconfig) => {
                    Err(ResolveDependencyTreeError::CatalogMisconfiguration(misconfig.error))
                }
            }
        })
        .collect()
}

/// Compute the `pkgIdWithPatchHash` for a freshly-resolved package.
///
/// Mirrors upstream's
/// [`pkgIdWithPatchHash` block](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1502-L1507)
/// in `resolveDependencies`:
///
/// 1. Prefix the resolver's `id` with `<name>@` when it doesn't already
///    start that way. The npm-registry resolver always returns
///    `name@version`; git / tarball / local resolvers return URL-shaped
///    ids and need the prefix so the snapshot key starts with the
///    package name.
/// 2. Look the `(name, version)` pair up in `ctx.patched_dependencies`
///    via [`get_patch_info`] (exact → unique range → wildcard).
/// 3. On a match, append `(patch_hash=<hash>)` and record the matched
///    key on `ctx.applied_patches` so the post-walk
///    `ERR_PNPM_UNUSED_PATCH` check sees the hit.
///
/// Packages whose resolver didn't supply [`pacquet_resolving_resolver_base::ResolveResult::name_ver`]
/// (git / tarball / local — they learn the name from the manifest at
/// fetch time) skip the patch lookup. That matches the surface
/// `patchedDependencies` covers today: keys are `name[@version]`, so a
/// package without a resolve-time name can't match a configured entry
/// anyway. The lookup is also skipped when no patches are configured.
async fn build_pkg_id_with_patch_hash(
    ctx: &TreeCtx,
    result: &pacquet_resolving_resolver_base::ResolveResult,
) -> Result<String, ResolveDependencyTreeError> {
    let raw_id = result.id.as_str();
    // `link:`-resolved workspace deps are short-circuited downstream
    // by `id.starts_with("link:")` checks (see [`is_link`] in the
    // tree walker and `importer_dep_version`'s
    // `dep_path_str.strip_prefix("link:")` arm). Leaving the id
    // unprefixed preserves those short-circuits — pnpm prefixes them
    // too but routes them through a separate `isLinkedDependency`
    // branch that pacquet hasn't ported yet.
    //
    // [`is_link`]: fn@resolve_node
    if raw_id.starts_with("link:") {
        return Ok(raw_id.to_string());
    }
    // Resolvers that learn the name from the fetched manifest (git,
    // tarball, directory) leave `name_ver` unset. Upstream reads
    // `pkg.name` from the manifest itself
    // ([`resolveDependencies.ts:1502-1507`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1502-L1507))
    // and prefixes the id regardless — so a `file:project-1` id
    // becomes `project-1@file:project-1`. Skipping the prefix would
    // leave `(` as the first paren-bearing character in the downstream
    // depPath, which `PkgNameVerPeer`'s `@`-split parser can't recover
    // from (it finds the `@` inside the peer suffix first).
    let manifest_name = result
        .manifest
        .as_ref()
        .and_then(|manifest| manifest.get("name"))
        .and_then(serde_json::Value::as_str);
    let (name, version) = match (result.name_ver.as_ref(), manifest_name) {
        (Some(name_ver), _) => (name_ver.name.to_string(), name_ver.suffix.to_string()),
        (None, Some(name)) => (name.to_string(), String::new()),
        (None, None) => return Ok(raw_id.to_string()),
    };
    let prefixed = if raw_id.starts_with(&format!("{name}@")) {
        raw_id.to_string()
    } else {
        format!("{name}@{raw_id}")
    };
    // `patched_dependencies` keys carry a `name@version` shape, so
    // entries that came in without a `name_ver` (file: / git: /
    // tarball: resolutions whose name we just learned from the
    // manifest above) can't match unless the manifest also surfaced
    // a version. Bail out when version is empty so the patch lookup
    // doesn't run a `name@""` query.
    if version.is_empty() {
        return Ok(prefixed);
    }
    let Some(groups) = ctx.patched_dependencies.as_deref() else {
        return Ok(prefixed);
    };
    let Some(patch) = get_patch_info(Some(groups), &name, &version)? else {
        return Ok(prefixed);
    };
    lock_recoverable(&ctx.workspace.applied_patches).insert(patch.key.clone());
    Ok(format!("{prefixed}(patch_hash={})", patch.hash))
}

/// Render `{alias}@{bare}` (either half dropped when absent) for the
/// error message. Mirrors upstream's `render_specifier` shape in
/// `default-resolver`.
fn render_specifier(wanted: &WantedDependency) -> String {
    let alias = wanted.alias.as_deref().unwrap_or("");
    let bare = wanted.bare_specifier.as_deref().unwrap_or("");
    match (alias.is_empty(), bare.is_empty()) {
        (true, true) => String::new(),
        (true, false) => bare.to_string(),
        (false, true) => alias.to_string(),
        (false, false) => format!("{alias}@{bare}"),
    }
}

/// Extract `dependencies` + `optionalDependencies` from a resolved
/// package's manifest. Peer dependencies are **not** walked as regular
/// edges here — they're hoisted to the importer level by the
/// [`fn@crate::resolve_importer`] orchestrator (which calls [`extend_tree`]
/// with the hoist-picker's output) so a peer ends up shared across
/// every consumer, not nested under each one.
///
/// Peers are still recorded on [`ResolvedPackage::peer_dependencies`]
/// (via [`extract_peer_dependencies`]) so the peer-resolution stage
/// can compute the correct depPath suffix once everything is walked.
///
/// Each entry carries an `optional` flag describing which manifest
/// group it came from — `false` for `dependencies`, `true` for
/// `optionalDependencies`. The walker propagates this through
/// `current_is_optional` so [`ResolvedPackage::optional`] reflects
/// whether every path to the node went through an optional edge.
fn extract_children(
    result: &pacquet_resolving_resolver_base::ResolveResult,
) -> Result<Vec<ChildSpec>, ResolveDependencyTreeError> {
    let Some(manifest) = result.manifest.as_ref() else { return Ok(Vec::new()) };
    let parent = render_parent(result);
    let mut out = Vec::new();
    collect_deps(manifest, "dependencies", false, &parent, &mut out)?;
    collect_deps(manifest, "optionalDependencies", true, &parent, &mut out)?;
    Ok(out)
}

fn collect_deps(
    manifest: &Value,
    key: &str,
    optional: bool,
    parent: &str,
    out: &mut Vec<ChildSpec>,
) -> Result<(), ResolveDependencyTreeError> {
    let Some(map) = manifest.get(key).and_then(Value::as_object) else { return Ok(()) };
    for (name, range) in map {
        if let Some(range_str) = range.as_str() {
            if !crate::is_valid_dependency_alias(name) {
                return Err(ResolveDependencyTreeError::InvalidDependencyName {
                    parent: parent.to_string(),
                    alias: name.clone(),
                });
            }
            out.push((name.clone(), range_str.to_string(), optional));
        }
    }
    Ok(())
}

fn render_parent(result: &pacquet_resolving_resolver_base::ResolveResult) -> String {
    if let Some(name_ver) = result.name_ver.as_ref() {
        format!("Package \"{}@{}\"", name_ver.name, name_ver.suffix)
    } else {
        format!("Package \"{}\"", result.id)
    }
}

/// Extract `peerDependencies` from a resolved package's manifest, with
/// `peerDependenciesMeta[name].optional` folded onto each entry.
/// Mirrors upstream's
/// [`peerDependenciesWithoutOwn`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1791-L1815):
/// names also present in `dependencies` or `optionalDependencies` are
/// skipped because those edges already supply the same package
/// directly.
fn extract_peer_dependencies(
    result: &pacquet_resolving_resolver_base::ResolveResult,
) -> BTreeMap<String, PeerDep> {
    let Some(manifest) = result.manifest.as_ref() else { return BTreeMap::new() };
    let mut peers: BTreeMap<String, PeerDep> = BTreeMap::new();

    let own_deps: HashSet<String> = ["dependencies", "optionalDependencies"]
        .iter()
        .flat_map(|key| {
            manifest
                .get(*key)
                .and_then(Value::as_object)
                .into_iter()
                .flat_map(|map| map.keys().cloned())
        })
        .collect();

    if let Some(map) = manifest.get("peerDependencies").and_then(Value::as_object) {
        for (name, range) in map {
            if own_deps.contains(name) {
                continue;
            }
            if let Some(range_str) = range.as_str() {
                peers.insert(
                    name.clone(),
                    PeerDep { version: range_str.to_string(), optional: false },
                );
            }
        }
    }

    if let Some(meta) = manifest.get("peerDependenciesMeta").and_then(Value::as_object) {
        for (name, info) in meta {
            if own_deps.contains(name) {
                continue;
            }
            let optional = info.get("optional").and_then(Value::as_bool).unwrap_or(false);
            // peerDependenciesMeta can declare a peer without a
            // matching peerDependencies entry — upstream treats those
            // as version "*". Mirror that shape.
            peers
                .entry(name.clone())
                .and_modify(|entry| entry.optional = entry.optional || optional)
                .or_insert_with(|| PeerDep { version: "*".to_string(), optional });
        }
    }

    peers
}

/// `true` when the package has no `dependencies`, `optionalDependencies`,
/// `peerDependencies`, or `peerDependenciesMeta`. Mirrors upstream's
/// [`pkgIsLeaf`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1735-L1742).
///
/// Conservatively returns `false` when the manifest is missing — a
/// future visit may reveal children, and collapsing onto a leaf
/// `NodeId` would lose that information.
fn pkg_is_leaf(result: &pacquet_resolving_resolver_base::ResolveResult) -> bool {
    let Some(manifest) = result.manifest.as_ref() else { return false };
    is_empty_or_absent(manifest.get("dependencies"))
        && is_empty_or_absent(manifest.get("optionalDependencies"))
        && is_empty_or_absent(manifest.get("peerDependencies"))
        && is_empty_or_absent(manifest.get("peerDependenciesMeta"))
}

fn is_empty_or_absent(value: Option<&Value>) -> bool {
    value.and_then(Value::as_object).is_none_or(serde_json::Map::is_empty)
}

/// Provenance tags that count as non-exotic for `blockExoticSubdeps`.
/// Mirrors upstream's
/// [`NON_EXOTIC_RESOLVED_VIA`](https://github.com/pnpm/pnpm/blob/df990fdb51/installing/deps-resolver/src/resolveDependencies.ts#L1831-L1841).
const NON_EXOTIC_RESOLVED_VIA: &[&str] = &[
    "custom-resolver",
    "github.com/denoland/deno",
    "github.com/oven-sh/bun",
    "jsr-registry",
    "local-filesystem",
    "named-registry",
    "nodejs.org",
    "npm-registry",
    "workspace",
];

fn is_exotic_resolved_via(resolved_via: &str) -> bool {
    !NON_EXOTIC_RESOLVED_VIA.contains(&resolved_via)
}
