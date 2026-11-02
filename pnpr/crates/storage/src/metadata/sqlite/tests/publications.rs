//! Atomic publications, their recorded outcomes, and publication lookups.

use super::{npm, packument, read, storage, write};
use crate::metadata::{BatchDocument, PublicationCommit, PublicationRequest, SqliteMetadata};
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
        principal: Some("alice".to_string()),
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
    database.commit_publication("acme", documents, Vec::new(), Some(request)).await
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
async fn a_version_written_outside_a_publish_request_gets_a_publication() {
    let tmp = TempDir::new().unwrap();
    let storage = storage(&tmp);
    write(&storage, "plain", &packument("plain", &["1.0.0"])).await;
    write(&storage, "plain", &packument("plain", &["1.0.0", "1.1.0"])).await;
    let database = storage.sqlite_metadata().unwrap();
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

#[tokio::test]
async fn a_publication_without_a_request_records_its_publisher() {
    let tmp = TempDir::new().unwrap();
    let storage = storage(&tmp);
    let database = storage.sqlite_metadata().unwrap();
    // No request, as a staged publish's approval commits: the publisher still
    // comes from the document.
    let mut core = document("core", &packument("core", &["1.0.0"]));
    core.principal = Some("carol".to_string());
    database
        .commit_publication("acme", vec![core], Vec::new(), None)
        .await
        .unwrap();
    let history = database
        .tag_history(&npm("core"), "acme", "latest")
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

#[tokio::test]
async fn a_publication_cannot_bring_back_a_removed_version() {
    let tmp = TempDir::new().unwrap();
    let storage = storage(&tmp);
    let database = storage.sqlite_metadata().unwrap();
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

#[tokio::test]
async fn competing_publications_of_one_version_commit_once() {
    let tmp = TempDir::new().unwrap();
    // Two stores over one database: two writers that share no in-process lock.
    let (first, second) = (storage(&tmp), storage(&tmp));
    let mut theirs = packument("core", &["1.0.0"]);
    theirs["versions"]["1.0.0"]["dist"]["integrity"] = json!("sha512-theirs");
    let ours = vec![
        document("core", &packument("core", &["1.0.0"])),
        document("extra", &packument("extra", &["1.0.0"])),
    ];
    let theirs = vec![document("core", &theirs)];
    let left = commit(first.sqlite_metadata().unwrap(), ours, request("ours", None));
    let right = commit(second.sqlite_metadata().unwrap(), theirs, request("theirs", None));
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
    let database = first.sqlite_metadata().unwrap();
    assert_eq!(database.catalog_generation("acme").unwrap(), 1);
    let second_publication = database.publication("acme", "2").await.unwrap();
    assert!(second_publication.is_none());
}
