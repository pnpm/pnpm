use std::{
    collections::BTreeSet,
    fmt::Write,
};

use pnpm_lockfile::{
    Lockfile,
    PeerEdgeOptions,
};
use pnpm_modules_yaml::IncludedDependencies;
use pretty_assertions::assert_eq;

use super::{
    BoundPeerEdges,
    BuildGraphOptions,
    build_dependency_graph,
};
use crate::TreeNodeId;

/// `parent` declares `peer` as a peer dependency, and `peer` has a regular
/// dependency of its own. `root_dependencies` lists what the root project
/// depends on directly.
fn load_peer_lockfile(dir: &std::path::Path, root_dependencies: &[&str]) -> Lockfile {
    let mut yaml =
        String::from("lockfileVersion: '9.0'\n\nimporters:\n\n  .:\n    dependencies:\n");
    for name in root_dependencies {
        let version = if *name == "parent" { "1.0.0(peer@1.0.0)" } else { "1.0.0" };
        writeln!(yaml, "      {name}:\n        specifier: 1.0.0\n        version: {version}")
            .unwrap();
    }
    yaml.push_str(
        "
packages:

  parent@1.0.0:
    resolution: {integrity: sha512-parent}
    peerDependencies:
      peer: ^1.0.0

  peer@1.0.0:
    resolution: {integrity: sha512-peer}

  peer-dep@1.0.0:
    resolution: {integrity: sha512-peer-dep}

snapshots:

  parent@1.0.0(peer@1.0.0):
    dependencies:
      peer: 1.0.0

  peer@1.0.0:
    dependencies:
      peer-dep: 1.0.0

  peer-dep@1.0.0: {}
",
    );
    std::fs::write(dir.join("pnpm-lock.yaml"), yaml).unwrap();
    Lockfile::load_wanted_from_dir(dir).unwrap().unwrap()
}

fn package_nodes(lockfile: &Lockfile, bound_peer_edges: BoundPeerEdges) -> BTreeSet<String> {
    let graph = build_dependency_graph(
        vec![TreeNodeId::Importer(".".to_string())],
        &BuildGraphOptions {
            lockfile,
            include: IncludedDependencies {
                dependencies: true,
                dev_dependencies: true,
                optional_dependencies: true,
            },
            only_projects: false,
            peer_edges: PeerEdgeOptions::default(),
            bound_peer_edges,
        },
    );
    graph.nodes
        .keys()
        .filter_map(|node_id| match node_id {
            TreeNodeId::Package(dep_path) => Some(dep_path.to_string()),
            TreeNodeId::Importer(_) => None,
        })
        .collect()
}

#[test]
fn skipping_bound_peer_edges_leaves_out_packages_reached_only_as_a_peer() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile = load_peer_lockfile(dir.path(), &["parent"]);

    let followed = package_nodes(&lockfile, BoundPeerEdges::Follow);
    let skipped = package_nodes(&lockfile, BoundPeerEdges::Skip);
    dbg!(&followed, &skipped);
    assert_eq!(
        followed,
        BTreeSet::from([
            "parent@1.0.0(peer@1.0.0)".to_string(),
            "peer@1.0.0".to_string(),
            "peer-dep@1.0.0".to_string(),
        ]),
    );
    assert_eq!(skipped, BTreeSet::from(["parent@1.0.0(peer@1.0.0)".to_string()]));
}

#[test]
fn skipping_bound_peer_edges_keeps_packages_that_are_also_reached_directly() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile = load_peer_lockfile(dir.path(), &["parent", "peer"]);

    let skipped = package_nodes(&lockfile, BoundPeerEdges::Skip);
    dbg!(&skipped);
    assert_eq!(
        skipped,
        BTreeSet::from([
            "parent@1.0.0(peer@1.0.0)".to_string(),
            "peer@1.0.0".to_string(),
            "peer-dep@1.0.0".to_string(),
        ]),
    );
}

/// `wrapper` declares `mid` in both `dependencies` and `peerDependencies`,
/// and its key carries no `(mid@1.0.0)` binding, so `mid` is its own
/// dependency rather than a resolved peer.
#[test]
fn skipping_bound_peer_edges_keeps_an_own_dependency_declared_as_a_peer() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      wrapper:
        specifier: 1.0.0
        version: 1.0.0

packages:

  wrapper@1.0.0:
    resolution: {integrity: sha512-wrapper}
    peerDependencies:
      mid: ^1.0.0

  mid@1.0.0:
    resolution: {integrity: sha512-mid}

snapshots:

  wrapper@1.0.0:
    dependencies:
      mid: 1.0.0

  mid@1.0.0: {}
",
    )
    .unwrap();
    let lockfile = Lockfile::load_wanted_from_dir(dir.path()).unwrap().unwrap();

    let skipped = package_nodes(&lockfile, BoundPeerEdges::Skip);
    dbg!(&skipped);
    assert_eq!(skipped, BTreeSet::from(["mid@1.0.0".to_string(), "wrapper@1.0.0".to_string()]));
}

/// `wrapper`'s key has no peer suffix, so its `link:` dependency on `mid`
/// is its own even though it also declares `mid` as a peer.
#[test]
fn skipping_bound_peer_edges_keeps_a_linked_own_dependency_of_an_unsuffixed_key() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      wrapper:
        specifier: 1.0.0
        version: 1.0.0

packages:

  wrapper@1.0.0:
    resolution: {integrity: sha512-wrapper}
    peerDependencies:
      mid: ^1.0.0

snapshots:

  wrapper@1.0.0:
    dependencies:
      mid: link:../mid
",
    )
    .unwrap();
    let lockfile = Lockfile::load_wanted_from_dir(dir.path()).unwrap().unwrap();
    let graph = build_dependency_graph(
        vec![TreeNodeId::Importer(".".to_string())],
        &BuildGraphOptions {
            lockfile: &lockfile,
            include: IncludedDependencies {
                dependencies: true,
                dev_dependencies: true,
                optional_dependencies: true,
            },
            only_projects: false,
            peer_edges: PeerEdgeOptions::default(),
            bound_peer_edges: BoundPeerEdges::Skip,
        },
    );

    let wrapper = &graph.nodes[&TreeNodeId::Package("wrapper@1.0.0".parse().unwrap())];
    let aliases: Vec<&str> = wrapper.edges
        .iter()
        .map(|edge| edge.alias.as_str())
        .collect();
    dbg!(&aliases);
    assert_eq!(aliases, ["mid"]);
}
