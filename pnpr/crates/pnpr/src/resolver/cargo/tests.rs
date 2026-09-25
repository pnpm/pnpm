use super::{IndexFetcher, MAX_INDEX_TOTAL_BYTES, index_budget_has_room, over_index_budget};
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

    let lock = tokio::sync::Mutex::new(());
    let result =
        IndexFetcher::cached_or_evict(&stale_path, Duration::from_mins(1), &lock.lock().await)
            .await;

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

    let lock = tokio::sync::Mutex::new(());
    let result =
        IndexFetcher::cached_or_evict(&fresh_path, Duration::from_hours(1), &lock.lock().await)
            .await;

    assert_eq!(result.as_deref(), Some("fresh contents"));
    assert!(fresh_path.exists(), "fresh entry must remain on disk");
}
