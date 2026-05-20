use std::sync::Arc;

use async_recursion::async_recursion;
use derive_more::{Display, Error};
use futures_util::future;
use miette::Diagnostic;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_resolving_resolver_base::{ResolveError, ResolveOptions, Resolver, WantedDependency};
use pipe_trait::Pipe;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::resolved_tree::{DirectDep, ResolvedPackage, ResolvedTree};

/// Options threaded into [`resolve_dependency_tree`].
///
/// Mirrors upstream's per-importer options; pacquet's slice is single-
/// importer so the bag is smaller. `base_opts` is the [`ResolveOptions`]
/// every per-package `resolve()` call sees; the tree walker doesn't
/// mutate it. `auto_install_peers` controls whether a parent's
/// `peerDependencies` are folded into its child set during the walk.
#[derive(Debug)]
pub struct ResolveDependencyTreeOptions {
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
}

/// Walk `manifest` plus the entries in `dependency_groups`, dispatch
/// each direct dep through `resolver`, recurse on each picked
/// package's own `dependencies` / `peerDependencies`, and return a
/// flat tree keyed by `name@version`.
///
/// Mirrors upstream's
/// [`resolveDependencyTree`](https://github.com/pnpm/pnpm/blob/f657b5cb44/installing/deps-resolver/src/resolveDependencyTree.ts#L172-L357)
/// for the npm-shaped slice pacquet currently exposes.
///
/// Resolves siblings in parallel via `try_join_all` at every level.
/// The per-package dedupe gate is a shared `HashMap` behind a
/// [`tokio::sync::Mutex`]: a sibling that's already resolving an id
/// `X` makes a later visitor see the placeholder, attach the
/// outstanding `DirectDep { id: X }`, and skip the recursion the
/// in-flight task is already running.
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
    let ctx = Arc::new(Ctx {
        auto_install_peers: opts.auto_install_peers,
        base_opts: opts.base_opts,
        packages: Mutex::new(std::collections::HashMap::new()),
        policy_violations: Mutex::new(Vec::new()),
    });

    let direct_specs: Vec<(String, String)> = manifest
        .dependencies(dependency_groups)
        .map(|(name, range)| (name.to_string(), range.to_string()))
        .collect();

    let direct = direct_specs
        .into_iter()
        .map(|(name, range)| {
            let ctx = Arc::clone(&ctx);
            async move {
                let wanted = WantedDependency {
                    alias: Some(name),
                    bare_specifier: Some(range),
                    ..WantedDependency::default()
                };
                resolve_node(&ctx, resolver, wanted).await
            }
        })
        .pipe(future::try_join_all)
        .await?;
    let direct: Vec<DirectDep> = direct.into_iter().flatten().collect();

    let ctx = Arc::try_unwrap(ctx).ok().expect("resolve tasks must drop their ctx clones");
    Ok(ResolvedTree {
        direct,
        packages: ctx.packages.into_inner(),
        policy_violations: ctx.policy_violations.into_inner(),
    })
}

struct Ctx {
    auto_install_peers: bool,
    base_opts: ResolveOptions,
    packages: Mutex<std::collections::HashMap<String, ResolvedPackage>>,
    policy_violations: Mutex<Vec<pacquet_resolving_resolver_base::ResolutionPolicyViolation>>,
}

#[async_recursion]
async fn resolve_node<Chain>(
    ctx: &Ctx,
    resolver: &Chain,
    wanted: WantedDependency,
) -> Result<Option<DirectDep>, ResolveDependencyTreeError>
where
    Chain: Resolver + ?Sized,
{
    let result = resolver
        .resolve(&wanted, &ctx.base_opts)
        .await
        .map_err(|err: ResolveError| ResolveDependencyTreeError::Resolve(err.to_string()))?;
    let Some(result) = result else { return Ok(None) };

    if let Some(violation) = result.policy_violation.clone() {
        ctx.policy_violations.lock().await.push(violation);
    }

    let id = result.id.to_string();
    let alias =
        result.alias.clone().or(wanted.alias.clone()).unwrap_or_else(|| result.id.name.to_string());

    // Insert a placeholder under the global lock so concurrent
    // resolves for the same id collapse to one walker. The first to
    // get past the gate populates the children; later visitors return
    // a `DirectDep` referencing the (eventually fully populated) id.
    {
        let mut packages = ctx.packages.lock().await;
        if packages.contains_key(&id) {
            return Ok(Some(DirectDep { alias, id }));
        }
        packages.insert(
            id.clone(),
            ResolvedPackage { id: id.clone(), result: result.clone(), children: Vec::new() },
        );
    }

    let child_specs = extract_children(&result, ctx.auto_install_peers);
    let children: Vec<_> = child_specs
        .into_iter()
        .map(|(child_name, child_range)| {
            let child_wanted = WantedDependency {
                alias: Some(child_name),
                bare_specifier: Some(child_range),
                ..WantedDependency::default()
            };
            resolve_node(ctx, resolver, child_wanted)
        })
        .pipe(future::try_join_all)
        .await?;
    let children: Vec<DirectDep> = children.into_iter().flatten().collect();

    ctx.packages.lock().await.get_mut(&id).expect("placeholder inserted above").children = children;

    Ok(Some(DirectDep { alias, id }))
}

/// Extract `(name, version_range)` pairs from a resolved package's
/// manifest fragment, filtered by `auto_install_peers` to optionally
/// fold `peerDependencies` into the child set.
fn extract_children(
    result: &pacquet_resolving_resolver_base::ResolveResult,
    with_peers: bool,
) -> Vec<(String, String)> {
    let Some(manifest) = result.manifest.as_ref() else { return Vec::new() };
    let mut out = Vec::new();
    collect_deps(manifest, "dependencies", &mut out);
    if with_peers {
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
