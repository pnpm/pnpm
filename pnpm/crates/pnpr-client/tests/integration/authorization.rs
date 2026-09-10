use super::{Ipv4Addr, PnprClient, TcpListener, capture_one_request, deps, options};

/// The request must identify the caller to pnpr (`Authorization`) but
/// must never carry the client's own upstream registry credentials in the
/// body — pnpr selects upstream auth from its route policy. A raw TCP
/// listener captures the wire bytes and asserts both invariants; the
/// canned 500 just short-circuits the client after the capture.
#[tokio::test]
async fn sends_the_identity_header_but_no_upstream_credentials() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.expect("bind capture");
    let addr = listener.local_addr().expect("capture addr");
    let capture = tokio::spawn(capture_one_request(listener));

    let client = PnprClient::new(format!("http://{addr}/"));
    let opts =
        options("https://npm.acme.test/", "Bearer pnpr-token", deps([("@acme/foo", "1.0.0")]));
    let result = client.resolve(opts).await;
    assert!(result.is_err(), "the canned 500 should surface as an error");

    let request = capture.await.expect("capture task");
    assert!(
        request.to_lowercase().contains("authorization: bearer pnpr-token"),
        "the identity header must be sent, got:\n{request}",
    );
    assert!(
        !request.contains("authHeaders"),
        "the request body must not carry upstream credentials, got:\n{request}",
    );
    for field in ["autoInstallPeers", "dedupePeers", "excludeLinksFromLockfile"] {
        assert!(
            request.contains(&format!(r#""{field}":null"#)),
            "an unsent {field} must stay unset rather than turn into `false`, got:\n{request}",
        );
    }
}
