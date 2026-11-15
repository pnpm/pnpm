use super::{Db, npm, packument, storage, write};
use crate::{
    Storage,
    database::{NewRegistryGraph, NewScimUser, RegistryGraphWrite},
};
use serde_json::Map;

/// Store `document` as the next registry graph revision.
async fn write_graph(storage: &Storage, document: &str) {
    let current = storage.read_registry_graph_generation().await.unwrap();
    let revision =
        NewRegistryGraph { document, actor_id: "-", restored_from: None, vacated: &[], bound: &[] };
    let written = storage.write_registry_graph(current, revision).await.unwrap();
    assert!(matches!(written, RegistryGraphWrite::Written(_)), "{written:?}");
}

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
    write_graph(
        &storage,
        r#"{"registries":{"local":{"type":"hosted","access":"$authenticated"}}}"#,
    )
    .await;
    let pending = storage.pending_cdn_invalidations().await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].package, "*");
    write_graph(&storage, r#"{"registries":{"local":{"type":"hosted"}}}"#).await;
    storage.acknowledge_cdn_invalidations(pending.clone()).await.unwrap();
    assert!(
        storage.pending_cdn_invalidations().await.unwrap()[0].generation > pending[0].generation,
    );
}

async fn cdn_ignores_scim_and_staged_writes_while_invalidating_registry_graphs(db: Db) {
    let storage = storage(&db);
    storage.enable_cdn_invalidations("example").await.unwrap();
    let startup = storage.pending_cdn_invalidations().await.unwrap();
    storage.acknowledge_cdn_invalidations(startup).await.unwrap();

    storage
        .insert_scim_user(NewScimUser {
            user_id: "u-1".to_string(),
            active: true,
            attributes: Map::new(),
        })
        .await
        .unwrap();
    storage
        .replace_scim_user(
            None,
            NewScimUser { user_id: "u-1".to_string(), active: false, attributes: Map::new() },
        )
        .await
        .unwrap();
    storage.remove_scim_user("u-1").await.unwrap();
    storage.create_staged_meta("11111111-2222-3333-4444-555555555555", b"staged").await.unwrap();
    assert!(
        storage
            .pending_cdn_invalidations()
            .await
            .unwrap()
            .is_empty(),
    );

    write_graph(&storage, r#"{"registries":{"local":{"type":"hosted"}}}"#).await;
    let pending = storage.pending_cdn_invalidations().await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].package, "*");
}

async fn cdn_artifact_mapping_changes_invalidate_package(db: Db) {
    use super::blobs;
    use crate::database::Unmapping;
    use pnpr_package_name::Ecosystem;

    let storage = storage(&db);
    storage.enable_cdn_invalidations("example").await.unwrap();
    let startup = storage.pending_cdn_invalidations().await.unwrap();
    storage.acknowledge_cdn_invalidations(startup).await.unwrap();
    let artifact = blobs::tgz("widget", "1.0.0", &blobs::id(b"archive"), 1);
    let file = artifact.artifact.clone();
    storage
        .sql_database()
        .map_artifact("", artifact)
        .await
        .unwrap();
    let mapped = storage.pending_cdn_invalidations().await.unwrap();
    assert_eq!(mapped[0].package, "widget");
    storage
        .sql_database()
        .unmap_artifacts("", Ecosystem::Npm, "widget", (None, vec![Unmapping::from(file)]))
        .await
        .unwrap();
    storage.acknowledge_cdn_invalidations(mapped.clone()).await.unwrap();
    let removed = storage.pending_cdn_invalidations().await.unwrap();
    assert_eq!(removed[0].package, "widget");
    assert!(removed[0].generation > mapped[0].generation);
}

async fn cdn_delivery_prioritizes_policy_and_retains_oldest_package_changes(db: Db) {
    let storage = storage(&db);
    storage.enable_cdn_invalidations("example").await.unwrap();
    for index in 0..31 {
        let name = format!("widget-{index:02}");
        write(&storage, &name, &packument(&name, &["1.0.0"])).await;
    }
    let first = storage.pending_cdn_invalidations().await.unwrap();
    assert_eq!(first.len(), 30);
    write(&storage, "widget-00", &packument("widget-00", &["1.0.0", "2.0.0"])).await;
    storage.acknowledge_cdn_invalidations(first).await.unwrap();
    write_graph(
        &storage,
        r#"{"registries":{"local":{"type":"hosted","access":"$authenticated"}}}"#,
    )
    .await;
    let later = storage.pending_cdn_invalidations().await.unwrap();
    assert_eq!(later[0].package, "*");
    assert_eq!(later[1].package, "widget-29");
    assert_eq!(later[2].package, "widget-30");
    assert_eq!(later[3].package, "widget-00");
    storage.acknowledge_cdn_invalidations(later).await.unwrap();
    assert!(
        storage
            .pending_cdn_invalidations()
            .await
            .unwrap()
            .is_empty(),
    );
}

conformance!(
    cdn_invalidations_survive_restarts_and_concurrent_acknowledgement,
    cdn_policy_changes_invalidate_all_aliases,
    cdn_ignores_scim_and_staged_writes_while_invalidating_registry_graphs,
    cdn_artifact_mapping_changes_invalidate_package,
    cdn_delivery_prioritizes_policy_and_retains_oldest_package_changes,
);
