mod overrides;

mod workspace_links;

mod resolution_order;

mod optional_dependencies;

mod peer_dependencies;

mod behavior;

use std::{str::FromStr, sync::Mutex, time::Duration};

use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_resolving_resolver_base::{
    LatestQuery, ResolveError, ResolveFuture, ResolveLatestFuture, ResolveOptions, ResolveResult,
    Resolver, WantedDependency,
};
use pretty_assertions::assert_eq;
use rustc_hash::FxHashMap as HashMap;

use crate::resolve_dependency_tree::{
    ResolveDependencyTreeError, ResolveDependencyTreeOptions, resolve_dependency_tree,
};

/// Stub resolver fed from a `(name, range)` → `ResolveResult` map.
/// Records each `(name, range)` query so tests can assert dedup.
struct StubResolver {
    table: HashMap<(String, String), ResolveResult>,
    calls: Mutex<Vec<(String, String)>>,
}

impl Resolver for StubResolver {
    fn resolve<'a>(
        &'a self,
        wanted: &'a WantedDependency,
        _opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        let key = (
            wanted.alias.clone().unwrap_or_default(),
            wanted.bare_specifier.clone().unwrap_or_default(),
        );
        self.calls.lock().unwrap().push(key.clone());
        let result = self.table.get(&key).cloned();
        Box::pin(async move { Ok::<_, ResolveError>(result) })
    }

    fn resolve_latest<'a>(
        &'a self,
        _query: &'a LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(async { Ok(None) })
    }
}

struct DelayedAliasResolver {
    table: HashMap<(String, String), ResolveResult>,
    delayed_alias: String,
}

impl Resolver for DelayedAliasResolver {
    fn resolve<'a>(
        &'a self,
        wanted: &'a WantedDependency,
        _opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        let key = (
            wanted.alias.clone().unwrap_or_default(),
            wanted.bare_specifier.clone().unwrap_or_default(),
        );
        let result = self.table.get(&key).cloned();
        let should_delay = key.0 == self.delayed_alias;
        Box::pin(async move {
            if should_delay {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Ok::<_, ResolveError>(result)
        })
    }

    fn resolve_latest<'a>(
        &'a self,
        _query: &'a LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(async { Ok(None) })
    }
}

/// Stub resolver fed from a `name` → versions table, picking the way
/// the npm resolver does: the highest version the range admits, except
/// that a version the walk's preferred-versions overlay carries wins
/// over it. Lets a test vary one edge's pick by the level it resolves
/// under. `delayed` holds back one `(alias, range)` edge, so the
/// occurrence behind it reaches a shared package second.
struct OverlayPickResolver {
    versions: HashMap<String, Vec<ResolveResult>>,
    delayed: (String, String),
}

impl Resolver for OverlayPickResolver {
    fn resolve<'a>(
        &'a self,
        wanted: &'a WantedDependency,
        opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        let name = wanted.alias.clone().unwrap_or_default();
        let bare = wanted.bare_specifier.clone().unwrap_or_default();
        let range = node_semver::Range::from_str(&bare).expect("test range");
        let preferred: Vec<&str> = opts
            .preferred_versions_overlay
            .as_ref()
            .map(|overlay| overlay.versions_for(&name))
            .unwrap_or_default();
        let satisfying: Vec<&ResolveResult> = self
            .versions
            .get(&name)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter(|result| {
                result.name_ver.as_ref().is_some_and(|name_ver| range.satisfies(&name_ver.suffix))
            })
            .collect();
        let highest = |from: Vec<&ResolveResult>| {
            from.into_iter()
                .max_by(|left, right| {
                    version_of(left).partial_cmp(version_of(right)).expect("comparable versions")
                })
                .cloned()
        };
        let result = highest(
            satisfying
                .iter()
                .copied()
                .filter(|result| preferred.contains(&version_of(result).to_string().as_str()))
                .collect(),
        )
        .or_else(|| highest(satisfying));
        let delayed = (name, bare) == self.delayed;
        Box::pin(async move {
            if delayed {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Ok::<_, ResolveError>(result)
        })
    }

    fn resolve_latest<'a>(
        &'a self,
        _query: &'a LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(async { Ok(None) })
    }
}

fn version_of(result: &ResolveResult) -> &node_semver::Version {
    &result.name_ver.as_ref().expect("test result carries a name and version").suffix
}

/// The versions table both settlement tests resolve against: `pin` in
/// two versions, and a `shared` package whose own `pin` edge the level
/// it is reached from decides.
fn settlement_versions(
    extra: impl IntoIterator<Item = (String, Vec<ResolveResult>)>,
) -> HashMap<String, Vec<ResolveResult>> {
    let mut versions: HashMap<String, Vec<ResolveResult>> = HashMap::from_iter([
        (
            "shared".to_string(),
            vec![fake_result(
                "shared",
                "1.0.0",
                serde_json::json!({
                    "name": "shared",
                    "version": "1.0.0",
                    "dependencies": { "pin": "^1.0.0" }
                }),
            )],
        ),
        (
            "pin".to_string(),
            vec![
                fake_result(
                    "pin",
                    "1.0.0",
                    serde_json::json!({ "name": "pin", "version": "1.0.0" }),
                ),
                fake_result(
                    "pin",
                    "1.5.0",
                    serde_json::json!({ "name": "pin", "version": "1.5.0" }),
                ),
            ],
        ),
    ]);
    versions.extend(extra);
    versions
}

fn dependency_result(name: &str, dependencies: &serde_json::Value) -> (String, Vec<ResolveResult>) {
    (
        name.to_string(),
        vec![fake_result(
            name,
            "1.0.0",
            serde_json::json!({ "name": name, "version": "1.0.0", "dependencies": dependencies }),
        )],
    )
}

async fn resolve_settlement_tree(
    resolver: &OverlayPickResolver,
    root_deps: serde_json::Value,
) -> crate::ResolvedTree {
    let (_tmp, manifest) = fake_manifest(root_deps);
    resolve_dependency_tree(
        resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions::default(),
            patched_dependencies: None,
            manifest_hook: None,
            overrides_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .unwrap()
}

fn fake_result(name: &str, version: &str, manifest: serde_json::Value) -> ResolveResult {
    use pnpm_lockfile::{LockfileResolution, PkgName, PkgNameVer, TarballResolution};
    let name_ver = PkgNameVer::new(
        PkgName::parse(name).unwrap(),
        node_semver::Version::from_str(version).unwrap(),
    );
    ResolveResult {
        id: (&name_ver).into(),
        name_ver: Some(name_ver),
        latest: Some(version.to_string()),
        published_at: None,
        manifest: Some(std::sync::Arc::new(manifest)),
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball: format!("https://registry.example/{name}-{version}.tgz"),
            integrity: None,
            revision: None,
            git_hosted: None,
            path: None,
        }),
        resolved_via: "npm-registry".to_string(),
        normalized_bare_specifier: None,
        alias: Some(name.to_string()),
        policy_violation: None,
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "test helper called from multiple sites with owned literals; by-value keeps the call sites clean"
)]
fn fake_manifest(root_deps: serde_json::Value) -> (tempfile::TempDir, PackageManifest) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("package.json");
    let json = serde_json::json!({
        "name": "root",
        "version": "0.0.0",
        "dependencies": root_deps,
    });
    std::fs::write(&path, serde_json::to_string(&json).unwrap()).expect("write package.json");
    let manifest = PackageManifest::from_path(path).expect("parse package.json");
    (tmp, manifest)
}

mod block_exotic_subdeps;

mod peers;

mod patched_dependencies;

mod optional_propagation;

mod importer_wanted_specs;

mod peer_own_dep_shadowing;

mod level_preferred_versions;

mod cycle_edges;

/// The `(name, dir)` pairs recorded by [`RecordingHooks`].
pub(crate) type RecordedReadPackageCalls = std::sync::Arc<Mutex<Vec<(String, Option<String>)>>>;

/// [`pnpm_hooks::PnpmfileHooks`] stub that records the `(name, dir)` pair
/// of every `read_package` call and returns the manifest unchanged.
pub(crate) struct RecordingHooks {
    pub(crate) calls: RecordedReadPackageCalls,
}

#[async_trait::async_trait]
impl pnpm_hooks::PnpmfileHooks for RecordingHooks {
    async fn read_package(
        &self,
        pkg: serde_json::Value,
        ctx: pnpm_hooks::HookContext,
    ) -> Result<pnpm_hooks::ReadPackageResult, pnpm_hooks::HookError> {
        let name = pkg.get("name").and_then(|name| name.as_str()).unwrap_or_default().to_string();
        self.calls.lock().unwrap().push((name, ctx.dir));
        Ok(std::sync::Arc::new(pkg))
    }

    async fn after_all_resolved(
        &self,
        lockfile: serde_json::Value,
        _ctx: pnpm_hooks::HookContext,
    ) -> Result<serde_json::Value, pnpm_hooks::HookError> {
        Ok(lockfile)
    }

    async fn pre_resolution(
        &self,
        _ctx: pnpm_hooks::PreResolutionHookContext,
        _logger: pnpm_hooks::PreResolutionHookLogger,
    ) {
    }

    async fn filter_log(&self, _log: serde_json::Value, _ctx: pnpm_hooks::HookContext) -> bool {
        true
    }
}

/// [`pnpm_hooks::PnpmfileHooks`] stub whose `readPackage` replaces every
/// manifest wholesale, the way an embedder substitutes a workspace project's
/// raw manifest.
struct ReplacingHook {
    replacement: serde_json::Value,
}

#[async_trait::async_trait]
impl pnpm_hooks::PnpmfileHooks for ReplacingHook {
    async fn read_package(
        &self,
        _pkg: serde_json::Value,
        _ctx: pnpm_hooks::HookContext,
    ) -> Result<pnpm_hooks::ReadPackageResult, pnpm_hooks::HookError> {
        Ok(std::sync::Arc::new(self.replacement.clone()))
    }

    async fn after_all_resolved(
        &self,
        _lockfile: serde_json::Value,
        _ctx: pnpm_hooks::HookContext,
    ) -> Result<serde_json::Value, pnpm_hooks::HookError> {
        Ok(serde_json::Value::Null)
    }

    async fn pre_resolution(
        &self,
        _ctx: pnpm_hooks::PreResolutionHookContext,
        _logger: pnpm_hooks::PreResolutionHookLogger,
    ) {
    }

    async fn filter_log(&self, _log: serde_json::Value, _ctx: pnpm_hooks::HookContext) -> bool {
        true
    }
}
