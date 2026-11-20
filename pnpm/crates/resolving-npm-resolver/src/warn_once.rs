//! Bounded warn-once sets for resolver warnings that would otherwise repeat
//! for every dependency edge that resolves the same package.

use std::sync::Mutex;

/// Records `key` in `warned` and reports whether it was not there yet, so the
/// caller emits its warning only the first time. Once the set holds
/// `capacity` keys the oldest one is evicted, which bounds the memory a
/// long-lived process spends on it.
pub(crate) fn first_warning(
    warned: &Mutex<indexmap::IndexSet<String>>,
    key: String,
    capacity: usize,
) -> bool {
    let mut warned = warned.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if warned.contains(&key) {
        return false;
    }
    if warned.len() >= capacity {
        warned.shift_remove_index(0);
    }
    warned.insert(key)
}
