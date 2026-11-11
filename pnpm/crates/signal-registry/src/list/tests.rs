use super::SignalList;

#[test]
fn a_walk_reaches_every_pushed_entry() {
    static LIST: SignalList<u32> = SignalList::new();
    for value in 1..=3 {
        LIST.push(value);
    }
    let mut seen = Vec::new();
    LIST.for_each(|entry| seen.push(*entry));
    seen.sort_unstable();
    assert_eq!(seen, [1, 2, 3]);
}

/// Pins that a pushed entry's reference outlives the push, which is what
/// lets a caller keep it in a guard.
#[test]
fn a_pushed_entry_is_borrowed_for_the_process() {
    static LIST: SignalList<u32> = SignalList::new();
    let entry: &'static u32 = LIST.push(7);
    LIST.push(8);
    assert_eq!(*entry, 7);
}

/// Pins the reuse that bounds how far the list grows: a claim that accepts
/// hands back the existing entry rather than appending another.
#[test]
fn a_claimed_entry_is_reused_instead_of_appended() {
    static LIST: SignalList<std::sync::atomic::AtomicBool> = SignalList::new();
    use std::sync::atomic::Ordering;

    let take = |entry: &std::sync::atomic::AtomicBool| {
        entry.compare_exchange(false, true, Ordering::SeqCst, Ordering::Acquire).is_ok()
    };
    let first = LIST.claim_or_push(take, || std::sync::atomic::AtomicBool::new(true));
    first.store(false, Ordering::SeqCst);
    let second = LIST.claim_or_push(take, || std::sync::atomic::AtomicBool::new(true));
    assert!(std::ptr::eq(first, second), "the free entry was not reused");

    let mut entries = 0;
    LIST.for_each(|_| entries += 1);
    assert_eq!(entries, 1);
}

/// Pins that a claim nobody accepts appends, so a second concurrent holder
/// gets an entry of its own rather than sharing one.
#[test]
fn a_rejected_claim_appends_an_entry() {
    static LIST: SignalList<u32> = SignalList::new();
    LIST.claim_or_push(|_| false, || 1);
    LIST.claim_or_push(|_| false, || 2);
    let mut entries = 0;
    LIST.for_each(|_| entries += 1);
    assert_eq!(entries, 2);
}
