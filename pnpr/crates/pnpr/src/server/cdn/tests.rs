use super::{CdnInvalidation, CloudflareCdnConfig, purge, purge_tags, sha256_hex};

#[tokio::test]
async fn purge_requires_a_successful_api_result() {
    let mut server = mockito::Server::new_async().await;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    for (status, body, succeeds) in [
        (200, r#"{"success":true}"#, true),
        (200, r#"{"success":false}"#, false),
        (429, r#"{"success":true}"#, false),
        (200, "invalid", false),
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
        assert_eq!(result.is_ok(), succeeds);
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
