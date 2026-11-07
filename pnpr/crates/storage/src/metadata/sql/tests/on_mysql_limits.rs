use super::{Db, storage};
use crate::RegistryRecord;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires PNPR_TEST_MYSQL_URL"]
async fn oversized_keys_fail_without_truncating_or_advancing_generation() {
    let db = Db::network("PNPR_TEST_MYSQL_URL").await;
    let storage = storage(&db);
    let kind = RegistryRecord::TeamRoster;
    let boundary = "x".repeat(512);
    assert!(storage.create_registry_record(kind, &boundary, b"original").await.unwrap());
    let original = storage
        .read_registry_record(kind, &boundary)
        .await
        .unwrap()
        .unwrap();
    assert!(
        storage
            .create_registry_record(kind, &format!("{boundary}x"), b"replacement")
            .await
            .is_err(),
    );
    assert_eq!(
        storage
            .read_registry_record(kind, &boundary)
            .await
            .unwrap()
            .unwrap()
            .bytes,
        b"original",
    );
    assert!(storage.create_registry_record(kind, "next", b"next").await.unwrap());
    let next = storage
        .read_registry_record(kind, "next")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next.generation, original.generation + 1);
}
