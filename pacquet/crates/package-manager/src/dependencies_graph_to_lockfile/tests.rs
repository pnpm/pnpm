use super::{GraphToLockfileOptions, ImporterLockfileInput, dependencies_graph_to_lockfile};
use indexmap::IndexMap;
use pacquet_deps_path::DepPath;
use pacquet_lockfile::{
    DirectoryResolution, ImporterDepVersion, LockfileResolution, PackageKey, PkgName, PkgNameVer,
    RegistryResolution, SnapshotDepRef, VariationsResolution,
};
use pacquet_package_manifest::PackageManifest;
use pacquet_resolving_deps_resolver::{DependenciesGraph, DependenciesGraphNode, PeerDep};
use pacquet_resolving_resolver_base::{PkgResolutionId, ResolveResult};
use serde_json::json;
use ssri::Integrity;
use std::{
    collections::{BTreeMap, HashSet},
    str::FromStr,
};
use tempfile::TempDir;

/// Shared empty catalogs for the catalog-free fixtures in this module.
static EMPTY_CATALOGS: pacquet_catalogs_types::Catalogs = BTreeMap::new();

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
    }
}

const FAKE_INTEGRITY: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

fn make_registry_resolution() -> LockfileResolution {
    LockfileResolution::Registry(RegistryResolution {
        integrity: Integrity::from_str(FAKE_INTEGRITY).expect("parse fake integrity"),
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
        peer_dependencies,
        transitive_peer_dependencies,
        resolved_peer_names: HashSet::new(),
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

/// Bare-bones fresh-install lockfile shape: one direct prod dep with no
/// transitive deps. Exercises the importer-side specifier wiring, the
/// regular (non-alias, non-peer) version cell, and the packages /
/// snapshots split.
#[test]
fn fresh_install_records_a_single_direct_dependency() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": { "react": "^17.0.2" },
    }));

    let node = make_node(
        "react",
        "17.0.2",
        json!({ "name": "react", "version": "17.0.2" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::new(),
    );

    let mut graph = DependenciesGraph::new();
    graph.insert(node.dep_path.clone(), node);

    let mut direct = BTreeMap::new();
    direct.insert("react".to_string(), DepPath::from("react@17.0.2".to_string()));

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, true, false, None, None,
    ));

    assert_eq!(lockfile.lockfile_version.major, 9);

    let importer = lockfile.root_project().expect("root importer exists");
    let dependencies = importer.dependencies.as_ref().expect("dependencies map exists");
    let react_key = PkgName::parse("react").unwrap();
    let entry = dependencies.get(&react_key).expect("react entry");
    assert_eq!(entry.specifier, "^17.0.2");
    assert!(matches!(&entry.version, ImporterDepVersion::Regular(_)));

    let packages = lockfile.packages.as_ref().expect("packages map");
    let metadata_key: PackageKey = "react@17.0.2".parse().unwrap();
    assert!(packages.contains_key(&metadata_key));

    let snapshots = lockfile.snapshots.as_ref().expect("snapshots map");
    assert!(snapshots.contains_key(&metadata_key));
    let snapshot = &snapshots[&metadata_key];
    assert!(snapshot.dependencies.is_none());
    assert!(snapshot.optional_dependencies.is_none());
    assert!(snapshot.transitive_peer_dependencies.is_none());
}

/// `dedupePeers: true` round-trips through the lockfile's
/// `settings:` block; `false` (the default) omits the key entirely.
/// Mirrors upstream's
/// [`dedupePeers: opts.dedupePeers || undefined`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-installer/src/install/index.ts#L602)
/// and the
/// [`'dedupePeers: version-only peer suffixes' install test`](https://github.com/pnpm/pnpm/blob/39101f5e37/installing/deps-installer/test/install/peerDependencies.ts#L2064-L2093)
/// `expect(lockfile.settings.dedupePeers).toBe(true)` assertion. The
/// omission case ports the
/// [`lockfile/fs/test/write.test.ts:106-140`](https://github.com/pnpm/pnpm/blob/39101f5e37/lockfile/fs/test/write.test.ts#L106-L140)
/// `'dedupePeers' in (written.settings ?? {})` assertion.
#[test]
fn dedupe_peers_round_trips_through_lockfile_settings() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
    }));
    let graph = DependenciesGraph::new();
    let direct = BTreeMap::new();

    let mut importers = BTreeMap::new();
    importers.insert(
        ".".to_string(),
        ImporterLockfileInput { manifest: &manifest, direct_dependencies_by_alias: direct.clone() },
    );
    let on = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        importers,
        graph: &graph,
        auto_install_peers: false,
        dedupe_peers: true,
        exclude_links_from_lockfile: false,
        inject_workspace_packages: false,
        peers_suffix_max_length: None,
        overrides: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        catalogs: &EMPTY_CATALOGS,
        registry: "https://registry.npmjs.org",
        lockfile_include_tarball_url: false,
    });
    let on_settings = on.settings.as_ref().expect("settings written");
    assert_eq!(on_settings.dedupe_peers, Some(true));
    let on_yaml = serde_saphyr::to_string(on_settings).unwrap();
    assert!(on_yaml.contains("dedupePeers: true"), "yaml: {on_yaml}");

    let mut importers = BTreeMap::new();
    importers.insert(
        ".".to_string(),
        ImporterLockfileInput { manifest: &manifest, direct_dependencies_by_alias: direct },
    );
    let off = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        importers,
        graph: &graph,
        auto_install_peers: false,
        dedupe_peers: false,
        exclude_links_from_lockfile: false,
        inject_workspace_packages: false,
        peers_suffix_max_length: None,
        overrides: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        catalogs: &EMPTY_CATALOGS,
        registry: "https://registry.npmjs.org",
        lockfile_include_tarball_url: false,
    });
    let off_settings = off.settings.as_ref().expect("settings written");
    assert_eq!(off_settings.dedupe_peers, None);
    let off_yaml = serde_saphyr::to_string(off_settings).unwrap();
    assert!(!off_yaml.contains("dedupePeers"), "yaml: {off_yaml}");
}

/// A non-empty `patched_dependencies` map flows verbatim into the
/// lockfile's top-level `patchedDependencies` block; an empty map is
/// normalized to `None` so the key is omitted on serialization.
#[test]
fn patched_dependencies_flow_into_lockfile_and_empty_is_omitted() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": { "react": "^17.0.2" },
    }));
    let node = make_node(
        "react",
        "17.0.2",
        json!({ "name": "react", "version": "17.0.2" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::new(),
    );
    let mut graph = DependenciesGraph::new();
    graph.insert(node.dep_path.clone(), node);
    let mut direct = BTreeMap::new();
    direct.insert("react".to_string(), DepPath::from("react@17.0.2".to_string()));

    let build = |patched: Option<BTreeMap<String, String>>| {
        let mut importers = BTreeMap::new();
        importers.insert(
            ".".to_string(),
            ImporterLockfileInput {
                manifest: &manifest,
                direct_dependencies_by_alias: direct.clone(),
            },
        );
        dependencies_graph_to_lockfile(GraphToLockfileOptions {
            importers,
            graph: &graph,
            auto_install_peers: false,
            dedupe_peers: false,
            exclude_links_from_lockfile: false,
            inject_workspace_packages: false,
            peers_suffix_max_length: None,
            overrides: None,
            ignored_optional_dependencies: None,
            patched_dependencies: patched,
            package_extensions_checksum: None,
            pnpmfile_checksum: None,
            catalogs: &EMPTY_CATALOGS,
            registry: "https://registry.npmjs.org",
            lockfile_include_tarball_url: false,
        })
    };

    let with_patch = build(Some(BTreeMap::from([(
        "graceful-fs@4.2.11".to_string(),
        "68ebc232025360cb3dcd3081f4067f4e9fc022ab6b6f71a3230e86c7a5b337d1".to_string(),
    )])));
    assert_eq!(
        with_patch
            .patched_dependencies
            .as_ref()
            .and_then(|map| map.get("graceful-fs@4.2.11"))
            .map(String::as_str),
        Some("68ebc232025360cb3dcd3081f4067f4e9fc022ab6b6f71a3230e86c7a5b337d1"),
    );

    assert!(build(Some(BTreeMap::new())).patched_dependencies.is_none());
    assert!(build(None).patched_dependencies.is_none());
}

/// `dev` and `optional` direct dependencies land in their own importer
/// sections — `devDependencies` and `optionalDependencies` — and are
/// kept out of the plain `dependencies` map.
#[test]
fn dev_and_optional_direct_deps_split_into_distinct_importer_sections() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "devDependencies": { "typescript": "^5.1.6" },
        "optionalDependencies": { "fsevents": "^2.3.2" },
    }));

    let typescript = make_node(
        "typescript",
        "5.1.6",
        json!({ "name": "typescript", "version": "5.1.6", "bin": "typescript.js" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::new(),
    );
    let fsevents = make_node(
        "fsevents",
        "2.3.2",
        json!({ "name": "fsevents", "version": "2.3.2", "os": ["darwin"] }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::new(),
    );

    let mut graph = DependenciesGraph::new();
    graph.insert(typescript.dep_path.clone(), typescript);
    graph.insert(fsevents.dep_path.clone(), fsevents);

    let mut direct = BTreeMap::new();
    direct.insert("typescript".to_string(), DepPath::from("typescript@5.1.6".to_string()));
    direct.insert("fsevents".to_string(), DepPath::from("fsevents@2.3.2".to_string()));

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, false, false, None, None,
    ));

    let importer = lockfile.root_project().expect("root importer");
    assert!(importer.dependencies.is_none(), "no prod deps declared");
    let dev = importer.dev_dependencies.as_ref().expect("dev deps");
    assert!(dev.contains_key(&PkgName::parse("typescript").unwrap()));
    let opt = importer.optional_dependencies.as_ref().expect("optional deps");
    assert!(opt.contains_key(&PkgName::parse("fsevents").unwrap()));

    // hasBin / os surface on `packages:` metadata.
    let packages = lockfile.packages.as_ref().unwrap();
    let typescript_key: PackageKey = "typescript@5.1.6".parse().unwrap();
    assert_eq!(packages[&typescript_key].has_bin, Some(true));
    let fsevents_key: PackageKey = "fsevents@2.3.2".parse().unwrap();
    assert_eq!(packages[&fsevents_key].os.as_deref(), Some(["darwin".to_string()].as_slice()));
}

/// A `catalog:` dependency resolved through an `npm:` alias still records a
/// `catalogs:` snapshot entry — `{ specifier: npm:@zkochan/js-yaml@0.0.11,
/// version: 0.0.11 }`. The importer stores the aliased dep as
/// [`ImporterDepVersion::Alias`], so the version must be read via `ver_peer`
/// (the alias's suffix), not `as_regular` which returns `None` for aliases
/// and silently dropped the entry.
#[test]
fn aliased_catalog_dependency_records_catalog_snapshot() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": { "js-yaml": "catalog:" },
    }));

    let zkochan_js_yaml = make_node(
        "@zkochan/js-yaml",
        "0.0.11",
        json!({ "name": "@zkochan/js-yaml", "version": "0.0.11" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::new(),
    );
    let mut graph = DependenciesGraph::new();
    graph.insert(zkochan_js_yaml.dep_path.clone(), zkochan_js_yaml);

    let mut direct = BTreeMap::new();
    direct.insert("js-yaml".to_string(), DepPath::from("@zkochan/js-yaml@0.0.11".to_string()));

    let mut catalogs: pacquet_catalogs_types::Catalogs = BTreeMap::new();
    catalogs
        .entry("default".to_string())
        .or_default()
        .insert("js-yaml".to_string(), "npm:@zkochan/js-yaml@0.0.11".to_string());

    let mut importers = BTreeMap::new();
    importers.insert(
        ".".to_string(),
        ImporterLockfileInput { manifest: &manifest, direct_dependencies_by_alias: direct },
    );
    let lockfile = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        importers,
        graph: &graph,
        auto_install_peers: false,
        dedupe_peers: false,
        exclude_links_from_lockfile: false,
        inject_workspace_packages: false,
        peers_suffix_max_length: None,
        overrides: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        catalogs: &catalogs,
        registry: "https://registry.npmjs.org",
        lockfile_include_tarball_url: false,
    });

    let snapshots = lockfile.catalogs.as_ref().expect("catalogs snapshot present");
    let entry = snapshots
        .get("default")
        .and_then(|catalog| catalog.get("js-yaml"))
        .expect("aliased catalog entry recorded");
    assert_eq!(entry.specifier, "npm:@zkochan/js-yaml@0.0.11");
    assert_eq!(entry.version, "0.0.11");
}

/// A `runtime:` dependency (`node@runtime:26.3.0`, a `Variations`
/// resolution whose name lives only in the fetched manifest) records the
/// prefix-stripped importer version (`runtime:26.3.0`, not
/// `node@runtime:26.3.0`) and a `version: 26.3.0` field on its `packages:`
/// entry — matching pnpm's `depPathToRef` and `toLockfileDependency`
/// (`depPath.includes(':')` ⇒ emit the manifest version for non-directory
/// resolutions).
#[test]
fn runtime_dependency_strips_importer_prefix_and_records_package_version() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": { "node": "runtime:26.3.0" },
    }));

    let dep_path = DepPath::from("node@runtime:26.3.0".to_string());
    let resolve_result = ResolveResult {
        id: PkgResolutionId::from("node@runtime:26.3.0"),
        name_ver: None,
        latest: None,
        published_at: None,
        manifest: Some(std::sync::Arc::new(json!({
            "name": "node",
            "version": "26.3.0",
            "bin": { "node": "bin/node" },
        }))),
        resolution: LockfileResolution::Variations(VariationsResolution { variants: vec![] }),
        resolved_via: "node-runtime".to_string(),
        normalized_bare_specifier: None,
        alias: Some("node".to_string()),
        policy_violation: None,
    };
    let node = DependenciesGraphNode {
        dep_path: dep_path.clone(),
        resolved_package_id: "node@runtime:26.3.0".to_string(),
        resolve_result: std::sync::Arc::new(resolve_result),
        children: BTreeMap::new(),
        peer_dependencies: BTreeMap::new(),
        transitive_peer_dependencies: HashSet::new(),
        resolved_peer_names: HashSet::new(),
        depth: 1,
        installable: true,
        is_pure: true,
        optional: false,
    };

    let mut graph = DependenciesGraph::new();
    graph.insert(dep_path.clone(), node);

    let mut direct = BTreeMap::new();
    direct.insert("node".to_string(), dep_path);

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, false, false, None, None,
    ));

    let importer = lockfile.root_project().expect("root importer");
    let entry = importer
        .dependencies
        .as_ref()
        .expect("deps")
        .get(&PkgName::parse("node").unwrap())
        .unwrap();
    assert_eq!(entry.specifier, "runtime:26.3.0");
    match &entry.version {
        ImporterDepVersion::Regular(ver) => assert_eq!(ver.to_string(), "runtime:26.3.0"),
        other => panic!("expected Regular(runtime:26.3.0), got {other:?}"),
    }

    let metadata_key: PackageKey = "node@runtime:26.3.0".parse().unwrap();
    let metadata = &lockfile.packages.as_ref().expect("packages")[&metadata_key];
    assert_eq!(metadata.version.as_deref(), Some("26.3.0"));
}

/// A package with a peer-suffixed depPath produces a peer-keyed snapshot
/// entry, but the matching `packages:` entry uses the peer-stripped
/// pkgId. `peerDependencies` metadata lives on `packages:`, not the
/// snapshot.
#[test]
fn peer_suffixed_dep_path_splits_into_distinct_snapshot_and_package_keys() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": {
            "react": "^17.0.2",
            "react-dom": "^17.0.2",
        },
    }));

    let react = make_node(
        "react",
        "17.0.2",
        json!({ "name": "react", "version": "17.0.2" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::new(),
    );

    let mut react_dom_children = BTreeMap::new();
    react_dom_children.insert("react".to_string(), DepPath::from("react@17.0.2".to_string()));
    let mut react_dom_peers = BTreeMap::new();
    react_dom_peers
        .insert("react".to_string(), PeerDep { version: "17.0.2".to_string(), optional: false });
    let react_dom_dep_path = DepPath::from("react-dom@17.0.2(react@17.0.2)".to_string());
    let react_dom = DependenciesGraphNode {
        dep_path: react_dom_dep_path.clone(),
        resolved_package_id: "react-dom@17.0.2".to_string(),
        resolve_result: std::sync::Arc::new(make_resolve_result(
            "react-dom",
            "17.0.2",
            json!({
                "name": "react-dom",
                "version": "17.0.2",
                "peerDependencies": { "react": "17.0.2" },
            }),
        )),
        children: react_dom_children,
        peer_dependencies: react_dom_peers,
        transitive_peer_dependencies: HashSet::new(),
        resolved_peer_names: std::iter::once("react".to_string()).collect(),
        depth: 1,
        installable: true,
        is_pure: false,
        optional: false,
    };

    let mut graph = DependenciesGraph::new();
    graph.insert(react.dep_path.clone(), react);
    graph.insert(react_dom_dep_path.clone(), react_dom);

    let mut direct = BTreeMap::new();
    direct.insert("react".to_string(), DepPath::from("react@17.0.2".to_string()));
    direct.insert("react-dom".to_string(), react_dom_dep_path);

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, true, false, None, None,
    ));

    let snapshots = lockfile.snapshots.as_ref().expect("snapshots");
    let snap_key: PackageKey = "react-dom@17.0.2(react@17.0.2)".parse().unwrap();
    assert!(snapshots.contains_key(&snap_key), "snapshot keyed by peer-suffixed depPath");
    let pkg_key: PackageKey = "react-dom@17.0.2".parse().unwrap();
    let packages = lockfile.packages.as_ref().expect("packages");
    let metadata = packages.get(&pkg_key).expect("package metadata for peer-stripped key");
    assert!(metadata.peer_dependencies.is_some(), "peer_deps on packages metadata");

    let importer = lockfile.root_project().unwrap();
    let dom =
        importer.dependencies.as_ref().unwrap().get(&PkgName::parse("react-dom").unwrap()).unwrap();
    match &dom.version {
        ImporterDepVersion::Regular(ver) => {
            assert_eq!(ver.to_string(), "17.0.2(react@17.0.2)");
        }
        other => panic!("expected Regular(...), got {other:?}"),
    }
}

/// Snapshot children declared by the resolved manifest's
/// `optionalDependencies` map land in the snapshot's
/// `optionalDependencies` (not `dependencies`).
#[test]
fn snapshot_partitions_optional_children_by_manifest_optional_dependencies() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": { "outer": "^1.0.0" },
    }));

    let inner = make_node(
        "inner",
        "1.0.0",
        json!({ "name": "inner", "version": "1.0.0" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::new(),
    );

    let mut outer_children = BTreeMap::new();
    outer_children.insert("inner".to_string(), DepPath::from("inner@1.0.0".to_string()));
    let outer = make_node(
        "outer",
        "1.0.0",
        json!({
            "name": "outer",
            "version": "1.0.0",
            "optionalDependencies": { "inner": "^1.0.0" },
        }),
        outer_children,
        BTreeMap::new(),
        HashSet::new(),
    );

    let mut graph = DependenciesGraph::new();
    graph.insert(inner.dep_path.clone(), inner);
    graph.insert(outer.dep_path.clone(), outer);

    let mut direct = BTreeMap::new();
    direct.insert("outer".to_string(), DepPath::from("outer@1.0.0".to_string()));

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, false, false, None, None,
    ));

    let snapshots = lockfile.snapshots.as_ref().unwrap();
    let outer_key: PackageKey = "outer@1.0.0".parse().unwrap();
    let outer_snap = &snapshots[&outer_key];
    assert!(outer_snap.dependencies.is_none(), "no regular dep for an optional-only child");
    let opt = outer_snap.optional_dependencies.as_ref().expect("opt deps map");
    let inner_key = PkgName::parse("inner").unwrap();
    match opt.get(&inner_key).expect("inner under optionalDependencies") {
        SnapshotDepRef::Plain(ver) => assert_eq!(ver.to_string(), "1.0.0"),
        other => panic!("expected Plain, got {other:?}"),
    }
}

/// `transitivePeerDependencies` carries every name in the node's
/// transitive set, sorted and deduplicated.
#[test]
fn snapshot_records_transitive_peer_dependencies_sorted() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": { "outer": "^1.0.0" },
    }));

    let mut transitive: HashSet<String> = HashSet::new();
    transitive.insert("z-peer".to_string());
    transitive.insert("a-peer".to_string());
    let outer = make_node(
        "outer",
        "1.0.0",
        json!({ "name": "outer", "version": "1.0.0" }),
        BTreeMap::new(),
        BTreeMap::new(),
        transitive,
    );
    let mut graph = DependenciesGraph::new();
    graph.insert(outer.dep_path.clone(), outer);

    let mut direct = BTreeMap::new();
    direct.insert("outer".to_string(), DepPath::from("outer@1.0.0".to_string()));

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, true, false, None, None,
    ));

    let snapshots = lockfile.snapshots.as_ref().unwrap();
    let outer_key: PackageKey = "outer@1.0.0".parse().unwrap();
    let recorded = snapshots[&outer_key]
        .transitive_peer_dependencies
        .as_ref()
        .expect("transitive peers recorded");
    assert_eq!(recorded.as_slice(), ["a-peer".to_string(), "z-peer".to_string()].as_slice());
}

/// `SnapshotEntry.optional` is copied from the resolver's
/// [`DependenciesGraphNode::optional`] field — `true` for snapshots
/// the walker marked as reachable only via `optionalDependencies`
/// edges, `false` for everything else. Confirms the adapter doesn't
/// silently drop the bit (the regression that motivated the field's
/// addition in the first place).
#[test]
fn snapshot_optional_flag_round_trips_from_dependencies_graph_node() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": { "regular": "^1.0.0" },
        "optionalDependencies": { "opt": "^1.0.0" },
    }));

    let regular = make_node(
        "regular",
        "1.0.0",
        json!({ "name": "regular", "version": "1.0.0" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::new(),
    );
    let opt = make_node_with_optional(
        "opt",
        "1.0.0",
        json!({ "name": "opt", "version": "1.0.0" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::new(),
        true,
    );

    let mut graph = DependenciesGraph::new();
    graph.insert(regular.dep_path.clone(), regular);
    graph.insert(opt.dep_path.clone(), opt);

    let mut direct = BTreeMap::new();
    direct.insert("regular".to_string(), DepPath::from("regular@1.0.0".to_string()));
    direct.insert("opt".to_string(), DepPath::from("opt@1.0.0".to_string()));

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, false, false, None, None,
    ));

    let snapshots = lockfile.snapshots.as_ref().expect("snapshots map");
    let regular_key: PackageKey = "regular@1.0.0".parse().unwrap();
    let opt_key: PackageKey = "opt@1.0.0".parse().unwrap();
    assert!(!snapshots[&regular_key].optional, "non-optional snapshot stays optional: false");
    assert!(
        snapshots[&opt_key].optional,
        "snapshot marked optional in the graph propagates to the lockfile",
    );
}

/// Scenario from
/// [pnpm/pnpm#11916](https://github.com/pnpm/pnpm/issues/11916): root
/// declares `optionalDependencies.a` and `dependencies.b`; `a.dependencies = {c}`
/// and `b.dependencies = {a}`. The resolver's [`DependenciesGraphNode::optional`]
/// field is stale on `c` — the tree walker marks it `optional: true`
/// on the `optional → a → c` descent and then misses the AND-fold on
/// the `prod → b → a` revisit because the lazy-children path doesn't
/// re-traverse `a`'s subtree. The lockfile-pruner BFS re-derives the
/// flag from the importer-rooted graph, so `c` ends up `optional:
/// false` (reachable through the all-non-optional path `prod → b →
/// a → c`).
#[test]
fn transitive_optional_is_recomputed_for_packages_reachable_via_a_non_optional_path() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies":         { "b": "^1.0.0" },
        "optionalDependencies": { "a": "^1.0.0" },
    }));

    // `c` was first reached via the optional path, so the resolver
    // left `node.optional = true`. The pruner should flip it back to
    // false.
    let node_c = make_node_with_optional(
        "c",
        "1.0.0",
        json!({ "name": "c", "version": "1.0.0" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::new(),
        true,
    );

    // `a` was revisited from `b` so the resolver AND-folded its flag
    // to false — that part already works. The bug is purely in the
    // descendants.
    let mut a_children = BTreeMap::new();
    a_children.insert("c".to_string(), DepPath::from("c@1.0.0".to_string()));
    let node_a = make_node_with_optional(
        "a",
        "1.0.0",
        json!({ "name": "a", "version": "1.0.0", "dependencies": { "c": "^1.0.0" } }),
        a_children,
        BTreeMap::new(),
        HashSet::new(),
        false,
    );

    let mut b_children = BTreeMap::new();
    b_children.insert("a".to_string(), DepPath::from("a@1.0.0".to_string()));
    let node_b = make_node_with_optional(
        "b",
        "1.0.0",
        json!({ "name": "b", "version": "1.0.0", "dependencies": { "a": "^1.0.0" } }),
        b_children,
        BTreeMap::new(),
        HashSet::new(),
        false,
    );

    let mut graph = DependenciesGraph::new();
    graph.insert(node_a.dep_path.clone(), node_a);
    graph.insert(node_b.dep_path.clone(), node_b);
    graph.insert(node_c.dep_path.clone(), node_c);

    let mut direct = BTreeMap::new();
    direct.insert("a".to_string(), DepPath::from("a@1.0.0".to_string()));
    direct.insert("b".to_string(), DepPath::from("b@1.0.0".to_string()));

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, false, false, None, None,
    ));

    let snapshots = lockfile.snapshots.as_ref().expect("snapshots map");
    let a_key: PackageKey = "a@1.0.0".parse().unwrap();
    let b_key: PackageKey = "b@1.0.0".parse().unwrap();
    let c_key: PackageKey = "c@1.0.0".parse().unwrap();
    assert!(!snapshots[&b_key].optional, "b is a direct prod dep");
    assert!(!snapshots[&a_key].optional, "a is reachable via prod → b → a");
    assert!(!snapshots[&c_key].optional, "c is reachable via prod → b → a → c");
}

/// Ported from upstream pnpm's
/// [`'subdependency is both optional and dev'`](https://github.com/pnpm/pnpm/blob/b9de85dcb6/lockfile/pruner/test/index.ts#L378-L449)
/// pruner test: when one shared subdep is reached via a dev parent's
/// `optionalDependencies` and a prod parent's `dependencies`, only
/// the strictly-optional subdep ends up `optional: true`.
#[test]
fn shared_subdep_reached_through_dev_optional_and_prod_paths_is_marked_non_optional() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies":    { "prod-parent": "^1.0.0" },
        "devDependencies": { "parent": "^1.0.0" },
    }));

    let subdep = make_node_with_optional(
        "subdep",
        "1.0.0",
        json!({ "name": "subdep", "version": "1.0.0" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::new(),
        true,
    );
    let subdep2 = make_node_with_optional(
        "subdep2",
        "1.0.0",
        json!({ "name": "subdep2", "version": "1.0.0" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::new(),
        true,
    );

    let mut parent_children = BTreeMap::new();
    parent_children.insert("subdep".to_string(), DepPath::from("subdep@1.0.0".to_string()));
    parent_children.insert("subdep2".to_string(), DepPath::from("subdep2@1.0.0".to_string()));
    let parent = make_node_with_optional(
        "parent",
        "1.0.0",
        json!({
            "name": "parent",
            "version": "1.0.0",
            "optionalDependencies": { "subdep": "^1.0.0", "subdep2": "^1.0.0" },
        }),
        parent_children,
        BTreeMap::new(),
        HashSet::new(),
        false,
    );

    let mut prod_children = BTreeMap::new();
    prod_children.insert("subdep2".to_string(), DepPath::from("subdep2@1.0.0".to_string()));
    let prod_parent = make_node_with_optional(
        "prod-parent",
        "1.0.0",
        json!({
            "name": "prod-parent",
            "version": "1.0.0",
            "dependencies": { "subdep2": "^1.0.0" },
        }),
        prod_children,
        BTreeMap::new(),
        HashSet::new(),
        false,
    );

    let mut graph = DependenciesGraph::new();
    graph.insert(parent.dep_path.clone(), parent);
    graph.insert(prod_parent.dep_path.clone(), prod_parent);
    graph.insert(subdep.dep_path.clone(), subdep);
    graph.insert(subdep2.dep_path.clone(), subdep2);

    let mut direct = BTreeMap::new();
    direct.insert("parent".to_string(), DepPath::from("parent@1.0.0".to_string()));
    direct.insert("prod-parent".to_string(), DepPath::from("prod-parent@1.0.0".to_string()));

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, false, false, None, None,
    ));

    let snapshots = lockfile.snapshots.as_ref().unwrap();
    let subdep_key: PackageKey = "subdep@1.0.0".parse().unwrap();
    let subdep2_key: PackageKey = "subdep2@1.0.0".parse().unwrap();
    assert!(snapshots[&subdep_key].optional, "subdep only reachable via dev → optional path");
    assert!(
        !snapshots[&subdep2_key].optional,
        "subdep2 is reachable via prod-parent → subdep2 (all non-optional)",
    );
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
        peer_dependencies: BTreeMap::new(),
        transitive_peer_dependencies: HashSet::new(),
        resolved_peer_names: HashSet::new(),
        depth: 0,
        installable: true,
        is_pure: true,
        optional: false,
    }
}

/// A direct dependency resolved to a workspace sibling lands as
/// `ImporterDepVersion::Link(<rel-path>)` instead of failing to parse
/// the depPath as `name@version`. Mirrors upstream's
/// [`depPathToRef`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/depPathToRef.ts)
/// branch for `link:` resolutions.
#[test]
fn workspace_link_direct_dep_renders_as_importer_link() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "app",
        "version": "1.0.0",
        "dependencies": { "shared": "workspace:*" },
    }));

    let link_node = make_link_node("../shared", json!({ "name": "shared", "version": "1.0.0" }));
    let mut graph = DependenciesGraph::new();
    graph.insert(link_node.dep_path.clone(), link_node.clone());

    let mut direct = BTreeMap::new();
    direct.insert("shared".to_string(), link_node.dep_path);

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, false, false, None, None,
    ));

    let importer = lockfile.root_project().expect("root importer");
    let dep = importer.dependencies.as_ref().expect("dependencies map");
    let entry = dep.get(&PkgName::parse("shared").unwrap()).expect("shared entry");
    assert_eq!(entry.specifier, "workspace:*");
    match &entry.version {
        ImporterDepVersion::Link(target) => assert_eq!(target, "../shared"),
        other => panic!("expected Link(..), got {other:?}"),
    }

    // `link:` nodes don't make it into packages: / snapshots: — the
    // sibling project carries its own importer entry, and the symlink
    // is materialized by `SymlinkDirectDependencies`.
    assert!(lockfile.packages.is_none() || lockfile.packages.as_ref().unwrap().is_empty());
    assert!(lockfile.snapshots.is_none() || lockfile.snapshots.as_ref().unwrap().is_empty());
}

/// A transitive dep that depends on a workspace sibling renders the
/// edge as `SnapshotDepRef::Link(<rel-path>)` instead of dropping it
/// from the snapshot's `dependencies` map.
#[test]
fn workspace_link_child_renders_as_snapshot_link() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "app",
        "version": "1.0.0",
        "dependencies": { "wrapper": "^1.0.0" },
    }));

    let link_node = make_link_node("../shared", json!({ "name": "shared", "version": "1.0.0" }));

    let mut wrapper_children = BTreeMap::new();
    wrapper_children.insert("shared".to_string(), link_node.dep_path.clone());
    let wrapper = make_node(
        "wrapper",
        "1.0.0",
        json!({ "name": "wrapper", "version": "1.0.0" }),
        wrapper_children,
        BTreeMap::new(),
        HashSet::new(),
    );

    let mut graph = DependenciesGraph::new();
    graph.insert(wrapper.dep_path.clone(), wrapper);
    graph.insert(link_node.dep_path.clone(), link_node);

    let mut direct = BTreeMap::new();
    direct.insert("wrapper".to_string(), DepPath::from("wrapper@1.0.0".to_string()));

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, false, false, None, None,
    ));

    let snapshots = lockfile.snapshots.as_ref().expect("snapshots map");
    let wrapper_key: PackageKey = "wrapper@1.0.0".parse().unwrap();
    let wrapper_snap = &snapshots[&wrapper_key];
    let deps = wrapper_snap.dependencies.as_ref().expect("wrapper dependencies");
    match deps.get(&PkgName::parse("shared").unwrap()).expect("shared child") {
        SnapshotDepRef::Link(target) => assert_eq!(target, "../shared"),
        other => panic!("expected Link(..), got {other:?}"),
    }
}

/// Multi-importer: each workspace project contributes its own
/// `importers[<id>]` entry with its own `specifiers` and dep groups,
/// and shared transitive deps are listed once in `packages:` /
/// `snapshots:`.
#[test]
fn multi_importer_workspace_writes_per_project_lockfile_entries() {
    let (_a_tmp, a_manifest) = write_manifest(json!({
        "name": "a",
        "version": "1.0.0",
        "dependencies": { "lodash": "^4.17.21" },
    }));
    let (_b_tmp, b_manifest) = write_manifest(json!({
        "name": "b",
        "version": "1.0.0",
        "dependencies": { "lodash": "^4.17.21" },
    }));

    // Same transitive dep shared by both importers; merging puts one
    // entry in the unified graph.
    let lodash = make_node(
        "lodash",
        "4.17.21",
        json!({ "name": "lodash", "version": "4.17.21" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::new(),
    );
    let mut graph = DependenciesGraph::new();
    graph.insert(lodash.dep_path.clone(), lodash);

    let mut a_direct = BTreeMap::new();
    a_direct.insert("lodash".to_string(), DepPath::from("lodash@4.17.21".to_string()));
    let mut b_direct = BTreeMap::new();
    b_direct.insert("lodash".to_string(), DepPath::from("lodash@4.17.21".to_string()));

    let mut importers = BTreeMap::new();
    importers.insert(
        "packages/a".to_string(),
        ImporterLockfileInput { manifest: &a_manifest, direct_dependencies_by_alias: a_direct },
    );
    importers.insert(
        "packages/b".to_string(),
        ImporterLockfileInput { manifest: &b_manifest, direct_dependencies_by_alias: b_direct },
    );

    let lockfile = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        importers,
        graph: &graph,
        auto_install_peers: false,
        dedupe_peers: false,
        exclude_links_from_lockfile: false,
        inject_workspace_packages: false,
        peers_suffix_max_length: None,
        overrides: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        catalogs: &EMPTY_CATALOGS,
        registry: "https://registry.npmjs.org",
        lockfile_include_tarball_url: false,
    });

    let a_snap = lockfile.importers.get("packages/a").expect("importer a");
    let b_snap = lockfile.importers.get("packages/b").expect("importer b");
    let lodash_name = PkgName::parse("lodash").unwrap();
    assert!(a_snap.dependencies.as_ref().unwrap().contains_key(&lodash_name));
    assert!(b_snap.dependencies.as_ref().unwrap().contains_key(&lodash_name));

    let packages = lockfile.packages.as_ref().expect("packages");
    let lodash_key: PackageKey = "lodash@4.17.21".parse().unwrap();
    assert!(packages.contains_key(&lodash_key), "single shared snapshot");
    assert_eq!(packages.len(), 1, "shared dep deduped to one entry");
}

/// Multi-importer cross-importer pruner BFS. Ported from upstream
/// pnpm's [`pruneSharedLockfile`](https://github.com/pnpm/pnpm/blob/d8a79a9c30/lockfile/pruner/src/index.ts#L17)
/// behavior — `copyPackageSnapshots` pools every importer's
/// `(devDepPaths, optionalDepPaths, prodDepPaths)` via `unnest(...)`
/// before the three `copyDependencySubGraph` walks, so a depPath
/// reachable via a non-optional path from any importer must end up
/// `optional: false` even when another importer reaches it only via
/// an optional path.
///
/// Scenario:
/// - `packages/a` has `prod-only` as a prod dep; `prod-only` →
///   `shared` as a non-optional child.
/// - `packages/b` has `opt-only` as an optional dep; `opt-only` →
///   `shared` as a non-optional child (so the optional flag flows
///   purely from the importer-level edge).
///
/// `shared`'s resolver-side `node.optional` is left as `true`
/// (simulating the resolver having first reached it via the optional
/// chain). The BFS must flip it back to `false` because importer A's
/// path is all non-optional.
#[test]
fn multi_importer_pruner_marks_shared_dep_non_optional_when_any_importer_reaches_via_prod() {
    let (_a_tmp, a_manifest) = write_manifest(json!({
        "name": "a",
        "version": "1.0.0",
        "dependencies": { "prod-only": "^1.0.0" },
    }));
    let (_b_tmp, b_manifest) = write_manifest(json!({
        "name": "b",
        "version": "1.0.0",
        "optionalDependencies": { "opt-only": "^1.0.0" },
    }));

    // Stale-optional flag on `shared` — the resolver tagged it
    // `optional: true` because the optional chain was walked first.
    // The pruner BFS should re-derive it from importer-rooted
    // reachability and flip it to false.
    let shared = make_node_with_optional(
        "shared",
        "1.0.0",
        json!({ "name": "shared", "version": "1.0.0" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::new(),
        true,
    );

    let mut prod_only_children = BTreeMap::new();
    prod_only_children.insert("shared".to_string(), DepPath::from("shared@1.0.0".to_string()));
    let prod_only = make_node_with_optional(
        "prod-only",
        "1.0.0",
        json!({
            "name": "prod-only",
            "version": "1.0.0",
            "dependencies": { "shared": "^1.0.0" },
        }),
        prod_only_children,
        BTreeMap::new(),
        HashSet::new(),
        false,
    );

    let mut opt_only_children = BTreeMap::new();
    opt_only_children.insert("shared".to_string(), DepPath::from("shared@1.0.0".to_string()));
    let opt_only = make_node_with_optional(
        "opt-only",
        "1.0.0",
        json!({
            "name": "opt-only",
            "version": "1.0.0",
            "dependencies": { "shared": "^1.0.0" },
        }),
        opt_only_children,
        BTreeMap::new(),
        HashSet::new(),
        true,
    );

    let mut graph = DependenciesGraph::new();
    graph.insert(shared.dep_path.clone(), shared);
    graph.insert(prod_only.dep_path.clone(), prod_only);
    graph.insert(opt_only.dep_path.clone(), opt_only);

    let mut a_direct = BTreeMap::new();
    a_direct.insert("prod-only".to_string(), DepPath::from("prod-only@1.0.0".to_string()));
    let mut b_direct = BTreeMap::new();
    b_direct.insert("opt-only".to_string(), DepPath::from("opt-only@1.0.0".to_string()));

    let mut importers = BTreeMap::new();
    importers.insert(
        "packages/a".to_string(),
        ImporterLockfileInput { manifest: &a_manifest, direct_dependencies_by_alias: a_direct },
    );
    importers.insert(
        "packages/b".to_string(),
        ImporterLockfileInput { manifest: &b_manifest, direct_dependencies_by_alias: b_direct },
    );

    let lockfile = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        importers,
        graph: &graph,
        auto_install_peers: false,
        dedupe_peers: false,
        exclude_links_from_lockfile: false,
        inject_workspace_packages: false,
        peers_suffix_max_length: None,
        overrides: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        catalogs: &EMPTY_CATALOGS,
        registry: "https://registry.npmjs.org",
        lockfile_include_tarball_url: false,
    });

    let snapshots = lockfile.snapshots.as_ref().expect("snapshots map");
    let prod_only_key: PackageKey = "prod-only@1.0.0".parse().unwrap();
    let opt_only_key: PackageKey = "opt-only@1.0.0".parse().unwrap();
    let shared_key: PackageKey = "shared@1.0.0".parse().unwrap();
    assert!(!snapshots[&prod_only_key].optional, "prod-only is a direct prod dep of packages/a");
    assert!(
        snapshots[&opt_only_key].optional,
        "opt-only is only reachable via packages/b's optional",
    );
    assert!(
        !snapshots[&shared_key].optional,
        "shared is reachable via packages/a → prod-only → shared (all non-optional)",
    );
}

/// Auto-installed peers (hoisted into `direct_dependencies_by_alias`
/// by the resolver when `autoInstallPeers: true` is on) must NOT seed
/// the pruner BFS — they aren't in the manifest, so
/// [`build_importer`](super::build_importer) excludes them from the
/// importer's lockfile entry, and upstream's
/// [`pruneSharedLockfile`](https://github.com/pnpm/pnpm/blob/d8a79a9c30/lockfile/pruner/src/index.ts#L27-L29)
/// seeds only from what was written to the importer entries.
///
/// Scenario: importer declares `parent` in `optionalDependencies`;
/// the resolver auto-installs `parent`'s peer `peer-x` and hoists it
/// to the importer's `direct_dependencies_by_alias`. With the
/// undeclared-alias skip, the BFS only seeds `parent` (optional), so
/// `peer-x` ends up `optional: true` — matching pnpm. Without the
/// skip, `peer-x` would have been treated as a Prod-defaulted
/// importer-level dep and forced to `optional: false`.
#[test]
fn auto_installed_peer_not_declared_in_manifest_is_skipped_from_pruner_seeds() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "optionalDependencies": { "parent": "^1.0.0" },
    }));

    let peer_x = make_node_with_optional(
        "peer-x",
        "1.0.0",
        json!({ "name": "peer-x", "version": "1.0.0" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::new(),
        true,
    );

    let mut parent_children = BTreeMap::new();
    parent_children.insert("peer-x".to_string(), DepPath::from("peer-x@1.0.0".to_string()));
    let parent = make_node_with_optional(
        "parent",
        "1.0.0",
        json!({
            "name": "parent",
            "version": "1.0.0",
            "dependencies": { "peer-x": "^1.0.0" },
        }),
        parent_children,
        BTreeMap::new(),
        HashSet::new(),
        true,
    );

    let mut graph = DependenciesGraph::new();
    graph.insert(parent.dep_path.clone(), parent);
    graph.insert(peer_x.dep_path.clone(), peer_x);

    // The resolver hoisted `peer-x` to the importer level even though
    // the manifest doesn't declare it — this is exactly what
    // `auto_install_peers` does.
    let mut direct = BTreeMap::new();
    direct.insert("parent".to_string(), DepPath::from("parent@1.0.0".to_string()));
    direct.insert("peer-x".to_string(), DepPath::from("peer-x@1.0.0".to_string()));

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, true, false, None, None,
    ));

    let snapshots = lockfile.snapshots.as_ref().expect("snapshots map");
    let parent_key: PackageKey = "parent@1.0.0".parse().unwrap();
    let peer_x_key: PackageKey = "peer-x@1.0.0".parse().unwrap();
    assert!(snapshots[&parent_key].optional, "parent is the importer's optional direct dep");
    assert!(
        snapshots[&peer_x_key].optional,
        "auto-installed peer reachable only via parent's optional path stays optional",
    );
}

/// Multi-importer with a `workspace:` link between two siblings.
/// Ported from the spirit of upstream pnpm's
/// [`headless install is used when package linked to another package in the workspace`](https://github.com/pnpm/pnpm/blob/d8a79a9c30/installing/deps-installer/test/install/multipleImporters.ts#L540)
/// scenario, narrowed to the lockfile-rendering side: importer `a`
/// depends on importer `b` via a `link:` resolved depPath, so
/// `importers["packages/a"].dependencies.b` renders as
/// `ImporterDepVersion::Link("../b")` and `importers["packages/b"]`
/// gets its own entry — no cross-pollution into `packages:` /
/// `snapshots:`.
#[test]
fn workspace_sibling_link_renders_per_importer_with_link_ref() {
    let (_a_tmp, a_manifest) = write_manifest(json!({
        "name": "@scope/a",
        "version": "1.0.0",
        "dependencies": { "b": "workspace:*" },
    }));
    let (_b_tmp, b_manifest) = write_manifest(json!({
        "name": "@scope/b",
        "version": "1.0.0",
        "dependencies": { "lodash": "^4.17.21" },
    }));

    let link_node = make_link_node("../b", json!({ "name": "@scope/b", "version": "1.0.0" }));
    let lodash = make_node(
        "lodash",
        "4.17.21",
        json!({ "name": "lodash", "version": "4.17.21" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::new(),
    );

    let mut graph = DependenciesGraph::new();
    graph.insert(link_node.dep_path.clone(), link_node.clone());
    graph.insert(lodash.dep_path.clone(), lodash);

    let mut a_direct = BTreeMap::new();
    a_direct.insert("b".to_string(), link_node.dep_path);
    let mut b_direct = BTreeMap::new();
    b_direct.insert("lodash".to_string(), DepPath::from("lodash@4.17.21".to_string()));

    let mut importers = BTreeMap::new();
    importers.insert(
        "packages/a".to_string(),
        ImporterLockfileInput { manifest: &a_manifest, direct_dependencies_by_alias: a_direct },
    );
    importers.insert(
        "packages/b".to_string(),
        ImporterLockfileInput { manifest: &b_manifest, direct_dependencies_by_alias: b_direct },
    );

    let lockfile = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        importers,
        graph: &graph,
        auto_install_peers: false,
        dedupe_peers: false,
        exclude_links_from_lockfile: false,
        inject_workspace_packages: false,
        peers_suffix_max_length: None,
        overrides: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        catalogs: &EMPTY_CATALOGS,
        registry: "https://registry.npmjs.org",
        lockfile_include_tarball_url: false,
    });

    // Importer a points at b via a link: ref carrying the relative
    // path the resolver produced.
    let a_snap = lockfile.importers.get("packages/a").expect("importer a");
    let b_in_a =
        a_snap.dependencies.as_ref().unwrap().get(&PkgName::parse("b").unwrap()).expect("b in a");
    assert_eq!(b_in_a.specifier, "workspace:*");
    match &b_in_a.version {
        ImporterDepVersion::Link(target) => assert_eq!(target, "../b"),
        other => panic!("expected Link(..), got {other:?}"),
    }

    // Importer b has its own lockfile entry, independent of importer a.
    let b_snap = lockfile.importers.get("packages/b").expect("importer b");
    assert!(
        b_snap.dependencies.as_ref().unwrap().contains_key(&PkgName::parse("lodash").unwrap()),
        "importer b carries its own deps",
    );

    // The link: node never lands in packages: / snapshots: — it's a
    // sibling project, not a resolved registry package.
    let packages = lockfile.packages.as_ref().expect("packages");
    let lodash_key: PackageKey = "lodash@4.17.21".parse().unwrap();
    assert!(packages.contains_key(&lodash_key));
    assert_eq!(packages.len(), 1, "only lodash lands in packages:");
}

/// Ported from upstream pnpm's
/// [`links are not added to the lockfile when excludeLinksFromLockfile is true`](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-installer/test/install/excludeLinksFromLockfile.ts#L27-L124)
/// e2e test, narrowed to the lockfile-rendering side: a direct
/// dependency whose manifest specifier is a bare `link:` path is
/// omitted from the importer's `dependencies` and `specifiers` maps
/// when [`GraphToLockfileOptions::exclude_links_from_lockfile`] is
/// `true`, while a registry-resolved sibling is still recorded.
#[test]
fn external_link_direct_dep_omitted_from_importer_when_exclude_links_from_lockfile_true() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": {
            "is-positive": "1.0.0",
            "external-1": "link:/abs/external-1",
        },
    }));

    let link_node =
        make_link_node("/abs/external-1", json!({ "name": "external-1", "version": "1.0.0" }));
    let is_positive = make_node(
        "is-positive",
        "1.0.0",
        json!({ "name": "is-positive", "version": "1.0.0" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::new(),
    );

    let mut graph = DependenciesGraph::new();
    graph.insert(link_node.dep_path.clone(), link_node.clone());
    graph.insert(is_positive.dep_path.clone(), is_positive);

    let mut direct = BTreeMap::new();
    direct.insert("external-1".to_string(), link_node.dep_path);
    direct.insert("is-positive".to_string(), DepPath::from("is-positive@1.0.0".to_string()));

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, false, true, None, None,
    ));

    let importer = lockfile.root_project().expect("root importer");
    let deps = importer.dependencies.as_ref().expect("dependencies map");
    assert!(
        deps.contains_key(&PkgName::parse("is-positive").unwrap()),
        "non-link direct dep is still recorded",
    );
    assert!(
        !deps.contains_key(&PkgName::parse("external-1").unwrap()),
        "link: direct dep is omitted from importer.dependencies",
    );
    let specifiers = importer.specifiers.as_ref().expect("specifiers map");
    assert!(
        !specifiers.contains_key("external-1"),
        "link: direct dep is omitted from importer.specifiers",
    );
    assert!(
        lockfile.settings.as_ref().expect("settings block").exclude_links_from_lockfile,
        "the setting round-trips into the lockfile settings block",
    );
}

/// Ported from upstream pnpm's
/// [`links resolved from workspace protocol dependencies are not removed`](https://github.com/pnpm/pnpm/blob/094aa6e57b/installing/deps-installer/test/install/excludeLinksFromLockfile.ts#L245-L298)
/// e2e test. A `workspace:` specifier resolves to a `link:` depPath
/// just like a bare `link:` spec, but `excludeLinksFromLockfile` is
/// scoped to the latter — workspace siblings stay in the importer
/// entry so the lockfile keeps a complete description of the
/// workspace graph.
#[test]
fn workspace_link_direct_dep_kept_when_exclude_links_from_lockfile_true() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "app",
        "version": "1.0.0",
        "dependencies": { "shared": "workspace:*" },
    }));

    let link_node = make_link_node("../shared", json!({ "name": "shared", "version": "1.0.0" }));
    let mut graph = DependenciesGraph::new();
    graph.insert(link_node.dep_path.clone(), link_node.clone());

    let mut direct = BTreeMap::new();
    direct.insert("shared".to_string(), link_node.dep_path);

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, false, true, None, None,
    ));

    let importer = lockfile.root_project().expect("root importer");
    let deps = importer.dependencies.as_ref().expect("dependencies map");
    let shared = deps.get(&PkgName::parse("shared").unwrap()).expect("shared entry");
    assert_eq!(shared.specifier, "workspace:*");
    match &shared.version {
        ImporterDepVersion::Link(target) => assert_eq!(target, "../shared"),
        other => panic!("expected Link(..), got {other:?}"),
    }
}
