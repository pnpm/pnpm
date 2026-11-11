use super::{Db, npm, packument, storage};
use crate::{
    DocumentWrite,
    metadata::{
        NewScimUser, RuleOverridesWrite, ScimUserRemoval, ScimUserWrite, StoredPackageOverride,
        StoredRuleOverrides, StoredTeam, TeamRosterWrite,
    },
};
use serde_json::{Map, json};
use std::collections::{BTreeMap, BTreeSet};

async fn record_keys_preserve_case(db: Db) {
    let storage = storage(&db);
    for key in ["npm/example", "npm/Example"] {
        let team = StoredTeam {
            id: format!("id-{key}"),
            name: key.replace('/', "-"),
            members: BTreeSet::new(),
        };
        storage
            .replace_team_roster(key, &[team])
            .await
            .unwrap();
    }
    for key in ["npm/example", "npm/Example"] {
        let roster = storage
            .read_team_roster(key)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(roster.teams.len(), 1);
        assert_eq!(roster.teams[0].id, format!("id-{key}"));
    }
}

async fn scim_users_track_incremental_generations_and_deprovisions(db: Db) {
    let first = storage(&db);
    let second = storage(&db);
    let mut attrs = Map::new();
    attrs.insert("displayName".to_string(), json!("Alice"));
    let created = first
        .insert_scim_user(NewScimUser {
            user_id: "u-alice".to_string(),
            active: true,
            attributes: attrs.clone(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(created.deprovisions, 0);
    assert!(
        second
            .insert_scim_user(NewScimUser {
                user_id: "u-alice".to_string(),
                active: true,
                attributes: Map::new(),
            })
            .await
            .unwrap()
            .is_none(),
    );

    let initial_delta = second.read_scim_statuses_since(0).await.unwrap();
    assert_eq!(initial_delta.len(), 1);
    assert!(
        second
            .read_scim_statuses_since(created.generation)
            .await
            .unwrap()
            .is_empty(),
    );

    let ScimUserWrite::Written(deactivated) = first
        .replace_scim_user(
            Some(created.generation),
            NewScimUser { user_id: "u-alice".to_string(), active: false, attributes: attrs },
        )
        .await
        .unwrap()
    else {
        panic!("expected write to succeed");
    };
    assert_eq!(deactivated.deprovisions, 1);
    assert_eq!(
        second
            .replace_scim_user(
                Some(created.generation),
                NewScimUser {
                    user_id: "u-alice".to_string(),
                    active: true,
                    attributes: Map::new(),
                },
            )
            .await
            .unwrap(),
        ScimUserWrite::Conflict,
    );

    let delta = second.read_scim_statuses_since(created.generation).await.unwrap();
    assert_eq!(delta.len(), 1);
    assert_eq!(delta[0].deprovisions, 1);
    assert!(!delta[0].active);

    let ScimUserRemoval::Removed(removed) = second.remove_scim_user("u-alice").await.unwrap()
    else {
        panic!("expected removal");
    };
    assert!(!removed.active);
    assert!(
        first
            .read_scim_user("u-alice")
            .await
            .unwrap()
            .is_none(),
    );
    assert!(
        first
            .list_active_scim_users()
            .await
            .unwrap()
            .is_empty(),
    );
}

async fn team_rosters_and_rule_overrides_use_current_generations(db: Db) {
    let first = storage(&db);
    let second = storage(&db);
    let seed = [StoredTeam {
        id: "t-seed".to_string(),
        name: "readers".to_string(),
        members: BTreeSet::from(["u-1".to_string()]),
    }];
    let TeamRosterWrite::Written(created) =
        first.create_team_in_roster("npm/local", &seed, "t-devs", "devs").await.unwrap()
    else {
        panic!("expected team creation");
    };
    assert_eq!(created.teams.len(), 2);
    assert_eq!(
        second.create_team_in_roster("npm/local", &seed, "t-dup", "devs").await.unwrap(),
        TeamRosterWrite::Conflict,
    );
    let TeamRosterWrite::Written(with_member) =
        second.add_member_to_team("npm/local", &seed, "devs", "u-2").await.unwrap()
    else {
        panic!("expected member add");
    };
    assert!(with_member.generation > created.generation);

    let mut packages = BTreeMap::new();
    packages.insert(
        "@acme/*".to_string(),
        StoredPackageOverride {
            access: Some(vec!["team:t-devs".to_string()]),
            ..Default::default()
        },
    );
    let overrides = StoredRuleOverrides { access: Some(vec!["$all".to_string()]), packages };
    let RuleOverridesWrite::Written(Some(gen1)) = first
        .replace_rule_overrides("npm/local", Some(None), &overrides)
        .await
        .unwrap()
    else {
        panic!("expected initial override write");
    };
    assert_eq!(
        second
            .replace_rule_overrides("npm/local", Some(None), &overrides)
            .await
            .unwrap(),
        RuleOverridesWrite::Conflict,
    );
    assert_eq!(
        second
            .read_rule_overrides("npm/local")
            .await
            .unwrap()
            .unwrap()
            .overrides,
        overrides,
    );
    assert_eq!(
        second
            .replace_rule_overrides("npm/local", Some(Some(gen1)), &StoredRuleOverrides::default())
            .await
            .unwrap(),
        RuleOverridesWrite::Written(None),
    );
    assert!(
        first
            .read_rule_overrides("npm/local")
            .await
            .unwrap()
            .is_none(),
    );
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

async fn removing_an_owned_document_keeps_its_owners_and_remover(db: Db) {
    let storage = storage(&db).for_hosted("org", crate::Ecosystem::Oci);
    let name =
        pnpr_package_name::CanonicalPackageName::parse("acme/app", crate::Ecosystem::Oci).unwrap();
    let document = json!({ "name": "acme/app", "owners": ["owner-id"] }).to_string();
    storage.write_hosted_document_if_current(&name, document.as_bytes(), None, None).await.unwrap();
    let current = storage
        .read_hosted_document_for_update(&name)
        .await
        .unwrap()
        .unwrap();
    let removal = Some(&current.version);
    storage
        .write_hosted_document_if_current(&name, b"", removal, Some("remover-id"))
        .await
        .unwrap();
    let removed = storage
        .read_tombstone(&name)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(removed.maintainers, ["owner-id"]);
    assert_eq!(removed.unpublished_by.as_deref(), Some("remover-id"));
}

conformance!(
    record_keys_preserve_case,
    scim_users_track_incremental_generations_and_deprovisions,
    team_rosters_and_rule_overrides_use_current_generations,
    staged_records_keep_only_bodies_in_the_object_store,
    unpublish_rolls_back_ownership_and_version_bans_together,
    removing_an_owned_document_keeps_its_owners_and_remover,
);
