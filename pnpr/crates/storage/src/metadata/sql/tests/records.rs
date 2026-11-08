use super::{Db, npm, packument, storage};
use crate::{DocumentWrite, RegistryRecord, SCIM_DIRECTORY};
use serde_json::json;

async fn record_keys_preserve_case(db: Db) {
    let storage = storage(&db);
    let kind = RegistryRecord::TeamRoster;
    for key in ["example", "Example"] {
        assert!(storage.create_registry_record(kind, key, key.as_bytes()).await.unwrap());
    }
    for key in ["example", "Example"] {
        assert_eq!(
            storage
                .read_registry_record(kind, key)
                .await
                .unwrap()
                .unwrap()
                .bytes,
            key.as_bytes(),
        );
    }
}

async fn registry_records_use_current_generations(db: Db) {
    let first = storage(&db);
    let second = storage(&db);
    for kind in
        [RegistryRecord::TeamRoster, RegistryRecord::RuleOverrides, RegistryRecord::ScimUsers]
    {
        let key = SCIM_DIRECTORY;
        assert!(first.create_registry_record(kind, key, b"initial").await.unwrap());
        assert!(!second.create_registry_record(kind, key, b"duplicate").await.unwrap());
        let original = second
            .read_registry_record(kind, key)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(original.bytes, b"initial");
        let left = first.replace_registry_record_if_current(kind, key, &original, b"left");
        let right = second.replace_registry_record_if_current(kind, key, &original, b"right");
        let (left, right) = tokio::join!(left, right);
        assert_ne!(left.unwrap().is_some(), right.unwrap().is_some());
        let current = first
            .read_registry_record(kind, key)
            .await
            .unwrap()
            .unwrap();
        assert!(current.generation > original.generation);
        let restored = second
            .replace_registry_record_if_current(kind, key, &current, b"initial")
            .await
            .unwrap()
            .unwrap();
        assert!(
            first
                .replace_registry_record_if_current(kind, key, &original, b"stale")
                .await
                .unwrap()
                .is_none(),
        );
        assert!(second.remove_registry_record(kind, key).await.unwrap());
        assert!(first.create_registry_record(kind, key, b"initial").await.unwrap());
        assert!(
            second
                .replace_registry_record_if_current(kind, key, &restored, b"stale")
                .await
                .unwrap()
                .is_none(),
        );
        assert_eq!(
            second
                .read_registry_record(kind, key)
                .await
                .unwrap()
                .unwrap()
                .bytes,
            b"initial",
        );
    }
    for namespace in [".team-rosters", ".rule-overrides", ".scim-users"] {
        assert!(
            !db.tmp
                .path()
                .join("storage")
                .join(namespace)
                .exists(),
        );
    }
}

async fn staged_records_keep_only_bodies_in_the_object_store(db: Db) {
    let first = storage(&db);
    let second = storage(&db);
    let id = "11111111-2222-3333-4444-555555555555";
    first.create_staged_body(id, b"body").await.unwrap();
    first.create_staged_meta(id, b"metadata").await.unwrap();
    assert!(second.create_staged_meta(id, b"duplicate").await.is_err());
    assert_eq!(second.list_staged_ids().await.unwrap(), [id]);
    assert_eq!(
        second
            .read_staged_body(id)
            .await
            .unwrap()
            .unwrap(),
        b"body",
    );
    let original = second
        .read_staged_meta(id)
        .await
        .unwrap()
        .unwrap();
    let claimed = first
        .replace_staged_meta_if_current(id, &original, b"claimed")
        .await
        .unwrap()
        .unwrap();
    assert!(
        second
            .replace_staged_meta_if_current(id, &original, b"stale")
            .await
            .unwrap()
            .is_none(),
    );
    assert_eq!(
        second
            .read_staged_meta(id)
            .await
            .unwrap()
            .unwrap(),
        claimed,
    );
    let directory = db.tmp.path().join("storage/.staged");
    assert!(
        !directory
            .join(format!("{id}.json"))
            .exists(),
    );
    assert!(
        directory
            .join(format!("{id}.body.json"))
            .exists(),
    );
    assert!(second.remove_staged(id).await.unwrap());
    assert!(
        first
            .read_staged_meta(id)
            .await
            .unwrap()
            .is_none(),
    );
    assert!(
        first
            .read_staged_body(id)
            .await
            .unwrap()
            .is_none(),
    );
    assert!(
        first
            .list_staged_ids()
            .await
            .unwrap()
            .is_empty(),
    );
    first.create_staged_meta(id, b"claimed").await.unwrap();
    assert!(
        second
            .replace_staged_meta_if_current(id, &claimed, b"stale")
            .await
            .unwrap()
            .is_none(),
    );
}

async fn execute(db: &Db, sql: &str) {
    let adapter = db.adapter();
    let sql = sql.to_string();
    tokio::task::spawn_blocking(move || {
        adapter.write(&mut |connection| connection.execute_batch(&sql))
    })
    .await
    .unwrap()
    .unwrap();
}

async fn unpublish_rolls_back_ownership_and_version_bans_together(db: Db) {
    let first = storage(&db);
    let second = storage(&db);
    let name = npm("example-package");
    let mut document = packument(name.as_str(), &["1.0.0", "2.0.0"]);
    document["maintainers"] = json!([
        {"name": "former-name", "userId": "publisher-id"},
        {"name": "current-name", "userId": "publisher-id"}
    ]);
    first
        .write_hosted_document_if_current(
            &name,
            &serde_json::to_vec(&document).unwrap(),
            None,
            None,
        )
        .await
        .unwrap();
    let stored = second
        .read_hosted_document_for_update(&name)
        .await
        .unwrap()
        .unwrap();
    document["versions"]
        .as_object_mut()
        .unwrap()
        .remove("1.0.0");
    let updated = serde_json::to_vec(&document).unwrap();
    execute(&db, db.sql(
        "CREATE TRIGGER refuse_version_removal BEFORE INSERT ON removed_versions BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        "CREATE FUNCTION refuse_removal() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected failure'; END $$; CREATE TRIGGER refuse_version_removal BEFORE INSERT ON removed_versions FOR EACH ROW EXECUTE FUNCTION refuse_removal();",
        "CREATE TRIGGER refuse_version_removal BEFORE INSERT ON removed_versions FOR EACH ROW SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT = 'injected failure'",
    )).await;
    assert!(
        first
            .write_hosted_document_if_current(
                &name,
                &updated,
                Some(&stored.version),
                Some("publisher-id")
            )
            .await
            .is_err(),
    );
    assert!(
        first
            .remove_package(&name, Some("publisher-id"))
            .await
            .is_err(),
    );
    assert!(
        second
            .read_tombstone(&name)
            .await
            .unwrap()
            .is_none(),
    );
    let unchanged = second
        .read_hosted_document_for_update(&name)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unchanged.bytes, stored.bytes);
    assert!(
        matches!((&unchanged.version, &stored.version), (crate::HostedDocumentVersion::Generation(left), crate::HostedDocumentVersion::Generation(right)) if left == right),
    );
    execute(
        &db,
        db.sql(
            "DROP TRIGGER refuse_version_removal",
            "DROP TRIGGER refuse_version_removal ON removed_versions",
            "DROP TRIGGER refuse_version_removal",
        ),
    )
    .await;
    assert_eq!(
        first
            .write_hosted_document_if_current(
                &name,
                &updated,
                Some(&stored.version),
                Some("publisher-id")
            )
            .await
            .unwrap(),
        DocumentWrite::Written,
    );
    let removed = second
        .read_tombstone(&name)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(removed.maintainers, ["publisher-id"]);
    assert_eq!(removed.unpublished_by.as_deref(), Some("publisher-id"));
    assert_eq!(removed.unpublished_versions, ["1.0.0"]);
    assert_eq!(
        first
            .write_hosted_document_if_current(
                &name,
                &stored.bytes,
                Some(&stored.version),
                Some("other-id")
            )
            .await
            .unwrap(),
        DocumentWrite::Conflict,
    );
    assert_eq!(
        second
            .read_tombstone(&name)
            .await
            .unwrap()
            .unwrap(),
        removed,
    );
    assert!(
        first
            .remove_package(&name, Some("publisher-id"))
            .await
            .unwrap(),
    );
    let removed = second
        .read_tombstone(&name)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(removed.unpublished_versions, ["1.0.0", "2.0.0"]);
    assert_eq!(removed.maintainers, ["publisher-id"]);
    assert!(
        second
            .read_hosted_document_for_update(&name)
            .await
            .unwrap()
            .is_none(),
    );
}

conformance!(
    record_keys_preserve_case,
    registry_records_use_current_generations,
    staged_records_keep_only_bodies_in_the_object_store,
    unpublish_rolls_back_ownership_and_version_bans_together,
);
