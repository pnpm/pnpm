use super::{extend_preference_seeds, targets};
use pnpm_config::Config;
use pnpm_lockfile::Lockfile;
use pnpm_resolving_deps_resolver::UpdateTargets;
use pnpm_resolving_resolver_base::{
    PreferredVersions, VersionSelectorEntry, VersionSelectorType, VersionSelectorWithWeight,
};
use std::{collections::BTreeMap, sync::Arc};

#[test]
fn automatic_dedupe_preserves_shared_seeds_and_stronger_importer_preferences() {
    let mut preferred = Arc::new(PreferredVersions::default());
    let mut importers: BTreeMap<_, _> = (0..1000)
        .map(|index| (index.to_string(), Arc::clone(&preferred)))
        .collect();
    let strong = VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
        selector_type: VersionSelectorType::Version,
        weight: pnpm_resolving_resolver_base::EXISTING_VERSION_SELECTOR_WEIGHT + 1,
    });
    importers.insert(
        "specific".into(),
        Arc::new(BTreeMap::from_iter([(
            "foo".into(),
            BTreeMap::from_iter([("1.0.0".into(), strong.clone())]),
        )])),
    );
    let versions = BTreeMap::from_iter(
        [("foo", ["1.0.0", "1.5.0"]), ("bar", ["2.0.0", "2.5.0"])].map(|(name, versions)| {
            (
                name.into(),
                versions
                    .into_iter()
                    .map(|version| {
                        (version.into(), VersionSelectorEntry::Plain(VersionSelectorType::Version))
                    })
                    .collect(),
            )
        }),
    );
    let mut targets = UpdateTargets::default();
    assert!(extend_preference_seeds(&versions, &mut preferred, &mut importers, &mut targets));
    assert!((0..1000).all(|index| Arc::ptr_eq(&preferred, &importers[&index.to_string()])));
    assert!(!Arc::ptr_eq(&preferred, &importers["specific"]));
    assert_eq!(importers["specific"]["foo"]["1.0.0"], strong);
    assert!(targets.covers("foo", None));
    assert!(targets.covers("bar", None));
    let settled = Arc::clone(&preferred);
    assert!(!extend_preference_seeds(&versions, &mut preferred, &mut importers, &mut targets));
    assert!(Arc::ptr_eq(&preferred, &settled));
}

#[test]
fn automatic_dedupe_targets_actual_versions_not_peer_snapshots() {
    let lockfile: Lockfile = serde_saphyr::from_str(
        r"
lockfileVersion: '12.0'
importers: {}
packages:
  foo@1.0.0: {resolution: {integrity: sha512-AAAA}}
  bar@1.0.0: {resolution: {integrity: sha512-AAAA}}
  bar@2.0.0: {resolution: {integrity: sha512-AAAA}}
snapshots:
  foo@1.0.0(peer@1.0.0): {}
  foo@1.0.0(peer@2.0.0): {}
  bar@1.0.0: {}
  bar@2.0.0: {}
",
    )
    .unwrap();
    let config = Config { auto_dedupe: true, ..Config::default() };
    let candidates = targets(&config, Some(&lockfile), true);
    assert!(!candidates.covers("foo", None));
    assert!(candidates.covers("bar", None));
    assert!(targets(&config, Some(&lockfile), false).covers("foo", None));
    assert!(!targets(&Config::default(), Some(&lockfile), false).covers("bar", None));
}
