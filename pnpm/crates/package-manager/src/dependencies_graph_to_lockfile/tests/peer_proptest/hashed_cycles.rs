use super::{Reuse, in_order, model, resolve_with};
use pnpm_lockfile::LockfileFormat;

#[test]
fn injected_peer_cycle_with_hashed_suffix_serializes() {
    use model::{Edge, PackageVersion, Peer, PeerRange, Workspace};
    let empty = PackageVersion { dependencies: vec![], peers: vec![] };
    let peer = |target, major| Peer {
        target,
        range: PeerRange::Major(major),
        optional: false,
        also_dependency: false,
    };
    let workspace = Workspace {
        packages: vec![
            vec![
                PackageVersion {
                    dependencies: vec![Edge { target: 2, major: 1 }],
                    peers: vec![peer(1, 2), peer(2, 1)],
                },
                empty.clone(),
            ],
            vec![empty.clone(), empty.clone()],
            vec![
                PackageVersion {
                    dependencies: vec![Edge { target: 1, major: 2 }],
                    peers: vec![peer(0, 1)],
                },
                empty,
            ],
        ],
        importers: vec![vec![Edge { target: 0, major: 1 }]],
    };
    assert_round_trip(&workspace);
}

#[test]
fn injected_mutual_peers_with_hashed_suffix_serialize() {
    use model::{Edge, PackageVersion, Peer, PeerRange, Workspace};
    let empty = PackageVersion { dependencies: vec![], peers: vec![] };
    let peer = |target, major, also_dependency| Peer {
        target,
        range: PeerRange::Major(major),
        optional: false,
        also_dependency,
    };
    let workspace = Workspace {
        packages: vec![
            vec![
                PackageVersion {
                    dependencies: vec![Edge { target: 3, major: 2 }],
                    peers: vec![peer(1, 2, true), peer(2, 1, false)],
                },
                empty.clone(),
            ],
            vec![
                empty.clone(),
                PackageVersion { dependencies: vec![], peers: vec![peer(3, 2, false)] },
            ],
            vec![empty.clone(), empty.clone()],
            vec![
                empty,
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![peer(1, 2, false), peer(0, 1, false)],
                },
            ],
        ],
        importers: vec![vec![Edge { target: 0, major: 1 }], vec![Edge { target: 2, major: 1 }]],
    };
    assert_round_trip(&workspace);
}

#[test]
fn injected_peer_cycles_do_not_bind_unlisted_transitive_providers() {
    use model::{Edge, PackageVersion, Peer, PeerRange, Workspace};
    let empty = PackageVersion { dependencies: vec![], peers: vec![] };
    let peer = |target, major, also_dependency| Peer {
        target,
        range: PeerRange::Major(major),
        optional: false,
        also_dependency,
    };
    let workspace = Workspace {
        packages: vec![
            vec![
                PackageVersion {
                    dependencies: vec![],
                    peers: vec![peer(1, 2, true), peer(2, 1, false)],
                },
                empty.clone(),
            ],
            vec![
                empty.clone(),
                PackageVersion { dependencies: vec![], peers: vec![peer(4, 1, false)] },
            ],
            vec![
                PackageVersion { dependencies: vec![], peers: vec![peer(3, 2, false)] },
                empty.clone(),
            ],
            vec![empty.clone(), empty.clone()],
            vec![PackageVersion { dependencies: vec![], peers: vec![peer(1, 2, false)] }, empty],
        ],
        importers: vec![
            vec![Edge { target: 4, major: 1 }],
            vec![Edge { target: 2, major: 1 }, Edge { target: 3, major: 2 }],
        ],
    };
    assert_round_trip(&workspace);
}

fn assert_round_trip(workspace: &model::Workspace) {
    let settings = model::Settings {
        auto_install_peers: false,
        injected: true,
        peers_suffix_max_length: 20,
        format: LockfileFormat::V13,
        ..Default::default()
    };
    let initial = resolve_with(workspace, &in_order(workspace), settings, Reuse::Fresh).unwrap();
    let mut lockfile = model::lockfile_with_settings(
        &initial,
        workspace,
        model::Settings { format: LockfileFormat::V9, ..settings },
    );
    lockfile.lockfile_version = LockfileFormat::V13.version();
    let restored = model::round_trip(&lockfile);
    assert_eq!(lockfile.importers, restored.importers);
    assert_eq!(lockfile.snapshots, restored.snapshots);
}
