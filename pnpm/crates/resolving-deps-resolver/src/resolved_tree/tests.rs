use super::AncestorIds;
use std::sync::Arc;

fn ancestors(base: &[&str], appended: &[&str]) -> AncestorIds {
    appended.iter().fold(
        AncestorIds::from(Arc::new(base.iter().map(ToString::to_string).collect())),
        |ids, id| ids.pushed((*id).to_string()),
    )
}

#[test]
fn detects_cycle_sequences_across_base_and_appended_ids() {
    let ids = ancestors(&["a", "b", "c"], &["d", "b", "e"]);

    assert!(ids.forms_cycle("a", "c"));
    assert!(ids.forms_cycle("c", "d"));
    assert!(ids.forms_cycle("d", "e"));
    assert!(ids.forms_cycle("b", "b"));
    assert!(!ids.forms_cycle("d", "c"));
    assert!(!ids.forms_cycle("e", "a"));
    assert!(!ids.forms_cycle("missing", "e"));
}

/// The peer walk realizes one of these per distinct root-to-package
/// path — millions of them on a workspace with a large, cyclic peer
/// graph — so an extra inline field is not a few bytes but a few
/// hundred megabytes. Wanted-lockfile carry-over lives behind
/// [`super::LockedResolution`] for that reason; keep it there.
///
/// 48 bytes is the current layout on a 64-bit target, not a budget with
/// room in it: inlining one more `Option<String>` would cost 24.
#[test]
fn tree_node_keeps_its_per_occurrence_footprint_small() {
    assert!(
        size_of::<super::DependenciesTreeNode>() <= 48,
        "DependenciesTreeNode grew to {} bytes; box rarely-set state instead of inlining it",
        size_of::<super::DependenciesTreeNode>(),
    );
}

#[test]
fn locked_resolution_is_unallocated_until_written() {
    let mut node = super::DependenciesTreeNode::new(
        Arc::from("a@1.0.0".to_string()),
        super::TreeChildren::Realized(Arc::new(std::collections::BTreeMap::new())),
        0,
        true,
    );

    assert!(node.locked.is_none(), "a fresh resolution carries no wanted-lockfile state");
    assert!(node.previous_dep_path().is_none());
    assert!(node.locked_peer_context().is_none());
    assert!(node.must_win_dependency_names().is_none());
    assert!(node.locked_peer_names().is_none());
    assert!(node.has_no_locked_peers());

    node.locked_mut().previous_dep_path = Some(pacquet_deps_path::DepPath::from("a@1.0.0"));

    assert_eq!(
        node.previous_dep_path(),
        Some(&pacquet_deps_path::DepPath::from("a@1.0.0")),
        "the accessor reads back what `locked_mut` wrote",
    );
    assert!(node.has_no_locked_peers(), "a previous depPath is not a locked peer");
}
