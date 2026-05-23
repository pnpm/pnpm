use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard};

use async_recursion::async_recursion;
use derive_more::{Display, Error};
use futures_util::future;
use miette::Diagnostic;
use pacquet_catalogs_resolver::{
    CatalogResolutionError, CatalogResolutionResult, WantedDependency as CatalogWantedDependency,
    resolve_from_catalog,
};
use pacquet_catalogs_types::Catalogs;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_patching::{PatchGroupRecord, PatchKeyConflictError, get_patch_info};
use pacquet_resolving_resolver_base::{ResolveError, ResolveOptions, Resolver, WantedDependency};
use pipe_trait::Pipe;
use serde_json::Value;

/// Acquire a [`Mutex`] guard, recovering from poisoning the same way
/// the rest of pacquet does (`build_modules.rs`, `pick_package.rs`,
/// …). The mutexes guarded by this helper hold short HashMap /
/// HashSet inserts with no invariants that survive a panic, so the
/// install can keep going after the unrelated panic that poisoned
/// the lock — better than escalating into a hard install-wide
/// failure.
fn lock_recoverable<Inner>(mutex: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}

use crate::{
    node_id::NodeId,
    resolved_tree::{DependenciesTreeNode, DirectDep, PeerDep, ResolvedPackage, ResolvedTree},
};

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
#[derive(Debug)]
pub struct ResolveDependencyTreeOptions {
    pub base_opts: ResolveOptions,
    /// Configured `patchedDependencies`, grouped by package name. Threaded
    /// through so the per-node walker can append `(patch_hash=<hash>)` to
    /// each matched package's `pkgIdWithPatchHash` and record the patch
    /// key on [`crate::ResolvedTree::applied_patches`] for the
    /// `ERR_PNPM_UNUSED_PATCH` post-walk check. Mirrors upstream's
    /// [`ctx.patchedDependencies`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L164)
    /// thread.
    pub patched_dependencies: Option<Arc<PatchGroupRecord>>,
}

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
    /// protocol (git, tarball, file, …) while `block_exotic_subdeps`
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
    let ctx = TreeCtx::new(opts.base_opts).with_patched_dependencies(opts.patched_dependencies);
    let optional_names = importer_optional_dependency_names(manifest);
    let wanted: Vec<(String, String, bool)> = manifest
        .dependencies(dependency_groups)
        .map(|(name, range)| {
            let optional = optional_names.contains(name);
            (name.to_string(), range.to_string(), optional)
        })
        .collect();
    let direct = extend_tree(&ctx, resolver, wanted).await?;
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

/// Cache key for [`TreeCtx::resolved_by_wanted`].
///
/// The npm-shaped slice pacquet exposes today calls
/// [`Resolver::resolve`] with only three [`WantedDependency`] fields
/// populated — `alias`, `bare_specifier`, and `optional` (see the
/// `WantedDependency` literals in [`extend_tree`] and the recursive
/// arm of [`fn@resolve_node`]). Anything else stays at `Default::default()`,
/// so a tuple over those three fields uniquely identifies a wanted
/// dep across revisits.
///
/// `optional` is part of the key because the npm resolver's
/// `pick_package` toggles between the abbreviated and full packument
/// based on `wanted.optional` ([`pickPackage.ts:391`](https://github.com/pnpm/pnpm/blob/2a9bd897bf/resolving/npm-resolver/src/pickPackage.ts#L201)) —
/// caching by `(alias, bare_specifier)` alone would let an optional
/// caller satisfy itself with a non-optional caller's abbreviated
/// result, losing the `libc`/`cpu`/`os` filter inputs that mode
/// supplies.
type WantedKey = (Option<String>, Option<String>, Option<bool>);

/// One entry in [`TreeCtx::children_specs_by_id`] —
/// `(child_alias, child_range, child_optional)` triples extracted from
/// a resolved package's manifest's `dependencies` plus
/// `optionalDependencies` sections.
type ChildSpec = (String, String, bool);

/// Mutable workspace for an in-flight tree walk. The orchestrator
/// (`resolve_importer`) holds one of these across hoist iterations and
/// extends it via [`extend_tree`] so newly-hoisted peer dependencies
/// reuse the existing per-id dedup map instead of restarting the walk.
pub struct TreeCtx {
    base_opts: ResolveOptions,
    packages: Mutex<HashMap<String, ResolvedPackage>>,
    dependencies_tree: Mutex<HashMap<NodeId, DependenciesTreeNode>>,
    all_peer_dep_names: Mutex<HashSet<String>>,
    policy_violations: Mutex<Vec<pacquet_resolving_resolver_base::ResolutionPolicyViolation>>,
    /// Configured `patchedDependencies` (already grouped by name).
    /// Shared by `Arc` so the lookup table doesn't get cloned per
    /// recursive call. `None` when no patches are configured for this
    /// install.
    patched_dependencies: Option<Arc<PatchGroupRecord>>,
    /// Keys of the `patchedDependencies` entries whose patch was
    /// matched against at least one resolved package. Mirrors upstream's
    /// `ctx.appliedPatches`.
    applied_patches: Mutex<HashSet<String>>,
    /// Memoised [`Resolver::resolve`] results keyed by the parts of
    /// [`WantedDependency`] the npm slice actually populates (see
    /// [`WantedKey`]).
    ///
    /// pnpm's `resolveDependencies` carries the equivalent skip via the
    /// [`isNew` gate](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1584) —
    /// the second time a `pkgIdWithPatchHash` is reached, the walker
    /// reuses the existing `resolvedPkgsById` envelope and never
    /// re-enters the resolver chain. pacquet shortcuts at the layer
    /// above that (the wanted-dep edge) because a `(name, range)`
    /// pair that's already been resolved doesn't need
    /// `pick_package`'s in-memory packument lookup, cache-key
    /// formatting, or semver matching repeated. On the `alotta-files`
    /// fixture this collapses the ~3 redundant `resolver.resolve()`
    /// calls per package the tree walk used to drive into one.
    resolved_by_wanted:
        Mutex<HashMap<WantedKey, Arc<pacquet_resolving_resolver_base::ResolveResult>>>,
    /// Cached `extract_children` output keyed by `pkgIdWithPatchHash`.
    /// First visit walks the manifest's `dependencies` /
    /// `optionalDependencies`; subsequent visits clone the `Arc` and
    /// skip the JSON traversal. Paired with [`Self::resolved_by_wanted`]
    /// so a revisit's per-call CPU is two HashMap lookups and an
    /// `Arc::clone`.
    children_specs_by_id: Mutex<HashMap<String, Arc<Vec<ChildSpec>>>>,
}

impl TreeCtx {
    /// Construct an empty context. Calls to [`extend_tree`] populate
    /// `packages` / `dependencies_tree` / `all_peer_dep_names`.
    pub fn new(base_opts: ResolveOptions) -> Self {
        TreeCtx {
            base_opts,
            packages: Mutex::new(HashMap::new()),
            dependencies_tree: Mutex::new(HashMap::new()),
            all_peer_dep_names: Mutex::new(HashSet::new()),
            policy_violations: Mutex::new(Vec::new()),
            patched_dependencies: None,
            applied_patches: Mutex::new(HashSet::new()),
            resolved_by_wanted: Mutex::new(HashMap::new()),
            children_specs_by_id: Mutex::new(HashMap::new()),
        }
    }

    /// Attach the install's `patchedDependencies` map. When `Some`,
    /// the per-node walker looks every resolved `name@version` up via
    /// [`get_patch_info`] and appends `(patch_hash=<hash>)` to the
    /// `pkgIdWithPatchHash` on a match.
    pub fn with_patched_dependencies(
        mut self,
        patched_dependencies: Option<Arc<PatchGroupRecord>>,
    ) -> Self {
        self.patched_dependencies = patched_dependencies;
        self
    }

    /// Take ownership of `self` and emit the final [`ResolvedTree`]
    /// the peer-resolution stage consumes. The orchestrator passes its
    /// cumulative [`DirectDep`] list (initial walk + each hoist
    /// iteration's contributions) as `direct`.
    pub fn into_resolved_tree(self, direct: Vec<DirectDep>) -> ResolvedTree {
        // `std::sync::Mutex::into_inner` returns `Result` to surface
        // poisoning; recover from it the same way the per-acquire
        // `lock_recoverable` helper does so a panic in an unrelated
        // task doesn't escalate into a hard install failure here.
        ResolvedTree {
            direct,
            packages: self.packages.into_inner().unwrap_or_else(|err| err.into_inner()),
            dependencies_tree: self
                .dependencies_tree
                .into_inner()
                .unwrap_or_else(|err| err.into_inner()),
            all_peer_dep_names: self
                .all_peer_dep_names
                .into_inner()
                .unwrap_or_else(|err| err.into_inner()),
            policy_violations: self
                .policy_violations
                .into_inner()
                .unwrap_or_else(|err| err.into_inner()),
            applied_patches: self
                .applied_patches
                .into_inner()
                .unwrap_or_else(|err| err.into_inner()),
        }
    }

    /// Build a snapshot of the current tree state without consuming
    /// `self`. The orchestrator's hoist loop snapshots after each
    /// [`extend_tree`] call to run [`fn@crate::resolve_peers`] over the
    /// growing tree and find missing peers to hoist next.
    pub fn snapshot(&self, direct: Vec<DirectDep>) -> ResolvedTree {
        ResolvedTree {
            direct,
            packages: lock_recoverable(&self.packages).clone(),
            dependencies_tree: lock_recoverable(&self.dependencies_tree).clone(),
            all_peer_dep_names: lock_recoverable(&self.all_peer_dep_names).clone(),
            policy_violations: lock_recoverable(&self.policy_violations).clone(),
            applied_patches: lock_recoverable(&self.applied_patches).clone(),
        }
    }

    /// Iterate over every `(name, version)` pair the walk has resolved
    /// so far. Used by the orchestrator to keep `allPreferredVersions`
    /// in sync — mirrors upstream's resolveDependency-time push at
    /// [`resolveDependencies.ts:1440`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1440).
    pub fn resolved_versions(&self) -> Vec<(String, String)> {
        lock_recoverable(&self.packages)
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
    wanted: Vec<(String, String, bool)>,
) -> Result<Vec<DirectDep>, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    let results = wanted
        .into_iter()
        .map(|(name, range, optional)| async move {
            let wanted = WantedDependency {
                alias: Some(name),
                bare_specifier: Some(range),
                optional: Some(optional),
                ..WantedDependency::default()
            };
            resolve_node(ctx, resolver, wanted, &[], 0, false).await
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
) -> Result<Option<DirectDep>, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    let current_is_optional = wanted.optional.unwrap_or(false) || parent_optional;

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
    let cache_key: WantedKey =
        (wanted.alias.clone(), wanted.bare_specifier.clone(), wanted.optional);
    let cached = lock_recoverable(&ctx.resolved_by_wanted).get(&cache_key).map(Arc::clone);
    let result = match cached {
        Some(result) => result,
        None => {
            let result =
                resolver.resolve(&wanted, &ctx.base_opts).await.map_err(|err: ResolveError| {
                    ResolveDependencyTreeError::Resolve(err.to_string())
                })?;
            let Some(result) = result else {
                return Err(ResolveDependencyTreeError::SpecNotSupported {
                    specifier: render_specifier(&wanted),
                });
            };
            // Wrap in `Arc` once so the cache, the per-id
            // `ResolvedPackage` envelope, and the later peer-resolved
            // graph node share one heap-allocated `ResolveResult`
            // instead of cloning every `String` field per occurrence.
            let result = Arc::new(result);
            lock_recoverable(&ctx.resolved_by_wanted)
                .entry(cache_key)
                .or_insert_with(|| Arc::clone(&result));
            result
        }
    };

    if let Some(violation) = result.policy_violation.clone() {
        lock_recoverable(&ctx.policy_violations).push(violation);
    }

    if ctx.base_opts.block_exotic_subdeps
        && depth > 0
        && is_exotic_resolved_via(&result.resolved_via)
    {
        return Err(ResolveDependencyTreeError::ExoticSubdep {
            specifier: wanted.alias.clone().or(wanted.bare_specifier.clone()).unwrap_or_default(),
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
        .or(wanted.alias.clone())
        .or_else(|| result.name_ver.as_ref().map(|nv| nv.name.to_string()))
        .unwrap_or_else(|| id.clone());

    // Build (or look up) the ResolvedPackage envelope. The first
    // visitor populates it; later visitors AND-fold the `optional`
    // flag so a single non-optional path flips it back to `false`.
    // Mirrors upstream's
    // [`resolvedPkgsById[...].optional = ... && currentIsOptional`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1630)
    // arm.
    {
        let mut packages = lock_recoverable(&ctx.packages);
        match packages.get_mut(&id) {
            Some(existing) => {
                existing.optional = existing.optional && current_is_optional;
            }
            None => {
                let peer_dependencies = extract_peer_dependencies(&result);
                // Collect peer names for the peer-resolution stage's
                // `parentPkgs` filter (only peers count as parents).
                {
                    let mut all_peers = lock_recoverable(&ctx.all_peer_dep_names);
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
                    },
                );
            }
        }
    }

    // Leaves (no deps / optional deps / peers / peerDependenciesMeta)
    // reuse the package id as their `NodeId`, collapsing every parent
    // edge onto one tree node. Non-leaves still get a fresh per-
    // occurrence id so the peer resolver can attach different peer
    // suffixes per call site. Mirrors upstream's
    // [`resolveDependencies.ts:1580`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1580).
    let is_leaf = pkg_is_leaf(&result);
    let node_id = if is_leaf { NodeId::leaf(&id) } else { NodeId::next() };

    let next_ancestors: Vec<String> =
        ancestor_ids.iter().cloned().chain(std::iter::once(id.clone())).collect();

    // Look up cached children specs first; only walk the manifest on
    // a miss. The cache value is held by `Arc` so revisits clone the
    // refcount instead of the inner `Vec<(String, String, bool)>`.
    let child_specs = {
        let cache = lock_recoverable(&ctx.children_specs_by_id);
        cache.get(&id).map(Arc::clone)
    };
    let child_specs = match child_specs {
        Some(specs) => specs,
        None => {
            let specs = Arc::new(extract_children(&result));
            lock_recoverable(&ctx.children_specs_by_id)
                .entry(id.clone())
                .or_insert_with(|| Arc::clone(&specs));
            specs
        }
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
                resolve_node(
                    ctx,
                    resolver,
                    child_wanted,
                    &next_ancestors,
                    depth + 1,
                    current_is_optional,
                )
                .await
            }
        })
        .pipe(future::try_join_all)
        .await?;
    let children: BTreeMap<String, NodeId> =
        child_results.into_iter().flatten().map(|dep| (dep.alias, dep.node_id)).collect();

    // Repeat-visit leaves collapse onto one tree node; keep the
    // shallowest depth seen so downstream consumers that read
    // `tree_node.depth` (the peer pass folds it onto the graph node's
    // `depth`) match upstream's
    // [`Math.min(...)` arm](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1636-L1637).
    // Per-occurrence counter ids are unique by construction, so the
    // `and_modify` arm is dead for non-leaves.
    lock_recoverable(&ctx.dependencies_tree)
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
    specs: Vec<(String, String, bool)>,
    catalogs: &Catalogs,
) -> Result<Vec<(String, String, bool)>, ResolveDependencyTreeError> {
    specs
        .into_iter()
        .map(|(name, range, optional)| {
            let wanted =
                CatalogWantedDependency { alias: name.clone(), bare_specifier: range.clone() };
            match resolve_from_catalog(catalogs, &wanted) {
                CatalogResolutionResult::Found(found) => {
                    Ok((name, found.resolution.specifier, optional))
                }
                CatalogResolutionResult::Unused => Ok((name, range, optional)),
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
    let (name, version) = match result.name_ver.as_ref() {
        Some(name_ver) => (name_ver.name.to_string(), name_ver.suffix.to_string()),
        None => return Ok(raw_id.to_string()),
    };
    let prefixed = if raw_id.starts_with(&format!("{name}@")) {
        raw_id.to_string()
    } else {
        format!("{name}@{raw_id}")
    };
    let Some(groups) = ctx.patched_dependencies.as_deref() else {
        return Ok(prefixed);
    };
    let Some(patch) = get_patch_info(Some(groups), &name, &version)? else {
        return Ok(prefixed);
    };
    lock_recoverable(&ctx.applied_patches).insert(patch.key.clone());
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
fn extract_children(result: &pacquet_resolving_resolver_base::ResolveResult) -> Vec<ChildSpec> {
    let Some(manifest) = result.manifest.as_ref() else { return Vec::new() };
    let mut out = Vec::new();
    collect_deps(manifest, "dependencies", false, &mut out);
    collect_deps(manifest, "optionalDependencies", true, &mut out);
    out
}

fn collect_deps(manifest: &Value, key: &str, optional: bool, out: &mut Vec<ChildSpec>) {
    let Some(map) = manifest.get(key).and_then(Value::as_object) else { return };
    for (name, range) in map {
        if let Some(range_str) = range.as_str() {
            out.push((name.clone(), range_str.to_string(), optional));
        }
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
                .or_insert(PeerDep { version: "*".to_string(), optional });
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
