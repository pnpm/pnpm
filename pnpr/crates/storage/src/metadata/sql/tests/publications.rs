//! Atomic publications, their recorded outcomes, and publication lookups.

use super::{Db, npm, packument, read, storage, write};
use crate::metadata::{
    BatchDocument, PublicationCommit, PublicationRequest, PublishNotes, ReleaseRecords, SqlMetadata,
};
use pnpr_error::RegistryError;
use pnpr_package_name::Ecosystem;
use serde_json::{Value, json};
use std::sync::Arc;

fn document(name: &str, document: &Value) -> BatchDocument {
    BatchDocument {
        ecosystem: Ecosystem::Npm,
        name: name.to_string(),
        bytes: serde_json::to_vec(document).unwrap(),
        expected: None,
        principal: Some("alice".to_string()),
        records: ReleaseRecords::new(),
    }
}

fn request(digest: &str, name: Option<&str>) -> PublicationRequest {
    PublicationRequest {
        principal: "alice".to_string(),
        idempotency_key: format!("request:{digest}"),
        request_digest: digest.to_string(),
        name: name.map(str::to_string),
    }
}

async fn commit(
    database: &Arc<SqlMetadata>,
    documents: Vec<BatchDocument>,
    request: PublicationRequest,
) -> Result<PublicationCommit, RegistryError> {
    database.commit_publication(
        "acme",
        documents,
        Vec::new(),
        PublishNotes::default(),
        Some(request),
    )
    .await
}

async fn a_batch_commits_every_package_under_one_generation_or_none(db: Db) {
    let storage = storage(&db);
    let database = storage.sql_metadata();
    let org = storage.for_hosted("acme", Ecosystem::Npm);
    let core = packument("core", &["2.0.0"]);
    let widgets = packument("widgets", &["2.0.0"]);
    let batch = vec![document("core", &core), document("widgets", &widgets)];
    let committed = commit(database, batch, request("one", Some("v2"))).await.unwrap();
    assert_eq!(committed.generation.value, 1);
    assert_eq!(committed.publication, Some(1));
    assert_eq!(read(&org, "core").await, core);
    assert_eq!(read(&org, "widgets").await, widgets);

    // A batch whose second member rewrites an accepted version commits none
    // of its members.
    let mut changed = packument("widgets", &["2.0.0"]);
    changed["versions"]["2.0.0"]["description"] = json!("changed");
    let batch =
        vec![document("fresh", &packument("fresh", &["1.0.0"])), document("widgets", &changed)];
    let err = commit(database, batch, request("two", None)).await.unwrap_err();
    assert!(matches!(err, RegistryError::DocumentWriteConflict { .. }), "{err:?}");
    assert!(
        org.read_hosted_document(&npm("fresh")).await
            .unwrap()
            .is_none(),
    );
    assert_eq!(database.catalog_generation("acme").await.unwrap(), 1);
}

async fn a_retried_request_gets_its_recorded_outcome(db: Db) {
    let storage = storage(&db);
    let database = storage.sql_metadata();
    let doc = packument("core", &["1.0.0"]);
    let first = commit(database, vec![document("core", &doc)], request("digest", None))
        .await
        .unwrap();
    let retry = commit(database, vec![document("core", &doc)], request("digest", None))
        .await
        .unwrap();
    assert!(retry.replayed);
    assert_eq!((retry.generation, retry.publication), (first.generation, first.publication));
    let recorded = database
        .recorded_outcome("acme", &request("digest", None))
        .await
        .unwrap();
    assert_eq!(recorded.map(|outcome| outcome.generation), Some(1));
    let mut by_bob = request("digest", None);
    by_bob.principal = "bob".to_string();
    // Another principal, or the same request sent to another catalog, is a
    // publish of its own.
    for (catalog, other) in [("acme", by_bob), ("other", request("digest", None))] {
        let found = database.recorded_outcome(catalog, &other).await.unwrap();
        assert!(found.is_none(), "{catalog}");
    }

    let mut reused = request("other", None);
    reused.idempotency_key = "request:digest".to_string();
    let err = commit(database, vec![document("core", &doc)], reused).await.unwrap_err();
    assert!(matches!(err, RegistryError::AdminConflict { .. }), "{err:?}");
}

async fn an_unpublished_member_invalidates_the_recorded_outcome(db: Db) {
    let storage = storage(&db);
    let database = storage.sql_metadata();
    let org = storage.for_hosted("acme", Ecosystem::Npm);
    let core = packument("core", &["1.0.0"]);
    let widgets = packument("widgets", &["1.0.0"]);
    commit(
        database,
        vec![document("core", &core), document("widgets", &widgets)],
        request("one", None),
    )
    .await
    .unwrap();
    assert!(
        org.remove_package(&npm("widgets")).await
            .unwrap(),
    );
    assert!(
        database
            .recorded_outcome("acme", &request("one", None))
            .await
            .unwrap()
            .is_none(),
    );

    let mut retained = document("core", &core);
    retained.expected = Some(crate::HostedDocumentVersion::Unversioned);
    let err = commit(database, vec![retained, document("widgets", &widgets)], request("one", None))
        .await
        .unwrap_err();
    assert!(matches!(err, RegistryError::CannotRepublishUnpublishedVersion { .. }), "{err:?}");
    assert_eq!(database.catalog_generation("acme").await.unwrap(), 2);
    assert_eq!(read(&org, "core").await, core);
    let mut reused = request("different", None);
    reused.idempotency_key = "request:one".to_string();
    let err = commit(database, vec![document("core", &core)], reused).await.unwrap_err();
    assert!(matches!(err, RegistryError::AdminConflict { .. }), "{err:?}");
}

async fn publications_are_found_by_id_name_and_member(db: Db) {
    let storage = storage(&db);
    let database = storage.sql_metadata();
    let batch = vec![
        document("core", &packument("core", &["1.0.0"])),
        document("widgets", &packument("widgets", &["1.0.0"])),
    ];
    commit(database, batch, request("one", Some("v1"))).await.unwrap();
    let by_name = database
        .publication("acme", "v1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(database.publication("acme", "1").await.unwrap(), Some(by_name.clone()));
    let members: Vec<_> = by_name.members
        .iter()
        .map(|member| member.name.as_str())
        .collect();
    assert_eq!(members, ["core", "widgets"]);
    assert_eq!(by_name.publisher.as_deref(), Some("alice"));
    assert_eq!(by_name.members[0].artifact, "sha512-1.0.0");
    let initial = database
        .initial_publication(&npm("widgets"), "acme", "1.0.0")
        .await
        .unwrap();
    assert_eq!(initial.map(|publication| publication.id), Some(1));

    let mut taken = document("core", &packument("core", &["1.0.0", "1.1.0"]));
    taken.expected = Some(crate::HostedDocumentVersion::Unversioned);
    let taken = vec![taken];
    let err = commit(database, taken, request("two", Some("v1"))).await.unwrap_err();
    assert!(matches!(err, RegistryError::AdminConflict { .. }), "{err:?}");
}

async fn a_version_written_outside_a_publish_request_gets_a_publication(db: Db) {
    let storage = storage(&db);
    write(&storage, "plain", &packument("plain", &["1.0.0"])).await;
    write(&storage, "plain", &packument("plain", &["1.0.0", "1.1.0"])).await;
    let database = storage.sql_metadata();
    let first = database
        .initial_publication(&npm("plain"), "", "1.0.0")
        .await
        .unwrap()
        .unwrap();
    assert_eq!((first.id, first.publisher), (1, None));
    let second = database
        .initial_publication(&npm("plain"), "", "1.1.0")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second.id, 2);
    let versions: Vec<&str> = second.members
        .iter()
        .map(|member| member.version.as_str())
        .collect();
    assert_eq!(versions, ["1.1.0"]);
    assert!(
        database
            .initial_publication(&npm("plain"), "", "9.9.9")
            .await
            .unwrap()
            .is_none(),
    );
}

async fn a_publication_without_a_request_records_its_publisher(db: Db) {
    let storage = storage(&db);
    let database = storage.sql_metadata();
    // No request, as a staged publish's approval commits: the publisher still
    // comes from the document.
    let mut core = document("core", &packument("core", &["1.0.0"]));
    core.principal = Some("carol".to_string());
    database
        .commit_publication("acme", vec![core], Vec::new(), PublishNotes::default(), None)
        .await
        .unwrap();
    let history = database
        .tag_history(&npm("core"), "acme", "latest")
        .await
        .unwrap();
    let principals: Vec<_> = history
        .iter()
        .map(|entry| entry.principal.as_deref())
        .collect();
    assert_eq!(principals, [Some("carol")]);
    let publication = database
        .initial_publication(&npm("core"), "acme", "1.0.0")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(publication.publisher.as_deref(), Some("carol"));
}

async fn a_publication_cannot_bring_back_a_removed_version(db: Db) {
    let storage = storage(&db);
    let database = storage.sql_metadata();
    let org = storage.for_hosted("acme", Ecosystem::Npm);
    let batch = vec![document("core", &packument("core", &["1.0.0"]))];
    commit(database, batch, request("one", None)).await.unwrap();
    let removed = org
        .remove_package(&npm("core"))
        .await
        .unwrap();
    assert!(removed);

    let batch = vec![document("core", &packument("core", &["1.0.0", "2.0.0"]))];
    let err = commit(database, batch, request("two", None)).await.unwrap_err();
    assert!(matches!(err, RegistryError::CannotRepublishUnpublishedVersion { .. }), "{err:?}");
    assert!(
        org.read_hosted_document(&npm("core")).await
            .unwrap()
            .is_none(),
    );
}

async fn competing_publications_of_one_version_commit_once(db: Db) {
    // Two stores over one database: two writers that share no in-process lock.
    let (first, second) = (storage(&db), storage(&db));
    let mut theirs = packument("core", &["1.0.0"]);
    theirs["versions"]["1.0.0"]["dist"]["integrity"] = json!("sha512-theirs");
    let ours = vec![
        document("core", &packument("core", &["1.0.0"])),
        document("extra", &packument("extra", &["1.0.0"])),
    ];
    let theirs = vec![document("core", &theirs)];
    let left = commit(first.sql_metadata(), ours, request("ours", None));
    let right = commit(second.sql_metadata(), theirs, request("theirs", None));
    let (left, right) = tokio::join!(left, right);
    let winner = match (left, right) {
        (Ok(won), Err(RegistryError::DocumentWriteConflict { .. })) => {
            assert!(read(&first.for_hosted("acme", Ecosystem::Npm), "extra").await.is_object());
            won
        }
        (Err(RegistryError::DocumentWriteConflict { .. }), Ok(won)) => {
            // The losing batch committed none of its members.
            let extra = first
                .for_hosted("acme", Ecosystem::Npm)
                .read_hosted_document(&npm("extra"))
                .await
                .unwrap();
            assert!(extra.is_none());
            won
        }
        outcomes => panic!("exactly one publication commits, got {outcomes:?}"),
    };
    assert_eq!(winner.generation.value, 1);
    let database = first.sql_metadata();
    assert_eq!(database.catalog_generation("acme").await.unwrap(), 1);
    let second_publication = database.publication("acme", "2").await.unwrap();
    assert!(second_publication.is_none());
}

/// A packument of `name` whose versions were accepted under the npm profile.
fn verified(name: &str, versions: &[&str]) -> Value {
    let mut document = packument(name, versions);
    for version in versions {
        document["versions"][*version]["_archiveManifest"] =
            json!({ "profile": "npm-install-manifest/1", "packageJson": "sha512-input" });
    }
    document
}

/// A release record of `name@version`, revision 0, under the npm profile.
fn record_of(name: &str, version: &str) -> Vec<u8> {
    let record = json!({
        "schema": "pnpr-release-record/1",
        "purl": format!("pkg:npm/{name}@{version}"),
        "name": name,
        "version": version,
        "revision": 0,
        "interpretation": "npm-install-manifest/1",
        "manifest": {},
    });
    serde_json::to_vec(&record).unwrap()
}

async fn a_version_is_accepted_with_its_release_record(db: Db) {
    let storage = storage(&db);
    let database = storage.sql_metadata();
    let org = storage.for_hosted("acme", Ecosystem::Npm);
    let mut core = document("core", &verified("core", &["1.0.0"]));
    let record = record_of("core", "1.0.0");
    core.records.insert("1.0.0".to_string(), record.clone());
    commit(database, vec![core], request("one", None)).await.unwrap();
    let read_record = async |version, revision| {
        org.read_release_record(&npm("core"), version, revision).await
            .unwrap()
    };
    assert_eq!(read_record("1.0.0", 0).await, Some(record.clone()));
    assert_eq!(read_record("1.0.0", 1).await, None);
    assert_eq!(read_record("2.0.0", 0).await, None);

    // A version the store holds keeps the record it was accepted with.
    let mut restated = document("core", &read(&org, "core").await);
    restated.expected = Some(crate::HostedDocumentVersion::Generation(1));
    restated.records.insert("1.0.0".to_string(), record_of("core", "1.0.0-other"));
    commit(database, vec![restated], request("two", None)).await.unwrap();
    assert_eq!(read_record("1.0.0", 0).await, Some(record));

    // Removing the version removes its record.
    write(&org, "core", &packument("core", &[])).await;
    assert_eq!(read_record("1.0.0", 0).await, None);
}

async fn a_release_record_must_describe_its_version(db: Db) {
    let storage = storage(&db);
    let database = storage.sql_metadata();
    let org = storage.for_hosted("acme", Ecosystem::Npm);
    for (version, record) in [
        ("1.0.0", record_of("core", "2.0.0")),
        ("1.0.0", record_of("other", "1.0.0")),
        ("1.0.0", b"{}".to_vec()),
    ] {
        let mut core = document("core", &verified("core", &[version]));
        core.records.insert(version.to_string(), record);
        let err = commit(database, vec![core], request("one", None)).await.unwrap_err();
        assert!(matches!(err, RegistryError::Internal { .. }), "{err:?}");
    }
    assert!(
        org.read_hosted_document(&npm("core")).await
            .unwrap()
            .is_none(),
    );
}

conformance! {
    a_version_is_accepted_with_its_release_record,
    a_release_record_must_describe_its_version,
    a_batch_commits_every_package_under_one_generation_or_none,
    a_retried_request_gets_its_recorded_outcome,
    an_unpublished_member_invalidates_the_recorded_outcome,
    publications_are_found_by_id_name_and_member,
    a_version_written_outside_a_publish_request_gets_a_publication,
    a_publication_without_a_request_records_its_publisher,
    a_publication_cannot_bring_back_a_removed_version,
    competing_publications_of_one_version_commit_once,
}
