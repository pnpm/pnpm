mod runtimes;

mod resolution;

mod catalogs;

mod workspace_injection;

mod importer_records;

mod installation;

mod lockfile;

mod links;

use super::{
    DependenciesGraphToLockfileError, GraphToLockfileOptions, ImporterLockfileInput,
    dependencies_graph_to_lockfile as try_dependencies_graph_to_lockfile,
};
use indexmap::IndexMap;
use pnpm_deps_path::DepPath;
use pnpm_lockfile::{
    DirectoryResolution, ImporterDepVersion, LockfileResolution, PkgName, PkgNameVer,
    ProjectSnapshot, RegistryResolution, ResolvedDependencyMap, ResolvedDependencySpec,
    TarballResolution,
};
use pnpm_package_manifest::PackageManifest;
use pnpm_resolving_deps_resolver::{
    DependenciesGraph, DependenciesGraphNode, PeerDep, UpdateReuseScope,
};
use pnpm_resolving_resolver_base::{PkgResolutionId, ResolveResult};
use rustc_hash::FxHashSet as HashSet;

static EMPTY_REGISTRY_OPTIONS: std::collections::BTreeMap<String, pnpm_lockfile::RegistryOptions> =
    std::collections::BTreeMap::new();

static EMPTY_NAMED_REGISTRIES: std::sync::LazyLock<std::collections::HashMap<String, String>> =
    std::sync::LazyLock::new(std::collections::HashMap::new);
use serde_json::json;
use ssri::Integrity;
use std::{collections::BTreeMap, str::FromStr, sync::Arc};
use tempfile::TempDir;

fn dependencies_graph_to_lockfile(opts: GraphToLockfileOptions<'_>) -> pnpm_lockfile::Lockfile {
    try_dependencies_graph_to_lockfile(opts).expect("convert dependency graph to lockfile")
}

/// Shared empty catalogs for the catalog-free fixtures in this module.
static EMPTY_CATALOGS: pnpm_catalogs_types::Catalogs = BTreeMap::new();

/// Build a single-importer [`GraphToLockfileOptions`] under the root key
/// (`"."`). Every existing test exercises the single-importer shape;
/// multi-importer cases are constructed inline.
fn single_importer_opts<'a>(
    manifest: &'a PackageManifest,
    graph: &'a DependenciesGraph,
    direct: BTreeMap<String, DepPath>,
    auto_install_peers: bool,
    exclude_links_from_lockfile: bool,
    overrides: Option<IndexMap<String, String>>,
    ignored_optional_dependencies: Option<Vec<String>>,
) -> GraphToLockfileOptions<'a> {
    let mut importers = BTreeMap::new();
    importers.insert(
        ".".to_string(),
        ImporterLockfileInput { manifest, direct_dependencies_by_alias: direct },
    );
    GraphToLockfileOptions {
        registries_by_prefix: &EMPTY_NAMED_REGISTRIES,
        registry_options_by_url: &EMPTY_REGISTRY_OPTIONS,
        importers,
        graph,
        auto_install_peers,
        dedupe_peers: false,
        exclude_links_from_lockfile,
        inject_workspace_packages: false,
        peers_suffix_max_length: None,
        overrides,
        ignored_optional_dependencies,
        patched_dependencies: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        catalogs: &EMPTY_CATALOGS,
        registry: "https://registry.npmjs.org",
        lockfile_include_tarball_url: false,
        previous_importers: None,
        previous_packages: None,
        update_reuse_scope: UpdateReuseScope::All,
        update_reuse_scopes_by_importer: BTreeMap::new(),
        time: BTreeMap::new(),
    }
}

const FAKE_INTEGRITY: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

fn make_registry_resolution() -> LockfileResolution {
    LockfileResolution::Registry(RegistryResolution {
        integrity: Integrity::from_str(FAKE_INTEGRITY).expect("parse fake integrity"),
        revision: None,
    })
}

fn make_resolve_result(name: &str, version: &str, manifest: serde_json::Value) -> ResolveResult {
    let name_ver: PkgNameVer = format!("{name}@{version}").parse().expect("parse fake PkgNameVer");
    ResolveResult {
        id: (&name_ver).into(),
        name_ver: Some(name_ver),
        latest: None,
        published_at: None,
        manifest: Some(std::sync::Arc::new(manifest)),
        resolution: make_registry_resolution(),
        resolved_via: "npm-registry".to_string(),
        normalized_bare_specifier: None,
        alias: Some(name.to_string()),
        policy_violation: None,
    }
}

fn make_node(
    name: &str,
    version: &str,
    manifest: serde_json::Value,
    children: BTreeMap<String, DepPath>,
    peer_dependencies: BTreeMap<String, PeerDep>,
    transitive_peer_dependencies: HashSet<String>,
) -> DependenciesGraphNode {
    make_node_with_optional(
        name,
        version,
        manifest,
        children,
        peer_dependencies,
        transitive_peer_dependencies,
        false,
    )
}

fn make_node_with_optional(
    name: &str,
    version: &str,
    manifest: serde_json::Value,
    children: BTreeMap<String, DepPath>,
    peer_dependencies: BTreeMap<String, PeerDep>,
    transitive_peer_dependencies: HashSet<String>,
    optional: bool,
) -> DependenciesGraphNode {
    let dep_path = DepPath::from(format!("{name}@{version}"));
    DependenciesGraphNode {
        dep_path,
        resolved_package_id: format!("{name}@{version}"),
        resolve_result: std::sync::Arc::new(make_resolve_result(name, version, manifest)),
        children,
        optional_children: HashSet::default(),
        peer_dependencies,
        transitive_peer_dependencies,
        resolved_peer_names: HashSet::default(),
        depth: 1,
        installable: true,
        is_pure: true,
        optional,
    }
}

/// Write a `package.json` to a temp dir and return the loaded manifest.
#[expect(
    clippy::needless_pass_by_value,
    reason = "test helper called from multiple sites with owned literals; by-value keeps the call sites clean"
)]
fn write_manifest(deps_value: serde_json::Value) -> (TempDir, PackageManifest) {
    let tmp = TempDir::new().expect("create tempdir");
    let manifest_path = tmp.path().join("package.json");
    std::fs::write(&manifest_path, serde_json::to_string_pretty(&deps_value).unwrap())
        .expect("write manifest");
    let manifest = PackageManifest::from_path(manifest_path).expect("read manifest");
    (tmp, manifest)
}

const GIT_TARBALL_URL: &str =
    "https://codeload.github.com/kevva/is-negative/tar.gz/163360a8d3ae6bee9524541043197ff356f8ed99";

/// A git-hosted node as the resolve pass produces it: the dep path is
/// the `<name>@<archive-url>` `build_pkg_id_with_patch_hash` derives
/// from the archive's own `package.json`, which the git resolver reads
/// during resolution.
fn git_hosted_node(alias: &str) -> (DepPath, DependenciesGraphNode) {
    let dep_path = DepPath::from(format!("is-negative@{GIT_TARBALL_URL}"));
    let resolve_result = ResolveResult {
        id: PkgResolutionId::from(GIT_TARBALL_URL),
        name_ver: None,
        latest: None,
        published_at: None,
        manifest: Some(Arc::new(json!({ "name": "is-negative", "version": "1.0.0" }))),
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball: GIT_TARBALL_URL.to_string(),
            integrity: None,
            revision: None,
            git_hosted: Some(true),
            path: None,
        }),
        resolved_via: "git-repository".to_string(),
        normalized_bare_specifier: Some("github:kevva/is-negative#1.0.0".to_string()),
        alias: Some(alias.to_string()),
        policy_violation: None,
    };
    let node = DependenciesGraphNode {
        dep_path: dep_path.clone(),
        resolved_package_id: dep_path.to_string(),
        resolve_result: Arc::new(resolve_result),
        children: BTreeMap::new(),
        optional_children: HashSet::default(),
        peer_dependencies: BTreeMap::new(),
        transitive_peer_dependencies: HashSet::default(),
        resolved_peer_names: HashSet::default(),
        depth: 1,
        installable: true,
        is_pure: true,
        optional: false,
    };
    (dep_path, node)
}

fn error_from_single_node_graph(alias: &str, dep_path: &str) -> DependenciesGraphToLockfileError {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": { alias: "^1.0.0" },
    }));
    let dep_path = DepPath::from(dep_path.to_string());
    let mut node = make_node(
        alias,
        "1.0.0",
        json!({ "name": alias, "version": "1.0.0" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::default(),
    );
    node.dep_path = dep_path.clone();

    let mut graph = DependenciesGraph::default();
    graph.insert(dep_path.clone(), node);
    let direct = BTreeMap::from([(alias.to_string(), dep_path)]);

    dbg!(
        try_dependencies_graph_to_lockfile(single_importer_opts(
            &manifest, &graph, direct, false, false, None, None,
        ))
        .unwrap_err(),
    )
}

/// Build a fake `DependenciesGraphNode` whose id is a `link:` workspace
/// reference. The local resolver produces these for `workspace:` specs
/// and leaves `name_ver` as `None`. Used in the link-shape lockfile
/// tests below.
fn make_link_node(target: &str, manifest: serde_json::Value) -> DependenciesGraphNode {
    let id_text = format!("link:{target}");
    let resolve_result = ResolveResult {
        id: PkgResolutionId::from(id_text.clone()),
        name_ver: None,
        latest: None,
        published_at: None,
        manifest: Some(std::sync::Arc::new(manifest)),
        resolution: LockfileResolution::Directory(DirectoryResolution {
            directory: target.to_string(),
        }),
        resolved_via: "workspace".to_string(),
        normalized_bare_specifier: None,
        alias: None,
        policy_violation: None,
    };
    DependenciesGraphNode {
        dep_path: DepPath::from(id_text.clone()),
        resolved_package_id: id_text,
        resolve_result: std::sync::Arc::new(resolve_result),
        children: BTreeMap::new(),
        optional_children: HashSet::default(),
        peer_dependencies: BTreeMap::new(),
        transitive_peer_dependencies: HashSet::default(),
        resolved_peer_names: HashSet::default(),
        depth: 0,
        installable: true,
        is_pure: true,
        optional: false,
    }
}

/// Build a fake `DependenciesGraphNode` for a package resolved from a
/// local directory (`file:<dir>`). The local resolver keys these by
/// `<name>@file:<dir>` and leaves `name_ver` as `None` — the name lives
/// in the fetched manifest only.
fn make_file_node(name: &str, directory: &str) -> DependenciesGraphNode {
    let id_text = format!("file:{directory}");
    let dep_path = DepPath::from(format!("{name}@{id_text}"));
    let resolve_result = ResolveResult {
        id: PkgResolutionId::from(id_text),
        name_ver: None,
        latest: None,
        published_at: None,
        manifest: Some(Arc::new(json!({ "name": name, "version": "1.0.0" }))),
        resolution: LockfileResolution::Directory(DirectoryResolution {
            directory: directory.to_string(),
        }),
        resolved_via: "local-filesystem".to_string(),
        normalized_bare_specifier: None,
        alias: None,
        policy_violation: None,
    };
    DependenciesGraphNode {
        resolved_package_id: dep_path.to_string(),
        dep_path,
        resolve_result: Arc::new(resolve_result),
        children: BTreeMap::new(),
        optional_children: HashSet::default(),
        peer_dependencies: BTreeMap::new(),
        transitive_peer_dependencies: HashSet::default(),
        resolved_peer_names: HashSet::default(),
        depth: 1,
        installable: true,
        is_pure: true,
        optional: false,
    }
}

/// A single-importer previous lockfile whose `alias` dependency was
/// recorded as `link:<target>`.
fn previous_importers_with_link(
    alias: &str,
    specifier: &str,
    target: &str,
) -> std::collections::HashMap<String, ProjectSnapshot> {
    let mut deps = ResolvedDependencyMap::new();
    deps.insert(
        PkgName::parse(alias).unwrap(),
        ResolvedDependencySpec {
            specifier: specifier.to_string(),
            version: ImporterDepVersion::Link(target.to_string()),
        },
    );
    let snapshot = ProjectSnapshot { dependencies: Some(deps), ..Default::default() };
    let mut importers = std::collections::HashMap::new();
    importers.insert(".".to_string(), snapshot);
    importers
}

/// A `consumer -> n` edge whose fresh resolution is a divergent `file:`
/// injection, with a previous lockfile that recorded it as `link:`.
/// Shared by the guard tests below.
fn injected_link_fixture()
-> (TempDir, PackageManifest, DependenciesGraph, BTreeMap<String, DepPath>) {
    let (tmp, manifest) = write_manifest(json!({
        "name": "consumer",
        "version": "1.0.0",
        "dependencies": { "n": "workspace:*" },
    }));
    let file_node = make_file_node("n", "packages/n");
    let mut graph = DependenciesGraph::default();
    graph.insert(file_node.dep_path.clone(), file_node.clone());
    let mut direct = BTreeMap::new();
    direct.insert("n".to_string(), file_node.dep_path);
    (tmp, manifest, graph, direct)
}

/// Build a node whose depPath is registry-qualified
/// (`<name>@<registryName>:<version>`, lockfile format 12.0) and whose
/// resolution carries `tarball_url`.
fn make_named_registry_node(
    name: &str,
    registry_name: &str,
    version: &str,
    tarball_url: &str,
) -> DependenciesGraphNode {
    let dep_path = DepPath::from(format!("{name}@{registry_name}:{version}"));
    let name_ver: PkgNameVer = format!("{name}@{version}").parse().expect("parse PkgNameVer");
    let resolve_result = ResolveResult {
        id: PkgResolutionId::from(format!("{name}@{registry_name}:{version}")),
        name_ver: Some(name_ver),
        latest: None,
        published_at: None,
        manifest: Some(std::sync::Arc::new(json!({ "name": name, "version": version }))),
        resolution: LockfileResolution::Tarball(TarballResolution {
            tarball: tarball_url.to_string(),
            integrity: Some(Integrity::from_str(FAKE_INTEGRITY).expect("parse fake integrity")),
            revision: None,
            git_hosted: None,
            path: None,
        }),
        resolved_via: "named-registry".to_string(),
        normalized_bare_specifier: None,
        alias: Some(name.to_string()),
        policy_violation: None,
    };
    DependenciesGraphNode {
        dep_path,
        resolved_package_id: format!("{name}@{registry_name}:{version}"),
        resolve_result: std::sync::Arc::new(resolve_result),
        children: BTreeMap::new(),
        optional_children: HashSet::default(),
        peer_dependencies: BTreeMap::new(),
        transitive_peer_dependencies: HashSet::default(),
        resolved_peer_names: HashSet::default(),
        depth: 1,
        installable: true,
        is_pure: true,
        optional: false,
    }
}

fn named_registries_with(
    registry_name: &str,
    url: &str,
) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    map.insert(registry_name.to_string(), url.to_string());
    map
}
