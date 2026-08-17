use derive_more::{Display, Error};
use futures_util::future;
use miette::Diagnostic;
use pipe_trait::Pipe;
use pnpm_catalogs_resolver::CatalogResolutionError;
use pnpm_catalogs_types::Catalogs;
use pnpm_hooks::PnpmfileHooks;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_patching::{PatchGroupRecord, PatchKeyConflictError};
use pnpm_resolving_resolver_base::{
    GitResolveError, NoMatchingVersionError, PreferredVersionsOverlay, RegistryResponseError,
    ResolveOptions, Resolver, WantedDependency,
};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use serde_json::Value;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::{
    parent_pkg_aliases::ParentPkgAliases,
    resolved_tree::{DirectDep, ResolvedTree},
};

mod catalogs;
mod manifest;
mod reuse;
mod tree_ctx;
mod walk;
mod workspace_ctx;

#[cfg(test)]
mod test_support;

pub use tree_ctx::TreeCtx;
pub use workspace_ctx::WorkspaceTreeCtx;

pub(crate) use catalogs::resolve_catalog_specifiers;
pub(crate) use reuse::{record_changed_direct_deps, unwrap_package_name};
pub(crate) use workspace_ctx::SyncCursor;

use reuse::{ReuseSource, record_direct_dep_versions};
use walk::{
    NodeSeed, level_aliases, level_versions, resolve_node_seed, walk_from_seeds,
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

/// Which dependencies `pacquet update` excludes from lockfile-resolution
/// reuse. An excluded package re-resolves to highest-in-range, and its
/// whole subtree re-resolves with it (so the bump's new transitive deps
/// are picked up).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum UpdateReuseScope {
    /// Reuse every still-satisfied dependency. `install` / `add`.
    #[default]
    All,
    /// Reuse nothing — the whole graph re-resolves. `pacquet update`
    /// with no selectors.
    None,
    /// Reuse everything except the named packages (matched at any depth
    /// the update reaches). `pacquet update <pattern>`.
    Except(HashSet<String>),
}

/// How deep `pacquet update` reaches — the `--depth` ceiling. A node
/// below it keeps its locked resolution even when its name is an update
/// target, matching pnpm's `currentDepth <= updateDepth` gate.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct UpdateDepth(Option<i32>);

impl UpdateDepth {
    /// `--depth Infinity`, the default.
    pub const UNLIMITED: Self = Self(None);

    /// A depth no dependency graph can reach — `usize::MAX`, which the
    /// CLI uses for the `Infinity` default, among them — is unlimited.
    #[must_use]
    pub fn new(depth: usize) -> Self {
        i32::try_from(depth).map_or(Self::UNLIMITED, |depth| Self(Some(depth)))
    }

    /// Whether an update reaches a node at `depth`.
    #[must_use]
    fn reaches(self, depth: i32) -> bool {
        self.0.is_none_or(|max_depth| depth <= max_depth)
    }

    /// The depth to memoise a subtree-reuse answer under. Beyond the
    /// ceiling no node is an update target, so every deeper level shares
    /// one answer — and an unlimited update never varies by depth at all.
    #[must_use]
    fn memo_bucket(self, depth: i32) -> i32 {
        match self.0 {
            None => 0,
            Some(max_depth) => depth.min(max_depth.saturating_add(1)),
        }
    }
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
/// [`crate::WorkspaceResolveOptions::skipped_optional_log`].
pub type SkippedOptionalLogFn = Arc<dyn Fn(SkippedOptionalDependency) + Send + Sync>;

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
    pub deprecated: String,
    pub depth: i32,
}

/// Sink for [`Deprecation`] notifications, pre-bound to the install's
/// reporter so the resolver stays reporter-agnostic. See
/// [`crate::WorkspaceResolveOptions::deprecation_log`].
pub type DeprecationLogFn = Arc<dyn Fn(Deprecation) + Send + Sync>;

/// Error envelope returned by the tree walker.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum ResolveDependencyTreeError {
    /// One of the resolver chain calls failed (network, parse, etc.).
    /// The inner error is the boxed type the resolver returned.
    #[display("Failed to resolve dependency: {_0}")]
    Resolve(#[error(not(source))] String),

    /// The registry publishes the package but nothing the request accepts,
    /// raised with the `ERR_PNPM_NO_MATCHING_VERSION` code.
    #[diagnostic(transparent)]
    NoMatchingVersion(#[error(source)] NoMatchingVersionError),

    /// The registry answered the metadata request with a non-2xx status,
    /// raised with the matching `ERR_PNPM_FETCH_<status>` code.
    #[diagnostic(transparent)]
    RegistryResponse(#[error(source)] RegistryResponseError),

    /// A git dependency's `git ls-remote` failed, raised with the
    /// `ERR_PNPM_GIT_RESOLVE_FAILED` code.
    #[diagnostic(transparent)]
    GitResolve(#[error(source)] GitResolveError),

    /// An optional dependency failed to resolve while the wanted
    /// lockfile still holds a package entry satisfying the wanted
    /// range. Rethrown loudly instead of skipped, because skipping
    /// would erase the locked entries and make the lockfile differ
    /// depending on which machine ran the install
    /// (<https://github.com/pnpm/pnpm/issues/12853>).
    #[display("{_0}")]
    #[diagnostic(help(
        "This optional dependency is not skipped, because the lockfile contains a resolution for it. Skipping it would remove the locked entries, making the lockfile differ depending on which machine ran the install. If the version was intentionally removed from the registry, update the dependent package or remove the entries from the lockfile."
    ))]
    LockedOptionalResolutionFailure(#[error(not(source))] Box<ResolveDependencyTreeError>),

    /// No resolver in the chain claimed the spec, raised with the
    /// `ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER` code.
    #[display("\"{specifier}\" isn't supported by any available resolver.")]
    #[diagnostic(code(ERR_PNPM_SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER))]
    SpecNotSupported {
        #[error(not(source))]
        specifier: String,
    },

    /// A `catalog:` specifier on a direct dependency referenced a
    /// missing entry, used a forbidden protocol, or was otherwise
    /// misconfigured. The inner error carries the
    /// `ERR_PNPM_CATALOG_ENTRY_*` code and message.
    #[diagnostic(transparent)]
    CatalogMisconfiguration(#[error(source)] CatalogResolutionError),

    /// `patchedDependencies` configured more than one version range that
    /// satisfies the same `name@version` and the user did not break the
    /// tie with an exact-version entry. Propagated verbatim from
    /// [`pnpm_patching::get_patch_info`].
    #[display("{_0}")]
    #[diagnostic(transparent)]
    PatchKeyConflict(#[error(source)] PatchKeyConflictError),

    /// A transitive dependency was resolved through an exotic
    /// protocol (git, tarball, file, ...) while `block_exotic_subdeps`
    /// is on, raised with the `ERR_PNPM_EXOTIC_SUBDEP` code.
    #[display(
        "Exotic dependency \"{specifier}\" (resolved via {resolved_via}) is not allowed in subdependencies when blockExoticSubdeps is enabled"
    )]
    #[diagnostic(code(ERR_PNPM_EXOTIC_SUBDEP))]
    ExoticSubdep {
        #[error(not(source))]
        specifier: String,
        resolved_via: String,
    },

    /// A dependency alias contained a path-separator segment that would
    /// escape the intended `node_modules` directory when joined onto a
    /// modules path, raised with the `ERR_PNPM_INVALID_DEPENDENCY_NAME` code.
    #[display(
        "{parent} contains a dependency with an invalid name: {alias:?}. Dependency names must be a single package name or \"@scope/name\" — they cannot contain path-separator segments such as \"..\"."
    )]
    #[diagnostic(code(ERR_PNPM_INVALID_DEPENDENCY_NAME))]
    InvalidDependencyName {
        #[error(not(source))]
        parent: String,
        alias: String,
    },

    /// A pnpmfile hook (`readPackage`) threw, timed out, or returned an
    /// invalid package manifest; a bad hook aborts the install. Carries
    /// `ERR_PNPM_PNPMFILE_FAIL` for all of those. pnpm splits them across
    /// two codes, reserving `ERR_PNPM_BAD_READ_PACKAGE_HOOK_RESULT` for a
    /// hook that returns a non-manifest; pacquet does not distinguish the
    /// two yet.
    #[display("{_0}")]
    #[diagnostic(code(ERR_PNPM_PNPMFILE_FAIL))]
    PnpmfileHook(#[error(not(source))] pnpm_hooks::HookError),
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
    record_changed_direct_deps(&ctx, pnpm_lockfile::Lockfile::ROOT_IMPORTER_KEY, &wanted);
    let parent_pkg_aliases =
        ParentPkgAliases::root(wanted.iter().map(|(alias, ..)| alias.clone()).collect());
    let direct = extend_tree(
        &ctx,
        resolver,
        wanted,
        pnpm_lockfile::Lockfile::ROOT_IMPORTER_KEY,
        &parent_pkg_aliases,
    )
    .await?;
    Ok(ctx.into_resolved_tree(direct))
}

/// Collect the names of the importer manifest's `optionalDependencies`
/// entries so the walker can tag each direct dep with the right
/// `wanted.optional` flag. `optionalDependencies` wins over the other
/// groups when an alias appears in more than one, so the
/// `ResolvedPackage.optional` propagation starts from the right
/// per-direct-dep value.
pub(crate) fn importer_optional_dependency_names(manifest: &PackageManifest) -> HashSet<String> {
    manifest.dependencies([DependencyGroup::Optional]).map(|(name, _)| name.to_string()).collect()
}

/// Collect the names of the importer manifest's `dependenciesMeta` entries
/// whose `injected` flag is `true`. This per-alias `injected` opt-in
/// flips a workspace dep onto the hard-linked `file:` path even when the
/// global `injectWorkspacePackages` is off. Only importer-level deps are
/// consulted; the recursive walker does not inherit this from any
/// resolved package's own `dependenciesMeta` — the opt-in is
/// importer-scoped.
pub(crate) fn importer_injected_dependency_names(manifest: &PackageManifest) -> HashSet<String> {
    let Some(meta) =
        manifest.value().get("dependenciesMeta").and_then(serde_json::Value::as_object)
    else {
        return HashSet::default();
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
/// An alias declared in several groups yields one spec, merged by
/// spreading the groups in order: `peerDependencies` first (when
/// `auto_install_peers`), then `devDependencies` < `dependencies` <
/// `optionalDependencies`, a later group's range replacing an earlier
/// one — matching `filterDependenciesByType` in
/// `@pnpm/pkg-manifest.utils` (`{...dev, ...prod, ...optional}`), so a
/// regular dep wins over a devDependency of the same alias, and either
/// wins over its peer range.
///
/// Shared by [`fn@crate::resolve_importer`] (which walks them) and the
/// `time-based` cutoff pre-pass in [`fn@crate::resolve_workspace`]
/// (which only needs the resolved direct-dep publish dates), so both
/// see the identical direct-dep set — the importer-dep computation runs
/// once before resolving an importer's deps.
pub(crate) fn importer_direct_wanted_specs<DependencyGroupList>(
    manifest: &PackageManifest,
    dependency_groups: DependencyGroupList,
    auto_install_peers: bool,
    catalogs: &Catalogs,
) -> Result<Vec<WantedSpec>, ResolveDependencyTreeError>
where
    DependencyGroupList: IntoIterator<Item = DependencyGroup>,
{
    let included: Vec<DependencyGroup> = dependency_groups.into_iter().collect();
    let mut groups: Vec<DependencyGroup> = Vec::new();
    if auto_install_peers || included.contains(&DependencyGroup::Peer) {
        groups.push(DependencyGroup::Peer);
    }
    groups.extend(
        [DependencyGroup::Dev, DependencyGroup::Prod, DependencyGroup::Optional]
            .into_iter()
            .filter(|group| included.contains(group)),
    );
    let optional_names = importer_optional_dependency_names(manifest);
    let injected_names = importer_injected_dependency_names(manifest);
    let mut order: Vec<&str> = Vec::new();
    let mut ranges: HashMap<&str, &str> = HashMap::default();
    for (name, range) in manifest.dependencies(groups) {
        if !crate::is_valid_dependency_alias(name) {
            return Err(ResolveDependencyTreeError::InvalidDependencyName {
                parent: "The current package".to_string(),
                alias: name.to_string(),
            });
        }
        if ranges.insert(name, range).is_none() {
            order.push(name);
        }
    }
    let wanted: Vec<WantedSpec> = order
        .into_iter()
        .map(|name| {
            (
                name.to_string(),
                ranges[name].to_string(),
                optional_names.contains(name),
                injected_names.contains(name),
            )
        })
        .collect();
    resolve_catalog_specifiers(wanted, catalogs)
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
/// [`ResolvedPackage`]: crate::ResolvedPackage
pub async fn extend_tree<Chain>(
    ctx: &TreeCtx,
    resolver: &Chain,
    wanted: Vec<WantedSpec>,
    importer_id: &str,
    parent_pkg_aliases: &Arc<ParentPkgAliases>,
) -> Result<Vec<DirectDep>, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    ctx.workspace.bump_revision();
    // Direct deps reuse via the importer's recorded resolution when a
    // prior lockfile exists; without one the gate is a no-op.
    let reuse = if ctx.workspace.wanted_lockfile.is_some() {
        ReuseSource::Importer { importer_id: importer_id.to_string() }
    } else {
        ReuseSource::Off
    };
    // Phase 1: resolve every direct dep before any subtree walk, so
    // the level's resolved versions seed the children's
    // preferred-versions overlay (a per-level fold; the direct deps
    // themselves resolve against the importer's static preferred map
    // only).
    let root_ancestors = Arc::new(Vec::new());
    let seeds = wanted
        .into_iter()
        .map(|(name, range, optional, injected)| {
            let reuse = reuse.clone();
            let root_ancestors = Arc::clone(&root_ancestors);
            async move {
                // `injected: Some(true)` only when the importer manifest's
                // `dependenciesMeta[name].injected = true` opted this dep
                // in. Otherwise leave it `None`: an absent meta entry
                // yields no flag rather than `false`. The resolver OR's
                // this with the global `inject_workspace_packages` flag,
                // so `None` and `Some(false)` would produce identical
                // behavior — but keeping `None` aligns the [`WantedKey`]
                // cache buckets across the two pacquet branches that
                // surface `injected`.
                let wanted = WantedDependency {
                    alias: Some(name),
                    bare_specifier: Some(range),
                    optional: Some(optional),
                    injected: injected.then_some(true),
                    ..WantedDependency::default()
                };
                let base_overlay = ctx.base_opts.preferred_versions_overlay.clone();
                let seed = resolve_node_seed(
                    ctx,
                    resolver,
                    wanted,
                    &root_ancestors,
                    0,
                    false,
                    reuse,
                    base_overlay,
                    None,
                    parent_pkg_aliases,
                    false,
                )
                .await?;
                warm_children_resolutions(ctx, resolver, &seed).await;
                Ok::<NodeSeed, ResolveDependencyTreeError>(seed)
            }
        })
        .pipe(future::try_join_all)
        .await?;
    // The level chain extends any caller-seeded overlay so descendant
    // picks and cache keys keep honoring it.
    let direct_versions = level_versions(ctx, &seeds);
    // Recorded only now the level barrier has passed, so the subtree walk
    // sees the resolved direct-dep versions.
    record_direct_dep_versions(ctx, importer_id, &direct_versions);
    let children_overlay = PreferredVersionsOverlay::layer(
        ctx.base_opts.preferred_versions_overlay.clone(),
        direct_versions,
    );
    let children_pkg_aliases = parent_pkg_aliases.extend(level_aliases(&seeds));
    // Phase 2: settle this level's children ownership and walk the tree
    // below it a level at a time.
    let direct =
        walk_from_seeds(ctx, resolver, seeds, children_overlay, children_pkg_aliases).await?;
    ctx.workspace.record_preferred_version_roots(direct.iter().map(|dep| dep.id.as_str()));
    // Second bump, after every write of this wave (including the roots
    // above) has landed: a `run_preferred_versions` read racing with
    // this call could bind the entry bump's revision to a partial
    // closure, and without a completion bump it would never refresh.
    ctx.workspace.bump_revision();
    Ok(direct)
}

#[cfg(test)]
mod tests;
