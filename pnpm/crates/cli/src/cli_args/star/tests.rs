use super::star_action;
use pnpm_config::Config;

#[tokio::test]
async fn star_action_honors_registry_override_for_scoped_package() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("PUT", "/-/user/v1/star")
        .with_status(200)
        .expect(1)
        .create_async()
        .await;

    let server_url = server.url();
    let mut config = Config::default();
    config.registries_by_scope.insert("@scope".to_string(), "https://other.registry/".to_string());
    let mut auth_headers = (*config.auth_headers).clone();
    auth_headers.insert_url_header(&server_url, "Bearer token".to_string());
    config.auth_headers = std::sync::Arc::new(auth_headers);

    let result = star_action(&config, Some(&server_url), "@scope/pkg", true).await;
    assert!(result.is_ok());
    mock.assert_async().await;
}

#[tokio::test]
async fn unstar_action_honors_registry_override_for_scoped_package() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("DELETE", "/-/user/v1/star")
        .with_status(200)
        .expect(1)
        .create_async()
        .await;

    let server_url = server.url();
    let mut config = Config::default();
    config.registries_by_scope.insert("@scope".to_string(), "https://other.registry/".to_string());
    let mut auth_headers = (*config.auth_headers).clone();
    auth_headers.insert_url_header(&server_url, "Bearer token".to_string());
    config.auth_headers = std::sync::Arc::new(auth_headers);

    let result = star_action(&config, Some(&server_url), "@scope/pkg", false).await;
    assert!(result.is_ok());
    mock.assert_async().await;
}
