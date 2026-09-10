use super::{
    AccessList, AuthState, Body, CHALLENGE, Request, ServiceExt, StatusCode, TempDir, Value, app,
    app_allowing_deletes, basic, body_bytes, check_protocol_surface, digest_of, get, header,
    image_manifest, json, oci_config, push_image, router_with_auth, sha256_hex, token,
};

#[tokio::test]
async fn the_version_check_answers_at_the_host_root() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    let request =
        Request::get("/v2/").header(header::AUTHORIZATION, &auth).body(Body::empty()).unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("docker-distribution-api-version").unwrap(), "registry/2.0");
}

#[tokio::test]
async fn the_version_check_challenges_an_anonymous_caller_even_where_reads_are_open() {
    let tmp = TempDir::new().unwrap();
    // Reads are `$all` here, and the challenge still has to come: a client
    // settles its authentication scheme on this one response, so a 200 would
    // leave it with no way to authenticate a later push.
    let response = get(&app(&tmp), "/v2/").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers().get(header::WWW_AUTHENTICATE).unwrap(), CHALLENGE);
}

#[tokio::test]
async fn a_challenged_ping_does_not_stop_an_anonymous_pull() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;

    assert_eq!(get(&app, "/v2/").await.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(get(&app, "/v2/acme/app/manifests/1.0").await.status(), StatusCode::OK);
}

#[tokio::test]
async fn an_image_pushed_in_one_request_each_pulls_back() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    let manifest_digest = push_image(&app, &auth, "acme/app", "1.0").await;

    let response = get(&app, "/v2/acme/app/manifests/1.0").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("docker-content-digest").unwrap(), &manifest_digest);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/vnd.oci.image.manifest.v1+json",
    );
    assert_eq!(body_bytes(response.into_body()).await, image_manifest("config", &["layer"]));

    let response = get(&app, &format!("/v2/acme/app/manifests/{manifest_digest}")).await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = get(&app, &format!("/v2/acme/app/blobs/{}", digest_of(b"layer"))).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_bytes(response.into_body()).await, b"layer".as_slice());
}

#[tokio::test]
async fn a_head_request_carries_the_headers_without_the_body() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;

    for path in
        ["/v2/acme/app/manifests/1.0", &format!("/v2/acme/app/blobs/{}", digest_of(b"layer"))]
    {
        let request = Request::head(path).body(Body::empty()).unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert!(response.headers().contains_key("docker-content-digest"), "{path}");
        assert!(response.headers().contains_key(header::CONTENT_LENGTH), "{path}");
        assert!(body_bytes(response.into_body()).await.is_empty(), "{path}");
    }
}

#[tokio::test]
async fn bytes_that_do_not_match_the_promised_digest_are_refused() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);

    let lie = digest_of(b"something else");
    let request = Request::post(format!("/v2/acme/app/blobs/uploads/?digest={lie}"))
        .header(header::AUTHORIZATION, &auth)
        .body(Body::from("real bytes"))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let response = get(&app, &format!("/v2/acme/app/blobs/{lie}")).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_anonymous_push_is_refused_with_a_challenge() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);

    let request = Request::post("/v2/acme/app/blobs/uploads/").body(Body::empty()).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers().get(header::WWW_AUTHENTICATE).unwrap(), CHALLENGE);
}

#[tokio::test]
async fn a_repository_name_the_grammar_refuses_never_reaches_storage() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);

    for name in ["acme/../etc", "acme/.hidden", "acme/-leading"] {
        let response = get(&app, &format!("/v2/{name}/tags/list")).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{name}");
        let payload: Value =
            serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
        assert_eq!(payload["errors"][0]["code"], "NAME_INVALID", "{name}");
    }
}

#[tokio::test]
async fn a_repository_name_is_case_folded_rather_than_refused() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;

    // Two spellings are one repository, not two directories that a
    // case-insensitive filesystem would then collide.
    let response = get(&app, "/v2/ACME/App/tags/list").await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["name"], "acme/app");
    assert_eq!(payload["tags"], json!(["1.0"]));
}

#[tokio::test]
async fn the_named_form_serves_the_same_repository() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    push_image(&app, &auth, "acme/app", "1.0").await;

    let response = get(&app, "/oci/~images/v2/acme/app/tags/list").await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    assert_eq!(payload["tags"], json!(["1.0"]));
}

#[tokio::test]
async fn an_index_over_pushed_children_publishes() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);
    // The child is pushed first, the way a multi-architecture push does it,
    // so the index that names it resolves.
    let child = push_image(&app, &auth, "acme/app", "child").await;

    let index = serde_json::to_vec(&json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "manifests": [{ "digest": child, "size": image_manifest("config", &["layer"]).len() }],
    }))
    .unwrap();
    let request = Request::put("/v2/acme/app/manifests/multi")
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_TYPE, "application/vnd.oci.image.index.v1+json")
        .body(Body::from(index))
        .unwrap();
    assert_eq!(app.clone().oneshot(request).await.unwrap().status(), StatusCode::CREATED);
    assert_eq!(get(&app, "/v2/acme/app/manifests/multi").await.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_range_that_contradicts_itself_or_the_body_is_refused() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);

    let request = Request::post("/v2/acme/app/blobs/uploads/")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let location = response.headers().get(header::LOCATION).unwrap().to_str().unwrap().to_string();

    // An end before the start, and a body that is not the length the range
    // declares. Both would otherwise leave the upload somewhere neither side
    // named.
    for (range, body) in [("5-2", "hello"), ("0-99", "hello")] {
        let request = Request::patch(&location)
            .header(header::AUTHORIZATION, &auth)
            .header(header::CONTENT_RANGE, range)
            .header(header::CONTENT_LENGTH, body.len())
            .body(Body::from(body))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{range}");
    }

    let request =
        Request::get(&location).header(header::AUTHORIZATION, &auth).body(Body::empty()).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.headers().get(header::RANGE).unwrap(), "0-0");
}

#[tokio::test]
async fn a_path_that_names_no_endpoint_is_not_found() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);

    // A slash inside a reference decodes into another path segment, which
    // leaves a tail naming no endpoint rather than a manifest with an odd
    // name. Nothing is served there, which is not the same as a method being
    // refused on something that is.
    for path in ["/v2/acme/app/manifests/has%2Fslash", "/v2/acme/app/nonsense/1.0", "/v2/acme"] {
        let response = get(&app, path).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn a_chunk_that_ends_early_leaves_the_prefix_to_resume_from() {
    let tmp = TempDir::new().unwrap();
    let app = app(&tmp);
    let auth = basic(&token(&app).await);

    let request = Request::post("/v2/acme/app/blobs/uploads/")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let location = response.headers().get(header::LOCATION).unwrap().to_str().unwrap().to_string();

    let torn = futures_util::stream::iter([
        Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"hello")),
        Err(std::io::Error::other("the connection went away")),
    ]);
    let request = Request::patch(&location)
        .header(header::AUTHORIZATION, &auth)
        .body(Body::from_stream(torn))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // The bytes that did arrive are an ordered prefix of the blob, so the
    // upload keeps them and says so. Dropping them would cost the client the
    // whole layer for one lost connection.
    let request =
        Request::get(&location).header(header::AUTHORIZATION, &auth).body(Body::empty()).unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.headers().get(header::RANGE).unwrap(), "0-4");

    let request = Request::patch(&location)
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_RANGE, "5-10")
        .body(Body::from(" world"))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);

    let digest = digest_of(b"hello world");
    let request = Request::put(format!("{location}?digest={digest}"))
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn protocol_surface_on_filesystem() {
    let tmp = TempDir::new().unwrap();
    check_protocol_surface(app_allowing_deletes(&tmp)).await;
}

#[tokio::test]
async fn protocol_surface_on_object_store() {
    let tmp = TempDir::new().unwrap();
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    config.hosted_store = pnpr::HostedStoreConfig::ObjectStore {
        store: std::sync::Arc::new(object_store::memory::InMemory::new()),
        prefix: "protocol/".into(),
    };
    let hosted = config.hosted.get_mut("images").unwrap();
    hosted.rules = std::mem::take(&mut hosted.rules)
        .with_default_unpublish(AccessList::from_tokens(["$authenticated"]));
    check_protocol_surface(router_with_auth(config, AuthState::in_memory())).await;
}

#[tokio::test]
async fn scoped_bearer_credentials_cannot_write_escape_repository_or_survive_revocation() {
    let tmp = TempDir::new().unwrap();
    let mut config = oci_config(tmp.path().to_path_buf(), "$all");
    config.oci.bearer_auth = true;
    let auth_state = AuthState::in_memory();
    let app = router_with_auth(config, auth_state.clone());
    let parent = token(&app).await;
    let auth = basic(&parent);
    push_image(&app, &auth, "acme/app", "latest").await;
    push_image(&app, &auth, "acme/other", "latest").await;
    let challenge = get(&app, "/v2/").await;
    assert!(challenge.headers()[header::WWW_AUTHENTICATE].to_str().unwrap().starts_with("Bearer "));
    let response = app
        .clone()
        .oneshot(
            Request::get("/v2/token?service=pnpr&scope=repository:acme/app:pull")
                .header(header::AUTHORIZATION, &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(&body_bytes(response.into_body()).await).unwrap();
    let scoped = format!("Bearer {}", payload["token"].as_str().unwrap());
    for (method, path, expected) in [
        ("GET", "/v2/acme/app/manifests/latest", StatusCode::OK),
        ("GET", "/v2/acme/other/manifests/latest", StatusCode::UNAUTHORIZED),
        ("POST", "/v2/acme/app/blobs/uploads/", StatusCode::UNAUTHORIZED),
        ("GET", "/-/npm/v1/tokens", StatusCode::UNAUTHORIZED),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(header::AUTHORIZATION, &scoped)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "{method} {path}");
    }
    auth_state.tokens.revoke_by_key(&sha256_hex(parent.as_bytes())).await.unwrap();
    let response = app
        .oneshot(
            Request::get("/v2/acme/app/manifests/latest")
                .header(header::AUTHORIZATION, scoped)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
