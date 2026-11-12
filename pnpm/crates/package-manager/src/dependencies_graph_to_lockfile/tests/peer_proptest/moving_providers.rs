use super::{Reuse, config, fixture, graph, in_order, lockfile_with_settings, model, resolve_with};
use proptest::prelude::*;

#[derive(Debug, Clone)]
struct DependencyChange {
    importer: usize,
    target: usize,
    major: Option<u8>,
}

fn dependency_change() -> impl Strategy<Value = DependencyChange> {
    (0usize..3, 0usize..5, prop::option::of(1u8..=2)).prop_map(|(importer, target, major)| {
        DependencyChange { importer, target, major }
    })
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn moving_providers_settle_after_each_install(
        workspace in model::workspace(),
        settings in model::settings(),
        changes in proptest::collection::vec(dependency_change(), 1..=4),
    ) {
        check_changes(workspace, settings, changes)?;
    }
}

fn check_changes(
    mut workspace: model::Workspace,
    settings: model::Settings,
    changes: Vec<DependencyChange>,
) -> proptest::test_runner::TestCaseResult {
    let order = in_order(&workspace);
    let initial = resolve_with(&workspace, &order, settings, Reuse::Fresh)
        .expect("initial workspace resolves");
    let mut lockfile = lockfile_with_settings(&initial, &workspace, settings);
    for DependencyChange { importer, target, major } in changes {
        let importer = importer % workspace.importers.len();
        let target = target % workspace.packages.len();
        let edges = &mut workspace.importers[importer];
        edges.retain(|edge| edge.target != target);
        if let Some(major) = major {
            edges.push(model::Edge { target, major });
        }
        let installed = resolve_with(&workspace, &order, settings, Reuse::Install(&lockfile))
            .expect("changed workspace resolves");
        lockfile = lockfile_with_settings(&installed, &workspace, settings);
        if !settings.injected || settings.format == pnpm_lockfile::LockfileFormat::V13 {
            let freshness =
                fixture::Fixture::new(&workspace, settings).frozen_check(&lockfile, settings);
            let expected = Ok(());
            prop_assert_eq!(freshness, expected);
        }
        let reinstalled = resolve_with(&workspace, &order, settings, Reuse::Install(&lockfile))
            .expect("reinstall changed workspace");
        let rewritten = lockfile_with_settings(&reinstalled, &workspace, settings);
        let mut installed_graph = lockfile.clone();
        let mut reinstalled_graph = rewritten.clone();
        crate::prune_unreachable_packages(&mut installed_graph);
        crate::prune_unreachable_packages(&mut reinstalled_graph);
        graph::assert_equivalent(&installed_graph, &reinstalled_graph);
        lockfile = rewritten;
    }

    Ok(())
}

#[test]
fn changed_optional_provider_is_stable() {
    use model::{
        Edge, PackageVersion, Peer,
        PeerRange::{Any, Major},
        Settings, Workspace,
    };
    let workspace = Workspace {
        packages: vec![
            vec![
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 2,
                        range: Major(1),
                        optional: false,
                        also_dependency: false,
                    }],
                },
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 1,
                        range: Any,
                        optional: false,
                        also_dependency: false,
                    }],
                },
            ],
            vec![
                PackageVersion { dependencies: vec![], peers: vec![] },
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 2,
                        range: Any,
                        optional: true,
                        also_dependency: false,
                    }],
                },
            ],
            vec![
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 1,
                        range: Major(2),
                        optional: false,
                        also_dependency: false,
                    }],
                },
                PackageVersion { dependencies: vec![], peers: vec![] },
            ],
        ],
        importers: vec![
            vec![Edge { target: 2, major: 1 }],
            vec![Edge { target: 0, major: 1 }],
            vec![Edge { target: 2, major: 2 }],
        ],
    };
    for format in [pnpm_lockfile::LockfileFormat::V9, pnpm_lockfile::LockfileFormat::V13] {
        for peers_suffix_max_length in [20, 1000] {
            let settings = Settings { peers_suffix_max_length, format, ..Default::default() };
            check_changes(
                workspace.clone(),
                settings,
                vec![DependencyChange { importer: 1, target: 0, major: Some(2) }],
            )
            .unwrap();
        }
    }
}

#[test]
fn changed_injected_provider_is_stable() {
    use model::{
        Edge, PackageVersion, Peer,
        PeerRange::{Any, Major},
        Settings, Workspace,
    };
    let workspace = Workspace {
        packages: vec![
            vec![
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![
                        Peer {
                            target: 4,
                            range: Major(2),
                            optional: false,
                            also_dependency: false,
                        },
                        Peer { target: 1, range: Major(1), optional: true, also_dependency: false },
                    ],
                },
                PackageVersion {
                    dependencies: vec![Edge { target: 4, major: 2 }, Edge { target: 2, major: 2 }],
                    peers: vec![],
                },
            ],
            vec![
                PackageVersion {
                    dependencies: vec![Edge { target: 0, major: 2 }, Edge { target: 4, major: 2 }],
                    peers: vec![Peer {
                        target: 4,
                        range: Major(1),
                        optional: true,
                        also_dependency: false,
                    }],
                },
                PackageVersion { dependencies: vec![], peers: vec![] },
            ],
            vec![
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![
                        Peer {
                            target: 4,
                            range: Major(1),
                            optional: false,
                            also_dependency: false,
                        },
                        Peer {
                            target: 0,
                            range: Major(1),
                            optional: false,
                            also_dependency: false,
                        },
                    ],
                },
                PackageVersion { dependencies: vec![Edge { target: 0, major: 1 }], peers: vec![] },
            ],
            vec![
                PackageVersion { dependencies: vec![], peers: vec![] },
                PackageVersion { dependencies: vec![], peers: vec![] },
            ],
            vec![
                PackageVersion { dependencies: vec![], peers: vec![] },
                PackageVersion {
                    dependencies: vec![Edge { target: 2, major: 1 }],
                    peers: vec![Peer {
                        target: 0,
                        range: Any,
                        optional: true,
                        also_dependency: false,
                    }],
                },
            ],
        ],
        importers: vec![
            vec![Edge { target: 0, major: 1 }],
            vec![Edge { target: 2, major: 2 }],
            vec![Edge { target: 0, major: 2 }],
        ],
    };
    let settings = Settings {
        peers_suffix_max_length: 20,
        auto_install_peers: true,
        catalogs: false,
        injected: true,
        format: pnpm_lockfile::LockfileFormat::V13,
    };
    let changes = vec![DependencyChange { importer: 1, target: 1, major: Some(1) }];
    check_changes(workspace, settings, changes).unwrap();
}

#[test]
fn changed_catalog_provider_is_frozen_fresh() {
    use model::{Edge, PackageVersion, Peer, PeerRange, Settings, Workspace};
    let leaf = || PackageVersion { dependencies: vec![], peers: vec![] };
    let workspace = Workspace {
        packages: vec![
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
                leaf(),
            ],
            vec![leaf(), leaf()],
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
        ],
        importers: vec![vec![Edge { target: 1, major: 2 }], vec![Edge { target: 0, major: 1 }]],
    };
    let settings = Settings {
        peers_suffix_max_length: 20,
        auto_install_peers: true,
        catalogs: true,
        injected: true,
        format: pnpm_lockfile::LockfileFormat::V13,
    };
    check_changes(
        workspace,
        settings,
        vec![
            DependencyChange { importer: 1, target: 2, major: Some(1) },
            DependencyChange { importer: 0, target: 3, major: Some(2) },
        ],
    )
    .unwrap();
}

#[test]
fn changed_catalog_provider_with_peers_is_frozen_fresh() {
    use model::{Edge, PackageVersion, Peer, PeerRange, Settings, Workspace};
    let leaf = || PackageVersion { dependencies: vec![], peers: vec![] };
    let workspace = Workspace {
        packages: vec![
            vec![
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 2,
                        range: PeerRange::Major(2),
                        optional: true,
                        also_dependency: true,
                    }],
                },
                leaf(),
            ],
            vec![leaf(), leaf()],
            vec![
                leaf(),
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 3,
                        range: PeerRange::Major(1),
                        optional: false,
                        also_dependency: false,
                    }],
                },
            ],
            vec![leaf(), leaf()],
        ],
        importers: vec![vec![Edge { target: 2, major: 1 }], vec![Edge { target: 3, major: 2 }]],
    };
    let settings = Settings {
        peers_suffix_max_length: 1000,
        auto_install_peers: true,
        catalogs: true,
        injected: true,
        format: pnpm_lockfile::LockfileFormat::V13,
    };
    let fresh = resolve_with(&workspace, &in_order(&workspace), settings, Reuse::Fresh).unwrap();
    let lockfile = lockfile_with_settings(&fresh, &workspace, settings);
    let mut fixture = fixture::Fixture::new(&workspace, settings);
    fixture.frozen_check(&lockfile, settings).unwrap();
    fixture.catalogs
        .get_mut("two")
        .unwrap()
        .insert("p2".to_string(), "^1.0.0".to_string());
    assert!(fixture.frozen_check(&lockfile, settings).is_err());
    check_changes(
        workspace,
        settings,
        vec![DependencyChange { importer: 1, target: 0, major: Some(1) }],
    )
    .unwrap();
}

#[test]
fn required_catalog_peer_keeps_the_owners_child_classification() {
    use model::{Edge, PackageVersion, Peer, PeerRange, Settings, Workspace};
    let leaf = || PackageVersion { dependencies: vec![], peers: vec![] };
    let workspace = Workspace {
        packages: vec![
            vec![
                PackageVersion {
                    dependencies: vec![Edge { target: 1, major: 2 }],
                    peers: vec![Peer {
                        target: 1,
                        range: PeerRange::Major(1),
                        optional: false,
                        also_dependency: false,
                    }],
                },
                leaf(),
            ],
            vec![leaf(), leaf()],
            vec![leaf(), leaf()],
        ],
        importers: vec![vec![Edge { target: 1, major: 1 }], vec![Edge { target: 2, major: 1 }]],
    };
    for peers_suffix_max_length in [20, 1000] {
        let settings = Settings {
            peers_suffix_max_length,
            auto_install_peers: false,
            catalogs: true,
            injected: true,
            format: pnpm_lockfile::LockfileFormat::V13,
        };
        let fresh =
            resolve_with(&workspace, &in_order(&workspace), settings, Reuse::Fresh).unwrap();
        let lockfile = lockfile_with_settings(&fresh, &workspace, settings);
        fixture::Fixture::new(&workspace, settings).frozen_check(&lockfile, settings).unwrap();
        check_changes(
            workspace.clone(),
            settings,
            vec![DependencyChange { importer: 0, target: 0, major: Some(1) }],
        )
        .unwrap();
    }
}

#[test]
fn changed_required_provider_on_a_dependency_cycle_is_stable() {
    use model::{Edge, PackageVersion, Peer, PeerRange, Settings, Workspace};
    let leaf = || PackageVersion { dependencies: vec![], peers: vec![] };
    for provider_major in [1, 2] {
        let workspace = Workspace {
            packages: vec![
                vec![
                    PackageVersion {
                        dependencies: vec![Edge { target: 1, major: 1 }],
                        peers: vec![],
                    },
                    leaf(),
                ],
                vec![
                    PackageVersion {
                        dependencies: vec![],
                        peers: vec![Peer {
                            target: 2,
                            range: PeerRange::Major(2),
                            optional: false,
                            also_dependency: true,
                        }],
                    },
                    PackageVersion {
                        dependencies: vec![Edge { target: 0, major: 1 }],
                        peers: vec![Peer {
                            target: 0,
                            range: PeerRange::Major(1),
                            optional: false,
                            also_dependency: false,
                        }],
                    },
                ],
                vec![
                    leaf(),
                    PackageVersion {
                        dependencies: vec![
                            Edge { target: 1, major: 2 },
                            Edge { target: 0, major: provider_major },
                        ],
                        peers: vec![],
                    },
                ],
            ],
            importers: vec![vec![Edge { target: 0, major: 2 }]],
        };
        let settings = Settings {
            peers_suffix_max_length: 20,
            auto_install_peers: false,
            catalogs: true,
            injected: true,
            format: pnpm_lockfile::LockfileFormat::V13,
        };
        check_changes(
            workspace,
            settings,
            vec![DependencyChange { importer: 0, target: 0, major: Some(1) }],
        )
        .unwrap();
    }
}

#[test]
fn many_required_catalog_peers_are_frozen_fresh() {
    use model::{Edge, PackageVersion, Peer, PeerRange, Settings, Workspace};
    let leaf = PackageVersion { dependencies: vec![], peers: vec![] };
    let mut packages = vec![vec![leaf.clone(), leaf]; 14];
    packages[0][0] = PackageVersion {
        dependencies: vec![Edge { target: 1, major: 2 }],
        peers: (1..14)
            .map(|target| Peer {
                target,
                range: PeerRange::Major(1),
                optional: false,
                also_dependency: false,
            })
            .collect(),
    };
    let workspace = Workspace {
        packages,
        importers: vec![
            (1..14)
                .map(|target| Edge { target, major: 1 })
                .collect(),
            vec![],
        ],
    };
    let settings = Settings {
        peers_suffix_max_length: 20,
        auto_install_peers: false,
        catalogs: true,
        injected: true,
        format: pnpm_lockfile::LockfileFormat::V13,
    };
    let fresh = resolve_with(&workspace, &in_order(&workspace), settings, Reuse::Fresh).unwrap();
    let lockfile = lockfile_with_settings(&fresh, &workspace, settings);
    fixture::Fixture::new(&workspace, settings).frozen_check(&lockfile, settings).unwrap();
}

#[test]
fn missing_required_catalog_peer_child_is_outdated() {
    use model::{PackageVersion, Peer, PeerRange, Settings, Workspace};
    let leaf = || PackageVersion { dependencies: vec![], peers: vec![] };
    let workspace = Workspace {
        packages: vec![
            vec![
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 1,
                        range: PeerRange::Major(1),
                        optional: false,
                        also_dependency: true,
                    }],
                },
                leaf(),
            ],
            vec![leaf(), leaf()],
        ],
        importers: vec![vec![]],
    };
    let settings = Settings {
        auto_install_peers: false,
        catalogs: true,
        injected: true,
        format: pnpm_lockfile::LockfileFormat::V13,
        ..Default::default()
    };
    let fresh = resolve_with(&workspace, &in_order(&workspace), settings, Reuse::Fresh).unwrap();
    let mut lockfile = lockfile_with_settings(&fresh, &workspace, settings);
    let fixture = fixture::Fixture::new(&workspace, settings);
    fixture.frozen_check(&lockfile, settings).unwrap();
    let shared = "shared".parse().unwrap();
    let key = lockfile.importers["."].dependencies.as_ref().unwrap()[&shared]
        .version
        .resolved_key(&shared)
        .unwrap();
    let removed = lockfile.snapshots
        .as_mut()
        .unwrap()
        .get_mut(&key)
        .unwrap()
        .dependencies
        .as_mut()
        .unwrap()
        .remove(&"p1".parse().unwrap());
    assert!(removed.is_some());
    assert!(fixture.frozen_check(&lockfile, settings).is_err());
    let orphan_key = format!("{}(p1@2.0.0)", key.without_peer()).parse().unwrap();
    assert_ne!(key, orphan_key);
    let mut orphan = lockfile.snapshots.as_ref().unwrap()[&key].clone();
    orphan.dependencies
        .as_mut()
        .unwrap()
        .insert("p1".parse().unwrap(), "2.0.0".parse().unwrap());
    lockfile.snapshots
        .as_mut()
        .unwrap()
        .insert(orphan_key, orphan);
    assert!(fixture.frozen_check(&lockfile, settings).is_err());
}

#[test]
fn removed_direct_provider_is_stable() {
    use model::{Edge, PackageVersion, Peer, PeerRange, Settings, Workspace};
    let leaf = PackageVersion { dependencies: vec![], peers: vec![] };
    let workspace = Workspace {
        packages: vec![
            vec![leaf.clone(), leaf.clone()],
            vec![
                leaf,
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 0,
                        range: PeerRange::Any,
                        optional: false,
                        also_dependency: false,
                    }],
                },
            ],
        ],
        importers: vec![vec![Edge { target: 0, major: 1 }, Edge { target: 1, major: 2 }]],
    };
    check_changes(
        workspace,
        Settings::default(),
        vec![DependencyChange { importer: 0, target: 0, major: None }],
    )
    .unwrap();
}

#[test]
fn removed_nested_optional_provider_is_stable() {
    use model::{Edge, PackageVersion, Peer, PeerRange, Settings, Workspace};
    let leaf = || PackageVersion { dependencies: vec![], peers: vec![] };
    let peer = |target, major, optional| Peer {
        target,
        range: PeerRange::Major(major),
        optional,
        also_dependency: false,
    };
    let workspace = Workspace {
        packages: vec![
            vec![
                leaf(),
                PackageVersion { dependencies: vec![Edge { target: 2, major: 2 }], peers: vec![] },
            ],
            vec![
                PackageVersion { dependencies: vec![], peers: vec![peer(2, 1, false)] },
                PackageVersion { dependencies: vec![], peers: vec![peer(0, 2, false)] },
            ],
            vec![leaf(), PackageVersion { dependencies: vec![], peers: vec![peer(1, 1, true)] }],
        ],
        importers: vec![
            vec![Edge { target: 1, major: 2 }],
            vec![Edge { target: 0, major: 2 }],
            vec![Edge { target: 2, major: 2 }],
        ],
    };
    let settings = Settings {
        auto_install_peers: false,
        injected: true,
        format: pnpm_lockfile::LockfileFormat::V13,
        peers_suffix_max_length: 20,
        ..Default::default()
    };
    check_changes(
        workspace,
        settings,
        vec![
            DependencyChange { importer: 1, target: 1, major: Some(1) },
            DependencyChange { importer: 1, target: 0, major: None },
            DependencyChange { importer: 1, target: 1, major: None },
        ],
    )
    .unwrap();
}

#[test]
fn added_provider_updates_optional_candidates_child_owner() {
    use model::{Edge, PackageVersion, Peer, PeerRange, Settings, Workspace};
    let leaf = || PackageVersion { dependencies: vec![], peers: vec![] };
    let workspace = Workspace {
        packages: vec![
            vec![
                leaf(),
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 1,
                        range: PeerRange::Major(2),
                        optional: false,
                        also_dependency: true,
                    }],
                },
            ],
            vec![
                leaf(),
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 0,
                        range: PeerRange::Major(1),
                        optional: false,
                        also_dependency: false,
                    }],
                },
            ],
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
        importers: vec![vec![Edge { target: 2, major: 1 }], vec![Edge { target: 1, major: 1 }]],
    };
    check_changes(
        workspace,
        Settings { auto_install_peers: false, ..Default::default() },
        vec![
            DependencyChange { importer: 1, target: 0, major: Some(2) },
            DependencyChange { importer: 0, target: 1, major: Some(1) },
        ],
    )
    .unwrap();
}

#[test]
fn adding_a_provider_with_a_required_peer_cycle_is_stable() {
    use model::{Edge, PackageVersion, Peer, PeerRange, Settings, Workspace};
    let leaf = || PackageVersion { dependencies: vec![], peers: vec![] };
    let workspace = Workspace {
        packages: vec![
            vec![
                PackageVersion {
                    dependencies: vec![Edge { target: 1, major: 1 }],
                    peers: vec![Peer {
                        target: 2,
                        range: PeerRange::Major(1),
                        optional: false,
                        also_dependency: false,
                    }],
                },
                leaf(),
            ],
            vec![
                leaf(),
                PackageVersion { dependencies: vec![Edge { target: 0, major: 1 }], peers: vec![] },
            ],
            vec![
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 1,
                        range: PeerRange::Any,
                        optional: true,
                        also_dependency: false,
                    }],
                },
                leaf(),
            ],
            vec![leaf(), leaf()],
        ],
        importers: vec![vec![Edge { target: 0, major: 1 }]; 3],
    };
    for format in [pnpm_lockfile::LockfileFormat::V9, pnpm_lockfile::LockfileFormat::V13] {
        for peers_suffix_max_length in [20, 1000] {
            check_changes(
                workspace.clone(),
                Settings { peers_suffix_max_length, format, ..Default::default() },
                vec![DependencyChange { importer: 1, target: 1, major: Some(2) }],
            )
            .unwrap();
        }
    }
}

#[test]
fn removing_a_direct_provider_preserves_nested_injected_peer_pins() {
    use model::{Edge, PackageVersion, Peer, PeerRange, Settings, Workspace};
    let leaf = || PackageVersion { dependencies: vec![], peers: vec![] };
    let workspace = Workspace {
        packages: vec![
            vec![
                PackageVersion {
                    dependencies: vec![Edge { target: 1, major: 2 }],
                    peers: vec![Peer {
                        target: 1,
                        range: PeerRange::Major(1),
                        optional: true,
                        also_dependency: false,
                    }],
                },
                leaf(),
            ],
            vec![
                leaf(),
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![Peer {
                        target: 2,
                        range: PeerRange::Major(1),
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
                        also_dependency: false,
                    }],
                },
                leaf(),
            ],
        ],
        importers: vec![vec![Edge { target: 1, major: 1 }]],
    };
    check_changes(
        workspace,
        Settings {
            injected: true,
            format: pnpm_lockfile::LockfileFormat::V13,
            ..Default::default()
        },
        vec![
            DependencyChange { importer: 0, target: 1, major: None },
            DependencyChange { importer: 0, target: 0, major: Some(1) },
            DependencyChange { importer: 0, target: 1, major: None },
            DependencyChange { importer: 0, target: 1, major: None },
        ],
    )
    .unwrap();
}

#[test]
fn adding_a_consumer_to_a_peer_cycle_preserves_required_pins() {
    use model::{Edge, PackageVersion, Peer, PeerRange, Settings, Workspace};
    let leaf = || PackageVersion { dependencies: vec![], peers: vec![] };
    let peer = |target, range| Peer { target, range, optional: false, also_dependency: false };
    let workspace = Workspace {
        packages: vec![
            vec![
                PackageVersion { dependencies: vec![], peers: vec![peer(2, PeerRange::Major(1))] },
                leaf(),
            ],
            vec![
                PackageVersion { dependencies: vec![Edge { target: 0, major: 2 }], peers: vec![] },
                leaf(),
            ],
            vec![
                PackageVersion { dependencies: vec![], peers: vec![peer(0, PeerRange::Major(1))] },
                leaf(),
            ],
            vec![leaf(), leaf()],
            vec![
                leaf(),
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![peer(0, PeerRange::Any), peer(1, PeerRange::Major(1))],
                },
            ],
        ],
        importers: vec![vec![Edge { target: 3, major: 1 }]],
    };
    for format in [pnpm_lockfile::LockfileFormat::V9, pnpm_lockfile::LockfileFormat::V13] {
        for peers_suffix_max_length in [20, 1000] {
            check_changes(
                workspace.clone(),
                Settings {
                    peers_suffix_max_length,
                    catalogs: true,
                    injected: true,
                    format,
                    ..Default::default()
                },
                vec![
                    DependencyChange { importer: 0, target: 4, major: Some(2) },
                    DependencyChange { importer: 0, target: 0, major: Some(1) },
                ],
            )
            .unwrap();
        }
    }
}
