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
