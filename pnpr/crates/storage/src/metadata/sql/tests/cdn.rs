use super::{Db, npm, packument, storage, write};
use crate::metadata::{NewScimUser, StoredRuleOverrides};
use serde_json::Map;

fn private_rule_overrides() -> StoredRuleOverrides {
    StoredRuleOverrides { access: Some(vec!["$authenticated".to_string()]), ..Default::default() }
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
    storage
        .replace_rule_overrides("npm/local", None, &private_rule_overrides())
        .await
        .unwrap();
    let pending = storage.pending_cdn_invalidations().await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].package, "*");
    storage
        .replace_rule_overrides("npm/local", None, &StoredRuleOverrides::default())
        .await
        .unwrap();
    storage.acknowledge_cdn_invalidations(pending.clone()).await.unwrap();
    assert!(
        storage.pending_cdn_invalidations().await.unwrap()[0].generation > pending[0].generation,
    );
}

async fn cdn_ignores_scim_and_staged_writes_while_invalidating_team_rosters(db: Db) {
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

    storage
        .create_team_in_roster("npm/local", &[], "t-1", "devs")
        .await
        .unwrap();
    let pending = storage.pending_cdn_invalidations().await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].package, "*");
}

async fn cdn_artifact_mapping_changes_invalidate_package(db: Db) {
    use super::blobs;
    use crate::metadata::Unmapping;
    use pnpr_package_name::Ecosystem;

    let storage = storage(&db);
    storage.enable_cdn_invalidations("example").await.unwrap();
    let startup = storage.pending_cdn_invalidations().await.unwrap();
    storage.acknowledge_cdn_invalidations(startup).await.unwrap();
    let artifact = blobs::tgz("widget", "1.0.0", &blobs::id(b"archive"), 1);
    let file = artifact.artifact.clone();
    storage
        .sql_metadata()
        .map_artifact("", artifact)
        .await
        .unwrap();
    let mapped = storage.pending_cdn_invalidations().await.unwrap();
    assert_eq!(mapped[0].package, "widget");
    storage
        .sql_metadata()
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
    storage
        .replace_rule_overrides("npm/local", None, &private_rule_overrides())
        .await
        .unwrap();
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

async fn cdn_ignores_idempotent_team_member_and_rule_override_writes(db: Db) {
    let storage = storage(&db);
    storage.enable_cdn_invalidations("example").await.unwrap();
    let startup = storage.pending_cdn_invalidations().await.unwrap();
    storage.acknowledge_cdn_invalidations(startup).await.unwrap();

    storage
        .create_team_in_roster("npm/local", &[], "t-1", "devs")
        .await
        .unwrap();
    storage
        .add_member_to_team("npm/local", &[], "devs", "u-1")
        .await
        .unwrap();
    let roster_gen = storage
        .read_team_roster_generation("npm/local")
        .await
        .unwrap()
        .unwrap();
    let pending = storage.pending_cdn_invalidations().await.unwrap();
    storage.acknowledge_cdn_invalidations(pending).await.unwrap();

    storage
        .add_member_to_team("npm/local", &[], "devs", "u-1")
        .await
        .unwrap();
    storage
        .remove_member_from_team("npm/local", &[], "devs", "ghost")
        .await
        .unwrap();
    assert_eq!(storage.read_team_roster_generation("npm/local").await.unwrap(), Some(roster_gen));
    assert!(
        storage
            .pending_cdn_invalidations()
            .await
            .unwrap()
            .is_empty(),
    );

    let overrides = private_rule_overrides();
    storage.replace_rule_overrides("npm/local", None, &overrides).await.unwrap();
    let overrides_gen = storage
        .read_rule_overrides_generation("npm/local")
        .await
        .unwrap()
        .unwrap();
    let pending = storage.pending_cdn_invalidations().await.unwrap();
    storage.acknowledge_cdn_invalidations(pending).await.unwrap();

    storage.replace_rule_overrides("npm/local", None, &overrides).await.unwrap();
    assert_eq!(
        storage.read_rule_overrides_generation("npm/local").await.unwrap(),
        Some(overrides_gen),
    );
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
    cdn_ignores_scim_and_staged_writes_while_invalidating_team_rosters,
    cdn_artifact_mapping_changes_invalidate_package,
    cdn_delivery_prioritizes_policy_and_retains_oldest_package_changes,
    cdn_ignores_idempotent_team_member_and_rule_override_writes,
);
