use super::{ArtifactBlobRequest, BASE64, OwnerScope, PnprClient, PnprClientError, Sha512};
use base64::Engine as _;
use sha2::Digest as _;

#[tokio::test]
async fn artifact_blob_download_rejects_bytes_that_do_not_match_the_integrity() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/-/pnpr/v0/artifacts/blob")
        .with_status(200)
        .with_body("poisoned blob")
        .create_async()
        .await;
    let integrity = format!("sha512-{}", BASE64.encode(Sha512::digest(b"expected blob")));

    let error = PnprClient::new(server.url())
        .download_artifact_blob(
            &ArtifactBlobRequest { owner: OwnerScope::organization("acme"), integrity },
            None,
        )
        .await
        .expect_err("a corrupt artifact blob must be rejected");

    assert!(matches!(error, PnprClientError::Protocol(_)), "got: {error}");
    mock.assert_async().await;
}
