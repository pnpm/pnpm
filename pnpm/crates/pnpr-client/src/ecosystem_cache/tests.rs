use crate::{CARGO_ECOSYSTEM, PYPI_ECOSYSTEM, PnprClient, server_resolves};

#[tokio::test]
async fn caches_supported_and_unsupported_ecosystems_across_clients() {
    let mut server = mockito::Server::new_async().await;
    let handshake = server
        .mock("GET", "/cached-capabilities/-/pnpr")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"pnpr":{"versions":[0],"ecosystems":["pypi"]}}"#)
        .expect(2)
        .create_async()
        .await;
    let url = format!("{}/cached-capabilities", server.url());
    let first = PnprClient::new(&url);
    let second = PnprClient::new(&url);

    let first_python = server_resolves(&first, &url, PYPI_ECOSYSTEM);
    let second_python = server_resolves(&second, &url, PYPI_ECOSYSTEM);
    let (first_python, second_python) = tokio::join!(first_python, second_python);
    assert!(dbg!(first_python.unwrap()));
    assert!(dbg!(second_python.unwrap()));
    assert!(!dbg!(server_resolves(&first, &url, CARGO_ECOSYSTEM).await.unwrap()));
    assert!(!dbg!(server_resolves(&second, &url, CARGO_ECOSYSTEM).await.unwrap()));
    handshake.assert_async().await;
}

#[tokio::test]
async fn caches_handshake_failures_across_clients() {
    let mut server = mockito::Server::new_async().await;
    let handshake = server
        .mock("GET", "/cached-failure/-/pnpr")
        .with_status(503)
        .expect(1)
        .create_async()
        .await;
    let url = format!("{}/cached-failure", server.url());
    let first = PnprClient::new(&url);
    let second = PnprClient::new(&url);

    let first_result = server_resolves(&first, &url, PYPI_ECOSYSTEM);
    let second_result = server_resolves(&second, &url, PYPI_ECOSYSTEM);
    let (first_error, second_error) = tokio::join!(first_result, second_result);
    let first_error = first_error.unwrap_err();
    let second_error = second_error.unwrap_err();
    assert_eq!(first_error.to_string(), "ask the pnpr server whether it resolves pypi");
    assert_eq!(format!("{first_error:?}"), format!("{second_error:?}"));
    handshake.assert_async().await;
}
