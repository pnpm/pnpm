//! Regression benchmark for the large multi-importer full-resolution path.
//!
//! This models the Bit workspace that exposed the regression:
//!
//! - 331 component importers;
//! - a shared 5,000-package layered dependency graph;
//! - 20 direct roots per importer;
//! - peer hoisting enabled, which runs the importer peer-discovery barrier;
//! - no wanted lockfile, so the whole graph is resolved.
//!
//! Run once (the benchmark intentionally has no statistical harness because a
//! regressed run can take minutes and several GiB):
//!
//! ```text
//! cargo bench -p pacquet-resolving-deps-resolver --bench workspace_full_resolution
//! ```

use std::{
    collections::{BTreeMap, HashMap},
    hint::black_box,
    path::PathBuf,
    str::FromStr,
    sync::Arc,
    time::Instant,
};

use pacquet_lockfile::{LockfileResolution, PkgName, PkgNameVer, TarballResolution};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_resolving_deps_resolver::{
    ResolveImporterOptions, UpdateDepth, UpdateReuseScope, WorkspaceImporter,
    WorkspaceResolveOptions, resolve_workspace,
};
use pacquet_resolving_resolver_base::{
    LatestQuery, PkgResolutionId, PreferredVersions, ResolveError, ResolveFuture,
    ResolveLatestFuture, ResolveOptions, ResolveResult, Resolver, WantedDependency,
};

const IMPORTER_COUNT: usize = 331;
const LAYER_COUNT: usize = 250;
const PACKAGES_PER_LAYER: usize = 20;
const CHILDREN_PER_PACKAGE: usize = 4;

struct GraphResolver {
    packages: HashMap<String, ResolveResult>,
}

impl Resolver for GraphResolver {
    fn resolve<'a>(
        &'a self,
        wanted: &'a WantedDependency,
        _opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        let result = wanted.alias.as_ref().and_then(|alias| self.packages.get(alias)).cloned();
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

fn package_name(layer: usize, index: usize) -> String {
    format!("pkg-{layer:02}-{index:02}")
}

fn dependencies_for_package(layer: usize, index: usize) -> serde_json::Value {
    if layer + 1 == LAYER_COUNT {
        return serde_json::Value::Object(serde_json::Map::new());
    }
    let dependencies = (0..CHILDREN_PER_PACKAGE)
        .map(|offset| {
            let child = package_name(layer + 1, (index + offset) % PACKAGES_PER_LAYER);
            (child, serde_json::Value::String("1.0.0".to_string()))
        })
        .collect();
    serde_json::Value::Object(dependencies)
}

fn graph_resolver() -> GraphResolver {
    let packages = (0..LAYER_COUNT)
        .flat_map(|layer| {
            (0..PACKAGES_PER_LAYER).map(move |index| {
                let name = package_name(layer, index);
                let name_ver = PkgNameVer::new(
                    PkgName::parse(&name).expect("benchmark package name is valid"),
                    node_semver::Version::from_str("1.0.0")
                        .expect("benchmark package version is valid"),
                );
                let manifest = serde_json::json!({
                    "name": name,
                    "version": "1.0.0",
                    "dependencies": dependencies_for_package(layer, index),
                });
                let result = ResolveResult {
                    id: PkgResolutionId::from(&name_ver),
                    name_ver: Some(name_ver),
                    latest: Some("1.0.0".to_string()),
                    published_at: None,
                    manifest: Some(Arc::new(manifest)),
                    resolution: LockfileResolution::Tarball(TarballResolution {
                        tarball: format!("https://registry.example/{name}-1.0.0.tgz"),
                        integrity: None,
                        git_hosted: None,
                        path: None,
                    }),
                    resolved_via: "npm-registry".to_string(),
                    normalized_bare_specifier: None,
                    alias: Some(name.clone()),
                    policy_violation: None,
                };
                (name, result)
            })
        })
        .collect();
    GraphResolver { packages }
}

fn importer_manifest(index: usize) -> PackageManifest {
    let dependencies = (0..PACKAGES_PER_LAYER)
        .map(|root| (package_name(0, root), serde_json::Value::String("1.0.0".to_string())))
        .collect();
    PackageManifest::from_value(
        PathBuf::from(format!("/workspace/component-{index:03}/package.json")),
        serde_json::json!({
            "name": format!("component-{index:03}"),
            "version": "1.0.0",
            "dependencies": serde_json::Value::Object(dependencies),
        }),
    )
}

fn importer_options(importer: &WorkspaceImporter<'_>) -> ResolveImporterOptions {
    ResolveImporterOptions {
        auto_install_peers: true,
        auto_install_peers_from_highest_match: false,
        resolve_peers_from_workspace_root: false,
        dedupe_peers: true,
        dedupe_peer_dependents: true,
        all_preferred_versions: PreferredVersions::new(),
        override_bare_specifier: None,
        patched_dependencies: None,
        base_opts: ResolveOptions {
            project_dir: PathBuf::from("/workspace").join(&importer.id),
            ..ResolveOptions::default()
        },
        pick_lowest_direct: false,
        subdep_published_by: None,
        catalogs: pacquet_catalogs_types::Catalogs::new(),
        exclude_links_from_lockfile: false,
        lockfile_dir: Some(PathBuf::from("/workspace")),
        modules_dir: Some(PathBuf::from("/workspace/node_modules")),
        peers_suffix_max_length: 1000,
        catalog_server: false,
        manifest_hook: None,
        pnpmfile_hook: None,
    }
}

fn workspace_options() -> WorkspaceResolveOptions {
    WorkspaceResolveOptions {
        dedupe_peers: true,
        dedupe_injected_deps: true,
        dedupe_peer_dependents: true,
        resolve_peers_from_workspace_root: false,
        exclude_links_from_lockfile: false,
        lockfile_dir: PathBuf::from("/workspace"),
        peers_suffix_max_length: 1000,
        manifest_hook: None,
        pick_lowest_direct: false,
        time_based: false,
        wanted_lockfile: None,
        update_reuse_scope: UpdateReuseScope::All,
        update_reuse_scopes_by_importer: BTreeMap::new(),
        update_depth: UpdateDepth::UNLIMITED,
        pnpmfile_hook: None,
        read_package_log: None,
        skipped_optional_log: None,
        allowed_deprecated_versions: BTreeMap::new(),
        deprecation_log: None,
        auto_install_peers: true,
        registries: HashMap::new(),
    }
}

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("create benchmark runtime");
    let manifests: Vec<_> = (0..IMPORTER_COUNT).map(importer_manifest).collect();
    let importer_ids: Vec<_> =
        (0..IMPORTER_COUNT).map(|index| format!("components/component-{index:03}")).collect();
    let importers: Vec<_> = importer_ids
        .iter()
        .zip(&manifests)
        .map(|(id, manifest)| WorkspaceImporter { id: id.clone(), manifest })
        .collect();
    let resolver = graph_resolver();

    let started = Instant::now();
    let result = runtime
        .block_on(resolve_workspace(
            &resolver,
            &importers,
            &[DependencyGroup::Prod],
            workspace_options(),
            importer_options,
        ))
        .expect("benchmark workspace resolves");
    let elapsed = started.elapsed();
    black_box(&result);

    println!(
        "full workspace resolution: {IMPORTER_COUNT} importers, {} packages, {} graph nodes in {elapsed:.2?}",
        resolver.packages.len(),
        result.peers.graph.len(),
    );
}
