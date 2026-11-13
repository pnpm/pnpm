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
    let installed_lockfile = super::lockfile_with_settings(&installed, &workspace, settings);
    let reinstalled =
        super::resolve_with(&workspace, &order, settings, Reuse::Install(&installed_lockfile))
            .unwrap();
    assert_eq!(
        installed_lockfile,
        super::lockfile_with_settings(&reinstalled, &workspace, settings),
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
    assert_v13_round_trip(&result, &workspace, settings);
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
    assert_v13_round_trip(&result, &workspace, settings);
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
    let mut stale = lockfile.clone();
    for (key, snapshot) in stale.snapshots.as_mut().unwrap() {
        if key.name.to_string() == "shared"
            && let Some(reference) = snapshot.optional_dependencies
                .as_mut()
                .and_then(|dependencies| dependencies.get_mut(&"p1".parse().unwrap()))
        {
            *reference = "2.0.0".parse().unwrap();
        }
    }
    assert_ne!(stale.snapshots, lockfile.snapshots);
    assert!(
        super::fixture::Fixture::new(&workspace, settings).frozen_check(&stale, settings).is_err(),
    );

    let fixture = super::fixture::Fixture::new(&workspace, settings);
    let provider_dir = fixture.root.path().join("provider");
    std::fs::create_dir(&provider_dir).unwrap();
    for (key, snapshot) in stale.snapshots.as_mut().unwrap() {
        if key.name.to_string() == "shared"
            && let Some(reference) = snapshot.optional_dependencies
                .as_mut()
                .and_then(|dependencies| dependencies.get_mut(&"p1".parse().unwrap()))
        {
            *reference = "file:provider".parse().unwrap();
        }
    }
    for (version, fresh) in [("1.0.0", true), ("2.0.0", false)] {
        std::fs::write(
            provider_dir.join("package.json"),
            serde_json::json!({ "name": "p1", "version": version }).to_string(),
        )
        .unwrap();
        assert_eq!(fixture.frozen_check(&stale, settings).is_ok(), fresh, "{version}");
    }
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
    assert_v13_round_trip(&result, &workspace, settings);
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
    assert_v13_round_trip(&result, &workspace, settings);
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
    assert_v13_round_trip(&result, &workspace, settings);
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
    assert_v13_round_trip(&result, &workspace, settings);
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
    assert_v13_round_trip(&result, &workspace, settings);
}

fn assert_v13_round_trip(
    result: &pnpm_resolving_deps_resolver::ResolveWorkspaceResult,
    workspace: &Workspace,
    settings: super::Settings,
) {
    let legacy = super::lockfile_with_settings(
        result,
        workspace,
        super::Settings { format: pnpm_lockfile::LockfileFormat::V9, ..settings },
    );
    let next = super::lockfile_with_settings(result, workspace, settings);
    assert_eq!(next.importers, legacy.importers);
    assert_eq!(next.snapshots, legacy.snapshots);
    for (key, expected) in legacy.packages.as_ref().unwrap() {
        let actual = &next.packages.as_ref().unwrap()[key];
        assert_eq!(actual.peer_dependencies, expected.peer_dependencies, "{key}");
        assert_eq!(actual.peer_dependencies_meta, expected.peer_dependencies_meta, "{key}");
    }
}

#[test]
fn frozen_check_rejects_stale_recorded_catalog_children() {
    let workspace = Workspace {
        packages: vec![vec![depending_on(1, 1), leaf()], vec![leaf(), leaf()]],
        importers: vec![direct(1, 1)],
    };
    let settings = super::Settings {
        catalogs: true,
        injected: true,
        format: pnpm_lockfile::LockfileFormat::V13,
        ..Default::default()
    };
    let result =
        super::resolve_with(&workspace, &in_order(&workspace), settings, Reuse::Fresh).unwrap();
    let lockfile = super::lockfile_with_settings(&result, &workspace, settings);
    let fixture = super::fixture::Fixture::new(&workspace, settings);
    fixture.frozen_check(&lockfile, settings).unwrap();
    for version in [None, Some("2.0.0")] {
        let mut stale = lockfile.clone();
        let (_, shared) = stale.snapshots
            .as_mut()
            .unwrap()
            .iter_mut()
            .find(|(key, _)| key.name.to_string() == "shared")
            .unwrap();
        let dependencies = shared.dependencies.as_mut().unwrap();
        let alias = "p1".parse().unwrap();
        match version {
            Some(version) => {
                dependencies.insert(alias, version.parse().unwrap());
            }
            None => {
                dependencies.remove(&alias);
            }
        }
        assert!(fixture.frozen_check(&stale, settings).is_err(), "stale child: {version:?}");
    }
}

#[test]
fn optional_peer_dependency_cycle_is_stable_on_reinstall() {
    let workspace = Workspace {
        packages: vec![
            vec![leaf(), depending_on(2, 1)],
            vec![depending_on(2, 1), depending_on(3, 1)],
            vec![
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 1,
                        range: PeerRange::Major(2),
                        optional: true,
                        also_dependency: true,
                    }],
                },
                leaf(),
            ],
            vec![
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 2,
                        range: PeerRange::Major(1),
                        optional: true,
                        also_dependency: false,
                    }],
                },
                leaf(),
            ],
        ],
        importers: vec![direct(1, 1), direct(0, 2), direct(3, 1)],
    };
    for format in [pnpm_lockfile::LockfileFormat::V9, pnpm_lockfile::LockfileFormat::V13] {
        for peers_suffix_max_length in [20, 1000] {
            let settings = super::model::Settings {
                peers_suffix_max_length,
                auto_install_peers: false,
                format,
                ..Default::default()
            };
            let order = in_order(&workspace);
            let fresh = super::resolve_with(&workspace, &order, settings, Reuse::Fresh).unwrap();
            let lockfile = super::lockfile_with_settings(&fresh, &workspace, settings);
            let reused =
                super::resolve_with(&workspace, &order, settings, Reuse::Install(&lockfile))
                    .unwrap();
            let expected = outcome_of(&fresh, &workspace);
            let actual = outcome_of(&reused, &workspace);
            dbg!(&expected, &actual);
            assert_eq!(expected, actual, "{settings:?}");
        }
    }
}

#[test]
fn required_peer_fallback_cycle_is_stable_without_auto_install() {
    let workspace = Workspace {
        packages: vec![
            vec![depending_on(3, 2), depending_on(3, 1)],
            vec![
                PackageVersion { dependencies: vec![], peers: vec![peer(3, 1, false, true)] },
                peer_of(3, 1),
            ],
            vec![depending_on(1, 1), leaf()],
            vec![
                PackageVersion { dependencies: vec![], peers: vec![peer(1, 2, false, true)] },
                depending_on(0, 2),
            ],
            vec![
                PackageVersion { dependencies: direct(1, 1), peers: vec![peer(3, 2, true, false)] },
                leaf(),
            ],
        ],
        importers: vec![vec![Edge { target: 4, major: 1 }, Edge { target: 0, major: 1 }]],
    };
    let settings = super::model::Settings {
        auto_install_peers: false,
        catalogs: true,
        format: pnpm_lockfile::LockfileFormat::V13,
        ..Default::default()
    };
    let order = in_order(&workspace);
    let fresh = super::resolve_with(&workspace, &order, settings, Reuse::Fresh).unwrap();
    let lockfile = super::lockfile_with_settings(&fresh, &workspace, settings);
    let reused =
        super::resolve_with(&workspace, &order, settings, Reuse::Install(&lockfile)).unwrap();
    let expected = outcome_of(&fresh, &workspace);
    let actual = outcome_of(&reused, &workspace);
    dbg!(&expected, &actual);
    assert_eq!(expected, actual);
}

#[test]
fn reinstall_preserves_injected_peer_dependency_classification() {
    let workspace = Workspace {
        packages: vec![
            vec![
                PackageVersion {
                    dependencies: direct(1, 1),
                    peers: vec![peer(3, 2, false, false)],
                },
                leaf(),
            ],
            vec![depending_on(3, 1), peer_of(2, 1)],
            vec![leaf(), leaf()],
            vec![
                PackageVersion { dependencies: vec![], peers: vec![peer(4, 2, true, false)] },
                depending_on(4, 2),
            ],
            vec![
                leaf(),
                PackageVersion { dependencies: vec![], peers: vec![peer(2, 1, false, true)] },
            ],
        ],
        importers: vec![
            direct(2, 2),
            vec![Edge { target: 3, major: 2 }, Edge { target: 1, major: 2 }],
        ],
    };
    for format in [pnpm_lockfile::LockfileFormat::V9, pnpm_lockfile::LockfileFormat::V13] {
        for peers_suffix_max_length in [20, 1000] {
            for catalogs in [false, true] {
                let settings = super::Settings {
                    auto_install_peers: false,
                    injected: true,
                    format,
                    peers_suffix_max_length,
                    catalogs,
                };
                let order = in_order(&workspace);
                let fresh =
                    super::resolve_with(&workspace, &order, settings, Reuse::Fresh).unwrap();
                let lockfile = super::lockfile_with_settings(&fresh, &workspace, settings);
                let expected = outcome_of(&fresh, &workspace);
                assert_eq!(expected.graph["p4@2.0.0"]["p2"], "p2@1.0.0");
                let reused =
                    super::resolve_with(&workspace, &order, settings, Reuse::Install(&lockfile))
                        .unwrap();
                let actual = outcome_of(&reused, &workspace);
                dbg!(&expected, &actual);
                assert_eq!(expected, actual, "{settings:?}");
                let reused_lockfile = super::lockfile_with_settings(&reused, &workspace, settings);
                dbg!(&lockfile, &reused_lockfile);
                assert_eq!(lockfile, reused_lockfile);
            }
        }
    }
}

#[test]
fn peer_only_importer_keeps_a_provider_locked_by_a_range() {
    use super::{Settings, lockfile_with_settings, resolve_with};
    let workspace = Workspace {
        packages: vec![
            vec![
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 1,
                        range: PeerRange::Any,
                        optional: false,
                        also_dependency: false,
                    }],
                },
                leaf(),
            ],
            vec![leaf(), leaf()],
        ],
        importers: vec![direct(1, 2), direct(1, 1)],
    };
    let order = in_order(&workspace);
    for format in [pnpm_lockfile::LockfileFormat::V9, pnpm_lockfile::LockfileFormat::V13] {
        for catalogs in [false, true] {
            let settings = Settings { injected: true, catalogs, format, ..Default::default() };
            let fresh = resolve_with(&workspace, &order, settings, Reuse::Fresh).unwrap();
            let expected = outcome_of(&fresh, &workspace);
            assert_eq!(expected.direct["shared"]["p1"], "p1@2.0.0");
            let lockfile = lockfile_with_settings(&fresh, &workspace, settings);
            for reuse in [Reuse::Install(&lockfile), Reuse::Dedupe(&lockfile)] {
                let reused = resolve_with(&workspace, &order, settings, reuse).unwrap();
                let actual = outcome_of(&reused, &workspace);
                dbg!(&expected, &actual);
                assert_eq!(expected, actual);
            }
        }
    }
}

#[test]
fn retained_peer_context_preserves_transitive_cycle_context() {
    use super::{Settings, lockfile_with_settings, resolve_with};
    let any_peer = |target, also_dependency| PackageVersion {
        dependencies: vec![],
        peers: vec![Peer { target, range: PeerRange::Any, optional: false, also_dependency }],
    };
    let workspace = Workspace {
        packages: vec![
            vec![
                leaf(),
                PackageVersion { dependencies: vec![], peers: vec![peer(4, 2, false, true)] },
            ],
            vec![depending_on(4, 2), any_peer(0, true)],
            vec![leaf(), leaf()],
            vec![leaf(), leaf()],
            vec![peer_of(0, 1), any_peer(0, false)],
        ],
        importers: vec![
            vec![Edge { target: 1, major: 1 }, Edge { target: 4, major: 1 }],
            direct(0, 2),
        ],
    };
    let order = in_order(&workspace);
    let settings = Settings { auto_install_peers: false, ..Default::default() };
    let fresh = resolve_with(&workspace, &order, settings, Reuse::Fresh).unwrap();
    let lockfile = lockfile_with_settings(&fresh, &workspace, settings);
    let reused = resolve_with(&workspace, &order, settings, Reuse::Install(&lockfile)).unwrap();
    let expected = outcome_of(&fresh, &workspace);
    let actual = outcome_of(&reused, &workspace);
    dbg!(&expected, &actual);
    assert_eq!(expected, actual);
}

#[test]
fn cyclic_provider_keeps_its_optional_dependency_context() {
    use super::{Settings, lockfile_with_settings, resolve_with};
    let workspace = Workspace {
        packages: vec![
            vec![
                PackageVersion { dependencies: vec![], peers: vec![peer(3, 1, true, true)] },
                peer_of(2, 1),
            ],
            vec![leaf(), leaf()],
            vec![depending_on(0, 1), depending_on(0, 1)],
            vec![
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 2,
                        range: PeerRange::Any,
                        optional: false,
                        also_dependency: false,
                    }],
                },
                leaf(),
            ],
        ],
        importers: vec![
            direct(0, 2),
            vec![Edge { target: 3, major: 1 }, Edge { target: 2, major: 2 }],
        ],
    };
    let order = in_order(&workspace);
    let settings = Settings { auto_install_peers: true, ..Default::default() };
    let fresh = resolve_with(&workspace, &order, settings, Reuse::Fresh).unwrap();
    let lockfile = lockfile_with_settings(&fresh, &workspace, settings);
    let reused = resolve_with(&workspace, &order, settings, Reuse::Install(&lockfile)).unwrap();
    let expected = outcome_of(&fresh, &workspace);
    let actual = outcome_of(&reused, &workspace);
    dbg!(&expected, &actual);
    assert_eq!(expected, actual);
}

#[test]
fn unchanged_locked_cycle_keeps_transitive_peer_context() {
    use super::{Settings, lockfile_with_settings, resolve_with};
    let workspace = Workspace {
        packages: vec![
            vec![
                depending_on(3, 2),
                PackageVersion {
                    dependencies: vec![Edge { target: 2, major: 2 }],
                    peers: vec![Peer {
                        target: 1,
                        range: PeerRange::Any,
                        optional: false,
                        also_dependency: false,
                    }],
                },
            ],
            vec![peer_of(0, 1), peer_of(2, 2)],
            vec![
                depending_on(1, 2),
                PackageVersion {
                    dependencies: vec![Edge { target: 0, major: 1 }, Edge { target: 1, major: 1 }],
                    peers: vec![],
                },
            ],
            vec![
                peer_of(0, 2),
                PackageVersion {
                    dependencies: vec![Edge { target: 0, major: 2 }],
                    peers: vec![peer(2, 1, false, false)],
                },
            ],
        ],
        importers: vec![direct(0, 2), direct(2, 1), direct(3, 1)],
    };
    let order = in_order(&workspace);
    let settings = Settings { auto_install_peers: false, ..Default::default() };
    let fresh = resolve_with(&workspace, &order, settings, Reuse::Fresh).unwrap();
    let lockfile = lockfile_with_settings(&fresh, &workspace, settings);
    let reused = resolve_with(&workspace, &order, settings, Reuse::Install(&lockfile)).unwrap();
    let expected = outcome_of(&fresh, &workspace);
    let actual = outcome_of(&reused, &workspace);
    dbg!(&expected, &actual);
    assert_eq!(expected, actual);
}

#[test]
fn retained_leaf_provider_keeps_a_hashed_cycle_context() {
    use super::{Settings, lockfile_with_settings, resolve_with};
    let workspace = Workspace {
        packages: vec![
            vec![
                depending_on(1, 1),
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 2,
                        range: PeerRange::Any,
                        optional: true,
                        also_dependency: false,
                    }],
                },
            ],
            vec![depending_on(3, 1), leaf()],
            vec![
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 3,
                        range: PeerRange::Any,
                        optional: false,
                        also_dependency: false,
                    }],
                },
                peer_of(0, 2),
            ],
            vec![
                PackageVersion {
                    dependencies: vec![Edge { target: 2, major: 2 }],
                    peers: vec![Peer {
                        target: 1,
                        range: PeerRange::Any,
                        optional: false,
                        also_dependency: false,
                    }],
                },
                leaf(),
            ],
        ],
        importers: vec![direct(2, 1), direct(3, 1)],
    };
    let order = in_order(&workspace);
    let settings =
        Settings { auto_install_peers: true, peers_suffix_max_length: 20, ..Default::default() };
    let fresh = resolve_with(&workspace, &order, settings, Reuse::Fresh).unwrap();
    let lockfile = lockfile_with_settings(&fresh, &workspace, settings);
    let reused = resolve_with(&workspace, &order, settings, Reuse::Install(&lockfile)).unwrap();
    let expected = outcome_of(&fresh, &workspace);
    let actual = outcome_of(&reused, &workspace);
    dbg!(&expected, &actual);
    assert_eq!(expected, actual);
}

#[test]
fn shared_children_do_not_reuse_another_occurrences_peer_context() {
    use super::{Settings, lockfile_with_settings, resolve_with};
    let workspace = Workspace {
        packages: vec![
            vec![
                PackageVersion {
                    dependencies: vec![Edge { target: 1, major: 1 }],
                    peers: vec![Peer {
                        target: 3,
                        range: PeerRange::Major(1),
                        optional: true,
                        also_dependency: true,
                    }],
                },
                leaf(),
            ],
            vec![
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 2,
                        range: PeerRange::Major(2),
                        optional: true,
                        also_dependency: false,
                    }],
                },
                leaf(),
            ],
            vec![peer_of(0, 1), leaf()],
            vec![peer_of(2, 1), leaf()],
        ],
        importers: vec![
            vec![Edge { target: 1, major: 1 }, Edge { target: 3, major: 1 }],
            vec![Edge { target: 2, major: 2 }, Edge { target: 0, major: 1 }],
        ],
    };
    let order = in_order(&workspace);
    let settings = Settings { auto_install_peers: true, ..Default::default() };
    let fresh = resolve_with(&workspace, &order, settings, Reuse::Fresh).unwrap();
    let lockfile = lockfile_with_settings(&fresh, &workspace, settings);
    let reused = resolve_with(&workspace, &order, settings, Reuse::Install(&lockfile)).unwrap();
    let expected = outcome_of(&fresh, &workspace);
    let actual = outcome_of(&reused, &workspace);
    dbg!(&expected, &actual);
    assert_eq!(expected, actual);
}

#[test]
fn optional_cycle_peer_keeps_its_transitive_context() {
    use super::{Settings, lockfile_with_settings, resolve_with};
    let workspace = Workspace {
        packages: vec![
            vec![
                depending_on(2, 1),
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 1,
                        range: PeerRange::Any,
                        optional: false,
                        also_dependency: false,
                    }],
                },
            ],
            vec![peer_of(4, 1), leaf()],
            vec![
                PackageVersion {
                    dependencies: vec![Edge { target: 0, major: 2 }],
                    peers: vec![Peer {
                        target: 0,
                        range: PeerRange::Major(1),
                        optional: true,
                        also_dependency: false,
                    }],
                },
                leaf(),
            ],
            vec![leaf(), leaf()],
            vec![
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 0,
                        range: PeerRange::Any,
                        optional: true,
                        also_dependency: false,
                    }],
                },
                leaf(),
            ],
        ],
        importers: vec![direct(0, 1), direct(2, 1), direct(1, 1)],
    };
    let order = in_order(&workspace);
    for format in [pnpm_lockfile::LockfileFormat::V9, pnpm_lockfile::LockfileFormat::V13] {
        let settings = Settings {
            auto_install_peers: true,
            peers_suffix_max_length: 20,
            format,
            ..Default::default()
        };
        let fresh = resolve_with(&workspace, &order, settings, Reuse::Fresh).unwrap();
        let lockfile = lockfile_with_settings(&fresh, &workspace, settings);
        let reused = resolve_with(&workspace, &order, settings, Reuse::Install(&lockfile)).unwrap();
        let expected = outcome_of(&fresh, &workspace);
        let actual = outcome_of(&reused, &workspace);
        dbg!(&expected, &actual);
        assert_eq!(expected, actual, "{format:?}");
    }
}

#[test]
fn retained_direct_provider_preserves_shared_cycle_contexts() {
    use super::{Settings, lockfile_with_settings, resolve_with};
    let workspace = Workspace {
        packages: vec![
            vec![leaf(), depending_on(1, 2)],
            vec![
                depending_on(3, 2),
                PackageVersion {
                    dependencies: vec![Edge { target: 3, major: 1 }],
                    peers: vec![Peer {
                        target: 0,
                        range: PeerRange::Any,
                        optional: false,
                        also_dependency: false,
                    }],
                },
            ],
            vec![
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 1,
                        range: PeerRange::Major(1),
                        optional: true,
                        also_dependency: true,
                    }],
                },
                peer_of(1, 2),
            ],
            vec![depending_on(2, 2), depending_on(0, 2)],
        ],
        importers: vec![
            vec![Edge { target: 2, major: 1 }, Edge { target: 0, major: 1 }],
            direct(2, 2),
        ],
    };
    let order = in_order(&workspace);
    let settings =
        Settings { auto_install_peers: true, peers_suffix_max_length: 1000, ..Default::default() };
    let fresh = resolve_with(&workspace, &order, settings, Reuse::Fresh).unwrap();
    let lockfile = lockfile_with_settings(&fresh, &workspace, settings);
    let reused = resolve_with(&workspace, &order, settings, Reuse::Install(&lockfile)).unwrap();
    assert_eq!(outcome_of(&fresh, &workspace), outcome_of(&reused, &workspace));
}

#[test]
fn bare_non_leaf_provider_does_not_replace_an_injected_cycle_context() {
    use super::{Settings, lockfile_with_settings, resolve_with};
    let workspace = Workspace {
        packages: vec![
            vec![
                depending_on(3, 2),
                PackageVersion {
                    dependencies: vec![Edge { target: 1, major: 2 }],
                    peers: vec![Peer {
                        target: 1,
                        range: PeerRange::Major(1),
                        optional: true,
                        also_dependency: false,
                    }],
                },
            ],
            vec![leaf(), depending_on(2, 1)],
            vec![
                PackageVersion {
                    dependencies: vec![Edge { target: 0, major: 2 }],
                    peers: vec![Peer {
                        target: 3,
                        range: PeerRange::Any,
                        optional: false,
                        also_dependency: false,
                    }],
                },
                peer_of(0, 2),
            ],
            vec![
                PackageVersion {
                    dependencies: vec![Edge { target: 2, major: 1 }, Edge { target: 1, major: 1 }],
                    peers: vec![],
                },
                leaf(),
            ],
        ],
        importers: vec![direct(3, 1), direct(2, 2)],
    };
    let order = in_order(&workspace);
    let settings = Settings {
        injected: true,
        auto_install_peers: false,
        peers_suffix_max_length: 1000,
        ..Default::default()
    };
    let fresh = resolve_with(&workspace, &order, settings, Reuse::Fresh).unwrap();
    let lockfile = lockfile_with_settings(&fresh, &workspace, settings);
    let reused = resolve_with(&workspace, &order, settings, Reuse::Install(&lockfile)).unwrap();
    assert_eq!(outcome_of(&fresh, &workspace), outcome_of(&reused, &workspace));
}
