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

/// Options threaded into [`resolve_dependency_tree`].
///
/// Mirrors upstream's per-importer options; pacquet's slice is single-
/// importer so the bag is smaller. `base_opts` is the [`ResolveOptions`]
/// every per-package `resolve()` call sees; the tree walker doesn't
/// mutate it.
#[derive(Debug)]
pub struct ResolveDependencyTreeOptions {
    /// When `true`, fold each visited package's `peerDependencies`
    /// into the regular dependency walk so peer packages get
    /// installed alongside their hosts. Pacquet's stand-in for
    /// upstream's
    /// [`hoistPeers`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/hoistPeers.ts)
    /// pre-pass until that full algorithm lands. Independent of
    /// peer-suffix construction in [`crate::resolve_peers`] — that
    /// stage runs regardless and produces the same depPath shape
    /// whether peers were installed automatically or supplied by the
    /// user.
    pub auto_install_peers: bool,
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
    let ctx = Ctx {
        auto_install_peers: opts.auto_install_peers,
        base_opts: opts.base_opts,
        packages: Mutex::new(HashMap::new()),
        dependencies_tree: Mutex::new(HashMap::new()),
        all_peer_dep_names: Mutex::new(HashSet::new()),
        policy_violations: Mutex::new(Vec::new()),
    };

    let direct_specs: Vec<(String, String)> = manifest
        .dependencies(dependency_groups)
        .map(|(name, range)| (name.to_string(), range.to_string()))
        .collect();

    let direct_results = direct_specs
        .into_iter()
        .map(|(name, range)| async {
            let wanted = WantedDependency {
                alias: Some(name),
                bare_specifier: Some(range),
                ..WantedDependency::default()
            };
            resolve_node(&ctx, resolver, wanted, &[], 0).await
        })
        .pipe(future::try_join_all)
        .await?;
    // Top-level cycles can't occur (the importer can't appear in its
    // own ancestor chain), but `resolve_node` may still return `None`
    // for any spec the cycle break gated out. Filter at the join.
    let direct: Vec<DirectDep> = direct_results.into_iter().flatten().collect();

    Ok(ResolvedTree {
        direct,
        packages: ctx.packages.into_inner(),
        dependencies_tree: ctx.dependencies_tree.into_inner(),
        all_peer_dep_names: ctx.all_peer_dep_names.into_inner(),
        policy_violations: ctx.policy_violations.into_inner(),
    })
}

struct Ctx {
    auto_install_peers: bool,
    base_opts: ResolveOptions,
    packages: Mutex<HashMap<String, ResolvedPackage>>,
    dependencies_tree: Mutex<HashMap<NodeId, DependenciesTreeNode>>,
    all_peer_dep_names: Mutex<HashSet<String>>,
    policy_violations: Mutex<Vec<pacquet_resolving_resolver_base::ResolutionPolicyViolation>>,
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
    ctx: &Ctx,
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

    let alias =
        result.alias.clone().or(wanted.alias.clone()).unwrap_or_else(|| result.id.name.to_string());

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

    let child_specs = extract_children(&result, ctx.auto_install_peers);
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

/// Extract regular `dependencies` from a resolved package's manifest,
/// optionally folding in `peerDependencies` when `auto_install_peers`
/// is on. Peers are still recorded on
/// [`ResolvedPackage::peer_dependencies`] regardless so the peer-
/// resolution stage can compute the correct depPath suffix.
fn extract_children(
    result: &pacquet_resolving_resolver_base::ResolveResult,
    auto_install_peers: bool,
) -> Vec<(String, String)> {
    let Some(manifest) = result.manifest.as_ref() else { return Vec::new() };
    let mut out = Vec::new();
    collect_deps(manifest, "dependencies", &mut out);
    if auto_install_peers {
        collect_deps(manifest, "peerDependencies", &mut out);
    }
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
