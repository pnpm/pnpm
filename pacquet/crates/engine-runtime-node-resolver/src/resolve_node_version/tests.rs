use pretty_assertions::assert_eq;

use pacquet_network::ThrottledClient;

use super::{NodeVersion, filter_versions, resolve_node_version, resolve_node_versions};

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
