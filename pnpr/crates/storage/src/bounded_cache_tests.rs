use super::BoundedCache;

#[test]
fn evicts_oldest_entries_to_fit_the_byte_budget() {
    let mut cache = BoundedCache::default();
    cache.insert("first", "first value", 4, 10);
    cache.insert("second", "second value", 4, 10);
    cache.insert("third", "third value", 4, 10);
    assert_eq!(cache.get(&"first"), None);
    assert_eq!(cache.get(&"second"), Some(&"second value"));
    assert_eq!(cache.get(&"third"), Some(&"third value"));
}

#[test]
fn replacing_an_entry_updates_its_weight_and_insertion_order() {
    let mut cache = BoundedCache::default();
    cache.insert("first", 1, 4, 10);
    cache.insert("second", 2, 4, 10);
    cache.insert("first", 3, 5, 10);
    cache.insert("third", 4, 4, 10);
    assert_eq!(cache.get(&"first"), Some(&3));
    assert_eq!(cache.get(&"second"), None);
    assert_eq!(cache.get(&"third"), Some(&4));
}

#[test]
fn oversized_entries_do_not_evict_existing_values() {
    let mut cache = BoundedCache::default();
    cache.insert("small", (), 4, 10);
    cache.insert("large", (), 11, 10);
    assert_eq!(cache.get(&"large"), None);
    assert_eq!(cache.get(&"small"), Some(&()));
}
