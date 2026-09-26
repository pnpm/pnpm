use super::{
    IndexFetcher, LockedEntry, MAX_INDEX_TOTAL_BYTES, index_budget_has_room, over_index_budget,
};
use crate::server::StripedLocks;
use std::{
    fs::File,
    io::Write,
    time::{Duration, SystemTime},
};

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

fn write_with_mtime(path: &std::path::Path, contents: &str, mtime: SystemTime) {
    let mut file = File::create(path).unwrap();
    file.write_all(contents.as_bytes()).unwrap();
    file.set_modified(mtime).unwrap();
}

#[tokio::test]
async fn cached_deletes_stale_entries() {
    let dir = tempfile::tempdir().unwrap();
    let stale_path = dir.path().join("stale-entry");
    let one_hour_ago = SystemTime::now() - Duration::from_hours(1);
    write_with_mtime(&stale_path, "old contents", one_hour_ago);

    let locks = StripedLocks::new();
    let entry = LockedEntry::lock(&locks, stale_path.clone()).await;
    let result = entry.cached_or_evict(Duration::from_mins(1)).await;

    assert!(result.is_none(), "stale entry must be a cache miss");
    assert!(!stale_path.exists(), "stale entry must be deleted from disk");
}

#[tokio::test]
async fn cached_unlocked_leaves_stale_entries() {
    let dir = tempfile::tempdir().unwrap();
    let stale_path = dir.path().join("stale-unlocked");
    let one_hour_ago = SystemTime::now() - Duration::from_hours(1);
    write_with_mtime(&stale_path, "old contents", one_hour_ago);

    let result = IndexFetcher::cached_entry(&stale_path, Duration::from_mins(1), false).await;

    assert!(result.is_none(), "stale entry must be a cache miss");
    assert!(stale_path.exists(), "unlocked check must preserve stale entry on disk");
}

#[tokio::test]
async fn cached_returns_fresh_entries() {
    let dir = tempfile::tempdir().unwrap();
    let fresh_path = dir.path().join("fresh-entry");
    tokio::fs::write(&fresh_path, "fresh contents").await.unwrap();

    let locks = StripedLocks::new();
    let entry = LockedEntry::lock(&locks, fresh_path.clone()).await;
    let result = entry.cached_or_evict(Duration::from_hours(1)).await;

    assert_eq!(result.as_deref(), Some("fresh contents"));
    assert!(fresh_path.exists(), "fresh entry must remain on disk");
}

/// Two callers miss a stale entry together. The one holding the lock evicts
/// and refreshes it; the waiting one must then read the refreshed entry and
/// leave it on disk rather than evict it again.
#[tokio::test]
async fn a_waiting_caller_reads_the_entry_the_lock_holder_refreshed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("contended-entry");
    write_with_mtime(&path, "old contents", SystemTime::now() - Duration::from_hours(1));
    let locks = std::sync::Arc::new(StripedLocks::new());
    let ttl = Duration::from_mins(1);

    let first = LockedEntry::lock(&locks, path.clone()).await;
    assert!(first.cached_or_evict(ttl).await.is_none());
    let waiting = tokio::spawn({
        let locks = std::sync::Arc::clone(&locks);
        let path = path.clone();
        async move {
            let entry = LockedEntry::lock(&locks, path).await;
            entry.cached_or_evict(ttl).await
        }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!waiting.is_finished(), "the waiting caller must block on the held lock");
    IndexFetcher::store(first.path().to_path_buf(), "fresh contents".to_string()).await;
    drop(first);

    assert_eq!(waiting.await.unwrap().as_deref(), Some("fresh contents"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "fresh contents");
}
