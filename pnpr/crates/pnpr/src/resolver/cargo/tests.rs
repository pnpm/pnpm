use super::{MAX_INDEX_TOTAL_BYTES, index_budget_has_room, over_index_budget};

#[test]
fn the_index_budget_covers_every_entry_a_resolve_holds() {
    assert_eq!(over_index_budget(MAX_INDEX_TOTAL_BYTES, "serde"), None);
    let exhausted = over_index_budget(MAX_INDEX_TOTAL_BYTES + 1, "serde")
        .expect("one byte past the budget is refused");
    assert!(exhausted.contains("serde"), "{exhausted}");
}

#[test]
fn a_full_budget_leaves_no_room_for_another_entry() {
    assert!(index_budget_has_room(MAX_INDEX_TOTAL_BYTES - 1));
    assert!(!index_budget_has_room(MAX_INDEX_TOTAL_BYTES));
}
