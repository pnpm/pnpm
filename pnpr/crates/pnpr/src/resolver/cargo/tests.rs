use super::{MAX_INDEX_TOTAL_BYTES, over_index_budget};

#[test]
fn the_index_budget_covers_every_entry_a_resolve_holds() {
    assert_eq!(over_index_budget(MAX_INDEX_TOTAL_BYTES, "serde"), None);
    let exhausted = over_index_budget(MAX_INDEX_TOTAL_BYTES + 1, "serde")
        .expect("one byte past the budget is refused");
    assert!(exhausted.contains("serde"), "{exhausted}");
}
