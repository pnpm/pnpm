use super::{ProviderFinder, SnapshotDepRef};
use crate::Lockfile;

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
    for root in ["first@1.0.0", "second@1.0.0"] {
        let (root, _) = snapshots
            .get_key_value(&root.parse().unwrap())
            .unwrap();
        for version in ["1.0.0", "2.0.0", "3.0.0"] {
            let expected: SnapshotDepRef = version.parse().unwrap();
            let found = finder.provider_below(root, "provider", |reference| *reference == expected);
            assert_eq!(found, (version != "3.0.0").then_some(&expected));
        }
        assert_eq!(finder.provider_below(root, "missing", |_| true), None);
    }
}
