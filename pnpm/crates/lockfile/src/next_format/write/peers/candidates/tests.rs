use super::{
    MAX_CACHED_BINDINGS, ProviderCandidates, ProviderConstraint, ProviderFinder, SnapshotDepRef,
};
use crate::Lockfile;
use std::collections::HashMap;

#[test]
fn distinct_provider_chain_keeps_candidate_storage_linear() {
    const LENGTH: usize = 3000;
    let key = |version| format!("node-{version}@1.0.0(provider@{version}.0.0)").parse().unwrap();
    let mut snapshots = HashMap::new();
    for version in 1..=LENGTH {
        let mut dependencies = HashMap::from([(
            "provider".parse().unwrap(),
            format!("{version}.0.0").parse().unwrap(),
        )]);
        if version < LENGTH {
            dependencies.insert("next".parse().unwrap(), SnapshotDepRef::Alias(key(version + 1)));
        }
        snapshots.insert(
            key(version),
            crate::SnapshotEntry { dependencies: Some(dependencies), ..Default::default() },
        );
    }
    let own_peers = snapshots
        .iter()
        .map(|(key, entry)| (key, super::super::own_peer_names(key, entry, None, None)))
        .collect();
    let finder = ProviderFinder::new(&snapshots, &own_peers);
    let mut candidates = ProviderCandidates::of(&finder, "provider");
    assert_eq!(
        candidates.components
            .iter()
            .map(|component| component.references.len())
            .sum::<usize>(),
        LENGTH,
    );
    assert_eq!(
        candidates.components
            .iter()
            .map(|component| component.descendants.len())
            .sum::<usize>(),
        LENGTH - 1,
    );
    for version in (1..=MAX_CACHED_BINDINGS + 2).chain([LENGTH, 1]) {
        let constraint = ProviderConstraint::new(
            &"provider".parse().unwrap(),
            Some(&[format!("provider@{version}.0.0")]),
            None,
        );
        let expected: SnapshotDepRef = format!("{version}.0.0").parse().unwrap();
        assert_eq!(candidates.matching(&key(1), &constraint), Some(&expected));
        assert!(candidates.cached.len() <= MAX_CACHED_BINDINGS);
        assert!(
            candidates.cached
                .iter()
                .map(|cached| cached.providers.len())
                .sum::<usize>()
                <= MAX_CACHED_BINDINGS * LENGTH,
        );
    }
}

#[test]
fn shared_subtrees_and_cycles_find_each_binding_and_missing_peers() {
    let lockfile: Lockfile = serde_saphyr::from_str(
        r"lockfileVersion: '9.0'
snapshots:
  first@1.0.0:
    dependencies:
      second: 1.0.0
      left: 1.0.0(provider@1.0.0)
    transitivePeerDependencies: [provider, missing]
  second@1.0.0:
    dependencies:
      first: 1.0.0
      right: 1.0.0(provider@2.0.0)
    transitivePeerDependencies: [provider, missing]
  left@1.0.0(provider@1.0.0):
    dependencies:
      provider: 1.0.0
  right@1.0.0(provider@2.0.0):
    dependencies:
      provider: 2.0.0
",
    )
    .unwrap();
    let mut snapshots = lockfile.snapshots.unwrap();
    let shared = snapshots[&"first@1.0.0".parse().unwrap()].clone();
    for index in 0..512 {
        snapshots.insert(format!("parent-{index}@1.0.0").parse().unwrap(), shared.clone());
    }
    let own_peers = snapshots
        .iter()
        .map(|(key, entry)| (key, super::super::own_peer_names(key, entry, None, None)))
        .collect();
    let mut finder = ProviderFinder::new(&snapshots, &own_peers);
    for (root, _) in
        snapshots.iter().filter(|(_, entry)| entry.transitive_peer_dependencies.is_some())
    {
        for version in ["1.0.0", "2.0.0", "3.0.0"] {
            let expected: SnapshotDepRef = version.parse().unwrap();
            let constraint = ProviderConstraint::new(
                &"provider".parse().unwrap(),
                Some(&[format!("provider@{version}")]),
                None,
            );
            let found = finder.provider_below(root, "provider", &constraint);
            assert_eq!(found, (version != "3.0.0").then_some(&expected));
        }
        let missing = ProviderConstraint::new(&"missing".parse().unwrap(), None, None);
        assert_eq!(finder.provider_below(root, "missing", &missing), None);
    }
}
