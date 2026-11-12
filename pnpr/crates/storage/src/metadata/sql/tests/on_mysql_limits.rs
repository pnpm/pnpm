use super::{Db, storage};
use crate::metadata::{NewRegistryGraph, RegistryGraphWrite};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires PNPR_TEST_MYSQL_URL"]
async fn oversized_keys_fail_without_truncating_or_advancing_generation() {
    let db = Db::network("PNPR_TEST_MYSQL_URL").await;
    let storage = storage(&db);
    let boundary = "x".repeat(512);
    let revision =
        |actor_id| NewRegistryGraph { document: "{}", actor_id, restored_from: None, vacated: &[] };
    let first = storage
        .write_registry_graph(None, revision(&boundary))
        .await
        .unwrap();
    assert!(matches!(first, RegistryGraphWrite::Written(1)), "{first:?}");

    let oversized = format!("{boundary}x");
    assert!(
        storage
            .write_registry_graph(Some(1), revision(&oversized))
            .await
            .is_err(),
    );
    let stored = storage
        .read_registry_graph()
        .await
        .unwrap()
        .unwrap();
    assert_eq!((stored.generation, stored.actor_id), (1, boundary));
}
