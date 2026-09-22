use super::{
    CRATES_IO_SPARSE_INDEX,
    Config,
};
use crate::cargo_deps::lockfile::resolve_via_pnpr;

fn handshake_body(ecosystems: &[&str]) -> String {
    serde_json::json!({
        "pnpr": { "versions": [0], "artifacts": [], "fixLockfile": [0], "ecosystems": ecosystems },
    })
    .to_string()
}

fn config_for_pnpr(server: &str) -> Config {
    let mut config = Config::new();
    config.pnpr_server = Some(server.to_string());
    config
}

#[tokio::test]
async fn cargo_resolution_is_offloaded_to_the_pnpr_server() {
    const METADATA: &str = r#"{
      "packages": [{
        "id": "path+file:///home/dev/private-workspace#app@0.1.0",
        "name": "app",
        "version": "0.1.0",
        "manifest_path": "/home/dev/private-workspace/Cargo.toml",
        "dependencies": []
      }],
      "workspace_members": ["path+file:///home/dev/private-workspace#app@0.1.0"]
    }"#;
    let mut server = mockito::Server::new_async().await;
    let handshake = server
        .mock("GET", "/-/pnpr")
        .with_body(handshake_body(&["npm", "cargo"]))
        .create_async()
        .await;
    let sent_metadata = pnpm_cargo_resolver::resolve_inputs(METADATA).unwrap();
    assert!(!sent_metadata.contains("private-workspace"), "{sent_metadata}");
    let resolve = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .match_body(mockito::Matcher::PartialJson(serde_json::json!({
            "ecosystem": "cargo",
            "metadata": sent_metadata,
            "registry": CRATES_IO_SPARSE_INDEX,
        })))
        .with_header("content-type", "application/x-ndjson")
        .with_body("{\"type\":\"done\",\"lockfile\":\"version = 4\\n\"}\n")
        .expect(1)
        .create_async()
        .await;

    let lockfile = resolve_via_pnpr(&config_for_pnpr(&server.url()), METADATA)
        .await
        .unwrap();

    assert_eq!(lockfile.as_deref(), Some("version = 4\n"));
    handshake.assert_async().await;
    resolve.assert_async().await;
}

#[tokio::test]
async fn configured_cargo_registry_is_sent_to_the_pnpr_server() {
    let mut server = mockito::Server::new_async().await;
    let handshake = server
        .mock("GET", "/-/pnpr")
        .with_body(handshake_body(&["cargo"]))
        .create_async()
        .await;
    let resolve = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .match_body(mockito::Matcher::PartialJson(serde_json::json!({
            "ecosystem": "cargo",
            "registry": "https://registry.example.test/index/",
        })))
        .with_header("content-type", "application/x-ndjson")
        .with_body("{\"type\":\"done\",\"lockfile\":\"version = 4\\n\"}\n")
        .create_async()
        .await;
    let mut config = config_for_pnpr(&server.url());
    config.indexes_by_ecosystem.insert(
        pnpm_config::Ecosystem::Cargo,
        vec!["https://registry.example.test/index/".to_string().into()],
    );

    let lockfile = resolve_via_pnpr(&config, r#"{"packages":[],"workspace_members":[]}"#)
        .await
        .unwrap();

    assert_eq!(lockfile.as_deref(), Some("version = 4\n"));
    handshake.assert_async().await;
    resolve.assert_async().await;
}

#[tokio::test]
async fn a_server_without_cargo_support_leaves_resolution_local() {
    let mut server = mockito::Server::new_async().await;
    let handshake = server
        .mock("GET", "/-/pnpr")
        .with_body(handshake_body(&["npm"]))
        .create_async()
        .await;
    let resolve = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .expect(0)
        .create_async()
        .await;

    let lockfile = resolve_via_pnpr(&config_for_pnpr(&server.url()), "{}").await.unwrap();

    assert_eq!(lockfile, None);
    handshake.assert_async().await;
    resolve.assert_async().await;
}

#[tokio::test]
async fn an_unterminated_pnpr_response_does_not_grow_without_bound() {
    let mut server = mockito::Server::new_async().await;
    let handshake = server
        .mock("GET", "/-/pnpr")
        .with_body(handshake_body(&["npm", "cargo"]))
        .create_async()
        .await;
    let resolve = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .with_header("content-type", "application/x-ndjson")
        .with_body("x".repeat(33 * 1024 * 1024))
        .create_async()
        .await;

    let metadata = r#"{"packages":[],"workspace_members":[]}"#;
    let error = resolve_via_pnpr(&config_for_pnpr(&server.url()), metadata)
        .await
        .unwrap_err();

    assert!(
        error
            .chain()
            .any(|cause| cause.to_string().contains("exceeds the")),
        "the oversized body is refused by its size, not by parsing: {error:?}",
    );
    handshake.assert_async().await;
    resolve.assert_async().await;
}

#[tokio::test]
async fn a_second_terminal_frame_fails_the_resolve() {
    let mut server = mockito::Server::new_async().await;
    let handshake = server
        .mock("GET", "/-/pnpr")
        .with_body(handshake_body(&["npm", "cargo"]))
        .create_async()
        .await;
    let resolve = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .with_header("content-type", "application/x-ndjson")
        .with_body(concat!(
            "{\"type\":\"done\",\"lockfile\":\"version = 4\\n\"}\n",
            "{\"type\":\"error\",\"message\":\"resolution failed\"}\n",
        ))
        .create_async()
        .await;

    let metadata = r#"{"packages":[],"workspace_members":[]}"#;
    let error = resolve_via_pnpr(&config_for_pnpr(&server.url()), metadata)
        .await
        .unwrap_err();

    assert!(
        error
            .chain()
            .any(|cause| cause.to_string().contains("more than one terminal frame")),
        "a response that also reports a failure is not a lockfile to write: {error:?}",
    );
    handshake.assert_async().await;
    resolve.assert_async().await;
}

#[tokio::test]
async fn concurrent_roots_share_one_handshake() {
    const METADATA: &str = r#"{"packages":[],"workspace_members":[]}"#;
    let mut server = mockito::Server::new_async().await;
    let handshake = server
        .mock("GET", "/-/pnpr")
        .with_body(handshake_body(&["npm", "cargo"]))
        .expect(1)
        .create_async()
        .await;
    let resolve = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .with_header("content-type", "application/x-ndjson")
        .with_body("{\"type\":\"done\",\"lockfile\":\"version = 4\\n\"}\n")
        .expect(4)
        .create_async()
        .await;
    let config = config_for_pnpr(&server.url());

    let roots = (0..4).map(|_| resolve_via_pnpr(&config, METADATA));
    for resolved in futures_util::future::join_all(roots).await {
        resolved.unwrap().expect("the server resolves Cargo");
    }

    handshake.assert_async().await;
    resolve.assert_async().await;
}

#[tokio::test]
async fn concurrent_roots_share_one_failed_handshake() {
    const METADATA: &str = r#"{"packages":[],"workspace_members":[]}"#;
    let mut server = mockito::Server::new_async().await;
    let handshake = server
        .mock("GET", "/-/pnpr")
        .with_status(500)
        .expect(1)
        .create_async()
        .await;
    let config = config_for_pnpr(&server.url());

    let roots = (0..4).map(|_| resolve_via_pnpr(&config, METADATA));
    for resolved in futures_util::future::join_all(roots).await {
        let error = resolved.unwrap_err();
        assert!(
            error.to_string().contains("whether it resolves cargo"),
            "every root reports the same refusal: {error:?}",
        );
    }

    handshake.assert_async().await;
}

#[tokio::test]
async fn the_handshake_is_asked_once_per_server() {
    const METADATA: &str = r#"{"packages":[],"workspace_members":[]}"#;
    let mut server = mockito::Server::new_async().await;
    let handshake = server
        .mock("GET", "/-/pnpr")
        .with_body(handshake_body(&["npm", "cargo"]))
        .expect(1)
        .create_async()
        .await;
    let resolve = server
        .mock("POST", "/-/pnpr/v0/resolve")
        .with_header("content-type", "application/x-ndjson")
        .with_body("{\"type\":\"done\",\"lockfile\":\"version = 4\\n\"}\n")
        .expect(2)
        .create_async()
        .await;
    let config = config_for_pnpr(&server.url());

    for _ in 0..2 {
        resolve_via_pnpr(&config, METADATA).await.unwrap().expect("the server resolves Cargo");
    }

    handshake.assert_async().await;
    resolve.assert_async().await;
}

#[tokio::test]
async fn an_offline_install_does_not_reach_the_pnpr_server() {
    let mut server = mockito::Server::new_async().await;
    let handshake = server
        .mock("GET", "/-/pnpr")
        .expect(0)
        .create_async()
        .await;
    let mut config = config_for_pnpr(&server.url());
    config.offline = true;

    let lockfile = resolve_via_pnpr(&config, "{}").await.unwrap();

    assert_eq!(lockfile, None);
    handshake.assert_async().await;
}
