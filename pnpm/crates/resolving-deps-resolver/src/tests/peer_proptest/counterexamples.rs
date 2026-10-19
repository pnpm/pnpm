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
/// peer `p1@^2`. `.` depends on `p1@^1`, `packages/a` on `p2@^2`.
/// Resolving over the fresh lockfile keeps `p0@2.0.0(p1@2.0.0)`.
#[test]
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

/// `p0@1` and `p0@2` depend on `p3@^2`, `p1@2` on `p2@^1`, `p2@1` on
/// `p3@^2`. `p3@1` has the peer `p0@^2`, and `p3@2` depends on `p0@^1` and
/// has it as an optional peer too. `.` depends on `p1@^2`, `packages/a` on
/// `p3@^1`, `packages/b` on `p0@^1`.
#[test]
fn a_second_dedupe_keeps_the_first_ones_peer_bindings() {
    let workspace = Workspace {
        packages: vec![
            vec![depending_on(3, 2), depending_on(3, 2)],
            vec![leaf(), depending_on(2, 1)],
            vec![depending_on(3, 2), leaf()],
            vec![
                peer_of(0, 2),
                PackageVersion {
                    dependencies: vec![Edge { target: 0, major: 1 }],
                    peers: vec![peer(0, 1, true, false)],
                },
            ],
        ],
        importers: vec![direct(1, 2), direct(3, 1), direct(0, 1)],
    };
    let order = in_order(&workspace);
    let fresh = resolve(&workspace, &order, Reuse::Fresh).expect("fresh resolve");
    let first = resolve(&workspace, &order, Reuse::Dedupe(&lockfile_of(&fresh, &workspace)))
        .expect("first dedupe");
    let second = resolve(&workspace, &order, Reuse::Dedupe(&lockfile_of(&first, &workspace)))
        .expect("second dedupe");
    assert_eq!(outcome_of(&first, &workspace), outcome_of(&second, &workspace));
}

fn version(dependencies: Vec<Edge>, peers: Vec<Peer>) -> PackageVersion {
    PackageVersion { dependencies, peers }
}

fn assert_install_over_own_lockfile_matches_fresh(workspace: &Workspace) {
    let order = in_order(workspace);
    let fresh = resolve(workspace, &order, Reuse::Fresh).expect("fresh resolve");
    let lockfile = lockfile_of(&fresh, workspace);
    let reused = resolve(workspace, &order, Reuse::Install(&lockfile)).expect("reuse resolve");
    assert_eq!(outcome_of(&fresh, workspace), outcome_of(&reused, workspace));
}

/// `p0@1` has the optional peer `p1@^1`, also a dependency, `p0@2` depends
/// on `p1@^1`, and `p1@1` on `p0@^1`, a cycle through an optional peer. `.`
/// depends on `p0@^2`.
#[test]
fn install_over_own_lockfile_keeps_an_optional_peer_on_a_cycle() {
    assert_install_over_own_lockfile_matches_fresh(&Workspace {
        packages: vec![
            vec![version(Vec::new(), vec![peer(1, 1, true, true)]), depending_on(1, 1)],
            vec![depending_on(0, 1), leaf()],
        ],
        importers: vec![direct(0, 2)],
    });
}

/// `p0@1` depends on `p1@^1`, and `p1@1` depends on `p0@^1` and has it as
/// an optional peer. `.` depends on `p1@^2`, whose `p1@2` depends on
/// `p0@^1`.
#[test]
fn install_over_own_lockfile_keeps_an_optional_peer_back_into_its_dependent() {
    assert_install_over_own_lockfile_matches_fresh(&Workspace {
        packages: vec![
            vec![depending_on(1, 1), leaf()],
            vec![
                version(vec![Edge { target: 0, major: 1 }], vec![peer(0, 1, true, false)]),
                depending_on(0, 1),
            ],
        ],
        importers: vec![direct(1, 2)],
    });
}

/// `p0@1` has the optional peers `p2@^1`, also a dependency, and `p1@^1`.
/// `p0@2` has the optional peer `p1` and the peer `p2@^1`. `p1@2` and
/// `p2@1` depend on `p0@^1`. `.` depends on `p0@^2`, `packages/a` on
/// `p1@^2`.
#[test]
fn a_second_dedupe_keeps_an_optional_peer_on_a_cycle() {
    let workspace = Workspace {
        packages: vec![
            vec![
                version(Vec::new(), vec![peer(2, 1, true, true), peer(1, 1, true, false)]),
                version(
                    Vec::new(),
                    vec![
                        Peer {
                            target: 1,
                            range: PeerRange::Any,
                            optional: true,
                            also_dependency: false,
                        },
                        peer(2, 1, false, false),
                    ],
                ),
            ],
            vec![leaf(), depending_on(0, 1)],
            vec![depending_on(0, 1), leaf()],
        ],
        importers: vec![direct(0, 2), direct(1, 2)],
    };
    let order = in_order(&workspace);
    let fresh = resolve(&workspace, &order, Reuse::Fresh).expect("fresh resolve");
    let first = resolve(&workspace, &order, Reuse::Dedupe(&lockfile_of(&fresh, &workspace)))
        .expect("first dedupe");
    let second = resolve(&workspace, &order, Reuse::Dedupe(&lockfile_of(&first, &workspace)))
        .expect("second dedupe");
    assert_eq!(outcome_of(&first, &workspace), outcome_of(&second, &workspace));
}

/// `p0@1` has required peer `p1@^1`, optional peer `p2@^1`, and peer `p3@^2`
/// that is also an own dependency (`p3@^2`). With `peers_suffix_max_length: 20`
/// and `auto_install_peers: false`, `p0@1`'s suffix `(p1@1.0.0)(p2@1.0.0)` is
/// hashed while `p3@2.0.0` remains an unshadowed own dependency. Re-resolving
/// over the lockfile must recover the hashed bindings rather than treating
/// `p3` as a bound peer.
#[test]
fn hashed_suffix_keeps_unshadowed_own_dependency_without_auto_install_peers() {
    let workspace = Workspace {
        packages: vec![
            vec![
                version(
                    Vec::new(),
                    vec![
                        peer(1, 1, false, false),
                        peer(2, 1, true, false),
                        peer(3, 2, false, true),
                    ],
                ),
                leaf(),
            ],
            vec![leaf(), leaf()],
            vec![leaf(), leaf()],
            vec![leaf(), leaf()],
        ],
        importers: vec![vec![
            Edge { target: 0, major: 1 },
            Edge { target: 1, major: 1 },
            Edge { target: 2, major: 1 },
        ]],
    };
    let settings =
        super::model::Settings { peers_suffix_max_length: 20, auto_install_peers: false };
    let order = in_order(&workspace);
    let fresh =
        super::resolve_with(&workspace, &order, settings, Reuse::Fresh).expect("fresh resolve");
    let lockfile = lockfile_of(&fresh, &workspace);
    let reused = super::resolve_with(&workspace, &order, settings, Reuse::Install(&lockfile))
        .expect("reuse resolve");
    assert_eq!(outcome_of(&fresh, &workspace), outcome_of(&reused, &workspace));

    let (initial_ws, moved) = super::model::move_dependency(&workspace, 0, 1);
    let initial =
        super::resolve_with(&initial_ws, &order, settings, Reuse::Fresh).expect("initial resolve");
    let moved_lockfile = lockfile_of(&initial, &initial_ws);
    let moved_fresh = super::outcome_with(&moved, &order, settings, Reuse::Fresh);
    let moved_installed =
        super::outcome_with(&moved, &order, settings, Reuse::Install(&moved_lockfile));
    let moved_deduped =
        super::outcome_with(&moved, &order, settings, Reuse::Dedupe(&moved_lockfile));
    assert_eq!(moved_fresh, moved_installed);
    assert_eq!(moved_fresh, moved_deduped);
}
