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
    Storage::new(
        &HostedStoreConfig::Fs,
        tmp.path().join("storage"),
        tmp.path().join("cache"),
        &pnpr_config::MetadataBackend::Sqlite(tmp.path().join("metadata.sqlite")),
    )
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
    // Separators of either platform never reach a path.
    let windows = format!(r"{}\{}", &DIGEST[..64], &DIGEST[65..]);
    assert!(
        storage
            .stage_archive(&key("alice", &windows), b"x".to_vec())
            .await
            .is_err(),
    );
    let nested = format!("{}/{}", &DIGEST[..64], &DIGEST[65..]);
    assert!(
        storage
            .stage_archive(&key("alice", &nested), b"x".to_vec())
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

#[tokio::test]
async fn a_reservation_never_hides_a_staged_archive() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let staged = key("alice", DIGEST);
    storage.stage_archive(&staged, b"bytes".to_vec()).await.unwrap();

    // Another upload of the same bytes, which started before this one
    // completed, reserves afterwards.
    assert!(!storage.reserve_staged_archive(&staged, 5).await.unwrap());
    assert_eq!(storage.staged_archive_size(&staged).await.unwrap(), Some(5));
    assert_eq!(
        storage
            .read_staged_archive(&staged)
            .await
            .unwrap()
            .as_deref(),
        Some(b"bytes".as_slice()),
    );
}

#[tokio::test]
async fn completing_an_upload_is_not_undone_by_an_older_completion() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let staged = key("alice", DIGEST);
    let now = unix_now();
    storage.stage_archive_at(&staged, b"bytes".to_vec(), now).await.unwrap();
    storage
        .stage_archive_at(&staged, b"bytes".to_vec(), now - 60)
        .await
        .unwrap();

    let record = storage
        .read_staged_archive_record(&staged)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((record.staged_at, record.complete), (now, true));
}

/// What one upload does once its bytes are verified: reserve, then store.
async fn upload(storage: &Storage, staged: &StagedArchiveKey<'_>, bytes: &[u8]) {
    storage
        .reserve_staged_archive(staged, bytes.len() as u64)
        .await
        .unwrap();
    storage.stage_archive(staged, bytes.to_vec()).await.unwrap();
}

#[tokio::test]
async fn parallel_uploads_and_a_sweep_leave_the_archive_staged() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let staged = key("alice", DIGEST);
    let other = key("alice", OTHER_DIGEST);
    for _ in 0..8 {
        let first = upload(&storage, &staged, b"bytes");
        let second = upload(&storage, &staged, b"bytes");
        let third = upload(&storage, &other, b"other");
        let sweep = async { storage.sweep_staged_archives().await.unwrap() };
        let usage = async { storage.staged_archive_usage("alice").await.unwrap() };
        let (..) = tokio::join!(first, second, third, sweep, usage);
        assert_eq!(storage.staged_archive_size(&staged).await.unwrap(), Some(5));
        assert_eq!(
            storage
                .read_staged_archive(&other)
                .await
                .unwrap()
                .as_deref(),
            Some(b"other".as_slice()),
        );
        assert_eq!(
            storage.staged_archive_usage("alice").await.unwrap(),
            StagedArchiveUsage { bytes: 10, archives: 2 },
        );
    }
}

#[tokio::test]
async fn a_record_that_does_not_parse_is_swept_with_its_archive() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let staged = key("alice", DIGEST);
    storage.stage_archive(&staged, b"bytes".to_vec()).await.unwrap();
    let namespace = staged.namespace();
    let record = staged.record().unwrap();
    let stored = storage.root
        .read_record(&namespace, &record)
        .await
        .unwrap()
        .unwrap();
    storage.root.replace_record_if_current(&namespace, &record, &stored, b"{").await.unwrap();

    assert_eq!(storage.sweep_staged_archives().await.unwrap(), 1);
    assert_eq!(storage.staged_archive_count("alice").await.unwrap(), 0);
}

#[tokio::test]
async fn retiring_a_claimed_archive_frees_it_unless_it_was_renewed() {
    let tmp = TempDir::new().unwrap();
    let storage = storage_in(&tmp);
    let staged = key("alice", DIGEST);
    storage.stage_archive(&staged, b"bytes".to_vec()).await.unwrap();

    storage.retire_staged_archive(&staged).await.unwrap();
    assert_eq!(storage.claimable_staged_archive_size(&staged).await.unwrap(), None);
    // Counting the principal's usage sweeps what is retired.
    assert_eq!(storage.staged_archive_usage("alice").await.unwrap(), StagedArchiveUsage::default());
    assert_eq!(storage.staged_archive_count("alice").await.unwrap(), 0);

    // Uploaded again after the commit read its record: the renewal stands.
    let renewed = key("alice", OTHER_DIGEST);
    storage.stage_archive(&renewed, b"other".to_vec()).await.unwrap();
    let namespace = renewed.namespace();
    let record = renewed.record().unwrap();
    let read = storage.root
        .read_record(&namespace, &record)
        .await
        .unwrap()
        .unwrap();
    storage
        .stage_archive_at(&renewed, b"other".to_vec(), unix_now() + 1)
        .await
        .unwrap();
    let retired = StagedArchiveRecord { staged_at: 0, complete: false, size: 5 };
    let written = storage.root
        .replace_record_if_current(
            &namespace,
            &record,
            &read,
            &serde_json::to_vec(&retired).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(written, crate::DocumentWrite::Conflict);
    assert_eq!(storage.claimable_staged_archive_size(&renewed).await.unwrap(), Some(5));
}
