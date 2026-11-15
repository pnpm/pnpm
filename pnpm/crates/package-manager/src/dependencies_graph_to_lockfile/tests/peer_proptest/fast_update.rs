use super::{Reuse, config, fixture::Fixture, graph, in_order, model, resolve_with};

use pnpm_lockfile::{Lockfile, LockfileFormat};
use proptest::prelude::*;

#[derive(Debug, Clone, Copy)]
enum Mutation {
    Remove,
    Add,
    Retarget,
}

fn mutate(workspace: &mut model::Workspace, mutation: Mutation, importer: usize, edge: usize) {
    let importer = importer % workspace.importers.len();
    let edges = &mut workspace.importers[importer];
    let picked = edges[edge % edges.len()].clone();
    match mutation {
        Mutation::Remove => edges.retain(|dependency| dependency.target != picked.target),
        Mutation::Add => {
            let target = (picked.target + 1) % workspace.packages.len();
            edges.retain(|dependency| dependency.target != target);
            edges.push(model::Edge { target, major: model::MAJORS[target % model::MAJORS.len()] });
        }
        Mutation::Retarget => {
            for dependency in edges
                .iter_mut()
                .filter(|dependency| dependency.target == picked.target)
            {
                dependency.major = if picked.major == 1 { 2 } else { 1 };
            }
        }
    }
}

fn fast_update(
    lockfile: &Lockfile,
    fixture: &Fixture,
    settings: model::Settings,
) -> Option<Lockfile> {
    let manifests: Vec<_> = fixture.manifests
        .iter()
        .map(|(id, manifest)| (id.clone(), manifest))
        .collect();
    let projects: Vec<_> = fixture.manifests
        .iter()
        .map(|(id, manifest)| (fixture.root.path().join(id), manifest))
        .collect();
    crate::fast_update_compose::try_compose_fast_updates(
        lockfile,
        &manifests,
        &projects,
        &fixture.config(settings),
        None,
        false,
    )
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn generated_peer_free_updates_use_fast_update(
        mut workspace in model::workspace(),
        next_format in any::<bool>(),
        mutation in prop::sample::select(vec![Mutation::Remove, Mutation::Add, Mutation::Retarget]),
    ) {
        for version in workspace.packages.iter_mut().flatten() {
            version.peers.clear();
        }
        workspace.importers = vec![vec![model::Edge { target: 0, major: 1 }]];
        for major in model::MAJORS {
            workspace.importers.push((0..workspace.packages.len())
                .map(|target| model::Edge { target, major }).collect());
        }
        let settings = model::Settings {
            format: if next_format { LockfileFormat::V13 } else { LockfileFormat::V9 },
            ..Default::default()
        };
        prop_assert!(check_update(workspace, settings, (mutation, 0, 0)));
    }

    #[test]
    fn accepted_fast_update_matches_full_resolution(
        workspace in model::fixed_provider_workspace(),
        settings in model::settings(),
        mutation in prop::sample::select(vec![Mutation::Remove, Mutation::Add, Mutation::Retarget]),
        importer in 0usize..3,
        edge in 0usize..3,
    ) {
        let settings = if settings.injected {
            model::Settings { format: LockfileFormat::V13, ..settings }
        } else { settings };
        check_update(workspace, settings, (mutation, importer, edge));
    }
}

#[test]
fn removal_of_peer_dependent_preserves_other_importer_children() {
    use model::{Edge, PackageVersion, Peer, PeerRange, Settings, Workspace};
    let empty = PackageVersion { dependencies: vec![], peers: vec![] };
    let workspace = Workspace {
        packages: vec![
            vec![
                PackageVersion {
                    dependencies: vec![Edge { target: 1, major: 2 }],
                    peers: vec![Peer {
                        target: 1,
                        range: PeerRange::Major(2),
                        optional: true,
                        also_dependency: false,
                    }],
                },
                empty.clone(),
            ],
            vec![empty.clone(), empty],
        ],
        importers: vec![
            vec![Edge { target: 0, major: 1 }, Edge { target: 1, major: 2 }],
            vec![Edge { target: 0, major: 1 }],
        ],
    };
    let settings = Settings { auto_install_peers: false, ..Default::default() };
    let order = in_order(&workspace);
    let initial = resolve_with(&workspace, &order, settings, Reuse::Fresh).unwrap();
    let lockfile = model::lockfile_with_settings(&initial, &workspace, settings);
    let mut changed = workspace;
    mutate(&mut changed, Mutation::Remove, 0, 0);
    let fixture = Fixture::new(&changed, settings);
    assert!(fast_update(&lockfile, &fixture, settings).is_none());
}

fn check_update(
    workspace: model::Workspace,
    settings: model::Settings,
    (mutation, importer, edge): (Mutation, usize, usize),
) -> bool {
    let order = in_order(&workspace);
    let initial = resolve_with(&workspace, &order, settings, Reuse::Fresh)
        .expect("initial workspace resolves");
    let lockfile = model::lockfile_with_settings(&initial, &workspace, settings);
    let mut changed = workspace;
    mutate(&mut changed, mutation, importer, edge);
    let fixture = Fixture::new(&changed, settings);
    if let Some(candidate) = fast_update(&lockfile, &fixture, settings) {
        let candidate = model::round_trip(&candidate);
        let full = resolve_with(&changed, &order, settings, Reuse::Install(&lockfile))
            .expect("changed workspace resolves");
        let expected = model::lockfile_with_settings(&full, &changed, settings);
        assert_eq!(fixture.frozen_check(&candidate, settings), Ok(()));
        graph::assert_equivalent(&candidate, &expected);
        true
    } else {
        false
    }
}

#[test]
fn peer_free_additions_removals_and_retargets_still_use_fast_update() {
    use model::{Edge, PackageVersion, Settings, Workspace};
    let empty = PackageVersion { dependencies: vec![], peers: vec![] };
    let workspace = Workspace {
        packages: vec![vec![empty; 2]; 2],
        importers: vec![
            vec![Edge { target: 0, major: 1 }],
            vec![Edge { target: 0, major: 2 }, Edge { target: 1, major: 2 }],
        ],
    };
    for format in [LockfileFormat::V9, LockfileFormat::V13] {
        for mutation in [Mutation::Add, Mutation::Remove, Mutation::Retarget] {
            let settings = Settings { format, ..Default::default() };
            assert!(
                check_update(workspace.clone(), settings, (mutation, 0, 0)),
                "{format:?}: {mutation:?}",
            );
        }
    }
}

#[test]
fn removing_required_peer_dependent_releases_optional_provider() {
    use model::{Edge, PackageVersion, Peer, PeerRange, Settings, Workspace};
    let empty = PackageVersion { dependencies: vec![], peers: vec![] };
    let dependent = |optional| PackageVersion {
        dependencies: vec![],
        peers: vec![Peer {
            target: 0,
            range: PeerRange::Major(1),
            optional,
            also_dependency: false,
        }],
    };
    let workspace = Workspace {
        packages: vec![
            vec![empty.clone(); 2],
            vec![empty.clone(), dependent(false)],
            vec![dependent(true), empty],
        ],
        importers: vec![vec![Edge { target: 2, major: 1 }], vec![Edge { target: 1, major: 2 }]],
    };
    let settings = Settings::default();
    assert!(!check_update(workspace, settings, (Mutation::Remove, 1, 0)));
}
