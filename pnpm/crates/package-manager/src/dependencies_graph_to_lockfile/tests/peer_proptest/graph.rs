use pnpm_lockfile::{BoundProviders, Lockfile, PkgName, PkgNameVerPeer};
use std::collections::{BTreeMap, HashSet};

type Edges = BTreeMap<String, PkgNameVerPeer>;

// Deduplication may select another snapshot key with the same wiring after a removal.
pub(super) fn assert_equivalent(actual: &Lockfile, expected: &Lockfile) {
    assert_eq!(actual.packages, expected.packages);
    assert_eq!(
        actual.importers.keys().collect::<HashSet<_>>(),
        expected.importers.keys().collect(),
    );
    let mut pending = Vec::new();
    for (id, expected_importer) in &expected.importers {
        let actual_importer = &actual.importers[id];
        for (actual_group, expected_group) in [
            (&actual_importer.dependencies, &expected_importer.dependencies),
            (&actual_importer.dev_dependencies, &expected_importer.dev_dependencies),
            (&actual_importer.optional_dependencies, &expected_importer.optional_dependencies),
        ] {
            pair_edges(
                importer_edges(actual_group.as_ref()),
                importer_edges(expected_group.as_ref()),
                &mut pending,
            );
        }
    }
    compare_snapshots(actual, expected, pending);
}

fn compare_snapshots(
    actual: &Lockfile,
    expected: &Lockfile,
    mut pending: Vec<(PkgNameVerPeer, PkgNameVerPeer)>,
) {
    let mut visited = HashSet::new();
    while let Some((actual_key, expected_key)) = pending.pop() {
        assert_eq!(actual_key.without_peer(), expected_key.without_peer());
        if !visited.insert((actual_key.clone(), expected_key.clone())) {
            continue;
        }
        assert_peer_bindings(actual, expected, &actual_key, &expected_key);
        let actual_snapshot = &actual.snapshots.as_ref().unwrap()[&actual_key];
        let expected_snapshot = &expected.snapshots.as_ref().unwrap()[&expected_key];
        assert_eq!(actual_snapshot.optional, expected_snapshot.optional);
        for (actual_group, expected_group) in [
            (&actual_snapshot.dependencies, &expected_snapshot.dependencies),
            (&actual_snapshot.optional_dependencies, &expected_snapshot.optional_dependencies),
        ] {
            pair_edges(
                snapshot_edges(actual_group.as_ref()),
                snapshot_edges(expected_group.as_ref()),
                &mut pending,
            );
        }
    }
}
fn assert_peer_bindings(
    actual: &Lockfile,
    expected: &Lockfile,
    actual_key: &PkgNameVerPeer,
    expected_key: &PkgNameVerPeer,
) {
    let actual_bindings = BoundProviders::of_snapshot(actual.into(), actual_key);
    let expected_bindings = BoundProviders::of_snapshot(expected.into(), expected_key);
    if actual_bindings.is_hashed() || expected_bindings.is_hashed() {
        assert_eq!(actual_key, expected_key, "unrecovered peer bindings must have the same key");
    } else {
        assert_eq!(actual_bindings, expected_bindings, "peer providers differ");
    }
}

fn pair_edges(actual: Edges, expected: Edges, pending: &mut Vec<(PkgNameVerPeer, PkgNameVerPeer)>) {
    assert_eq!(actual.keys().collect::<Vec<_>>(), expected.keys().collect::<Vec<_>>());
    pending.extend(actual.into_values().zip(expected.into_values()));
}

fn importer_edges(group: Option<&pnpm_lockfile::ResolvedDependencyMap>) -> Edges {
    group
        .into_iter()
        .flatten()
        .map(|(alias, dependency)| {
            (alias.to_string(), dependency.version.resolved_key(alias).unwrap())
        })
        .collect()
}

fn snapshot_edges(
    group: Option<&std::collections::HashMap<PkgName, pnpm_lockfile::SnapshotDepRef>>,
) -> Edges {
    group
        .into_iter()
        .flatten()
        .map(|(alias, dependency)| (alias.to_string(), dependency.resolve(alias).unwrap()))
        .collect()
}

#[test]
fn oracle_rejects_different_peer_bindings_without_child_edges() {
    let lockfile = |major| {
        serde_saphyr::from_str::<Lockfile>(&format!(
            r"lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      consumer:
        specifier: 1.0.0
        version: 1.0.0(provider@{major}.0.0)
snapshots:
  consumer@1.0.0(provider@{major}.0.0): {{}}
",
        ))
        .unwrap()
    };
    assert!(std::panic::catch_unwind(|| assert_equivalent(&lockfile(1), &lockfile(2))).is_err());
}
