use std::collections::{BTreeMap, HashMap, HashSet};

use async_recursion::async_recursion;
use derive_more::{Display, Error};
use futures_util::future;
use miette::Diagnostic;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_resolving_resolver_base::{ResolveError, ResolveOptions, Resolver, WantedDependency};
use pipe_trait::Pipe;
use serde_json::Value;
use tokio::sync::Mutex;

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
/// [`tokio::sync::Mutex`]: a sibling already resolving an id `X` makes
/// later visitors skip the recursion the in-flight task is running and
/// reuse the eventually-populated `ResolvedPackage`. Per-occurrence
/// tree nodes are still allocated for each visit — only the
/// `ResolvedPackage` envelope is shared.
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
    let ctx = TreeCtx::new(opts.base_opts);
    let wanted: Vec<(String, String)> = manifest
        .dependencies(dependency_groups)
        .map(|(name, range)| (name.to_string(), range.to_string()))
        .collect();
    let direct = extend_tree(&ctx, resolver, wanted).await?;
    Ok(ctx.into_resolved_tree(direct))
}

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
        }
    }

    /// Take ownership of `self` and emit the final [`ResolvedTree`]
    /// the peer-resolution stage consumes. The orchestrator passes its
    /// cumulative [`DirectDep`] list (initial walk + each hoist
    /// iteration's contributions) as `direct`.
    pub fn into_resolved_tree(self, direct: Vec<DirectDep>) -> ResolvedTree {
        ResolvedTree {
            direct,
            packages: self.packages.into_inner(),
            dependencies_tree: self.dependencies_tree.into_inner(),
            all_peer_dep_names: self.all_peer_dep_names.into_inner(),
            policy_violations: self.policy_violations.into_inner(),
        }
    }

    /// Build a snapshot of the current tree state without consuming
    /// `self`. The orchestrator's hoist loop snapshots after each
    /// [`extend_tree`] call to run [`fn@crate::resolve_peers`] over the
    /// growing tree and find missing peers to hoist next.
    pub async fn snapshot(&self, direct: Vec<DirectDep>) -> ResolvedTree {
        ResolvedTree {
            direct,
            packages: self.packages.lock().await.clone(),
            dependencies_tree: self.dependencies_tree.lock().await.clone(),
            all_peer_dep_names: self.all_peer_dep_names.lock().await.clone(),
            policy_violations: self.policy_violations.lock().await.clone(),
        }
    }

    /// Iterate over every `(name, version)` pair the walk has resolved
    /// so far. Used by the orchestrator to keep `allPreferredVersions`
    /// in sync — mirrors upstream's resolveDependency-time push at
    /// [`resolveDependencies.ts:1440`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolveDependencies.ts#L1440).
    pub async fn resolved_versions(&self) -> Vec<(String, String)> {
        self.packages
            .lock()
            .await
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
    wanted: Vec<(String, String)>,
) -> Result<Vec<DirectDep>, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    let results = wanted
        .into_iter()
        .map(|(name, range)| async {
            let wanted = WantedDependency {
                alias: Some(name),
                bare_specifier: Some(range),
                ..WantedDependency::default()
            };
            resolve_node(ctx, resolver, wanted, &[], 0).await
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
) -> Result<Option<DirectDep>, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    let result = resolver
        .resolve(&wanted, &ctx.base_opts)
        .await
        .map_err(|err: ResolveError| ResolveDependencyTreeError::Resolve(err.to_string()))?;
    let Some(result) = result else {
        return Err(ResolveDependencyTreeError::SpecNotSupported {
            specifier: render_specifier(&wanted),
        });
    };

    if let Some(violation) = result.policy_violation.clone() {
        ctx.policy_violations.lock().await.push(violation);
    }

    let id = result.id.to_string();

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
    // visitor populates it; later visitors collapse onto it.
    {
        let mut packages = ctx.packages.lock().await;
        if !packages.contains_key(&id) {
            let peer_dependencies = extract_peer_dependencies(&result);
            // Collect peer names for the peer-resolution stage's
            // `parentPkgs` filter (only peers count as parents).
            {
                let mut all_peers = ctx.all_peer_dep_names.lock().await;
                for name in peer_dependencies.keys() {
                    all_peers.insert(name.clone());
                }
            }
            packages.insert(
                id.clone(),
                ResolvedPackage { id: id.clone(), result: result.clone(), peer_dependencies },
            );
        }
    }

    // Allocate a fresh NodeId for this occurrence. Two parents sharing
    // the same `pkgIdWithPatchHash` get different `NodeId`s so the peer
    // resolver can attach different peer suffixes per call site.
    let node_id = NodeId::next();

    let next_ancestors: Vec<String> =
        ancestor_ids.iter().cloned().chain(std::iter::once(id.clone())).collect();

    let child_specs = extract_children(&result);
    let child_results =
        child_specs
            .into_iter()
            .map(|(child_name, child_range)| {
                let child_wanted = WantedDependency {
                    alias: Some(child_name),
                    bare_specifier: Some(child_range),
                    ..WantedDependency::default()
                };
                let next_ancestors = next_ancestors.clone();
                async move {
                    resolve_node(ctx, resolver, child_wanted, &next_ancestors, depth + 1).await
                }
            })
            .pipe(future::try_join_all)
            .await?;
    let children: BTreeMap<String, NodeId> =
        child_results.into_iter().flatten().map(|dep| (dep.alias, dep.node_id)).collect();

    ctx.dependencies_tree.lock().await.insert(
        node_id,
        DependenciesTreeNode {
            resolved_package_id: id.clone(),
            children,
            depth,
            installable: true,
        },
    );

    Ok(Some(DirectDep { alias, node_id, id }))
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
fn extract_children(
    result: &pacquet_resolving_resolver_base::ResolveResult,
) -> Vec<(String, String)> {
    let Some(manifest) = result.manifest.as_ref() else { return Vec::new() };
    let mut out = Vec::new();
    collect_deps(manifest, "dependencies", &mut out);
    collect_deps(manifest, "optionalDependencies", &mut out);
    out
}

fn collect_deps(manifest: &Value, key: &str, out: &mut Vec<(String, String)>) {
    let Some(map) = manifest.get(key).and_then(Value::as_object) else { return };
    for (name, range) in map {
        if let Some(range_str) = range.as_str() {
            out.push((name.clone(), range_str.to_string()));
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
