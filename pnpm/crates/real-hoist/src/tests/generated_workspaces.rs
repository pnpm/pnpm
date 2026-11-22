//! Generated lockfiles on which upstream's hoister places a package where
//! one of its dependencies resolves to another package version. pnpm v11
//! runs that hoister and does the same on each of them.

use super::{
    HoistOpts,
    Lockfile,
    determinism::lockfile_yaml,
    resolution::edges_to_another_package,
};
use crate::{
    hoist_until_peers_resolve,
    tree,
};
use std::path::Path;

/// Upstream reports `DEPENDS` for a node whose hoisted dependency does not
/// resolve from the root, and moves it once its peer provider has moved.
const WAITING_ON_A_PEER_PROVIDER: [u64; 11] =
    [114, 249, 601, 932, 1206, 1426, 1863, 2360, 2512, 2577, 2671];

/// Upstream moves a node away from a dependency that was hoisted out of a
/// node nested in it.
const HOISTED_OUT_OF_A_NESTED_NODE: [u64; 18] = [
    399, 426, 513, 618, 643, 942, 997, 1126, 1356, 1516, 1713, 1950, 2058, 2102, 2359, 2713, 2772,
    2931,
];

/// The edges of workspace `number` that resolve to another package.
fn edges_to_another_package_in(number: u64) -> Vec<String> {
    let yaml = lockfile_yaml(number * 7919 + 13);
    let lockfile = Lockfile::parse(&yaml, Path::new("pnpm-lock.yaml"))
        .expect("parse the lockfile")
        .expect("the lockfile is not empty");
    // Not through `hoist`, whose resolution oracle also rejects a package
    // version reached through another peer variant.
    let (input, result) =
        hoist_until_peers_resolve(&lockfile, &HoistOpts::default()).expect("hoist should succeed");
    let edges = edges_to_another_package(&input, &result);
    tree::tear_down(&input);
    edges
}

#[test]
fn a_node_waiting_on_a_peer_provider_keeps_its_dependencies() {
    for number in WAITING_ON_A_PEER_PROVIDER {
        let edges = edges_to_another_package_in(number);
        assert!(edges.is_empty(), "workspace {number}:\n{}", edges.join("\n"));
    }
}

#[test]
fn a_node_keeps_the_dependencies_hoisted_out_of_the_nodes_nested_in_it() {
    for number in HOISTED_OUT_OF_A_NESTED_NODE {
        let edges = edges_to_another_package_in(number);
        assert!(edges.is_empty(), "workspace {number}:\n{}", edges.join("\n"));
    }
}
