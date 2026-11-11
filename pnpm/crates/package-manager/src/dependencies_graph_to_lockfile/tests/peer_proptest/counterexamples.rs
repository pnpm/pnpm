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
    let settings = super::model::Settings {
        peers_suffix_max_length: 20,
        auto_install_peers: false,
        ..Default::default()
    };
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

#[test]
fn incremental_install_preserves_a_valid_optional_peer_pin() {
    let mut workspace = Workspace {
        packages: vec![
            vec![
                depending_on(1, 2),
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 1,
                        range: PeerRange::Any,
                        optional: true,
                        also_dependency: false,
                    }],
                },
            ],
            vec![leaf(), leaf()],
        ],
        importers: vec![direct(1, 1), direct(0, 2)],
    };
    let settings = super::Settings { auto_install_peers: false, ..super::Settings::default() };
    let order = in_order(&workspace);
    let initial = super::resolve_with(&workspace, &order, settings, Reuse::Fresh).unwrap();
    let lockfile = super::lockfile_with_settings(&initial, &workspace, settings);
    workspace.importers[0].push(Edge { target: 0, major: 1 });
    let installed =
        super::resolve_with(&workspace, &order, settings, Reuse::Install(&lockfile)).unwrap();
    let fresh = super::resolve_with(&workspace, &order, settings, Reuse::Fresh).unwrap();
    assert_eq!(
        installed.peers.direct_dependencies_by_importer["packages/a"]["p0"].as_str(),
        "p0@2.0.0(p1@1.0.0)",
    );
    assert_eq!(
        fresh.peers.direct_dependencies_by_importer["packages/a"]["p0"].as_str(),
        "p0@2.0.0(p1@2.0.0)",
    );
}

#[test]
fn frozen_check_accepts_the_injected_childrens_owner_peer_split() {
    for optional in [false, true] {
        for provider_major in [1, 2] {
            let workspace = Workspace {
                packages: vec![
                    vec![
                        PackageVersion {
                            dependencies: direct(1, 1),
                            peers: vec![peer(1, provider_major, optional, false)],
                        },
                        leaf(),
                    ],
                    vec![leaf(), leaf()],
                ],
                importers: vec![direct(1, provider_major), direct(0, 1)],
            };
            for peers_suffix_max_length in [20, 1000] {
                let settings = super::Settings {
                    auto_install_peers: false,
                    injected: true,
                    format: pnpm_lockfile::LockfileFormat::V13,
                    peers_suffix_max_length,
                    ..Default::default()
                };
                let resolved =
                    super::resolve_with(&workspace, &in_order(&workspace), settings, Reuse::Fresh)
                        .unwrap();
                let lockfile = super::lockfile_with_settings(&resolved, &workspace, settings);
                let fixture = super::fixture::Fixture::new(&workspace, settings);
                fixture.frozen_check(&lockfile, settings).unwrap();
            }
        }
    }
}

#[test]
fn injected_cycle_is_independent_of_importer_order() {
    let any_peer = |target, optional, also_dependency| Peer {
        target,
        range: PeerRange::Any,
        optional,
        also_dependency,
    };
    let workspace = Workspace {
        packages: vec![
            vec![
                PackageVersion {
                    dependencies: direct(1, 1),
                    peers: vec![any_peer(3, false, false)],
                },
                PackageVersion {
                    dependencies: direct(3, 2),
                    peers: vec![peer(1, 2, true, false), any_peer(3, true, true)],
                },
            ],
            vec![peer_of(2, 2), leaf()],
            vec![
                PackageVersion { dependencies: direct(1, 2), peers: vec![peer(4, 1, true, false)] },
                PackageVersion { dependencies: vec![], peers: vec![any_peer(4, false, false)] },
            ],
            vec![
                leaf(),
                PackageVersion {
                    dependencies: vec![Edge { target: 2, major: 2 }, Edge { target: 1, major: 1 }],
                    peers: vec![any_peer(4, false, false)],
                },
            ],
            vec![
                leaf(),
                PackageVersion {
                    dependencies: direct(2, 1),
                    peers: vec![peer(2, 1, false, false)],
                },
            ],
        ],
        importers: vec![
            vec![Edge { target: 4, major: 2 }, Edge { target: 0, major: 2 }],
            vec![Edge { target: 1, major: 1 }, Edge { target: 2, major: 1 }],
        ],
    };
    let settings = super::Settings { injected: true, ..Default::default() };
    let first = super::resolve_with(&workspace, &[0, 1], settings, Reuse::Fresh).unwrap();
    let expected = outcome_of(&first, &workspace);
    let lockfile = super::lockfile_with_settings(&first, &workspace, settings);
    for order in [[0, 1], [1, 0]] {
        for reuse in [Reuse::Fresh, Reuse::Install(&lockfile)] {
            let result = super::resolve_with(&workspace, &order, settings, reuse).unwrap();
            assert_eq!(expected, outcome_of(&result, &workspace));
        }
    }
}

#[test]
fn v13_round_trip_preserves_missing_peer_in_dependency_cycle() {
    let workspace = Workspace {
        packages: vec![
            vec![
                PackageVersion {
                    dependencies: direct(2, 1),
                    peers: vec![peer(1, 2, false, false)],
                },
                leaf(),
            ],
            vec![leaf(), leaf()],
            vec![PackageVersion { dependencies: direct(0, 1), peers: vec![] }, leaf()],
        ],
        importers: vec![direct(2, 1)],
    };
    let settings = super::Settings {
        auto_install_peers: false,
        format: pnpm_lockfile::LockfileFormat::V13,
        ..Default::default()
    };
    let result = super::resolve_with(&workspace, &[0], settings, Reuse::Fresh).unwrap();
    let lockfile = super::lockfile_with_settings(&result, &workspace, settings);
    assert_eq!(
        lockfile.snapshots.as_ref().unwrap()[&"p2@1.0.0".parse().unwrap()]
            .transitive_peer_dependencies,
        Some(vec!["p1".to_string()]),
    );
}

#[test]
fn v13_round_trip_preserves_optional_peer_dependency_cycle() {
    let workspace = Workspace {
        packages: vec![
            vec![
                PackageVersion { dependencies: vec![], peers: vec![peer(2, 1, true, true)] },
                leaf(),
            ],
            vec![
                leaf(),
                PackageVersion {
                    dependencies: vec![Edge { target: 2, major: 1 }, Edge { target: 0, major: 1 }],
                    peers: vec![],
                },
            ],
            vec![
                PackageVersion {
                    dependencies: direct(1, 2),
                    peers: vec![peer(0, 1, false, false)],
                },
                leaf(),
            ],
        ],
        importers: vec![direct(0, 1)],
    };
    let settings = super::Settings {
        auto_install_peers: false,
        format: pnpm_lockfile::LockfileFormat::V13,
        ..Default::default()
    };
    let result = super::resolve_with(&workspace, &[0], settings, Reuse::Fresh).unwrap();
    super::lockfile_with_settings(&result, &workspace, settings);
}

#[test]
fn frozen_check_accepts_injected_catalog_optional_peer_dependency() {
    let workspace = Workspace {
        packages: vec![
            vec![
                PackageVersion { dependencies: vec![], peers: vec![peer(1, 1, true, true)] },
                leaf(),
            ],
            vec![leaf(), leaf()],
        ],
        importers: vec![direct(1, 1), direct(0, 1)],
    };
    let settings = super::Settings {
        auto_install_peers: false,
        catalogs: true,
        injected: true,
        format: pnpm_lockfile::LockfileFormat::V13,
        ..Default::default()
    };
    let result =
        super::resolve_with(&workspace, &in_order(&workspace), settings, Reuse::Fresh).unwrap();
    let lockfile = super::lockfile_with_settings(&result, &workspace, settings);
    super::fixture::Fixture::new(&workspace, settings).frozen_check(&lockfile, settings).unwrap();
}

#[test]
fn v13_round_trip_preserves_injected_peer_provider_variants() {
    let workspace = Workspace {
        packages: vec![
            vec![peer_of(1, 1), leaf()],
            vec![
                leaf(),
                PackageVersion { dependencies: vec![], peers: vec![peer(0, 1, true, false)] },
            ],
        ],
        importers: vec![
            direct(0, 1),
            vec![Edge { target: 0, major: 1 }, Edge { target: 1, major: 2 }],
            direct(1, 2),
        ],
    };
    let settings = super::Settings {
        injected: true,
        format: pnpm_lockfile::LockfileFormat::V13,
        ..Default::default()
    };
    let result =
        super::resolve_with(&workspace, &in_order(&workspace), settings, Reuse::Fresh).unwrap();
    super::lockfile_with_settings(&result, &workspace, settings);
}

#[test]
fn v13_round_trip_preserves_hashed_catalog_injected_peers() {
    let workspace = Workspace {
        packages: vec![
            vec![PackageVersion { dependencies: direct(2, 1), peers: vec![] }, peer_of(1, 2)],
            vec![PackageVersion { dependencies: direct(0, 1), peers: vec![] }, leaf()],
            vec![PackageVersion { dependencies: direct(0, 2), peers: vec![] }, peer_of(0, 1)],
        ],
        importers: vec![
            vec![Edge { target: 1, major: 1 }, Edge { target: 2, major: 2 }],
            direct(2, 2),
        ],
    };
    let settings = super::Settings {
        peers_suffix_max_length: 20,
        catalogs: true,
        injected: true,
        format: pnpm_lockfile::LockfileFormat::V13,
        ..Default::default()
    };
    let result =
        super::resolve_with(&workspace, &in_order(&workspace), settings, Reuse::Fresh).unwrap();
    super::lockfile_with_settings(&result, &workspace, settings);
}

#[test]
fn v13_round_trip_preserves_auto_installed_injected_peer_cycle() {
    let workspace = Workspace {
        packages: vec![
            vec![peer_of(2, 2), leaf()],
            vec![
                PackageVersion { dependencies: direct(2, 2), peers: vec![] },
                PackageVersion { dependencies: direct(2, 1), peers: vec![] },
            ],
            vec![
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 0,
                        range: PeerRange::Any,
                        optional: false,
                        also_dependency: false,
                    }],
                },
                peer_of(0, 1),
            ],
        ],
        importers: vec![
            vec![Edge { target: 2, major: 1 }, Edge { target: 1, major: 1 }],
            direct(0, 2),
            direct(1, 2),
        ],
    };
    let settings = super::Settings {
        injected: true,
        format: pnpm_lockfile::LockfileFormat::V13,
        ..Default::default()
    };
    let result =
        super::resolve_with(&workspace, &in_order(&workspace), settings, Reuse::Fresh).unwrap();
    super::lockfile_with_settings(&result, &workspace, settings);
}

#[test]
fn v13_round_trip_preserves_injected_transitive_peer_cycle() {
    let workspace = Workspace {
        packages: vec![
            vec![
                PackageVersion {
                    dependencies: direct(1, 1),
                    peers: vec![peer(2, 2, false, false)],
                },
                leaf(),
            ],
            vec![depending_on(2, 2), leaf()],
            vec![leaf(), peer_of(0, 1)],
        ],
        importers: vec![direct(2, 1), direct(0, 2), direct(1, 1)],
    };
    let settings = super::Settings {
        injected: true,
        format: pnpm_lockfile::LockfileFormat::V13,
        ..Default::default()
    };
    let result =
        super::resolve_with(&workspace, &in_order(&workspace), settings, Reuse::Fresh).unwrap();
    super::lockfile_with_settings(&result, &workspace, settings);
}

#[test]
fn v13_round_trip_preserves_hashed_auto_installed_peer_diamond() {
    let workspace = Workspace {
        packages: vec![
            vec![peer_of(1, 1), leaf()],
            vec![leaf(), leaf()],
            vec![
                leaf(),
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![peer(3, 1, false, false), peer(0, 1, true, true)],
                },
            ],
            vec![peer_of(1, 1), leaf()],
        ],
        importers: vec![direct(2, 2)],
    };
    let settings = super::Settings {
        peers_suffix_max_length: 20,
        injected: true,
        format: pnpm_lockfile::LockfileFormat::V13,
        ..Default::default()
    };
    let result =
        super::resolve_with(&workspace, &in_order(&workspace), settings, Reuse::Fresh).unwrap();
    super::lockfile_with_settings(&result, &workspace, settings);
}
