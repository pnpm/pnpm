use super::{StagedArchive, StagingError, reference_staged_archive, stage_archives};
use crate::parse_supported_registry_url;
use mockito::Matcher;
use pnpm_network::ThrottledClient;
use serde_json::json;

const STORED: &str = "sha512-c3RvcmVk";
const MISSING: &str = "sha512-bWlzc2luZw==";

fn archive(integrity: &str, bytes: &'static [u8]) -> StagedArchive<'static> {
    StagedArchive { integrity: integrity.to_owned(), bytes }
}

fn integrity_query(integrity: &str) -> Matcher {
    Matcher::UrlEncoded("integrity".to_owned(), integrity.to_owned())
}

#[test]
fn an_attachment_names_its_staged_archive_instead_of_carrying_it() {
    let mut document = json!({
        "_attachments": {
            "pkg-1.0.0.tgz": { "content_type": "application/octet-stream", "data": "AAAA", "length": 3 },
        },
    });
    reference_staged_archive(&mut document, STORED);
    assert_eq!(
        document["_attachments"]["pkg-1.0.0.tgz"],
        json!({ "content_type": "application/octet-stream", "integrity": STORED, "length": 3 }),
    );
}

#[tokio::test]
async fn only_archives_the_registry_lacks_are_uploaded() {
    let mut server = mockito::Server::new_async().await;
    let registry =
        parse_supported_registry_url(&format!("{}/npm/", server.url())).unwrap().normalized_url;
    let stored = server
        .mock("HEAD", "/npm/-/pnpr/v0/artifacts/blob")
        .match_query(integrity_query(STORED))
        .match_header("authorization", "Bearer t")
        .with_status(200)
        .expect(1)
        .create_async()
        .await;
    let missing = server
        .mock("HEAD", "/npm/-/pnpr/v0/artifacts/blob")
        .match_query(integrity_query(MISSING))
        .with_status(404)
        .expect(1)
        .create_async()
        .await;
    let upload = server
        .mock("PUT", "/npm/-/pnpr/v0/artifacts/blob")
        .match_query(integrity_query(MISSING))
        .match_header("authorization", "Bearer t")
        .match_header("content-type", "application/octet-stream")
        .match_body("missing")
        .with_status(201)
        .expect(1)
        .create_async()
        .await;

    let uploaded = stage_archives(
        &ThrottledClient::default(),
        &registry,
        Some("Bearer t"),
        &[archive(STORED, b"stored"), archive(MISSING, b"missing")],
    )
    .await
    .unwrap();
    assert_eq!(uploaded, 1, "only the missing archive is uploaded");

    stored.assert_async().await;
    missing.assert_async().await;
    upload.assert_async().await;
}

#[tokio::test]
async fn a_refused_upload_fails_with_what_the_registry_answered() {
    let mut server = mockito::Server::new_async().await;
    let registry =
        parse_supported_registry_url(&format!("{}/", server.url())).unwrap().normalized_url;
    server
        .mock("HEAD", "/-/pnpr/v0/artifacts/blob")
        .match_query(Matcher::Any)
        .with_status(404)
        .create_async()
        .await;
    server
        .mock("PUT", "/-/pnpr/v0/artifacts/blob")
        .match_query(Matcher::Any)
        .with_status(400)
        .with_body("over the limit")
        .create_async()
        .await;

    let error = stage_archives(
        &ThrottledClient::default(),
        &registry,
        None,
        &[archive(MISSING, b"missing")],
    )
    .await
    .unwrap_err();
    let StagingError::Failed { integrity, reason, .. } = error else {
        panic!("expected a staging failure, got {error:?}");
    };
    assert_eq!(integrity, MISSING);
    assert!(reason.contains("400") && reason.contains("over the limit"), "{reason}");
}

#[tokio::test]
async fn a_refused_upload_keeps_only_the_start_of_a_long_answer() {
    let mut server = mockito::Server::new_async().await;
    let registry =
        parse_supported_registry_url(&format!("{}/", server.url())).unwrap().normalized_url;
    server
        .mock("HEAD", "/-/pnpr/v0/artifacts/blob")
        .match_query(Matcher::Any)
        .with_status(404)
        .create_async()
        .await;
    server
        .mock("PUT", "/-/pnpr/v0/artifacts/blob")
        .match_query(Matcher::Any)
        .with_status(400)
        .with_body("x".repeat(super::ERROR_BODY_LIMIT * 4))
        .create_async()
        .await;

    let error = stage_archives(
        &ThrottledClient::default(),
        &registry,
        None,
        &[archive(MISSING, b"missing")],
    )
    .await
    .unwrap_err();
    let StagingError::Failed { reason, .. } = error else {
        panic!("expected a staging failure, got {error:?}");
    };
    assert!(reason.ends_with('…'), "{reason}");
    assert!(reason.len() < super::ERROR_BODY_LIMIT + 64, "{} bytes", reason.len());
}
