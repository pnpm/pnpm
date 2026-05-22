use std::collections::{BTreeMap, HashSet};
use std::str::FromStr;

use pacquet_deps_path::DepPath;
use pacquet_lockfile::{
    ImporterDepVersion, LockfileResolution, PackageKey, PkgName, PkgNameVer, RegistryResolution,
    SnapshotDepRef,
};
use pacquet_package_manifest::PackageManifest;
use pacquet_resolving_deps_resolver::{
    DependenciesGraph, DependenciesGraphNode, PeerDep, PeerDependencyIssues, ResolveImporterResult,
    ResolvePeersResult, ResolvedTree,
};
use pacquet_resolving_resolver_base::ResolveResult;
use serde_json::json;
use ssri::Integrity;
use tempfile::TempDir;

use super::{GraphToLockfileOptions, dependencies_graph_to_lockfile};

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

    let resolved = ResolveImporterResult {
        resolved_tree: ResolvedTree::default(),
        peers_result: ResolvePeersResult {
            graph,
            direct_dependencies_by_alias: direct,
            peer_dependency_issues: PeerDependencyIssues::default(),
        },
    };

    let lockfile = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        manifest: &manifest,
        resolved: &resolved,
        auto_install_peers: true,
        exclude_links_from_lockfile: false,
        overrides: None,
        ignored_optional_dependencies: None,
    });

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

    let resolved = ResolveImporterResult {
        resolved_tree: ResolvedTree::default(),
        peers_result: ResolvePeersResult {
            graph,
            direct_dependencies_by_alias: direct,
            peer_dependency_issues: PeerDependencyIssues::default(),
        },
    };

    let lockfile = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        manifest: &manifest,
        resolved: &resolved,
        auto_install_peers: false,
        exclude_links_from_lockfile: false,
        overrides: None,
        ignored_optional_dependencies: None,
    });

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
        resolved_peer_names: ["react".to_string()].into_iter().collect(),
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

    let resolved = ResolveImporterResult {
        resolved_tree: ResolvedTree::default(),
        peers_result: ResolvePeersResult {
            graph,
            direct_dependencies_by_alias: direct,
            peer_dependency_issues: PeerDependencyIssues::default(),
        },
    };

    let lockfile = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        manifest: &manifest,
        resolved: &resolved,
        auto_install_peers: true,
        exclude_links_from_lockfile: false,
        overrides: None,
        ignored_optional_dependencies: None,
    });

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

    let resolved = ResolveImporterResult {
        resolved_tree: ResolvedTree::default(),
        peers_result: ResolvePeersResult {
            graph,
            direct_dependencies_by_alias: direct,
            peer_dependency_issues: PeerDependencyIssues::default(),
        },
    };

    let lockfile = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        manifest: &manifest,
        resolved: &resolved,
        auto_install_peers: false,
        exclude_links_from_lockfile: false,
        overrides: None,
        ignored_optional_dependencies: None,
    });

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

    let resolved = ResolveImporterResult {
        resolved_tree: ResolvedTree::default(),
        peers_result: ResolvePeersResult {
            graph,
            direct_dependencies_by_alias: direct,
            peer_dependency_issues: PeerDependencyIssues::default(),
        },
    };

    let lockfile = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        manifest: &manifest,
        resolved: &resolved,
        auto_install_peers: true,
        exclude_links_from_lockfile: false,
        overrides: None,
        ignored_optional_dependencies: None,
    });

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

    let resolved = ResolveImporterResult {
        resolved_tree: ResolvedTree::default(),
        peers_result: ResolvePeersResult {
            graph,
            direct_dependencies_by_alias: direct,
            peer_dependency_issues: PeerDependencyIssues::default(),
        },
    };

    let lockfile = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        manifest: &manifest,
        resolved: &resolved,
        auto_install_peers: false,
        exclude_links_from_lockfile: false,
        overrides: None,
        ignored_optional_dependencies: None,
    });

    let snapshots = lockfile.snapshots.as_ref().expect("snapshots map");
    let regular_key: PackageKey = "regular@1.0.0".parse().unwrap();
    let opt_key: PackageKey = "opt@1.0.0".parse().unwrap();
    assert!(!snapshots[&regular_key].optional, "non-optional snapshot stays optional: false");
    assert!(
        snapshots[&opt_key].optional,
        "snapshot marked optional in the graph propagates to the lockfile",
    );
}
