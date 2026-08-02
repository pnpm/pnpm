//! Multi-importer workspace resolution: the peer-hoist loop and the
//! importer barrier around it.
//!
//! Two sizes of the same fixture — a layered dependency graph shared by
//! many importers, peer hoisting enabled, no wanted lockfile — so the
//! whole graph resolves:
//!
//! - [`Size::Ci`] is what the criterion group measures on every PR. It is
//!   sized so a run costs a fraction of a second while still spreading
//!   enough importers over enough packages to expose per-importer work
//!   that scales with the workspace.
//! - [`Size::Full`] is a large real workspace's scale: 331 importers over
//!   5,000 packages. It is too slow and too memory-hungry for a
//!   statistical harness — a regressed run can take minutes and several
//!   GiB — so `--full-workspace-resolution` runs each shape once and
//!   prints its timing instead.
//!
//! The [`Shape::PeersHoisted`] shape is the one that runs the hoist loop.
//! The other two say nothing about it: with every peer provided up front
//! the loop converges in one round.

use criterion::Criterion;
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

const PACKAGES_PER_LAYER: usize = 20;
const CHILDREN_PER_PACKAGE: usize = 4;
/// Framework packages the peer-heavy scenario adds: importers depend on all
/// of them directly; every graph package peer-depends on a rotating subset.
const FRAMEWORK_COUNT: usize = 30;
const PEERS_PER_PACKAGE: usize = 8;
/// How many of the layer-0 roots each importer declares in the
/// hoist-heavy scenario, rotated per importer so the importers' forests
/// differ instead of every one resolving the same direct set.
const ROOTS_PER_IMPORTER: usize = 6;

/// How large a workspace the fixture models. The per-importer work the
/// hoist loop does scales with both factors, so both shrink for CI.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Ci,
    Full,
}

impl Size {
    fn importer_count(self) -> usize {
        match self {
            Size::Ci => 120,
            Size::Full => 331,
        }
    }

    fn layer_count(self) -> usize {
        match self {
            Size::Ci => 60,
            Size::Full => 250,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// No peers anywhere.
    Plain,
    /// Every package peer-depends on framework packages the importers
    /// declare directly, so nothing is ever missing.
    PeersProvided,
    /// The same peers, but no importer declares the frameworks: each one
    /// has to hoist them itself, so the hoist loop runs several rounds
    /// with tree extensions in between.
    PeersHoisted,
}

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

fn framework_name(index: usize) -> String {
    format!("framework-{index:02}")
}

fn framework_peers_for_package(layer: usize, index: usize) -> serde_json::Value {
    let peers = (0..PEERS_PER_PACKAGE)
        .map(|offset| {
            let framework = framework_name((layer + index + offset) % FRAMEWORK_COUNT);
            (framework, serde_json::Value::String("1.0.0".to_string()))
        })
        .collect();
    serde_json::Value::Object(peers)
}

fn dependencies_for_package(layer: usize, index: usize, size: Size) -> serde_json::Value {
    if layer + 1 == size.layer_count() {
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

fn graph_resolver(shape: Shape, size: Size) -> GraphResolver {
    let with_peers = shape != Shape::Plain;
    let mut packages: HashMap<String, ResolveResult> = (0..size.layer_count())
        .flat_map(|layer| {
            (0..PACKAGES_PER_LAYER).map(move |index| {
                let name = package_name(layer, index);
                let name_ver = PkgNameVer::new(
                    PkgName::parse(&name).expect("benchmark package name is valid"),
                    node_semver::Version::from_str("1.0.0")
                        .expect("benchmark package version is valid"),
                );
                let mut manifest = serde_json::json!({
                    "name": name,
                    "version": "1.0.0",
                    "dependencies": dependencies_for_package(layer, index, size),
                });
                if with_peers {
                    manifest["peerDependencies"] = framework_peers_for_package(layer, index);
                }
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
    if with_peers {
        for index in 0..FRAMEWORK_COUNT {
            let name = framework_name(index);
            let name_ver = PkgNameVer::new(
                PkgName::parse(&name).expect("benchmark framework name is valid"),
                node_semver::Version::from_str("1.0.0")
                    .expect("benchmark framework version is valid"),
            );
            let manifest = serde_json::json!({
                "name": name,
                "version": "1.0.0",
                "dependencies": {},
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
            packages.insert(name, result);
        }
    }
    GraphResolver { packages }
}

fn importer_manifest(index: usize, shape: Shape) -> PackageManifest {
    let roots: Vec<usize> = if shape == Shape::PeersHoisted {
        (0..ROOTS_PER_IMPORTER).map(|offset| (index + offset) % PACKAGES_PER_LAYER).collect()
    } else {
        (0..PACKAGES_PER_LAYER).collect()
    };
    let mut dependencies: serde_json::Map<String, serde_json::Value> = roots
        .into_iter()
        .map(|root| (package_name(0, root), serde_json::Value::String("1.0.0".to_string())))
        .collect();
    if shape == Shape::PeersProvided {
        for framework in 0..FRAMEWORK_COUNT {
            dependencies
                .insert(framework_name(framework), serde_json::Value::String("1.0.0".to_string()));
        }
    }
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
        all_preferred_versions: Arc::new(PreferredVersions::new()),
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
        overrides_hook: None,
        pnpmfile_hook: None,
    }
}

fn workspace_options() -> WorkspaceResolveOptions {
    WorkspaceResolveOptions {
        named_registries: HashMap::new(),
        dedupe_peers: true,
        dedupe_injected_deps: true,
        dedupe_peer_dependents: true,
        resolve_peers_from_workspace_root: false,
        exclude_links_from_lockfile: false,
        lockfile_dir: PathBuf::from("/workspace"),
        peers_suffix_max_length: 1000,
        manifest_hook: None,
        overrides_hook: None,
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

/// One resolve of the whole workspace. Everything before the resolve is
/// fixture construction, which the caller hoists out of the timed region.
fn resolve_once(
    runtime: &tokio::runtime::Runtime,
    importers: &[WorkspaceImporter<'_>],
    resolver: &GraphResolver,
) -> usize {
    runtime
        .block_on(resolve_workspace(
            resolver,
            importers,
            &[DependencyGroup::Prod],
            workspace_options(),
            importer_options,
        ))
        .expect("benchmark workspace resolves")
        .peers
        .graph
        .len()
}

struct Workspace {
    manifests: Vec<PackageManifest>,
    ids: Vec<String>,
    resolver: GraphResolver,
}

impl Workspace {
    fn new(shape: Shape, size: Size) -> Self {
        Workspace {
            manifests: (0..size.importer_count())
                .map(|index| importer_manifest(index, shape))
                .collect(),
            ids: (0..size.importer_count())
                .map(|index| format!("components/component-{index:03}"))
                .collect(),
            resolver: graph_resolver(shape, size),
        }
    }

    fn importers(&self) -> Vec<WorkspaceImporter<'_>> {
        self.ids
            .iter()
            .zip(&self.manifests)
            .map(|(id, manifest)| WorkspaceImporter { id: id.clone(), manifest })
            .collect()
    }
}

/// The CI-gated measurement. Only the hoist-heavy shape is benched: it is
/// the one whose cost the peer-hoist loop dominates, and the other two
/// shapes would spend the budget re-measuring the plain tree walk that
/// every other scenario already covers.
pub fn bench_workspace_resolution(criterion: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("create benchmark runtime");
    let workspace = Workspace::new(Shape::PeersHoisted, Size::Ci);
    let importers = workspace.importers();

    let mut group = criterion.benchmark_group("workspace_resolution");
    // A resolve costs a large fraction of a second, so criterion's default
    // 100 samples would put this one group in the minutes.
    group.sample_size(10);
    group.bench_function("hoist_heavy", |bencher| {
        bencher.iter(|| black_box(resolve_once(&runtime, &importers, &workspace.resolver)));
    });
    group.finish();
}

/// Run each shape once at [`Size::Full`] and print its timing. See the
/// module docs for why this size has no statistical harness.
pub fn run_full_workspace_resolution() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("create benchmark runtime");
    for (label, shape) in [
        ("full workspace resolution", Shape::Plain),
        ("peer-heavy workspace resolution", Shape::PeersProvided),
        ("hoist-heavy workspace resolution", Shape::PeersHoisted),
    ] {
        let workspace = Workspace::new(shape, Size::Full);
        let importers = workspace.importers();
        let started = Instant::now();
        let nodes = resolve_once(&runtime, &importers, &workspace.resolver);
        let elapsed = started.elapsed();
        println!(
            "{label}: {} importers, {} packages, {nodes} graph nodes in {elapsed:.2?}",
            Size::Full.importer_count(),
            workspace.resolver.packages.len(),
        );
    }
}
