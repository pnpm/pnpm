use super::{
    super::DependenciesGraphToLockfileError, dependencies_graph_to_lockfile,
    error_from_single_node_graph, make_file_node, make_named_registry_node, make_node,
    make_node_with_optional, named_registries_with, single_importer_opts, write_manifest,
};
use crate::dependencies_graph_to_lockfile::packages::read_string_or_list;
use pnpm_deps_path::DepPath;
use pnpm_lockfile::{
    ImporterDepVersion, LockfileResolution, PackageKey, PackageMetadata, PkgName,
    RegistryResolution, SnapshotDepRef,
};
use pnpm_resolving_deps_resolver::{DependenciesGraph, DependenciesGraphNode};
use pnpm_resolving_resolver_base::ResolveResult;
use rustc_hash::FxHashSet as HashSet;
use serde_json::json;
use ssri::Integrity;
use std::{collections::BTreeMap, str::FromStr};

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

    let version = crate::dependencies_graph_to_lockfile::importers::importer_dep_version(
        "@scope/comp1",
        &node,
    )
    .unwrap();
    assert_eq!(
        version,
        ImporterDepVersion::File("comp1(react@16.0.0)".to_string()),
        "same-name injected deps must use the plain file: ref",
    );
    // A genuinely renamed alias keeps the alias form.
    let renamed =
        crate::dependencies_graph_to_lockfile::importers::importer_dep_version("renamed", &node)
            .unwrap();
    assert!(
        matches!(renamed, ImporterDepVersion::Alias(_)),
        "renamed aliases must keep the <name>@<ref> form: {renamed:?}",
    );
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
