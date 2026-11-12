use super::super::RebuildOptions;
use super::key;
use std::collections::HashSet;

#[test]
fn rebuild_selection_can_target_one_version_with_peer_suffixes() {
    let selection = RebuildOptions {
        selected_names: Some(HashSet::from(["actual-addon@1.0.0".to_owned()])),
        ..RebuildOptions::default()
    };

    assert!(selection.selects_snapshot(&key("actual-addon", "1.0.0(peer@2.0.0)")));
    assert!(!selection.selects_snapshot(&key("actual-addon", "2.0.0")));
}
