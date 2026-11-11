use super::{CdnInvalidation, CloudflareCdnConfig, purge, purge_tags, sha256_hex};

#[tokio::test]
async fn purge_requires_a_successful_api_result() {
    let mut server = mockito::Server::new_async().await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    for (status, body, expected_error) in [
        (200, r#"{"success":true}"#, None),
        (200, r#"{"success":false}"#, Some("Cloudflare rejected the purge")),
        (429, r#"{"success":true}"#, Some("Cloudflare returned HTTP 429 Too Many Requests")),
        (200, "invalid", Some("invalid Cloudflare response")),
    ] {
        let mock = server
            .mock("POST", "/purge")
            .match_header("authorization", "Bearer test-token")
            .match_body(mockito::Matcher::Json(serde_json::json!({"tags": ["example"]})))
            .with_status(status)
            .with_body(body)
            .create_async()
            .await;
        let result = purge(
            &client,
            &format!("{}/purge", server.url()),
            "test-token",
            vec!["example".into()],
        )
        .await;
        assert_eq!(
            result
                .as_ref()
                .err()
                .map(String::as_str),
            expected_error,
        );
        mock.assert_async().await;
    }
}

#[test]
fn package_tags_match_the_worker_and_policy_purges_cover_every_package() {
    let config = CloudflareCdnConfig {
        zone_id: "0".repeat(32),
        api_token: "test-token".into(),
        cache_tag: "example".into(),
    };
    let mut events = vec![CdnInvalidation { package: "widget".into(), generation: 1 }];
    assert_eq!(purge_tags(&config, &events), vec![format!("example-{}", sha256_hex(b"widget"))]);
    events.push(CdnInvalidation { package: "*".into(), generation: 2 });
    assert_eq!(purge_tags(&config, &events), ["example"]);
}

#[tokio::test]
async fn cdn_public_tarballs_are_cacheable_and_private_responses_stay_private() {
    use crate::server::{AppState, caller_scoped, state::AppInner, tarball_response};
    use axum::{body::Body, http::header};
    use pnpr_config::Config;
    use pnpr_fixtures::MockConfigPresets;
    use pnpr_registry::Ecosystem;
    use std::sync::Arc;

    let temp = tempfile::TempDir::new().unwrap();
    let mut config =
        Config::static_serve("127.0.0.1:0".parse().unwrap(), temp.path().to_path_buf());
    config.http.cloudflare_cdn = Some(CloudflareCdnConfig {
        zone_id: "0".repeat(32),
        api_token: "test-token".into(),
        cache_tag: "example".into(),
    });
    let state = AppState {
        inner: Arc::new(AppInner::new(config, pnpr_auth::AuthState::in_memory(), None).unwrap()),
    };
    let response = caller_scoped(
        &state,
        Ecosystem::Npm,
        None,
        Some("widget"),
        tarball_response(Body::from("archive"), Some(7)),
    );
    assert_eq!(response.headers()[header::CACHE_CONTROL], "public, max-age=300");
    assert_eq!(response.headers()[header::CONTENT_LENGTH], "7");
    let private = crate::server::private_no_cache(tarball_response(Body::empty(), None));
    assert_eq!(
        super::public_response(private).headers()[header::CACHE_CONTROL],
        "private, no-store",
    );
}
