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

fn package_nodes(lockfile: &Lockfile, skip_peer_edges: bool) -> BTreeSet<String> {
    let graph = build_dependency_graph(
        &[TreeNodeId::Importer(".".to_string())],
        &BuildGraphOptions {
            lockfile,
            include: IncludedDependencies {
                dependencies: true,
                dev_dependencies: true,
                optional_dependencies: true,
            },
            only_projects: false,
            peer_edges: PeerEdgeOptions::default(),
            skip_peer_edges,
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
fn skipping_peer_edges_leaves_out_packages_reached_only_as_a_peer() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile = load_peer_lockfile(dir.path(), &["parent"]);

    assert_eq!(
        package_nodes(&lockfile, false),
        BTreeSet::from([
            "parent@1.0.0(peer@1.0.0)".to_string(),
            "peer@1.0.0".to_string(),
            "peer-dep@1.0.0".to_string(),
        ]),
    );
    assert_eq!(
        package_nodes(&lockfile, true),
        BTreeSet::from(["parent@1.0.0(peer@1.0.0)".to_string()]),
    );
}

#[test]
fn skipping_peer_edges_keeps_packages_that_are_also_reached_directly() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile = load_peer_lockfile(dir.path(), &["parent", "peer"]);

    assert_eq!(
        package_nodes(&lockfile, true),
        BTreeSet::from([
            "parent@1.0.0(peer@1.0.0)".to_string(),
            "peer@1.0.0".to_string(),
            "peer-dep@1.0.0".to_string(),
        ]),
    );
}
