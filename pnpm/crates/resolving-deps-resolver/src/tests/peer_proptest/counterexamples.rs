//! Minimized counterexamples the properties in the parent module found.

use super::{
    Reuse, Workspace, in_order, lockfile_of,
    model::{Edge, PackageVersion, Peer, PeerRange},
    resolve,
};
use pretty_assertions::assert_eq;

fn leaf() -> PackageVersion {
    PackageVersion { dependencies: Vec::new(), peers: Vec::new() }
}

fn depending_on(target: usize, major: u8) -> PackageVersion {
    PackageVersion { dependencies: vec![Edge { target, major }], peers: Vec::new() }
}

fn peer_of(target: usize, major: u8) -> PackageVersion {
    PackageVersion {
        dependencies: Vec::new(),
        peers: vec![Peer {
            target,
            range: PeerRange::Major(major),
            optional: false,
            also_dependency: false,
        }],
    }
}

fn direct(target: usize, major: u8) -> Vec<Edge> {
    vec![Edge { target, major }]
}

/// `p0@2` depends on `p2@^1`, and `p2@1` has the peer `p0@^1`.
/// `.` and `packages/b` depend on `p2@^1`, `packages/a` on `p0@^2`.
fn peer_borrowed_across_importers() -> Workspace {
    Workspace {
        packages: vec![
            vec![leaf(), depending_on(2, 1)],
            vec![leaf(), leaf()],
            vec![peer_of(0, 1), leaf()],
        ],
        importers: vec![direct(2, 1), direct(0, 2), direct(2, 1)],
    }
}

/// `packages/b` has no `p0` of its own, and `p2@1` wants `p0@^1`. A
/// fresh resolve still gives its `p2` the `p0@2.0.0` that only
/// `packages/a`'s `p0@2 -> p2` subtree provides, a version outside the
/// peer range, and reports no peer issue. Resolving over that lockfile
/// gives it the auto-installed `p0@1.0.0` that `.` uses, so the lockfile
/// changes on the second run. Every importer order resolves the same.
#[test]
#[ignore = "known divergence: a fresh resolve hands a peer-dependent the provider from another importer's subtree (pnpm/tasks#143)"]
fn fresh_resolve_does_not_borrow_a_peer_from_another_importers_subtree() {
    let workspace = peer_borrowed_across_importers();
    let order = in_order(&workspace);
    let fresh = resolve(&workspace, &order, Reuse::Fresh).expect("fresh resolve");
    let dep_path = fresh.peers.direct_dependencies_by_importer["packages/b"]["p2"].to_string();
    assert_eq!(dep_path, "p2@1.0.0(p0@1.0.0)");
}

/// Two importers both depend on `p0@^1`, whose peer `p1@^1` neither
/// declares. A fresh resolve auto-installs `p1` as a direct dependency of
/// `.` alone, while a resolve over that lockfile adds it to `packages/a`
/// too. The lockfile records neither entry, so the parent property
/// compares declared aliases only.
#[test]
#[ignore = "known divergence: auto-installed peers reach only the first importer on a fresh resolve (pnpm/tasks#143)"]
fn auto_installed_peer_reaches_the_same_importers_over_the_lockfile() {
    let workspace = Workspace {
        packages: vec![vec![peer_of(1, 1), leaf()], vec![leaf(), leaf()]],
        importers: vec![direct(0, 1), direct(0, 1)],
    };
    let order = in_order(&workspace);
    let fresh = resolve(&workspace, &order, Reuse::Fresh).expect("fresh resolve");
    let lockfile = lockfile_of(&fresh, &workspace);
    let reused = resolve(&workspace, &order, Reuse::Install(&lockfile)).expect("reuse resolve");
    assert_eq!(
        fresh.peers.direct_dependencies_by_importer,
        reused.peers.direct_dependencies_by_importer,
    );
}
