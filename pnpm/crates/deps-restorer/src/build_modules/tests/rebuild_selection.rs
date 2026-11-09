use super::super::RebuildOptions;
use pnpm_lockfile::PackageKey;
use std::collections::HashSet;

#[test]
fn exact_rebuild_selection_keeps_other_versions_and_peer_variants_unselected() {
    let selected = "example@1.0.0(peer@1.0.0)";
    let options = RebuildOptions {
        selected_snapshots: Some(HashSet::from([selected.parse().unwrap()])),
        ..Default::default()
    };
    for key in [selected, "example@1.0.0(peer@2.0.0)", "example@2.0.0", "other@1.0.0"] {
        let snapshot: PackageKey = key.parse().unwrap();
        assert_eq!(options.selects_snapshot(&snapshot), key == selected);
        assert_eq!(options.settles_dependency(key), key == selected);
    }
}

#[test]
fn name_rebuild_selection_keeps_all_matching_versions_and_peer_variants() {
    let options = RebuildOptions {
        selected_names: Some(HashSet::from(["example".to_owned()])),
        ..Default::default()
    };
    for key in
        ["example@1.0.0(peer@1.0.0)", "example@1.0.0(peer@2.0.0)", "example@2.0.0", "other@1.0.0"]
    {
        let snapshot: PackageKey = key.parse().unwrap();
        assert_eq!(options.selects_snapshot(&snapshot), key.starts_with("example@"));
        assert_eq!(options.settles_dependency(key), key.starts_with("example@"));
    }
}
