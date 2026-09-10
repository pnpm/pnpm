use super::{
    super::{GraphToLockfileOptions, ImporterLockfileInput},
    EMPTY_CATALOGS, EMPTY_NAMED_REGISTRIES, EMPTY_REGISTRY_OPTIONS, dependencies_graph_to_lockfile,
    make_link_node, make_node, single_importer_opts, write_manifest,
};
use indexmap::IndexMap;
use pnpm_deps_path::DepPath;
use pnpm_lockfile::{ImporterDepVersion, PackageKey, PkgName, SnapshotDepRef};
use pnpm_resolving_deps_resolver::{DependenciesGraph, PeerDep, UpdateReuseScope};
use rustc_hash::FxHashSet as HashSet;
use serde_json::json;
use std::collections::BTreeMap;
use text_block_macros::text_block;

#[test]
fn recorded_publish_dates_reach_the_lockfiles_time_section() {
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
        HashSet::default(),
    );
    let mut graph = DependenciesGraph::default();
    graph.insert(node.dep_path.clone(), node);
    let direct = BTreeMap::from([("react".to_string(), DepPath::from("react@17.0.2".to_string()))]);

    let time =
        BTreeMap::from([("react@17.0.2".to_string(), "2021-03-22T14:00:00.000Z".to_string())]);
    let mut opts = single_importer_opts(&manifest, &graph, direct, true, false, None, None);
    opts.time = time.clone();

    assert_eq!(dependencies_graph_to_lockfile(opts).time, Some(time));
}
#[test]
fn empty_deprecation_message_is_not_written_to_the_lockfile() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": { "legacy": "1.0.0" },
    }));
    let node = make_node(
        "legacy",
        "1.0.0",
        json!({ "name": "legacy", "version": "1.0.0", "deprecated": "" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::default(),
    );
    let mut graph = DependenciesGraph::default();
    graph.insert(node.dep_path.clone(), node);
    let mut direct = BTreeMap::new();
    direct.insert("legacy".to_string(), DepPath::from("legacy@1.0.0".to_string()));

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, true, false, None, None,
    ));
    let package_key: PackageKey = "legacy@1.0.0".parse().expect("package key");
    assert_eq!(lockfile.packages.as_ref().expect("packages")[&package_key].deprecated, None);
}
#[test]
fn generated_lockfile_preserves_libc_manifest_shape() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": {
            "list-libc": "1.0.0",
            "scalar-libc": "1.0.0",
        },
    }));
    let mut graph = DependenciesGraph::default();
    for (name, libc) in [("list-libc", json!(["glibc"])), ("scalar-libc", json!("musl"))] {
        let node = make_node(
            name,
            "1.0.0",
            json!({ "name": name, "version": "1.0.0", "libc": libc }),
            BTreeMap::new(),
            BTreeMap::new(),
            HashSet::default(),
        );
        graph.insert(node.dep_path.clone(), node);
    }
    let direct = BTreeMap::from([
        ("list-libc".to_string(), DepPath::from("list-libc@1.0.0".to_string())),
        ("scalar-libc".to_string(), DepPath::from("scalar-libc@1.0.0".to_string())),
    ]);

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, false, false, None, None,
    ));

    let yaml = lockfile.to_yaml_string().expect("serialize generated lockfile");
    let expected = format!(
        "{}\n",
        text_block! {
                "lockfileVersion: '9.0'"
                ""
                "settings:"
                "  autoInstallPeers: false"
                "  excludeLinksFromLockfile: false"
                ""
                "importers:"
                ""
                "  .:"
                "    dependencies:"
                "      list-libc:"
                "        specifier: 1.0.0"
                "        version: 1.0.0"
                "      scalar-libc:"
                "        specifier: 1.0.0"
                "        version: 1.0.0"
                ""
                "packages:"
                ""
                "  list-libc@1.0.0:"
                "    resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==}"
                "    libc: [glibc]"
                ""
                "  scalar-libc@1.0.0:"
                "    resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==}"
                "    libc: musl"
                ""
                "snapshots:"
                ""
                "  list-libc@1.0.0: {}"
                ""
                "  scalar-libc@1.0.0: {}"
        },
    );
    eprintln!("GENERATED LOCKFILE:\n{yaml}\n");
    assert_eq!(yaml, expected);
}
#[test]
fn dedupe_peers_round_trips_through_lockfile_settings() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
    }));
    let graph = DependenciesGraph::default();
    let direct = BTreeMap::new();

    let mut importers = BTreeMap::new();
    importers.insert(
        ".".to_string(),
        ImporterLockfileInput { manifest: &manifest, direct_dependencies_by_alias: direct.clone() },
    );
    let on = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        registries_by_prefix: &EMPTY_NAMED_REGISTRIES,
        registry_options_by_url: &EMPTY_REGISTRY_OPTIONS,
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
        previous_importers: None,
        previous_packages: None,
        update_reuse_scope: UpdateReuseScope::All,
        update_reuse_scopes_by_importer: BTreeMap::new(),
        time: BTreeMap::new(),
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
    let off_settings = off.settings.as_ref().expect("settings written");
    assert_eq!(off_settings.dedupe_peers, None);
    let off_yaml = serde_saphyr::to_string(off_settings).unwrap();
    assert!(!off_yaml.contains("dedupePeers"), "yaml: {off_yaml}");
}
#[test]
fn overrides_flow_into_lockfile_verbatim_including_convergence_selectors() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
    }));
    let graph = DependenciesGraph::default();

    let mut overrides = IndexMap::new();
    overrides.insert("foo@^4.0.0".to_string(), "4.0.9".to_string());
    overrides.insert("form-data@".to_string(), "4.0.6".to_string());

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest,
        &graph,
        BTreeMap::new(),
        true,
        false,
        Some(overrides.clone()),
        None,
    ));
    assert_eq!(lockfile.overrides.as_ref(), Some(&overrides));

    let yaml = serde_saphyr::to_string(&lockfile).unwrap();
    eprintln!("YAML:\n{yaml}\n");
    let reparsed: pnpm_lockfile::Lockfile = serde_saphyr::from_str(&yaml).unwrap();
    assert_eq!(reparsed.overrides, Some(overrides));
}
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
        HashSet::default(),
    );
    let mut graph = DependenciesGraph::default();
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
            patched_dependencies: patched,
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
#[test]
fn snapshot_link_uses_lockfile_root_while_importer_link_uses_project_root() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "app",
        "version": "1.0.0",
        "dependencies": {
            "consumer": "1.0.0",
            "peer": "workspace:*",
            "shared": "workspace:*",
            "wrapper": "1.0.0",
        },
    }));
    let shared = make_link_node("packages/shared", json!({ "name": "shared", "version": "1.0.0" }));
    let peer = make_link_node("packages/peer", json!({ "name": "peer", "version": "1.0.0" }));
    let wrapper = make_node(
        "wrapper",
        "1.0.0",
        json!({ "name": "wrapper", "version": "1.0.0" }),
        BTreeMap::from([("shared".to_string(), shared.dep_path.clone())]),
        BTreeMap::new(),
        HashSet::default(),
    );
    let mut consumer = make_node(
        "consumer",
        "1.0.0",
        json!({
            "name": "consumer",
            "version": "1.0.0",
            "peerDependencies": { "peer": "*" },
        }),
        BTreeMap::from([("peer".to_string(), peer.dep_path.clone())]),
        BTreeMap::from([(
            "peer".to_string(),
            PeerDep { version: "*".to_string(), optional: false },
        )]),
        HashSet::default(),
    );
    consumer.dep_path = DepPath::from("consumer@1.0.0(peer@packages+peer)");
    consumer.resolved_peer_names.insert("peer".to_string());

    let mut graph = DependenciesGraph::default();
    for node in [shared, peer, wrapper, consumer] {
        graph.insert(node.dep_path.clone(), node);
    }
    let direct = BTreeMap::from([
        ("consumer".to_string(), DepPath::from("consumer@1.0.0(peer@packages+peer)")),
        ("peer".to_string(), DepPath::from("link:../../../packages/peer")),
        ("shared".to_string(), DepPath::from("link:../../../packages/shared")),
        ("wrapper".to_string(), DepPath::from("wrapper@1.0.0")),
    ]);
    let importers = BTreeMap::from([(
        "apps/nested/app".to_string(),
        ImporterLockfileInput { manifest: &manifest, direct_dependencies_by_alias: direct },
    )]);

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

    let importer = lockfile.importers.get("apps/nested/app").expect("nested importer");
    let importer_dependencies = importer.dependencies.as_ref().expect("importer dependencies");
    for name in ["shared", "peer"] {
        let dependency = importer_dependencies.get(&PkgName::parse(name).unwrap()).unwrap();
        match &dependency.version {
            ImporterDepVersion::Link(target) => {
                assert_eq!(target, &format!("../../../packages/{name}"));
            }
            other => panic!("expected importer Link(..), got {other:?}"),
        }
    }

    let snapshots = lockfile.snapshots.as_ref().expect("snapshots map");
    let wrapper_key: PackageKey = "wrapper@1.0.0".parse().unwrap();
    let wrapper_dependencies = snapshots[&wrapper_key].dependencies.as_ref().unwrap();
    assert_eq!(
        wrapper_dependencies.get(&PkgName::parse("shared").unwrap()),
        Some(&SnapshotDepRef::Link("packages/shared".to_string())),
    );
    let consumer_snapshot = snapshots
        .iter()
        .find(|(key, _)| key.to_string().starts_with("consumer@1.0.0("))
        .map(|(_, snapshot)| snapshot)
        .expect("consumer peer snapshot");
    assert_eq!(
        consumer_snapshot.dependencies.as_ref().unwrap().get(&PkgName::parse("peer").unwrap()),
        Some(&SnapshotDepRef::Link("packages/peer".to_string())),
    );
}
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

    let lodash = make_node(
        "lodash",
        "4.17.21",
        json!({ "name": "lodash", "version": "4.17.21" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::default(),
    );
    let mut graph = DependenciesGraph::default();
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
    let b_snap = lockfile.importers.get("packages/b").expect("importer b");
    let lodash_name = PkgName::parse("lodash").unwrap();
    assert!(a_snap.dependencies.as_ref().unwrap().contains_key(&lodash_name));
    assert!(b_snap.dependencies.as_ref().unwrap().contains_key(&lodash_name));

    let packages = lockfile.packages.as_ref().expect("packages");
    let lodash_key: PackageKey = "lodash@4.17.21".parse().unwrap();
    assert!(packages.contains_key(&lodash_key), "single shared snapshot");
    assert_eq!(packages.len(), 1, "shared dep deduped to one entry");
}
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
        HashSet::default(),
    );

    let mut graph = DependenciesGraph::default();
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
#[test]
fn workspace_link_direct_dep_kept_when_exclude_links_from_lockfile_true() {
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
/// A lockfile with no named-registry package stays on 9.0, so projects
/// that don't use the feature keep a byte-identical lockfile.
#[test]
fn a_plain_package_leaves_the_lockfile_on_9_0() {
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
        HashSet::default(),
    );
    let mut graph = DependenciesGraph::default();
    graph.insert(node.dep_path.clone(), node);

    let mut direct = BTreeMap::new();
    direct.insert("react".to_string(), DepPath::from("react@17.0.2".to_string()));

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, true, false, None, None,
    ));

    assert_eq!(lockfile.lockfile_version.minor, 0);
}
