pub use catalogs::{ChildCatalogs, local_directory_child_specs};
pub use failure::{DependencyResolutionError, ResolutionCancel, ResolveDependencyTreeError};
pub use manifest::{declared_specifiers, manifest_child_specs};

pub use update_scope::{
    LockedRuntimeReuse, UpdateDepth, UpdateReuseScope, UpdateTargets, VersionLine,
};

pub use reuse::real_package_name_of;

pub use tree_ctx::{TreeCtx, directory_package_dir};
pub(crate) use tree_ctx::{declaring_manifest_dir, resolves_from_declaring_manifest};

pub use workspace_ctx::WorkspaceTreeCtx;

pub(crate) use catalogs::resolve_catalog_specifiers;

pub(crate) use importer::importer_direct_wanted_specs;

pub(crate) use reuse::{
    direct_dep_specifier_unchanged, preseed_changed_direct_versions, record_changed_direct_deps,
    resolve_direct_dependency_detached,
};

pub(crate) use workspace_ctx::SyncCursor;

mod update_scope;

use futures_util::future;
use pipe_trait::Pipe;
use pnpm_hooks::PnpmfileHooks;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_patching::PatchGroupRecord;
use pnpm_resolving_resolver_base::{
    PreferredVersionsOverlay, ResolveOptions, Resolver, WantedDependency,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard},
};

use crate::{
    parent_pkg_aliases::ParentPkgAliases,
    resolved_tree::{AncestorPkgIds, DirectDep, ResolvedTree},
};

mod catalogs;
mod failure;
mod finalized;
mod importer;
mod locked_resolution;
mod manifest;
mod reuse;
mod tree_ctx;
mod walk;
mod workspace_ctx;

#[cfg(test)]
mod test_support;

use importer::{
    describe_importer, importer_injected_dependency_names, importer_optional_dependency_names,
};
use reuse::{ReuseSource, record_direct_dep_versions};
use walk::{
    ChildEdge, NodeSeed, level_aliases, level_versions, resolve_node_seed, walk_from_seeds,
    warm_children_resolutions,
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

/// [`fn@lock_recoverable`] for a read guard on a [`RwLock`].
fn read_recoverable<Inner>(lock: &RwLock<Inner>) -> RwLockReadGuard<'_, Inner> {
    lock.read().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// [`fn@lock_recoverable`] for a write guard on a [`RwLock`].
fn write_recoverable<Inner>(lock: &RwLock<Inner>) -> RwLockWriteGuard<'_, Inner> {
    lock.write().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Options threaded into [`fn@resolve_dependency_tree`].
///
/// This entry point is single-importer, so the option bag is small.
/// `base_opts` is the [`ResolveOptions`] every per-package `resolve()`
/// call sees; the tree walker doesn't mutate it.
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
    /// Post-pnpmfile [`ManifestHook`] (overrides). See
    /// `WorkspaceTreeCtx::overrides_hook` for the ordering contract.
    pub overrides_hook: Option<ManifestHook>,
    pub pnpmfile_hook: Option<Arc<dyn PnpmfileHooks>>,
    /// `context.log(...)` sink for the `pnpmfile_hook`'s `readPackage`
    /// calls. `None` leaves hook logging a no-op. See
    /// [`WorkspaceTreeCtx::with_read_package_log`].
    pub read_package_log: Option<pnpm_hooks::LogFn>,
    /// The install's `autoInstallPeers` setting. See
    /// [`WorkspaceTreeCtx::with_auto_install_peers`].
    pub auto_install_peers: bool,
}

impl std::fmt::Debug for ResolveDependencyTreeOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolveDependencyTreeOptions")
            .field("base_opts", &self.base_opts)
            .field("patched_dependencies", &self.patched_dependencies)
            .field("manifest_hook", &self.manifest_hook.as_ref().map(|_| "<hook>"))
            .field("overrides_hook", &self.overrides_hook.as_ref().map(|_| "<hook>"))
            .field("pnpmfile_hook", &self.pnpmfile_hook.as_ref().map(|_| "<hook>"))
            .field("read_package_log", &self.read_package_log.as_ref().map(|_| "<log>"))
            .field("auto_install_peers", &self.auto_install_peers)
            .finish()
    }
}

/// Per-manifest mutation applied to every resolved package's
/// manifest before downstream consumers (children walk, peer
/// extraction, lockfile build) see it. Takes the `Arc<Value>` the
/// resolver returned and yields either the same `Arc` (no-op) or a
/// fresh `Arc` carrying a deep-cloned + extended manifest.
///
/// A `readPackage`-hook signature collapsed to the only field pacquet
/// currently touches (the manifest). Threaded into [`TreeCtx`] so a
/// single `Arc::clone` reaches every recursive call.
pub type ManifestHook = Arc<dyn Fn(Arc<Value>) -> Arc<Value> + Send + Sync>;

/// One skipped-optional-dependency notification from the tree walker:
/// an optional dependency failed to resolve and the walker dropped the
/// edge instead of failing the install. Mirrors the package payload of
/// the `pnpm:skipped-optional-dependency` (`reason=resolution_failure`)
/// debug log so the install layer can forward it to the reporter wire.
#[derive(Debug, Clone)]
pub struct SkippedOptionalDependency {
    /// Rendering of the resolution error that caused the skip.
    pub details: String,
    /// The edge's install alias, when it carries one.
    pub name: Option<String>,
    /// The wanted specifier, present only when [`Self::name`] is.
    pub version: Option<String>,
    /// The wanted specifier.
    pub bare_specifier: String,
    /// The resolved packages between the importer and the failing
    /// edge, importer first. Empty for a direct optional dependency —
    /// the default reporter renders only those.
    pub parents: Vec<SkippedOptionalDependencyParent>,
    /// The project directory the failing edge was resolved for.
    pub prefix: String,
}

/// One ancestor on a [`SkippedOptionalDependency::parents`] chain.
#[derive(Debug, Clone)]
pub struct SkippedOptionalDependencyParent {
    /// The ancestor's `pkgIdWithPatchHash`.
    pub id: String,
    pub name: String,
    pub version: String,
}

/// Sink for [`SkippedOptionalDependency`] notifications, pre-bound to
/// the install's reporter so the resolver stays reporter-agnostic. See
/// [`crate::WorkspaceResolveHooks::skipped_optional_log`].
pub type SkippedOptionalLogFn = Arc<dyn Fn(SkippedOptionalDependency) + Send + Sync>;

/// A package whose resolution and whole dependency subtree are settled
/// and carry no `peerDependencies`, so its lockfile dep path is its
/// package id and its child edges are known before peers resolve. See
/// [`crate::WorkspaceResolveHooks::finalized_package`].
#[derive(Debug, Clone)]
pub struct FinalizedPackage {
    /// The package id with its patch hash: the snapshot key the
    /// package will have in the lockfile.
    pub pkg_id: Arc<str>,
    pub result: Arc<pnpm_resolving_resolver_base::ResolveResult>,
    /// The package's resolved `dependencies` and `optionalDependencies`
    /// edges.
    pub children: Vec<FinalizedChild>,
}

/// One child edge of a [`FinalizedPackage`].
#[derive(Debug, Clone)]
pub struct FinalizedChild {
    /// The name the child is linked under in the package's `node_modules`.
    pub alias: String,
    /// The child's package id, its lockfile snapshot key.
    pub pkg_id: Arc<str>,
    pub optional: bool,
}

/// Sink for [`FinalizedPackage`] notifications, called from the tree
/// walk as soon as a package's subtree settles. The call must be cheap
/// and must not block: it runs on the resolver's task between levels.
pub type FinalizedPackageFn = Arc<dyn Fn(FinalizedPackage) + Send + Sync>;

/// One deprecation notification from the tree walker: a newly-resolved
/// package carries a non-empty `deprecated` field in its registry
/// manifest and is not covered by `allowedDeprecatedVersions`. Mirrors
/// the package payload of the `pnpm:deprecation` debug log so the
/// install layer can forward it to the reporter wire.
#[derive(Debug, Clone)]
pub struct Deprecation {
    pub pkg_name: String,
    pub pkg_version: String,
    pub pkg_id: String,
    pub prefix: String,
    pub depth: i32,
    /// A version of the same package that is not deprecated, when the
    /// resolver knew one. Absent for a resolution reused from the lockfile,
    /// which holds no packument to work it out from.
    pub non_deprecated_alternative: Option<pnpm_resolving_resolver_base::NonDeprecatedAlternative>,
}

/// Sink for [`Deprecation`] notifications, pre-bound to the install's
/// reporter so the resolver stays reporter-agnostic. See
/// [`crate::WorkspaceResolveHooks::deprecation_log`].
pub type DeprecationLogFn = Arc<dyn Fn(Deprecation) + Send + Sync>;

/// Walk `manifest` plus the entries in `dependency_groups`, dispatch
/// each direct dep through `resolver`, recurse on each picked
/// package's own `dependencies`, and return a [`ResolvedTree`] that
/// carries both the flat dedup map (`packages`) and the per-occurrence
/// tree (`dependencies_tree`).
///
/// Covers the npm-shaped slice pacquet currently exposes.
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
///
/// [`ResolvedPackage`]: crate::ResolvedPackage
/// [`DependenciesTreeNode`]: crate::DependenciesTreeNode
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
        .with_overrides_hook(opts.overrides_hook)
        .with_pnpmfile_hook(opts.pnpmfile_hook)
        .with_read_package_log(opts.read_package_log)
        .with_auto_install_peers(opts.auto_install_peers);
    let wanted = root_wanted_specs(manifest, dependency_groups)?;
    record_changed_direct_deps(&ctx, pnpm_lockfile::Lockfile::ROOT_IMPORTER_KEY, &wanted);
    let parent_pkg_aliases = ParentPkgAliases::root(
        wanted
            .iter()
            .map(|spec| spec.alias.clone())
            .collect(),
    );
    let direct = extend_tree(
        &ctx,
        resolver,
        wanted,
        Some(pnpm_lockfile::Lockfile::ROOT_IMPORTER_KEY),
        &parent_pkg_aliases,
    )
    .await?;
    Ok(ctx.into_resolved_tree(direct))
}

/// The root manifest's dependencies in `dependency_groups`, each checked
/// to be an installable alias.
fn root_wanted_specs<DependencyGroupList>(
    manifest: &PackageManifest,
    dependency_groups: DependencyGroupList,
) -> Result<Vec<DependencySpec>, ResolveDependencyTreeError>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    let optional_names = importer_optional_dependency_names(manifest);
    let injected_names = importer_injected_dependency_names(manifest);
    manifest
        .dependencies(dependency_groups)
        .map(|(name, range)| {
            crate::is_valid_dependency_alias(name)
                .then(|| DependencySpec {
                    alias: name.to_string(),
                    bare_specifier: range.to_string(),
                    optional: optional_names.contains(name),
                    injected: injected_names.contains(name),
                })
                .ok_or_else(|| ResolveDependencyTreeError::InvalidDependencyName {
                    parent: describe_importer(manifest),
                    alias: name.to_string(),
                })
        })
        .collect()
}

/// Whether `manifest`'s `dependenciesMeta` injects the dependency `name`.
#[must_use]
pub fn dependency_is_injected(manifest: &Value, name: &str) -> bool {
    manifest
        .get("dependenciesMeta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get(name))
        .is_some_and(dependency_meta_is_injected)
}

/// Whether a `dependenciesMeta` entry sets `injected`.
#[must_use]
pub fn dependency_meta_is_injected(meta: &Value) -> bool {
    meta.get("injected")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// One dependency as a manifest declares it, before resolution.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DependencySpec {
    pub alias: String,
    pub bare_specifier: String,
    /// Declared under `optionalDependencies`.
    pub optional: bool,
    /// The importer manifest's `dependenciesMeta[alias].injected` flag. It
    /// picks the `file:` resolution shape for this one dependency even when
    /// [`pnpm_resolving_resolver_base::ResolverProjectOptions::inject_workspace_packages`]
    /// is off.
    pub injected: bool,
}

/// `injected: Some(true)` only when the importer manifest's
/// `dependenciesMeta[name].injected = true` opted this dep in. Otherwise
/// leave it `None`: an absent meta entry yields no flag rather than
/// `false`. The resolver OR's this with the global
/// `inject_workspace_packages` flag, so `None` and `Some(false)` would
/// produce identical behavior — but keeping `None` aligns the
/// [`WantedKey`](workspace_ctx::WantedKey) cache buckets across the two pacquet branches that
/// surface `injected`.
pub(crate) fn wanted_from_spec(spec: DependencySpec) -> WantedDependency {
    let DependencySpec {
        alias,
        bare_specifier,
        optional,
        injected,
    } = spec;
    WantedDependency {
        alias: Some(alias),
        bare_specifier: Some(bare_specifier),
        optional: Some(optional),
        injected: injected.then_some(true),
        ..WantedDependency::default()
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
///
/// `reuse_importer_id` reads manifest dependencies from that importer. Pass `None`
/// for hoisted peers, which must not inherit a removed direct dependency's resolution.
///
/// [`ResolvedPackage`]: crate::ResolvedPackage
pub async fn extend_tree<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    wanted: Vec<DependencySpec>,
    reuse_importer_id: Option<&str>,
    parent_pkg_aliases: &Arc<ParentPkgAliases>,
) -> Result<Vec<DirectDep>, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    ctx.workspace.tree.bump_revision();
    let reuse = match reuse_importer_id.filter(|_| ctx.workspace.reuse.lockfile.is_some()) {
        Some(importer_id) => ReuseSource::Importer { importer_id: importer_id.to_string() },
        None => ReuseSource::Off,
    };
    // Phase 1: resolve every direct dep before any subtree walk, so
    // the level's resolved versions seed the children's
    // preferred-versions overlay (a per-level fold; the direct deps
    // themselves resolve against the importer's static preferred map
    // only).
    let root = DirectRoot {
        reuse,
        ancestors: Arc::new(Vec::new()),
        parent_pkg_aliases,
        base_overlay: &ctx.options.base.version.preferred_versions_overlay,
    };
    let seeds = wanted
        .into_iter()
        .map(|spec| seed_direct(ctx, resolver, spec, &root))
        .pipe(future::try_join_all)
        .await?;
    // The level chain extends any caller-seeded overlay so descendant
    // picks and cache keys keep honoring it.
    let direct_versions = level_versions(ctx, &seeds);
    // Recorded only now the level barrier has passed, so the subtree walk
    // sees the resolved direct-dep versions.
    record_direct_dep_versions(ctx, &ctx.importer.id, &direct_versions);
    let children_overlay = PreferredVersionsOverlay::layer_direct(
        ctx.options.base.version.preferred_versions_overlay.clone(),
        direct_versions,
    );
    let children_pkg_aliases = parent_pkg_aliases.extend(level_aliases(&seeds));
    // Phase 2: settle this level's children ownership and walk the tree
    // below it a level at a time.
    let direct =
        walk_from_seeds(ctx, resolver, seeds, children_overlay, children_pkg_aliases).await?;
    ctx.workspace.versions.record_preferred_version_roots(direct.iter().map(|dep| &*dep.id));
    // Second bump, after every write of this wave (including the roots
    // above) has landed: a `run_preferred_versions` read racing with
    // this call could bind the entry bump's revision to a partial
    // closure, and without a completion bump it would never refresh.
    ctx.workspace.tree.bump_revision();
    Ok(direct)
}

/// What every direct dep of one importer wave resolves against.
struct DirectRoot<'r> {
    reuse: ReuseSource,
    ancestors: AncestorPkgIds,
    parent_pkg_aliases: &'r Arc<ParentPkgAliases>,
    base_overlay: &'r Option<Arc<PreferredVersionsOverlay>>,
}

async fn seed_direct<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    spec: DependencySpec,
    root: &DirectRoot<'_>,
) -> Result<NodeSeed, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    let seed = resolve_node_seed(
        ctx,
        resolver,
        wanted_from_spec(spec),
        ChildEdge {
            ancestor_ids: &root.ancestors,
            depth: 0,
            parent_optional: false,
            reuse: root.reuse.clone(),
            pick_overlay: root.base_overlay.clone(),
            parent_dir: None,
            parent_pkg_aliases: root.parent_pkg_aliases,
            parent_is_workspace: false,
        },
    )
    .await?;
    warm_children_resolutions(ctx, resolver, &seed).await;
    Ok(seed)
}

#[cfg(test)]
mod tests;
