use pretty_assertions::assert_eq;

use pnpm_network::{AuthHeaders, ThrottledClient, nerf_dart};

use super::{
    NodeVersion, filter_versions, resolve_node_version, resolve_node_version_with_auth,
    resolve_node_versions,
};

fn make_versions() -> Vec<NodeVersion> {
    vec![
        NodeVersion { version: "22.0.0".to_string(), lts: None },
        NodeVersion { version: "20.10.0".to_string(), lts: Some("Iron".to_string()) },
        NodeVersion { version: "20.5.0".to_string(), lts: None },
        NodeVersion { version: "18.18.0".to_string(), lts: Some("Hydrogen".to_string()) },
        NodeVersion { version: "16.20.0".to_string(), lts: Some("Gallium".to_string()) },
    ]
}

#[test]
fn lts_selector_picks_every_lts_release() {
    let (picked, range) = filter_versions(&make_versions(), "lts");
    assert_eq!(picked, vec!["20.10.0", "18.18.0", "16.20.0"]);
    assert_eq!(range, "*");
}

#[test]
fn lts_codename_is_case_insensitive() {
    let (picked, range) = filter_versions(&make_versions(), "iron");
    assert_eq!(picked, vec!["20.10.0"]);
    assert_eq!(range, "*");
}

#[test]
fn semver_range_passes_through() {
    let (picked, range) = filter_versions(&make_versions(), "^20");
    assert_eq!(picked.len(), 5);
    assert_eq!(range, "^20");
}

#[tokio::test]
async fn empty_selector_picks_latest_version() {
    let mut server = mockito::Server::new_async().await;
    let _index = server
        .mock("GET", "/index.json")
        .with_status(200)
        .with_body(
            r#"[
                { "version": "v22.1.0", "lts": false },
                { "version": "v20.10.0", "lts": "Iron" }
            ]"#,
        )
        .expect(4)
        .create_async()
        .await;
    let base_url = format!("{}/", server.url());
    let http_client = ThrottledClient::new_for_installs();

    let picked = resolve_node_version(&http_client, "", Some(&base_url)).await.unwrap();
    assert_eq!(picked, Some("22.1.0".to_string()));

    let picked = resolve_node_versions(&http_client, Some(""), Some(&base_url)).await.unwrap();
    assert_eq!(picked, vec!["22.1.0"]);

    let picked = resolve_node_version(&http_client, "  ", Some(&base_url)).await.unwrap();
    assert_eq!(picked, Some("22.1.0".to_string()));

    let picked = resolve_node_versions(&http_client, Some("  "), Some(&base_url)).await.unwrap();
    assert_eq!(picked, vec!["22.1.0"]);
}

#[tokio::test]
async fn authenticated_version_resolve_uses_matching_mirror_credentials() {
    let mut server = mockito::Server::new_async().await;
    let index = server
        .mock("GET", "/index.json")
        .match_header("authorization", "Bearer mirror-token")
        .with_status(200)
        .with_body(r#"[{ "version": "v22.1.0", "lts": false }]"#)
        .create_async()
        .await;
    let base_url = format!("{}/", server.url());
    let auth_headers =
        AuthHeaders::from_creds_map([(nerf_dart(&base_url), "Bearer mirror-token".to_string())]);
    let http_client = ThrottledClient::new_for_installs();

    let picked =
        resolve_node_version_with_auth(&http_client, &auth_headers, "latest", Some(&base_url))
            .await
            .unwrap();

    assert_eq!(picked, Some("22.1.0".to_string()));
    index.assert_async().await;
}

#[tokio::test]
async fn authenticated_version_resolve_omits_unmatched_credentials() {
    let mut server = mockito::Server::new_async().await;
    let index = server
        .mock("GET", "/index.json")
        .match_header("authorization", mockito::Matcher::Missing)
        .with_status(200)
        .with_body(r#"[{ "version": "v22.1.0", "lts": false }]"#)
        .create_async()
        .await;
    let base_url = format!("{}/", server.url());
    let auth_headers = AuthHeaders::from_creds_map([(
        "//other.example/".to_string(),
        "Bearer other-token".to_string(),
    )]);
    let http_client = ThrottledClient::new_for_installs();

    let picked =
        resolve_node_version_with_auth(&http_client, &auth_headers, "latest", Some(&base_url))
            .await
            .unwrap();

    assert_eq!(picked, Some("22.1.0".to_string()));
    index.assert_async().await;
}

#[tokio::test]
async fn authenticated_version_resolve_reselects_credentials_after_redirects() {
    let mut server = mockito::Server::new_async().await;
    let redirect = server
        .mock("GET", "/private/index.json")
        .match_header("authorization", "Bearer mirror-token")
        .with_status(302)
        .with_header("location", "/public/index.json")
        .create_async()
        .await;
    let index = server
        .mock("GET", "/public/index.json")
        .match_header("authorization", mockito::Matcher::Missing)
        .with_status(200)
        .with_body(r#"[{ "version": "v22.1.0", "lts": false }]"#)
        .create_async()
        .await;
    let base_url = format!("{}/private/", server.url());
    let auth_headers =
        AuthHeaders::from_creds_map([(nerf_dart(&base_url), "Bearer mirror-token".to_string())]);
    let http_client = ThrottledClient::new_for_installs();

    let picked =
        resolve_node_version_with_auth(&http_client, &auth_headers, "latest", Some(&base_url))
            .await
            .unwrap();

    assert_eq!(picked, Some("22.1.0".to_string()));
    redirect.assert_async().await;
    index.assert_async().await;
}
