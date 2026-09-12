use super::{build_deps_graph, build_deps_graph_for_platform, build_deps_subgraph};
use pnpm_graph_hasher::calc_dep_state_input_key;
use pnpm_lockfile::{
    BinaryArchive, BinaryResolution, BinarySpec, LockfileResolution, PackageKey, PackageMetadata,
    PkgName, PkgVerPeer, PlatformAssetResolution, PlatformAssetTarget, PlatformSelector,
    RegistryResolution, SnapshotDepRef, SnapshotEntry, VariationsResolution,
};
use pretty_assertions::assert_eq;
use ssri::Integrity;
use std::collections::HashMap;

fn name(text: &str) -> PkgName {
    PkgName::parse(text).expect("parse pkg name")
}

fn ver(text: &str) -> PkgVerPeer {
    text.parse().expect("parse PkgVerPeer")
}

fn key(name_text: &str, version: &str) -> PackageKey {
    PackageKey::new(name(name_text), ver(version))
}

fn integrity() -> Integrity {
    // Valid-shaped sha512 integrity. Content is irrelevant since
    // the adapter just stringifies it.
    "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        .parse()
        .expect("parse integrity")
}

fn registry_metadata() -> PackageMetadata {
    PackageMetadata {
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: integrity(),
            revision: None,
        }),
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

fn variations_metadata() -> PackageMetadata {
    let variant = |integrity: Integrity, os: &str, libc: Option<&str>| PlatformAssetResolution {
        resolution: LockfileResolution::Binary(BinaryResolution {
            url: format!("https://example.com/{os}.tar.gz"),
            integrity,
            bin: BinarySpec::Single("bin/node".to_string()),
            archive: BinaryArchive::Tarball,
            prefix: None,
        }),
        targets: vec![PlatformAssetTarget {
            os: os.to_string(),
            cpu: "x64".to_string(),
            libc: libc.map(str::to_string),
        }],
    };
    PackageMetadata {
        resolution: LockfileResolution::Variations(VariationsResolution {
            variants: vec![
                variant("sha512-AAAA".parse().unwrap(), "linux", None),
                variant("sha512-BBBB".parse().unwrap(), "linux", Some("musl")),
                variant("sha512-CCCC".parse().unwrap(), "darwin", None),
            ],
        }),
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies: None,
        peer_dependencies_meta: None,
    }
}

#[test]
fn registry_resolution_full_pkg_id_uses_integrity_verbatim() {
    let pkg = key("@scope/foo", "1.0.0");
    let snapshots = HashMap::from([(pkg.clone(), SnapshotEntry::default())]);
    let packages = HashMap::from([(pkg.clone(), registry_metadata())]);

    let graph = build_deps_graph(&snapshots, &packages);
    let node = graph.get(&pkg).expect("graph node");
    let expected_prefix = "@scope/foo@1.0.0:sha512-";
    assert!(
        node.full_pkg_id.starts_with(expected_prefix),
        "expected full_pkg_id to start with `{expected_prefix}`, got `{}`",
        node.full_pkg_id,
    );
}

#[test]
fn variation_input_keys_use_the_selected_platform_integrity() {
    let pkg = key("node", "runtime:22.0.0");
    let snapshots = HashMap::from([(pkg.clone(), SnapshotEntry::default())]);
    let packages = HashMap::from([(pkg.clone(), variations_metadata())]);
    let linux_glibc = PlatformSelector {
        os: "linux".to_string(),
        cpu: "x64".to_string(),
        libc: Some("glibc".to_string()),
    };
    let linux_musl = PlatformSelector {
        os: "linux".to_string(),
        cpu: "x64".to_string(),
        libc: Some("musl".to_string()),
    };
    let darwin = PlatformSelector { os: "darwin".to_string(), cpu: "x64".to_string(), libc: None };

    let input_key = |selector: &PlatformSelector| {
        let graph = build_deps_graph_for_platform(&snapshots, &packages, selector);
        calc_dep_state_input_key(&graph, &pkg, None)
    };
    let keys = [input_key(&linux_glibc), input_key(&linux_musl), input_key(&darwin)];
    assert_eq!(keys.iter().collect::<std::collections::HashSet<_>>().len(), 3);
}

#[test]
fn dependencies_become_children() {
    let parent_key = key("parent", "1.0.0");
    let child_key = key("child", "2.0.0");
    let dependencies = HashMap::from([(name("child"), SnapshotDepRef::Plain(ver("2.0.0")))]);
    let snapshots = HashMap::from([
        (
            parent_key.clone(),
            SnapshotEntry { dependencies: Some(dependencies), ..Default::default() },
        ),
        (child_key.clone(), SnapshotEntry::default()),
    ]);
    let packages = HashMap::from([
        (parent_key.clone(), registry_metadata()),
        (child_key.clone(), registry_metadata()),
    ]);

    let graph = build_deps_graph(&snapshots, &packages);
    let parent_node = graph.get(&parent_key).expect("parent node");
    assert_eq!(parent_node.children.len(), 1);
    let resolved = parent_node.children.get("child").expect("alias `child` present");
    assert_eq!(resolved, &child_key);
}

#[test]
fn optional_dependencies_fold_into_children() {
    let parent_key = key("parent", "1.0.0");
    let opt_key = key("optional", "3.0.0");
    let optional = HashMap::from([(name("optional"), SnapshotDepRef::Plain(ver("3.0.0")))]);
    let snapshots = HashMap::from([
        (
            parent_key.clone(),
            SnapshotEntry {
                dependencies: None,
                optional_dependencies: Some(optional),
                ..Default::default()
            },
        ),
        (opt_key.clone(), SnapshotEntry::default()),
    ]);
    let packages =
        HashMap::from([(parent_key.clone(), registry_metadata()), (opt_key, registry_metadata())]);

    let graph = build_deps_graph(&snapshots, &packages);
    let parent_node = graph.get(&parent_key).expect("parent node");
    assert!(parent_node.children.contains_key("optional"));
}

#[test]
fn snapshot_without_metadata_is_skipped() {
    let pkg = key("orphan", "1.0.0");
    let snapshots = HashMap::from([(pkg, SnapshotEntry::default())]);
    let packages: HashMap<PackageKey, PackageMetadata> = HashMap::new();

    let graph = build_deps_graph(&snapshots, &packages);
    assert!(graph.is_empty(), "orphan snapshot must not produce a graph node");
}

#[test]
fn subgraph_with_empty_roots_is_empty() {
    let parent_key = key("a", "1.0.0");
    let child_key = key("b", "1.0.0");
    let deps = HashMap::from([(name("b"), SnapshotDepRef::Plain(ver("1.0.0")))]);
    let snapshots = HashMap::from([
        (parent_key, SnapshotEntry { dependencies: Some(deps), ..Default::default() }),
        (child_key, SnapshotEntry::default()),
    ]);
    let packages = HashMap::from([
        (key("a", "1.0.0"), registry_metadata()),
        (key("b", "1.0.0"), registry_metadata()),
    ]);

    let graph = build_deps_subgraph(&snapshots, &packages, std::iter::empty());
    assert!(graph.is_empty(), "empty roots must produce an empty graph");
}

#[test]
fn subgraph_walks_forward_closure() {
    let key_a = key("a", "1.0.0");
    let key_b = key("b", "1.0.0");
    let key_c = key("c", "1.0.0");
    let key_d = key("d", "1.0.0"); // unrelated
    let a_deps = HashMap::from([(name("b"), SnapshotDepRef::Plain(ver("1.0.0")))]);
    let b_deps = HashMap::from([(name("c"), SnapshotDepRef::Plain(ver("1.0.0")))]);
    let snapshots = HashMap::from([
        (key_a.clone(), SnapshotEntry { dependencies: Some(a_deps), ..Default::default() }),
        (key_b.clone(), SnapshotEntry { dependencies: Some(b_deps), ..Default::default() }),
        (key_c.clone(), SnapshotEntry::default()),
        (key_d.clone(), SnapshotEntry::default()),
    ]);
    let packages = HashMap::from([
        (key_a.clone(), registry_metadata()),
        (key_b.clone(), registry_metadata()),
        (key_c.clone(), registry_metadata()),
        (key_d.clone(), registry_metadata()),
    ]);

    let graph = build_deps_subgraph(&snapshots, &packages, std::iter::once(key_a.clone()));
    assert!(graph.contains_key(&key_a));
    assert!(graph.contains_key(&key_b), "b is in a's closure");
    assert!(graph.contains_key(&key_c), "c is in a's closure via b");
    assert!(!graph.contains_key(&key_d), "d is unrelated; must not be included");
}

#[test]
fn subgraph_terminates_on_cycle() {
    let key_a = key("a", "1.0.0");
    let key_b = key("b", "1.0.0");
    let a_deps = HashMap::from([(name("b"), SnapshotDepRef::Plain(ver("1.0.0")))]);
    let b_deps = HashMap::from([(name("a"), SnapshotDepRef::Plain(ver("1.0.0")))]);
    let snapshots = HashMap::from([
        (key_a.clone(), SnapshotEntry { dependencies: Some(a_deps), ..Default::default() }),
        (key_b.clone(), SnapshotEntry { dependencies: Some(b_deps), ..Default::default() }),
    ]);
    let packages =
        HashMap::from([(key_a.clone(), registry_metadata()), (key_b.clone(), registry_metadata())]);

    let graph = build_deps_subgraph(&snapshots, &packages, std::iter::once(key_a.clone()));
    assert_eq!(graph.len(), 2);
    assert!(graph.contains_key(&key_a));
    assert!(graph.contains_key(&key_b));
}
