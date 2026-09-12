use super::{
    super::{LockfileToHoistedDepGraphOptions, lockfile_to_hoisted_dep_graph},
    dep_key, directory_resolution, lockfile_with, metadata_stub, pkg_name, resolved_dep, ver_peer,
    workspace_lockfile,
};
use pnpm_lockfile::{
    Lockfile, PackageMetadata, ResolvedDependencyMap, ResolvedDependencySpec, SnapshotDepRef,
    SnapshotEntry,
};
use pnpm_modules_yaml::DepPath;
use pretty_assertions::assert_eq;
use std::{collections::HashMap, path::PathBuf};

/// A peer-suffixed reference (`b@1.0.0(peer@2.0.0)`) must resolve its
/// metadata through the peer-stripped `packages:` key (`b@1.0.0`) —
/// lockfile `packages:` keys never carry peer suffixes. Before the
/// fallback in `lookup_package_metadata`, the walker silently dropped
/// every peered node and its whole subtree from the graph, so the
/// hoisted linker never materialized them.
#[test]
fn walker_resolves_peered_reference_via_peerless_packages_key() {
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("b"), resolved_dep("1.0.0(peer@2.0.0)"));
    root_deps.insert(pkg_name("peer"), resolved_dep("2.0.0"));

    // `packages:` keys are peer-stripped.
    let mut packages = HashMap::new();
    packages.insert(dep_key("b", "1.0.0"), metadata_stub());
    packages.insert(dep_key("peer", "2.0.0"), metadata_stub());

    // `snapshots:` keys carry the peer suffix.
    let mut b_deps = HashMap::new();
    b_deps.insert(pkg_name("peer"), SnapshotDepRef::Plain(ver_peer("2.0.0")));
    let mut snapshots = HashMap::new();
    snapshots.insert(
        dep_key("b", "1.0.0(peer@2.0.0)"),
        SnapshotEntry { dependencies: Some(b_deps), ..SnapshotEntry::default() },
    );
    snapshots.insert(dep_key("peer", "2.0.0"), SnapshotEntry::default());

    let lockfile = lockfile_with(root_deps, packages, snapshots);
    let lockfile_dir = PathBuf::from("/repo");
    let opts = LockfileToHoistedDepGraphOptions {
        lockfile_dir: lockfile_dir.clone(),
        ..LockfileToHoistedDepGraphOptions::default()
    };
    let result = lockfile_to_hoisted_dep_graph(&lockfile, None, &opts).expect("walker succeeds");

    let b_dir = lockfile_dir.join("node_modules").join("b");
    assert!(
        result.graph.contains_key(&b_dir),
        "peered node must be in the graph: {:?}",
        result.graph.keys().collect::<Vec<_>>(),
    );
    assert_eq!(
        result.hoisted_locations["b@1.0.0(peer@2.0.0)"],
        vec!["node_modules/b".to_string()],
        "the peered depPath must keep its full-suffix key in hoistedLocations",
    );
    // The peer itself is a plain dep of the root and must be there too.
    assert!(result.graph.contains_key(&lockfile_dir.join("node_modules").join("peer")));
}
/// The hoister collapses every peer variant of one package version onto
/// a single node keyed by the first snapshot key it sees, so the graph
/// holds no entry under the other variants' keys. Edges declared against
/// one of those — an importer's own dependency, or a sibling package's —
/// still have to resolve to the copy that node produced.
#[test]
fn walker_wires_edges_declared_against_a_collapsed_peer_variant() {
    let mut root_deps = ResolvedDependencyMap::new();
    root_deps.insert(pkg_name("b"), resolved_dep("1.0.0(peer@2.0.0)"));
    root_deps.insert(pkg_name("peer"), resolved_dep("2.0.0"));
    root_deps.insert(pkg_name("c"), resolved_dep("1.0.0"));

    let mut foo_deps = ResolvedDependencyMap::new();
    foo_deps.insert(pkg_name("b"), resolved_dep("1.0.0(peer@3.0.0)"));

    let mut packages = HashMap::new();
    packages.insert(dep_key("b", "1.0.0"), metadata_stub());
    packages.insert(dep_key("c", "1.0.0"), metadata_stub());
    packages.insert(dep_key("peer", "2.0.0"), metadata_stub());
    packages.insert(dep_key("peer", "3.0.0"), metadata_stub());

    let mut snapshots = HashMap::new();
    for peer_version in ["2.0.0", "3.0.0"] {
        let mut b_deps = HashMap::new();
        b_deps.insert(pkg_name("peer"), SnapshotDepRef::Plain(ver_peer(peer_version)));
        snapshots.insert(
            dep_key("b", &format!("1.0.0(peer@{peer_version})")),
            SnapshotEntry { dependencies: Some(b_deps), ..SnapshotEntry::default() },
        );
        snapshots.insert(dep_key("peer", peer_version), SnapshotEntry::default());
    }
    // `c` depends on the variant the root importer does not declare.
    let mut c_deps = HashMap::new();
    c_deps.insert(pkg_name("b"), SnapshotDepRef::Plain(ver_peer("1.0.0(peer@3.0.0)")));
    snapshots.insert(
        dep_key("c", "1.0.0"),
        SnapshotEntry { dependencies: Some(c_deps), ..SnapshotEntry::default() },
    );

    let lockfile = workspace_lockfile(
        vec![(Lockfile::ROOT_IMPORTER_KEY, root_deps), ("packages/foo", foo_deps)],
        packages,
        snapshots,
    );
    let lockfile_dir = PathBuf::from("/repo");
    let opts = LockfileToHoistedDepGraphOptions {
        lockfile_dir: lockfile_dir.clone(),
        ..LockfileToHoistedDepGraphOptions::default()
    };
    let result = lockfile_to_hoisted_dep_graph(&lockfile, None, &opts).expect("walker succeeds");

    let modules = lockfile_dir.join("node_modules");
    assert_eq!(
        result.direct_dependencies_by_importer_id["packages/foo"].get("b"),
        Some(&modules.join("b")),
        "the importer that declared the collapsed variant keeps its direct dependency",
    );
    assert_eq!(
        result.graph[&modules.join("c")].children.get("b"),
        Some(&modules.join("b")),
        "a snapshot edge on the collapsed variant resolves to the surviving copy",
    );
}
/// Peer variants of an injected directory dependency are exempt from
/// the collapse (see [`pnpm_real_hoist::pkg_id`]), so the walk has to
/// keep a location — and a direct-dependency entry — per variant,
/// where every collapsed package funnels into one.
#[test]
fn walker_keeps_file_dep_peer_variants_apart() {
    let mut r1_deps = ResolvedDependencyMap::new();
    r1_deps.insert(
        pkg_name("comp"),
        ResolvedDependencySpec {
            specifier: "workspace:*".to_string(),
            version: ver_peer("file:comp(peer@1.0.0)").into(),
        },
    );
    r1_deps.insert(pkg_name("peer"), resolved_dep("1.0.0"));
    let mut r2_deps = ResolvedDependencyMap::new();
    r2_deps.insert(
        pkg_name("comp"),
        ResolvedDependencySpec {
            specifier: "workspace:*".to_string(),
            version: ver_peer("file:comp(peer@2.0.0)").into(),
        },
    );
    r2_deps.insert(pkg_name("peer"), resolved_dep("2.0.0"));

    let mut packages = HashMap::new();
    packages.insert(
        dep_key("comp", "file:comp"),
        PackageMetadata { resolution: directory_resolution("comp"), ..metadata_stub() },
    );
    packages.insert(dep_key("peer", "1.0.0"), metadata_stub());
    packages.insert(dep_key("peer", "2.0.0"), metadata_stub());

    let mut snapshots = HashMap::new();
    for peer_version in ["1.0.0", "2.0.0"] {
        let mut comp_deps = HashMap::new();
        comp_deps.insert(pkg_name("peer"), SnapshotDepRef::Plain(ver_peer(peer_version)));
        snapshots.insert(
            dep_key("comp", &format!("file:comp(peer@{peer_version})")),
            SnapshotEntry { dependencies: Some(comp_deps), ..SnapshotEntry::default() },
        );
        snapshots.insert(dep_key("peer", peer_version), SnapshotEntry::default());
    }

    let lockfile = workspace_lockfile(
        vec![
            (Lockfile::ROOT_IMPORTER_KEY, ResolvedDependencyMap::new()),
            ("node_modules/.bit_roots/r1", r1_deps),
            ("node_modules/.bit_roots/r2", r2_deps),
        ],
        packages,
        snapshots,
    );
    let opts = LockfileToHoistedDepGraphOptions {
        lockfile_dir: PathBuf::from("/repo"),
        ..LockfileToHoistedDepGraphOptions::default()
    };
    let result = lockfile_to_hoisted_dep_graph(&lockfile, None, &opts).expect("walker succeeds");

    let r1_comp = result.direct_dependencies_by_importer_id["node_modules/.bit_roots/r1"]
        .get("comp")
        .expect("r1 keeps its comp direct dependency")
        .clone();
    let r2_comp = result.direct_dependencies_by_importer_id["node_modules/.bit_roots/r2"]
        .get("comp")
        .expect("r2 keeps its comp direct dependency")
        .clone();
    assert_ne!(
        r1_comp, r2_comp,
        "each importer's direct dependency must be its own variant's copy",
    );
    let r1_peer = result.graph[&r1_comp].children.get("peer").expect("r1's copy resolves peer");
    let r2_peer = result.graph[&r2_comp].children.get("peer").expect("r2's copy resolves peer");
    assert_eq!(
        result.graph[r1_peer].dep_path,
        DepPath::from("peer@1.0.0".to_string()),
        "r1's copy must resolve the peer version r1 pinned",
    );
    assert_eq!(
        result.graph[r2_peer].dep_path,
        DepPath::from("peer@2.0.0".to_string()),
        "r2's copy must resolve the peer version r2 pinned",
    );
}
