use super::{
    DependenciesGraphToLockfileError, GraphToLockfileOptions, ImporterLockfileInput,
    dependencies_graph_to_lockfile as try_dependencies_graph_to_lockfile, manifest_has_bin,
    read_string_or_list,
};
use indexmap::IndexMap;
use pnpm_deps_path::DepPath;
use pnpm_lockfile::{
    DirectoryResolution, GitResolution, ImporterDepVersion, LockfileResolution, PackageKey,
    PackageMetadata, PkgName, PkgNameVer, ProjectSnapshot, RegistryResolution,
    ResolvedDependencyMap, ResolvedDependencySpec, SnapshotDepRef, TarballResolution,
    VariationsResolution,
};
use pnpm_package_manifest::PackageManifest;
use pnpm_resolving_deps_resolver::{
    ChildEdge, DependenciesGraph, DependenciesGraphNode, DependenciesTreeNode, DirectDep, NodeId,
    PeerDep, ResolvePeersOptions, ResolvedPackage, ResolvedTree, TreeChildren, UpdateReuseScope,
    resolve_peers,
};
use pnpm_resolving_resolver_base::{PkgResolutionId, ResolveResult};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

static EMPTY_REGISTRY_OPTIONS: std::collections::BTreeMap<String, pnpm_lockfile::RegistryOptions> =
    std::collections::BTreeMap::new();

static EMPTY_NAMED_REGISTRIES: std::sync::LazyLock<std::collections::HashMap<String, String>> =
    std::sync::LazyLock::new(std::collections::HashMap::new);
use serde_json::json;
use ssri::Integrity;
use std::{collections::BTreeMap, str::FromStr, sync::Arc};
use tempfile::TempDir;
use text_block_macros::text_block;

#[test]
fn recognizes_bin_directories_in_package_manifests() {
    assert_eq!(
        manifest_has_bin(Some(&json!({
            "directories": {
                "bin": "cli"
            }
        }))),
        Some(true),
    );
    assert_eq!(manifest_has_bin(Some(&json!({ "directories": { "bin": "" } }))), None);
}

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

/// An install that resolved no publish dates leaves the section out
/// rather than writing an empty map.
#[test]
fn no_recorded_publish_dates_leave_out_the_time_section() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": { "react": "^17.0.2" },
    }));
    let graph = DependenciesGraph::default();
    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest,
        &graph,
        BTreeMap::new(),
        true,
        false,
        None,
        None,
    ));

    assert_eq!(lockfile.time, None);
}

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
        HashSet::default(),
    );

    let mut graph = DependenciesGraph::default();
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
fn fresh_install_records_string_libc_without_coercing_scalar_bundle_metadata() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": { "sass-embedded-linux-musl-x64": "1.100.0" },
    }));
    let node = make_node(
        "sass-embedded-linux-musl-x64",
        "1.100.0",
        json!({
            "name": "sass-embedded-linux-musl-x64",
            "version": "1.100.0",
            "cpu": ["x64"],
            "os": ["linux"],
            "libc": "musl",
            "bundledDependencies": "not-an-array",
        }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::default(),
    );
    let mut graph = DependenciesGraph::default();
    graph.insert(node.dep_path.clone(), node);
    let direct = BTreeMap::from([(
        "sass-embedded-linux-musl-x64".to_string(),
        DepPath::from("sass-embedded-linux-musl-x64@1.100.0".to_string()),
    )]);

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, false, false, None, None,
    ));

    let package_key: PackageKey = "sass-embedded-linux-musl-x64@1.100.0".parse().unwrap();
    let metadata = &lockfile.packages.as_ref().expect("packages")[&package_key];
    assert_eq!(metadata.libc.as_deref(), Some(["musl".to_string()].as_slice()));
    assert!(metadata.bundled_dependencies.is_none());
}

#[test]
fn string_or_list_metadata_accepts_arrays_and_rejects_other_values() {
    let array_manifest = json!({ "libc": ["glibc", "musl"] });
    assert_eq!(
        read_string_or_list(Some(&array_manifest), "libc"),
        Some(vec!["glibc".to_string(), "musl".to_string()].into()),
    );

    let object_manifest = json!({ "libc": { "name": "musl" } });
    assert_eq!(read_string_or_list(Some(&object_manifest), "libc"), None);
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
fn duplicate_manifest_alias_uses_pnpm_dependency_field_precedence() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "devDependencies": { "duplicated": "^1.0.0" },
        "optionalDependencies": { "duplicated": "^1.0.0" },
    }));

    let duplicated = make_node(
        "duplicated",
        "1.0.0",
        json!({ "name": "duplicated", "version": "1.0.0" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::default(),
    );
    let mut graph = DependenciesGraph::default();
    graph.insert(duplicated.dep_path.clone(), duplicated);

    let mut direct = BTreeMap::new();
    direct.insert("duplicated".to_string(), DepPath::from("duplicated@1.0.0".to_string()));

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, false, false, None, None,
    ));

    let importer = lockfile.root_project().expect("root importer");
    assert!(importer.dev_dependencies.is_none(), "optionalDependencies wins over devDependencies");
    let opt = importer.optional_dependencies.as_ref().expect("optional deps");
    assert!(opt.contains_key(&PkgName::parse("duplicated").unwrap()));
}

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
        HashSet::default(),
    );
    let mut graph = DependenciesGraph::default();
    graph.insert(zkochan_js_yaml.dep_path.clone(), zkochan_js_yaml);

    let mut direct = BTreeMap::new();
    direct.insert("js-yaml".to_string(), DepPath::from("@zkochan/js-yaml@0.0.11".to_string()));

    let mut catalogs: pnpm_catalogs_types::Catalogs = BTreeMap::new();
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
        catalogs: &catalogs,
        registry: "https://registry.npmjs.org",
        lockfile_include_tarball_url: false,
        previous_importers: None,
        previous_packages: None,
        update_reuse_scope: UpdateReuseScope::All,
        update_reuse_scopes_by_importer: BTreeMap::new(),
        time: BTreeMap::new(),
    });

    let snapshots = lockfile.catalogs.as_ref().expect("catalogs snapshot present");
    let entry = snapshots
        .get("default")
        .and_then(|catalog| catalog.get("js-yaml"))
        .expect("aliased catalog entry recorded");
    assert_eq!(entry.specifier, "npm:@zkochan/js-yaml@0.0.11");
    assert_eq!(entry.version, "0.0.11");
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

#[test]
fn malformed_importer_dependency_path_returns_structured_error() {
    let error = error_from_single_node_graph("broken", "1.0.0(react@17.0.0");

    let DependenciesGraphToLockfileError::ImporterDependency { alias, dep_path, .. } = error else {
        panic!("expected an importer-dependency error, got {error}");
    };
    assert_eq!(alias, "broken");
    assert_eq!(dep_path, "1.0.0(react@17.0.0");
}

/// A resolver that hands back no package name leaves a bare
/// `file:<path>` depPath, which keys neither `packages:` nor
/// `snapshots:`. Dropping it would write a lockfile whose importer
/// points at a package neither map describes — see
/// <https://github.com/pnpm/pnpm/issues/13410>.
#[test]
fn nameless_dep_path_returns_structured_error() {
    let error = error_from_single_node_graph("no-manifest", "file:no-manifest-1.0.0.tgz");

    let DependenciesGraphToLockfileError::UnkeyedDepPath { dep_path, .. } = error else {
        panic!("expected an unkeyed-depPath error, got {error}");
    };
    assert_eq!(dep_path, "file:no-manifest-1.0.0.tgz");
}

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
        HashSet::default(),
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
        optional_children: HashSet::default(),
        peer_dependencies: react_dom_peers,
        transitive_peer_dependencies: HashSet::default(),
        resolved_peer_names: std::iter::once("react".to_string()).collect(),
        depth: 1,
        installable: true,
        is_pure: false,
        optional: false,
    };

    let mut graph = DependenciesGraph::default();
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
        HashSet::default(),
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
        HashSet::default(),
    );

    let mut graph = DependenciesGraph::default();
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

#[test]
fn snapshot_preserves_optional_child_edges_from_resolved_tree() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": { "outer": "^1.0.0" },
    }));

    let outer_id: Arc<str> = "outer@1.0.0".into();
    let inner_id: Arc<str> = "inner@1.0.0".into();
    let outer_node_id = NodeId::next();

    let mut tree = ResolvedTree {
        direct: vec![DirectDep {
            alias: "outer".to_string(),
            node_id: outer_node_id.clone(),
            id: outer_id.to_string(),
        }],
        packages: HashMap::from_iter([
            (
                Arc::<str>::clone(&outer_id),
                ResolvedPackage {
                    id: Arc::<str>::clone(&outer_id),
                    result: Arc::new(make_resolve_result(
                        "outer",
                        "1.0.0",
                        json!({ "name": "outer", "version": "1.0.0" }),
                    )),
                    peer_dependencies: BTreeMap::new(),
                    optional: false,
                    is_leaf: false,
                },
            ),
            (
                Arc::<str>::clone(&inner_id),
                ResolvedPackage {
                    id: Arc::<str>::clone(&inner_id),
                    result: Arc::new(make_resolve_result(
                        "inner",
                        "1.0.0",
                        json!({ "name": "inner", "version": "1.0.0" }),
                    )),
                    peer_dependencies: BTreeMap::new(),
                    optional: true,
                    is_leaf: true,
                },
            ),
        ]),
        dependencies_tree: HashMap::from_iter([(
            outer_node_id,
            DependenciesTreeNode::new(
                Arc::<str>::clone(&outer_id),
                TreeChildren::Lazy { parent_ids: Arc::new(Vec::new()).into() },
                0,
                true,
            ),
        )]),
        all_peer_dep_names: HashSet::default(),
        policy_violations: Vec::new(),
        applied_patches: HashSet::default(),
        children_by_id: HashMap::from_iter([(
            outer_id,
            Arc::new(vec![ChildEdge {
                alias: "inner".to_string(),
                pkg_id: inner_id,
                optional: true,
            }]),
        )]),
    };

    let resolved = resolve_peers(&mut tree, ResolvePeersOptions::default());
    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest,
        &resolved.graph,
        resolved.direct_dependencies_by_alias,
        false,
        false,
        None,
        None,
    ));

    let snapshots = lockfile.snapshots.as_ref().unwrap();
    let outer_key: PackageKey = "outer@1.0.0".parse().unwrap();
    let outer_snap = &snapshots[&outer_key];
    assert!(outer_snap.dependencies.is_none(), "optional child must not be written as regular");
    let opt = outer_snap.optional_dependencies.as_ref().expect("opt deps map");
    assert!(opt.contains_key(&PkgName::parse("inner").unwrap()));

    let inner_key: PackageKey = "inner@1.0.0".parse().unwrap();
    assert!(snapshots[&inner_key].optional, "optional child edge keeps the child optional");
}

#[test]
fn snapshot_records_transitive_peer_dependencies_sorted() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": { "outer": "^1.0.0" },
    }));

    let mut transitive: HashSet<String> = HashSet::default();
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
    let mut graph = DependenciesGraph::default();
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
        HashSet::default(),
    );
    let opt = make_node_with_optional(
        "opt",
        "1.0.0",
        json!({ "name": "opt", "version": "1.0.0" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::default(),
        true,
    );

    let mut graph = DependenciesGraph::default();
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

#[test]
fn transitive_optional_is_recomputed_for_packages_reachable_via_a_non_optional_path() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies":         { "b": "^1.0.0" },
        "optionalDependencies": { "a": "^1.0.0" },
    }));

    let node_c = make_node_with_optional(
        "c",
        "1.0.0",
        json!({ "name": "c", "version": "1.0.0" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::default(),
        true,
    );

    let mut a_children = BTreeMap::new();
    a_children.insert("c".to_string(), DepPath::from("c@1.0.0".to_string()));
    let node_a = make_node_with_optional(
        "a",
        "1.0.0",
        json!({ "name": "a", "version": "1.0.0", "dependencies": { "c": "^1.0.0" } }),
        a_children,
        BTreeMap::new(),
        HashSet::default(),
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
        HashSet::default(),
        false,
    );

    let mut graph = DependenciesGraph::default();
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
        HashSet::default(),
        true,
    );
    let subdep2 = make_node_with_optional(
        "subdep2",
        "1.0.0",
        json!({ "name": "subdep2", "version": "1.0.0" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::default(),
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
        HashSet::default(),
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
        HashSet::default(),
        false,
    );

    let mut graph = DependenciesGraph::default();
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

#[test]
fn file_dep_child_renders_as_bare_file_ref() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "app",
        "version": "1.0.0",
        "dependencies": { "nested-parent": "file:./parent" },
    }));

    let child = make_file_node("nested-child", "child");
    let mut parent = make_file_node("nested-parent", "parent");
    parent.children.insert("nested-child".to_string(), child.dep_path.clone());

    let mut graph = DependenciesGraph::default();
    let parent_dep_path = parent.dep_path.clone();
    graph.insert(parent_dep_path.clone(), parent);
    graph.insert(child.dep_path.clone(), child);

    let direct = BTreeMap::from([("nested-parent".to_string(), parent_dep_path)]);

    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, false, false, None, None,
    ));

    let snapshots = lockfile.snapshots.as_ref().expect("snapshots map");
    let parent_key: PackageKey = "nested-parent@file:parent".parse().unwrap();
    let deps = snapshots[&parent_key].dependencies.as_ref().expect("nested-parent dependencies");
    let child_ref = deps.get(&PkgName::parse("nested-child").unwrap()).expect("nested-child child");
    assert_eq!(dbg!(child_ref).to_string(), "file:child");
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
        HashSet::default(),
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
        HashSet::default(),
        true,
    );

    let mut graph = DependenciesGraph::default();
    graph.insert(parent.dep_path.clone(), parent);
    graph.insert(peer_x.dep_path.clone(), peer_x);

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

/// An injected workspace dep whose alias equals its package name must
/// serialize as the plain `file:<path>(peers)` ref, matching pnpm v11 —
/// the `<name>@<ref>` alias form is reserved for renamed deps, and
/// consumers compose `alias@version` into a snapshot key, so a
/// self-aliased ref would double-prefix that key.
#[test]
fn same_name_injected_dep_serializes_as_plain_file_ref() {
    use pnpm_lockfile::ImporterDepVersion;

    let node = DependenciesGraphNode {
        dep_path: DepPath::from("@scope/comp1@file:comp1(react@16.0.0)".to_string()),
        resolved_package_id: "file:comp1".to_string(),
        resolve_result: std::sync::Arc::new(ResolveResult {
            id: "file:comp1".into(),
            // Directory resolutions carry no structured name.
            name_ver: None,
            latest: None,
            published_at: None,
            manifest: Some(std::sync::Arc::new(
                serde_json::json!({ "name": "@scope/comp1", "version": "1.0.0" }),
            )),
            resolution: pnpm_lockfile::DirectoryResolution { directory: "comp1".to_string() }
                .into(),
            resolved_via: "local-filesystem".to_string(),
            normalized_bare_specifier: None,
            alias: Some("@scope/comp1".to_string()),
            policy_violation: None,
        }),
        children: BTreeMap::new(),
        optional_children: HashSet::default(),
        peer_dependencies: BTreeMap::new(),
        transitive_peer_dependencies: HashSet::default(),
        resolved_peer_names: HashSet::default(),
        depth: 0,
        installable: true,
        is_pure: false,
        optional: false,
    };

    let version = super::importer_dep_version("@scope/comp1", &node).unwrap();
    assert_eq!(
        version,
        ImporterDepVersion::File("comp1(react@16.0.0)".to_string()),
        "same-name injected deps must use the plain file: ref",
    );
    // A genuinely renamed alias keeps the alias form.
    let renamed = super::importer_dep_version("renamed", &node).unwrap();
    assert!(
        matches!(renamed, ImporterDepVersion::Alias(_)),
        "renamed aliases must keep the <name>@<ref> form: {renamed:?}",
    );
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

// A `pacquet update n` (UpdateReuseScope::Except containing the package
// name) targets the dependency, so its divergent `file:` resolution is
// kept rather than reverted to the prior `link:`.
#[test]
fn injected_workspace_dep_flips_to_file_when_update_targets_it() {
    let (_tmp, manifest, graph, direct) = injected_link_fixture();
    let previous = previous_importers_with_link("n", "workspace:*", "../n");

    let lockfile = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        registries_by_prefix: &EMPTY_NAMED_REGISTRIES,
        registry_options_by_url: &EMPTY_REGISTRY_OPTIONS,
        previous_importers: Some(&previous),
        update_reuse_scope: UpdateReuseScope::Except(
            std::iter::once(("n".to_string(), None)).collect(),
        ),
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
        "an update that targets n must keep the fresh file: resolution, got {:?}",
        entry.version,
    );
}

// A changed specifier (a new or edited manifest entry) targets the
// dependency too, so the divergent `file:` stands.
#[test]
fn injected_workspace_dep_flips_to_file_when_specifier_changed() {
    let (_tmp, manifest, graph, direct) = injected_link_fixture();
    // Previous lockfile recorded a different specifier than the manifest
    // now declares (`workspace:*`).
    let previous = previous_importers_with_link("n", "workspace:^1.0.0", "../n");

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
    assert!(
        matches!(&entry.version, ImporterDepVersion::File(_)),
        "a spec change must keep the fresh file: resolution, got {:?}",
        entry.version,
    );
}

// `pacquet update n --recursive` lowers to a `ByImporter` policy whose
// global scope is `All`, with the named package recorded per importer.
// The guard resolves the effective per-importer scope, so `n` in the
// importer that declares it is targeted and its divergent `file:` stands.
#[test]
fn injected_workspace_dep_flips_to_file_when_recursive_update_targets_it_per_importer() {
    let (_tmp, manifest, graph, direct) = injected_link_fixture();
    let previous = previous_importers_with_link("n", "workspace:*", "../n");
    // Global All (recursive updates never withhold globally); the named
    // package lives in the per-importer scope for the root importer (".").
    let scopes_by_importer = BTreeMap::from([(
        ".".to_string(),
        UpdateReuseScope::Except(std::iter::once(("n".to_string(), None)).collect()),
    )]);

    let lockfile = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        registries_by_prefix: &EMPTY_NAMED_REGISTRIES,
        registry_options_by_url: &EMPTY_REGISTRY_OPTIONS,
        previous_importers: Some(&previous),
        update_reuse_scope: UpdateReuseScope::All,
        update_reuse_scopes_by_importer: scopes_by_importer,
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
        "a recursive update naming n must keep the fresh file: resolution, got {:?}",
        entry.version,
    );
}

// The pnpm/pnpm#10433 scenario for the recursive path: `pacquet update <other>
// --recursive` records `<other>` (not `n`) in the per-importer scope, so
// `n` is untargeted and its `link:` must be preserved even though it
// re-resolved to a divergent `file:`. Without honoring the per-importer
// scope the guard would read the global `All` and (correctly, here) also
// preserve — so to prove the per-importer scope is actually consulted, the
// companion test above names `n` and asserts the opposite outcome.
#[test]
fn injected_workspace_dep_keeps_link_when_recursive_update_targets_other_pkg() {
    let (_tmp, manifest, graph, direct) = injected_link_fixture();
    let previous = previous_importers_with_link("n", "workspace:*", "../n");
    let scopes_by_importer = BTreeMap::from([(
        ".".to_string(),
        UpdateReuseScope::Except(std::iter::once(("some-other-pkg".to_string(), None)).collect()),
    )]);

    let lockfile = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        registries_by_prefix: &EMPTY_NAMED_REGISTRIES,
        registry_options_by_url: &EMPTY_REGISTRY_OPTIONS,
        previous_importers: Some(&previous),
        update_reuse_scope: UpdateReuseScope::All,
        update_reuse_scopes_by_importer: scopes_by_importer,
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
        other => panic!("a recursive update of another package must keep n's link:, got {other:?}"),
    }
}

// Bare `pacquet update` (UpdateReuseScope::None) re-resolves the whole
// graph, so every workspace dependency is targeted and its divergent
// `file:` resolution stands — exercises the `None` arm of the guard.
#[test]
fn injected_workspace_dep_flips_to_file_on_scope_wide_update() {
    let (_tmp, manifest, graph, direct) = injected_link_fixture();
    let previous = previous_importers_with_link("n", "workspace:*", "../n");

    let lockfile = dependencies_graph_to_lockfile(GraphToLockfileOptions {
        registries_by_prefix: &EMPTY_NAMED_REGISTRIES,
        registry_options_by_url: &EMPTY_REGISTRY_OPTIONS,
        previous_importers: Some(&previous),
        update_reuse_scope: UpdateReuseScope::None,
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
        "a scope-wide update must keep the fresh file: resolution, got {:?}",
        entry.version,
    );
}

// `node_pkg_name` (the guard's `update <name>` scope matcher) reads the
// structured `name_ver` when the resolver produced one and falls back to
// the fetched manifest's `name` for directory resolutions, whose
// `name_ver` is unset.
#[test]
fn node_pkg_name_prefers_name_ver_and_falls_back_to_manifest() {
    let mut node = make_file_node("n", "packages/n");
    assert_eq!(super::node_pkg_name(&node), Some("n".to_string()));

    let resolve_result = ResolveResult {
        name_ver: Some("renamed@1.0.0".parse().expect("parse PkgNameVer")),
        ..(*node.resolve_result).clone()
    };
    node.resolve_result = std::sync::Arc::new(resolve_result);
    assert_eq!(super::node_pkg_name(&node), Some("renamed".to_string()));
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

/// A canonical named-registry tarball drops its URL — it is rebuilt from
/// the alias on read — and its presence stamps lockfile format 12.0.
#[test]
fn named_registry_package_keeps_the_format_and_drops_a_canonical_tarball() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": { "foo": "work:1.0.0" },
    }));

    let node = make_named_registry_node(
        "foo",
        "work",
        "1.0.0",
        "https://npm.enterprise.example.com/foo/-/foo-1.0.0.tgz",
    );
    let mut graph = DependenciesGraph::default();
    graph.insert(node.dep_path.clone(), node);

    let mut direct = BTreeMap::new();
    direct.insert("foo".to_string(), DepPath::from("foo@work:1.0.0".to_string()));

    let registries_by_prefix = named_registries_with("work", "https://npm.enterprise.example.com/");
    let mut opts = single_importer_opts(&manifest, &graph, direct, true, false, None, None);
    opts.registries_by_prefix = &registries_by_prefix;

    let lockfile = dependencies_graph_to_lockfile(opts);

    // The registry-qualified key is additive, so it must not move the format.
    assert_eq!(lockfile.lockfile_version.major, 9);
    assert_eq!(lockfile.lockfile_version.minor, 0);

    let packages = lockfile.packages.as_ref().expect("packages map");
    let key: PackageKey = "foo@work:1.0.0".parse().unwrap();
    let metadata = packages.get(&key).expect("registry-qualified entry");
    assert!(
        matches!(metadata.resolution, LockfileResolution::Registry(_)),
        "a canonical named-registry tarball is rebuilt from the alias, so the URL is dropped: {:?}",
        metadata.resolution,
    );
    // The depPath already carries a parseable semver, so no redundant
    // `version` key is written.
    assert_eq!(metadata.version, None);
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

/// An alias the writer can't resolve must never drop the tarball URL:
/// testing it against the default registry could classify it as
/// reconstructible and leave an entry no install can fetch.
#[test]
fn an_unresolvable_alias_keeps_the_tarball_url() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": { "foo": "work:1.0.0" },
    }));

    // Canonical under the *default* registry, which is what makes the
    // unguarded fallback drop it.
    let tarball = "https://registry.npmjs.org/foo/-/foo-1.0.0.tgz";
    let node = make_named_registry_node("foo", "work", "1.0.0", tarball);
    let mut graph = DependenciesGraph::default();
    graph.insert(node.dep_path.clone(), node);

    let mut direct = BTreeMap::new();
    direct.insert("foo".to_string(), DepPath::from("foo@work:1.0.0".to_string()));

    // `work` is deliberately absent from the map.
    let lockfile = dependencies_graph_to_lockfile(single_importer_opts(
        &manifest, &graph, direct, true, false, None, None,
    ));

    let packages = lockfile.packages.as_ref().expect("packages map");
    let key: PackageKey = "foo@work:1.0.0".parse().unwrap();
    let metadata = packages.get(&key).expect("registry-qualified entry");
    match &metadata.resolution {
        LockfileResolution::Tarball(resolution) => assert_eq!(resolution.tarball, tarball),
        other => panic!("an unresolvable alias must keep its tarball URL, got {other:?}"),
    }
}

/// pnpm/pnpm#13846: registries serve `deprecated` inconsistently for
/// the same published version.
#[test]
fn unchanged_resolutions_keep_their_previous_package_metadata() {
    let (_tmp, manifest) = write_manifest(json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": { "react": "^17.0.2" },
    }));
    let build = |previous: Option<&std::collections::HashMap<PackageKey, PackageMetadata>>| {
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
        let direct =
            BTreeMap::from([("react".to_string(), DepPath::from("react@17.0.2".to_string()))]);
        let mut opts = single_importer_opts(&manifest, &graph, direct, true, false, None, None);
        opts.previous_packages = previous;
        let lockfile = dependencies_graph_to_lockfile(opts);
        let key: PackageKey = "react@17.0.2".parse().unwrap();
        lockfile.packages.expect("packages map")[&key].clone()
    };

    let fresh = build(None);
    assert_eq!(fresh.deprecated, None, "the freshly served metadata carries no deprecation");

    let mut previous_entry = fresh;
    previous_entry.deprecated = Some("No longer maintained".to_string());
    let previous = std::collections::HashMap::from([(
        "react@17.0.2".parse::<PackageKey>().unwrap(),
        previous_entry.clone(),
    )]);
    assert_eq!(
        build(Some(&previous)),
        previous_entry,
        "an unchanged resolution keeps its recorded deprecation",
    );

    let mut republished = previous_entry;
    republished.resolution = LockfileResolution::Registry(RegistryResolution {
        integrity: Integrity::from_str(
            "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB==",
        )
        .expect("parse fake integrity"),
        revision: None,
    });
    let previous = std::collections::HashMap::from([(
        "react@17.0.2".parse::<PackageKey>().unwrap(),
        republished,
    )]);
    assert_eq!(
        build(Some(&previous)).deprecated,
        None,
        "a changed resolution takes the freshly served metadata",
    );
}

/// With several unkeyable nodes, the reported failure must be the first
/// one in the graph's own iteration order — the parallel node fan-out
/// folds its results in that order, like the serial loop it replaced.
#[test]
fn the_first_unkeyable_node_in_graph_order_is_the_reported_one() {
    let mut graph: DependenciesGraph = HashMap::default();
    for name in ["alpha", "beta"] {
        let mut node = make_node(
            name,
            "1.0.0",
            json!({ "name": name, "version": "1.0.0" }),
            BTreeMap::new(),
            BTreeMap::new(),
            HashSet::default(),
        );
        node.dep_path = DepPath::from(format!("!broken-{name}!"));
        assert!(
            node.dep_path.as_str().parse::<PackageKey>().is_err(),
            "the fixture path must not key a snapshot row",
        );
        graph.insert(node.dep_path.clone(), node);
    }
    let expected = graph
        .values()
        .map(|node| node.dep_path.as_str().to_string())
        .next()
        .expect("two nodes were inserted");

    let (_tmp, manifest) = write_manifest(json!({ "name": "root", "version": "1.0.0" }));
    let result = try_dependencies_graph_to_lockfile(single_importer_opts(
        &manifest,
        &graph,
        BTreeMap::new(),
        false,
        false,
        None,
        None,
    ));

    let Err(DependenciesGraphToLockfileError::UnkeyedDepPath { dep_path, .. }) = result else {
        panic!("an unkeyable dep path must fail the conversion");
    };
    assert_eq!(dep_path, expected);
}

/// A multi-importer workspace built through the parallel importer
/// fan-out must record every importer with the entries the serial loop
/// recorded.
#[test]
fn every_importer_of_a_workspace_is_recorded() {
    let node = make_node(
        "dep",
        "1.0.0",
        json!({ "name": "dep", "version": "1.0.0" }),
        BTreeMap::new(),
        BTreeMap::new(),
        HashSet::default(),
    );
    let dep_path = node.dep_path.clone();
    let mut graph: DependenciesGraph = HashMap::default();
    graph.insert(dep_path.clone(), node);

    let manifests: Vec<(TempDir, PackageManifest)> = ["root", "a", "b"]
        .into_iter()
        .map(|name| {
            write_manifest(json!({
                "name": name, "version": "1.0.0", "dependencies": { "dep": "^1.0.0" },
            }))
        })
        .collect();
    let direct: BTreeMap<String, DepPath> = BTreeMap::from([("dep".to_string(), dep_path)]);
    let mut opts =
        single_importer_opts(&manifests[0].1, &graph, direct.clone(), false, false, None, None);
    opts.importers = [".", "packages/a", "packages/b"]
        .into_iter()
        .zip(&manifests)
        .map(|(id, (_, manifest))| {
            (
                id.to_string(),
                ImporterLockfileInput { manifest, direct_dependencies_by_alias: direct.clone() },
            )
        })
        .collect();

    let lockfile = dependencies_graph_to_lockfile(opts);
    for id in [".", "packages/a", "packages/b"] {
        let importer = lockfile.importers.get(id).expect("every importer must be recorded");
        let recorded = importer
            .dependencies
            .as_ref()
            .and_then(|deps| deps.get(&PkgName::parse("dep").expect("parse alias")))
            .expect("the direct dependency must be recorded");
        assert_eq!(recorded.specifier, "^1.0.0", "importer {id}");
    }
}
