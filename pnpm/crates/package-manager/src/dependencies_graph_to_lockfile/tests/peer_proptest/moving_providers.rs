use super::{Reuse, config, fixture, graph, in_order, lockfile_with_settings, model, resolve_with};
use proptest::prelude::*;

#[derive(Debug, Clone)]
struct DependencyChange {
    importer: usize,
    target: usize,
    major: u8,
}

fn dependency_change() -> impl Strategy<Value = DependencyChange> {
    (0usize..3, 0usize..5, any::<bool>()).prop_map(|(importer, target, second_major)| {
        DependencyChange { importer, target, major: if second_major { 2 } else { 1 } }
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
        edges.push(model::Edge { target, major });
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
                vec![DependencyChange { importer: 1, target: 0, major: 2 }],
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
    let changes = vec![DependencyChange { importer: 1, target: 1, major: 1 }];
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
            DependencyChange { importer: 1, target: 2, major: 1 },
            DependencyChange { importer: 0, target: 3, major: 2 },
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
    check_changes(workspace, settings, vec![DependencyChange { importer: 1, target: 0, major: 1 }])
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
            vec![DependencyChange { importer: 0, target: 0, major: 1 }],
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
            vec![DependencyChange { importer: 0, target: 0, major: 1 }],
        )
        .unwrap();
    }
}
