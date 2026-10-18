//! Minimized counterexamples the properties in the parent module found.

use super::{
    Reuse, Workspace, in_order, lockfile_of,
    model::{Edge, PackageVersion, Peer, PeerRange},
    outcome_of, resolve,
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

fn peer(target: usize, major: u8, optional: bool, also_dependency: bool) -> Peer {
    Peer { target, range: PeerRange::Major(major), optional, also_dependency }
}

/// `p0@2` has the optional peer `p1@^2`, also a dependency, and `p1@2`
/// depends on `p0@^2`, a peer cycle. `p2@2` depends on `p0@^2` and has the
/// peer `p1@^2`. `.` depends on `p1@^1`, `packages/a` on `p2@^2`. A
/// fresh resolve writes `p0@2.0.0(p1@2.0.0)`, and resolving over that
/// lockfile cuts the cycle at `p0@2.0.0`.
#[test]
#[ignore = "known divergence: the cycle cut differs between a fresh resolve and one over its lockfile (pnpm/tasks#143)"]
fn install_over_own_lockfile_keeps_a_cycle_members_peer_suffix() {
    let workspace = Workspace {
        packages: vec![
            vec![
                leaf(),
                PackageVersion { dependencies: Vec::new(), peers: vec![peer(1, 2, true, true)] },
            ],
            vec![
                PackageVersion { dependencies: Vec::new(), peers: vec![peer(0, 1, false, true)] },
                depending_on(0, 2),
            ],
            vec![
                leaf(),
                PackageVersion {
                    dependencies: vec![Edge { target: 0, major: 2 }],
                    peers: vec![peer(1, 2, false, false)],
                },
            ],
        ],
        importers: vec![direct(1, 1), direct(2, 2)],
    };
    let order = in_order(&workspace);
    let fresh = resolve(&workspace, &order, Reuse::Fresh).expect("fresh resolve");
    let lockfile = lockfile_of(&fresh, &workspace);
    let reused = resolve(&workspace, &order, Reuse::Install(&lockfile)).expect("reuse resolve");
    assert_eq!(outcome_of(&fresh, &workspace), outcome_of(&reused, &workspace));
}
