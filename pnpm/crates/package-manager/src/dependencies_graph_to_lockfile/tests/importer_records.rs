use super::{
    super::{DependenciesGraphToLockfileError, GraphToLockfileOptions, ImporterLockfileInput},
    EMPTY_CATALOGS, EMPTY_NAMED_REGISTRIES, EMPTY_REGISTRY_OPTIONS, GIT_TARBALL_URL,
    dependencies_graph_to_lockfile, error_from_single_node_graph, git_hosted_node,
    injected_link_fixture, make_link_node, make_node, make_node_with_optional,
    previous_importers_with_link, single_importer_opts, write_manifest,
};
use pnpm_deps_path::DepPath;
use pnpm_lockfile::{
    GitResolution, ImporterDepVersion, LockfileResolution, PackageKey, PkgName, SnapshotDepRef,
    VariationsResolution,
};
use pnpm_resolving_deps_resolver::{
    DependenciesGraph, DependenciesGraphNode, PeerDep, UpdateReuseScope,
};
use pnpm_resolving_resolver_base::{PkgResolutionId, ResolveResult};
use rustc_hash::FxHashSet as HashSet;
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};

#[test]
fn fresh_install_records_importer_manifest_metadata() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependenciesMeta": { "pkg-a": { "injected": true } },
        "publishConfig": { "directory": "dist", "linkDirectory": false },
    }));
    let graph = DependenciesGraph::default();

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest,
        &graph,
        BTreeMap::new(),
        false,
        false,
        None,
        None,
    ));
    let importer = lockfile.root_project().expect("root importer exists");

    assert_eq!(importer.dependencies_meta, Some(json!({ "pkg-a": { "injected": true } })));
    assert_eq!(importer.publish_directory.as_deref(), Some("dist"));
    assert_eq!(importer.link_directory, Some(false));
}
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
        HashSet::default(),
    );
    let fsevents = make_node(
        "fsevents",
        "2.3.2",
        json!({ "name": "fsevents", "version": "2.3.2", "os": ["darwin"] }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::default(),
    );

    let mut graph = DependenciesGraph::default();
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

    let packages = lockfile.packages.as_ref().unwrap();
    let typescript_key: PackageKey = "typescript@5.1.6".parse().unwrap();
    assert_eq!(packages[&typescript_key].has_bin, Some(true));
    let fsevents_key: PackageKey = "fsevents@2.3.2".parse().unwrap();
    assert_eq!(packages[&fsevents_key].os.as_deref(), Some(["darwin".to_string()].as_slice()));
}
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
        optional_children: HashSet::default(),
        peer_dependencies: BTreeMap::new(),
        transitive_peer_dependencies: HashSet::default(),
        resolved_peer_names: HashSet::default(),
        depth: 1,
        installable: true,
        is_pure: true,
        optional: false,
    };

    let mut graph = DependenciesGraph::default();
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
#[test]
fn git_hosted_dependency_records_bare_tarball_url_in_importer() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": {
            "is-negative": "github:kevva/is-negative#1.0.0",
        },
    }));

    let (dep_path, node) = git_hosted_node("is-negative");
    let mut graph = DependenciesGraph::default();
    graph.insert(dep_path.clone(), node);
    let direct = BTreeMap::from([("is-negative".to_string(), dep_path)]);

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, false, false, None, None,
    ));

    let importer = lockfile.root_project().expect("root importer");
    let entry = importer
        .dependencies
        .as_ref()
        .expect("dependencies")
        .get(&PkgName::parse("is-negative").unwrap())
        .expect("git dependency");
    assert_eq!(entry.specifier, "github:kevva/is-negative#1.0.0");
    // The alias matches the package name, so the `is-negative@` prefix
    // is stripped — same shape upstream's `depPathToRef` writes.
    match &entry.version {
        ImporterDepVersion::Regular(version) => assert_eq!(version.to_string(), GIT_TARBALL_URL),
        other => panic!("expected Regular({GIT_TARBALL_URL}), got {other:?}"),
    }

    let package_key: PackageKey = format!("is-negative@{GIT_TARBALL_URL}").parse().unwrap();
    let packages = lockfile.packages.as_ref().expect("packages");
    assert_eq!(packages[&package_key].version.as_deref(), Some("1.0.0"));
    assert!(lockfile.snapshots.as_ref().expect("snapshots").contains_key(&package_key));
}
/// A renamed git dep keeps the `<name>@<ref>` alias form, so the
/// importer entry still composes to the snapshot key that
/// `packages:` / `snapshots:` are keyed by.
#[test]
fn aliased_git_hosted_dependency_keeps_package_name_in_importer_ref() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": {
            "renamed": "github:kevva/is-negative#1.0.0",
        },
    }));

    let (dep_path, node) = git_hosted_node("renamed");
    let mut graph = DependenciesGraph::default();
    graph.insert(dep_path.clone(), node);
    let direct = BTreeMap::from([("renamed".to_string(), dep_path)]);

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, false, false, None, None,
    ));

    let importer = lockfile.root_project().expect("root importer");
    let entry = importer
        .dependencies
        .as_ref()
        .expect("dependencies")
        .get(&PkgName::parse("renamed").unwrap())
        .expect("git dependency");
    let version = dbg!(&entry.version);
    let ImporterDepVersion::Alias(parsed) = version else {
        panic!("expected Alias(is-negative@{GIT_TARBALL_URL}), got {version:?}");
    };
    assert_eq!(parsed.to_string(), format!("is-negative@{GIT_TARBALL_URL}"));

    let package_key: PackageKey = format!("is-negative@{GIT_TARBALL_URL}").parse().unwrap();
    assert!(lockfile.packages.as_ref().expect("packages").contains_key(&package_key));
    assert!(lockfile.snapshots.as_ref().expect("snapshots").contains_key(&package_key));
}
/// A non-host git dep (ssh / self-hosted / `git+file:`) resolves to a
/// `type: git` snapshot whose id *is* its depPath, and whose name lives
/// only in the fetched manifest. When the manifest alias matches that
/// name, the importer entry drops the `<name>@` prefix and records the
/// bare `git+<repo>#<commit>` ref — the shape pnpm v11 writes, verified
/// byte-for-byte against pnpm 11.13.1.
#[test]
fn non_host_git_dependency_records_bare_git_url_in_importer() {
    const GIT_REF: &str =
        "git+ssh://git@example.com/org/is-negative.git#0123456789012345678901234567890123456789";
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": { "is-negative": GIT_REF },
    }));

    // The install pipeline keys a git dep's depPath by `<name>@<ref>`
    // once the name is read from the fetched manifest, so the `<name>@`
    // prefix is present here — the exact shape `real_name` has to strip
    // back off for the importer entry.
    let dep_path = DepPath::from(format!("is-negative@{GIT_REF}"));
    let resolve_result = ResolveResult {
        id: PkgResolutionId::from(GIT_REF),
        name_ver: None,
        latest: None,
        published_at: None,
        manifest: Some(Arc::new(json!({ "name": "is-negative", "version": "1.0.0" }))),
        resolution: LockfileResolution::Git(GitResolution {
            repo: "ssh://git@example.com/org/is-negative.git".to_string(),
            commit: "0123456789012345678901234567890123456789".to_string(),
            integrity: None,
            path: None,
        }),
        resolved_via: "git-repository".to_string(),
        normalized_bare_specifier: Some(GIT_REF.to_string()),
        alias: Some("is-negative".to_string()),
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
    let mut graph = DependenciesGraph::default();
    graph.insert(dep_path.clone(), node);
    let direct = BTreeMap::from([("is-negative".to_string(), dep_path)]);

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, false, false, None, None,
    ));

    let importer = lockfile.root_project().expect("root importer");
    let entry = importer
        .dependencies
        .as_ref()
        .expect("dependencies")
        .get(&PkgName::parse("is-negative").unwrap())
        .expect("git dependency");
    match &entry.version {
        // The `is-negative@` prefix is stripped: the bare git ref, not
        // `is-negative@git+...`.
        ImporterDepVersion::Regular(version) => assert_eq!(version.to_string(), GIT_REF),
        other => panic!("expected Regular({GIT_REF}), got {other:?}"),
    }

    let packages = lockfile.packages.as_ref().expect("packages");
    let (package_key, metadata) =
        packages.iter().find(|(key, _)| key.to_string().contains("is-negative")).expect("package");
    assert!(matches!(metadata.resolution, LockfileResolution::Git(_)));
    assert_eq!(metadata.version.as_deref(), Some("1.0.0"));
    assert!(
        lockfile.snapshots.as_ref().expect("snapshots").contains_key(package_key),
        "the snapshot is keyed by the same depPath",
    );
}
#[test]
fn malformed_importer_dependency_path_returns_structured_error() {
    let error = error_from_single_node_graph("broken", "1.0.0(react@17.0.0");

    let DependenciesGraphToLockfileError::ImporterDependency { alias, dep_path, .. } = error else {
        panic!("expected an importer-dependency error, got {error}");
    };
    assert_eq!(alias, "broken");
    assert_eq!(dep_path, "1.0.0(react@17.0.0");
}
#[test]
fn workspace_link_direct_dep_renders_as_importer_link() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "app",
        "version": "1.0.0",
        "dependencies": { "shared": "workspace:*" },
    }));

    let link_node = make_link_node("../shared", json!({ "name": "shared", "version": "1.0.0" }));
    let mut graph = DependenciesGraph::default();
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

    assert!(lockfile.packages.is_none() || lockfile.packages.as_ref().unwrap().is_empty());
    assert!(lockfile.snapshots.is_none() || lockfile.snapshots.as_ref().unwrap().is_empty());
}
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
        HashSet::default(),
    );

    let mut graph = DependenciesGraph::default();
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

    let shared = make_node_with_optional(
        "shared",
        "1.0.0",
        json!({ "name": "shared", "version": "1.0.0" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::default(),
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
        HashSet::default(),
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
        HashSet::default(),
        true,
    );

    let mut graph = DependenciesGraph::default();
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
        registries_by_prefix: &EMPTY_NAMED_REGISTRIES,
        registry_options_by_url: &EMPTY_REGISTRY_OPTIONS,
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
        previous_importers: None,
        previous_packages: None,
        update_reuse_scope: UpdateReuseScope::All,
        update_reuse_scopes_by_importer: BTreeMap::new(),
        time: BTreeMap::new(),
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
        HashSet::default(),
    );

    let mut graph = DependenciesGraph::default();
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
        registries_by_prefix: &EMPTY_NAMED_REGISTRIES,
        registry_options_by_url: &EMPTY_REGISTRY_OPTIONS,
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
        previous_importers: None,
        previous_packages: None,
        update_reuse_scope: UpdateReuseScope::All,
        update_reuse_scopes_by_importer: BTreeMap::new(),
        time: BTreeMap::new(),
    });

    let a_snap = lockfile.importers.get("packages/a").expect("importer a");
    let b_in_a =
        a_snap.dependencies.as_ref().unwrap().get(&PkgName::parse("b").unwrap()).expect("b in a");
    assert_eq!(b_in_a.specifier, "workspace:*");
    match &b_in_a.version {
        ImporterDepVersion::Link(target) => assert_eq!(target, "../b"),
        other => panic!("expected Link(..), got {other:?}"),
    }

    let b_snap = lockfile.importers.get("packages/b").expect("importer b");
    assert!(
        b_snap.dependencies.as_ref().unwrap().contains_key(&PkgName::parse("lodash").unwrap()),
        "importer b carries its own deps",
    );

    let packages = lockfile.packages.as_ref().expect("packages");
    let lodash_key: PackageKey = "lodash@4.17.21".parse().unwrap();
    assert!(packages.contains_key(&lodash_key));
    assert_eq!(packages.len(), 1, "only lodash lands in packages:");
}
/// Regression for <https://github.com/pnpm/pnpm/issues/13325>: a peer
/// the hoist installed for the importer is a direct dependency of the
/// resolved tree either way, but it only belongs in the importer's
/// lockfile entry when `autoInstallPeers` materializes the manifest's
/// `peerDependencies` into its dependencies.
#[test]
fn importer_records_a_peer_only_alias_only_under_auto_install_peers() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": { "consumer": "1.0.0" },
        "peerDependencies": { "peer": "^1.0.0" },
        "peerDependenciesMeta": { "peer": { "optional": true } },
    }));
    let peer = make_node(
        "peer",
        "1.0.0",
        json!({ "name": "peer", "version": "1.0.0" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::default(),
    );
    let consumer = make_node(
        "consumer",
        "1.0.0",
        json!({
            "name": "consumer",
            "version": "1.0.0",
            "peerDependencies": { "peer": "^1.0.0" },
            "peerDependenciesMeta": { "peer": { "optional": true } },
        }),
        BTreeMap::from([("peer".to_string(), peer.dep_path.clone())]),
        BTreeMap::from([(
            "peer".to_string(),
            PeerDep { version: "^1.0.0".to_string(), optional: true },
        )]),
        HashSet::default(),
    );
    let mut graph = DependenciesGraph::default();
    for node in [peer, consumer] {
        graph.insert(node.dep_path.clone(), node);
    }
    let direct = BTreeMap::from([
        ("consumer".to_string(), DepPath::from("consumer@1.0.0".to_string())),
        ("peer".to_string(), DepPath::from("peer@1.0.0".to_string())),
    ]);

    let peer_key = PkgName::parse("peer").unwrap();
    let without_auto_install = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest,
        &graph,
        direct.clone(),
        false,
        false,
        None,
        None,
    ));
    let importer = without_auto_install.root_project().expect("root importer exists");
    dbg!(&importer.dependencies);
    assert!(
        !importer.dependencies.as_ref().is_some_and(|deps| deps.contains_key(&peer_key)),
        "a peer-only alias must stay out of the importer entry under `autoInstallPeers: false`",
    );
    assert!(
        !importer.specifiers.as_ref().is_some_and(|specs| specs.contains_key("peer")),
        "a peer-only alias must stay out of the importer specifiers under `autoInstallPeers: false`",
    );

    let with_auto_install = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, true, false, None, None,
    ));
    let importer = with_auto_install.root_project().expect("root importer exists");
    let entry = importer
        .dependencies
        .as_ref()
        .and_then(|deps| deps.get(&peer_key))
        .expect("auto-installed peer entry");
    assert_eq!(entry.specifier, "^1.0.0");
}
// pnpm/pnpm#10433: a plain install (UpdateReuseScope::All) that does not
// target a workspace dependency must keep its prior `link:` importer
// entry, even though the fresh resolution landed on a divergent `file:`.
#[test]
fn injected_workspace_dep_keeps_prior_link_on_untargeted_install() {
    let (_tmp, manifest, graph, direct) = injected_link_fixture();
    let previous = previous_importers_with_link("n", "workspace:*", "../n");

    let lockfile = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        registries_by_prefix: &EMPTY_NAMED_REGISTRIES,
        registry_options_by_url: &EMPTY_REGISTRY_OPTIONS,
        previous_importers: Some(&previous),
        update_reuse_scope: UpdateReuseScope::All,
        ..single_importer_opts(&manifest, &graph, direct, false, false, None, None)
    });

    let importer = lockfile.root_project().expect("root importer");
    let entry = importer
        .dependencies
        .as_ref()
        .and_then(|deps| deps.get(&PkgName::parse("n").unwrap()))
        .expect("n entry");
    match &entry.version {
        ImporterDepVersion::Link(target) => assert_eq!(target, "../n"),
        other => panic!("expected the prior Link(..) to be preserved, got {other:?}"),
    }
}
// Without a previous `link:` to preserve (a first install), the divergent
// `file:` resolution stands — the guard only preserves, never invents.
#[test]
fn injected_workspace_dep_renders_file_without_prior_link() {
    let (_tmp, manifest, graph, direct) = injected_link_fixture();

    let lockfile = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        registries_by_prefix: &EMPTY_NAMED_REGISTRIES,
        registry_options_by_url: &EMPTY_REGISTRY_OPTIONS,
        previous_importers: None,
        previous_packages: None,
        update_reuse_scope: UpdateReuseScope::All,
        ..single_importer_opts(&manifest, &graph, direct, false, false, None, None)
    });

    let importer = lockfile.root_project().expect("root importer");
    let entry = importer
        .dependencies
        .as_ref()
        .and_then(|deps| deps.get(&PkgName::parse("n").unwrap()))
        .expect("n entry");
    assert!(
        matches!(&entry.version, ImporterDepVersion::File(_)),
        "expected File(..) with no prior link to preserve, got {:?}",
        entry.version,
    );
}
