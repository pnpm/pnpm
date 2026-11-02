//! Atomic publications, their recorded outcomes, and publication lookups.

use super::{npm, packument, read, storage, write};
use crate::metadata::{
    BatchDocument, PublicationCommit, PublicationOrigin, PublicationRequest, PublishNotes,
    SqliteMetadata,
};
use pnpr_error::RegistryError;
use pnpr_package_name::Ecosystem;
use serde_json::{Value, json};
use std::sync::Arc;
use tempfile::TempDir;

fn document(name: &str, document: &Value) -> BatchDocument {
    BatchDocument {
        ecosystem: Ecosystem::Npm,
        name: name.to_string(),
        bytes: serde_json::to_vec(document).unwrap(),
        expected: None,
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
    database: &Arc<SqliteMetadata>,
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

#[tokio::test]
async fn a_batch_commits_every_package_under_one_generation_or_none() {
    let tmp = TempDir::new().unwrap();
    let storage = storage(&tmp);
    let database = storage.sqlite_metadata().unwrap();
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
    assert_eq!(database.catalog_generation("acme").unwrap(), 1);
}

#[tokio::test]
async fn a_retried_request_gets_its_recorded_outcome() {
    let tmp = TempDir::new().unwrap();
    let storage = storage(&tmp);
    let database = storage.sqlite_metadata().unwrap();
    let doc = packument("core", &["1.0.0"]);
    let first = commit(database, vec![document("core", &doc)], request("digest", None))
        .await
        .unwrap();
    let retry = commit(database, vec![document("core", &doc)], request("digest", None))
        .await
        .unwrap();
    assert!(retry.replayed);
    assert_eq!((retry.generation, retry.publication), (first.generation, first.publication));
    let recorded = database.recorded_outcome("alice", "request:digest", "digest").await.unwrap();
    assert_eq!(recorded.map(|outcome| outcome.generation), Some(1));
    assert!(
        database
            .recorded_outcome("bob", "request:digest", "digest")
            .await
            .unwrap()
            .is_none(),
    );

    let mut reused = request("other", None);
    reused.idempotency_key = "request:digest".to_string();
    let err = commit(database, vec![document("core", &doc)], reused).await.unwrap_err();
    assert!(matches!(err, RegistryError::AdminConflict { .. }), "{err:?}");
}

#[tokio::test]
async fn publications_are_found_by_id_name_and_member() {
    let tmp = TempDir::new().unwrap();
    let storage = storage(&tmp);
    let database = storage.sqlite_metadata().unwrap();
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

#[tokio::test]
async fn a_version_from_outside_any_publication_gets_an_imported_one() {
    let tmp = TempDir::new().unwrap();
    let storage = storage(&tmp);
    write(&storage, "legacy", &packument("legacy", &["1.0.0"])).await;
    let database = storage.sqlite_metadata().unwrap();
    let imported = database
        .initial_publication(&npm("legacy"), "", "1.0.0")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(imported.origin, PublicationOrigin::Imported);
    assert_eq!(imported.publisher, None);
    assert!(
        database
            .initial_publication(&npm("legacy"), "", "9.9.9")
            .await
            .unwrap()
            .is_none(),
    );
}
