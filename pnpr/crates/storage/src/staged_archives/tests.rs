use super::{
    INCOMPLETE_MAX_AGE, STAGED_ARCHIVE_MAX_AGE, SWEEP_GRACE, StagedArchiveKey, StagedArchiveRecord,
    StagedArchiveUsage, unix_now,
};
use crate::Storage;
use pnpr_config::HostedStoreConfig;
use tempfile::TempDir;

const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\
                      0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OTHER_DIGEST: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210\
                            fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

fn storage_in(tmp: &TempDir) -> Storage {
    Storage::new(&HostedStoreConfig::Fs, tmp.path().join("storage"), tmp.path().join("cache"))
        .unwrap()
}

fn key<'a>(principal: &'a str, digest: &'a str) -> StagedArchiveKey<'a> {
    StagedArchiveKey { principal, digest }
}

#[tokio::test]
async fn a_staged_archive_is_claimable_only_by_its_principal() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    assert!(
        storage
            .stage_archive(&key("alice", DIGEST), b"bytes".to_vec())
            .await
            .unwrap(),
    );

    assert_eq!(
        storage
            .read_staged_archive(&key("alice", DIGEST))
            .await
            .unwrap()
            .as_deref(),
        Some(b"bytes".as_slice()),
    );
    assert_eq!(
        storage
            .staged_archive_size(&key("alice", DIGEST))
            .await
            .unwrap(),
        Some(5),
    );
    assert_eq!(
        storage
            .read_staged_archive(&key("bob", DIGEST))
            .await
            .unwrap(),
        None,
    );
    assert_eq!(
        storage
            .staged_archive_size(&key("bob", DIGEST))
            .await
            .unwrap(),
        None,
    );
}

#[tokio::test]
async fn staging_an_archive_again_renews_it_and_reports_no_new_archive() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let old = unix_now() - STAGED_ARCHIVE_MAX_AGE.as_secs() + 60;
    assert!(
        storage
            .stage_archive_at(&key("alice", DIGEST), b"bytes".to_vec(), old)
            .await
            .unwrap(),
    );
    // Too close to its expiry to count as stored for a publisher deciding
    // what to upload.
    assert_eq!(
        storage
            .staged_archive_size(&key("alice", DIGEST))
            .await
            .unwrap(),
        None,
    );

    assert!(
        !storage
            .stage_archive(&key("alice", DIGEST), b"bytes".to_vec())
            .await
            .unwrap(),
    );

    assert_eq!(
        storage
            .staged_archive_size(&key("alice", DIGEST))
            .await
            .unwrap(),
        Some(5),
    );
}

#[tokio::test]
async fn an_expired_archive_cannot_be_claimed_and_is_swept() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let now = unix_now();
    let expired = now - STAGED_ARCHIVE_MAX_AGE.as_secs();
    storage
        .stage_archive_at(&key("alice", DIGEST), b"old".to_vec(), expired)
        .await
        .unwrap();
    storage
        .stage_archive_at(&key("alice", OTHER_DIGEST), b"new".to_vec(), now)
        .await
        .unwrap();

    assert_eq!(
        storage
            .read_staged_archive(&key("alice", DIGEST))
            .await
            .unwrap(),
        None,
    );
    // Within the sweep's grace, an expired archive stays in place.
    assert_eq!(storage.sweep_staged_archives().await.unwrap(), 0);

    let collectable = now + SWEEP_GRACE.as_secs();
    assert_eq!(storage.sweep_staged_archives_at(collectable).await.unwrap(), 1);
    assert!(
        storage.root
            .list_record_keys(&key("alice", DIGEST).namespace())
            .await
            .unwrap()
            .iter()
            .all(|stored| !stored.starts_with(DIGEST)),
    );
    assert_eq!(
        storage
            .read_staged_archive(&key("alice", OTHER_DIGEST))
            .await
            .unwrap()
            .as_deref(),
        Some(b"new".as_slice()),
    );
}

#[tokio::test]
async fn usage_counts_only_claimable_archives_of_the_principal() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let now = unix_now();
    let collectable = now - STAGED_ARCHIVE_MAX_AGE.as_secs() - SWEEP_GRACE.as_secs();
    storage
        .stage_archive_at(&key("alice", DIGEST), vec![0; 7], collectable)
        .await
        .unwrap();
    storage
        .stage_archive_at(&key("alice", OTHER_DIGEST), vec![0; 3], now)
        .await
        .unwrap();
    storage
        .stage_archive_at(&key("bob", DIGEST), vec![0; 11], now)
        .await
        .unwrap();

    assert_eq!(
        storage.staged_archive_usage("alice").await.unwrap(),
        StagedArchiveUsage { bytes: 3, archives: 1 },
    );
    // The expired archive went with the usage check.
    assert_eq!(
        storage.root
            .list_record_keys(&key("alice", DIGEST).namespace())
            .await
            .unwrap()
            .len(),
        2,
    );
    assert_eq!(
        storage.staged_archive_usage("bob").await.unwrap(),
        StagedArchiveUsage { bytes: 11, archives: 1 },
    );
}

#[tokio::test]
async fn an_archive_without_its_record_is_swept() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let staged = key("alice", DIGEST);
    storage.root
        .create_record(&staged.namespace(), DIGEST, b"orphan")
        .await
        .unwrap();

    assert_eq!(storage.sweep_staged_archives().await.unwrap(), 1);
    assert_eq!(
        storage.root
            .list_record_keys(&staged.namespace())
            .await
            .unwrap(),
        Vec::<String>::new(),
    );
}

#[tokio::test]
async fn an_invalid_digest_is_refused() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    assert!(
        storage
            .stage_archive(&key("alice", "../escape"), b"x".to_vec())
            .await
            .is_err(),
    );
    assert!(
        storage
            .read_staged_archive(&key("alice", "ABC"))
            .await
            .is_err(),
    );
}

#[tokio::test]
async fn a_reservation_counts_against_its_principal_but_is_not_staged() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let staged = key("alice", DIGEST);
    storage.reserve_staged_archive(&staged, 9).await.unwrap();

    assert_eq!(storage.staged_archive_size(&staged).await.unwrap(), None);
    assert_eq!(storage.read_staged_archive(&staged).await.unwrap(), None);
    assert_eq!(
        storage.staged_archive_usage("alice").await.unwrap(),
        StagedArchiveUsage { bytes: 9, archives: 1 },
    );
    assert_eq!(storage.staged_archive_count("alice").await.unwrap(), 1);

    storage.release_staged_archive(&staged).await.unwrap();
    assert_eq!(storage.staged_archive_usage("alice").await.unwrap(), StagedArchiveUsage::default());
}

#[tokio::test]
async fn releasing_leaves_an_archive_another_upload_completed() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let staged = key("alice", DIGEST);
    storage.reserve_staged_archive(&staged, 5).await.unwrap();
    storage.stage_archive(&staged, b"bytes".to_vec()).await.unwrap();

    storage.release_staged_archive(&staged).await.unwrap();
    assert_eq!(storage.staged_archive_size(&staged).await.unwrap(), Some(5));
}

#[tokio::test]
async fn an_upload_that_stopped_stops_counting_and_is_swept() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let staged = key("alice", DIGEST);
    let stopped = unix_now() - INCOMPLETE_MAX_AGE.as_secs();
    let record = StagedArchiveRecord { staged_at: stopped, size: 9, complete: false };
    storage.write_staged_archive_record(&staged, &record).await.unwrap();

    assert_eq!(storage.staged_archive_usage("alice").await.unwrap(), StagedArchiveUsage::default());
    let collectable = unix_now() + SWEEP_GRACE.as_secs();
    storage.sweep_staged_archives_at(collectable).await.unwrap();
    assert_eq!(storage.staged_archive_count("alice").await.unwrap(), 0);
}

#[tokio::test]
async fn a_sweep_leaves_what_is_not_an_archive_alone() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let staged = key("alice", DIGEST);
    // The temporary file of a write in flight.
    let temporary = format!("{DIGEST}.tmp.1.2.0000000000000003");
    storage.root
        .create_record(&staged.namespace(), &temporary, b"partial")
        .await
        .unwrap();

    assert_eq!(storage.sweep_staged_archives().await.unwrap(), 0);
    assert_eq!(
        storage.root
            .list_record_keys(&staged.namespace())
            .await
            .unwrap(),
        vec![temporary],
    );
}
