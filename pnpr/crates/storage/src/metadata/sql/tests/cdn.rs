use super::{Db, npm, packument, storage, write};
use crate::RegistryRecord;

async fn cdn_invalidations_survive_restarts_and_concurrent_acknowledgement(db: Db) {
    let first = storage(&db);
    write(&first, "widget", &packument("widget", &["1.0.0"])).await;
    assert!(
        first
            .pending_cdn_invalidations()
            .await
            .unwrap()
            .is_empty(),
    );
    first.enable_cdn_invalidations("example").await.unwrap();
    assert!(first.enable_cdn_invalidations("another-zone").await.is_err());
    let startup = first.pending_cdn_invalidations().await.unwrap();
    assert_eq!(startup[0].package, "*");
    first.acknowledge_cdn_invalidations(startup).await.unwrap();
    write(&first, "widget", &packument("widget", &["1.0.0", "2.0.0"])).await;
    let pending = first.pending_cdn_invalidations().await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].package, "widget");
    drop(first);
    let second = storage(&db);
    assert_eq!(
        second.pending_cdn_invalidations().await.unwrap()[0].generation,
        pending[0].generation,
    );
    second
        .remove_package(&npm("widget"), None)
        .await
        .unwrap();
    second.acknowledge_cdn_invalidations(pending.clone()).await.unwrap();
    let removed = second.pending_cdn_invalidations().await.unwrap();
    assert_eq!(removed.len(), 1);
    assert!(removed[0].generation > pending[0].generation);
    second.acknowledge_cdn_invalidations(removed).await.unwrap();
    assert!(
        second
            .pending_cdn_invalidations()
            .await
            .unwrap()
            .is_empty(),
    );
    write(&second, "gadget", &packument("gadget", &["1.0.0"])).await;
    second.acknowledge_cdn_invalidations(pending).await.unwrap();
    assert_eq!(second.pending_cdn_invalidations().await.unwrap()[0].package, "gadget");
}

async fn cdn_policy_changes_invalidate_all_aliases(db: Db) {
    let storage = storage(&db);
    storage.enable_cdn_invalidations("example").await.unwrap();
    let startup = storage.pending_cdn_invalidations().await.unwrap();
    storage.acknowledge_cdn_invalidations(startup).await.unwrap();
    let kind = RegistryRecord::RuleOverrides;
    storage.create_registry_record(kind, "npm/local", b"private").await.unwrap();
    let pending = storage.pending_cdn_invalidations().await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].package, "*");
    storage.remove_registry_record(kind, "npm/local").await.unwrap();
    storage.acknowledge_cdn_invalidations(pending.clone()).await.unwrap();
    assert!(
        storage.pending_cdn_invalidations().await.unwrap()[0].generation > pending[0].generation,
    );
}

conformance!(
    cdn_invalidations_survive_restarts_and_concurrent_acknowledgement,
    cdn_policy_changes_invalidate_all_aliases
);
