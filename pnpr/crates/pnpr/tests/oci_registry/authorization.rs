use super::{
    AccessList, AuthState, Body, Request, ServiceExt, StatusCode, TempDir, Value, basic,
    body_bytes, get, header, oci_config, push_blob, push_image, router_with_auth, token,
};

#[tokio::test]
async fn reads_of_a_private_repository_are_kept_out_of_shared_caches() {
    let tmp = TempDir::new().unwrap();
    let config = oci_config(tmp.path().to_path_buf(), "$authenticated");
    let app = router_with_auth(config, AuthState::in_memory());
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;

    for path in ["/v2/acme/app/manifests/1.0", "/v2/acme/app/tags/list", "/v2/_catalog"] {
        let request =
            Request::get(path).header(header::AUTHORIZATION, &auth).body(Body::empty()).unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(
            response.headers().get(header::CACHE_CONTROL).map(|value| value.to_str().unwrap()),
            Some("private, no-store"),
            "{path} must not be storable by a shared cache",
        );
    }
}

#[tokio::test]
async fn deletion_requires_read_access_even_with_a_permissive_unpublish_rule() {
    let tmp = TempDir::new().unwrap();
    let mut config = oci_config(tmp.path().to_path_buf(), "alice");
    let hosted = config.hosted.get_mut("images").unwrap();
    hosted.rules =
        std::mem::take(&mut hosted.rules).with_default_unpublish(AccessList::from_tokens(["$all"]));
    let app = router_with_auth(config, AuthState::in_memory());
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "latest").await;
    let digest = push_blob(&app, &auth, "acme/app", b"orphan").await;
    for path in
        ["/v2/acme/app/manifests/latest".to_string(), format!("/v2/acme/app/blobs/{digest}")]
    {
        let response =
            app.clone().oneshot(Request::delete(path).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    let response = app
        .oneshot(
            Request::get(format!("/v2/acme/app/blobs/{digest}"))
                .header(header::AUTHORIZATION, &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn token_scopes_ignore_unknown_resources_and_count_distinct_repositories() {
    let tmp = TempDir::new().unwrap();
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    config.oci.bearer_auth = true;
    let app = router_with_auth(config, AuthState::in_memory());
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    for index in 0..32 {
        query.append_pair("scope", &format!("repository:other/app{index}:pull"));
    }
    query.append_pair("scope", "repository(plugin):other/app0:pull");
    query.append_pair("scope", "registry:catalog:*");
    query.append_pair("scope", "repository:host:5000/app:pull");
    let query = query.finish();
    let response = get(&app, &format!("/v2/token?{query}")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::get("/v2/_catalog")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", payload["token"].as_str().unwrap()),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = get(&app, "/v2/token?scope=repository(plugin):acme/app:pull").await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::get("/v2/acme/app/manifests/latest")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", payload["token"].as_str().unwrap()),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = get(&app, &format!("/v2/token?{query}&scope=repository:other/extra:pull")).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
